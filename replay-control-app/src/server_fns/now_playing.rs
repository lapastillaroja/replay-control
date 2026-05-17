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
