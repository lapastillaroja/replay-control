use super::*;

/// A recent entry enriched with box art URL for the home page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentWithArt {
    #[serde(flatten)]
    pub entry: RecentEntry,
    pub box_art_url: Option<String>,
}

/// Result of a storage refresh operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshResult {
    pub changed: bool,
    pub storage_kind: String,
    pub storage_root: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogLevelConfig {
    pub level: String,
}

#[cfg(feature = "ssr")]
const REPLAY_CONTROL_ENV_FILE: &str = "/etc/default/replay-control";
#[cfg(feature = "ssr")]
const RUST_LOG_INFO: &str = "info";
#[cfg(feature = "ssr")]
const RUST_LOG_DEBUG: &str =
    "info,replay_control_app=debug,replay_control_core=debug,replay_control_core_server=debug";

#[server(prefix = "/sfn")]
pub async fn get_info() -> Result<SystemInfo, ServerFnError> {
    #[cfg(feature = "ssr")]
    let fn_start = std::time::Instant::now();
    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();
    let summaries = state
        .cache
        .cached_systems(&storage, &state.library_pool)
        .await;
    let total_favorites = state.cache.get_favorites_count(&storage).await;

    let disk =
        storage
            .disk_usage()
            .await
            .unwrap_or(replay_control_core_server::storage::DiskUsage {
                total_bytes: 0,
                available_bytes: 0,
                used_bytes: 0,
            });

    let systems_with_games = summaries.iter().filter(|s| s.game_count > 0).count();
    let total_games: usize = summaries.iter().map(|s| s.game_count).sum();

    let (ethernet_ip, wifi_ip) = get_network_ips().await;

    #[cfg(feature = "ssr")]
    tracing::debug!(
        elapsed_ms = fn_start.elapsed().as_millis(),
        "get_info complete"
    );
    Ok(SystemInfo {
        storage_kind: format!("{:?}", storage.kind).to_lowercase(),
        storage_root: storage.root.display().to_string(),
        disk_total_bytes: disk.total_bytes,
        disk_used_bytes: disk.used_bytes,
        disk_available_bytes: disk.available_bytes,
        total_systems: summaries.len(),
        systems_with_games,
        total_games,
        total_favorites,
        ethernet_ip,
        wifi_ip,
    })
}

#[cfg(feature = "ssr")]
async fn get_network_ips() -> (Option<String>, Option<String>) {
    // Single `ip` call covers all interfaces — fewer process spawns than one per prefix.
    let output = tokio::process::Command::new("ip")
        .args(["-4", "-o", "addr", "show"])
        .output()
        .await
        .ok();
    let Some(output) = output else {
        return (None, None);
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let extract_from_output = |prefix: &str| -> Option<String> {
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 && parts[1].starts_with(prefix) {
                // Format: "2: eth0    inet 192.168.1.100/24 ..."
                return parts[3].split('/').next().map(|s| s.to_string());
            }
        }
        None
    };
    let eth = extract_from_output("eth").or_else(|| extract_from_output("enp"));
    let wifi = extract_from_output("wlan").or_else(|| extract_from_output("wlp"));
    (eth, wifi)
}

#[server(prefix = "/sfn")]
pub async fn get_systems() -> Result<Vec<SystemSummary>, ServerFnError> {
    #[cfg(feature = "ssr")]
    let fn_start = std::time::Instant::now();
    let state = expect_context::<crate::api::AppState>();
    let result = state
        .cache
        .cached_systems(&state.storage(), &state.library_pool)
        .await;
    #[cfg(feature = "ssr")]
    tracing::debug!(
        elapsed_ms = fn_start.elapsed().as_millis(),
        "get_systems complete"
    );
    Ok(result)
}

#[server(prefix = "/sfn", endpoint = "/get_recents")]
pub async fn get_recents() -> Result<Vec<RecentWithArt>, ServerFnError> {
    #[cfg(feature = "ssr")]
    let fn_start = std::time::Instant::now();
    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();
    let entries = state
        .cache
        .get_recents(&storage)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    // Collect (system, rom_filename) pairs for a batch DB lookup.
    let keys: Vec<(String, String)> = entries
        .iter()
        .map(|e| (e.game.system.clone(), e.game.rom_filename.clone()))
        .collect();

    // Batch-lookup box_art_url from game_library (most entries will have it).
    let db_entries = state
        .library_pool
        .read(move |conn| LibraryDb::lookup_game_entries(conn, &keys).unwrap_or_default())
        .await
        .unwrap_or_default();
    #[cfg(feature = "ssr")]
    tracing::debug!(
        elapsed_ms = fn_start.elapsed().as_millis(),
        "get_recents db_read complete"
    );

    // Box art comes from the DB `box_art_url` field (set by enrichment pipeline).
    // If NULL, no art is available — show placeholder.
    let mut enriched = Vec::with_capacity(entries.len());
    for entry in entries {
        let box_art_url = db_entries
            .get(&(entry.game.system.clone(), entry.game.rom_filename.clone()))
            .and_then(|e| e.box_art_url.clone());
        enriched.push(RecentWithArt { entry, box_art_url });
    }
    // The homepage only displays 1 hero + 10 scroll = 11 entries.
    // Cap at 15 to avoid serialising the full list (~95 entries, ~39KB).
    enriched.truncate(15);

    #[cfg(feature = "ssr")]
    tracing::debug!(
        elapsed_ms = fn_start.elapsed().as_millis(),
        "get_recents complete"
    );
    Ok(enriched)
}

/// Read system logs.
/// Tries journalctl first; falls back to log files when journald has Storage=none
/// (as on RePlayOS, which disables persistent/volatile journal).
#[server(prefix = "/sfn")]
pub async fn get_system_logs(source: String, lines: usize) -> Result<String, ServerFnError> {
    let lines = lines.min(500);

    // Try journalctl first.
    let journal_text = read_journalctl(&source, lines).await;
    if !journal_text.is_empty() {
        return Ok(journal_text);
    }

    // Fallback: read from log files.
    match source.as_str() {
        "replay" => Ok(read_log_file_tail("/var/log/replay.log", lines).await),
        "replay-control" => Ok(read_log_file_tail("/var/log/replay-control.log", lines).await),
        _ => {
            // "all": combine both log files, interleave by showing replay-control first.
            let rc = read_log_file_tail("/var/log/replay-control.log", lines).await;
            let rp = read_log_file_tail("/var/log/replay.log", lines).await;
            if rc.is_empty() && rp.is_empty() {
                Ok("No logs available.".to_string())
            } else if rc.is_empty() {
                Ok(rp)
            } else if rp.is_empty() {
                Ok(rc)
            } else {
                Ok(format!(
                    "=== Replay Control ===\n{rc}\n\n=== RePlayOS ===\n{rp}"
                ))
            }
        }
    }
}

#[server(prefix = "/sfn")]
pub async fn get_log_level_config() -> Result<LogLevelConfig, ServerFnError> {
    #[cfg(feature = "ssr")]
    {
        let content = tokio::fs::read_to_string(REPLAY_CONTROL_ENV_FILE)
            .await
            .unwrap_or_default();
        Ok(LogLevelConfig {
            level: rust_log_level_from_env_file(&content).to_string(),
        })
    }

    #[cfg(not(feature = "ssr"))]
    {
        Ok(LogLevelConfig {
            level: "info".to_string(),
        })
    }
}

#[server(prefix = "/sfn")]
pub async fn save_log_level_config(level: String) -> Result<(), ServerFnError> {
    #[cfg(feature = "ssr")]
    {
        let rust_log = match level.as_str() {
            "debug" => RUST_LOG_DEBUG,
            "info" => RUST_LOG_INFO,
            _ => return Err(ServerFnError::new("Invalid log level")),
        };

        let content = tokio::fs::read_to_string(REPLAY_CONTROL_ENV_FILE)
            .await
            .unwrap_or_default();
        let updated = set_rust_log_in_env_file(&content, rust_log);
        tokio::fs::write(REPLAY_CONTROL_ENV_FILE, updated)
            .await
            .map_err(|e| ServerFnError::new(e.to_string()))?;
        Ok(())
    }

    #[cfg(not(feature = "ssr"))]
    {
        let _ = level;
        Ok(())
    }
}

#[cfg(feature = "ssr")]
fn rust_log_level_from_env_file(content: &str) -> &'static str {
    let Some(value) = rust_log_value_from_env_file(content) else {
        return "info";
    };
    if value.split(',').any(|part| {
        let part = part.trim();
        part == "debug" || part.ends_with("=debug")
    }) {
        "debug"
    } else {
        "info"
    }
}

#[cfg(feature = "ssr")]
fn rust_log_value_from_env_file(content: &str) -> Option<String> {
    content.lines().find_map(|line| {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            return None;
        }
        let value = trimmed.strip_prefix("RUST_LOG=")?;
        Some(value.trim().trim_matches(['"', '\'']).to_string())
    })
}

#[cfg(feature = "ssr")]
fn set_rust_log_in_env_file(content: &str, rust_log: &str) -> String {
    let mut found = false;
    let mut lines = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with('#') && trimmed.starts_with("RUST_LOG=") {
            lines.push(format!("RUST_LOG={rust_log}"));
            found = true;
        } else {
            lines.push(line.to_string());
        }
    }

    if !found {
        lines.push(format!("RUST_LOG={rust_log}"));
    }

    let mut output = lines.join("\n");
    output.push('\n');
    output
}

#[cfg(feature = "ssr")]
async fn read_journalctl(source: &str, lines: usize) -> String {
    let mut cmd = tokio::process::Command::new("journalctl");
    cmd.args(["--no-pager", "--lines", &lines.to_string(), "--reverse"]);

    match source {
        "replay-control" => {
            cmd.args(["-u", "replay-control"]);
        }
        "replay" => {
            cmd.args(["-u", "replay"]);
        }
        _ => {}
    }

    let output = match cmd.output().await {
        Ok(o) if o.status.success() => o,
        _ => return String::new(),
    };

    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    // journalctl with Storage=none prints "No journal files were found" + "-- No entries --"
    if text.contains("No journal files")
        || text.contains("-- No entries --")
        || text.trim().is_empty()
    {
        return String::new();
    }
    text
}

/// Read the last N lines of a log file (newest last, reversed for display).
#[cfg(feature = "ssr")]
async fn read_log_file_tail(path: &str, lines: usize) -> String {
    let output = tokio::process::Command::new("tail")
        .args(["-n", &lines.to_string(), path])
        .output()
        .await;
    match output {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout).into_owned();
            // Reverse lines so newest is first (matching journalctl --reverse).
            let reversed: Vec<&str> = text.lines().rev().collect();
            reversed.join("\n")
        }
        _ => String::new(),
    }
}

#[server(prefix = "/sfn")]
pub async fn refresh_storage() -> Result<RefreshResult, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let changed = state
        .refresh_storage()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    let storage = state.storage();
    Ok(RefreshResult {
        changed,
        storage_kind: format!("{:?}", storage.kind).to_lowercase(),
        storage_root: storage.root.display().to_string(),
    })
}
