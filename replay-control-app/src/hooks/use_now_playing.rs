use leptos::prelude::*;

use crate::types::NowPlayingState;

/// Reactive view of the now-playing signal provided at the App root.
pub fn use_now_playing() -> Signal<NowPlayingState> {
    let now_playing = expect_context::<RwSignal<NowPlayingState>>();
    Signal::derive(move || now_playing.get())
}
