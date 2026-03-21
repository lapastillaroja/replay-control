use super::*;
#[cfg(feature = "ssr")]
use replay_control_core::metadata_db::MetadataDb;

/// Get image stats: (boxart_count, snap_count, media_size_bytes).
/// Returns zeros when the DB is unavailable (e.g., during import).
#[server(prefix = "/sfn")]
pub async fn get_image_stats() -> Result<(usize, usize, u64), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let (with_boxart, with_snap) = state.metadata_pool.read(|conn| {
        MetadataDb::image_stats(conn).unwrap_or((0, 0))
    }).unwrap_or((0, 0));
    let storage = state.storage();
    let media_size = replay_control_core::thumbnails::media_dir_size(&storage.root);
    Ok((with_boxart, with_snap, media_size))
}

/// Clear all images.
#[server(prefix = "/sfn")]
pub async fn clear_images() -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();
    replay_control_core::thumbnails::clear_media(&storage.root)
        .map_err(|e| ServerFnError::new(e.to_string()))
}

/// Cleanup orphaned images: delete metadata rows and thumbnail files for ROMs
/// that no longer exist in the game library.
///
/// Returns `(metadata_rows_deleted, image_files_deleted, bytes_freed)`.
#[server(prefix = "/sfn")]
pub async fn cleanup_orphaned_images() -> Result<(usize, usize, u64), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();

    // Guard: refuse to run during rebuild/import/thumbnail update.
    if state.is_busy() {
        return Err(ServerFnError::new(
            "Cannot cleanup while a metadata operation is running",
        ));
    }

    let storage = state.storage();

    // 1. Delete orphaned metadata rows.
    // 2. Delete orphaned thumbnail files.
    // Combined into a single pool read to avoid acquiring two connections.
    let (metadata_deleted, files_deleted, bytes_freed) = state.metadata_pool.read(|conn| {
        let meta_del = MetadataDb::delete_orphaned_metadata(conn).unwrap_or(0);
        let (files_del, freed) = replay_control_core::thumbnails::delete_orphaned_thumbnails(
            &storage.root,
            conn,
        ).unwrap_or((0, 0));
        (meta_del, files_del, freed)
    }).unwrap_or((0, 0, 0));

    Ok((metadata_deleted, files_deleted, bytes_freed))
}
