use super::*;

#[cfg(feature = "ssr")]
use replay_control_core_server::config::ReplayConfig;

/// WiFi configuration (password is never sent to the client).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WifiConfig {
    pub ssid: String,
    pub country: String,
    pub mode: String,
    pub hidden: bool,
}

/// NFS share configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NfsConfig {
    pub server: String,
    pub share: String,
    pub version: String,
}

/// RetroAchievements configuration. The password is write-only and is never
/// returned to the browser.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetroAchievementsConfig {
    pub username: String,
    pub password_configured: bool,
}

/// Skin info for the skin page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkinInfo {
    pub index: u32,
    pub name: String,
    pub bg: String,
    pub surface: String,
    pub surface_hover: String,
    pub border: String,
    pub text: String,
    pub text_secondary: String,
    pub accent: String,
    pub accent_hover: String,
}

#[cfg(feature = "ssr")]
impl SkinInfo {
    fn from_palette(index: u32, name: String, p: &replay_control_core::skins::SkinPalette) -> Self {
        Self {
            index,
            name,
            bg: p.bg.to_string(),
            surface: p.surface.to_string(),
            surface_hover: p.surface_hover.to_string(),
            border: p.border.to_string(),
            text: p.text.to_string(),
            text_secondary: p.text_secondary.to_string(),
            accent: p.accent.to_string(),
            accent_hover: p.accent_hover.to_string(),
        }
    }
}

/// Error for config-reading server fns when there is no readable `replay.cfg`
/// — off-device, or on-device in the `ConfigUnavailable` state. We return this
/// rather than fabricating an empty config object (the read-side equivalent of
/// the deleted `ReplayConfig::empty()`). The UI gates these pages off-device,
/// so reaching here means the config genuinely isn't available.
#[cfg(feature = "ssr")]
fn config_unavailable() -> ServerFnError {
    ServerFnError::new("System configuration is unavailable")
}

#[server(prefix = "/sfn")]
pub async fn get_wifi_config() -> Result<WifiConfig, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let guard = state
        .replay_config
        .read()
        .expect("replay_config lock poisoned");
    let config = guard.as_ref().ok_or_else(config_unavailable)?;
    Ok(WifiConfig {
        ssid: config.wifi_name().unwrap_or("").to_string(),
        country: config.wifi_country().unwrap_or("").to_string(),
        mode: config.wifi_mode().unwrap_or("transition").to_string(),
        hidden: config.wifi_hidden(),
    })
}

#[server(prefix = "/sfn")]
pub async fn save_wifi_config(
    ssid: String,
    password: String,
    country: String,
    auth_mode: String,
    hidden: bool,
) -> Result<String, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let is_device = state.mode.is_device();
    apply_replay_config_change(is_device, move || {
        state.update_wifi(&ssid, &password, &country, &auth_mode, hidden)
    })
    .await
}

#[server(prefix = "/sfn")]
pub async fn get_nfs_config() -> Result<NfsConfig, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let guard = state
        .replay_config
        .read()
        .expect("replay_config lock poisoned");
    let config = guard.as_ref().ok_or_else(config_unavailable)?;
    Ok(NfsConfig {
        server: config.nfs_server().unwrap_or("").to_string(),
        share: config.nfs_share().unwrap_or("").to_string(),
        version: config.nfs_version().unwrap_or("4").to_string(),
    })
}

#[server(prefix = "/sfn")]
pub async fn save_nfs_config(
    server: String,
    share: String,
    version: String,
) -> Result<String, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let is_device = state.mode.is_device();
    apply_replay_config_change(is_device, move || {
        state.update_nfs(&server, &share, &version)
    })
    .await
}

#[server(prefix = "/sfn")]
pub async fn get_retroachievements_config() -> Result<RetroAchievementsConfig, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let config_path = state.config_file_path();

    // Prefer reading fresh from disk so credentials changed out-of-band
    // (e.g. set on the TV via the RePlayOS UI) are reflected immediately.
    // Any read/parse failure — missing file, transient empty file during an
    // atomic-rename window, malformed bytes — falls back to the in-memory
    // last-known-good config rather than surfacing an error: the GET path
    // should never blank or fail just because we caught the file mid-rewrite.
    let fresh = ReplayConfig::from_file(&config_path).ok();
    let (username, password_configured) = match fresh {
        Some(config) => (
            config
                .retroachievements_username()
                .unwrap_or("")
                .to_string(),
            config.retroachievements_password_configured(),
        ),
        None => {
            let guard = state
                .replay_config
                .read()
                .expect("replay_config lock poisoned");
            let config = guard.as_ref().ok_or_else(config_unavailable)?;
            (
                config
                    .retroachievements_username()
                    .unwrap_or("")
                    .to_string(),
                config.retroachievements_password_configured(),
            )
        }
    };

    Ok(RetroAchievementsConfig {
        username,
        password_configured,
    })
}

#[server(prefix = "/sfn")]
pub async fn save_retroachievements_config_and_restart(
    username: String,
    password: String,
) -> Result<String, ServerFnError> {
    // All-or-nothing: both empty clears the credentials; otherwise both must
    // be provided. Validated at the server-fn entry so the rule applies in
    // both Device (which writes) and Standalone (which skips) — a malformed
    // input is a malformed input regardless of mode, not something the
    // Standalone skip-path should silently accept.
    if username.trim().is_empty() != password.trim().is_empty() {
        return Err(ServerFnError::new(
            "RetroAchievements username and password must be provided together",
        ));
    }
    let state = expect_context::<crate::api::AppState>();
    let is_device = state.mode.is_device();
    apply_replay_config_change(is_device, move || {
        state.update_retroachievements_credentials(&username, &password)
    })
    .await
}

/// Stop the frontend, write the config, then start it again. The `systemctl`
/// calls and the config file I/O are all blocking, so the whole sequence runs
/// on a blocking thread to avoid stalling the async runtime.
///
/// `is_device` is a bool, not the full `Mode`, because this function only
/// branches on "does the OS own this config?" — pre-resolving it at the call
/// site (`state.mode.is_device()`) keeps the signature honest about what it
/// actually needs.
#[cfg(feature = "ssr")]
async fn apply_replay_config_change<F>(
    is_device: bool,
    write_config: F,
) -> Result<String, ServerFnError>
where
    F: FnOnce() -> Result<(), Box<dyn std::error::Error>> + Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        apply_replay_config_change_blocking(is_device, write_config)
    })
    .await
    .map_err(|e| ServerFnError::new(format!("config change task failed: {e}")))?
}

#[cfg(feature = "ssr")]
fn apply_replay_config_change_blocking<F>(
    is_device: bool,
    write_config: F,
) -> Result<String, ServerFnError>
where
    F: FnOnce() -> Result<(), Box<dyn std::error::Error>>,
{
    if !is_device {
        // Standalone is documented as having system-mutation features disabled —
        // no `replay.service` to restart, no `replay.cfg` we're meant to own, no
        // surrounding RePlayOS state to keep in sync. Refuse the write entirely
        // (don't silently mutate the user's storage folder) and return the same
        // skipped result the device path would have for an inert call. The UI
        // hides these pages off-device; a direct /sfn POST hits this guard.
        let _ = write_config;
        return Ok("Save skipped (standalone mode)".to_string());
    }

    replay_control_core_server::replay_service::stop()
        .map_err(|e| ServerFnError::new(format!("Failed to stop: {e}")))?;

    // The frontend is now down. We must bring it back up no matter what happens
    // during the write — including a panic (e.g. a poisoned config lock), which
    // the guard catches on unwind. The guard is disarmed once we run the
    // explicit start below so that start's own error reaches the user.
    let mut restart_guard = StartReplayOnDrop::armed();
    let save_result = write_config();
    restart_guard.disarm();

    let start_result = replay_control_core_server::replay_service::start();

    match (save_result, start_result) {
        (Ok(()), Ok(())) => Ok("ReplayOS restarted".to_string()),
        (Err(save_error), Ok(())) => {
            Err(ServerFnError::new(format!("Failed to save: {save_error}")))
        }
        (Ok(()), Err(start_error)) => Err(ServerFnError::new(format!(
            "Saved, but failed to restart ReplayOS: {start_error}"
        ))),
        (Err(save_error), Err(start_error)) => Err(ServerFnError::new(format!(
            "Failed to save: {save_error}; also failed to start ReplayOS: {start_error}"
        ))),
    }
}

/// Restarts `replay.service` on drop unless disarmed. Used to guarantee the TV
/// frontend is brought back up after `apply_replay_config_change` stops it,
/// even if the config write panics or returns early.
#[cfg(feature = "ssr")]
struct StartReplayOnDrop {
    armed: bool,
}

#[cfg(feature = "ssr")]
impl StartReplayOnDrop {
    fn armed() -> Self {
        Self { armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

#[cfg(feature = "ssr")]
impl Drop for StartReplayOnDrop {
    fn drop(&mut self) {
        if self.armed
            && let Err(e) = replay_control_core_server::replay_service::start()
        {
            tracing::error!("failed to restart replay.service after aborted config change: {e}");
        }
    }
}

#[server(prefix = "/sfn")]
pub async fn restart_replay_ui() -> Result<String, ServerFnError> {
    if !expect_context::<crate::api::AppState>().mode.is_device() {
        return Ok("Restart skipped (standalone mode)".to_string());
    }

    replay_control_core_server::replay_service::restart()
        .map_err(|e| ServerFnError::new(format!("Restart failed: {e}")))?;
    Ok("ReplayOS restarted".to_string())
}

#[server(prefix = "/sfn")]
pub async fn reboot_system() -> Result<String, ServerFnError> {
    if !expect_context::<crate::api::AppState>().mode.is_device() {
        return Ok("Reboot skipped (standalone mode)".to_string());
    }

    // Best-effort flush before reboot, but never *wait* on it: the network share
    // is mounted `hard`, so `sync` blocks indefinitely whenever the NFS server is
    // unreachable — e.g. right after a Wi-Fi change drops the network. Waiting on
    // it (the previous `.output()`) hung this handler so the reboot was never
    // issued. systemd's shutdown sequence syncs and unmounts filesystems anyway,
    // so a fire-and-forget flush is enough.
    let _ = std::process::Command::new("sync").spawn();

    let output = std::process::Command::new("reboot")
        .output()
        .map_err(|e| ServerFnError::new(format!("Failed to reboot: {e}")))?;

    if output.status.success() {
        Ok("Rebooting...".to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(ServerFnError::new(format!("Reboot failed: {stderr}")))
    }
}

#[server(prefix = "/sfn")]
pub async fn get_hostname() -> Result<String, ServerFnError> {
    let content = std::fs::read_to_string("/etc/hostname")
        .map_err(|e| ServerFnError::new(format!("Failed to read hostname: {e}")))?;
    Ok(content.trim().to_string())
}

#[server(prefix = "/sfn")]
pub async fn save_hostname(hostname: String) -> Result<String, ServerFnError> {
    if !expect_context::<crate::api::AppState>().mode.is_device() {
        return Ok("Hostname change skipped (standalone mode)".to_string());
    }

    let hostname = hostname.trim().to_lowercase();

    // Validate: 1-63 chars, lowercase alphanumeric + hyphens, no leading/trailing hyphens.
    if hostname.is_empty() || hostname.len() > 63 {
        return Err(ServerFnError::new("Hostname must be 1-63 characters"));
    }
    if hostname.starts_with('-') || hostname.ends_with('-') {
        return Err(ServerFnError::new(
            "Hostname must not start or end with a hyphen",
        ));
    }
    if !hostname
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(ServerFnError::new(
            "Hostname must contain only lowercase letters, digits, and hyphens",
        ));
    }

    // Read old hostname for /etc/hosts update.
    let old_hostname = std::fs::read_to_string("/etc/hostname")
        .unwrap_or_default()
        .trim()
        .to_string();

    // Step 1: Set hostname via hostnamectl (updates /etc/hostname + kernel).
    let output = std::process::Command::new("hostnamectl")
        .args(["set-hostname", &hostname])
        .output()
        .map_err(|e| ServerFnError::new(format!("Failed to set hostname: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ServerFnError::new(format!("hostnamectl failed: {stderr}")));
    }

    // Step 2: Update /etc/hosts — replace old hostname with new.
    if !old_hostname.is_empty()
        && old_hostname != hostname
        && let Ok(hosts) = std::fs::read_to_string("/etc/hosts")
    {
        let updated = hosts.replace(&old_hostname, &hostname);
        let _ = std::fs::write("/etc/hosts", updated);
    }

    // Step 3: Restart Avahi so mDNS broadcasts the new name.
    let _ = std::process::Command::new("systemctl")
        .args(["restart", "avahi-daemon"])
        .output();

    Ok(format!("Hostname set to {hostname}"))
}

/// Skin page data: (active_skin_index, sync_enabled, skins_list).
#[server(prefix = "/sfn")]
pub async fn get_skins() -> Result<(u32, bool, Vec<SkinInfo>), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let skin_pref = state.prefs.read().expect("prefs lock poisoned").skin;
    let current = skin_pref.unwrap_or_else(|| {
        state
            .replay_config
            .read()
            .expect("replay_config lock poisoned")
            .as_ref()
            .map(|c| c.system_skin())
            .unwrap_or(0)
    });
    let sync = skin_pref.is_none();

    let mut skins: Vec<SkinInfo> = replay_control_core::skins::SKIN_NAMES
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let idx = i as u32;
            SkinInfo::from_palette(
                idx,
                name.to_string(),
                replay_control_core::skins::palette(idx).unwrap(),
            )
        })
        .collect();

    // Without a synthetic entry the grid would have nothing to highlight as
    // active when the user is on a custom replayos skin, leaving them with
    // no signal about what's selected.
    if replay_control_core::skins::is_custom(current) {
        skins.push(SkinInfo::from_palette(
            current,
            format!("CUSTOM #{current}"),
            replay_control_core::skins::palette_or_default(current),
        ));
    }

    Ok((current, sync, skins))
}

#[server(prefix = "/sfn")]
pub async fn set_skin(index: u32) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    // Persist to settings.cfg (not replay.cfg).
    replay_control_core_server::settings::write_skin(&state.settings, Some(index))
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    state.prefs.write().expect("prefs lock poisoned").skin = Some(index);

    let skin_css = replay_control_core::skins::theme_css(index);
    let _ = state.config_tx.send(crate::api::ConfigEvent::SkinChanged {
        skin_index: index,
        skin_css,
    });
    Ok(())
}

#[server(prefix = "/sfn")]
pub async fn set_skin_sync(enabled: bool) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    if enabled {
        // Clear the skin from settings.cfg so we defer to replay.cfg.
        replay_control_core_server::settings::write_skin(&state.settings, None)
            .map_err(|e| ServerFnError::new(e.to_string()))?;
        state.prefs.write().expect("prefs lock poisoned").skin = None;
    } else {
        let current = state.effective_skin();
        replay_control_core_server::settings::write_skin(&state.settings, Some(current))
            .map_err(|e| ServerFnError::new(e.to_string()))?;
        state.prefs.write().expect("prefs lock poisoned").skin = Some(current);
    }

    let effective = state.effective_skin();
    let skin_css = replay_control_core::skins::theme_css(effective);
    let _ = state.config_tx.send(crate::api::ConfigEvent::SkinChanged {
        skin_index: effective,
        skin_css,
    });
    Ok(())
}

/// Get the font size preference from cached preferences.
#[server(prefix = "/sfn")]
pub async fn get_font_size() -> Result<String, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    Ok(state
        .prefs
        .read()
        .expect("prefs lock poisoned")
        .font_size
        .clone())
}

/// Save the font size preference to `.replay-control/settings.cfg`.
#[server(prefix = "/sfn")]
pub async fn save_font_size(size: String) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    replay_control_core_server::settings::write_font_size(&state.settings, &size)
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    state.prefs.write().expect("prefs lock poisoned").font_size = size;
    Ok(())
}

/// Get the GitHub API key from `.replay-control/settings.cfg`.
#[server(prefix = "/sfn")]
pub async fn get_github_api_key() -> Result<String, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    Ok(
        replay_control_core_server::settings::read_github_api_key(&state.settings)
            .unwrap_or_default(),
    )
}

/// Save the GitHub API key to `.replay-control/settings.cfg`.
#[server(prefix = "/sfn")]
pub async fn save_github_api_key(key: String) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    replay_control_core_server::settings::write_github_api_key(&state.settings, key.trim())
        .map_err(|e| ServerFnError::new(e.to_string()))
}

/// Get the current region preference from `.replay-control/settings.cfg`.
#[server(prefix = "/sfn")]
pub async fn get_region_preference() -> Result<String, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let pref = state.region_preference();
    Ok(pref.as_str().to_string())
}

/// Set the region preference in `.replay-control/settings.cfg`.
/// Invalidates the ROM cache so lists are re-sorted with the new preference.
#[server(prefix = "/sfn")]
pub async fn save_region_preference(value: String) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let pref = replay_control_core::rom_tags::RegionPreference::from_str_value(&value);
    replay_control_core_server::settings::write_region_preference(&state.settings, pref)
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    state.prefs.write().expect("prefs lock poisoned").region = pref;
    // Drop only L1; the resolver below rewrites the region-dependent
    // mirror columns. A destructive `clear_all_game_library` here would
    // truncate rows that any concurrent rebuild/import is filling — see
    // `docs/architecture/cross-activity-coordination.md` finding F-3.
    state.cache.invalidate_l1().await;
    state.invalidate_user_caches().await;
    let region_secondary = state.region_preference_secondary();
    match state
        .library_writer
        .try_write_with_timeout(
            crate::api::db_pools::LIBRARY_MAINTENANCE_WRITE_TIMEOUT,
            move |conn| {
                replay_control_core_server::library_db::LibraryDb::resolve_release_date_for_library(
                    conn,
                    pref,
                    region_secondary,
                )
            },
        )
        .await
    {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => tracing::warn!("Region preference release-date resolve failed: {e}"),
        Err(e) => tracing::warn!("Region preference release-date resolve write failed: {e}"),
    }
    Ok(())
}

/// Get the secondary (fallback) region preference from `.replay-control/settings.cfg`.
/// Returns empty string if not set.
#[server(prefix = "/sfn")]
pub async fn get_region_preference_secondary() -> Result<String, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let pref = state.region_preference_secondary();
    Ok(pref.map(|p| p.as_str().to_string()).unwrap_or_default())
}

/// Set the secondary (fallback) region preference in `.replay-control/settings.cfg`.
/// Pass empty string to clear. Invalidates the ROM cache.
#[server(prefix = "/sfn")]
pub async fn save_region_preference_secondary(value: String) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let pref = if value.is_empty() {
        None
    } else {
        Some(replay_control_core::rom_tags::RegionPreference::from_str_value(&value))
    };
    replay_control_core_server::settings::write_region_preference_secondary(&state.settings, pref)
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    state
        .prefs
        .write()
        .expect("prefs lock poisoned")
        .region_secondary = pref;
    // Drop only L1; see save_region_preference for why.
    state.cache.invalidate_l1().await;
    state.invalidate_user_caches().await;
    let region_primary = state.region_preference();
    match state
        .library_writer
        .try_write_with_timeout(
            crate::api::db_pools::LIBRARY_MAINTENANCE_WRITE_TIMEOUT,
            move |conn| {
                replay_control_core_server::library_db::LibraryDb::resolve_release_date_for_library(
                    conn,
                    region_primary,
                    pref,
                )
            },
        )
        .await
    {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => {
            tracing::warn!("Secondary region preference release-date resolve failed: {e}")
        }
        Err(e) => {
            tracing::warn!("Secondary region preference release-date resolve write failed: {e}")
        }
    }
    Ok(())
}
/// Get the language preference from `.replay-control/settings.cfg`.
/// Returns (primary, secondary) where each is empty string if not set.
#[server(prefix = "/sfn")]
pub async fn get_language_preference() -> Result<(String, String), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let settings = state.settings.load();
    let primary = settings.language_primary().unwrap_or_default().to_string();
    let secondary = settings
        .language_secondary()
        .unwrap_or_default()
        .to_string();
    Ok((primary, secondary))
}

/// Save the language preference to `.replay-control/settings.cfg`.
/// Empty strings clear the respective fields (revert to auto-detection).
#[server(prefix = "/sfn")]
pub async fn save_language_preference(
    primary: String,
    secondary: String,
) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    replay_control_core_server::settings::write_language_preferences(
        &state.settings,
        primary.trim(),
        secondary.trim(),
    )
    .map_err(|e| ServerFnError::new(e.to_string()))
}

/// Change the root password on the Pi.
/// Verifies the current password before applying the new one.
#[server(prefix = "/sfn")]
pub async fn change_root_password(
    current_password: String,
    new_password: String,
) -> Result<String, ServerFnError> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    if !expect_context::<crate::api::AppState>().mode.is_device() {
        return Ok("Password change skipped (standalone mode)".to_string());
    }

    if new_password.is_empty() {
        return Err(ServerFnError::new("Password cannot be empty"));
    }

    // Verify the current password against /etc/shadow.
    // Both `su` and `unix_chkpwd` skip authentication when called by root,
    // so we must verify directly.
    let shadow = std::fs::read_to_string("/etc/shadow")
        .map_err(|e| ServerFnError::new(format!("Cannot read shadow file: {e}")))?;

    let stored_hash = shadow
        .lines()
        .find(|line| line.starts_with("root:"))
        .and_then(|line| line.split(':').nth(1))
        .ok_or_else(|| ServerFnError::new("Cannot find root password hash"))?;

    if stored_hash == "*" || stored_hash == "!" || stored_hash.is_empty() {
        return Err(ServerFnError::new("Root account has no password set"));
    }

    // Verify via libcrypt's crypt() called through Python3 ctypes.
    // This avoids cross-compilation issues with libcrypt soname mismatches
    // and supports all hash algorithms including yescrypt ($y$).
    // Password is sent via stdin to avoid exposing it in /proc/cmdline.
    let mut child = Command::new("python3")
        .args([
            "-c",
            "import sys,ctypes; d=sys.stdin.read().split('\\n',1); \
             l=ctypes.CDLL('libcrypt.so.1'); l.crypt.restype=ctypes.c_char_p; \
             r=l.crypt(d[0].encode(),d[1].encode()); print(r.decode() if r else '')",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| ServerFnError::new(format!("Failed to verify password: {e}")))?;

    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| ServerFnError::new("Failed to open stdin"))?;
        stdin
            .write_all(format!("{current_password}\n{stored_hash}").as_bytes())
            .map_err(|e| ServerFnError::new(format!("Failed to verify password: {e}")))?;
        // stdin is dropped here, closing the pipe so Python sees EOF.
    }

    let output = child
        .wait_with_output()
        .map_err(|e| ServerFnError::new(format!("Failed to verify password: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ServerFnError::new(format!(
            "Password verification failed: {stderr}"
        )));
    }

    let computed_hash = String::from_utf8_lossy(&output.stdout).trim().to_string();

    if computed_hash != stored_hash {
        return Err(ServerFnError::new("Current password is incorrect"));
    }

    // Apply the new password via chpasswd.
    let mut child = Command::new("chpasswd")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| ServerFnError::new(format!("Failed to run chpasswd: {e}")))?;

    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(format!("root:{new_password}\n").as_bytes())
            .map_err(|e| ServerFnError::new(format!("Failed to write to chpasswd: {e}")))?;
    }

    let output = child
        .wait_with_output()
        .map_err(|e| ServerFnError::new(format!("chpasswd failed: {e}")))?;

    if output.status.success() {
        Ok("Password changed successfully".to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(ServerFnError::new(format!("chpasswd failed: {stderr}")))
    }
}

/// Get the UI locale from cached preferences.
/// Returns the stored locale preference code (including "auto").
#[server(prefix = "/sfn")]
pub async fn get_locale() -> Result<String, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    use replay_control_core::locale::Locale;
    let locale = state
        .prefs
        .read()
        .expect("prefs lock poisoned")
        .locale
        .map(|l| l.code())
        .unwrap_or(Locale::Auto.code());
    Ok(locale.to_string())
}

/// Save the UI locale to `.replay-control/settings.cfg`.
/// Validates against the supported locale list before writing.
#[server(prefix = "/sfn")]
pub async fn save_locale(locale: String) -> Result<(), ServerFnError> {
    use replay_control_core::locale::Locale;
    if !Locale::all_codes().contains(&locale.as_str()) {
        return Err(ServerFnError::new("Unsupported locale"));
    }
    let parsed = Locale::from_code(&locale);
    let state = expect_context::<crate::api::AppState>();
    replay_control_core_server::settings::write_locale(&state.settings, parsed)
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    state.prefs.write().expect("prefs lock poisoned").locale = parsed.effective();
    Ok(())
}

/// Get the user's preferred languages as a priority-ordered list.
/// Used by manual search to sort results by language relevance.
#[server(prefix = "/sfn")]
pub async fn get_preferred_languages() -> Result<Vec<String>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let settings = state.settings.load();
    let primary = settings.language_primary();
    let secondary = settings.language_secondary();
    let region = state.region_preference();
    Ok(replay_control_core_server::settings::preferred_languages(
        primary, secondary, region,
    ))
}

/// Get the analytics preference from `.replay-control/settings.cfg`.
/// Returns `true` if analytics is enabled (default).
#[server(prefix = "/sfn")]
pub async fn get_analytics_preference() -> Result<bool, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    Ok(replay_control_core_server::settings::read_analytics_enabled(&state.settings))
}

/// Save the analytics preference to `.replay-control/settings.cfg`.
#[server(prefix = "/sfn")]
pub async fn save_analytics_preference(enabled: bool) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    replay_control_core_server::settings::write_analytics(&state.settings, enabled)
        .map_err(|e| ServerFnError::new(e.to_string()))
}

/// Trigger an immediate update check against GitHub API.
/// Nukes the update dir first, checks, writes available.json if found.
#[server(prefix = "/sfn")]
pub async fn check_for_updates()
-> Result<Option<replay_control_core::update::AvailableUpdate>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    crate::api::background::BackgroundManager::perform_update_check(&state)
        .await
        .map_err(|e| ServerFnError::new(format!("Update check failed: {e}")))
}

/// Read the current update channel from settings.cfg.
#[server(prefix = "/sfn")]
pub async fn get_update_channel() -> Result<String, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    Ok(
        replay_control_core_server::settings::read_update_channel(&state.settings)
            .as_str()
            .to_string(),
    )
}

/// Save the update channel. Nukes the update dir and triggers an immediate re-check.
#[server(prefix = "/sfn")]
pub async fn save_update_channel(channel: String) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let channel_val = replay_control_core::update::UpdateChannel::from_str_value(&channel);
    replay_control_core_server::settings::write_update_channel(&state.settings, channel_val)
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    // Nuke stale update state and trigger re-check with new channel.
    replay_control_core_server::update::nuke_update_dir();
    let state_clone = state.clone();
    tokio::spawn(async move {
        match crate::api::background::BackgroundManager::perform_update_check(&state_clone).await {
            Ok(_) => {}
            Err(e) => tracing::debug!("Re-check after channel switch failed: {e}"),
        }
    });
    Ok(())
}

/// Mark a version as skipped in settings.cfg.
#[server(prefix = "/sfn")]
pub async fn skip_version(version: String) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    replay_control_core_server::settings::write_skipped_version(&state.settings, &version)
        .map_err(|e| ServerFnError::new(e.to_string()))
}

/// Start the download + install process for a specific release tag.
/// Returns Ok after spawning the helper script (server will restart shortly).
#[server(prefix = "/sfn")]
pub async fn start_update(tag: String) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    crate::api::background::BackgroundManager::start_update(&state, &tag)
        .await
        .map_err(|e| ServerFnError::new(format!("Update failed: {e}")))
}
