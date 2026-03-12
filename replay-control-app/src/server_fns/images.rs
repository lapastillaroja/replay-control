use super::*;

/// Get image stats: (boxart_count, snap_count, media_size_bytes).
#[server(prefix = "/sfn")]
pub async fn get_image_stats() -> Result<(usize, usize, u64), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let (with_boxart, with_snap) = {
        let guard = state
            .metadata_db()
            .ok_or_else(|| ServerFnError::new("Cannot open metadata DB"))?;
        let db = guard
            .as_ref()
            .ok_or_else(|| ServerFnError::new("Metadata DB not available"))?;
        db.image_stats()
            .map_err(|e| ServerFnError::new(e.to_string()))?
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
