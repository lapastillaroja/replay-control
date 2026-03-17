use axum::extract::{Multipart, Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};

use super::AppState;

async fn upload_rom(
    State(state): State<AppState>,
    Path(system): Path<String>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let system_dir = state.storage().system_roms_dir(&system);
    if !system_dir.exists() {
        std::fs::create_dir_all(&system_dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    let mut uploaded = Vec::new();

    while let Ok(Some(field)) = multipart.next_field().await {
        let filename = field
            .file_name()
            .map(String::from)
            .unwrap_or_else(|| "unknown".to_string());

        // TODO(perf): This loads the entire file into memory. For large ROMs,
        // stream to a temp file instead (e.g., via tokio::io::copy from the
        // field stream to a tokio::fs::File). Acceptable for now since uploads
        // are rare and typically single files.
        let data = field.bytes().await.map_err(|_| StatusCode::BAD_REQUEST)?;

        let dest = system_dir.join(&filename);
        tokio::fs::write(&dest, &data)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        tracing::info!("Uploaded: {} ({} bytes)", filename, data.len());
        uploaded.push(serde_json::json!({
            "filename": filename,
            "size_bytes": data.len(),
            "path": format!("/roms/{system}/{filename}"),
        }));
    }

    if !uploaded.is_empty() {
        state.cache.invalidate_system(&system);
    }

    Ok(Json(serde_json::json!({ "uploaded": uploaded })))
}

async fn list_upload_targets(State(state): State<AppState>) -> Json<Vec<serde_json::Value>> {
    let summaries = state.cache.get_systems(&state.storage());
    Json(
        summaries
            .into_iter()
            .map(|s| {
                serde_json::json!({
                    "folder_name": s.folder_name,
                    "display_name": s.display_name,
                    "game_count": s.game_count,
                })
            })
            .collect(),
    )
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/upload/:system", post(upload_rom))
        .route("/upload/targets", get(list_upload_targets))
}
