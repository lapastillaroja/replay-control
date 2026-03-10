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
                    if let Some(ref path) = meta.box_art_path {
                        info.box_art_url = Some(format!("/media/{}/{path}", info.system));
                    }
                    if let Some(ref path) = meta.screenshot_path {
                        info.screenshot_url = Some(format!("/media/{}/{path}", info.system));
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
        let storage = state.storage();
        let media_base = storage
            .root
            .join(replay_control_core::metadata_db::RC_DIR)
            .join("media")
            .join(&info.system);

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
fn resolve_box_art_url(
    state: &crate::api::AppState,
    system: &str,
    rom_filename: &str,
) -> Option<String> {
    // 1. Try metadata DB
    if let Some(guard) = state.metadata_db() {
        if let Some(db) = guard.as_ref() {
            if let Ok(Some(meta)) = db.lookup(system, rom_filename) {
                if let Some(ref path) = meta.box_art_path {
                    return Some(format!("/media/{system}/{path}"));
                }
            }
        }
    }
    // 2. Filesystem fallback
    let storage = state.storage();
    let media_base = storage
        .root
        .join(replay_control_core::metadata_db::RC_DIR)
        .join("media")
        .join(system);
    find_image_on_disk(&media_base, "boxart", rom_filename)
        .map(|path| format!("/media/{system}/{path}"))
}

/// Try to find an image file on disk for a ROM, checking exact and fuzzy name matches.
/// Skips broken files (< 200 bytes) that are git fake-symlink artifacts.
#[cfg(feature = "ssr")]
fn find_image_on_disk(
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
    let thumb_name = thumbnail_filename(stem);

    // 1. Exact match
    let exact = kind_dir.join(format!("{thumb_name}.png"));
    if exact.exists() && is_valid_image(&exact) {
        return Some(format!("{kind}/{thumb_name}.png"));
    }

    // 2. Fuzzy match: strip parenthesized tags and special separators, then
    //    compare base titles. Use thumbnail_filename() on ROM stem so that
    //    special chars (&, *, etc.) are normalized to _ just like the image files.
    let base_title = |s: &str| -> String {
        // Handle tilde dual-names: "Name1 ~ Name2" → use Name2 (usually the intl name)
        let s = s.rsplit_once(" ~ ").map(|(_, r)| r).unwrap_or(s);
        s.find(" (")
            .or_else(|| s.find(" ["))
            .map(|i| &s[..i])
            .unwrap_or(s)
            .trim()
            .to_lowercase()
    };

    let rom_base = base_title(&thumb_name);

    if let Ok(entries) = std::fs::read_dir(&kind_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if let Some(img_stem) = name.strip_suffix(".png") {
                if base_title(img_stem) == rom_base && is_valid_image(&entry.path()) {
                    return Some(format!("{kind}/{name}"));
                }
            }
        }
    }

    None
}

/// Quick check that a file is likely a real image (not a git fake-symlink text file).
#[cfg(feature = "ssr")]
fn is_valid_image(path: &std::path::Path) -> bool {
    // Real PNGs are almost always > 200 bytes.
    path.metadata().map(|m| m.len() >= 200).unwrap_or(false)
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

/// A recent entry enriched with box art URL for the home page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentWithArt {
    #[serde(flatten)]
    pub entry: RecentEntry,
    pub box_art_url: Option<String>,
}

/// A favorite enriched with box art URL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FavoriteWithArt {
    #[serde(flatten)]
    pub fav: Favorite,
    pub box_art_url: Option<String>,
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
    /// Whether this system is an arcade system (for clone filter visibility).
    #[serde(default)]
    pub is_arcade: bool,
}

/// Compute a relevance score for a ROM against a search query.
/// Higher = more relevant. Returns 0 for no match.
#[cfg(feature = "ssr")]
fn search_score(query: &str, display_name: &str, filename: &str) -> u32 {
    let display_lower = display_name.to_lowercase();
    let filename_lower = filename.to_lowercase();

    // Base score from match type
    let base = if display_lower == *query {
        10_000 // exact match on display name
    } else if display_lower.starts_with(query) {
        5_000 // display name starts with query
    } else if display_lower
        .split_whitespace()
        .any(|w| w.starts_with(query))
    {
        2_000 // a word in display name starts with query
    } else if display_lower.contains(query) {
        1_000 // display name contains query
    } else if filename_lower.contains(query) {
        500 // only filename contains query
    } else {
        return 0;
    };

    // Shorter names are more likely the original game
    let length_bonus: u32 = if display_name.len() < 40 { 100 } else { 0 };

    // Tier penalty: deprioritize non-original ROMs
    let (tier, region) = replay_control_core::rom_tags::classify(filename);
    let tier_penalty = match tier {
        replay_control_core::rom_tags::RomTier::Original => 0,
        replay_control_core::rom_tags::RomTier::Revision => 5,
        replay_control_core::rom_tags::RomTier::RegionVariant => 10,
        replay_control_core::rom_tags::RomTier::Translation => 50,
        replay_control_core::rom_tags::RomTier::Unlicensed => 60,
        replay_control_core::rom_tags::RomTier::Homebrew => 100,
        replay_control_core::rom_tags::RomTier::Hack => 200,
        replay_control_core::rom_tags::RomTier::PreRelease => 250,
        replay_control_core::rom_tags::RomTier::Pirate => 300,
    };

    // Region bonus: prefer common regions
    let region_bonus = match region {
        replay_control_core::rom_tags::RegionPriority::World => 20,
        replay_control_core::rom_tags::RegionPriority::Usa => 15,
        replay_control_core::rom_tags::RegionPriority::Europe => 10,
        replay_control_core::rom_tags::RegionPriority::Japan => 5,
        replay_control_core::rom_tags::RegionPriority::Other => 0,
        replay_control_core::rom_tags::RegionPriority::Unknown => 0,
    };

    (base + length_bonus + region_bonus).saturating_sub(tier_penalty)
}

#[server(prefix = "/sfn")]
pub async fn get_roms_page(
    system: String,
    offset: usize,
    limit: usize,
    search: String,
    #[server(default)]
    hide_hacks: bool,
    #[server(default)]
    hide_translations: bool,
    #[server(default)]
    hide_betas: bool,
    #[server(default)]
    hide_clones: bool,
    #[server(default)]
    genre: String,
) -> Result<RomPage, ServerFnError> {
    use replay_control_core::rom_tags;
    use replay_control_core::systems::{self as sys_db, SystemCategory};

    let state = expect_context::<crate::api::AppState>();
    let sys_info = sys_db::find_system(&system);
    let system_display = sys_info
        .map(|s| s.display_name.to_string())
        .unwrap_or_else(|| system.clone());
    let is_arcade = sys_info.is_some_and(|s| s.category == SystemCategory::Arcade);
    let storage = state.storage();
    let all_roms = state
        .cache
        .get_roms(&storage, &system)
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    // Apply tier-based, clone, and genre filters before search scoring.
    let pre_filtered: Vec<RomEntry> = all_roms
        .into_iter()
        .filter(|r| {
            if hide_hacks || hide_translations || hide_betas {
                let (tier, _) = rom_tags::classify(&r.game.rom_filename);
                if hide_hacks && tier == rom_tags::RomTier::Hack {
                    return false;
                }
                if hide_translations && tier == rom_tags::RomTier::Translation {
                    return false;
                }
                if hide_betas && tier == rom_tags::RomTier::PreRelease {
                    return false;
                }
            }
            if hide_clones && is_arcade {
                use replay_control_core::arcade_db;
                let stem = r
                    .game
                    .rom_filename
                    .strip_suffix(".zip")
                    .unwrap_or(&r.game.rom_filename);
                if let Some(info) = arcade_db::lookup_arcade_game(stem) {
                    if info.is_clone {
                        return false;
                    }
                }
            }
            true
        })
        .filter(|r| {
            if genre.is_empty() {
                return true;
            }
            let rom_genre = lookup_genre(&system, &r.game.rom_filename);
            rom_genre.eq_ignore_ascii_case(&genre)
        })
        .collect();

    let filtered: Vec<RomEntry> = if search.is_empty() {
        pre_filtered
    } else {
        let q = search.to_lowercase();
        let mut scored: Vec<(u32, RomEntry)> = pre_filtered
            .into_iter()
            .filter_map(|r| {
                let display = r
                    .game
                    .display_name
                    .as_deref()
                    .unwrap_or(&r.game.rom_filename);
                let score = search_score(&q, display, &r.game.rom_filename);
                if score > 0 { Some((score, r)) } else { None }
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0));
        scored.into_iter().map(|(_, r)| r).collect()
    };

    let total = filtered.len();
    let mut roms: Vec<RomEntry> = filtered.into_iter().skip(offset).take(limit).collect();
    let has_more = offset + roms.len() < total;

    replay_control_core::roms::mark_favorites(&storage, &system, &mut roms);

    // Populate box art URLs.
    for rom in &mut roms {
        rom.box_art_url = resolve_box_art_url(&state, &system, &rom.game.rom_filename);
    }

    Ok(RomPage {
        roms,
        total,
        has_more,
        system_display,
        is_arcade,
    })
}

#[server(prefix = "/sfn")]
pub async fn get_favorites() -> Result<Vec<FavoriteWithArt>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let favs = replay_control_core::favorites::list_favorites(&state.storage())
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    Ok(favs
        .into_iter()
        .map(|fav| {
            let box_art_url = resolve_box_art_url(&state, &fav.game.system, &fav.game.rom_filename);
            FavoriteWithArt { fav, box_art_url }
        })
        .collect())
}

#[server(prefix = "/sfn")]
pub async fn get_recents() -> Result<Vec<RecentWithArt>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();
    let entries = replay_control_core::recents::list_recents(&storage)
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    let enriched = entries
        .into_iter()
        .map(|entry| {
            let box_art_url =
                resolve_box_art_url(&state, &entry.game.system, &entry.game.rom_filename);
            RecentWithArt {
                entry,
                box_art_url,
            }
        })
        .collect();

    Ok(enriched)
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
    replay_control_core::favorites::remove_favorite(
        &state.storage(),
        &filename,
        subfolder.as_deref(),
    )
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
pub async fn get_system_favorites(system: String) -> Result<Vec<FavoriteWithArt>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let favs = replay_control_core::favorites::list_favorites_for_system(&state.storage(), &system)
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    Ok(favs
        .into_iter()
        .map(|fav| {
            let box_art_url = resolve_box_art_url(&state, &fav.game.system, &fav.game.rom_filename);
            FavoriteWithArt { fav, box_art_url }
        })
        .collect())
}

#[server(prefix = "/sfn")]
pub async fn delete_rom(relative_path: String) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    replay_control_core::roms::delete_rom(&state.storage(), &relative_path)
        .map_err(|e| ServerFnError::new(e.to_string()))
}

#[server(prefix = "/sfn")]
pub async fn rename_rom(
    relative_path: String,
    new_filename: String,
) -> Result<String, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let new_path =
        replay_control_core::roms::rename_rom(&state.storage(), &relative_path, &new_filename)
            .map_err(|e| ServerFnError::new(e.to_string()))?;
    Ok(new_path.display().to_string())
}

#[server(prefix = "/sfn")]
pub async fn launch_game(rom_path: String) -> Result<String, ServerFnError> {
    if !is_replayos() {
        return Ok("Launch simulated (not on RePlayOS)".into());
    }

    let state = expect_context::<crate::api::AppState>();
    replay_control_core::launch::launch_game(&state.storage(), &rom_path)
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    Ok("Game launching".into())
}

/// A user-taken screenshot URL for the game detail page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenshotUrl {
    pub url: String,
    pub timestamp: Option<i64>,
}

/// Detailed ROM info including unified game metadata and favorite status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RomDetail {
    pub game: GameInfo,
    pub size_bytes: u64,
    pub is_m3u: bool,
    pub is_favorite: bool,
    pub user_screenshots: Vec<ScreenshotUrl>,
}

#[server(prefix = "/sfn")]
pub async fn get_rom_detail(system: String, filename: String) -> Result<RomDetail, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();
    let all_roms = state
        .cache
        .get_roms(&storage, &system)
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    let rom = all_roms
        .into_iter()
        .find(|r| r.game.rom_filename == filename)
        .ok_or_else(|| ServerFnError::new(format!("ROM not found: {filename}")))?;

    let is_favorite = replay_control_core::favorites::is_favorite(&storage, &system, &filename);

    let game = resolve_game_info(&system, &filename, &rom.game.rom_path);

    let user_screenshots = replay_control_core::screenshots::find_screenshots_for_rom(
        &storage, &system, &filename,
    )
    .into_iter()
    .map(|s| ScreenshotUrl {
        url: format!("/captures/{}/{}", system, s.filename),
        timestamp: s.timestamp,
    })
    .collect();

    Ok(RomDetail {
        game,
        size_bytes: rom.size_bytes,
        is_m3u: rom.is_m3u,
        is_favorite,
        user_screenshots,
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
    let needs_ratings =
        primary == OrganizeCriteria::Rating || secondary == Some(OrganizeCriteria::Rating);
    let ratings = if needs_ratings {
        replay_control_core::metadata_db::MetadataDb::open(&state.storage().root)
            .ok()
            .and_then(|db| db.all_ratings().ok())
    } else {
        None
    };
    let result = replay_control_core::favorites::organize_favorites(
        &state.storage(),
        primary,
        secondary,
        keep_originals,
        ratings.as_ref(),
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

#[cfg(not(feature = "ssr"))]
pub use crate::types::{ImportProgress, ImportState, ImportStats, MetadataStats, SystemCoverage};
#[cfg(feature = "ssr")]
pub use replay_control_core::metadata_db::{
    ImportProgress, ImportState, ImportStats, MetadataStats, SystemCoverage,
};

/// Get metadata coverage stats.
#[server(prefix = "/sfn")]
pub async fn get_metadata_stats() -> Result<MetadataStats, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let guard = state
        .metadata_db()
        .ok_or_else(|| ServerFnError::new("Cannot open metadata DB"))?;
    let db = guard
        .as_ref()
        .ok_or_else(|| ServerFnError::new("Metadata DB not available"))?;
    db.stats().map_err(|e| ServerFnError::new(e.to_string()))
}

/// Start a background metadata import from a LaunchBox Metadata.xml file.
/// Returns immediately; poll `get_import_progress` for status.
#[server(prefix = "/sfn")]
pub async fn import_launchbox_metadata(xml_path: String) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let path = std::path::PathBuf::from(&xml_path);

    if !path.exists() {
        return Err(ServerFnError::new(format!("File not found: {xml_path}")));
    }

    if !state.start_import(path) {
        return Err(ServerFnError::new("An import is already running"));
    }

    tracing::info!("Started LaunchBox import from {xml_path}");
    Ok(())
}

/// Get current import progress (None if no import has been started).
#[server(prefix = "/sfn")]
pub async fn get_import_progress() -> Result<Option<ImportProgress>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let guard = state
        .import_progress
        .read()
        .expect("import_progress lock poisoned");
    Ok(guard.clone())
}

/// Get per-system metadata coverage stats.
#[server(prefix = "/sfn")]
pub async fn get_system_coverage() -> Result<Vec<SystemCoverage>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();

    // Get metadata entries per system from DB.
    let entries_per_system = {
        let guard = state
            .metadata_db()
            .ok_or_else(|| ServerFnError::new("Cannot open metadata DB"))?;
        let db = guard
            .as_ref()
            .ok_or_else(|| ServerFnError::new("Metadata DB not available"))?;
        db.entries_per_system()
            .map_err(|e| ServerFnError::new(e.to_string()))?
    };

    // Get total games per system from ROM cache.
    let storage = state.storage();
    let systems = state.cache.get_systems(&storage);

    let mut meta_map: std::collections::HashMap<String, usize> =
        entries_per_system.into_iter().collect();

    let mut coverage: Vec<SystemCoverage> = systems
        .into_iter()
        .filter(|s| s.game_count > 0)
        .map(|s| {
            let with_metadata = meta_map.remove(&s.folder_name).unwrap_or(0);
            SystemCoverage {
                system: s.folder_name,
                display_name: s.display_name,
                total_games: s.game_count,
                with_metadata,
            }
        })
        .collect();

    coverage.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    Ok(coverage)
}

/// Clear all cached metadata.
#[server(prefix = "/sfn")]
pub async fn clear_metadata() -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let guard = state
        .metadata_db()
        .ok_or_else(|| ServerFnError::new("Cannot open metadata DB"))?;
    let db = guard
        .as_ref()
        .ok_or_else(|| ServerFnError::new("Metadata DB not available"))?;
    db.clear().map_err(|e| ServerFnError::new(e.to_string()))
}

/// Clear metadata DB and trigger re-import from Metadata.xml.
/// The import runs in the background; poll `get_import_progress` for status.
#[server(prefix = "/sfn")]
pub async fn regenerate_metadata() -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    state.regenerate_metadata().map_err(ServerFnError::new)
}

/// Download LaunchBox metadata from the internet, extract, and import.
/// The entire process runs in the background; poll `get_import_progress` for status.
#[server(prefix = "/sfn")]
pub async fn download_metadata() -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    if !state.start_metadata_download() {
        return Err(ServerFnError::new(
            "A metadata operation is already running",
        ));
    }
    Ok(())
}

// ── Image management ──────────────────────────────────────────────

/// Image import progress.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageImportProgress {
    pub state: ImageImportState,
    pub system: String,
    pub system_display: String,
    pub processed: usize,
    pub total: usize,
    pub boxart_copied: usize,
    pub snap_copied: usize,
    pub elapsed_secs: u64,
    pub error: Option<String>,
    /// For "download all": which system number we're on (1-based).
    pub current_system: usize,
    /// For "download all": total number of systems to process.
    pub total_systems: usize,
}

/// Image import state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImageImportState {
    Cloning,
    Copying,
    Complete,
    Failed,
    Cancelled,
}

/// Image coverage per system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageCoverage {
    pub system: String,
    pub display_name: String,
    pub total_games: usize,
    pub with_boxart: usize,
    pub with_snap: usize,
    pub has_repo: bool,
}

/// Start downloading and importing images for a system.
#[server(prefix = "/sfn")]
pub async fn import_system_images(system: String) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    if !state.start_image_import(system.clone()) {
        return Err(ServerFnError::new("An image import is already running"));
    }
    Ok(())
}

/// Start downloading images for all supported systems sequentially.
#[server(prefix = "/sfn")]
pub async fn import_all_images() -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    if !state.start_all_images_import() {
        return Err(ServerFnError::new("An image import is already running"));
    }
    Ok(())
}

/// Cancel the current image import.
#[server(prefix = "/sfn")]
pub async fn cancel_image_import() -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    state
        .image_import_cancel
        .store(true, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

/// Get current image import progress.
#[server(prefix = "/sfn")]
pub async fn get_image_import_progress() -> Result<Option<ImageImportProgress>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let guard = state
        .image_import_progress
        .read()
        .expect("image_import_progress lock poisoned");
    Ok(guard.clone())
}

/// Get image coverage per system.
#[server(prefix = "/sfn")]
pub async fn get_image_coverage() -> Result<Vec<ImageCoverage>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();

    let images_per_system = {
        let guard = state
            .metadata_db()
            .ok_or_else(|| ServerFnError::new("Cannot open metadata DB"))?;
        let db = guard
            .as_ref()
            .ok_or_else(|| ServerFnError::new("Metadata DB not available"))?;
        db.images_per_system()
            .map_err(|e| ServerFnError::new(e.to_string()))?
    };

    let storage = state.storage();
    let systems = state.cache.get_systems(&storage);

    let mut img_map: std::collections::HashMap<String, (usize, usize)> = images_per_system
        .into_iter()
        .map(|(s, b, sn)| (s, (b, sn)))
        .collect();

    let mut coverage: Vec<ImageCoverage> = systems
        .into_iter()
        .filter(|s| s.game_count > 0)
        .map(|s| {
            let (with_boxart, with_snap) = img_map.remove(&s.folder_name).unwrap_or((0, 0));
            let has_repo =
                replay_control_core::thumbnails::thumbnail_repo_names(&s.folder_name).is_some();
            ImageCoverage {
                system: s.folder_name,
                display_name: s.display_name,
                total_games: s.game_count,
                with_boxart,
                with_snap,
                has_repo,
            }
        })
        .collect();

    coverage.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    Ok(coverage)
}

/// Get image stats.
#[server(prefix = "/sfn")]
pub async fn get_image_stats() -> Result<(usize, usize, u64), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let (with_boxart, with_snap) = {
        let guard = state
            .metadata_db()
            .ok_or_else(|| ServerFnError::new("Cannot open metadata DB"))?;
        let db = guard
            .as_ref()
            .ok_or_else(|| ServerFnError::new("Metadata DB not available"))?;
        db.image_stats()
            .map_err(|e| ServerFnError::new(e.to_string()))?
    };
    let storage = state.storage();
    let media_size = replay_control_core::thumbnails::media_dir_size(&storage.root);
    Ok((with_boxart, with_snap, media_size))
}

/// Read system logs from journalctl.
#[server(prefix = "/sfn")]
pub async fn get_system_logs(source: String, lines: usize) -> Result<String, ServerFnError> {
    let lines = lines.min(500);
    let mut cmd = std::process::Command::new("journalctl");
    cmd.args(["--no-pager", "--lines", &lines.to_string(), "--reverse"]);

    match source.as_str() {
        "replay-companion" => {
            cmd.args(["-u", "replay-companion"]);
        }
        "replay" => {
            cmd.args(["-u", "replay"]);
        }
        _ => {} // "all" — no unit filter
    }

    let output = cmd
        .output()
        .map_err(|e| ServerFnError::new(format!("Failed to read logs: {e}")))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        // journalctl may not exist on dev machines
        Ok("journalctl not available or no logs found.".to_string())
    }
}

/// Clear all images.
#[server(prefix = "/sfn")]
pub async fn clear_images() -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();
    replay_control_core::thumbnails::clear_media(&storage.root)
        .map_err(|e| ServerFnError::new(e.to_string()))
}

// ── Global Search ────────────────────────────────────────────────

/// A single result in global search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalSearchResult {
    pub rom_filename: String,
    pub display_name: String,
    pub system: String,
    pub genre: String,
    pub is_favorite: bool,
    pub box_art_url: Option<String>,
}

/// A group of search results for a single system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemSearchGroup {
    pub system: String,
    pub system_display: String,
    pub total_matches: usize,
    pub top_results: Vec<GlobalSearchResult>,
}

/// Aggregated global search results across all systems.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalSearchResults {
    pub groups: Vec<SystemSearchGroup>,
    pub total_results: usize,
    pub total_systems: usize,
}

/// Look up the normalized genre for a ROM on a given system.
#[cfg(feature = "ssr")]
fn lookup_genre(system: &str, rom_filename: &str) -> String {
    use replay_control_core::arcade_db;
    use replay_control_core::game_db;
    use replay_control_core::systems::{self, SystemCategory};

    let is_arcade = systems::find_system(system)
        .is_some_and(|s| s.category == SystemCategory::Arcade);

    if is_arcade {
        let stem = rom_filename.strip_suffix(".zip").unwrap_or(rom_filename);
        arcade_db::lookup_arcade_game(stem)
            .map(|info| info.normalized_genre.to_string())
            .unwrap_or_default()
    } else {
        let stem = rom_filename
            .rfind('.')
            .map(|i| &rom_filename[..i])
            .unwrap_or(rom_filename);
        let entry = game_db::lookup_game(system, stem);
        let game = entry.map(|e| e.game).or_else(|| {
            let normalized = game_db::normalize_filename(stem);
            game_db::lookup_by_normalized_title(system, &normalized)
        });
        game.map(|g| g.normalized_genre.to_string())
            .unwrap_or_default()
    }
}

#[server(prefix = "/sfn")]
pub async fn global_search(
    query: String,
    hide_hacks: bool,
    hide_translations: bool,
    hide_betas: bool,
    hide_clones: bool,
    genre: String,
    per_system_limit: usize,
) -> Result<GlobalSearchResults, ServerFnError> {
    use replay_control_core::rom_tags;
    use replay_control_core::systems::{self as sys_db, SystemCategory};

    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();
    let systems = state.cache.get_systems(&storage);
    let q = query.to_lowercase();
    let per_system_limit = if per_system_limit == 0 { 3 } else { per_system_limit };

    let mut groups: Vec<SystemSearchGroup> = Vec::new();
    let mut total_results = 0usize;

    for sys in &systems {
        if sys.game_count == 0 {
            continue;
        }

        let is_arcade = sys_db::find_system(&sys.folder_name)
            .is_some_and(|s| s.category == SystemCategory::Arcade);

        let all_roms = match state.cache.get_roms(&storage, &sys.folder_name) {
            Ok(roms) => roms,
            Err(_) => continue,
        };

        let mut scored: Vec<(u32, RomEntry)> = all_roms
            .into_iter()
            .filter(|r| {
                // Apply tier-based filters (hacks, translations, betas/protos).
                if hide_hacks || hide_translations || hide_betas {
                    let (tier, _) = rom_tags::classify(&r.game.rom_filename);
                    if hide_hacks && tier == rom_tags::RomTier::Hack {
                        return false;
                    }
                    if hide_translations && tier == rom_tags::RomTier::Translation {
                        return false;
                    }
                    if hide_betas && tier == rom_tags::RomTier::PreRelease {
                        return false;
                    }
                }
                // Apply clone filter (arcade only).
                if hide_clones && is_arcade {
                    use replay_control_core::arcade_db;
                    let stem = r.game.rom_filename.strip_suffix(".zip")
                        .unwrap_or(&r.game.rom_filename);
                    if let Some(info) = arcade_db::lookup_arcade_game(stem) {
                        if info.is_clone {
                            return false;
                        }
                    }
                }
                true
            })
            .filter(|r| {
                // Apply genre filter.
                if genre.is_empty() {
                    return true;
                }
                let rom_genre = lookup_genre(&sys.folder_name, &r.game.rom_filename);
                rom_genre.eq_ignore_ascii_case(&genre)
            })
            .filter_map(|r| {
                if q.is_empty() {
                    // No query: if genre is set, include all matching; otherwise skip.
                    if !genre.is_empty() {
                        // Assign a default score based on display name length.
                        let display = r
                            .game
                            .display_name
                            .as_deref()
                            .unwrap_or(&r.game.rom_filename);
                        let score = 1000u32.saturating_sub(display.len() as u32);
                        Some((score, r))
                    } else {
                        None
                    }
                } else {
                    let display = r
                        .game
                        .display_name
                        .as_deref()
                        .unwrap_or(&r.game.rom_filename);
                    let score = search_score(&q, display, &r.game.rom_filename);
                    if score > 0 { Some((score, r)) } else { None }
                }
            })
            .collect();

        if scored.is_empty() {
            continue;
        }

        scored.sort_by(|a, b| b.0.cmp(&a.0));
        let match_count = scored.len();
        total_results += match_count;

        // Mark favorites for the top results.
        let mut top_roms: Vec<RomEntry> = scored
            .into_iter()
            .take(per_system_limit)
            .map(|(_, r)| r)
            .collect();

        replay_control_core::roms::mark_favorites(&storage, &sys.folder_name, &mut top_roms);

        // Populate box art URLs.
        let media_base = storage
            .root
            .join(replay_control_core::metadata_db::RC_DIR)
            .join("media")
            .join(&sys.folder_name);

        let top_results: Vec<GlobalSearchResult> = top_roms
            .into_iter()
            .map(|mut rom| {
                rom.box_art_url =
                    find_image_on_disk(&media_base, "boxart", &rom.game.rom_filename)
                        .map(|path| format!("/media/{}/{path}", sys.folder_name));
                let genre_str = lookup_genre(&sys.folder_name, &rom.game.rom_filename);
                GlobalSearchResult {
                    display_name: rom
                        .game
                        .display_name
                        .unwrap_or_else(|| rom.game.rom_filename.clone()),
                    rom_filename: rom.game.rom_filename,
                    system: sys.folder_name.clone(),
                    genre: genre_str,
                    is_favorite: rom.is_favorite,
                    box_art_url: rom.box_art_url,
                }
            })
            .collect();

        groups.push(SystemSearchGroup {
            system: sys.folder_name.clone(),
            system_display: sys.display_name.clone(),
            total_matches: match_count,
            top_results,
        });
    }

    // Sort systems by match count descending.
    groups.sort_by(|a, b| b.total_matches.cmp(&a.total_matches));
    let total_systems = groups.len();

    Ok(GlobalSearchResults {
        groups,
        total_results,
        total_systems,
    })
}

#[server(prefix = "/sfn")]
pub async fn get_all_genres() -> Result<Vec<String>, ServerFnError> {
    use std::collections::BTreeSet;

    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();
    let systems = state.cache.get_systems(&storage);
    let mut genres = BTreeSet::new();

    for sys in &systems {
        if sys.game_count == 0 {
            continue;
        }
        let roms = match state.cache.get_roms(&storage, &sys.folder_name) {
            Ok(roms) => roms,
            Err(_) => continue,
        };
        for rom in &roms {
            let g = lookup_genre(&sys.folder_name, &rom.game.rom_filename);
            if !g.is_empty() {
                genres.insert(g);
            }
        }
    }

    Ok(genres.into_iter().collect())
}

/// Get genres available for a specific system.
#[server(prefix = "/sfn")]
pub async fn get_system_genres(system: String) -> Result<Vec<String>, ServerFnError> {
    use std::collections::BTreeSet;

    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();
    let roms = state
        .cache
        .get_roms(&storage, &system)
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    let mut genres = BTreeSet::new();
    for rom in &roms {
        let g = lookup_genre(&system, &rom.game.rom_filename);
        if !g.is_empty() {
            genres.insert(g);
        }
    }

    Ok(genres.into_iter().collect())
}

// ── Random Game ─────────────────────────────────────────────────

/// Pick a random game across all systems.
/// Weighted by system game count so larger collections get proportionally more picks.
/// Returns (system_folder_name, rom_filename).
#[server(prefix = "/sfn")]
pub async fn random_game() -> Result<(String, String), ServerFnError> {
    use rand::Rng;

    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();
    let systems = state.cache.get_systems(&storage);

    // Build a weighted list: (system_folder, game_count).
    let weighted: Vec<(String, usize)> = systems
        .iter()
        .filter(|s| s.game_count > 0)
        .map(|s| (s.folder_name.clone(), s.game_count))
        .collect();

    if weighted.is_empty() {
        return Err(ServerFnError::new("No games available"));
    }

    let total: usize = weighted.iter().map(|(_, c)| c).sum();
    let mut rng = rand::rng();
    let pick = rng.random_range(0..total);

    let mut cumulative = 0;
    let mut chosen_system = &weighted[0].0;
    for (sys, count) in &weighted {
        cumulative += count;
        if pick < cumulative {
            chosen_system = sys;
            break;
        }
    }

    let roms = state
        .cache
        .get_roms(&storage, chosen_system)
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    if roms.is_empty() {
        return Err(ServerFnError::new("No ROMs in selected system"));
    }

    let idx = rng.random_range(0..roms.len());
    let rom = &roms[idx];
    Ok((chosen_system.clone(), rom.game.rom_filename.clone()))
}

// ── Game Videos ──────────────────────────────────────────────────

#[cfg(not(feature = "ssr"))]
pub use crate::types::VideoEntry;
#[cfg(feature = "ssr")]
pub use replay_control_core::videos::VideoEntry;

/// A video recommendation from Piped search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoRecommendation {
    pub url: String,
    pub title: String,
    pub thumbnail_url: Option<String>,
    pub duration_text: Option<String>,
    pub channel: Option<String>,
}

/// Get saved videos for a game.
#[server(prefix = "/sfn")]
pub async fn get_game_videos(
    system: String,
    rom_filename: String,
) -> Result<Vec<VideoEntry>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();
    let game_key = format!("{system}/{rom_filename}");
    Ok(replay_control_core::videos::get_videos(
        &storage.root,
        &game_key,
    ))
}

/// Add a video to a game (from manual paste or recommendation pin).
#[server(prefix = "/sfn")]
pub async fn add_game_video(
    system: String,
    rom_filename: String,
    url: String,
    title: Option<String>,
    from_recommendation: bool,
    tag: Option<String>,
) -> Result<VideoEntry, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();
    let game_key = format!("{system}/{rom_filename}");

    let parsed =
        replay_control_core::video_url::parse_video_url(&url).map_err(ServerFnError::new)?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let entry = VideoEntry {
        id: format!("{}-{}", parsed.platform, parsed.video_id),
        url: parsed.canonical_url,
        platform: parsed.platform.as_str().to_string(),
        video_id: parsed.video_id,
        title,
        added_at: now,
        from_recommendation,
        tag,
    };

    replay_control_core::videos::add_video(&storage.root, &game_key, entry.clone())
        .map_err(ServerFnError::new)?;

    Ok(entry)
}

/// Remove a saved video from a game.
#[server(prefix = "/sfn")]
pub async fn remove_game_video(
    system: String,
    rom_filename: String,
    video_id: String,
) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();
    let game_key = format!("{system}/{rom_filename}");
    replay_control_core::videos::remove_video(&storage.root, &game_key, &video_id)
        .map_err(ServerFnError::new)
}

/// Search for video recommendations via the Piped API.
#[server(prefix = "/sfn")]
pub async fn search_game_videos(
    system: String,
    display_name: String,
    query_type: String,
) -> Result<Vec<VideoRecommendation>, ServerFnError> {
    // Normalize the title: strip parenthesized tags like "(USA)", "(World 910522)"
    let clean_title = {
        let mut s = display_name.as_str();
        // Repeatedly strip trailing parenthesized/bracketed tags
        loop {
            let trimmed = s.trim();
            if let Some(pos) = trimmed.rfind(" (") {
                if trimmed.ends_with(')') {
                    s = &trimmed[..pos];
                    continue;
                }
            }
            if let Some(pos) = trimmed.rfind(" [") {
                if trimmed.ends_with(']') {
                    s = &trimmed[..pos];
                    continue;
                }
            }
            break;
        }
        s.trim().to_string()
    };

    // Determine system label: arcade systems → "arcade", others → display name
    let system_label = if system.starts_with("arcade_") {
        "arcade".to_string()
    } else {
        replay_control_core::systems::find_system(&system)
            .map(|s| s.display_name.to_string())
            .unwrap_or_else(|| system.clone())
    };

    let query_suffix = match query_type.as_str() {
        "trailer" => "official trailer",
        "gameplay" => "gameplay",
        "1cc" => "1cc one credit clear",
        _ => "",
    };

    let query = format!("{clean_title} {system_label} {query_suffix}");
    let encoded_query = urlencoding::encode(&query);
    tracing::info!("Video search: query=\"{query}\"");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|e| ServerFnError::new(format!("HTTP client error: {e}")))?;

    // Try Piped instances first, then Invidious instances
    let piped_instances = [
        "https://pipedapi.kavin.rocks",
        "https://pipedapi.leptons.xyz",
        "https://pipedapi-libre.kavin.rocks",
    ];
    let invidious_instances = [
        "https://invidious.materialio.us",
        "https://yewtu.be",
        "https://inv.tux.pizza",
    ];

    // Try Piped instances
    for base_url in &piped_instances {
        let api_url =
            format!("{base_url}/search?q={encoded_query}&filter=videos");
        match client.get(&api_url).send().await {
            Ok(resp) => {
                if resp.status().is_success() {
                    match resp.json::<serde_json::Value>().await {
                        Ok(body) => {
                            let items = body
                                .get("items")
                                .and_then(|v| v.as_array())
                                .cloned()
                                .unwrap_or_default();
                            if !items.is_empty() {
                                tracing::info!(
                                    "Video search: Piped {base_url} returned {} results",
                                    items.len()
                                );
                                return Ok(parse_piped_results(&items));
                            }
                            tracing::warn!("Video search: Piped {base_url} returned empty results");
                        }
                        Err(e) => {
                            tracing::warn!("Video search: Piped {base_url} JSON parse error: {e}");
                        }
                    }
                } else {
                    tracing::warn!(
                        "Video search: Piped {base_url} returned status {}",
                        resp.status()
                    );
                }
            }
            Err(e) => {
                tracing::warn!("Video search: Piped {base_url} request failed: {e}");
            }
        }
    }

    // Try Invidious instances
    for base_url in &invidious_instances {
        let api_url =
            format!("{base_url}/api/v1/search?q={encoded_query}&type=video");
        match client.get(&api_url).send().await {
            Ok(resp) => {
                if resp.status().is_success() {
                    match resp.json::<Vec<serde_json::Value>>().await {
                        Ok(items) => {
                            if !items.is_empty() {
                                tracing::info!(
                                    "Video search: Invidious {base_url} returned {} results",
                                    items.len()
                                );
                                return Ok(parse_invidious_results(&items));
                            }
                            tracing::warn!(
                                "Video search: Invidious {base_url} returned empty results"
                            );
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Video search: Invidious {base_url} JSON parse error: {e}"
                            );
                        }
                    }
                } else {
                    tracing::warn!(
                        "Video search: Invidious {base_url} returned status {}",
                        resp.status()
                    );
                }
            }
            Err(e) => {
                tracing::warn!("Video search: Invidious {base_url} request failed: {e}");
            }
        }
    }

    tracing::error!("Video search: all instances failed for query \"{query}\"");
    Err(ServerFnError::new(
        "Video search unavailable. Paste URLs directly.".to_string(),
    ))
}

#[cfg(feature = "ssr")]
fn parse_piped_results(items: &[serde_json::Value]) -> Vec<VideoRecommendation> {
    items
        .iter()
        .filter_map(|item| {
            let url_path = item.get("url")?.as_str()?;
            let full_url = if url_path.starts_with("http") {
                url_path.to_string()
            } else {
                format!("https://www.youtube.com{url_path}")
            };
            let title = item
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Untitled")
                .to_string();
            let thumbnail_url = item
                .get("thumbnail")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let duration_secs = item.get("duration").and_then(|v| v.as_i64());
            let duration_text = duration_secs.map(|secs| {
                let mins = secs / 60;
                let s = secs % 60;
                format!("{mins}:{s:02}")
            });
            let channel = item
                .get("uploaderName")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            Some(VideoRecommendation {
                url: full_url,
                title,
                thumbnail_url,
                duration_text,
                channel,
            })
        })
        .take(10)
        .collect()
}

#[cfg(feature = "ssr")]
fn parse_invidious_results(items: &[serde_json::Value]) -> Vec<VideoRecommendation> {
    items
        .iter()
        .filter_map(|item| {
            let video_id = item.get("videoId")?.as_str()?;
            let full_url = format!("https://www.youtube.com/watch?v={video_id}");
            let title = item
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Untitled")
                .to_string();
            // Use medium-quality thumbnail from YouTube directly
            let thumbnail_url =
                Some(format!("https://i.ytimg.com/vi/{video_id}/mqdefault.jpg"));
            let duration_secs = item.get("lengthSeconds").and_then(|v| v.as_i64());
            let duration_text = duration_secs.map(|secs| {
                let mins = secs / 60;
                let s = secs % 60;
                format!("{mins}:{s:02}")
            });
            let channel = item
                .get("author")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            Some(VideoRecommendation {
                url: full_url,
                title,
                thumbnail_url,
                duration_text,
                channel,
            })
        })
        .take(10)
        .collect()
}
