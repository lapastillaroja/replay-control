use super::*;

/// Data source info for the UI.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
    tracing::info!("update_thumbnails: handler entered");
    let state = super::app_state()?;
    super::require_storage_mutation_allowed(&state, "update thumbnails").await?;
    if !state.thumbnails.start_thumbnail_update(&state) {
        return Err(ServerFnError::new(
            "Another metadata operation is already running",
        ));
    }
    Ok(())
}

/// Cancel the current thumbnail update.
#[server(prefix = "/sfn")]
pub async fn cancel_thumbnail_update() -> Result<(), ServerFnError> {
    let state = super::app_state()?;
    state.request_cancel();
    Ok(())
}

/// Clear the thumbnail index (all thumbnail_index rows + libretro data_sources).
#[server(prefix = "/sfn")]
pub async fn clear_thumbnail_index() -> Result<(), ServerFnError> {
    let state = super::app_state()?;
    super::require_storage_mutation_allowed(&state, "clear thumbnail index").await?;

    let _guard = state
        .try_start_activity(crate::api::Activity::Maintenance {
            kind: crate::api::MaintenanceKind::ClearThumbnailIndex,
        })
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    state
        .external_metadata_writer
        .try_write(|conn| {
            replay_control_core_server::external_metadata::clear_libretro_thumbnail_manifest(conn)
        })
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    // _guard drops → Idle
    Ok(())
}
