use super::*;
#[cfg(feature = "ssr")]
use replay_control_core::error::Error as CoreError;
#[cfg(feature = "ssr")]
use replay_control_core_server::library_db::{GameEntry, LibraryDb};
#[cfg(feature = "ssr")]
use replay_control_core_server::thumbnails;

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
    thumbnails::clear_media(&storage.root).map_err(|e| ServerFnError::new(e.to_string()))?;

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
    let media_systems = tokio::task::spawn_blocking({
        let storage_root = storage_root.clone();
        move || thumbnails::media_system_names(&storage_root)
    })
    .await
    .unwrap_or_default();

    let entries_result = state
        .library_reader
        .try_read(
            move |conn| -> Result<Vec<(String, Vec<GameEntry>)>, CoreError> {
                let mut entries_by_system = Vec::with_capacity(media_systems.len());
                for system in media_systems {
                    let entries = LibraryDb::load_system_entries(conn, &system)?;
                    entries_by_system.push((system, entries));
                }
                Ok(entries_by_system)
            },
        )
        .await;
    let entries_by_system = match entries_result {
        Ok(Ok(entries_by_system)) => entries_by_system,
        Ok(Err(e)) => {
            tracing::warn!("Cleanup orphaned images skipped: {e}");
            Vec::new()
        }
        Err(e) => {
            tracing::warn!("Cleanup orphaned images skipped: {e}");
            Vec::new()
        }
    };
    let orphans = tokio::task::spawn_blocking({
        let storage_root = storage_root.clone();
        move || thumbnails::find_orphaned_thumbnails_from_entries(&storage_root, &entries_by_system)
    })
    .await
    .unwrap_or_default();

    let (files_deleted, bytes_freed) =
        tokio::task::spawn_blocking(move || thumbnails::delete_thumbnail_files(&orphans))
            .await
            .unwrap_or((0, 0));

    Ok((0, files_deleted, bytes_freed))
}
