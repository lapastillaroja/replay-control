use super::*;
#[cfg(feature = "ssr")]
use replay_control_core::metadata_db::MetadataDb;

#[cfg(not(feature = "ssr"))]
pub use crate::types::{ImportProgress, ImportState, ImportStats, MetadataStats, SystemCoverage};
#[cfg(feature = "ssr")]
pub use replay_control_core::metadata_db::{
    ImportProgress, ImportState, ImportStats, MetadataStats, SystemCoverage,
};

/// Get metadata coverage stats.
/// Returns empty stats when the DB is unavailable (e.g., during import).
#[server(prefix = "/sfn")]
pub async fn get_metadata_stats() -> Result<MetadataStats, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let db_path = state.metadata_pool.db_path();
    let Some(result) = state.metadata_pool.read(|conn| {
        MetadataDb::stats(conn, &db_path)
    }) else {
        return Ok(MetadataStats::default());
    };
    result.map_err(|e| {
        tracing::warn!("get_metadata_stats failed: {e:?}");
        ServerFnError::new("Could not load metadata stats. Please try again.")
    })
}

/// Start a background metadata import from a LaunchBox metadata XML file.
/// Returns immediately; poll `get_import_progress` for status.
#[server(prefix = "/sfn")]
pub async fn import_launchbox_metadata(xml_path: String) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let path = std::path::PathBuf::from(&xml_path);

    if !path.exists() {
        return Err(ServerFnError::new(format!("File not found: {xml_path}")));
    }

    if !state.import.start_import(path, state.clone()) {
        return Err(ServerFnError::new(
            "Another metadata operation is already running",
        ));
    }

    tracing::info!("Started LaunchBox import from {xml_path}");
    Ok(())
}

/// Get current import progress (None if no import has been started).
#[server(prefix = "/sfn")]
pub async fn get_import_progress() -> Result<Option<ImportProgress>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    Ok(state.import.progress())
}

/// Get per-system metadata coverage stats.
#[server(prefix = "/sfn")]
pub async fn get_system_coverage() -> Result<Vec<SystemCoverage>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();

    // Get metadata entries and thumbnail counts per system from DB.
    // Return empty data when DB is unavailable (e.g., during import).
    let (entries_per_system, thumbnails_per_system) = state.metadata_pool.read(|conn| {
        let entries = MetadataDb::entries_per_system(conn).unwrap_or_default();
        let thumbnails = MetadataDb::thumbnails_per_system(conn).unwrap_or_default();
        (entries, thumbnails)
    }).unwrap_or_default();

    // Get total games per system from game library.
    let storage = state.storage();
    let systems = state.cache.get_systems(&storage);

    let mut meta_map: std::collections::HashMap<String, usize> =
        entries_per_system.into_iter().collect();
    let mut thumb_map: std::collections::HashMap<String, usize> =
        thumbnails_per_system.into_iter().collect();

    let mut coverage: Vec<SystemCoverage> = systems
        .into_iter()
        .filter(|s| s.game_count > 0)
        .map(|s| {
            let with_metadata = meta_map.remove(&s.folder_name).unwrap_or(0);
            let with_thumbnail = thumb_map.remove(&s.folder_name).unwrap_or(0);
            // Cap at total_games to prevent >100% display. This can happen transiently
            // when game_library hasn't been populated yet (before warmup completes) and
            // entries_per_system falls back to counting all game_metadata rows, which
            // may include disc files filtered out by M3U dedup in the filesystem scan.
            SystemCoverage {
                system: s.folder_name,
                display_name: s.display_name,
                total_games: s.game_count,
                with_metadata: with_metadata.min(s.game_count),
                with_thumbnail: with_thumbnail.min(s.game_count),
            }
        })
        .collect();

    coverage.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    Ok(coverage)
}

/// Stats for the built-in (compile-time) metadata databases.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuiltinDbStats {
    pub arcade_entries: usize,
    pub arcade_mame_version: String,
    pub game_rom_entries: usize,
    pub game_system_count: usize,
    pub wikidata_series_entries: usize,
    pub wikidata_series_count: usize,
}

/// Get stats for the built-in (compile-time) metadata databases.
#[server(prefix = "/sfn")]
pub async fn get_builtin_db_stats() -> Result<BuiltinDbStats, ServerFnError> {
    use replay_control_core::{arcade_db, game_db, series_db};

    Ok(BuiltinDbStats {
        arcade_entries: arcade_db::entry_count(),
        arcade_mame_version: arcade_db::MAME_VERSION.to_string(),
        game_rom_entries: game_db::total_rom_entries(),
        game_system_count: game_db::system_count(),
        wikidata_series_entries: series_db::entry_count(),
        wikidata_series_count: series_db::all_series_names().len(),
    })
}

/// Clear all cached metadata.
#[server(prefix = "/sfn")]
pub async fn clear_metadata() -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    state.metadata_pool.write(|conn| {
        MetadataDb::clear(conn)
    })
    .ok_or_else(|| ServerFnError::new("Cannot open metadata DB"))?
    .map_err(|e| ServerFnError::new(e.to_string()))?;

    // Checkpoint WAL after the DELETE + VACUUM.
    state.metadata_pool.checkpoint();
    Ok(())
}

/// Clear metadata DB and trigger re-import from launchbox-metadata.xml.
/// The import runs in the background; poll `get_import_progress` for status.
#[server(prefix = "/sfn")]
pub async fn regenerate_metadata() -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    state
        .import
        .regenerate_metadata(&state)
        .map_err(ServerFnError::new)
}

/// Check if a metadata operation is currently running (import or thumbnail update)
/// or if the background startup pipeline is still warming up.
/// Used by the UI to show a degraded-mode banner.
#[server(prefix = "/sfn")]
pub async fn is_metadata_busy() -> Result<bool, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    Ok(state.is_busy())
}

/// Get a human-readable label for the current background operation.
/// Empty string if idle.
#[server(prefix = "/sfn")]
pub async fn get_busy_label() -> Result<String, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    Ok(state.get_busy_label())
}

/// Check if the background game library scan (warmup) is in progress.
/// Used by the home page to show a "Scanning games..." message.
#[server(prefix = "/sfn")]
pub async fn is_scanning() -> Result<bool, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    Ok(state.is_scanning())
}

/// Download LaunchBox metadata from the internet, extract, and import.
/// The entire process runs in the background; poll `get_import_progress` for status.
#[server(prefix = "/sfn")]
pub async fn download_metadata() -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    if !state.import.start_metadata_download(&state) {
        return Err(ServerFnError::new(
            "A metadata operation is already running",
        ));
    }
    Ok(())
}

/// Rebuild the game library: clears game_library tables and triggers a full
/// rescan + enrichment from disk. Use when baked-in data changes or to force
/// a fresh scan of all systems.
///
/// Claims the shared busy flag so the UI shows a busy banner and concurrent
/// import/thumbnail operations are blocked while the rebuild runs in the
/// background. The flag is cleared when the background enrichment task
/// completes (or on error/panic).
#[server(prefix = "/sfn")]
pub async fn rebuild_game_library() -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();

    // Atomically claim the shared busy flag (same one used by import + thumbnails).
    if !state.claim_busy() {
        return Err(ServerFnError::new(
            "Another metadata operation is already running",
        ));
    }

    // Clear L1+L2 cache.
    state.cache.invalidate();

    // Rebuild in background; the busy flag is cleared when done (or on panic).
    state.spawn_cache_enrichment_with_flag(state.busy_flag());
    Ok(())
}
