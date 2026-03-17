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

#[server(prefix = "/sfn")]
pub async fn get_info() -> Result<SystemInfo, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();
    let summaries = state.cache.get_systems(&storage);
    let total_favorites = state.cache.get_favorites_count(&storage);

    let disk = storage
        .disk_usage()
        .unwrap_or(replay_control_core::storage::DiskUsage {
            total_bytes: 0,
            available_bytes: 0,
            used_bytes: 0,
        });

    let systems_with_games = summaries.iter().filter(|s| s.game_count > 0).count();
    let total_games: usize = summaries.iter().map(|s| s.game_count).sum();

    let (ethernet_ip, wifi_ip) = get_network_ips();

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
fn get_network_ips() -> (Option<String>, Option<String>) {
    let extract_ip = |iface_prefix: &str| -> Option<String> {
        let output = std::process::Command::new("ip")
            .args(["-4", "-o", "addr", "show"])
            .output()
            .ok()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 && parts[1].starts_with(iface_prefix) {
                // Format: "2: eth0    inet 192.168.1.100/24 ..."
                return parts[3].split('/').next().map(|s| s.to_string());
            }
        }
        None
    };
    let eth = extract_ip("eth").or_else(|| extract_ip("enp"));
    let wifi = extract_ip("wlan").or_else(|| extract_ip("wlp"));
    (eth, wifi)
}

#[server(prefix = "/sfn")]
pub async fn get_systems() -> Result<Vec<SystemSummary>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    Ok(state.cache.get_systems(&state.storage()))
}

#[server(prefix = "/sfn")]
pub async fn get_recents() -> Result<Vec<RecentWithArt>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();
    let entries = state
        .cache
        .get_recents(&storage)
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    // Build image indexes per-system (typically only a few distinct systems in recents).
    let mut image_indexes: std::collections::HashMap<
        String,
        std::sync::Arc<crate::api::cache::ImageIndex>,
    > = std::collections::HashMap::new();
    let enriched = entries
        .into_iter()
        .map(|entry| {
            let index = image_indexes
                .entry(entry.game.system.clone())
                .or_insert_with(|| state.cache.get_image_index(&state, &entry.game.system));
            let box_art_url = state.cache.resolve_box_art(
                &state,
                index,
                &entry.game.system,
                &entry.game.rom_filename,
            );
            RecentWithArt { entry, box_art_url }
        })
        .collect();

    Ok(enriched)
}

/// Read system logs.
/// Tries journalctl first; falls back to log files when journald has Storage=none
/// (as on RePlayOS, which disables persistent/volatile journal).
#[server(prefix = "/sfn")]
pub async fn get_system_logs(source: String, lines: usize) -> Result<String, ServerFnError> {
    let lines = lines.min(500);

    // Try journalctl first.
    let journal_text = read_journalctl(&source, lines);
    if !journal_text.is_empty() {
        return Ok(journal_text);
    }

    // Fallback: read from log files.
    match source.as_str() {
        "replay" => Ok(read_log_file_tail("/var/log/replay.log", lines)),
        "replay-control" => Ok(read_log_file_tail("/var/log/replay-control.log", lines)),
        _ => {
            // "all": combine both log files, interleave by showing replay-control first.
            let rc = read_log_file_tail("/var/log/replay-control.log", lines);
            let rp = read_log_file_tail("/var/log/replay.log", lines);
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

#[cfg(feature = "ssr")]
fn read_journalctl(source: &str, lines: usize) -> String {
    let mut cmd = std::process::Command::new("journalctl");
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

    let output = match cmd.output() {
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
fn read_log_file_tail(path: &str, lines: usize) -> String {
    let output = std::process::Command::new("tail")
        .args(["-n", &lines.to_string(), path])
        .output();
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
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    let storage = state.storage();
    Ok(RefreshResult {
        changed,
        storage_kind: format!("{:?}", storage.kind).to_lowercase(),
        storage_root: storage.root.display().to_string(),
    })
}
