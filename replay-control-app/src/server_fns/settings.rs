use super::*;

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
pub(crate) fn is_replayos() -> bool {
    std::path::Path::new("/opt/replay").exists()
}

#[server(prefix = "/sfn")]
pub async fn get_wifi_config() -> Result<WifiConfig, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let config = state.config.read().expect("config lock poisoned");
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
    mode: String,
    hidden: bool,
) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    state
        .update_wifi(&ssid, &password, &country, &mode, hidden)
        .map_err(|e| ServerFnError::new(e.to_string()))
}

#[server(prefix = "/sfn")]
pub async fn get_nfs_config() -> Result<NfsConfig, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let config = state.config.read().expect("config lock poisoned");
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
) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    state
        .update_nfs(&server, &share, &version)
        .map_err(|e| ServerFnError::new(e.to_string()))
}

#[server(prefix = "/sfn")]
pub async fn restart_replay_ui() -> Result<String, ServerFnError> {
    if !is_replayos() {
        return Ok("Restart skipped (not running on ReplayOS)".to_string());
    }

    let output = std::process::Command::new("systemctl")
        .args(["restart", "replay"])
        .output()
        .map_err(|e| ServerFnError::new(format!("Failed to restart: {e}")))?;

    if output.status.success() {
        Ok("ReplayOS restarted".to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(ServerFnError::new(format!("Restart failed: {stderr}")))
    }
}

#[server(prefix = "/sfn")]
pub async fn reboot_system() -> Result<String, ServerFnError> {
    if !is_replayos() {
        return Ok("Reboot skipped (not running on ReplayOS)".to_string());
    }

    // Sync filesystem before reboot (as recommended by ReplayOS docs).
    let _ = std::process::Command::new("sync").output();

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
    if !is_replayos() {
        return Ok("Hostname change skipped (not running on ReplayOS)".to_string());
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
            .config
            .read()
            .expect("config lock poisoned")
            .system_skin()
    });
    let sync = skin_pref.is_none();

    let skins = replay_control_core::skins::SKIN_NAMES
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let p = replay_control_core::skins::palette(i as u32).unwrap();
            SkinInfo {
                index: i as u32,
                name: name.to_string(),
                bg: p.bg.to_string(),
                surface: p.surface.to_string(),
                surface_hover: p.surface_hover.to_string(),
                border: p.border.to_string(),
                text: p.text.to_string(),
                text_secondary: p.text_secondary.to_string(),
                accent: p.accent.to_string(),
                accent_hover: p.accent_hover.to_string(),
            }
        })
        .collect();

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
    state.cache.invalidate(&state.metadata_pool).await;
    state.response_cache.invalidate_all();
    // Re-resolve release_date mirror columns for the new region preference.
    // Fast (milliseconds on a typical library) — no re-fetch, no re-parse.
    let region_secondary = state.region_preference_secondary();
    state
        .metadata_pool
        .write(move |conn| {
            let _ = replay_control_core_server::metadata_db::MetadataDb::resolve_release_date_for_library(
                conn,
                pref,
                region_secondary,
            );
        })
        .await;
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
    state.cache.invalidate(&state.metadata_pool).await;
    state.response_cache.invalidate_all();
    // Re-resolve release_date mirror columns for the new secondary region preference.
    let region_primary = state.region_preference();
    state
        .metadata_pool
        .write(move |conn| {
            let _ = replay_control_core_server::metadata_db::MetadataDb::resolve_release_date_for_library(
                conn,
                region_primary,
                pref,
            );
        })
        .await;
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

    if !is_replayos() {
        return Ok("Password change skipped (not running on RePlayOS)".to_string());
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
    crate::api::background::BackgroundManager::nuke_update_dir();
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
