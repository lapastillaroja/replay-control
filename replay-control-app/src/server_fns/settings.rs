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
        ssid: config.get("wifi_name").unwrap_or("").to_string(),
        country: config.get("wifi_country").unwrap_or("").to_string(),
        mode: config.get("wifi_mode").unwrap_or("transition").to_string(),
        hidden: config.get("wifi_hidden").unwrap_or("false") == "true",
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
        .update_config(|config| {
            config.set("wifi_name", &ssid);
            config.set("wifi_pwd", &password);
            config.set("wifi_country", &country);
            config.set("wifi_mode", &mode);
            config.set("wifi_hidden", if hidden { "true" } else { "false" });
        })
        .map_err(|e| ServerFnError::new(e.to_string()))
}

#[server(prefix = "/sfn")]
pub async fn get_nfs_config() -> Result<NfsConfig, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let config = state.config.read().expect("config lock poisoned");
    Ok(NfsConfig {
        server: config.get("nfs_server").unwrap_or("").to_string(),
        share: config.get("nfs_share").unwrap_or("").to_string(),
        version: config.get("nfs_version").unwrap_or("4").to_string(),
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
        .update_config(|config| {
            config.set("nfs_server", &server);
            config.set("nfs_share", &share);
            config.set("nfs_version", &version);
        })
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
    let current = state.effective_skin();
    let sync = state
        .skin_override
        .read()
        .expect("skin lock poisoned")
        .is_none();

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
    // When setting a skin manually, disable sync and store the override.
    let mut guard = state.skin_override.write().expect("skin lock poisoned");
    *guard = Some(index);
    Ok(())
}

#[server(prefix = "/sfn")]
pub async fn set_skin_sync(enabled: bool) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    if enabled {
        let mut guard = state.skin_override.write().expect("skin lock poisoned");
        *guard = None;
    } else {
        // Read the current effective skin before acquiring the write lock.
        let current = state.effective_skin();
        let mut guard = state.skin_override.write().expect("skin lock poisoned");
        *guard = Some(current);
    }
    Ok(())
}

/// Get the font size preference from `.replay-control/settings.cfg`.
#[server(prefix = "/sfn")]
pub async fn get_font_size() -> Result<String, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();
    Ok(replay_control_core::settings::read_font_size(&storage.root))
}

/// Save the font size preference to `.replay-control/settings.cfg`.
#[server(prefix = "/sfn")]
pub async fn save_font_size(size: String) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();
    replay_control_core::settings::write_font_size(&storage.root, &size)
        .map_err(|e| ServerFnError::new(e.to_string()))
}

/// Get the GitHub API key from `.replay-control/settings.cfg`.
#[server(prefix = "/sfn")]
pub async fn get_github_api_key() -> Result<String, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();
    Ok(replay_control_core::settings::read_github_api_key(&storage.root).unwrap_or_default())
}

/// Save the GitHub API key to `.replay-control/settings.cfg`.
#[server(prefix = "/sfn")]
pub async fn save_github_api_key(key: String) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();
    replay_control_core::settings::write_github_api_key(&storage.root, key.trim())
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
    let storage = state.storage();
    replay_control_core::settings::write_region_preference(&storage.root, pref)
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    // Invalidate cache so ROM lists are re-sorted with the new preference.
    state.cache.invalidate();
    Ok(())
}
