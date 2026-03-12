use super::*;

/// Progress for the two-phase thumbnail pipeline (index + download).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThumbnailProgress {
    pub phase: ThumbnailPhase,
    /// Display name of the current repo/system being processed.
    pub current_label: String,
    /// For index phase: repos done. For download phase: ROMs processed.
    pub step_done: usize,
    /// For index phase: total repos. For download phase: total ROMs.
    pub step_total: usize,
    /// Running count of images downloaded (download phase).
    pub downloaded: usize,
    /// Running count of index entries (index phase).
    pub entries_indexed: usize,
    pub elapsed_secs: u64,
    pub error: Option<String>,
}

/// Phase of the thumbnail pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThumbnailPhase {
    /// Fetching file listings from GitHub API.
    Indexing,
    /// Downloading images from raw.githubusercontent.com.
    Downloading,
    Complete,
    Failed,
    Cancelled,
}

/// Data source info for the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSourceSummary {
    /// For LaunchBox: entry count. For thumbnails: total across all repos.
    pub entry_count: usize,
    /// For thumbnails: number of repos indexed.
    pub repo_count: usize,
    /// Unix timestamp of oldest import (for freshness display).
    pub oldest_imported_at: Option<i64>,
    /// Human-readable relative time since oldest import (computed server-side).
    pub last_updated_text: String,
}

/// Trigger the two-phase thumbnail pipeline: (1) refresh index, (2) download images.
#[server(prefix = "/sfn")]
pub async fn update_thumbnails() -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    if !state.start_thumbnail_update() {
        return Err(ServerFnError::new(
            "Another metadata operation is already running",
        ));
    }
    Ok(())
}

/// Cancel the current thumbnail update.
#[server(prefix = "/sfn")]
pub async fn cancel_thumbnail_update() -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    state
        .thumbnail_cancel
        .store(true, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

/// Get current thumbnail pipeline progress.
#[server(prefix = "/sfn")]
pub async fn get_thumbnail_progress() -> Result<Option<ThumbnailProgress>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let guard = state
        .thumbnail_progress
        .read()
        .expect("thumbnail_progress lock poisoned");
    Ok(guard.clone())
}

/// Get data source info for the "Thumbnails (libretro)" section.
#[server(prefix = "/sfn")]
pub async fn get_thumbnail_data_source() -> Result<DataSourceSummary, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let guard = state
        .metadata_db()
        .ok_or_else(|| ServerFnError::new("Cannot open metadata DB"))?;
    let db = guard
        .as_ref()
        .ok_or_else(|| ServerFnError::new("Metadata DB not available"))?;

    let stats = db
        .get_data_source_stats("libretro-thumbnails")
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    let last_updated_text = stats
        .oldest_imported_at
        .map(|ts| {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            let diff = now - ts;
            if diff < 60 {
                "just now".to_string()
            } else if diff < 3600 {
                format!("{}m ago", diff / 60)
            } else if diff < 86400 {
                format!("{}h ago", diff / 3600)
            } else {
                format!("{}d ago", diff / 86400)
            }
        })
        .unwrap_or_default();

    Ok(DataSourceSummary {
        entry_count: stats.total_entries,
        repo_count: stats.repo_count,
        oldest_imported_at: stats.oldest_imported_at,
        last_updated_text,
    })
}

/// Clear the thumbnail index (all thumbnail_index rows + libretro data_sources).
#[server(prefix = "/sfn")]
pub async fn clear_thumbnail_index() -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let guard = state
        .metadata_db()
        .ok_or_else(|| ServerFnError::new("Cannot open metadata DB"))?;
    let db = guard
        .as_ref()
        .ok_or_else(|| ServerFnError::new("Metadata DB not available"))?;

    db.clear_thumbnail_index()
        .map_err(|e| ServerFnError::new(e.to_string()))
}
