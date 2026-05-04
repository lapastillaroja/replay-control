use super::*;
#[cfg(feature = "ssr")]
use replay_control_core_server::library_db::LibraryDb;

/// Get image stats: (boxart_count, snap_count, media_size_bytes).
/// Returns zeros when the DB is unavailable (e.g., during import).
///
/// `boxart_count` is read from `game_library.box_art_url`. Screenshot/title
/// counts aren't tracked centrally any more (filesystem fallback at request
/// time), so `snap_count` is always 0.
#[server(prefix = "/sfn")]
pub async fn get_image_stats() -> Result<(usize, usize, u64), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let (with_boxart, with_snap) = state
        .library_pool
        .read(|conn| LibraryDb::image_stats(conn).unwrap_or((0, 0)))
        .await
        .unwrap_or((0, 0));
    let storage = state.storage();
    let media_size = replay_control_core_server::thumbnails::media_dir_size(&storage.root);
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
    replay_control_core_server::thumbnails::clear_media(&storage.root)
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    // Clear box_art_url from game_library so the UI doesn't show 404 placeholders.
    if let Some(Err(e)) = state
        .library_pool
        .write(|conn| LibraryDb::clear_all_box_art_urls(conn))
        .await
    {
        tracing::warn!("Failed to clear box_art_url after image clear: {e}");
    }

    state.invalidate_user_caches().await;
    state.cache.invalidate_metadata_page().await;

    // _guard drops → Idle
    Ok(())
}

/// Delete thumbnail files on disk for ROMs that no longer exist in
/// `game_library`. Returns `(0, image_files_deleted, bytes_freed)` —
/// the first slot used to count deleted `game_metadata` rows; that table
/// is gone, so the caller-visible tuple shape is preserved with a
/// hard-coded 0.
#[server(prefix = "/sfn")]
pub async fn cleanup_orphaned_images() -> Result<(usize, usize, u64), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();

    let _guard = state
        .try_start_activity(crate::api::Activity::Maintenance {
            kind: crate::api::MaintenanceKind::CleanupOrphans,
        })
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    let storage = state.storage();
    let storage_root = storage.root.clone();
    let (files_deleted, bytes_freed) = state
        .library_pool
        .write(move |conn| {
            replay_control_core_server::thumbnails::delete_orphaned_thumbnails(&storage_root, conn)
                .unwrap_or((0, 0))
        })
        .await
        .unwrap_or((0, 0));

    state.cache.invalidate_metadata_page().await;
    Ok((0, files_deleted, bytes_freed))
}
