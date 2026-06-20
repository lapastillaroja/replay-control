use super::*;
use replay_control_core::library_db::LibraryResourceLink;
#[cfg(feature = "ssr")]
use replay_control_core_server::library_db::{LibraryDb, LibraryGameResource};
#[cfg(feature = "ssr")]
use replay_control_core_server::recents::add_recent;
#[cfg(feature = "ssr")]
use replay_control_core_server::roms::list_rom_group;

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
    /// Set when the search recognizer extracted a structured filter from
    /// the user's free-text query. Drives the pill on the system ROM list.
    #[serde(
        default,
        skip_serializing_if = "super::search::RecognizedFilter::is_empty"
    )]
    pub recognized: super::search::RecognizedFilter,
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
    /// For an M3U row, the lowercase extension of the disc image it references
    /// (e.g. `"chd"` for a multi-disc CHD set). The visible filename is `.m3u`,
    /// so this exposes the *effective* disc-image format the game detail view
    /// needs to decide RetroAchievements availability. `None` for non-M3U rows.
    #[serde(default)]
    pub disc_image_ext: Option<String>,
    /// Every `library_game_resource` row for this ROM, loaded once at SSR
    /// in `get_rom_detail` and partitioned client-side by `resource_type` /
    /// `source`. Today this carries the Shmups Wiki strategy-guide link
    /// (`resource_type="strategy_guide"`, `source="shmups_wiki"`); future
    /// external-link sources (HG101, etc.) plug into the same Vec without
    /// expanding the wire shape.
    ///
    /// Manuals and videos still load lazily through their own server fns —
    /// they do alias-expanded user_data joins and filesystem walks beyond
    /// the bare resource read, and live behind UI sections that aren't
    /// always shown.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub library_resources: Vec<LibraryResourceLink>,
}

impl RomDetail {
    /// Return the URL of the first `library_resources` entry whose
    /// `resource_type` and `source` match. Single helper so UI code
    /// doesn't repeat the `iter().find(…).map(url.clone())` pattern for
    /// every external link surfaced on the detail page.
    pub fn find_resource_url(&self, resource_type: &str, source: &str) -> Option<String> {
        self.library_resources
            .iter()
            .find(|r| r.resource_type == resource_type && r.source == source)
            .map(|r| r.url.clone())
    }
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
    #[server(default)] has_achievements: bool,
    #[server(default)] min_rating: Option<f32>,
    #[server(default)] min_year: Option<u16>,
    #[server(default)] max_year: Option<u16>,
) -> Result<RomPage, ServerFnError> {
    use replay_control_core::systems as sys_db;

    let state = expect_context::<crate::api::AppState>();
    let system_display = sys_db::system_display_name(&system);
    let is_arcade = sys_db::is_arcade_system(&system);
    let region_pref = state.region_preference();
    let region_secondary = state.region_preference_secondary();

    // Unified path: all filtering (content, text search) at the SQL level via
    // search_game_library(). GameEntry rows from the DB already carry genre,
    // rating, players, and driver_status, so enrichment is minimal (just box art
    // and favorites overlay).
    use replay_control_core_server::library_db::SearchFilter;

    let min_rating_f64 = min_rating.map(|r| r as f64);
    let genre_owned = genre.clone();
    let sys_owned = system.clone();

    // Route structured terms (board name) out of the free-text query and
    // into exact-filter dimensions before the ranked scorer runs.
    let recognized_query =
        replay_control_core_server::library::search_recognizer::recognize(search.trim());
    let recognized_board = recognized_query.filters.board;
    let remaining_query_display = recognized_query.remaining_text.clone();
    let search_owned = recognized_query.remaining_text.to_lowercase();

    let db_result = state
        .library_reader
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
                board: recognized_board,
                has_achievements,
            };
            LibraryDb::search_game_library_ranked(
                conn,
                Some(&sys_owned),
                &search_owned,
                &filter,
                offset,
                limit,
                region_pref,
                region_secondary,
            )
        })
        .await;

    let (page_entries, total) = db_result.and_then(|r| r.ok()).unwrap_or((Vec::new(), 0));
    let has_more = offset + page_entries.len() < total;

    // Enrich page entries: box art, favorites, genre (shared with developer page).
    let list_entries = super::enrich_game_entries(&state, page_entries).await;

    Ok(RomPage {
        roms: list_entries,
        total,
        has_more,
        system_display,
        is_arcade,
        recognized: super::search::RecognizedFilter {
            board: recognized_board.map(|b| b.display_label()),
            remaining_query: remaining_query_display,
        },
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
        .library_reader
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
        .external_metadata_reader
        .read({
            let system = system.clone();
            let filename = filename.clone();
            let arcade_display = arcade_display.clone();
            move |em_conn| {
                replay_control_core_server::thumbnail_manifest::count_boxart_variants(
                    em_conn,
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

    let library_resources = load_library_resources(&state, &system, &filename).await;

    // For an M3U playlist, resolve the disc image it points at so the client can
    // reason about the effective disc format (the visible filename is `.m3u`).
    let disc_image_ext = if entry.is_m3u {
        let m3u_path = storage.rom_abs_path(&entry.rom_path);
        replay_control_core_server::roms::m3u_first_disc_extension(&m3u_path)
    } else {
        None
    };

    #[cfg(feature = "ssr")]
    tracing::debug!(
        elapsed_ms = fn_start.elapsed().as_millis(),
        "get_rom_detail complete"
    );
    let size_bytes = list_rom_group(&storage, &system, &entry.rom_path)
        .map(|group| group.iter().map(|file| file.size_bytes).sum())
        .unwrap_or(entry.size_bytes);

    Ok(RomDetail {
        game,
        size_bytes,
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
        disc_image_ext,
        library_resources,
    })
}

/// One trip through the reader pool to fetch every `library_game_resource`
/// row for this ROM. Consumers partition by `resource_type` / `source`
/// (e.g. the Shmups Wiki strategy-guide link picks the row with
/// `resource_type == STRATEGY_GUIDE && source == SHMUPS_WIKI_SOURCE`).
/// Returns an empty Vec on pool-acquire failure or SQL error — the link
/// surfaces are best-effort and never block detail-page render.
#[cfg(feature = "ssr")]
async fn load_library_resources(
    state: &crate::api::AppState,
    system: &str,
    filename: &str,
) -> Vec<LibraryResourceLink> {
    let sys_owned = system.to_string();
    let fname_owned = filename.to_string();
    let result = state
        .library_reader
        .read(move |conn| LibraryDb::game_resources_for_rom(conn, &sys_owned, &fname_owned))
        .await;
    let rows: Vec<LibraryGameResource> = match result {
        Some(Ok(rows)) => rows,
        Some(Err(e)) => {
            tracing::warn!(
                system = %system,
                filename = %filename,
                error = %e,
                "load_library_resources: SQL failed; surfacing no external links"
            );
            Vec::new()
        }
        None => Vec::new(),
    };
    rows.into_iter().map(LibraryResourceLink::from).collect()
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
    super::require_storage_mutation_allowed(&state, "delete ROMs").await?;
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
        .invalidate_system(system, &state.library_writer)
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
    if let Err(e) = state
        .user_data_writer
        .try_write({
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
        .await
    {
        tracing::warn!("ROM delete user-data cascade failed: {e}");
    }

    // 4. Delete library.db game_library entry.
    if let Err(e) = state
        .library_writer
        .try_write({
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
        .await
    {
        tracing::warn!("ROM delete library cascade failed: {e}");
    }
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
    super::require_storage_mutation_allowed(&state, "rename ROMs").await?;
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
        .invalidate_system(system, &state.library_writer)
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
    if let Err(e) = state
        .user_data_writer
        .try_write({
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
        .await
    {
        tracing::warn!("ROM rename user-data cascade failed: {e}");
    }

    // 4. Update library.db game_library entry.
    if let Err(e) = state
        .library_writer
        .try_write({
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
        .await
    {
        tracing::warn!("ROM rename library cascade failed: {e}");
    }
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
pub async fn launch_game(rom_path: String, return_to: String) -> Result<String, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    if !state.mode.is_device() {
        #[cfg(feature = "ssr")]
        redirect_after_progressive_form(&return_to);
        return Ok("Launch simulated (standalone mode)".into());
    }

    super::require_storage_mutation_allowed(&state, "launch games").await?;
    let storage = state.storage();

    // Launching goes through the RePlayOS API: no integration ⇒ point the
    // user at onboarding instead of failing cryptically.
    let api = state
        .replay_api
        .clone()
        .filter(|api| api.client().has_token())
        .ok_or_else(|| {
            ServerFnError::new(
                "Launching games needs the RePlayOS Net Control connection — set it up in Settings",
            )
        })?;

    replay_control_core_server::launch::validate_rom_exists(&storage, &rom_path)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    let (system, game_file) = replay_control_core_server::launch::launch_parts(&rom_path)
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    tracing::info!(rom = %rom_path, system, game_file, "launching game via RePlayOS API");
    if let Err(e) = api.client().load_game(system, game_file).await {
        // Feed connection-state failures (401 after a TV-side code reset,
        // frontend down) into the status machine so the UI surfaces them.
        api.report_error(&e);
        return Err(ServerFnError::new(e.to_string()));
    }

    // Write our own recents marker even though RePlayOS writes one on
    // `load_game` too. Measured on the dev Pi (2026-06-14, NFS storage):
    // RePlayOS's marker lands ~120 ms after launch when warm and ~670 ms cold,
    // whereas this write is ~3 ms. Writing it here makes the just-launched game
    // show up in recents / on the home page immediately instead of lagging that
    // window (the launch redirect + invalidate below would otherwise rescan
    // before RePlayOS's marker exists). `list_recents` dedupes the two markers
    // by (system, rom_filename), so the overlap is harmless.
    let rom_filename = game_file
        .rsplit_once('/')
        .map(|(_, filename)| filename)
        .unwrap_or(game_file);
    if let Err(e) = add_recent(&storage, system, rom_filename, &rom_path) {
        tracing::warn!("Failed to create recents entry: {e}");
    }
    state.cache.invalidate_recents().await;
    state.cache.invalidate_recommendations().await;

    #[cfg(feature = "ssr")]
    redirect_after_progressive_form(&return_to);
    Ok("Game launching".into())
}

#[cfg(feature = "ssr")]
fn redirect_after_progressive_form(return_to: &str) {
    if !return_to.is_empty() && return_to.starts_with('/') && !return_to.starts_with("//") {
        leptos_axum::redirect(return_to);
    }
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use super::*;

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
