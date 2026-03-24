#[cfg(feature = "ssr")]
use super::search::system_player_counts;
use super::*;
#[cfg(feature = "ssr")]
use replay_control_core::metadata_db::MetadataDb;

/// A page of ROM results with total count.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RomPage {
    pub roms: Vec<RomListEntry>,
    pub total: usize,
    pub has_more: bool,
    /// Human-readable system name (e.g., "Arcade (Atomiswave/Naomi)")
    #[serde(default)]
    pub system_display: String,
    /// Whether this system is an arcade system (for clone filter visibility).
    #[serde(default)]
    pub is_arcade: bool,
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
    /// Number of distinct box art variants available (for "Change cover" affordance).
    #[serde(default)]
    pub variant_count: usize,
    /// Whether this ROM is a hack (suppresses "Change cover" affordance).
    #[serde(default)]
    pub is_hack: bool,
    /// Whether this ROM is a special version (FastROM, 60Hz, unlicensed, etc.).
    #[serde(default)]
    pub is_special: bool,
    /// Normalized base_title for cross-variant video sharing.
    #[serde(default)]
    pub base_title: String,
    /// Whether this ROM can be safely renamed.
    #[serde(default = "default_true")]
    pub rename_allowed: bool,
    /// Explanation when rename is not allowed.
    #[serde(default)]
    pub rename_reason: Option<String>,
    /// Multi-disc set info (if part of a disc set without M3U wrapper).
    #[serde(default)]
    pub disc_info: Option<DiscInfoDto>,
}

fn default_true() -> bool {
    true
}

/// Serializable multi-disc info for the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscInfoDto {
    pub disc_number: u32,
    pub total_discs: u32,
    pub siblings: Vec<String>,
}

/// Summary of files in a ROM group (for delete confirmation).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RomFileGroup {
    pub files: Vec<RomFileEntry>,
    pub total_size: u64,
    pub file_count: usize,
}

/// A single file entry in a ROM group summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RomFileEntry {
    pub filename: String,
    pub size_bytes: u64,
    pub kind: String,
}

// clippy::too_many_arguments — Leptos server functions require flat parameter lists
// for serialization; wrapping in a struct is not supported by the #[server] macro.
#[allow(clippy::too_many_arguments)]
#[server(prefix = "/sfn")]
pub async fn get_roms_page(
    system: String,
    offset: usize,
    limit: usize,
    search: String,
    #[server(default)] hide_hacks: bool,
    #[server(default)] hide_translations: bool,
    #[server(default)] hide_betas: bool,
    #[server(default)] hide_clones: bool,
    #[server(default)] genre: String,
    #[server(default)] multiplayer_only: bool,
    #[server(default)] min_rating: Option<f32>,
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
    let region_pref = state.region_preference();
    let region_secondary = state.region_preference_secondary();

    // Fast path: when no filters or search are active, try SQL-level pagination
    // directly from L1 or L2 cache. This avoids loading all ROMs into memory
    // on cold L1 cache (significant for systems with 3000+ ROMs).
    let has_filters = hide_hacks
        || hide_translations
        || hide_betas
        || hide_clones
        || !genre.is_empty()
        || multiplayer_only
        || min_rating.is_some();
    if !has_filters
        && search.is_empty()
        && let Some((mut roms, total)) = state
            .cache
            .get_roms_page_direct(&storage, &system, offset, limit)
            .await
    {
        let has_more = offset + roms.len() < total;

        // Overlay favorites.
        let fav_set = state.cache.get_favorites_set(&storage, &system);
        for rom in &mut roms {
            rom.is_favorite = fav_set.contains(&rom.game.rom_filename);
        }

        // Populate box art URLs.
        let image_index = state.cache.get_image_index(&state, &system).await;
        for rom in &mut roms {
            rom.box_art_url =
                state
                    .cache
                    .resolve_box_art(&state, &image_index, &system, &rom.game.rom_filename);
        }

        // Populate driver status for arcade systems.
        if is_arcade {
            use replay_control_core::arcade_db;
            for rom in &mut roms {
                let stem = rom
                    .game
                    .rom_filename
                    .strip_suffix(".zip")
                    .unwrap_or(&rom.game.rom_filename);
                if let Some(info) = arcade_db::lookup_arcade_game(stem) {
                    let status = match info.status {
                        arcade_db::DriverStatus::Working => "Working",
                        arcade_db::DriverStatus::Imperfect => "Imperfect",
                        arcade_db::DriverStatus::Preliminary => "Preliminary",
                        arcade_db::DriverStatus::Unknown => "Unknown",
                    };
                    rom.driver_status = Some(status.to_string());
                }
            }
        }

        // Populate players.
        {
            let filenames: Vec<&str> = roms.iter().map(|r| r.game.rom_filename.as_str()).collect();
            let players = system_player_counts(&system, &filenames);
            for rom in &mut roms {
                if let Some(&p) = players.get(&rom.game.rom_filename) {
                    rom.players = Some(p);
                }
            }
        }

        // Populate ratings from metadata DB.
        {
            let filenames: Vec<String> = roms.iter().map(|r| r.game.rom_filename.clone()).collect();
            if let Some(Ok(ratings)) = state
                .metadata_pool
                .read({
                    let system = system.clone();
                    move |conn| {
                        let refs: Vec<&str> = filenames.iter().map(|s| s.as_str()).collect();
                        MetadataDb::lookup_ratings(conn, &system, &refs)
                    }
                })
                .await
            {
                for rom in &mut roms {
                    if let Some(&rating) = ratings.get(&rom.game.rom_filename)
                        && rating > 0.0
                    {
                        rom.rating = Some(rating as f32);
                    }
                }
            }
        }

        let list_entries: Vec<RomListEntry> = roms
            .into_iter()
            .map(|rom| RomListEntry {
                display_name: rom
                    .game
                    .display_name
                    .unwrap_or_else(|| rom.game.rom_filename.clone()),
                system: rom.game.system,
                rom_filename: rom.game.rom_filename,
                rom_path: rom.game.rom_path,
                size_bytes: rom.size_bytes,
                is_m3u: rom.is_m3u,
                is_favorite: rom.is_favorite,
                box_art_url: rom.box_art_url,
                driver_status: rom.driver_status,
                rating: rom.rating,
                players: rom.players,
                genre: String::new(),
            })
            .collect();

        return Ok(RomPage {
            roms: list_entries,
            total,
            has_more,
            system_display,
            is_arcade,
        });
    }

    // Full path: load all ROMs and filter/search in memory.
    let all_roms = state
        .cache
        .get_roms(&storage, &system, region_pref, region_secondary)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    // Batch-load genre groups for genre filtering (single DB query).
    let genre_groups: std::collections::HashMap<String, String> = if !genre.is_empty() {
        state
            .cache
            .db_read({
                let system = system.clone();
                move |conn| {
                    MetadataDb::load_system_entries(conn, &system)
                        .map(|entries| {
                            entries
                                .into_iter()
                                .filter(|e| !e.genre_group.is_empty())
                                .map(|e| (e.rom_filename, e.genre_group))
                                .collect()
                        })
                        .unwrap_or_default()
                }
            })
            .await
            .unwrap_or_default()
    } else {
        std::collections::HashMap::new()
    };

    // Batch-load player counts for multiplayer filtering.
    let player_counts = if multiplayer_only {
        let filenames: Vec<&str> = all_roms
            .iter()
            .map(|r| r.game.rom_filename.as_str())
            .collect();
        system_player_counts(&system, &filenames)
    } else {
        std::collections::HashMap::new()
    };

    // Apply tier-based, clone, and genre filters before search scoring.
    let pre_filtered: Vec<&RomEntry> = all_roms
        .iter()
        .filter(|&r| {
            if hide_hacks || hide_translations || hide_betas {
                let (tier, _, _) = rom_tags::classify(&r.game.rom_filename);
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
                if let Some(info) = arcade_db::lookup_arcade_game(stem)
                    && info.is_clone
                {
                    return false;
                }
            }
            true
        })
        .filter(|r| {
            // Apply genre filter using genre_group from game_library.
            if genre.is_empty() {
                return true;
            }
            genre_groups
                .get(&r.game.rom_filename)
                .is_some_and(|gg| gg.eq_ignore_ascii_case(&genre))
        })
        .filter(|r| {
            if !multiplayer_only {
                return true;
            }
            player_counts
                .get(&r.game.rom_filename)
                .is_some_and(|&p| p >= 2)
        })
        .collect();

    // Apply minimum rating filter: batch-load all ratings for the system,
    // then exclude ROMs below the threshold (unrated games are excluded).
    let pre_filtered: Vec<&RomEntry> = if let Some(threshold) = min_rating {
        let ratings = state
            .metadata_pool
            .read({
                let system = system.clone();
                move |conn| MetadataDb::system_ratings(conn, &system).unwrap_or_default()
            })
            .await
            .unwrap_or_default();
        pre_filtered
            .into_iter()
            .filter(|r| {
                ratings
                    .get(&r.game.rom_filename)
                    .is_some_and(|&rating| rating >= threshold as f64)
            })
            .collect()
    } else {
        pre_filtered
    };

    let filtered: Vec<&RomEntry> = if search.is_empty() {
        pre_filtered
    } else {
        let q = search.to_lowercase();
        let mut scored: Vec<(u32, &RomEntry)> = pre_filtered
            .into_iter()
            .filter_map(|r| {
                let display = r
                    .game
                    .display_name
                    .as_deref()
                    .unwrap_or(&r.game.rom_filename);
                let score = search_score(
                    &q,
                    display,
                    &r.game.rom_filename,
                    region_pref,
                    region_secondary,
                );
                if score > 0 { Some((score, r)) } else { None }
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0));
        scored.into_iter().map(|(_, r)| r).collect()
    };

    let total = filtered.len();
    // Clone only the page-sized subset we need to mutate.
    let mut roms: Vec<RomEntry> = filtered
        .into_iter()
        .skip(offset)
        .take(limit)
        .cloned()
        .collect();
    let has_more = offset + roms.len() < total;

    // Use cached favorites set instead of per-request filesystem scan.
    let fav_set = state.cache.get_favorites_set(&storage, &system);
    for rom in &mut roms {
        rom.is_favorite = fav_set.contains(&rom.game.rom_filename);
    }

    // Populate box art URLs using cached per-system image index (single dir read).
    let image_index = state.cache.get_image_index(&state, &system).await;
    for rom in &mut roms {
        rom.box_art_url =
            state
                .cache
                .resolve_box_art(&state, &image_index, &system, &rom.game.rom_filename);
    }

    // Populate driver status for arcade systems.
    if is_arcade {
        use replay_control_core::arcade_db;
        for rom in &mut roms {
            let stem = rom
                .game
                .rom_filename
                .strip_suffix(".zip")
                .unwrap_or(&rom.game.rom_filename);
            if let Some(info) = arcade_db::lookup_arcade_game(stem) {
                let status = match info.status {
                    arcade_db::DriverStatus::Working => "Working",
                    arcade_db::DriverStatus::Imperfect => "Imperfect",
                    arcade_db::DriverStatus::Preliminary => "Preliminary",
                    arcade_db::DriverStatus::Unknown => "Unknown",
                };
                rom.driver_status = Some(status.to_string());
            }
        }
    }

    // Populate players from game_db / arcade_db (batch lookup).
    {
        let filenames: Vec<&str> = roms.iter().map(|r| r.game.rom_filename.as_str()).collect();
        let players = system_player_counts(&system, &filenames);
        for rom in &mut roms {
            if let Some(&p) = players.get(&rom.game.rom_filename) {
                rom.players = Some(p);
            }
        }
    }

    // Populate ratings from metadata DB (batch lookup for efficiency).
    {
        let filenames: Vec<String> = roms.iter().map(|r| r.game.rom_filename.clone()).collect();
        if let Some(Ok(ratings)) = state
            .metadata_pool
            .read({
                let system = system.clone();
                move |conn| {
                    let refs: Vec<&str> = filenames.iter().map(|s| s.as_str()).collect();
                    MetadataDb::lookup_ratings(conn, &system, &refs)
                }
            })
            .await
        {
            for rom in &mut roms {
                if let Some(&rating) = ratings.get(&rom.game.rom_filename)
                    && rating > 0.0
                {
                    rom.rating = Some(rating as f32);
                }
            }
        }
    }

    // Convert RomEntry → RomListEntry with always-resolved display_name.
    let list_entries: Vec<RomListEntry> = roms
        .into_iter()
        .map(|rom| RomListEntry {
            display_name: rom
                .game
                .display_name
                .unwrap_or_else(|| rom.game.rom_filename.clone()),
            system: rom.game.system,
            rom_filename: rom.game.rom_filename,
            rom_path: rom.game.rom_path,
            size_bytes: rom.size_bytes,
            is_m3u: rom.is_m3u,
            is_favorite: rom.is_favorite,
            box_art_url: rom.box_art_url,
            driver_status: rom.driver_status,
            rating: rom.rating,
            players: rom.players,
            genre: String::new(),
        })
        .collect();

    Ok(RomPage {
        roms: list_entries,
        total,
        has_more,
        system_display,
        is_arcade,
    })
}

#[server(prefix = "/sfn", endpoint = "/get_rom_detail")]
pub async fn get_rom_detail(system: String, filename: String) -> Result<RomDetail, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();

    let rom = state
        .cache
        .get_single_rom(&storage, &system, &filename)
        .await
        .ok_or_else(|| ServerFnError::new(format!("ROM not found: {filename}")))?;

    let is_favorite = replay_control_core::favorites::is_favorite(&storage, &system, &filename);

    let game = resolve_game_info(&system, &filename, &rom.game.rom_path).await;

    let user_screenshots =
        replay_control_core::screenshots::find_screenshots_for_rom(&storage, &system, &filename)
            .into_iter()
            .map(|s| ScreenshotUrl {
                url: format!("/captures/{}/{}", system, s.filename),
                timestamp: s.timestamp,
            })
            .collect();

    // Count box art variants (lightweight — only needs the thumbnail index).
    let variant_count = state
        .metadata_pool
        .read({
            let system = system.clone();
            let filename = filename.clone();
            move |conn| {
                replay_control_core::thumbnail_manifest::count_boxart_variants(
                    conn, &system, &filename,
                )
            }
        })
        .await
        .unwrap_or(0);

    let (tier, _, is_special) = replay_control_core::rom_tags::classify(&filename);
    let is_hack = tier == replay_control_core::rom_tags::RomTier::Hack;

    let base_title = replay_control_core::title_utils::base_title(&game.display_name);

    // Determine rename restrictions.
    let (rename_allowed, rename_reason) = replay_control_core::roms::check_rename_allowed(
        &storage,
        &system,
        rom.game.rom_path.trim_start_matches('/'),
    );

    // Detect multi-disc set.
    let disc_info =
        replay_control_core::roms::detect_disc_set(&storage, &system, &filename).map(|di| {
            DiscInfoDto {
                disc_number: di.disc_number,
                total_discs: di.total_discs,
                siblings: di.siblings,
            }
        });

    Ok(RomDetail {
        game,
        size_bytes: rom.size_bytes,
        is_m3u: rom.is_m3u,
        is_favorite,
        user_screenshots,
        variant_count,
        is_hack,
        is_special,
        base_title,
        rename_allowed,
        rename_reason,
        disc_info,
    })
}

/// Reject paths that attempt directory traversal.
#[cfg(feature = "ssr")]
fn validate_path_safe(path: &str) -> Result<(), ServerFnError> {
    if path.contains("..") || path.contains('\\') {
        return Err(ServerFnError::new("Invalid path"));
    }
    Ok(())
}

/// Get the file group for a ROM (for delete confirmation UI).
#[server(prefix = "/sfn")]
pub async fn get_rom_file_group(
    system: String,
    relative_path: String,
) -> Result<RomFileGroup, ServerFnError> {
    validate_path_safe(&relative_path)?;
    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();

    let mut group = replay_control_core::roms::list_rom_group(&storage, &system, &relative_path)
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    // If this ROM is part of a multi-disc set (no M3U), include sibling discs.
    let rom_filename = std::path::Path::new(&relative_path)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();
    if let Some(disc_info) =
        replay_control_core::roms::detect_disc_set(&storage, &system, &rom_filename)
    {
        let system_dir = storage.system_roms_dir(&system);
        for sibling in &disc_info.siblings {
            if *sibling == rom_filename {
                continue; // Already in the group as Primary.
            }
            let sibling_path = system_dir.join(sibling);
            if sibling_path.exists() {
                let size = std::fs::metadata(&sibling_path)
                    .map(|m| m.len())
                    .unwrap_or(0);
                group.push(replay_control_core::roms::GroupedFile {
                    path: sibling_path,
                    size_bytes: size,
                    kind: replay_control_core::roms::FileKind::Disc,
                });
            }
        }
    }

    let total_size: u64 = group.iter().map(|g| g.size_bytes).sum();
    let file_count = group.len();

    let files = group
        .into_iter()
        .map(|g| {
            let filename = g
                .path
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| g.path.display().to_string());
            let kind = match g.kind {
                replay_control_core::roms::FileKind::Primary => "primary",
                replay_control_core::roms::FileKind::Disc => "disc",
                replay_control_core::roms::FileKind::Companion => "companion",
                replay_control_core::roms::FileKind::DataDir => "directory",
            };
            RomFileEntry {
                filename,
                size_bytes: g.size_bytes,
                kind: kind.to_string(),
            }
        })
        .collect();

    Ok(RomFileGroup {
        files,
        total_size,
        file_count,
    })
}

#[server(prefix = "/sfn")]
pub async fn delete_rom(system: String, relative_path: String) -> Result<(), ServerFnError> {
    validate_path_safe(&relative_path)?;
    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();

    // Extract ROM filename for cleanup.
    let rom_filename = std::path::Path::new(&relative_path)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();

    // Check for multi-disc set — include siblings in the deletion.
    let disc_siblings: Vec<String> =
        replay_control_core::roms::detect_disc_set(&storage, &system, &rom_filename)
            .map(|di| di.siblings)
            .unwrap_or_default();

    // Delete the primary ROM group.
    let report = replay_control_core::roms::delete_rom_group(&storage, &system, &relative_path)
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    if !report.errors.is_empty() {
        tracing::warn!("Errors during ROM group delete: {:?}", report.errors);
    }

    // Delete multi-disc siblings (if any).
    for sibling in &disc_siblings {
        if *sibling == rom_filename {
            continue; // Already deleted as part of the primary group.
        }
        let sibling_rel = format!("roms/{system}/{sibling}");
        if let Err(e) = replay_control_core::roms::delete_rom_group(&storage, &system, &sibling_rel)
        {
            tracing::warn!("Failed to delete disc sibling {sibling}: {e}");
        }
    }

    // Phase 3: Orphan data cascade — clean up associated data.
    let filenames_to_clean: Vec<String> = if disc_siblings.is_empty() {
        vec![rom_filename]
    } else {
        disc_siblings
    };

    for fname in &filenames_to_clean {
        delete_rom_cleanup(&state, &storage, &system, fname).await;
    }

    // Invalidate caches.
    state.cache.invalidate_system(system).await;
    state.cache.invalidate_favorites();

    Ok(())
}

/// Find screenshot files matching a ROM filename prefix.
///
/// Returns `(path, suffix)` pairs where suffix starts with `_` or `.`
/// (e.g., `_001.png`, `.png`).
#[cfg(feature = "ssr")]
fn find_matching_screenshots(
    captures_dir: &std::path::Path,
    rom_filename: &str,
) -> Vec<(std::path::PathBuf, String)> {
    let mut matches = Vec::new();
    if captures_dir.exists()
        && let Ok(entries) = std::fs::read_dir(captures_dir)
    {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if let Some(rest) = name.strip_prefix(rom_filename)
                && (rest.starts_with('_') || rest.starts_with('.'))
            {
                matches.push((entry.path(), rest.to_string()));
            }
        }
    }
    matches
}

/// Clean up orphaned data after a ROM deletion.
#[cfg(feature = "ssr")]
async fn delete_rom_cleanup(
    state: &crate::api::AppState,
    storage: &replay_control_core::storage::StorageLocation,
    system: &str,
    rom_filename: &str,
) {
    // 1. Delete matching favorites (search all subfolders).
    let fav_filename = format!("{system}@{rom_filename}.fav");
    let favs_dir = storage.favorites_dir();
    delete_fav_recursive(&favs_dir, &fav_filename);

    // 2. Delete matching screenshots.
    let captures_dir = storage.captures_dir().join(system);
    for (path, _) in find_matching_screenshots(&captures_dir, rom_filename) {
        let _ = std::fs::remove_file(path);
    }

    // 3. Delete user_data.db entries (videos, box art overrides).
    state
        .user_data_pool
        .write({
            let system = system.to_string();
            let rom_filename = rom_filename.to_string();
            move |conn| {
                replay_control_core::user_data_db::UserDataDb::delete_for_rom(
                    conn,
                    &system,
                    &rom_filename,
                );
            }
        })
        .await;

    // 4. Delete metadata.db game_library entry.
    state
        .metadata_pool
        .write({
            let system = system.to_string();
            let rom_filename = rom_filename.to_string();
            move |conn| {
                replay_control_core::metadata_db::MetadataDb::delete_for_rom(
                    conn,
                    &system,
                    &rom_filename,
                );
            }
        })
        .await;
}

/// Recursively search for and delete a .fav file in the favorites directory tree.
#[cfg(feature = "ssr")]
fn delete_fav_recursive(dir: &std::path::Path, fav_filename: &str) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with('_') && !name.starts_with('.') {
                // Check if the file exists directly in this subdir.
                let candidate = path.join(fav_filename);
                if candidate.exists() {
                    let _ = std::fs::remove_file(&candidate);
                }
                delete_fav_recursive(&path, fav_filename);
            }
        } else if entry.file_name().to_string_lossy() == fav_filename {
            let _ = std::fs::remove_file(&path);
        }
    }
}

#[server(prefix = "/sfn")]
pub async fn rename_rom(
    system: String,
    relative_path: String,
    new_filename: String,
) -> Result<String, ServerFnError> {
    validate_path_safe(&relative_path)?;
    validate_path_safe(&new_filename)?;
    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();

    // Extract old filename for cascade.
    let old_filename = std::path::Path::new(&relative_path)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();

    let new_path = replay_control_core::roms::rename_rom(&storage, &relative_path, &new_filename)
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    // Phase 3: Rename cascade — update all associated data.
    rename_rom_cascade(&state, &storage, &system, &old_filename, &new_filename).await;

    // Invalidate caches.
    state.cache.invalidate_system(system).await;
    state.cache.invalidate_favorites();

    Ok(new_path.display().to_string())
}

/// Cascade rename updates to all data sources.
///
/// Errors are logged but do not block the rename — the file rename
/// has already succeeded by the time this is called.
#[cfg(feature = "ssr")]
async fn rename_rom_cascade(
    state: &crate::api::AppState,
    storage: &replay_control_core::storage::StorageLocation,
    system: &str,
    old_filename: &str,
    new_filename: &str,
) {
    // 1. Rename favorites (.fav file rename + content update).
    let old_fav = format!("{system}@{old_filename}.fav");
    let new_fav = format!("{system}@{new_filename}.fav");
    rename_fav_recursive(
        &storage.favorites_dir(),
        &old_fav,
        &new_fav,
        system,
        new_filename,
    );

    // 2. Rename screenshots.
    let captures_dir = storage.captures_dir().join(system);
    for (path, rest) in find_matching_screenshots(&captures_dir, old_filename) {
        let new_name = format!("{new_filename}{rest}");
        let new_path = captures_dir.join(&new_name);
        if let Err(e) = std::fs::rename(&path, &new_path) {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            tracing::warn!("Failed to rename screenshot {name} -> {new_name}: {e}");
        }
    }

    // 3. Update user_data.db (box art overrides, game videos).
    state
        .user_data_pool
        .write({
            let system = system.to_string();
            let old_filename = old_filename.to_string();
            let new_filename = new_filename.to_string();
            move |conn| {
                replay_control_core::user_data_db::UserDataDb::rename_for_rom(
                    conn,
                    &system,
                    &old_filename,
                    &new_filename,
                );
            }
        })
        .await;

    // 4. Update metadata.db game_library entry.
    state
        .metadata_pool
        .write({
            let system = system.to_string();
            let old_filename = old_filename.to_string();
            let new_filename = new_filename.to_string();
            move |conn| {
                replay_control_core::metadata_db::MetadataDb::rename_for_rom(
                    conn,
                    &system,
                    &old_filename,
                    &new_filename,
                );
            }
        })
        .await;
}

/// Recursively find and rename a .fav file, updating its content too.
#[cfg(feature = "ssr")]
fn rename_fav_recursive(
    dir: &std::path::Path,
    old_fav: &str,
    new_fav: &str,
    system: &str,
    new_filename: &str,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with('_') && !name.starts_with('.') {
                rename_fav_recursive(&path, old_fav, new_fav, system, new_filename);
            }
        } else if entry.file_name().to_string_lossy() == old_fav {
            let new_path = path.parent().unwrap_or(dir).join(new_fav);
            // Update the content (rom_path inside the .fav file).
            let new_content = format!("/roms/{system}/{new_filename}");
            if let Err(e) = std::fs::write(&path, &new_content) {
                tracing::warn!("Failed to update .fav content: {e}");
            }
            if let Err(e) = std::fs::rename(&path, &new_path) {
                tracing::warn!("Failed to rename .fav file: {e}");
            }
        }
    }
}

#[server(prefix = "/sfn")]
pub async fn launch_game(rom_path: String) -> Result<String, ServerFnError> {
    if !is_replayos() {
        return Ok("Launch simulated (not on RePlayOS)".into());
    }

    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();

    replay_control_core::launch::launch_game(&storage, &rom_path)
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    // Create a recents entry so the home page reflects the launch immediately.
    // Extract system and rom_filename from the rom_path.
    // rom_path format: "/roms/<system>/<optional_subdirs>/<rom_filename>"
    if let Some((system, rom_filename)) = parse_rom_path(&rom_path) {
        if let Err(e) =
            replay_control_core::recents::add_recent(&storage, &system, &rom_filename, &rom_path)
        {
            tracing::warn!("Failed to create recents entry: {e}");
        }
        state.cache.invalidate_recents();
    }

    Ok("Game launching".into())
}

/// Extract system folder and ROM filename from a rom_path.
///
/// Handles paths like `/roms/sega_smd/Sonic.md` (simple) and
/// `/roms/arcade_dc/Atomiswave/Horizontal Games/00 Clean Romset/ggx15.zip` (nested).
#[cfg(feature = "ssr")]
fn parse_rom_path(rom_path: &str) -> Option<(String, String)> {
    let path = rom_path.strip_prefix("/roms/")?;
    let (system, rest) = path.split_once('/')?;
    let rom_filename = rest.rsplit_once('/').map(|(_, f)| f).unwrap_or(rest);
    Some((system.to_string(), rom_filename.to_string()))
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use super::*;

    // --- parse_rom_path ---

    #[test]
    fn parse_simple_rom_path() {
        let result = parse_rom_path("/roms/sega_smd/Sonic.md");
        assert_eq!(
            result,
            Some(("sega_smd".to_string(), "Sonic.md".to_string()))
        );
    }

    #[test]
    fn parse_nested_rom_path() {
        let result =
            parse_rom_path("/roms/arcade_dc/Atomiswave/Horizontal Games/00 Clean Romset/ggx15.zip");
        assert_eq!(
            result,
            Some(("arcade_dc".to_string(), "ggx15.zip".to_string()))
        );
    }

    #[test]
    fn parse_rom_path_missing_prefix() {
        assert_eq!(parse_rom_path("sega_smd/Sonic.md"), None);
    }

    #[test]
    fn parse_rom_path_only_system() {
        // No filename after system
        assert_eq!(parse_rom_path("/roms/sega_smd"), None);
    }

    #[test]
    fn parse_rom_path_with_spaces() {
        let result = parse_rom_path("/roms/nintendo_snes/Super Mario World (USA).sfc");
        assert_eq!(
            result,
            Some((
                "nintendo_snes".to_string(),
                "Super Mario World (USA).sfc".to_string()
            ))
        );
    }

    // --- validate_path_safe ---

    #[test]
    fn safe_path_accepted() {
        assert!(validate_path_safe("sega_smd/Sonic.md").is_ok());
    }

    #[test]
    fn path_traversal_rejected() {
        assert!(validate_path_safe("../etc/passwd").is_err());
        assert!(validate_path_safe("foo/../../bar").is_err());
    }

    #[test]
    fn backslash_rejected() {
        assert!(validate_path_safe("foo\\bar.rom").is_err());
    }

    #[test]
    fn empty_path_accepted() {
        assert!(validate_path_safe("").is_ok());
    }

    // --- system_player_counts ---

    #[test]
    fn batch_player_counts_known_system() {
        // SNES has player data in game_db. Super Mario World is a known 1-player game.
        let filenames = vec!["Super Mario World (USA).sfc"];
        let counts = system_player_counts("nintendo_snes", &filenames);
        // Should return either a count > 0 or not be present (if the DB has it).
        // The key invariant: no entries with players == 0 in the map.
        for &p in counts.values() {
            assert!(
                p > 0,
                "Batch map should only contain positive player counts"
            );
        }
    }

    #[test]
    fn batch_player_counts_unknown_filenames() {
        let filenames = vec!["nonexistent_game_12345.sfc"];
        let counts = system_player_counts("nintendo_snes", &filenames);
        assert!(
            !counts.contains_key("nonexistent_game_12345.sfc"),
            "Unknown game should not appear in counts map"
        );
    }

    #[test]
    fn batch_player_counts_empty_input() {
        let counts = system_player_counts("nintendo_snes", &[]);
        assert!(counts.is_empty());
    }

    #[test]
    fn batch_player_counts_arcade_system() {
        // Arcade ROMs use .zip extension; system_player_counts should handle stripping it.
        let filenames = vec!["mslug6.zip"];
        let counts = system_player_counts("arcade_fbneo", &filenames);
        if let Some(&p) = counts.get("mslug6.zip") {
            assert!(p >= 2, "Metal Slug 6 should be at least 2 players, got {p}");
        }
    }
}
