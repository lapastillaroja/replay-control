#[cfg(feature = "ssr")]
use replay_control_core_server::library_db::LibraryDb;
#[cfg(feature = "ssr")]
use replay_control_core_server::user_data_db::UserDataDb;

mod boxart;
mod favorites;
mod images;
mod manuals;
mod metadata;
pub(crate) mod recommendations;
mod related;
mod roms;
mod search;
mod settings;
mod system;
mod thumbnails;
mod videos;

pub use boxart::*;
pub use favorites::*;
pub use images::*;
pub use manuals::*;
pub use metadata::*;
pub use recommendations::*;
pub use related::*;
pub use roms::*;
pub use search::*;
pub use settings::*;
pub use system::*;
pub use thumbnails::*;
pub use videos::*;

use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use server_fn::ServerFnError;

pub use replay_control_core::favorites::OrganizeCriteria;

pub const PAGE_SIZE: usize = 100;

/// Extract region preference strings from AppState for SQL queries.
#[cfg(feature = "ssr")]
pub(crate) fn region_strings(state: &crate::api::AppState) -> (String, String) {
    let pref = state.region_preference();
    let sec = state.region_preference_secondary();
    (
        pref.as_str().to_string(),
        sec.map(|r| r.as_str()).unwrap_or("").to_string(),
    )
}

/// Lightweight entry for ROM list views (game browser, search results).
///
/// Unlike `GameInfo` (which carries full metadata for the detail page),
/// this contains only the fields needed to render a ROM row in a list.
/// `display_name` is always resolved (never None) so the UI never needs
/// to fall back to the filename.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RomListEntry {
    pub system: String,
    pub rom_filename: String,
    pub rom_path: String,
    /// Always resolved — either from arcade_db, game_db, or filename derivation.
    pub display_name: String,
    pub size_bytes: u64,
    pub is_m3u: bool,
    #[serde(default)]
    pub is_favorite: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub box_art_url: Option<String>,
    /// Arcade driver emulation status (Working/Imperfect/Preliminary/Unknown).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub driver_status: Option<String>,
    /// Game rating (0.0-5.0 scale), from library DB.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<f32>,
    /// Maximum number of players, from game_db or arcade_db.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub players: Option<u8>,
    /// Genre string for display (e.g., "Platform", "Beat 'em Up").
    #[serde(default)]
    pub genre: String,
}

/// Batch-resolve favorites for a set of game references.
/// Box art comes from the DB `box_art_url` field (set by enrichment pipeline).
/// If NULL, no art is available — show placeholder.
/// Returns a `HashMap` keyed by `(system, rom_filename)` with `(box_art_url, is_favorite)`.
#[cfg(feature = "ssr")]
pub(crate) async fn enrich_box_art_and_favorites(
    state: &crate::api::AppState,
    entries: &[(String, String, Option<String>)], // (system, rom_filename, existing_box_art_url)
) -> std::collections::HashMap<(String, String), (Option<String>, bool)> {
    use std::collections::{HashMap, HashSet};

    let storage = state.storage();

    // Collect distinct systems for batch operations.
    let distinct_systems: HashSet<&str> = entries.iter().map(|(sys, _, _)| sys.as_str()).collect();

    // Fan out per-system favorite lookups concurrently. Each lookup may hit
    // the shared favorites cache on the fast path, or spawn a blocking fs walk
    // on miss — running them in parallel keeps the request off the critical
    // sequential path when multiple systems are involved (homepage, search).
    let fav_sets: HashMap<String, HashSet<String>> = {
        let mut set = tokio::task::JoinSet::new();
        for sys in distinct_systems {
            let state = state.clone();
            let storage = storage.clone();
            let sys = sys.to_string();
            set.spawn(async move {
                let favs = state.cache.get_favorites_set(&storage, &sys).await;
                (sys, favs)
            });
        }
        let mut out = HashMap::new();
        while let Some(res) = set.join_next().await {
            match res {
                Ok((sys, favs)) => {
                    out.insert(sys, favs);
                }
                Err(e) => {
                    tracing::warn!("favorites fan-out task failed: {e}");
                }
            }
        }
        out
    };

    // Build result map.
    let mut result = HashMap::with_capacity(entries.len());
    for (system, rom_filename, existing_url) in entries {
        let is_favorite = fav_sets
            .get(system.as_str())
            .is_some_and(|set| set.contains(rom_filename));
        result.insert(
            (system.clone(), rom_filename.clone()),
            (existing_url.clone(), is_favorite),
        );
    }
    result
}

/// Convert a slice of `GameEntry` rows into enriched `RomListEntry` values.
///
/// Handles multi-system results (e.g., developer page, global search) by
/// batching favorites per distinct system. Box art comes from the DB
/// `box_art_url` field (set by enrichment pipeline). All other metadata
/// fields (genre, rating, players, driver_status) are already populated
/// in `GameEntry` from the `game_library` table.
#[cfg(feature = "ssr")]
pub(crate) async fn enrich_game_entries(
    state: &crate::api::AppState,
    entries: Vec<replay_control_core_server::library_db::GameEntry>,
) -> Vec<RomListEntry> {
    // Build input tuples for the shared enrichment function.
    let input: Vec<(String, String, Option<String>)> = entries
        .iter()
        .map(|e| {
            (
                e.system.clone(),
                e.rom_filename.clone(),
                e.box_art_url.clone(),
            )
        })
        .collect();

    let enriched = enrich_box_art_and_favorites(state, &input).await;

    // Single pass: convert GameEntry -> RomListEntry with enrichment.
    entries
        .into_iter()
        .map(|entry| {
            let key = (entry.system.clone(), entry.rom_filename.clone());
            let (box_art_url, is_favorite) = enriched.get(&key).cloned().unwrap_or((None, false));
            RomListEntry {
                display_name: entry
                    .display_name
                    .unwrap_or_else(|| entry.rom_filename.clone()),
                system: entry.system,
                rom_filename: entry.rom_filename,
                rom_path: entry.rom_path,
                size_bytes: entry.size_bytes,
                is_m3u: entry.is_m3u,
                is_favorite,
                box_art_url,
                driver_status: entry.driver_status,
                rating: entry.rating,
                players: entry.players,
                genre: entry.genre_group,
            }
        })
        .collect()
}

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
    /// Full release date (ISO 8601 partial/full: `"YYYY"`, `"YYYY-MM"`, or `"YYYY-MM-DD"`).
    /// None if unknown. Populated from the region-resolved `release_date` column.
    pub release_date: Option<String>,
    /// Precision of `release_date`.
    pub release_precision: Option<replay_control_core::DatePrecision>,
    /// Which region the resolver picked for this date (UI hint; e.g., `"usa" | "japan"`).
    pub release_region_used: Option<String>,
    pub genre: String,
    pub developer: String,
    pub players: u8,
    /// Whether this game supports cooperative play.
    pub cooperative: bool,

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

    // --- Image URLs (relative paths under /media/) ---
    pub box_art_url: Option<String>,
    pub screenshot_url: Option<String>,
    pub title_url: Option<String>,
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
pub use replay_control_core::favorites::Favorite;
pub use replay_control_core::game_ref::GameRef;
pub use replay_control_core::recents::RecentEntry;
pub use replay_control_core::roms::{RomEntry, SystemSummary};

/// Strip the Leptos "error running server function: " prefix from server errors.
pub fn format_error(e: server_fn::ServerFnError) -> String {
    let msg = e.to_string();
    msg.strip_prefix("error running server function: ")
        .unwrap_or(&msg)
        .to_string()
}

/// Build full game metadata for the detail page from a `GameEntry` (DB source of truth).
///
/// Unlike the old `resolve_game_info()` which re-derived metadata from baked-in
/// databases, this reads directly from the enriched `GameEntry` stored in
/// `game_library`. Arcade-only fields (rotation, parent_rom, arcade_category)
/// are supplemented from `arcade_db` (cheap static lookups).
#[cfg(feature = "ssr")]
pub(crate) async fn build_game_detail(
    state: &crate::api::AppState,
    entry: &replay_control_core_server::library_db::GameEntry,
) -> GameInfo {
    use replay_control_core::systems;
    use replay_control_core_server::arcade_db;

    let sys_info = systems::find_system(&entry.system);
    let system_display = sys_info
        .map(|s| s.display_name.to_string())
        .unwrap_or_else(|| entry.system.clone());
    let is_arcade = systems::is_arcade_system(&entry.system);

    // Arcade-only fields from static arcade_db lookup.
    let (rotation, parent_rom, arcade_category, arcade_display) = if is_arcade {
        let stem = replay_control_core::title_utils::filename_stem(&entry.rom_filename);
        match arcade_db::lookup_arcade_game(&entry.system, stem).await {
            Some(info) => {
                let rotation = match info.rotation {
                    arcade_db::Rotation::Horizontal => "Horizontal",
                    arcade_db::Rotation::Vertical => "Vertical",
                    arcade_db::Rotation::Unknown => "Unknown",
                };
                let parent = if info.is_clone {
                    Some(info.parent.to_string())
                } else {
                    None
                };
                let category = if info.category.is_empty() {
                    None
                } else {
                    Some(info.category.to_string())
                };
                (
                    Some(rotation.to_string()),
                    parent,
                    category,
                    Some(info.display_name),
                )
            }
            None => (None, None, None, None),
        }
    } else {
        (None, None, None, None)
    };

    let mut info = GameInfo {
        system: entry.system.clone(),
        system_display,
        rom_filename: entry.rom_filename.clone(),
        rom_path: entry.rom_path.clone(),
        display_name: entry
            .display_name
            .clone()
            .unwrap_or_else(|| entry.rom_filename.clone()),
        year: entry
            .release_year()
            .map(|y| y.to_string())
            .unwrap_or_default(),
        release_date: entry.release_date.clone(),
        release_precision: entry.release_precision,
        release_region_used: entry.release_region_used.clone(),
        genre: entry
            .genre
            .clone()
            .unwrap_or_else(|| entry.genre_group.clone()),
        developer: entry.developer.clone(),
        players: entry.players.unwrap_or(0),
        cooperative: entry.cooperative,
        rotation,
        driver_status: entry.driver_status.clone(),
        is_clone: if is_arcade {
            Some(entry.is_clone)
        } else {
            None
        },
        parent_rom,
        arcade_category,
        region: if entry.region.is_empty() {
            None
        } else {
            Some(entry.region.clone())
        },
        description: None,
        rating: entry.rating,
        publisher: None,
        box_art_url: entry.box_art_url.clone(),
        screenshot_url: None,
        title_url: None,
    };

    // Enrich with detail-only fields not stored in GameEntry.
    enrich_detail_fields(state, &mut info, arcade_display.as_deref()).await;

    info
}

/// Enrich a `GameInfo` with detail-page-only fields not stored in `GameEntry`.
///
/// Fetches from `game_metadata` table: description, publisher, screenshot/title paths.
/// Handles box art override from `user_data_db` and filesystem fallback for
/// screenshots/title screens.
#[cfg(feature = "ssr")]
async fn enrich_detail_fields(
    state: &crate::api::AppState,
    info: &mut GameInfo,
    arcade_display: Option<&str>,
) {
    // Check user_data_db for box art override FIRST (highest priority).
    if let Some(override_path) = state
        .user_data_pool
        .read({
            let system = info.system.clone();
            let rom_filename = info.rom_filename.clone();
            move |conn| {
                UserDataDb::get_override(conn, &system, &rom_filename)
                    .ok()
                    .flatten()
            }
        })
        .await
        .flatten()
    {
        let full = state
            .storage()
            .rc_dir()
            .join("media")
            .join(&info.system)
            .join(&override_path);
        if is_valid_image(full).await {
            info.box_art_url = Some(format!("/media/{}/{override_path}", info.system));
        }
    }

    // Fetch detail-only fields (description, publisher) from
    // `game_description` — denormalized at enrichment time so the request
    // path stays on the library pool (no cross-pool acquire).
    let system = info.system.clone();
    let rom_filename = info.rom_filename.clone();
    let arcade_display_owned = arcade_display.map(str::to_owned);
    if let Some(Ok(Some(meta))) = state
        .library_pool
        .read(move |conn| LibraryDb::lookup_description(conn, &system, &rom_filename))
        .await
    {
        info.description = meta.description;
        info.publisher = meta.publisher;
    }

    // Filesystem fallback for screenshots and title screens.
    if info.screenshot_url.is_none() || info.title_url.is_none() {
        let media_base = state.storage().rc_dir().join("media").join(&info.system);
        if info.screenshot_url.is_none()
            && let Some(path) = resolve_image_on_disk(
                arcade_display_owned.clone(),
                media_base.clone(),
                replay_control_core_server::thumbnails::ThumbnailKind::Snap.media_dir(),
                info.rom_filename.clone(),
            )
            .await
        {
            info.screenshot_url = Some(format!("/media/{}/{path}", info.system));
        }
        if info.title_url.is_none()
            && let Some(path) = resolve_image_on_disk(
                arcade_display_owned,
                media_base,
                replay_control_core_server::thumbnails::ThumbnailKind::Title.media_dir(),
                info.rom_filename.clone(),
            )
            .await
        {
            info.title_url = Some(format!("/media/{}/{path}", info.system));
        }
    }
}

/// Resolve a box art URL for a ROM, checking library DB first, then filesystem.
#[cfg(feature = "ssr")]
pub(crate) async fn resolve_box_art_url(
    state: &crate::api::AppState,
    system: &str,
    rom_filename: &str,
    arcade_display: Option<&str>,
) -> Option<String> {
    let media_base = state.storage().rc_dir().join("media").join(system);

    // 0. Check user_data_db for box art override (highest priority).
    if let Some(override_path) = state
        .user_data_pool
        .read({
            let system = system.to_string();
            let rom_filename = rom_filename.to_string();
            move |conn| {
                UserDataDb::get_override(conn, &system, &rom_filename)
                    .ok()
                    .flatten()
            }
        })
        .await
        .flatten()
    {
        let full = state
            .storage()
            .rc_dir()
            .join("media")
            .join(system)
            .join(&override_path);
        if is_valid_image(full).await {
            return Some(format!("/media/{system}/{override_path}"));
        }
    }

    // 1. (Legacy game_metadata.box_art_path lookup removed — the new schema
    //    stores box_art_url on game_library and the caller passes that through
    //    `info.box_art_url`. Filesystem fallback below handles the rest.)
    //    Filesystem fallback — resolve_image_on_disk handles arcade name translation.
    resolve_image_on_disk(
        arcade_display.map(str::to_owned),
        media_base,
        replay_control_core_server::thumbnails::ThumbnailKind::Boxart.media_dir(),
        rom_filename.to_string(),
    )
    .await
    .map(|path| format!("/media/{system}/{path}"))
}

// Re-export image utilities from core for use in this crate.
#[cfg(feature = "ssr")]
pub(crate) use replay_control_core_server::thumbnails::{is_valid_image, resolve_image_on_disk};
