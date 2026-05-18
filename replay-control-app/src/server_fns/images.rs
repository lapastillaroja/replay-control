use super::*;
#[cfg(feature = "ssr")]
use replay_control_core_server::library_db::LibraryDb;

/// Clear all images.
#[server(prefix = "/sfn")]
pub async fn clear_images() -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    super::require_storage_mutation_allowed(&state, "clear images").await?;

    let _guard = state
        .try_start_activity(crate::api::Activity::Maintenance {
            kind: crate::api::MaintenanceKind::ClearImages,
        })
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    let storage = state.storage();
    replay_control_core_server::thumbnails::clear_media(&storage.root)
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    // Clear box_art_url from game_library so the UI doesn't show 404 placeholders.
    match state
        .library_writer
        .try_write(|conn| {
            LibraryDb::clear_all_box_art_urls(conn)?;
            LibraryDb::clear_thumbnail_media_stats(conn)
        })
        .await
    {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => tracing::warn!("Failed to clear box_art_url after image clear: {e}"),
        Err(e) => tracing::warn!("Failed to clear box_art_url after image clear: {e}"),
    }

    state.invalidate_user_caches().await;

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
    super::require_storage_mutation_allowed(&state, "cleanup orphaned images").await?;

    let _guard = state
        .try_start_activity(crate::api::Activity::Maintenance {
            kind: crate::api::MaintenanceKind::CleanupOrphans,
        })
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    let storage_root = state.storage().root.clone();
    let orphan_result = state
        .library_reader
        .try_read(move |conn| {
            replay_control_core_server::thumbnails::find_orphaned_thumbnails(&storage_root, conn)
        })
        .await;
    let orphans = match orphan_result {
        Ok(Ok(orphans)) => orphans,
        Ok(Err(e)) => {
            tracing::warn!("Cleanup orphaned images skipped: {e}");
            Vec::new()
        }
        Err(e) => {
            tracing::warn!("Cleanup orphaned images skipped: {e}");
            Vec::new()
        }
    };
    let (files_deleted, bytes_freed) =
        replay_control_core_server::thumbnails::delete_thumbnail_files(&orphans);

    Ok((0, files_deleted, bytes_freed))
}
