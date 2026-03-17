use super::*;

/// Get image stats: (boxart_count, snap_count, media_size_bytes).
/// Returns zeros when the DB is unavailable (e.g., during import).
#[server(prefix = "/sfn")]
pub async fn get_image_stats() -> Result<(usize, usize, u64), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let (with_boxart, with_snap) = match state.metadata_db() {
        Some(guard) if guard.as_ref().is_some() => {
            guard.as_ref().unwrap().image_stats().unwrap_or((0, 0))
        }
        _ => (0, 0),
    };
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
    let metadata_deleted = match state.metadata_db() {
        Some(guard) if guard.as_ref().is_some() => guard
            .as_ref()
            .unwrap()
            .delete_orphaned_metadata()
            .map_err(|e| ServerFnError::new(e.to_string()))?,
        _ => 0,
    };

    // 2. Delete orphaned thumbnail files.
    let (files_deleted, bytes_freed) = match state.metadata_db() {
        Some(guard) if guard.as_ref().is_some() => {
            replay_control_core::thumbnails::delete_orphaned_thumbnails(
                &storage.root,
                guard.as_ref().unwrap(),
            )
            .map_err(|e| ServerFnError::new(e.to_string()))?
        }
        _ => (0, 0),
    };

    Ok((metadata_deleted, files_deleted, bytes_freed))
}
