use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};

use super::AppState;

async fn list_recents(
    State(state): State<AppState>,
) -> Result<Json<Vec<replay_control_core::recents::RecentEntry>>, StatusCode> {
    replay_control_core::recents::list_recents(&state.storage())
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn last_played(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let entry = replay_control_core::recents::last_played(&state.storage())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match entry {
        Some(e) => Ok(Json(serde_json::to_value(e).unwrap())),
        None => Ok(Json(serde_json::json!(null))),
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/recents", get(list_recents))
        .route("/recents/last", get(last_played))
}
