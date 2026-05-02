use super::*;
#[cfg(feature = "ssr")]
use replay_control_core_server::user_data_db::UserDataDb;

#[server(prefix = "/sfn")]
pub async fn get_game_note(
    system: String,
    rom_filename: String,
) -> Result<Option<(String, u64)>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let result = state
        .user_data_pool
        .read(move |conn| UserDataDb::get_game_note(conn, &system, &rom_filename))
        .await;
    match result {
        Some(Ok(note)) => Ok(note),
        Some(Err(e)) => Err(ServerFnError::new(e.to_string())),
        None => Ok(None),
    }
}

#[server(prefix = "/sfn")]
pub async fn set_game_note(
    system: String,
    rom_filename: String,
    note: String,
) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let result = state
        .user_data_pool
        .write(move |conn| UserDataDb::set_game_note(conn, &system, &rom_filename, &note))
        .await;
    match result {
        Some(Ok(())) => Ok(()),
        Some(Err(e)) => Err(ServerFnError::new(e.to_string())),
        None => Ok(()),
    }
}

#[server(prefix = "/sfn")]
pub async fn clear_game_note(
    system: String,
    rom_filename: String,
) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let result = state
        .user_data_pool
        .write(move |conn| UserDataDb::clear_game_note(conn, &system, &rom_filename))
        .await;
    match result {
        Some(Ok(())) => Ok(()),
        Some(Err(e)) => Err(ServerFnError::new(e.to_string())),
        None => Ok(()),
    }
}
