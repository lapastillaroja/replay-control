use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde::Deserialize;

use super::AppState;

async fn list_favorites(
    State(state): State<AppState>,
) -> Result<Json<Vec<replay_control_core::favorites::Favorite>>, StatusCode> {
    replay_control_core::favorites::list_favorites(&state.storage())
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn list_system_favorites(
    State(state): State<AppState>,
    Path(system): Path<String>,
) -> Result<Json<Vec<replay_control_core::favorites::Favorite>>, StatusCode> {
    replay_control_core::favorites::list_favorites_for_system(&state.storage(), &system)
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn add_favorite(
    State(state): State<AppState>,
    Json(payload): Json<AddFavoriteRequest>,
) -> Result<(StatusCode, Json<replay_control_core::favorites::Favorite>), StatusCode> {
    replay_control_core::favorites::add_favorite(
        &state.storage(),
        &payload.system,
        &payload.rom_path,
        payload.grouped,
    )
    .await
    .map(|fav| (StatusCode::CREATED, Json(fav)))
    .map_err(|_| StatusCode::CONFLICT)
}

async fn remove_favorite(
    State(state): State<AppState>,
    Json(payload): Json<RemoveFavoriteRequest>,
) -> Result<StatusCode, StatusCode> {
    replay_control_core::favorites::remove_favorite(
        &state.storage(),
        &payload.filename,
        payload.subfolder.as_deref(),
    )
    .map(|_| StatusCode::NO_CONTENT)
    .map_err(|_| StatusCode::NOT_FOUND)
}

async fn group_favorites(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    replay_control_core::favorites::group_by_system(&state.storage())
        .map(|count| Json(serde_json::json!({ "moved": count })))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn flatten_all_favorites(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    replay_control_core::favorites::flatten_favorites(&state.storage())
        .map(|count| Json(serde_json::json!({ "moved": count })))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn check_favorite(
    State(state): State<AppState>,
    Path((system, rom_filename)): Path<(String, String)>,
) -> Json<serde_json::Value> {
    let is_fav = replay_control_core::favorites::is_favorite(
        &state.storage(),
        &system,
        &rom_filename,
    )
    .await;
    Json(serde_json::json!({ "is_favorite": is_fav }))
}

#[derive(Deserialize)]
struct AddFavoriteRequest {
    system: String,
    rom_path: String,
    #[serde(default)]
    grouped: bool,
}

#[derive(Deserialize)]
struct RemoveFavoriteRequest {
    filename: String,
    subfolder: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/favorites", get(list_favorites))
        .route("/favorites", post(add_favorite))
        .route("/favorites", delete(remove_favorite))
        .route("/favorites/group", put(group_favorites))
        .route("/favorites/flatten", put(flatten_all_favorites))
        .route("/favorites/:system", get(list_system_favorites))
        .route(
            "/favorites/check/:system/:rom_filename",
            get(check_favorite),
        )
}
