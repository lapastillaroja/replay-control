use super::*;

/// Image import progress.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageImportProgress {
    pub state: ImageImportState,
    pub system: String,
    pub system_display: String,
    pub processed: usize,
    pub total: usize,
    pub boxart_copied: usize,
    pub snap_copied: usize,
    pub elapsed_secs: u64,
    pub error: Option<String>,
    /// For "download all": which system number we're on (1-based).
    pub current_system: usize,
    /// For "download all": total number of systems to process.
    pub total_systems: usize,
}

/// Image import state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImageImportState {
    Cloning,
    Copying,
    Complete,
    Failed,
    Cancelled,
}

/// Image coverage per system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageCoverage {
    pub system: String,
    pub display_name: String,
    pub total_games: usize,
    pub with_boxart: usize,
    pub with_snap: usize,
    pub has_repo: bool,
}

/// Start downloading and importing images for a system.
#[server(prefix = "/sfn")]
pub async fn import_system_images(system: String) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    if !state.start_image_import(system.clone()) {
        return Err(ServerFnError::new("An image import is already running"));
    }
    Ok(())
}

/// Start downloading images for all supported systems sequentially.
#[server(prefix = "/sfn")]
pub async fn import_all_images() -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    if !state.start_all_images_import() {
        return Err(ServerFnError::new("An image import is already running"));
    }
    Ok(())
}

/// Re-match images for all systems using already-cloned repos (no download).
#[server(prefix = "/sfn")]
pub async fn rematch_all_images() -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    if !state.start_rematch_all_images() {
        return Err(ServerFnError::new(
            "No cloned repos found or an import is already running",
        ));
    }
    Ok(())
}

/// Cancel the current image import.
#[server(prefix = "/sfn")]
pub async fn cancel_image_import() -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    state
        .image_import_cancel
        .store(true, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

/// Get current image import progress.
#[server(prefix = "/sfn")]
pub async fn get_image_import_progress() -> Result<Option<ImageImportProgress>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let guard = state
        .image_import_progress
        .read()
        .expect("image_import_progress lock poisoned");
    Ok(guard.clone())
}

/// Get image coverage per system.
#[server(prefix = "/sfn")]
pub async fn get_image_coverage() -> Result<Vec<ImageCoverage>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();

    let images_per_system = {
        let guard = state
            .metadata_db()
            .ok_or_else(|| ServerFnError::new("Cannot open metadata DB"))?;
        let db = guard
            .as_ref()
            .ok_or_else(|| ServerFnError::new("Metadata DB not available"))?;
        db.images_per_system()
            .map_err(|e| ServerFnError::new(e.to_string()))?
    };

    let storage = state.storage();
    let systems = state.cache.get_systems(&storage);

    let mut img_map: std::collections::HashMap<String, (usize, usize)> = images_per_system
        .into_iter()
        .map(|(s, b, sn)| (s, (b, sn)))
        .collect();

    let mut coverage: Vec<ImageCoverage> = systems
        .into_iter()
        .filter(|s| s.game_count > 0)
        .map(|s| {
            let (with_boxart, with_snap) = img_map.remove(&s.folder_name).unwrap_or((0, 0));
            let has_repo =
                replay_control_core::thumbnails::thumbnail_repo_names(&s.folder_name).is_some();
            ImageCoverage {
                system: s.folder_name,
                display_name: s.display_name,
                total_games: s.game_count,
                with_boxart,
                with_snap,
                has_repo,
            }
        })
        .collect();

    coverage.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    Ok(coverage)
}

/// Get image stats.
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
