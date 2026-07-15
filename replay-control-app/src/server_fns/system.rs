use super::*;

#[cfg(feature = "ssr")]
use replay_control_core::systems::visible_systems;
#[cfg(feature = "ssr")]
use replay_control_core_server::recents;
#[cfg(feature = "ssr")]
use replay_control_core_server::storage::DiskUsage;

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

/// Outcome of saving the Replay Control log level.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct LogLevelSaveResult {
    /// True when the level actually changed and a service restart was scheduled
    /// (device only). The client schedules a page reload when this is set.
    pub restarting: bool,
}

#[cfg(feature = "ssr")]
const REPLAY_CONTROL_ENV_FILE: &str = "/etc/default/replay-control";
#[cfg(feature = "ssr")]
const RUST_LOG_ERROR: &str = "error";
#[cfg(feature = "ssr")]
const RUST_LOG_INFO: &str = "info";
#[cfg(feature = "ssr")]
const RUST_LOG_DEBUG: &str =
    "info,replay_control_app=debug,replay_control_core=debug,replay_control_core_server=debug";

/// Shared OS-level stats gather used by both `get_info` and `get_live_stats`.
///
/// Runs disk usage and network IP probes concurrently (both spawn subprocesses
/// and are independent). Skips library DB queries — callers layer those on top.
#[cfg(feature = "ssr")]
async fn gather_live_stats(state: &crate::api::AppState) -> SystemLiveStats {
    let storage = state.storage();
    let (disk_result, (ethernet_ip, wifi_ip), (ethernet_mac, wifi_mac)) =
        tokio::join!(storage.disk_usage(), get_network_ips(), get_network_macs());
    let disk = disk_result.unwrap_or(DiskUsage {
        total_bytes: 0,
        available_bytes: 0,
        used_bytes: 0,
    });
    let (model, cpu_temperature_c, available_ram_mb) = if state.mode.is_device() {
        (
            read_pi_model(),
            read_cpu_temperature_c(),
            read_available_ram_mb(),
        )
    } else {
        (None, None, None)
    };
    SystemLiveStats {
        storage_kind: format!("{:?}", storage.kind).to_lowercase(),
        storage_root: storage.root.display().to_string(),
        disk_total_bytes: disk.total_bytes,
        disk_used_bytes: disk.used_bytes,
        disk_available_bytes: disk.available_bytes,
        ethernet_ip,
        wifi_ip,
        ethernet_mac,
        wifi_mac,
        model,
        cpu_temperature_c,
        available_ram_mb,
        uptime_seconds: read_uptime_seconds(),
    }
}

#[server(prefix = "/sfn")]
pub async fn get_info() -> Result<SystemInfo, ServerFnError> {
    #[cfg(feature = "ssr")]
    let fn_start = std::time::Instant::now();
    let state = super::app_state()?;
    let storage = state.storage();
    let system_meta = state
        .library_reader
        .read(LibraryDb::load_all_system_meta)
        .await
        .and_then(Result::ok)
        .unwrap_or_default();
    let total_favorites = state.library.get_favorites_count(&storage).await;
    let live = gather_live_stats(&state).await;

    let systems_with_games = system_meta.iter().filter(|s| s.rom_count > 0).count();
    let total_games: usize = system_meta.iter().map(|s| s.rom_count).sum();

    #[cfg(feature = "ssr")]
    tracing::debug!(
        elapsed_ms = fn_start.elapsed().as_millis(),
        "get_info complete"
    );
    Ok(SystemInfo {
        storage_kind: live.storage_kind,
        storage_root: live.storage_root,
        disk_total_bytes: live.disk_total_bytes,
        disk_used_bytes: live.disk_used_bytes,
        disk_available_bytes: live.disk_available_bytes,
        total_systems: visible_systems().count(),
        systems_with_games,
        total_games,
        total_favorites,
        ethernet_ip: live.ethernet_ip,
        wifi_ip: live.wifi_ip,
        model: live.model,
        cpu_temperature_c: live.cpu_temperature_c,
        available_ram_mb: live.available_ram_mb,
        mode: state.mode.clone(),
    })
}

/// Lightweight variant of `get_info` for the settings page live-refresh loop.
///
/// Skips library DB queries (no game/system counts) — reads only the OS-level
/// fields that change at runtime: disk usage, network IPs, CPU temp, RAM.
#[server(prefix = "/sfn")]
pub async fn get_live_stats() -> Result<SystemLiveStats, ServerFnError> {
    let state = super::app_state()?;
    Ok(gather_live_stats(&state).await)
}

/// Pi model name, e.g. "Raspberry Pi 5" (from the device tree).
#[cfg(feature = "ssr")]
fn read_pi_model() -> Option<String> {
    std::fs::read_to_string("/proc/device-tree/model")
        .ok()
        .map(|s| s.trim_end_matches('\0').trim().to_string())
        .filter(|s| !s.is_empty())
}

/// CPU temperature in °C (from the thermal zone, reported in millidegrees).
#[cfg(feature = "ssr")]
fn read_cpu_temperature_c() -> Option<f64> {
    std::fs::read_to_string("/sys/class/thermal/thermal_zone0/temp")
        .ok()
        .and_then(|s| s.trim().parse::<f64>().ok())
        .map(|milli| milli / 1000.0)
}

/// Available RAM in MB (from `MemAvailable` in /proc/meminfo, reported in kB).
#[cfg(feature = "ssr")]
fn read_available_ram_mb() -> Option<u64> {
    let meminfo = std::fs::read_to_string("/proc/meminfo").ok()?;
    let line = meminfo.lines().find(|l| l.starts_with("MemAvailable:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb / 1024)
}

/// OS uptime in whole seconds (first field of /proc/uptime).
#[cfg(feature = "ssr")]
fn read_uptime_seconds() -> u64 {
    std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|s| {
            s.split_whitespace()
                .next()
                .and_then(|n| n.parse::<f64>().ok())
        })
        .map(|f| f as u64)
        .unwrap_or(0)
}

/// Lightweight mode probe for pages that need to gate device-only features
/// but don't load full `SystemInfo`. No DB/disk access.
#[server(prefix = "/sfn")]
pub async fn get_mode() -> Result<Mode, ServerFnError> {
    let state = super::app_state()?;
    Ok(state.mode.clone())
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

/// Pick the Ethernet and Wi-Fi MAC from `(interface_name, mac)` pairs, using the
/// same interface-name prefixes as the IP picker — wired (`eth*`/`enp*`) and
/// wireless (`wlan*`/`wlp*`). First prefix with a match wins; callers pass a
/// name-sorted list so the choice is deterministic across interfaces.
#[cfg(feature = "ssr")]
fn classify_macs(interfaces: &[(String, String)]) -> (Option<String>, Option<String>) {
    let pick = |prefixes: &[&str]| -> Option<String> {
        prefixes.iter().find_map(|prefix| {
            interfaces
                .iter()
                .find(|(name, _)| name.starts_with(prefix))
                .map(|(_, mac)| mac.clone())
        })
    };
    (pick(&["eth", "enp"]), pick(&["wlan", "wlp"]))
}

/// Read the wired / wireless interface MACs from sysfs.
///
/// Unlike the IPs, MACs come from hardware and exist whether or not the link is
/// up, so this enumerates `/sys/class/net` directly (in-kernel, microsecond
/// reads — no subprocess) rather than reusing the `ip addr` output, which only
/// lists connected interfaces.
#[cfg(feature = "ssr")]
async fn get_network_macs() -> (Option<String>, Option<String>) {
    let Ok(mut entries) = tokio::fs::read_dir("/sys/class/net").await else {
        return (None, None);
    };
    let mut interfaces: Vec<(String, String)> = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "lo" {
            continue;
        }
        if let Ok(mac) = tokio::fs::read_to_string(format!("/sys/class/net/{name}/address")).await {
            let mac = mac.trim().to_string();
            // Skip placeholder MACs that virtual/down interfaces report.
            if !mac.is_empty() && mac != "00:00:00:00:00:00" {
                interfaces.push((name, mac));
            }
        }
    }
    interfaces.sort();
    classify_macs(&interfaces)
}

#[server(prefix = "/sfn")]
pub async fn get_systems() -> Result<Vec<SystemSummary>, ServerFnError> {
    #[cfg(feature = "ssr")]
    let fn_start = std::time::Instant::now();
    let state = super::app_state()?;
    let result = crate::api::library_systems::system_summaries(&state.library_reader).await;
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
    let state = super::app_state()?;
    let storage = state.storage();
    let entries = state
        .library
        .get_recents(&storage)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    // Collect (system, rom_filename) pairs for a batch DB lookup.
    let keys: Vec<(String, String)> = entries
        .iter()
        .map(|e| (e.game.system.clone(), e.game.rom_filename.clone()))
        .collect();

    // Batch-lookup box_art_url from game_library (most entries will have it).
    let db_entries = crate::server_fns::lookup_entries_by_keys(&state, keys).await;
    #[cfg(feature = "ssr")]
    tracing::debug!(
        elapsed_ms = fn_start.elapsed().as_millis(),
        "get_recents db_read complete"
    );

    // Prefer the scanned library row as the display-name source of truth; keep
    // marker-derived data only when the row is missing (for stale recents).
    let mut enriched = Vec::with_capacity(entries.len());
    for mut entry in entries {
        let db_entry =
            db_entries.get(&(entry.game.system.clone(), entry.game.rom_filename.clone()));
        let box_art_url = db_entry.and_then(|e| e.box_art_url.clone());
        if let Some(db_entry) = db_entry {
            entry.game = GameRef::new_with_display(
                &db_entry.system,
                db_entry.rom_filename.clone(),
                db_entry.rom_path.clone(),
                db_entry.display_name.clone(),
            );
        }
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

#[server(prefix = "/sfn")]
pub async fn delete_recent(marker_filename: String) -> Result<(), ServerFnError> {
    let state = super::app_state()?;
    super::require_storage_mutation_allowed(&state, "delete recents").await?;
    let storage = state.storage();
    recents::delete_recent(&storage, &marker_filename)
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    // Bust the in-memory recents cache (and recommendations, which key off
    // recents) so the deletion surfaces on the next `get_recents` instead of
    // reappearing on reload. Mirrors the launch path; the ROM watcher also
    // invalidates on `_recent/` changes but is unreliable on NFS.
    state.library.invalidate_after_launch().await;
    Ok(())
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
                // Emptiness is signalled as an empty string; the page renders the
                // localized empty-state message (LogsEmpty / LogsReplayUnavailable).
                Ok(String::new())
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
pub async fn save_log_level_config(level: String) -> Result<LogLevelSaveResult, ServerFnError> {
    // Read the request context before any await: a mid-flight read
    // panics if the client disconnects and the reactive owner is
    // disposed while the env file is being read/written.
    let state = super::app_state()?;
    #[cfg(feature = "ssr")]
    {
        let rust_log = match level.as_str() {
            "error" => RUST_LOG_ERROR,
            "debug" => RUST_LOG_DEBUG,
            "info" => RUST_LOG_INFO,
            _ => return Err(ServerFnError::new("Invalid log level")),
        };

        let content = tokio::fs::read_to_string(REPLAY_CONTROL_ENV_FILE)
            .await
            .unwrap_or_default();
        // Only rewrite + restart when the value actually changes, so a no-op
        // save never disrupts the session.
        let changed = rust_log_value_from_env_file(&content).as_deref() != Some(rust_log);
        if changed {
            let updated = set_rust_log_in_env_file(&content, rust_log);
            tokio::fs::write(REPLAY_CONTROL_ENV_FILE, updated)
                .await
                .map_err(|e| ServerFnError::new(e.to_string()))?;
        }

        // RUST_LOG is read once at process start (systemd reads EnvironmentFile
        // on start; the tracing filter is built once in main). A change only
        // applies after a restart — do it for the user, device only.
        let restarting = changed && state.mode.is_device();
        if restarting {
            schedule_service_restart();
        }
        Ok(LogLevelSaveResult { restarting })
    }

    #[cfg(not(feature = "ssr"))]
    {
        let _ = level;
        Ok(LogLevelSaveResult { restarting: false })
    }
}

/// Restart the `replay-control` service shortly after the current response is
/// sent, so a `RUST_LOG` change takes effect. Deferred 300ms so this response —
/// and the client's scheduled reload — reach the browser before the process
/// goes down; systemd owns the restart once issued. Failures are logged rather
/// than returned, since the client cannot wait on a restart that kills this
/// process. Mirrors the certificate-rotation restart in `settings.rs`.
#[cfg(feature = "ssr")]
fn schedule_service_restart() {
    tokio::task::spawn(async {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        match std::process::Command::new("systemctl")
            .args(["--no-block", "restart", "replay-control"])
            .output()
        {
            Ok(output) if output.status.success() => {}
            Ok(output) => tracing::error!(
                "log-level service restart was rejected: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
            Err(error) => tracing::error!("log-level service restart failed to start: {error}"),
        }
    });
}

#[cfg(feature = "ssr")]
fn rust_log_level_from_env_file(content: &str) -> &'static str {
    let Some(value) = rust_log_value_from_env_file(content) else {
        return "info";
    };
    let parts: Vec<&str> = value.split(',').map(str::trim).collect();
    if parts
        .iter()
        .any(|part| *part == "debug" || part.ends_with("=debug"))
    {
        return "debug";
    }
    // The global directive is the bare level with no `target=` prefix; only a
    // global `error` maps to the Error option (a `=error` target override
    // doesn't quiet our own crates, so it still reads as Info).
    match parts.iter().find(|part| !part.contains('=')).copied() {
        Some("error") => "error",
        _ => "info",
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
    let state = super::app_state()?;
    let changed = state
        .reload_config_and_redetect_storage()
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    let storage = state.storage();
    Ok(RefreshResult {
        changed,
        storage_kind: format!("{:?}", storage.kind).to_lowercase(),
        storage_root: storage.root.display().to_string(),
    })
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use super::*;

    fn ifaces(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(n, m)| (n.to_string(), m.to_string()))
            .collect()
    }

    #[test]
    fn classify_macs_picks_wired_and_wireless_by_prefix() {
        let interfaces = ifaces(&[
            ("docker0", "02:42:aa:bb:cc:dd"),
            ("eth0", "aa:bb:cc:dd:ee:ff"),
            ("wlan0", "11:22:33:44:55:66"),
        ]);
        let (eth, wifi) = classify_macs(&interfaces);
        assert_eq!(eth.as_deref(), Some("aa:bb:cc:dd:ee:ff"));
        assert_eq!(wifi.as_deref(), Some("11:22:33:44:55:66"));
    }

    #[test]
    fn classify_macs_handles_predictable_names_and_missing_wifi() {
        // `enp*` / `wlp*` predictable names, no wireless adapter present.
        let (eth, wifi) = classify_macs(&ifaces(&[("enp3s0", "de:ad:be:ef:00:01")]));
        assert_eq!(eth.as_deref(), Some("de:ad:be:ef:00:01"));
        assert_eq!(wifi, None);
    }

    #[test]
    fn classify_macs_prefers_eth_over_enp() {
        // Both naming schemes present (sorted input): `eth*` wins, matching the
        // IP picker's `eth` then `enp` order.
        let (eth, _) = classify_macs(&ifaces(&[
            ("enp3s0", "de:ad:be:ef:00:01"),
            ("eth0", "aa:bb:cc:dd:ee:ff"),
        ]));
        assert_eq!(eth.as_deref(), Some("aa:bb:cc:dd:ee:ff"));
    }

    #[test]
    fn rust_log_level_reader_maps_error_debug_info() {
        // Missing RUST_LOG defaults to info.
        assert_eq!(rust_log_level_from_env_file(""), "info");
        assert_eq!(rust_log_level_from_env_file("RUST_LOG=info"), "info");
        // Global error → error.
        assert_eq!(rust_log_level_from_env_file("RUST_LOG=error"), "error");
        // Any debug target → debug (matches the DEBUG preset).
        assert_eq!(
            rust_log_level_from_env_file(&format!("RUST_LOG={RUST_LOG_DEBUG}")),
            "debug"
        );
        assert_eq!(
            rust_log_level_from_env_file("RUST_LOG=\"info,replay_control_app=debug\""),
            "debug"
        );
        // A `=error` target override doesn't quiet our crates → still info.
        assert_eq!(
            rust_log_level_from_env_file("RUST_LOG=info,some_dep=error"),
            "info"
        );
        // Commented line is ignored.
        assert_eq!(rust_log_level_from_env_file("# RUST_LOG=error"), "info");
    }

    #[test]
    fn set_rust_log_round_trips_each_preset() {
        for preset in [RUST_LOG_ERROR, RUST_LOG_INFO, RUST_LOG_DEBUG] {
            let written = set_rust_log_in_env_file("OTHER=1\n", preset);
            assert_eq!(
                rust_log_value_from_env_file(&written).as_deref(),
                Some(preset)
            );
        }
    }
}
