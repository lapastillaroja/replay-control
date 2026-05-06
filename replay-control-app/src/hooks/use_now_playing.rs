use leptos::prelude::*;

use crate::types::NowPlayingState;

/// Reactive view of the now-playing state, sourced from the `Resource`
/// provided at the App root. Falls back to `NotRunning` while the resource
/// is pending (rare — `Resource::new_blocking` resolves before SSR output).
pub fn use_now_playing() -> Signal<NowPlayingState> {
    let resource = expect_context::<Resource<NowPlayingState>>();
    Signal::derive(move || resource.get().unwrap_or(NowPlayingState::NotRunning))
}
