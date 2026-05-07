use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use super::AppState;

async fn list_systems(
    State(state): State<AppState>,
) -> Json<Vec<replay_control_core_server::roms::SystemSummary>> {
    Json(
        state
            .cache
            .cached_systems(&state.storage(), &state.library_reader)
            .await,
    )
}

async fn list_system_roms(
    State(state): State<AppState>,
    Path(system): Path<String>,
) -> Json<Vec<replay_control_core_server::roms::RomEntry>> {
    let storage = state.storage();

    // L2 only: this is a GET handler, so it stays read-only. A miss
    // returns an empty list and lets the background pipeline populate
    // L2 — request handlers used to fall through to a full L3 scan
    // here, which kicked off enrichment writes (TGDB aliases, Wikidata
    // series, release-date seeding) from a GET. That was the second
    // half of the cold-NFS poisoning vector traced in the
    // write-isolation investigation.
    if let Some(roms) = state
        .cache
        .load_roms_from_db(
            &storage,
            &system,
            &storage.roms_dir().join(&system),
            &state.library_reader,
        )
        .await
    {
        return Json(roms);
    }

    Json(Vec::new())
}

async fn delete_rom(
    State(state): State<AppState>,
    Json(payload): Json<DeleteRomRequest>,
) -> Result<StatusCode, StatusCode> {
    replay_control_core_server::roms::delete_rom_group(
        &state.storage(),
        &payload.system,
        &payload.relative_path,
    )
    .map_err(|_| StatusCode::NOT_FOUND)?;
    if let Err(e) = state.cache.invalidate(&state.library_writer).await {
        tracing::debug!("post-mutation cache.invalidate skipped: {e}");
    }
    state.invalidate_user_caches().await;
    Ok(StatusCode::NO_CONTENT)
}

async fn rename_rom(
    State(state): State<AppState>,
    Json(payload): Json<RenameRomRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let new_path = replay_control_core_server::roms::rename_rom(
        &state.storage(),
        &payload.relative_path,
        &payload.new_filename,
    )
    .map_err(|_| StatusCode::NOT_FOUND)?;
    if let Err(e) = state.cache.invalidate(&state.library_writer).await {
        tracing::debug!("post-mutation cache.invalidate skipped: {e}");
    }
    state.invalidate_user_caches().await;
    Ok(Json(serde_json::json!({
        "new_path": new_path.display().to_string()
    })))
}

async fn find_duplicates(State(state): State<AppState>) -> Json<Vec<DuplicateResponse>> {
    let dupes = replay_control_core_server::roms::find_duplicates(&state.storage()).await;
    Json(
        dupes
            .into_iter()
            .map(|(a, b)| DuplicateResponse {
                original: a.game.rom_path,
                duplicate: b.game.rom_path,
                filename: a.game.rom_filename,
                size_bytes: a.size_bytes,
            })
            .collect(),
    )
}

#[derive(Deserialize)]
struct DeleteRomRequest {
    system: String,
    relative_path: String,
}

#[derive(Deserialize)]
struct RenameRomRequest {
    relative_path: String,
    new_filename: String,
}

#[derive(Serialize)]
struct DuplicateResponse {
    original: String,
    duplicate: String,
    filename: String,
    size_bytes: u64,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/systems", get(list_systems))
        .route("/systems/:system/roms", get(list_system_roms))
        .route("/roms", delete(delete_rom))
        .route("/roms/rename", axum::routing::put(rename_rom))
        .route("/roms/duplicates", get(find_duplicates))
}
