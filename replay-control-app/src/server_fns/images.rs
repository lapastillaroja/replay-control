use super::*;
#[cfg(feature = "ssr")]
use replay_control_core::metadata_db::MetadataDb;

/// Get image stats: (boxart_count, snap_count, media_size_bytes).
/// Returns zeros when the DB is unavailable (e.g., during import).
#[server(prefix = "/sfn")]
pub async fn get_image_stats() -> Result<(usize, usize, u64), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let (with_boxart, with_snap) = state
        .metadata_pool
        .read(|conn| MetadataDb::image_stats(conn).unwrap_or((0, 0)))
        .await
        .unwrap_or((0, 0));
    let storage = state.storage();
    let media_size = replay_control_core::thumbnails::media_dir_size(&storage.root);
    Ok((with_boxart, with_snap, media_size))
}

/// Clear all images.
#[server(prefix = "/sfn")]
pub async fn clear_images() -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();

    let _guard = state
        .try_start_activity(crate::api::Activity::Maintenance {
            kind: crate::api::MaintenanceKind::ClearImages,
        })
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    let storage = state.storage();
    replay_control_core::thumbnails::clear_media(&storage.root)
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    // _guard drops → Idle
    Ok(())
}

/// Cleanup orphaned images: delete metadata rows and thumbnail files for ROMs
/// that no longer exist in the game library.
///
/// Returns `(metadata_rows_deleted, image_files_deleted, bytes_freed)`.
#[server(prefix = "/sfn")]
pub async fn cleanup_orphaned_images() -> Result<(usize, usize, u64), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();

    let _guard = state
        .try_start_activity(crate::api::Activity::Maintenance {
            kind: crate::api::MaintenanceKind::CleanupOrphans,
        })
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    let storage = state.storage();

    // 1. Delete orphaned metadata rows.
    // 2. Delete orphaned thumbnail files.
    let storage_root = storage.root.clone();
    let (metadata_deleted, files_deleted, bytes_freed) = state
        .metadata_pool
        .write(move |conn| {
            let meta_del = MetadataDb::delete_orphaned_metadata(conn).unwrap_or(0);
            let (files_del, freed) =
                replay_control_core::thumbnails::delete_orphaned_thumbnails(&storage_root, conn)
                    .unwrap_or((0, 0));
            (meta_del, files_del, freed)
        })
        .await
        .unwrap_or((0, 0, 0));

    // _guard drops → Idle
    Ok((metadata_deleted, files_deleted, bytes_freed))
}
