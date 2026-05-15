use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

use super::AppState;

#[derive(Serialize)]
struct SystemInfo {
    storage_kind: String,
    storage_root: String,
    disk_total_bytes: u64,
    disk_used_bytes: u64,
    disk_available_bytes: u64,
    total_systems: usize,
    systems_with_games: usize,
    total_games: usize,
    total_favorites: usize,
}

async fn get_system_info(State(state): State<AppState>) -> Json<SystemInfo> {
    let storage = state.storage();
    let system_meta = state
        .library_reader
        .read(replay_control_core_server::library_db::LibraryDb::load_all_system_meta)
        .await
        .and_then(Result::ok)
        .unwrap_or_default();
    let favorites = replay_control_core_server::favorites::list_favorites(&storage)
        .await
        .unwrap_or_default();

    let disk =
        storage
            .disk_usage()
            .await
            .unwrap_or(replay_control_core_server::storage::DiskUsage {
                total_bytes: 0,
                available_bytes: 0,
                used_bytes: 0,
            });

    let systems_with_games = system_meta.iter().filter(|s| s.rom_count > 0).count();
    let total_games: usize = system_meta.iter().map(|s| s.rom_count).sum();

    Json(SystemInfo {
        storage_kind: format!("{:?}", storage.kind).to_lowercase(),
        storage_root: storage.root.display().to_string(),
        disk_total_bytes: disk.total_bytes,
        disk_used_bytes: disk.used_bytes,
        disk_available_bytes: disk.available_bytes,
        total_systems: replay_control_core::systems::visible_systems().count(),
        systems_with_games,
        total_games,
        total_favorites: favorites.len(),
    })
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/info", get(get_system_info))
}
