use leptos::prelude::*;
use server_fn::ServerFnError;
use serde::{Deserialize, Serialize};

#[cfg(feature = "ssr")]
pub use replay_control_core::favorites::OrganizeCriteria;
#[cfg(not(feature = "ssr"))]
pub use crate::types::OrganizeCriteria;

pub const PAGE_SIZE: usize = 100;

/// Unified game metadata returned by server functions.
/// Populated from arcade_db or game_db depending on the system,
/// but consumers never need to know which source was used.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameInfo {
    // --- Identity (always present) ---
    pub system: String,
    pub system_display: String,
    pub rom_filename: String,
    pub rom_path: String,
    pub display_name: String,

    // --- Common metadata (from either DB) ---
    pub year: String,
    pub genre: String,
    pub developer: String,
    pub players: u8,

    // --- Arcade-specific (None for non-arcade) ---
    pub rotation: Option<String>,
    pub driver_status: Option<String>,
    pub is_clone: Option<bool>,
    pub parent_rom: Option<String>,
    pub arcade_category: Option<String>,

    // --- Console-specific (None for arcade) ---
    pub region: Option<String>,

    // --- External metadata (from local cache, None if not yet fetched) ---
    pub description: Option<String>,
    pub rating: Option<f32>,
    pub publisher: Option<String>,
}

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
pub use crate::types::{Favorite, GameRef, RecentEntry, RomEntry, SystemSummary};

/// Resolve full game metadata for any system.
/// This is the single function that bridges arcade_db and game_db.
#[cfg(feature = "ssr")]
fn resolve_game_info(system: &str, rom_filename: &str, rom_path: &str) -> GameInfo {
    use replay_control_core::arcade_db;
    use replay_control_core::game_db;
    use replay_control_core::rom_tags;
    use replay_control_core::systems::{self, SystemCategory};

    let sys_info = systems::find_system(system);
    let system_display = sys_info
        .map(|s| s.display_name.to_string())
        .unwrap_or_else(|| system.to_string());
    let is_arcade = sys_info.is_some_and(|s| s.category == SystemCategory::Arcade);

    let mut info = if is_arcade {
        let stem = rom_filename.strip_suffix(".zip").unwrap_or(rom_filename);
        match arcade_db::lookup_arcade_game(stem) {
            Some(info) => {
                let rotation = match info.rotation {
                    arcade_db::Rotation::Horizontal => "Horizontal",
                    arcade_db::Rotation::Vertical => "Vertical",
                    arcade_db::Rotation::Unknown => "Unknown",
                };
                let driver_status = match info.status {
                    arcade_db::DriverStatus::Working => "Working",
                    arcade_db::DriverStatus::Imperfect => "Imperfect",
                    arcade_db::DriverStatus::Preliminary => "Preliminary",
                    arcade_db::DriverStatus::Unknown => "Unknown",
                };
                GameInfo {
                    system: system.to_string(),
                    system_display,
                    rom_filename: rom_filename.to_string(),
                    rom_path: rom_path.to_string(),
                    display_name: info.display_name.to_string(),
                    year: info.year.to_string(),
                    genre: info.normalized_genre.to_string(),
                    developer: info.manufacturer.to_string(),
                    players: info.players,
                    rotation: Some(rotation.to_string()),
                    driver_status: Some(driver_status.to_string()),
                    is_clone: Some(info.is_clone),
                    parent_rom: if info.is_clone {
                        Some(info.parent.to_string())
                    } else {
                        None
                    },
                    arcade_category: if info.category.is_empty() {
                        None
                    } else {
                        Some(info.category.to_string())
                    },
                    region: None,
                    description: None,
                    rating: None,
                    publisher: None,
                }
            }
            None => GameInfo {
                system: system.to_string(),
                system_display,
                rom_filename: rom_filename.to_string(),
                rom_path: rom_path.to_string(),
                display_name: rom_filename.to_string(),
                year: String::new(),
                genre: String::new(),
                developer: String::new(),
                players: 0,
                rotation: None,
                driver_status: None,
                is_clone: None,
                parent_rom: None,
                arcade_category: None,
                region: None,
                description: None,
                rating: None,
                publisher: None,
            },
        }
    } else {
        let stem = rom_filename
            .rfind('.')
            .map(|i| &rom_filename[..i])
            .unwrap_or(rom_filename);

        // Try exact match, then normalized title fallback
        let entry = game_db::lookup_game(system, stem);
        let game = entry.map(|e| e.game);
        let region = entry.map(|e| e.region).unwrap_or("");

        // If exact match failed, try normalized title for display name
        let display_name = if let Some(g) = game {
            rom_tags::display_name_with_tags(g.display_name, rom_filename)
        } else if let Some(dn) = game_db::game_display_name(system, rom_filename) {
            rom_tags::display_name_with_tags(dn, rom_filename)
        } else {
            rom_filename.to_string()
        };

        // For metadata, also try normalized title fallback
        let game_meta = game.or_else(|| {
            let normalized = game_db::normalize_filename(stem);
            game_db::lookup_by_normalized_title(system, &normalized)
        });

        GameInfo {
            system: system.to_string(),
            system_display,
            rom_filename: rom_filename.to_string(),
            rom_path: rom_path.to_string(),
            display_name,
            year: game_meta
                .map(|g| {
                    if g.year > 0 {
                        g.year.to_string()
                    } else {
                        String::new()
                    }
                })
                .unwrap_or_default(),
            genre: game_meta
                .map(|g| g.normalized_genre.to_string())
                .unwrap_or_default(),
            developer: game_meta
                .map(|g| g.developer.to_string())
                .unwrap_or_default(),
            players: game_meta.map(|g| g.players).unwrap_or(0),
            rotation: None,
            driver_status: None,
            is_clone: None,
            parent_rom: None,
            arcade_category: None,
            region: if region.is_empty() {
                None
            } else {
                Some(region.to_string())
            },
            description: None,
            rating: None,
            publisher: None,
        }
    };

    // Enrich with external metadata from local cache.
    enrich_from_metadata_cache(&mut info);

    info
}

/// Look up cached external metadata and enrich the GameInfo.
#[cfg(feature = "ssr")]
fn enrich_from_metadata_cache(info: &mut GameInfo) {
    let state = leptos::prelude::expect_context::<crate::api::AppState>();
    if let Some(guard) = state.metadata_db() {
        if let Some(db) = guard.as_ref() {
            match db.lookup(&info.system, &info.rom_filename) {
                Ok(Some(meta)) => {
                    info.description = meta.description;
                    info.rating = meta.rating.map(|r| r as f32);
                    if meta.publisher.is_some() {
                        info.publisher = meta.publisher;
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    tracing::debug!("Metadata lookup failed for {}/{}: {e}", info.system, info.rom_filename);
                }
            }
        }
    }
}

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
            let display = r.game.display_name.as_deref().unwrap_or(&r.game.rom_filename);
            display.to_lowercase().contains(&q)
                || r.game.rom_filename.to_lowercase().contains(&q)
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

/// Detailed ROM info including unified game metadata and favorite status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RomDetail {
    pub game: GameInfo,
    pub size_bytes: u64,
    pub is_m3u: bool,
    pub is_favorite: bool,
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

    let game = resolve_game_info(&system, &filename, &rom.game.rom_path);

    Ok(RomDetail {
        game,
        size_bytes: rom.size_bytes,
        is_m3u: rom.is_m3u,
        is_favorite,
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

/// Skin page data: (active_skin_index, sync_enabled, skins_list).
#[server(prefix = "/sfn")]
pub async fn get_skins() -> Result<(u32, bool, Vec<SkinInfo>), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let current = state.effective_skin();
    let sync = state.skin_override.read().expect("skin lock poisoned").is_none();

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

// ── Metadata management ──────────────────────────────────────────

#[cfg(feature = "ssr")]
pub use replay_control_core::metadata_db::{ImportStats, MetadataStats};
#[cfg(not(feature = "ssr"))]
pub use crate::types::{ImportStats, MetadataStats};

/// Get metadata coverage stats.
#[server(prefix = "/sfn")]
pub async fn get_metadata_stats() -> Result<MetadataStats, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let guard = state.metadata_db().ok_or_else(|| ServerFnError::new("Cannot open metadata DB"))?;
    let db = guard.as_ref().ok_or_else(|| ServerFnError::new("Metadata DB not available"))?;
    db.stats().map_err(|e| ServerFnError::new(e.to_string()))
}

/// Import metadata from a LaunchBox Metadata.xml file.
/// The XML path should be accessible on the server's filesystem.
#[server(prefix = "/sfn")]
pub async fn import_launchbox_metadata(xml_path: String) -> Result<ImportStats, ServerFnError> {
    use replay_control_core::launchbox;

    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();
    let path = std::path::PathBuf::from(&xml_path);

    if !path.exists() {
        return Err(ServerFnError::new(format!("File not found: {xml_path}")));
    }

    tracing::info!("Starting LaunchBox import from {xml_path}");

    // Build ROM index from filesystem.
    let rom_index = launchbox::build_rom_index(&storage.root);

    // Open metadata DB.
    let mut guard = state.metadata_db().ok_or_else(|| ServerFnError::new("Cannot open metadata DB"))?;
    let db = guard.as_mut().ok_or_else(|| ServerFnError::new("Metadata DB not available"))?;

    // Parse and import.
    let stats = launchbox::import_launchbox(&path, db, &rom_index)
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    Ok(stats)
}

/// Clear all cached metadata.
#[server(prefix = "/sfn")]
pub async fn clear_metadata() -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let guard = state.metadata_db().ok_or_else(|| ServerFnError::new("Cannot open metadata DB"))?;
    let db = guard.as_ref().ok_or_else(|| ServerFnError::new("Metadata DB not available"))?;
    db.clear().map_err(|e| ServerFnError::new(e.to_string()))
}
