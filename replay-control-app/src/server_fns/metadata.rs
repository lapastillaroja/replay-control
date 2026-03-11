use super::*;

#[cfg(not(feature = "ssr"))]
pub use crate::types::{ImportProgress, ImportState, ImportStats, MetadataStats, SystemCoverage};
#[cfg(feature = "ssr")]
pub use replay_control_core::metadata_db::{
    ImportProgress, ImportState, ImportStats, MetadataStats, SystemCoverage,
};

/// Get metadata coverage stats.
#[server(prefix = "/sfn")]
pub async fn get_metadata_stats() -> Result<MetadataStats, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let guard = state
        .metadata_db()
        .ok_or_else(|| ServerFnError::new("Cannot open metadata DB"))?;
    let db = guard
        .as_ref()
        .ok_or_else(|| ServerFnError::new("Metadata DB not available"))?;
    db.stats().map_err(|e| ServerFnError::new(e.to_string()))
}

/// Start a background metadata import from a LaunchBox Metadata.xml file.
/// Returns immediately; poll `get_import_progress` for status.
#[server(prefix = "/sfn")]
pub async fn import_launchbox_metadata(xml_path: String) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let path = std::path::PathBuf::from(&xml_path);

    if !path.exists() {
        return Err(ServerFnError::new(format!("File not found: {xml_path}")));
    }

    if !state.start_import(path) {
        return Err(ServerFnError::new("An import is already running"));
    }

    tracing::info!("Started LaunchBox import from {xml_path}");
    Ok(())
}

/// Get current import progress (None if no import has been started).
#[server(prefix = "/sfn")]
pub async fn get_import_progress() -> Result<Option<ImportProgress>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let guard = state
        .import_progress
        .read()
        .expect("import_progress lock poisoned");
    Ok(guard.clone())
}

/// Get per-system metadata coverage stats.
#[server(prefix = "/sfn")]
pub async fn get_system_coverage() -> Result<Vec<SystemCoverage>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();

    // Get metadata entries per system from DB.
    let entries_per_system = {
        let guard = state
            .metadata_db()
            .ok_or_else(|| ServerFnError::new("Cannot open metadata DB"))?;
        let db = guard
            .as_ref()
            .ok_or_else(|| ServerFnError::new("Metadata DB not available"))?;
        db.entries_per_system()
            .map_err(|e| ServerFnError::new(e.to_string()))?
    };

    // Get total games per system from ROM cache.
    let storage = state.storage();
    let systems = state.cache.get_systems(&storage);

    let mut meta_map: std::collections::HashMap<String, usize> =
        entries_per_system.into_iter().collect();

    let mut coverage: Vec<SystemCoverage> = systems
        .into_iter()
        .filter(|s| s.game_count > 0)
        .map(|s| {
            let with_metadata = meta_map.remove(&s.folder_name).unwrap_or(0);
            SystemCoverage {
                system: s.folder_name,
                display_name: s.display_name,
                total_games: s.game_count,
                with_metadata,
            }
        })
        .collect();

    coverage.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    Ok(coverage)
}

/// Clear all cached metadata.
#[server(prefix = "/sfn")]
pub async fn clear_metadata() -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let guard = state
        .metadata_db()
        .ok_or_else(|| ServerFnError::new("Cannot open metadata DB"))?;
    let db = guard
        .as_ref()
        .ok_or_else(|| ServerFnError::new("Metadata DB not available"))?;
    db.clear().map_err(|e| ServerFnError::new(e.to_string()))
}

/// Clear metadata DB and trigger re-import from Metadata.xml.
/// The import runs in the background; poll `get_import_progress` for status.
#[server(prefix = "/sfn")]
pub async fn regenerate_metadata() -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    state.regenerate_metadata().map_err(ServerFnError::new)
}

/// Download LaunchBox metadata from the internet, extract, and import.
/// The entire process runs in the background; poll `get_import_progress` for status.
#[server(prefix = "/sfn")]
pub async fn download_metadata() -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    if !state.start_metadata_download() {
        return Err(ServerFnError::new(
            "A metadata operation is already running",
        ));
    }
    Ok(())
}
