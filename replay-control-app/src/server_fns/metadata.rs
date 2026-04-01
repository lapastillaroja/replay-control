use super::*;
#[cfg(feature = "ssr")]
use replay_control_core::metadata_db::MetadataDb;

// Re-export progress types from the activity module (SSR) or types module (WASM).
#[cfg(feature = "ssr")]
pub use crate::api::activity::{
    Activity, MaintenanceKind, RebuildPhase, RebuildProgress, StartupPhase, ThumbnailPhase,
    ThumbnailProgress,
};
#[cfg(not(feature = "ssr"))]
pub use crate::types::{
    Activity, MaintenanceKind, RebuildPhase, RebuildProgress, StartupPhase, ThumbnailPhase,
    ThumbnailProgress,
};

#[cfg(not(feature = "ssr"))]
pub use crate::types::{ImportProgress, ImportState, ImportStats, MetadataStats, SystemCoverage};
#[cfg(feature = "ssr")]
pub use replay_control_core::metadata_db::{
    ImportProgress, ImportState, ImportStats, MetadataStats, SystemCoverage,
};

/// Create an `Activity::ThumbnailUpdate` for client-side placeholder use.
/// Handles the difference between SSR (has `cancel` field) and WASM (no `cancel` field).
#[cfg(feature = "ssr")]
pub fn make_thumbnail_update_activity(progress: ThumbnailProgress) -> Activity {
    Activity::ThumbnailUpdate {
        progress,
        cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    }
}

#[cfg(not(feature = "ssr"))]
pub fn make_thumbnail_update_activity(progress: ThumbnailProgress) -> Activity {
    Activity::ThumbnailUpdate { progress }
}

/// Get current activity (replaces is_metadata_busy, get_busy_label, is_scanning,
/// get_import_progress, get_thumbnail_progress, get_rebuild_progress).
#[server(prefix = "/sfn")]
pub async fn get_activity() -> Result<Activity, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    Ok(state.activity())
}

/// Get metadata coverage stats.
/// Returns empty stats when the DB is unavailable (e.g., during import).
#[server(prefix = "/sfn")]
pub async fn get_metadata_stats() -> Result<MetadataStats, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let db_path = state.metadata_pool.db_path();
    let Some(result) = state
        .metadata_pool
        .read(move |conn| MetadataDb::stats(conn, &db_path))
        .await
    else {
        return Ok(MetadataStats::default());
    };
    result.map_err(|e| {
        tracing::warn!("get_metadata_stats failed: {e:?}");
        ServerFnError::new("Could not load metadata stats. Please try again.")
    })
}

/// Start a background metadata import from a LaunchBox metadata XML file.
/// Returns immediately; poll `get_activity` for status.
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

/// Get per-system metadata coverage stats.
#[server(prefix = "/sfn")]
pub async fn get_system_coverage() -> Result<Vec<SystemCoverage>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();

    // Get metadata entries and thumbnail counts per system from DB.
    // Return empty data when DB is unavailable (e.g., during import).
    let (entries_per_system, thumbnails_per_system) = state
        .metadata_pool
        .read(|conn| {
            let entries = MetadataDb::entries_per_system(conn).unwrap_or_default();
            let thumbnails = MetadataDb::thumbnails_per_system(conn).unwrap_or_default();
            (entries, thumbnails)
        })
        .await
        .unwrap_or_default();

    // Get total games per system from game library.
    let storage = state.storage();
    let systems = state.cache.cached_systems(&storage, &state.metadata_pool).await;

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

    let _guard = state
        .try_start_activity(crate::api::Activity::Maintenance {
            kind: crate::api::MaintenanceKind::ClearMetadata,
        })
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    state
        .metadata_pool
        .write(|conn| MetadataDb::clear(conn))
        .await
        .ok_or_else(|| ServerFnError::new("Cannot open metadata DB"))?
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    // Checkpoint WAL after the DELETE + VACUUM.
    state.metadata_pool.checkpoint().await;
    // _guard drops → Idle
    Ok(())
}

/// Clear metadata DB and trigger re-import from launchbox-metadata.xml.
/// The import runs in the background; poll `get_activity` for status.
#[server(prefix = "/sfn")]
pub async fn regenerate_metadata() -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    state
        .import
        .regenerate_metadata(&state)
        .await
        .map_err(ServerFnError::new)
}

/// Download LaunchBox metadata from the internet, extract, and import.
/// The entire process runs in the background; poll `get_activity` for status.
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
/// rescan + enrichment from disk.
#[server(prefix = "/sfn")]
pub async fn rebuild_game_library() -> Result<(), ServerFnError> {
    use crate::api::activity::{RebuildPhase, RebuildProgress};

    let state = expect_context::<crate::api::AppState>();

    let guard = state
        .try_start_activity(crate::api::Activity::Rebuild {
            progress: RebuildProgress {
                phase: RebuildPhase::Scanning,
                current_system: String::new(),
                systems_done: 0,
                systems_total: 0,
                elapsed_secs: 0,
                error: None,
            },
        })
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    // Clear L1+L2 cache.
    state.cache.invalidate(&state.metadata_pool).await;
    state.response_cache.invalidate_all();

    // Rebuild in background; the guard drops → Idle when done (or on panic).
    state.spawn_rebuild_enrichment(guard);
    Ok(())
}

// ── Corruption status & recovery ──────────────────────────────────

/// Corruption status for both databases.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorruptionStatus {
    pub metadata_corrupt: bool,
    pub user_data_corrupt: bool,
    pub user_data_backup_exists: bool,
}

/// Check if either database is flagged as corrupt.
#[server(prefix = "/sfn")]
pub async fn get_corruption_status() -> Result<CorruptionStatus, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let (metadata_corrupt, user_data_corrupt) = state.is_db_corrupt();
    let backup_exists = state
        .user_data_pool
        .db_path()
        .with_extension("db.bak")
        .exists();
    Ok(CorruptionStatus {
        metadata_corrupt,
        user_data_corrupt,
        user_data_backup_exists: backup_exists,
    })
}

/// Rebuild a corrupt metadata database: close, delete, reopen, trigger pipeline.
/// Metadata is a rebuildable cache — no data loss.
#[server(prefix = "/sfn")]
pub async fn rebuild_corrupt_metadata() -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    if !state.metadata_pool.is_corrupt() {
        return Err(ServerFnError::new("Metadata database is not corrupt"));
    }

    let db_path = state.metadata_pool.db_path();
    tracing::info!("Rebuilding corrupt metadata DB at {}", db_path.display());

    // Close pool (already closed by mark_corrupt, but be safe).
    state.metadata_pool.close();
    // Delete the corrupt DB files.
    replay_control_core::db_common::delete_db_files(&db_path);
    // Reopen at the current storage root — creates fresh schema.
    let storage = state.storage();
    if !state.metadata_pool.reopen(&storage.root) {
        return Err(ServerFnError::new(
            "Failed to reopen metadata DB after rebuild",
        ));
    }
    // Invalidate cache so stale data doesn't persist.
    state.cache.invalidate(&state.metadata_pool).await;
    state.response_cache.invalidate_all();
    // Trigger background re-import if XML exists.
    let _ = state.import.regenerate_metadata(&state).await;
    Ok(())
}

/// Repair a corrupt user_data database: close, delete, reopen with fresh schema.
/// Warning: box art overrides and saved videos will be lost.
#[server(prefix = "/sfn")]
pub async fn repair_corrupt_user_data() -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    if !state.user_data_pool.is_corrupt() {
        return Err(ServerFnError::new("User data database is not corrupt"));
    }

    let db_path = state.user_data_pool.db_path();
    tracing::info!("Repairing corrupt user data DB at {}", db_path.display());

    state.user_data_pool.close();
    replay_control_core::db_common::delete_db_files(&db_path);
    let storage = state.storage();
    if !state.user_data_pool.reopen(&storage.root) {
        return Err(ServerFnError::new(
            "Failed to reopen user data DB after repair",
        ));
    }
    Ok(())
}

/// Restore user_data.db from the startup backup.
/// Falls back to repair (fresh DB) if the backup is also corrupt.
#[server(prefix = "/sfn")]
pub async fn restore_user_data_backup() -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    if !state.user_data_pool.is_corrupt() {
        return Err(ServerFnError::new("User data database is not corrupt"));
    }

    let db_path = state.user_data_pool.db_path();
    let backup_path = db_path.with_extension("db.bak");
    if !backup_path.exists() {
        return Err(ServerFnError::new("No backup file found"));
    }

    tracing::info!(
        "Restoring user data DB from backup at {}",
        backup_path.display()
    );

    // Close pool, copy backup over the DB, reopen.
    state.user_data_pool.close();
    replay_control_core::db_common::delete_db_files(&db_path);
    std::fs::copy(&backup_path, &db_path)
        .map_err(|e| ServerFnError::new(format!("Failed to copy backup: {e}")))?;

    let storage = state.storage();
    if !state.user_data_pool.reopen(&storage.root) {
        // Restored copy is also corrupt — fall back to fresh DB.
        tracing::warn!("Restored user_data.db backup is also corrupt, creating fresh DB");
        replay_control_core::db_common::delete_db_files(&db_path);
        if !state.user_data_pool.reopen(&storage.root) {
            return Err(ServerFnError::new(
                "Failed to reopen user data DB after restore",
            ));
        }
    }
    Ok(())
}
