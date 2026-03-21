//! Plain REST endpoints for the libretro core.
//!
//! These are lightweight Axum handlers — not Leptos server functions — so they
//! have stable, hash-free URLs that the core can call reliably.

use axum::extract::{Path, State};
use replay_control_core::metadata_db::MetadataDb;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

use super::AppState;

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

/// GET /api/core/recents — returns JSON array of recently played games.
async fn recents(State(state): State<AppState>) -> Result<Json<Vec<CoreGameEntry>>, StatusCode> {
    let storage = state.storage();
    let entries = state
        .cache
        .get_recents(&storage)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Build image indexes per-system (typically only a few distinct systems).
    let mut image_indexes: std::collections::HashMap<
        String,
        std::sync::Arc<crate::api::cache::ImageIndex>,
    > = std::collections::HashMap::new();

    let result = entries
        .into_iter()
        .map(|entry| {
            let index = image_indexes
                .entry(entry.game.system.clone())
                .or_insert_with(|| state.cache.get_image_index(&state, &entry.game.system));
            let box_art_url = state.cache.resolve_box_art(
                &state,
                index,
                &entry.game.system,
                &entry.game.rom_filename,
            );
            CoreGameEntry {
                system: entry.game.system,
                system_display: entry.game.system_display,
                rom_filename: entry.game.rom_filename.clone(),
                display_name: entry
                    .game
                    .display_name
                    .unwrap_or_else(|| entry.game.rom_filename),
                box_art_url,
            }
        })
        .collect();

    Ok(Json(result))
}

/// GET /api/core/favorites — returns JSON array of favorites.
async fn favorites(State(state): State<AppState>) -> Result<Json<Vec<CoreGameEntry>>, StatusCode> {
    let storage = state.storage();
    let favs = replay_control_core::favorites::list_favorites(&storage)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut image_indexes: std::collections::HashMap<
        String,
        std::sync::Arc<crate::api::cache::ImageIndex>,
    > = std::collections::HashMap::new();

    let result = favs
        .into_iter()
        .map(|fav| {
            let index = image_indexes
                .entry(fav.game.system.clone())
                .or_insert_with(|| state.cache.get_image_index(&state, &fav.game.system));
            let box_art_url = state.cache.resolve_box_art(
                &state,
                index,
                &fav.game.system,
                &fav.game.rom_filename,
            );
            CoreGameEntry {
                system: fav.game.system,
                system_display: fav.game.system_display,
                rom_filename: fav.game.rom_filename.clone(),
                display_name: fav
                    .game
                    .display_name
                    .unwrap_or_else(|| fav.game.rom_filename),
                box_art_url,
            }
        })
        .collect();

    Ok(Json(result))
}

/// GET /api/core/game/:system/:filename — returns JSON game detail.
async fn game_detail(
    State(state): State<AppState>,
    Path((system, filename)): Path<(String, String)>,
) -> Result<Json<CoreGameDetail>, StatusCode> {
    use replay_control_core::arcade_db;
    use replay_control_core::game_db;
    use replay_control_core::rom_tags;
    use replay_control_core::systems::{self, SystemCategory};

    let storage = state.storage();

    // Verify the ROM exists in the library.
    let all_roms = state
        .cache
        .get_roms(
            &storage,
            &system,
            state.region_preference(),
            state.region_preference_secondary(),
        )
        .map_err(|_| StatusCode::NOT_FOUND)?;

    all_roms
        .iter()
        .find(|r| r.game.rom_filename == filename)
        .ok_or(StatusCode::NOT_FOUND)?;

    // Resolve base metadata from baked-in databases (arcade_db / game_db).
    let sys_info = systems::find_system(&system);
    let system_display = sys_info
        .map(|s| s.display_name.to_string())
        .unwrap_or_else(|| system.clone());
    let is_arcade = sys_info.is_some_and(|s| s.category == SystemCategory::Arcade);

    let (display_name, year, genre, developer, players, region) = if is_arcade {
        let stem = filename.strip_suffix(".zip").unwrap_or(&filename);
        match arcade_db::lookup_arcade_game(stem) {
            Some(info) => (
                info.display_name.to_string(),
                info.year.to_string(),
                if info.category.is_empty() {
                    info.normalized_genre.to_string()
                } else {
                    info.category.to_string()
                },
                info.manufacturer.to_string(),
                info.players,
                None,
            ),
            None => (filename.clone(), String::new(), String::new(), String::new(), 0, None),
        }
    } else {
        let stem = filename.rfind('.').map(|i| &filename[..i]).unwrap_or(&filename);
        let entry = game_db::lookup_game(&system, stem);
        let game = entry.map(|e| e.game);
        let region = entry.map(|e| e.region).unwrap_or("");

        let display_name = if let Some(g) = game {
            rom_tags::display_name_with_tags(g.display_name, &filename)
        } else if let Some(dn) = game_db::game_display_name(&system, &filename) {
            rom_tags::display_name_with_tags(dn, &filename)
        } else {
            let stem = filename.rfind('.').map(|i| &filename[..i]).unwrap_or(&filename);
            let base = stem
                .find(" (")
                .or_else(|| stem.find(" ["))
                .map(|i| stem[..i].trim())
                .unwrap_or(stem);
            let name = if base.is_empty() { stem } else { base };
            rom_tags::display_name_with_tags(name, &filename)
        };

        let game_meta = game.or_else(|| {
            let normalized = game_db::normalize_filename(stem);
            game_db::lookup_by_normalized_title(&system, &normalized)
        });

        (
            display_name,
            game_meta
                .map(|g| if g.year > 0 { g.year.to_string() } else { String::new() })
                .unwrap_or_default(),
            game_meta
                .map(|g| {
                    if g.genre.is_empty() { g.normalized_genre } else { g.genre }.to_string()
                })
                .unwrap_or_default(),
            game_meta
                .map(|g| g.developer.to_string())
                .unwrap_or_default(),
            game_meta.map(|g| g.players).unwrap_or(0),
            if region.is_empty() { None } else { Some(region.to_string()) },
        )
    };

    // Enrich from metadata DB (description, rating, publisher).
    let (mut description, mut rating, mut publisher, mut enriched_genre, mut enriched_developer) =
        (None, None, None, String::new(), String::new());

    if let Some(guard) = state.metadata_db()
        && let Some(db) = guard.as_ref()
    {
        if let Ok(Some(meta)) = MetadataDb::lookup(db, &system, &filename) {
            description = meta.description;
            rating = meta.rating.map(|r| r as f32);
            publisher = meta.publisher;
            if meta.developer.is_some() {
                enriched_developer = meta.developer.unwrap_or_default();
            }
            if meta.genre.is_some() {
                enriched_genre = meta.genre.unwrap_or_default();
            }
        }
    }

    Ok(Json(CoreGameDetail {
        display_name,
        system_display,
        year,
        developer: if developer.is_empty() && !enriched_developer.is_empty() {
            enriched_developer
        } else {
            developer
        },
        genre: if genre.is_empty() && !enriched_genre.is_empty() {
            enriched_genre
        } else {
            genre
        },
        players,
        rating,
        description,
        publisher,
        region,
    }))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/recents", get(recents))
        .route("/favorites", get(favorites))
        .route("/game/:system/:filename", get(game_detail))
}
