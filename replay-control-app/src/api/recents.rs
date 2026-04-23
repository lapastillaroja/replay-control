use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};

use super::AppState;

async fn list_recents(
    State(state): State<AppState>,
) -> Result<Json<Vec<replay_control_core_server::recents::RecentEntry>>, StatusCode> {
    replay_control_core_server::recents::list_recents(&state.storage())
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn last_played(State(state): State<AppState>) -> Result<Json<serde_json::Value>, StatusCode> {
    let entry = replay_control_core_server::recents::last_played(&state.storage())
        .await
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
