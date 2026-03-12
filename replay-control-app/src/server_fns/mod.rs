mod favorites;
mod images;
mod metadata;
mod recommendations;
mod roms;
mod search;
mod settings;
mod system;
mod thumbnails;
mod videos;

pub use favorites::*;
pub use images::*;
pub use metadata::*;
pub use recommendations::*;
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
pub(crate) fn resolve_game_info(system: &str, rom_filename: &str, rom_path: &str) -> GameInfo {
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
                    box_art_url: None,
                    screenshot_url: None,
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
            box_art_url: None,
            screenshot_url: None,
        }
    };

    // Enrich with external metadata from local cache.
    enrich_from_metadata_cache(&mut info);

    info
}

/// Look up cached external metadata and enrich the GameInfo.
#[cfg(feature = "ssr")]
pub(crate) fn enrich_from_metadata_cache(info: &mut GameInfo) {
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
                    if let Some(ref path) = meta.box_art_path {
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
    }

    // Filesystem fallback: if no image URLs from DB, check if images exist on disk.
    // This handles the case where images were downloaded but the DB was cleared/regenerated.
    if info.box_art_url.is_none() || info.screenshot_url.is_none() {
        let media_base = state.storage().rc_dir().join("media").join(&info.system);

        if info.box_art_url.is_none() {
            if let Some(path) = find_image_on_disk(&media_base, "boxart", &info.rom_filename) {
                info.box_art_url = Some(format!("/media/{}/{path}", info.system));
            }
        }
        if info.screenshot_url.is_none() {
            if let Some(path) = find_image_on_disk(&media_base, "snap", &info.rom_filename) {
                info.screenshot_url = Some(format!("/media/{}/{path}", info.system));
            }
        }
    }
}

/// Resolve a box art URL for a ROM, checking metadata DB first, then filesystem.
#[cfg(feature = "ssr")]
pub(crate) fn resolve_box_art_url(
    state: &crate::api::AppState,
    system: &str,
    rom_filename: &str,
) -> Option<String> {
    let media_base = state.storage().rc_dir().join("media").join(system);

    // 1. Try metadata DB — but validate the file on disk (catches git fake-symlink artifacts).
    //    If the DB path is a fake symlink, try resolving it before falling back to disk scan.
    if let Some(guard) = state.metadata_db() {
        if let Some(db) = guard.as_ref() {
            if let Ok(Some(meta)) = db.lookup(system, rom_filename) {
                if let Some(ref path) = meta.box_art_path {
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
                            .unwrap_or("boxart");
                        return Some(format!("/media/{system}/{kind}/{resolved}"));
                    }
                }
            }
        }
    }
    // 2. Filesystem fallback (find_image_on_disk already validates)
    find_image_on_disk(&media_base, "boxart", rom_filename)
        .map(|path| format!("/media/{system}/{path}"))
}

/// Try to find an image file on disk for a ROM, checking exact and fuzzy name matches.
/// When a file fails `is_valid_image()` (< 200 bytes), tries to resolve it as a git
/// fake-symlink artifact — reads the text content, checks if the referenced file exists
/// in the same directory and is a valid image. This only triggers for the rare small
/// files (fake symlinks), so the extra I/O (reading ~48 bytes + one stat) is negligible.
#[cfg(feature = "ssr")]
pub(crate) fn find_image_on_disk(
    media_base: &std::path::Path,
    kind: &str,
    rom_filename: &str,
) -> Option<String> {
    use replay_control_core::thumbnails::thumbnail_filename;

    let kind_dir = media_base.join(kind);
    if !kind_dir.exists() {
        return None;
    }

    let stem = rom_filename
        .rfind('.')
        .map(|i| &rom_filename[..i])
        .unwrap_or(rom_filename);
    // Strip known prefixes (e.g., "N64DD - " from 64DD games).
    let stem = stem.strip_prefix("N64DD - ").unwrap_or(stem);
    let thumb_name = thumbnail_filename(stem);

    // 1. Exact match
    let exact = kind_dir.join(format!("{thumb_name}.png"));
    if exact.exists() {
        if is_valid_image(&exact) {
            return Some(format!("{kind}/{thumb_name}.png"));
        }
        // File exists but is too small — try resolving as a fake symlink.
        if let Some(resolved) = try_resolve_fake_symlink(&exact, &kind_dir) {
            return Some(format!("{kind}/{resolved}"));
        }
    }

    // 2. Fuzzy match: strip parenthesized tags and special separators, then
    //    compare base titles. Use thumbnail_filename() on ROM stem so that
    //    special chars (&, *, etc.) are normalized to _ just like the image files.
    let base_title = |s: &str| -> String {
        // Handle tilde dual-names: "Name1 ~ Name2" -> use Name2 (usually the intl name)
        let s = s.rsplit_once(" ~ ").map(|(_, r)| r).unwrap_or(s);
        s.find(" (")
            .or_else(|| s.find(" ["))
            .map(|i| &s[..i])
            .unwrap_or(s)
            .trim()
            .to_lowercase()
    };

    let rom_base = base_title(&thumb_name);
    // Version-stripped key for TOSEC ROMs (e.g., "Sonic Adventure 2 v1.008" -> "sonic adventure 2")
    let rom_base_no_version = replay_control_core::thumbnails::strip_version(&rom_base);
    let has_version = rom_base_no_version.len() < rom_base.len();

    if let Ok(entries) = std::fs::read_dir(&kind_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if let Some(img_stem) = name.strip_suffix(".png") {
                let img_base = base_title(img_stem);
                if img_base == rom_base {
                    let path = entry.path();
                    if is_valid_image(&path) {
                        return Some(format!("{kind}/{name}"));
                    }
                    if let Some(resolved) = try_resolve_fake_symlink(&path, &kind_dir) {
                        return Some(format!("{kind}/{resolved}"));
                    }
                }
                // 3. Version-stripped match: "Virtua Tennis 2 v1.009" matches "Virtua Tennis 2"
                if has_version && img_base == rom_base_no_version {
                    let path = entry.path();
                    if is_valid_image(&path) {
                        return Some(format!("{kind}/{name}"));
                    }
                    if let Some(resolved) = try_resolve_fake_symlink(&path, &kind_dir) {
                        return Some(format!("{kind}/{resolved}"));
                    }
                }
            }
        }
    }

    None
}

/// Quick check that a file is likely a real image (not a git fake-symlink text file).
#[cfg(feature = "ssr")]
pub(crate) fn is_valid_image(path: &std::path::Path) -> bool {
    // Real PNGs are almost always > 200 bytes.
    path.metadata().map(|m| m.len() >= 200).unwrap_or(false)
}

/// Try to resolve a small file as a git fake-symlink artifact.
/// Reads its text content (a relative filename), checks if that file exists in
/// `parent_dir` and passes `is_valid_image()`. Returns the target filename on
/// success, `None` otherwise. Only called when `is_valid_image()` already failed,
/// so the extra I/O cost (read ~48 bytes of text + one stat) is negligible.
#[cfg(feature = "ssr")]
pub(crate) fn try_resolve_fake_symlink(
    path: &std::path::Path,
    parent_dir: &std::path::Path,
) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let target_name = std::str::from_utf8(&bytes).ok()?.trim();
    if target_name.is_empty() || !target_name.ends_with(".png") {
        return None;
    }
    let target_path = parent_dir.join(target_name);
    if target_path.exists() && is_valid_image(&target_path) {
        Some(target_name.to_string())
    } else {
        None
    }
}
