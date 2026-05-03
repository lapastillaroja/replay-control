use super::*;
#[cfg(feature = "ssr")]
use replay_control_core_server::library_db::LibraryDb;

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
    #[server(default)] coop_only: bool,
    #[server(default)] min_rating: Option<f32>,
    #[server(default)] min_year: Option<u16>,
    #[server(default)] max_year: Option<u16>,
) -> Result<RomPage, ServerFnError> {
    use replay_control_core::systems as sys_db;

    let state = expect_context::<crate::api::AppState>();
    let system_display = sys_db::find_system(&system)
        .map(|s| s.display_name.to_string())
        .unwrap_or_else(|| system.clone());
    let is_arcade = sys_db::is_arcade_system(&system);
    let region_pref = state.region_preference();
    let region_secondary = state.region_preference_secondary();

    // Unified path: all filtering (content, text search) at the SQL level via
    // search_game_library(). GameEntry rows from the DB already carry genre,
    // rating, players, and driver_status, so enrichment is minimal (just box art
    // and favorites overlay).
    use replay_control_core_server::library_db::SearchFilter;

    let q = search.to_lowercase();
    let query_words: Vec<String> = if q.is_empty() {
        Vec::new()
    } else {
        super::search::split_into_words(&q)
            .into_iter()
            .map(|w| w.to_string())
            .collect()
    };

    let min_rating_f64 = min_rating.map(|r| r as f64);
    let genre_owned = genre.clone();
    let sys_owned = system.clone();

    let db_result = state
        .library_pool
        .read(move |conn| {
            let filter = SearchFilter {
                hide_hacks,
                hide_translations,
                hide_betas,
                hide_clones,
                genre: &genre_owned,
                multiplayer_only,
                coop_only,
                min_rating: min_rating_f64,
                min_year,
                max_year,
            };
            LibraryDb::search_game_library(
                conn,
                Some(&sys_owned),
                None,
                &query_words,
                &filter,
                offset,
                limit,
            )
        })
        .await;

    let (entries, total) = db_result.and_then(|r| r.ok()).unwrap_or((Vec::new(), 0));

    // When text search is active, score and paginate in Rust (SQL returned all
    // matching rows without LIMIT/OFFSET so we can sort by relevance).
    let (page_entries, total, has_more) = if !search.is_empty() {
        let mut scored: Vec<(u32, replay_control_core_server::library_db::GameEntry)> = entries
            .into_iter()
            .filter_map(|entry| {
                let display = entry.display_name.as_deref().unwrap_or(&entry.rom_filename);
                let score = search_score(
                    &q,
                    display,
                    &entry.rom_filename,
                    region_pref,
                    region_secondary,
                );
                if score > 0 {
                    Some((score, entry))
                } else {
                    None
                }
            })
            .collect();
        scored.sort_by_key(|s| std::cmp::Reverse(s.0));
        let scored_total = scored.len();
        let page: Vec<_> = scored
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(|(_, e)| e)
            .collect();
        let hm = offset + page.len() < scored_total;
        (page, scored_total, hm)
    } else {
        let hm = offset + entries.len() < total;
        (entries, total, hm)
    };

    // Enrich page entries: box art, favorites, genre (shared with developer page).
    let list_entries = super::enrich_game_entries(&state, page_entries).await;

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
    #[cfg(feature = "ssr")]
    let fn_start = std::time::Instant::now();
    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();

    // Fetch the full GameEntry from game_library (source of truth for all metadata).
    let sys_owned = system.clone();
    let fname_owned = filename.clone();
    let entry = state
        .library_pool
        .read(move |conn| LibraryDb::load_single_entry(conn, &sys_owned, &fname_owned))
        .await
        .and_then(|r| r.ok())
        .flatten()
        .ok_or_else(|| {
            if !state.is_idle() {
                ServerFnError::new("Game data is temporarily unavailable while the library is being rebuilt. Please try again in a moment.")
            } else {
                ServerFnError::new(format!("ROM not found: {filename}"))
            }
        })?;

    let is_favorite =
        replay_control_core_server::favorites::is_favorite(&storage, &system, &filename).await;

    let game = build_game_detail(&state, &entry).await;

    #[cfg(feature = "ssr")]
    tracing::debug!(
        elapsed_ms = fn_start.elapsed().as_millis(),
        "get_rom_detail game_info resolved"
    );

    let user_screenshots = replay_control_core_server::screenshots::find_screenshots_for_rom(
        &storage, &system, &filename,
    )
    .into_iter()
    .map(|s| ScreenshotUrl {
        url: format!("/captures/{}/{}", system, s.filename),
        timestamp: s.timestamp,
    })
    .collect();

    // Count box art variants (manifest index only — no filesystem scan to avoid N+1).
    let arcade_display =
        replay_control_core_server::arcade_db::display_name_if_arcade(&system, &filename).await;
    let variant_count = state
        .library_pool
        .read({
            let system = system.clone();
            let filename = filename.clone();
            let arcade_display = arcade_display.clone();
            move |conn| {
                replay_control_core_server::thumbnail_manifest::count_boxart_variants(
                    conn,
                    &system,
                    &filename,
                    arcade_display.as_deref(),
                )
            }
        })
        .await
        .unwrap_or(0);

    let (tier, _, is_special) = replay_control_core::rom_tags::classify(&filename);
    let is_hack = tier == replay_control_core::rom_tags::RomTier::Hack;

    let base_title = replay_control_core::title_utils::base_title(&game.display_name);

    // Determine rename restrictions.
    let (rename_allowed, rename_reason) = replay_control_core_server::roms::check_rename_allowed(
        &storage,
        &system,
        entry.rom_path.trim_start_matches('/'),
    );

    // Detect multi-disc set.
    let disc_info = replay_control_core_server::roms::detect_disc_set(&storage, &system, &filename)
        .map(|di| DiscInfoDto {
            disc_number: di.disc_number,
            total_discs: di.total_discs,
            siblings: di.siblings,
        });

    #[cfg(feature = "ssr")]
    tracing::debug!(
        elapsed_ms = fn_start.elapsed().as_millis(),
        "get_rom_detail complete"
    );
    Ok(RomDetail {
        game,
        size_bytes: entry.size_bytes,
        is_m3u: entry.is_m3u,
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

    let mut group =
        replay_control_core_server::roms::list_rom_group(&storage, &system, &relative_path)
            .map_err(|e| ServerFnError::new(e.to_string()))?;

    // If this ROM is part of a multi-disc set (no M3U), include sibling discs.
    let rom_filename = std::path::Path::new(&relative_path)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();
    if let Some(disc_info) =
        replay_control_core_server::roms::detect_disc_set(&storage, &system, &rom_filename)
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
                group.push(replay_control_core_server::roms::GroupedFile {
                    path: sibling_path,
                    size_bytes: size,
                    kind: replay_control_core_server::roms::FileKind::Disc,
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
                replay_control_core_server::roms::FileKind::Primary => "primary",
                replay_control_core_server::roms::FileKind::Disc => "disc",
                replay_control_core_server::roms::FileKind::Companion => "companion",
                replay_control_core_server::roms::FileKind::DataDir => "directory",
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
        replay_control_core_server::roms::detect_disc_set(&storage, &system, &rom_filename)
            .map(|di| di.siblings)
            .unwrap_or_default();

    // Delete the primary ROM group.
    let report =
        replay_control_core_server::roms::delete_rom_group(&storage, &system, &relative_path)
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
        if let Err(e) =
            replay_control_core_server::roms::delete_rom_group(&storage, &system, &sibling_rel)
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

    if let Err(e) = state
        .cache
        .invalidate_system(system, &state.library_pool)
        .await
    {
        tracing::debug!("post-mutation invalidate_system skipped: {e}");
    }
    state.cache.invalidate_favorites().await;
    state.invalidate_user_caches().await;

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
    storage: &replay_control_core_server::storage::StorageLocation,
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
                replay_control_core_server::user_data_db::UserDataDb::delete_for_rom(
                    conn,
                    &system,
                    &rom_filename,
                );
            }
        })
        .await;

    // 4. Delete library.db game_library entry.
    state
        .library_pool
        .write({
            let system = system.to_string();
            let rom_filename = rom_filename.to_string();
            move |conn| {
                replay_control_core_server::library_db::LibraryDb::delete_for_rom(
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

    let new_path =
        replay_control_core_server::roms::rename_rom(&storage, &relative_path, &new_filename)
            .map_err(|e| ServerFnError::new(e.to_string()))?;

    // Phase 3: Rename cascade — update all associated data.
    rename_rom_cascade(&state, &storage, &system, &old_filename, &new_filename).await;

    if let Err(e) = state
        .cache
        .invalidate_system(system, &state.library_pool)
        .await
    {
        tracing::debug!("post-mutation invalidate_system skipped: {e}");
    }
    state.cache.invalidate_favorites().await;
    state.invalidate_user_caches().await;

    Ok(new_path.display().to_string())
}

/// Cascade rename updates to all data sources.
///
/// Errors are logged but do not block the rename — the file rename
/// has already succeeded by the time this is called.
#[cfg(feature = "ssr")]
async fn rename_rom_cascade(
    state: &crate::api::AppState,
    storage: &replay_control_core_server::storage::StorageLocation,
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
                replay_control_core_server::user_data_db::UserDataDb::rename_for_rom(
                    conn,
                    &system,
                    &old_filename,
                    &new_filename,
                );
            }
        })
        .await;

    // 4. Update library.db game_library entry.
    state
        .library_pool
        .write({
            let system = system.to_string();
            let old_filename = old_filename.to_string();
            let new_filename = new_filename.to_string();
            move |conn| {
                replay_control_core_server::library_db::LibraryDb::rename_for_rom(
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

    replay_control_core_server::launch::launch_game(&storage, &rom_path)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    // Create a recents entry so the home page reflects the launch immediately.
    // Extract system and rom_filename from the rom_path.
    // rom_path format: "/roms/<system>/<optional_subdirs>/<rom_filename>"
    if let Some((system, rom_filename)) = parse_rom_path(&rom_path) {
        if let Err(e) = replay_control_core_server::recents::add_recent(
            &storage,
            &system,
            &rom_filename,
            &rom_path,
        ) {
            tracing::warn!("Failed to create recents entry: {e}");
        }
        state.cache.invalidate_recents().await;
        state.cache.invalidate_recommendations().await;
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
}
