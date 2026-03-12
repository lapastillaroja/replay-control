use super::*;

/// Get image stats: (boxart_count, snap_count, media_size_bytes).
/// Returns zeros when the DB is unavailable (e.g., during import).
#[server(prefix = "/sfn")]
pub async fn get_image_stats() -> Result<(usize, usize, u64), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let (with_boxart, with_snap) = match state.metadata_db() {
        Some(guard) if guard.as_ref().is_some() => guard
            .as_ref()
            .unwrap()
            .image_stats()
            .unwrap_or((0, 0)),
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
