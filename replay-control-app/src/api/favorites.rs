use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde::Deserialize;

use super::AppState;

async fn list_favorites(
    State(state): State<AppState>,
) -> Result<Json<Vec<replay_control_core_server::favorites::Favorite>>, StatusCode> {
    replay_control_core_server::favorites::list_favorites(&state.storage())
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn list_system_favorites(
    State(state): State<AppState>,
    Path(system): Path<String>,
) -> Result<Json<Vec<replay_control_core_server::favorites::Favorite>>, StatusCode> {
    replay_control_core_server::favorites::list_favorites_for_system(&state.storage(), &system)
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn add_favorite(
    State(state): State<AppState>,
    Json(payload): Json<AddFavoriteRequest>,
) -> Result<
    (
        StatusCode,
        Json<replay_control_core_server::favorites::Favorite>,
    ),
    StatusCode,
> {
    state
        .require_storage_ready_or_conflict("add favorites")
        .await?;
    let fav = replay_control_core_server::favorites::add_favorite(
        &state.storage(),
        &payload.system,
        &payload.rom_path,
        payload.grouped,
    )
    .await
    .map_err(|_| StatusCode::CONFLICT)?;
    invalidate_favorites(&state).await;
    Ok((StatusCode::CREATED, Json(fav)))
}

async fn remove_favorite(
    State(state): State<AppState>,
    Json(payload): Json<RemoveFavoriteRequest>,
) -> Result<StatusCode, StatusCode> {
    state
        .require_storage_ready_or_conflict("remove favorites")
        .await?;
    // Match the server-fn semantics: with no subfolder given, the same `.fav`
    // may exist in multiple locations after reorganization, so remove it
    // everywhere rather than only from the root.
    match payload.subfolder.as_deref() {
        Some(sub) if !sub.is_empty() => {
            replay_control_core_server::favorites::remove_favorite(
                &state.storage(),
                &payload.filename,
                Some(sub),
            )
            .map_err(|_| StatusCode::NOT_FOUND)?;
        }
        _ => {
            replay_control_core_server::favorites::remove_favorite_everywhere(
                &state.storage(),
                &payload.filename,
            )
            .map_err(|_| StatusCode::NOT_FOUND)?;
        }
    }
    invalidate_favorites(&state).await;
    Ok(StatusCode::NO_CONTENT)
}

async fn group_favorites(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    state
        .require_storage_ready_or_conflict("group favorites")
        .await?;
    let count = replay_control_core_server::favorites::group_by_system(&state.storage())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    invalidate_favorites(&state).await;
    Ok(Json(serde_json::json!({ "moved": count })))
}

async fn flatten_all_favorites(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    state
        .require_storage_ready_or_conflict("flatten favorites")
        .await?;
    let count = replay_control_core_server::favorites::flatten_favorites(&state.storage())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    invalidate_favorites(&state).await;
    Ok(Json(serde_json::json!({ "moved": count })))
}

/// Drop the cached favorites list and per-user caches after a favorites
/// mutation, matching what the `/sfn` favorites handlers do — otherwise an
/// API-driven change serves stale favorites/recommendations until TTL.
async fn invalidate_favorites(state: &AppState) {
    state.library.invalidate_favorites().await;
    state.invalidate_user_caches().await;
}

async fn check_favorite(
    State(state): State<AppState>,
    Path((system, rom_filename)): Path<(String, String)>,
) -> Json<serde_json::Value> {
    let is_fav = replay_control_core_server::favorites::is_favorite(
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
        .route("/favorites/{system}", get(list_system_favorites))
        .route(
            "/favorites/check/{system}/{rom_filename}",
            get(check_favorite),
        )
}
