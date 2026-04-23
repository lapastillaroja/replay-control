use super::*;
#[cfg(feature = "ssr")]
use replay_control_core_server::metadata_db::MetadataDb;

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
    let state = expect_context::<crate::api::AppState>();
    state.request_cancel();
    Ok(())
}

/// Get data source info for the "Thumbnails (libretro)" section.
#[server(prefix = "/sfn")]
pub async fn get_thumbnail_data_source() -> Result<DataSourceSummary, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    // Gracefully return defaults when the DB is temporarily unavailable
    // (e.g., during a metadata import or thumbnail update operation).
    let Some(stats) = state
        .metadata_pool
        .read(|conn| MetadataDb::get_data_source_stats(conn, "libretro-thumbnails"))
        .await
    else {
        return Ok(DataSourceSummary {
            entry_count: 0,
            repo_count: 0,
            oldest_imported_at: None,
            last_updated_text: String::new(),
        });
    };
    let stats = stats.map_err(|e| {
        tracing::warn!("get_thumbnail_data_source failed: {e:?}");
        ServerFnError::new("Could not load thumbnail stats. Please try again.")
    })?;

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

    let _guard = state
        .try_start_activity(crate::api::Activity::Maintenance {
            kind: crate::api::MaintenanceKind::ClearThumbnailIndex,
        })
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    state
        .metadata_pool
        .write(|conn| MetadataDb::clear_thumbnail_index(conn))
        .await
        .ok_or_else(|| ServerFnError::new("Cannot open metadata DB"))?
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    // _guard drops → Idle
    Ok(())
}
