//! Plain REST endpoints for the libretro core.
//!
//! These are lightweight Axum handlers — not Leptos server functions — so they
//! have stable, hash-free URLs that the core can call reliably.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use replay_control_core::metadata_db::MetadataDb;
use serde::Serialize;

use super::AppState;
use replay_control_core::game_ref::GameRef;

/// Minimal game entry returned by recents/favorites list endpoints.
/// Matches the shape expected by the libretro core's JSON parser.
#[derive(Serialize)]
struct CoreGameEntry {
    system: String,
    system_display: String,
    rom_filename: String,
    display_name: String,
    box_art_url: Option<String>,
}

/// Game detail returned by the detail endpoint.
/// Matches the shape expected by the libretro core's JSON parser.
#[derive(Serialize)]
struct CoreGameDetail {
    display_name: String,
    system_display: String,
    year: String,
    developer: String,
    genre: String,
    players: u8,
    rating: Option<f32>,
    description: Option<String>,
    publisher: Option<String>,
    region: Option<String>,
}

/// Batch-lookup box art URLs and convert a list of `GameRef`s into `CoreGameEntry`s.
async fn game_refs_to_core_entries(state: &AppState, games: Vec<GameRef>) -> Vec<CoreGameEntry> {
    let keys: Vec<(String, String)> = games
        .iter()
        .map(|g| (g.system.clone(), g.rom_filename.clone()))
        .collect();
    let db_entries = state
        .metadata_pool
        .read(move |conn| MetadataDb::lookup_game_entries(conn, &keys).unwrap_or_default())
        .await
        .unwrap_or_default();

    games
        .into_iter()
        .map(|g| {
            let box_art_url = db_entries
                .get(&(g.system.clone(), g.rom_filename.clone()))
                .and_then(|e| e.box_art_url.clone());
            CoreGameEntry {
                system: g.system,
                system_display: g.system_display,
                rom_filename: g.rom_filename.clone(),
                display_name: g.display_name.unwrap_or(g.rom_filename),
                box_art_url,
            }
        })
        .collect()
}

/// GET /api/core/recents — returns JSON array of recently played games.
async fn recents(State(state): State<AppState>) -> Result<Json<Vec<CoreGameEntry>>, StatusCode> {
    let storage = state.storage();
    let games: Vec<GameRef> = state
        .cache
        .get_recents(&storage)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .into_iter()
        .map(|e| e.game)
        .collect();

    Ok(Json(game_refs_to_core_entries(&state, games).await))
}

/// GET /api/core/favorites — returns JSON array of favorites.
async fn favorites(State(state): State<AppState>) -> Result<Json<Vec<CoreGameEntry>>, StatusCode> {
    let storage = state.storage();
    let games: Vec<GameRef> = replay_control_core::favorites::list_favorites(&storage)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .into_iter()
        .map(|f| f.game)
        .collect();

    Ok(Json(game_refs_to_core_entries(&state, games).await))
}

/// GET /api/core/game/:system/:filename — returns JSON game detail.
async fn game_detail(
    State(state): State<AppState>,
    Path((system, filename)): Path<(String, String)>,
) -> Result<Json<CoreGameDetail>, StatusCode> {
    let sys_owned = system.clone();
    let fname_owned = filename.clone();
    let entry = state
        .metadata_pool
        .read(move |conn| MetadataDb::load_single_entry(conn, &sys_owned, &fname_owned))
        .await
        .and_then(|r| r.ok())
        .flatten()
        .ok_or(StatusCode::NOT_FOUND)?;

    let game = crate::server_fns::build_game_detail(&state, &entry).await;

    Ok(Json(CoreGameDetail {
        display_name: game.display_name,
        system_display: game.system_display,
        year: game.year,
        developer: game.developer,
        genre: game.genre,
        players: game.players,
        rating: game.rating,
        description: game.description,
        publisher: game.publisher,
        region: game.region,
    }))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/recents", get(recents))
        .route("/favorites", get(favorites))
        .route("/game/:system/:filename", get(game_detail))
}
