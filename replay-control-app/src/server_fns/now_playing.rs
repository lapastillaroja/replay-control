use leptos::prelude::*;
use server_fn::ServerFnError;

use crate::types::NowPlayingState;

/// Read the current Now-Playing state directly from `AppState`.
///
/// Hydrated UI primarily uses `/sse/now-playing`, whose first event contains
/// this same current state.
#[server(prefix = "/sfn")]
pub async fn get_initial_now_playing() -> Result<NowPlayingState, ServerFnError> {
    use crate::api::AppState;
    let state = expect_context::<AppState>();
    Ok(state.now_playing())
}

/// Stop the currently loaded game by restarting the RePlayOS frontend service.
///
/// RePlayOS does not currently expose a narrower "unload game" command, so a
/// service restart is the explicit stop path used by the UI.
#[server(prefix = "/sfn")]
pub async fn stop_current_game() -> Result<String, ServerFnError> {
    use crate::api::AppState;

    let state = expect_context::<AppState>();

    if !crate::server_fns::is_replayos() {
        state.set_now_playing(NowPlayingState::Menu);
        return Ok("Stop simulated (not running on ReplayOS)".to_string());
    }

    // Clear the autostart file first, otherwise the restart re-reads a
    // not-yet-cleaned-up launch and relaunches the same game.
    replay_control_core_server::launch::clear_autostart(&state.storage())
        .map_err(|e| ServerFnError::new(format!("Failed to clear autostart: {e}")))?;

    replay_control_core_server::replay_service::restart_async()
        .await
        .map_err(|e| ServerFnError::new(format!("Failed to stop game: {e}")))?;
    state.set_now_playing(NowPlayingState::Menu);
    Ok("Game stopped".to_string())
}
