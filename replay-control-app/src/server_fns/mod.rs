#[cfg(feature = "ssr")]
use replay_control_core::metadata_db::MetadataDb;
#[cfg(feature = "ssr")]
use replay_control_core::user_data_db::UserDataDb;

mod boxart;
mod favorites;
mod images;
mod manuals;
mod metadata;
mod recommendations;
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

#[cfg(not(feature = "ssr"))]
pub use crate::types::OrganizeCriteria;
#[cfg(feature = "ssr")]
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
    /// Game rating (0.0-5.0 scale), from metadata DB.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<f32>,
    /// Maximum number of players, from game_db or arcade_db.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub players: Option<u8>,
    /// Genre string for display (e.g., "Platform", "Beat 'em Up").
    #[serde(default)]
    pub genre: String,
}

/// Batch-resolve box art URLs and favorites for a set of game references.
/// Uses DB `box_art_url` when present, falls back to `ImageIndex` for misses.
/// Returns a `HashMap` keyed by `(system, rom_filename)` with `(resolved_box_art_url, is_favorite)`.
#[cfg(feature = "ssr")]
pub(crate) async fn enrich_box_art_and_favorites(
    state: &crate::api::AppState,
    entries: &[(String, String, Option<String>)], // (system, rom_filename, existing_box_art_url)
) -> std::collections::HashMap<(String, String), (Option<String>, bool)> {
    use std::collections::{HashMap, HashSet};

    let storage = state.storage();

    // Collect distinct systems for batch operations.
    let distinct_systems: HashSet<&str> = entries.iter().map(|(sys, _, _)| sys.as_str()).collect();

    // Only build image indexes for systems that have entries with missing box_art_url.
    let systems_needing_index: HashSet<&str> = entries
        .iter()
        .filter(|(_, _, url)| url.is_none())
        .map(|(sys, _, _)| sys.as_str())
        .collect();

    let mut image_indexes: HashMap<String, std::sync::Arc<crate::api::cache::ImageIndex>> =
        HashMap::new();
    for sys in &systems_needing_index {
        let index = state.cache.cached_image_index(state, sys).await;
        image_indexes.insert(sys.to_string(), index);
    }

    // Batch-load favorites per system.
    let fav_sets: HashMap<String, HashSet<String>> = distinct_systems
        .into_iter()
        .map(|sys| {
            let set = state.cache.get_favorites_set(&storage, sys);
            (sys.to_string(), set)
        })
        .collect();

    // Build result map.
    let mut result = HashMap::with_capacity(entries.len());
    for (system, rom_filename, existing_url) in entries {
        let box_art_url = existing_url.clone().or_else(|| {
            image_indexes.get(system.as_str()).and_then(|index| {
                state
                    .cache
                    .resolve_box_art(state, index, system, rom_filename)
            })
        });
        let is_favorite = fav_sets
            .get(system.as_str())
            .is_some_and(|set| set.contains(rom_filename));
        result.insert(
            (system.clone(), rom_filename.clone()),
            (box_art_url, is_favorite),
        );
    }
    result
}

/// Convert a slice of `GameEntry` rows into enriched `RomListEntry` values.
///
/// Handles multi-system results (e.g., developer page, global search) by
/// batching image index loads and favorites per distinct system. Box art is
/// resolved from the cached image index. All other metadata fields (genre,
/// rating, players, driver_status) are already populated in `GameEntry`
/// from the `game_library` table.
#[cfg(feature = "ssr")]
pub(crate) async fn enrich_game_entries(
    state: &crate::api::AppState,
    entries: Vec<replay_control_core::metadata_db::GameEntry>,
) -> Vec<RomListEntry> {
    // Build input tuples for the shared enrichment function.
    let input: Vec<(String, String, Option<String>)> = entries
        .iter()
        .map(|e| (e.system.clone(), e.rom_filename.clone(), e.box_art_url.clone()))
        .collect();

    let enriched = enrich_box_art_and_favorites(state, &input).await;

    // Single pass: convert GameEntry -> RomListEntry with enrichment.
    entries
        .into_iter()
        .map(|entry| {
            let key = (entry.system.clone(), entry.rom_filename.clone());
            let (box_art_url, is_favorite) = enriched
                .get(&key)
                .cloned()
                .unwrap_or((None, false));
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

/// Strip the Leptos "error running server function: " prefix from server errors.
pub fn format_error(e: server_fn::ServerFnError) -> String {
    let msg = e.to_string();
    msg.strip_prefix("error running server function: ")
        .unwrap_or(&msg)
        .to_string()
}

/// Resolve full game metadata for any system.
/// This is the single function that bridges arcade_db and game_db.
#[cfg(feature = "ssr")]
pub(crate) async fn resolve_game_info(
    system: &str,
    rom_filename: &str,
    rom_path: &str,
) -> GameInfo {
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
                    genre: if info.category.is_empty() {
                        info.normalized_genre.to_string()
                    } else {
                        info.category.to_string()
                    },
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
                    box_art_url: None,
                    screenshot_url: None,
                    title_url: None,
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
                box_art_url: None,
                screenshot_url: None,
                title_url: None,
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
            // No DB match — derive a clean display name from the filename.
            // Strip extension and parenthesized/bracketed tags for the base name,
            // then let display_name_with_tags re-append the useful tags.
            let stem = rom_filename
                .rfind('.')
                .map(|i| &rom_filename[..i])
                .unwrap_or(rom_filename);
            let base = stem
                .find(" (")
                .or_else(|| stem.find(" ["))
                .map(|i| stem[..i].trim())
                .unwrap_or(stem);
            let name = if base.is_empty() { stem } else { base };
            rom_tags::display_name_with_tags(name, rom_filename)
        };

        // For metadata, also try normalized title fallback
        let game_meta = game.or_else(|| {
            let normalized = game_db::normalize_filename(stem);
            game_db::lookup_by_normalized_title(system, &normalized)
        });

        // Extract TOSEC metadata as fallback for year/developer when game_db has none.
        let tosec = rom_tags::extract_tosec_metadata(rom_filename);

        let db_year = game_meta
            .map(|g| {
                if g.year > 0 {
                    g.year.to_string()
                } else {
                    String::new()
                }
            })
            .unwrap_or_default();
        let year = if db_year.is_empty() {
            tosec.year.map(|y| y.to_string()).unwrap_or_default()
        } else {
            db_year
        };

        let db_developer = game_meta
            .map(|g| g.developer.to_string())
            .unwrap_or_default();
        let developer = if db_developer.is_empty() {
            tosec
                .publisher
                .as_deref()
                .map(replay_control_core::developer::normalize_developer)
                .unwrap_or_default()
        } else {
            db_developer
        };

        GameInfo {
            system: system.to_string(),
            system_display,
            rom_filename: rom_filename.to_string(),
            rom_path: rom_path.to_string(),
            display_name,
            year,
            genre: game_meta
                .map(|g| {
                    if g.genre.is_empty() {
                        g.normalized_genre
                    } else {
                        g.genre
                    }
                    .to_string()
                })
                .unwrap_or_default(),
            developer,
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
            box_art_url: None,
            screenshot_url: None,
            title_url: None,
        }
    };

    // Enrich with external metadata from local cache.
    enrich_from_metadata_cache(&mut info).await;

    info
}

/// Look up cached external metadata and enrich the GameInfo.
#[cfg(feature = "ssr")]
pub(crate) async fn enrich_from_metadata_cache(info: &mut GameInfo) {
    let state = leptos::prelude::expect_context::<crate::api::AppState>();

    // Clone strings for use in `move` closures (must be Send + 'static).
    let system = info.system.clone();
    let rom_filename = info.rom_filename.clone();

    // Check user_data_db for box art override FIRST (highest priority).
    if let Some(override_path) = state
        .user_data_pool
        .read({
            let system = system.clone();
            let rom_filename = rom_filename.clone();
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
        if is_valid_image(&full) {
            info.box_art_url = Some(format!("/media/{}/{override_path}", info.system));
        }
    }

    if let Some(lookup_result) = state
        .metadata_pool
        .read(move |conn| MetadataDb::lookup(conn, &system, &rom_filename))
        .await
    {
        match lookup_result {
            Ok(Some(meta)) => {
                info.description = meta.description;
                info.rating = meta.rating.map(|r| r as f32);
                if meta.publisher.is_some() {
                    info.publisher = meta.publisher;
                }
                if info.developer.is_empty() && meta.developer.is_some() {
                    info.developer = meta.developer.unwrap_or_default();
                }
                // Use LaunchBox release_year as fallback when baked-in DB has none.
                if info.year.is_empty()
                    && let Some(year) = meta.release_year
                {
                    info.year = year.to_string();
                }
                // Use LaunchBox genre as fallback when baked-in DB has none.
                if info.genre.is_empty() && meta.genre.is_some() {
                    info.genre = meta.genre.unwrap_or_default();
                }
                // Only set box_art_url from metadata if no override was set above.
                if info.box_art_url.is_none()
                    && let Some(ref path) = meta.box_art_path
                {
                    let full = state
                        .storage()
                        .rc_dir()
                        .join("media")
                        .join(&info.system)
                        .join(path);
                    if is_valid_image(&full) {
                        info.box_art_url = Some(format!("/media/{}/{path}", info.system));
                    }
                }
                if let Some(ref path) = meta.screenshot_path {
                    let full = state
                        .storage()
                        .rc_dir()
                        .join("media")
                        .join(&info.system)
                        .join(path);
                    if is_valid_image(&full) {
                        info.screenshot_url = Some(format!("/media/{}/{path}", info.system));
                    }
                }
                if let Some(ref path) = meta.title_path {
                    let full = state
                        .storage()
                        .rc_dir()
                        .join("media")
                        .join(&info.system)
                        .join(path);
                    if is_valid_image(&full) {
                        info.title_url = Some(format!("/media/{}/{path}", info.system));
                    }
                }
            }
            Ok(None) => {}
            Err(e) => {
                tracing::debug!(
                    "Metadata lookup failed for {}/{}: {e}",
                    info.system,
                    info.rom_filename
                );
            }
        }
    }

    // Filesystem fallback: if no image URLs from DB, check filesystem.
    // For box art, use the unified resolve_box_art() (same path as cards/recommendations)
    // to ensure consistent box art between detail pages and series/recommendation cards.
    // For screenshots and title screens, use resolve_image_on_disk() which handles
    // arcade MAME codename → display name translation automatically.
    if info.box_art_url.is_none() || info.screenshot_url.is_none() || info.title_url.is_none() {
        if info.box_art_url.is_none() {
            let image_index = state.cache.cached_image_index(&state, &info.system).await;
            if let Some(url) =
                state
                    .cache
                    .resolve_box_art(&state, &image_index, &info.system, &info.rom_filename)
            {
                info.box_art_url = Some(url);
            }
        }
        let media_base = state.storage().rc_dir().join("media").join(&info.system);
        if info.screenshot_url.is_none()
            && let Some(path) = resolve_image_on_disk(
                &info.system,
                &media_base,
                replay_control_core::thumbnails::ThumbnailKind::Snap.media_dir(),
                &info.rom_filename,
            )
        {
            info.screenshot_url = Some(format!("/media/{}/{path}", info.system));
        }
        if info.title_url.is_none()
            && let Some(path) = resolve_image_on_disk(
                &info.system,
                &media_base,
                replay_control_core::thumbnails::ThumbnailKind::Title.media_dir(),
                &info.rom_filename,
            )
        {
            info.title_url = Some(format!("/media/{}/{path}", info.system));
        }
    }
}

/// Resolve a box art URL for a ROM, checking metadata DB first, then filesystem.
#[cfg(feature = "ssr")]
pub(crate) async fn resolve_box_art_url(
    state: &crate::api::AppState,
    system: &str,
    rom_filename: &str,
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
        if is_valid_image(&full) {
            return Some(format!("/media/{system}/{override_path}"));
        }
    }

    // 1. Try metadata DB — but validate the file on disk (catches git fake-symlink artifacts).
    //    If the DB path is a fake symlink, try resolving it before falling back to disk scan.
    if let Some(Some(meta)) = state
        .metadata_pool
        .read({
            let system = system.to_string();
            let rom_filename = rom_filename.to_string();
            move |conn| {
                MetadataDb::lookup(conn, &system, &rom_filename)
                    .ok()
                    .flatten()
            }
        })
        .await
        && let Some(ref path) = meta.box_art_path
    {
        let full_path = media_base.join(path);
        if is_valid_image(&full_path) {
            return Some(format!("/media/{system}/{path}"));
        }
        // DB path points to a fake symlink — try resolving it.
        let kind_dir = full_path.parent().unwrap_or(&media_base);
        if let Some(resolved) = try_resolve_fake_symlink(&full_path, kind_dir) {
            let kind = std::path::Path::new(path)
                .parent()
                .and_then(|p| p.to_str())
                .unwrap_or(replay_control_core::thumbnails::ThumbnailKind::Boxart.media_dir());
            return Some(format!("/media/{system}/{kind}/{resolved}"));
        }
    }
    // 2. Filesystem fallback — resolve_image_on_disk handles arcade name translation.
    resolve_image_on_disk(
        system,
        &media_base,
        replay_control_core::thumbnails::ThumbnailKind::Boxart.media_dir(),
        rom_filename,
    )
    .map(|path| format!("/media/{system}/{path}"))
}

// Re-export image utilities from core for use in this crate.
#[cfg(feature = "ssr")]
pub(crate) use replay_control_core::thumbnails::{
    is_valid_image, resolve_image_on_disk, try_resolve_fake_symlink,
};
