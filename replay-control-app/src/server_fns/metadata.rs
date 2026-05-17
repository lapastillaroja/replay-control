use super::*;
#[cfg(feature = "ssr")]
use replay_control_core_server::library_db::LibraryDb;

/// Status of the first-run setup checklist.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupStatus {
    /// Whether the setup card should be shown.
    pub show_setup: bool,
    /// LaunchBox metadata has been imported.
    pub has_metadata: bool,
    /// Thumbnail index has entries.
    pub has_thumbnail_index: bool,
}

/// Check whether the first-run setup card should be displayed.
/// Fast path: if the user has dismissed it (and not forced), returns
/// immediately with `show_setup: false` (no DB I/O).
/// Pass `force: true` (via `/?setup` query param) to always show the card —
/// the real `has_metadata` / `has_thumbnail_index` values are still queried
/// so the UI can label buttons "Update" instead of "Start".
#[server(prefix = "/sfn")]
pub async fn get_setup_status(force: bool) -> Result<SetupStatus, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();

    if !force {
        let dismissed = state
            .prefs
            .read()
            .expect("prefs lock poisoned")
            .setup_dismissed;
        if dismissed {
            return Ok(SetupStatus {
                show_setup: false,
                has_metadata: true,
                has_thumbnail_index: true,
            });
        }
    }

    let has_metadata = state
        .external_metadata_reader
        .read(|conn| {
            replay_control_core_server::external_metadata::launchbox_game_count(conn).unwrap_or(0)
                > 0
        })
        .await
        .unwrap_or(false);

    let has_thumbnail_index = state
        .external_metadata_reader
        .read(|conn| {
            replay_control_core_server::external_metadata::get_data_source_stats(
                conn,
                "libretro-thumbnails",
            )
            .map(|s| s.total_entries > 0)
            .unwrap_or(false)
        })
        .await
        .unwrap_or(false);

    let show_setup = force || !has_metadata || !has_thumbnail_index;

    Ok(SetupStatus {
        show_setup,
        has_metadata,
        has_thumbnail_index,
    })
}

/// Dismiss the first-run setup checklist. Persists to settings.cfg and
/// updates the in-memory cached prefs so subsequent SSR renders skip the DB check.
#[server(prefix = "/sfn")]
pub async fn dismiss_setup() -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    replay_control_core_server::settings::write_setup_dismissed(&state.settings, true)
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    state
        .prefs
        .write()
        .expect("prefs lock poisoned")
        .setup_dismissed = true;
    Ok(())
}

// Re-export progress types from the activity module (SSR) or types module (WASM).
#[cfg(feature = "ssr")]
pub use crate::api::activity::{
    Activity, IdentityPhase, IdentityProgress, MaintenanceKind, RebuildPhase, RebuildProgress,
    RefreshMetadataPhase, RefreshMetadataProgress, StartupPhase, ThumbnailPhase, ThumbnailProgress,
};
#[cfg(not(feature = "ssr"))]
pub use crate::types::{
    Activity, IdentityPhase, IdentityProgress, MaintenanceKind, RebuildPhase, RebuildProgress,
    RefreshMetadataPhase, RefreshMetadataProgress, StartupPhase, ThumbnailPhase, ThumbnailProgress,
};

pub use replay_control_core::library_db::{
    DriverStatusCounts, ImportProgress, ImportState, ImportStats, LibrarySummary, MetadataStats,
    SystemCoverage,
};

/// Aggregated `/settings/metadata` payload. Server-side this is built by
/// `LibraryService::metadata_page_snapshot` from a single `pool.read()`
/// closure plus off-pool helpers; clients render six panels from one
/// resource. See `api/library/metadata_snapshot.rs`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MetadataPageSnapshot {
    pub stats: MetadataStats,
    pub library_summary: LibrarySummary,
    pub coverage: Vec<SystemCoverage>,
    pub data_source: super::DataSourceSummary,
    /// (boxart_count, snap_count, media_size_bytes)
    pub image_stats: (usize, usize, u64),
    pub builtin_stats: BuiltinDbStats,
    /// Storage type tag (e.g. `"sd"`, `"usb"`, `"nvme"`, `"nfs"`).
    pub storage_kind: String,
    /// Mount point for ROM storage (e.g. `"/media/usb"`).
    pub storage_root: String,
}

/// Single-flight cache-backed snapshot of the metadata page.
///
/// Six per-stat server fns previously fanned out from this page; under SSR
/// fan-out they all queued through the size-1 read pool and a force-refresh
/// cancellation could orphan multiple in-flight closures. This server fn
/// returns the whole page in one call from an in-memory snapshot, with one
/// pool acquisition on cache miss. Invalidated at the same write-completion
/// sites that invalidate the other user-facing caches.
#[server(prefix = "/sfn")]
pub async fn get_metadata_page_snapshot() -> Result<MetadataPageSnapshot, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    Ok(state.cache.metadata_page_snapshot(&state).await)
}

/// Get metadata coverage stats.
/// Returns empty stats when the DB is unavailable (e.g., during import).
#[server(prefix = "/sfn")]
pub async fn get_metadata_stats() -> Result<MetadataStats, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let db_path = state.external_metadata_reader.db_path();
    let Some(result) = state
        .external_metadata_reader
        .read(move |conn| {
            replay_control_core_server::external_metadata::launchbox_stats(conn, &db_path)
        })
        .await
    else {
        return Ok(MetadataStats::default());
    };
    result.map_err(|e| {
        tracing::warn!("get_metadata_stats failed: {e:?}");
        ServerFnError::new("Could not load metadata stats. Please try again.")
    })
}

/// Get aggregate library summary stats for the metadata page summary cards.
#[server(prefix = "/sfn")]
pub async fn get_library_summary() -> Result<LibrarySummary, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let result = state.library_reader.read(LibraryDb::library_summary).await;
    match result {
        Some(Ok(summary)) => Ok(summary),
        Some(Err(e)) => {
            tracing::warn!("get_library_summary failed: {e:?}");
            Ok(LibrarySummary::default())
        }
        None => Ok(LibrarySummary::default()),
    }
}

/// Get per-system metadata coverage stats.
#[server(prefix = "/sfn")]
pub async fn get_system_coverage() -> Result<Vec<SystemCoverage>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();

    // Get metadata entries (from external_metadata pool), thumbnail counts,
    // coverage stats, and driver status per system. Return empty data when a
    // pool is unavailable (e.g., during import).
    let entries_per_system = state
        .external_metadata_reader
        .read(|conn| {
            replay_control_core_server::external_metadata::launchbox_entries_per_system(conn)
                .unwrap_or_default()
        })
        .await
        .unwrap_or_default();

    let (system_meta, thumbnails_per_system, coverage_stats, driver_status) = state
        .library_reader
        .read(|conn| {
            let system_meta = LibraryDb::load_all_system_meta(conn).unwrap_or_default();
            let thumbnails = LibraryDb::thumbnails_per_system(conn).unwrap_or_default();
            let stats = LibraryDb::system_coverage_stats(conn).unwrap_or_default();
            let drivers = LibraryDb::driver_status_per_system(conn).unwrap_or_default();
            (system_meta, thumbnails, stats, drivers)
        })
        .await
        .unwrap_or_default();

    Ok(
        replay_control_core_server::library_db::build_system_coverage(
            system_meta,
            entries_per_system,
            thumbnails_per_system,
            coverage_stats,
            driver_status,
        ),
    )
}

/// Stats for the bundled catalog (arcade, game, and series reference data).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BuiltinDbStats {
    pub arcade_entries: usize,
    pub arcade_mame_version: String,
    pub game_rom_entries: usize,
    pub game_system_count: usize,
    pub wikidata_series_entries: usize,
    pub wikidata_series_count: usize,
    pub manual_resource_entries: usize,
    pub mister_manual_resource_entries: usize,
    pub retrokit_manual_resource_entries: usize,
}

/// Get stats for the bundled catalog (arcade, game, and series reference data).
#[server(prefix = "/sfn")]
pub async fn get_builtin_db_stats() -> Result<BuiltinDbStats, ServerFnError> {
    use replay_control_core_server::{arcade_db, catalog_pool, game_db, series_db};
    let (
        arcade_entries,
        game_rom_entries,
        game_system_count,
        wikidata_series_entries,
        wikidata_series_count,
        catalog_resources,
    ) = tokio::join!(
        arcade_db::entry_count(),
        game_db::total_rom_entries(),
        game_db::system_count(),
        series_db::entry_count(),
        async { series_db::all_series_names().await.len() },
        catalog_pool::catalog_resource_stats(),
    );
    Ok(BuiltinDbStats {
        arcade_entries,
        arcade_mame_version: arcade_db::MAME_VERSION.to_string(),
        game_rom_entries,
        game_system_count,
        wikidata_series_entries,
        wikidata_series_count,
        manual_resource_entries: catalog_resources.manual_resources,
        mister_manual_resource_entries: catalog_resources.mister_manual_resources,
        retrokit_manual_resource_entries: catalog_resources.retrokit_manual_resources,
    })
}

/// Clear cached provider metadata and reset the XML hash stamp so the next
/// boot re-parses LaunchBox metadata from disk.
#[server(prefix = "/sfn")]
pub async fn clear_metadata() -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    super::require_storage_mutation_allowed(&state, "clear metadata").await?;

    let _guard = state
        .try_start_activity(crate::api::Activity::Maintenance {
            kind: crate::api::MaintenanceKind::ClearMetadata,
        })
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    state
        .external_metadata_writer
        .try_write(|conn| replay_control_core_server::external_metadata::clear_launchbox(conn))
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    state.cache.invalidate_metadata_page().await;
    Ok(())
}

/// Clear LaunchBox metadata and re-trigger the boot-time refresh path.
/// The XML hash stamp is wiped so the next pipeline tick re-parses.
#[server(prefix = "/sfn")]
pub async fn regenerate_metadata() -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    super::require_storage_mutation_allowed(&state, "regenerate metadata").await?;
    state
        .external_metadata_writer
        .try_write(|conn| replay_control_core_server::external_metadata::clear_launchbox(conn))
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    crate::api::BackgroundManager::spawn_external_metadata_refresh(state.clone());
    Ok(())
}

/// Download LaunchBox metadata from the internet, extract, then trigger
/// the standard refresh path.
#[server(prefix = "/sfn")]
pub async fn download_metadata() -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    super::require_storage_mutation_allowed(&state, "download metadata").await?;
    crate::api::BackgroundManager::spawn_external_metadata_download_and_refresh(state.clone());
    Ok(())
}

/// Rescan the game library: walk ROM directories and reconcile each visible
/// system to current disk state, applying additions, updates, and removals
/// without clearing the whole library first.
///
/// Per-system semantics:
/// - **Local storage** (SD/USB/NVMe): a missing top-level system folder is
///   treated as a user-initiated deletion and reconciled to empty (cached
///   rows are dropped, meta updates to `rom_count=0`).
/// - **NFS storage**: a missing top-level system folder is ambiguous (could
///   be a transient mount blip or a real remote-side delete) and returns
///   `Err`; cached rows are preserved.
/// - Any FS read error mid-walk returns `Err` and preserves cached state on
///   every storage kind.
#[server(prefix = "/sfn")]
pub async fn rescan_game_library() -> Result<(), ServerFnError> {
    use crate::api::activity::RebuildProgress;

    let state = expect_context::<crate::api::AppState>();
    super::require_storage_mutation_allowed(&state, "rescan the game library").await?;

    let guard = state
        .try_start_activity(crate::api::Activity::Rebuild {
            progress: RebuildProgress::initial(true),
        })
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    state.cache.invalidate_l1().await;
    state.invalidate_user_caches().await;

    state.spawn_populate(guard, true);
    Ok(())
}

/// Rebuild the game library: strict-reconciles every visible system from disk
/// and enriches each successful scan without pre-clearing the L2 cache.
#[server(prefix = "/sfn")]
pub async fn rebuild_game_library() -> Result<(), ServerFnError> {
    use crate::api::activity::RebuildProgress;

    let state = expect_context::<crate::api::AppState>();
    super::require_storage_mutation_allowed(&state, "rebuild the game library").await?;

    let guard = state
        .try_start_activity(crate::api::Activity::Rebuild {
            progress: RebuildProgress::initial(false),
        })
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    // Clear L1 / user caches only. **Do not pre-clear L2** — under the
    // strict reconcile rule, each per-system scan replaces L2 only on
    // success and preserves L2 on FS error. Pre-clearing destroys the
    // fallback rows, reopening the "rebuild during NFS hiccup wipes
    // your library" vector.
    state.cache.invalidate_l1().await;
    state.invalidate_user_caches().await;

    state.spawn_populate(guard, false);
    Ok(())
}

// ── Corruption status & recovery ──────────────────────────────────

/// Corruption status for both databases.
///
/// Pushed to clients via `ConfigEvent::CorruptionChanged` on `/sse/config`
/// (see `api::ConfigEvent`); also included in the stream's `init` payload.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CorruptionStatus {
    pub library_corrupt: bool,
    pub user_data_corrupt: bool,
    pub user_data_backup_exists: bool,
}

/// Rebuild a corrupt library database: close, delete, reopen, trigger pipeline.
/// The library DB is rebuildable — no data loss.
#[server(prefix = "/sfn")]
pub async fn rebuild_corrupt_library() -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    super::require_storage_mutation_allowed(&state, "rebuild the corrupt library").await?;
    if !state.library_reader.is_corrupt() {
        return Err(ServerFnError::new("Library database is not corrupt"));
    }

    let db_path = state.library_reader.db_path();
    tracing::info!("Rebuilding corrupt library DB at {}", db_path.display());

    // Drain in-flight ops, unlink files, and reopen with a fresh empty
    // schema — single atomic lifecycle transition. The previous
    // close/unlink/reopen choreography raced in-flight reads.
    if !state.library_writer.reset_to_empty().await {
        return Err(ServerFnError::new(
            "Failed to reopen library DB after rebuild",
        ));
    }
    // L2 was already wiped by reset_to_empty; this drops L1.
    if let Err(e) = state.cache.invalidate(&state.library_writer).await {
        tracing::warn!("post-rebuild cache.invalidate failed: {e}");
    }
    state.invalidate_user_caches().await;
    // Trigger background re-import if XML exists.
    crate::api::BackgroundManager::spawn_external_metadata_refresh(state.clone());
    Ok(())
}

/// Repair a corrupt user_data database: close, delete, reopen with fresh schema.
/// Warning: box art overrides and saved videos will be lost.
#[server(prefix = "/sfn")]
pub async fn repair_corrupt_user_data() -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    super::require_storage_mutation_allowed(&state, "repair user data").await?;
    if !state.user_data_reader.is_corrupt() {
        return Err(ServerFnError::new("User data database is not corrupt"));
    }

    let db_path = state.user_data_reader.db_path();
    tracing::info!("Repairing corrupt user data DB at {}", db_path.display());

    if !state.user_data_writer.reset_to_empty().await {
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
    super::require_storage_mutation_allowed(&state, "restore user data").await?;
    if !state.user_data_reader.is_corrupt() {
        return Err(ServerFnError::new("User data database is not corrupt"));
    }

    let db_path = state.user_data_reader.db_path();
    let backup_path = db_path.with_extension("db.bak");
    if !backup_path.exists() {
        return Err(ServerFnError::new("No backup file found"));
    }

    tracing::info!(
        "Restoring user data DB from backup at {}",
        backup_path.display()
    );

    // Drain → copy backup over current DB → reopen.
    if !state.user_data_writer.replace_with_file(&backup_path).await {
        // Restored copy is also corrupt — fall back to fresh DB.
        tracing::warn!("Restored user_data.db backup is also corrupt, creating fresh DB");
        if !state.user_data_writer.reset_to_empty().await {
            return Err(ServerFnError::new(
                "Failed to reopen user data DB after restore",
            ));
        }
    }
    Ok(())
}
