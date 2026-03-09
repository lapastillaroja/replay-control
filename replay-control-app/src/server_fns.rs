use leptos::prelude::*;
use server_fn::ServerFnError;
use serde::{Deserialize, Serialize};

#[cfg(feature = "ssr")]
pub use replay_control_core::favorites::OrganizeCriteria;
#[cfg(not(feature = "ssr"))]
pub use crate::types::OrganizeCriteria;

pub const PAGE_SIZE: usize = 100;

/// System info returned by get_info server function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    pub storage_kind: String,
    pub storage_root: String,
    pub disk_total_bytes: u64,
    pub disk_used_bytes: u64,
    pub disk_available_bytes: u64,
    pub total_systems: usize,
    pub systems_with_games: usize,
    pub total_games: usize,
    pub total_favorites: usize,
    pub ethernet_ip: Option<String>,
    pub wifi_ip: Option<String>,
}

// Re-export types for use in components.
// On the server, use replay-core types directly.
// On the client, use mirror types from types.rs.
#[cfg(feature = "ssr")]
pub use replay_control_core::favorites::Favorite;
#[cfg(feature = "ssr")]
pub use replay_control_core::game_ref::GameRef;
#[cfg(feature = "ssr")]
pub use replay_control_core::recents::RecentEntry;
#[cfg(feature = "ssr")]
pub use replay_control_core::roms::{RomEntry, SystemSummary};

#[cfg(not(feature = "ssr"))]
pub use crate::types::{
    ArcadeMetadata, Favorite, GameRef, RecentEntry, RomDetail, RomEntry, SystemSummary,
};

#[server(prefix = "/sfn")]
pub async fn get_info() -> Result<SystemInfo, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();
    let summaries = state.cache.get_systems(&storage);
    let favorites = replay_control_core::favorites::list_favorites(&storage).unwrap_or_default();

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
        total_favorites: favorites.len(),
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

/// A page of ROM results with total count.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RomPage {
    pub roms: Vec<RomEntry>,
    pub total: usize,
    pub has_more: bool,
    /// Human-readable system name (e.g., "Arcade (Atomiswave/Naomi)")
    #[serde(default)]
    pub system_display: String,
}

#[server(prefix = "/sfn")]
pub async fn get_roms_page(
    system: String,
    offset: usize,
    limit: usize,
    search: String,
) -> Result<RomPage, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let system_display = replay_control_core::systems::find_system(&system)
        .map(|s| s.display_name.to_string())
        .unwrap_or_else(|| system.clone());
    let storage = state.storage();
    let all_roms = state.cache.get_roms(&storage, &system)
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    let filtered: Vec<RomEntry> = if search.is_empty() {
        all_roms
    } else {
        let q = search.to_lowercase();
        all_roms.into_iter().filter(|r| {
            r.game.rom_filename.to_lowercase().contains(&q)
                || r.game.display_name.as_ref().is_some_and(|dn| dn.to_lowercase().contains(&q))
        }).collect()
    };

    let total = filtered.len();
    let mut roms: Vec<RomEntry> = filtered.into_iter().skip(offset).take(limit).collect();
    let has_more = offset + roms.len() < total;

    replay_control_core::roms::mark_favorites(&storage, &system, &mut roms);

    Ok(RomPage { roms, total, has_more, system_display })
}

#[server(prefix = "/sfn")]
pub async fn get_favorites() -> Result<Vec<Favorite>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    replay_control_core::favorites::list_favorites(&state.storage())
        .map_err(|e| ServerFnError::new(e.to_string()))
}

#[server(prefix = "/sfn")]
pub async fn get_recents() -> Result<Vec<RecentEntry>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    replay_control_core::recents::list_recents(&state.storage())
        .map_err(|e| ServerFnError::new(e.to_string()))
}

#[server(prefix = "/sfn")]
pub async fn add_favorite(
    system: String,
    rom_path: String,
    grouped: bool,
) -> Result<Favorite, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    replay_control_core::favorites::add_favorite(&state.storage(), &system, &rom_path, grouped)
        .map_err(|e| ServerFnError::new(e.to_string()))
}

#[server(prefix = "/sfn")]
pub async fn remove_favorite(
    filename: String,
    subfolder: Option<String>,
) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    replay_control_core::favorites::remove_favorite(&state.storage(), &filename, subfolder.as_deref())
        .map_err(|e| ServerFnError::new(e.to_string()))
}

#[server(prefix = "/sfn")]
pub async fn group_favorites() -> Result<usize, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    replay_control_core::favorites::group_by_system(&state.storage())
        .map_err(|e| ServerFnError::new(e.to_string()))
}

#[server(prefix = "/sfn")]
pub async fn flatten_favorites() -> Result<usize, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    replay_control_core::favorites::flatten_favorites(&state.storage())
        .map_err(|e| ServerFnError::new(e.to_string()))
}

#[server(prefix = "/sfn")]
pub async fn get_system_favorites(system: String) -> Result<Vec<Favorite>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    replay_control_core::favorites::list_favorites_for_system(&state.storage(), &system)
        .map_err(|e| ServerFnError::new(e.to_string()))
}

#[server(prefix = "/sfn")]
pub async fn delete_rom(relative_path: String) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    replay_control_core::roms::delete_rom(&state.storage(), &relative_path)
        .map_err(|e| ServerFnError::new(e.to_string()))
}

#[server(prefix = "/sfn")]
pub async fn rename_rom(relative_path: String, new_filename: String) -> Result<String, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let new_path = replay_control_core::roms::rename_rom(&state.storage(), &relative_path, &new_filename)
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    Ok(new_path.display().to_string())
}

/// Detailed ROM info including arcade metadata and favorite status.
#[cfg(feature = "ssr")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RomDetail {
    pub rom: RomEntry,
    pub is_favorite: bool,
    pub arcade_info: Option<ArcadeMetadata>,
}

#[cfg(feature = "ssr")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArcadeMetadata {
    pub year: String,
    pub manufacturer: String,
    pub players: u8,
    pub rotation: String,
    pub category: String,
    pub is_clone: bool,
    pub parent: String,
}

#[server(prefix = "/sfn")]
pub async fn get_rom_detail(system: String, filename: String) -> Result<RomDetail, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();
    let all_roms = state.cache.get_roms(&storage, &system)
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    let rom = all_roms
        .into_iter()
        .find(|r| r.game.rom_filename == filename)
        .ok_or_else(|| ServerFnError::new(format!("ROM not found: {filename}")))?;

    let is_favorite = replay_control_core::favorites::is_favorite(&storage, &system, &filename);

    let arcade_info = replay_control_core::arcade_db::lookup_arcade_game(
        filename.strip_suffix(".zip").unwrap_or(&filename),
    )
    .map(|info| {
        let rotation = match info.rotation {
            replay_control_core::arcade_db::Rotation::Horizontal => "Horizontal",
            replay_control_core::arcade_db::Rotation::Vertical => "Vertical",
            replay_control_core::arcade_db::Rotation::Unknown => "Unknown",
        };
        ArcadeMetadata {
            year: info.year.to_string(),
            manufacturer: info.manufacturer.to_string(),
            players: info.players,
            rotation: rotation.to_string(),
            category: info.category.to_string(),
            is_clone: info.is_clone,
            parent: info.parent.to_string(),
        }
    });

    Ok(RomDetail {
        rom,
        is_favorite,
        arcade_info,
    })
}

/// WiFi configuration (password is never sent to the client).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WifiConfig {
    pub ssid: String,
    pub country: String,
    pub mode: String,
    pub hidden: bool,
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

/// NFS share configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NfsConfig {
    pub server: String,
    pub share: String,
    pub version: String,
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

#[cfg(feature = "ssr")]
fn is_replayos() -> bool {
    std::path::Path::new("/opt/replay").exists()
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
        Err(ServerFnError::new(format!(
            "Restart failed: {stderr}"
        )))
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
        Err(ServerFnError::new(format!(
            "Reboot failed: {stderr}"
        )))
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
        return Err(ServerFnError::new("Hostname must not start or end with a hyphen"));
    }
    if !hostname.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
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
    if !old_hostname.is_empty() && old_hostname != hostname {
        if let Ok(hosts) = std::fs::read_to_string("/etc/hosts") {
            let updated = hosts.replace(&old_hostname, &hostname);
            let _ = std::fs::write("/etc/hosts", updated);
        }
    }

    // Step 3: Restart Avahi so mDNS broadcasts the new name.
    let _ = std::process::Command::new("systemctl")
        .args(["restart", "avahi-daemon"])
        .output();

    Ok(format!("Hostname set to {hostname}"))
}

/// Skin info for the theme page.
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

#[server(prefix = "/sfn")]
pub async fn get_skins() -> Result<(u32, Vec<SkinInfo>), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let current = state.config.read().expect("config lock poisoned").system_skin();

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

    Ok((current, skins))
}

#[server(prefix = "/sfn")]
pub async fn set_skin(index: u32) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    state
        .update_config(|config| {
            config.set("system_skin", &index.to_string());
        })
        .map_err(|e| ServerFnError::new(e.to_string()))
}

/// Result of organizing favorites into subfolders.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrganizeResult {
    pub organized: usize,
    pub skipped: usize,
}

#[server(prefix = "/sfn")]
pub async fn organize_favorites(
    primary: OrganizeCriteria,
    secondary: Option<OrganizeCriteria>,
    keep_originals: bool,
) -> Result<OrganizeResult, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let result = replay_control_core::favorites::organize_favorites(
        &state.storage(),
        primary,
        secondary,
        keep_originals,
    )
    .map_err(|e| ServerFnError::new(e.to_string()))?;
    Ok(OrganizeResult {
        organized: result.organized,
        skipped: result.skipped,
    })
}

/// Result of a storage refresh operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshResult {
    pub changed: bool,
    pub storage_kind: String,
    pub storage_root: String,
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
