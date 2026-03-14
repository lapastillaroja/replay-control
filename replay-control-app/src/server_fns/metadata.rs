use super::*;

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
    let Some(guard) = state.metadata_db() else {
        return Ok(MetadataStats::default());
    };
    let Some(db) = guard.as_ref() else {
        return Ok(MetadataStats::default());
    };
    db.stats().map_err(|e| ServerFnError::new(e.to_string()))
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

    if !state.start_import(path) {
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

    // Get metadata entries and thumbnail counts per system from DB.
    // Return empty data when DB is unavailable (e.g., during import).
    let (entries_per_system, thumbnails_per_system) = match state.metadata_db() {
        Some(guard) if guard.as_ref().is_some() => {
            let db = guard.as_ref().unwrap();
            let entries = db
                .entries_per_system()
                .map_err(|e| ServerFnError::new(e.to_string()))?;
            let thumbnails = db
                .thumbnails_per_system()
                .map_err(|e| ServerFnError::new(e.to_string()))?;
            (entries, thumbnails)
        }
        _ => (Vec::new(), Vec::new()),
    };

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
            SystemCoverage {
                system: s.folder_name,
                display_name: s.display_name,
                total_games: s.game_count,
                with_metadata,
                with_thumbnail,
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
}

/// Get stats for the built-in (compile-time) metadata databases.
#[server(prefix = "/sfn")]
pub async fn get_builtin_db_stats() -> Result<BuiltinDbStats, ServerFnError> {
    use replay_control_core::{arcade_db, game_db};

    Ok(BuiltinDbStats {
        arcade_entries: arcade_db::entry_count(),
        arcade_mame_version: arcade_db::MAME_VERSION.to_string(),
        game_rom_entries: game_db::total_rom_entries(),
        game_system_count: game_db::system_count(),
    })
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

/// Clear metadata DB and trigger re-import from launchbox-metadata.xml.
/// The import runs in the background; poll `get_import_progress` for status.
#[server(prefix = "/sfn")]
pub async fn regenerate_metadata() -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    state.regenerate_metadata().map_err(ServerFnError::new)
}

/// Check if a metadata operation is currently running (import or thumbnail update).
/// Used by the UI to show a degraded-mode banner.
#[server(prefix = "/sfn")]
pub async fn is_metadata_busy() -> Result<bool, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    Ok(state
        .metadata_operation_in_progress
        .load(std::sync::atomic::Ordering::Relaxed))
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

/// Rebuild the game library: clears game_library tables and triggers a full
/// rescan + enrichment from disk. Use when baked-in data changes or to force
/// a fresh scan of all systems.
///
/// Sets `metadata_operation_in_progress` so the UI shows a busy banner while
/// the rebuild runs in the background. The flag is cleared when the background
/// enrichment task completes (or on error/panic).
#[server(prefix = "/sfn")]
pub async fn rebuild_game_library() -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();

    // Atomically claim the operation slot.
    if state
        .metadata_operation_in_progress
        .swap(true, std::sync::atomic::Ordering::SeqCst)
    {
        return Err(ServerFnError::new(
            "Another metadata operation is already running",
        ));
    }

    // Clear L1+L2 cache. invalidate() uses the direct DB mutex (not
    // metadata_db()), so it works fine with the flag already set.
    state.cache.invalidate();

    // Rebuild in background; the flag is cleared when done (or on panic).
    state.spawn_cache_enrichment_with_flag(state.metadata_operation_in_progress.clone());
    Ok(())
}
