use super::*;
#[cfg(feature = "ssr")]
use replay_control_core_server::library_db::LibraryDb;
#[cfg(feature = "ssr")]
use replay_control_core_server::user_data_db::UserDataDb;
#[cfg(feature = "ssr")]
use replay_control_core::user_data_db::{GameStatus, StatusGameEntry};

#[server(prefix = "/sfn")]
pub async fn get_game_status(
    system: String,
    rom_filename: String,
) -> Result<Option<GameStatus>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let result = state
        .user_data_pool
        .read(move |conn| UserDataDb::get_game_status(conn, &system, &rom_filename))
        .await;
    match result {
        Some(Ok(status)) => Ok(status),
        Some(Err(e)) => Err(ServerFnError::new(e.to_string())),
        None => Ok(None),
    }
}

#[server(prefix = "/sfn")]
pub async fn set_game_status(
    system: String,
    rom_filename: String,
    status: GameStatus,
) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let result = state
        .user_data_pool
        .write(move |conn| UserDataDb::set_game_status(conn, &system, &rom_filename, status))
        .await;
    match result {
        Some(Ok(())) => Ok(()),
        Some(Err(e)) => Err(ServerFnError::new(e.to_string())),
        None => Ok(()),
    }
}

#[server(prefix = "/sfn")]
pub async fn clear_game_status(
    system: String,
    rom_filename: String,
) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let result = state
        .user_data_pool
        .write(move |conn| UserDataDb::clear_game_status(conn, &system, &rom_filename))
        .await;
    match result {
        Some(Ok(())) => Ok(()),
        Some(Err(e)) => Err(ServerFnError::new(e.to_string())),
        None => Ok(()),
    }
}

#[server(prefix = "/sfn")]
pub async fn get_games_by_status(status: GameStatus) -> Result<Vec<StatusGameEntry>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();

    let games_result = state
        .user_data_pool
        .read(move |conn| UserDataDb::get_games_by_status(conn, status))
        .await;

    let games = match games_result {
        Some(Ok(g)) => g,
        Some(Err(e)) => return Err(ServerFnError::new(e.to_string())),
        None => return Ok(Vec::new()),
    };

    if games.is_empty() {
        return Ok(Vec::new());
    }

    let keys: Vec<(String, String)> = games
        .iter()
        .map(|(sys, fname, _)| (sys.clone(), fname.clone()))
        .collect();

    let enriched = state
        .library_pool
        .read(move |conn| {
            let entries = LibraryDb::lookup_game_entries(conn, &keys).unwrap_or_default();
            let mut result = Vec::new();
            for (system, rom_filename, updated_at) in &games {
                if let Some(e) = entries.get(&(system.clone(), rom_filename.clone())) {
                    let genre = if e.genre_group.is_empty() {
                        None
                    } else {
                        Some(e.genre_group.clone())
                    };
                    result.push(StatusGameEntry {
                        system: system.clone(),
                        rom_filename: rom_filename.clone(),
                        display_name: e
                            .display_name
                            .clone()
                            .unwrap_or_else(|| rom_filename.clone()),
                        status,
                        box_art_url: e.box_art_url.clone(),
                        genre,
                        updated_at: *updated_at,
                    });
                }
            }
            result
        })
        .await
        .unwrap_or_default();

    Ok(enriched)
}

#[server(prefix = "/sfn")]
pub async fn get_status_counts() -> Result<std::collections::HashMap<GameStatus, usize>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let result = state
        .user_data_pool
        .read(move |conn| UserDataDb::get_status_counts(conn))
        .await;
    match result {
        Some(Ok(counts)) => Ok(counts),
        Some(Err(e)) => Err(ServerFnError::new(e.to_string())),
        None => Ok(std::collections::HashMap::new()),
    }
}
