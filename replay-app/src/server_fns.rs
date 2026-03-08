use leptos::prelude::*;
use server_fn::ServerFnError;
use serde::{Deserialize, Serialize};

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
}

// Re-export types for use in components.
// On the server, use replay-core types directly.
// On the client, use mirror types from types.rs.
#[cfg(feature = "ssr")]
pub use replay_core::favorites::Favorite;
#[cfg(feature = "ssr")]
pub use replay_core::game_ref::GameRef;
#[cfg(feature = "ssr")]
pub use replay_core::recents::RecentEntry;
#[cfg(feature = "ssr")]
pub use replay_core::roms::{RomEntry, SystemSummary};

#[cfg(not(feature = "ssr"))]
pub use crate::types::{
    ArcadeMetadata, Favorite, GameRef, RecentEntry, RomDetail, RomEntry, SystemSummary,
};

#[server(prefix = "/sfn")]
pub async fn get_info() -> Result<SystemInfo, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let summaries = state.cache.get_systems(&state.storage);
    let favorites = replay_core::favorites::list_favorites(&state.storage).unwrap_or_default();

    let disk = state
        .storage
        .disk_usage()
        .unwrap_or(replay_core::storage::DiskUsage {
            total_bytes: 0,
            available_bytes: 0,
            used_bytes: 0,
        });

    let systems_with_games = summaries.iter().filter(|s| s.game_count > 0).count();
    let total_games: usize = summaries.iter().map(|s| s.game_count).sum();

    Ok(SystemInfo {
        storage_kind: format!("{:?}", state.storage.kind).to_lowercase(),
        storage_root: state.storage.root.display().to_string(),
        disk_total_bytes: disk.total_bytes,
        disk_used_bytes: disk.used_bytes,
        disk_available_bytes: disk.available_bytes,
        total_systems: summaries.len(),
        systems_with_games,
        total_games,
        total_favorites: favorites.len(),
    })
}

#[server(prefix = "/sfn")]
pub async fn get_systems() -> Result<Vec<SystemSummary>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    Ok(state.cache.get_systems(&state.storage))
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
    let system_display = replay_core::systems::find_system(&system)
        .map(|s| s.display_name.to_string())
        .unwrap_or_else(|| system.clone());
    let all_roms = replay_core::roms::list_roms(&state.storage, &system)
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

    replay_core::roms::mark_favorites(&state.storage, &system, &mut roms);

    Ok(RomPage { roms, total, has_more, system_display })
}

#[server(prefix = "/sfn")]
pub async fn get_favorites() -> Result<Vec<Favorite>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    replay_core::favorites::list_favorites(&state.storage)
        .map_err(|e| ServerFnError::new(e.to_string()))
}

#[server(prefix = "/sfn")]
pub async fn get_recents() -> Result<Vec<RecentEntry>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    replay_core::recents::list_recents(&state.storage)
        .map_err(|e| ServerFnError::new(e.to_string()))
}

#[server(prefix = "/sfn")]
pub async fn add_favorite(
    system: String,
    rom_path: String,
    grouped: bool,
) -> Result<Favorite, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    replay_core::favorites::add_favorite(&state.storage, &system, &rom_path, grouped)
        .map_err(|e| ServerFnError::new(e.to_string()))
}

#[server(prefix = "/sfn")]
pub async fn remove_favorite(
    filename: String,
    subfolder: Option<String>,
) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    replay_core::favorites::remove_favorite(&state.storage, &filename, subfolder.as_deref())
        .map_err(|e| ServerFnError::new(e.to_string()))
}

#[server(prefix = "/sfn")]
pub async fn group_favorites() -> Result<usize, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    replay_core::favorites::group_by_system(&state.storage)
        .map_err(|e| ServerFnError::new(e.to_string()))
}

#[server(prefix = "/sfn")]
pub async fn flatten_favorites() -> Result<usize, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    replay_core::favorites::flatten_favorites(&state.storage)
        .map_err(|e| ServerFnError::new(e.to_string()))
}

#[server(prefix = "/sfn")]
pub async fn get_system_favorites(system: String) -> Result<Vec<String>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let favs = replay_core::favorites::list_favorites_for_system(&state.storage, &system)
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    Ok(favs.into_iter().map(|f| f.game.rom_filename).collect())
}

#[server(prefix = "/sfn")]
pub async fn delete_rom(relative_path: String) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    replay_core::roms::delete_rom(&state.storage, &relative_path)
        .map_err(|e| ServerFnError::new(e.to_string()))
}

#[server(prefix = "/sfn")]
pub async fn rename_rom(relative_path: String, new_filename: String) -> Result<String, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let new_path = replay_core::roms::rename_rom(&state.storage, &relative_path, &new_filename)
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
    let all_roms = replay_core::roms::list_roms(&state.storage, &system)
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    let rom = all_roms
        .into_iter()
        .find(|r| r.game.rom_filename == filename)
        .ok_or_else(|| ServerFnError::new(format!("ROM not found: {filename}")))?;

    let is_favorite = replay_core::favorites::is_favorite(&state.storage, &system, &filename);

    let arcade_info = replay_core::arcade_db::lookup_arcade_game(
        filename.strip_suffix(".zip").unwrap_or(&filename),
    )
    .map(|info| {
        let rotation = match info.rotation {
            replay_core::arcade_db::Rotation::Horizontal => "Horizontal",
            replay_core::arcade_db::Rotation::Vertical => "Vertical",
            replay_core::arcade_db::Rotation::Unknown => "Unknown",
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
