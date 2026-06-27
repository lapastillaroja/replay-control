use leptos::prelude::*;

use crate::types::NowPlayingState;

#[cfg(target_arch = "wasm32")]
thread_local! {
    static CLIENT_NOW_PLAYING: std::cell::Cell<Option<RwSignal<NowPlayingState>>> =
        const { std::cell::Cell::new(None) };
}

/// Provides the now-playing signal to the component tree.
///
/// On the client, cache the root signal so components created after async
/// boundaries can still subscribe even when their owner chain cannot see App's
/// context directly.
pub fn provide_now_playing(initial: NowPlayingState) {
    let now_playing = RwSignal::new(initial);
    provide_context(now_playing);
    #[cfg(target_arch = "wasm32")]
    CLIENT_NOW_PLAYING.with(|cell| cell.set(Some(now_playing)));
}

/// The now-playing `RwSignal` provided at the App root.
///
/// Returns the raw `RwSignal` (not a derived `Signal`): it is reactive via
/// `.get()`/`.read()` for views, and supports `.get_untracked()` for one-shot
/// reads inside event handlers. The untracked read has no reactive-owner
/// dependency, so handlers reading it survive an iOS Safari back/forward-cache
/// restore — a derived/owner-bound read silently aborts the handler after back.
pub fn use_now_playing() -> RwSignal<NowPlayingState> {
    #[cfg(target_arch = "wasm32")]
    {
        use_context::<RwSignal<NowPlayingState>>()
            .or_else(|| CLIENT_NOW_PLAYING.with(|cell| cell.get()))
            .expect("now-playing not initialized: provide_now_playing() must run at the App root")
    }

    #[cfg(not(target_arch = "wasm32"))]
    expect_context::<RwSignal<NowPlayingState>>()
}
