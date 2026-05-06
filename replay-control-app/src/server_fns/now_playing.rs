use leptos::prelude::*;
use server_fn::ServerFnError;

use crate::types::NowPlayingState;

/// Read the current Now-Playing state directly from `AppState` so SSR can
/// render the live hero/header on the first frame and hydration can adopt
/// the same value without a flash.
///
/// After hydration, `SseNowPlayingListener` writes new states into the same
/// `Resource<NowPlayingState>` via `Resource::set`, keeping the resource as
/// the single source of truth for all consumers.
#[server(prefix = "/sfn")]
pub async fn get_initial_now_playing() -> Result<NowPlayingState, ServerFnError> {
    use crate::api::AppState;
    let state = expect_context::<AppState>();
    Ok(state.now_playing())
}
