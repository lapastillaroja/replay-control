use leptos::prelude::*;
use leptos_router::components::A;

use crate::hooks::{use_live_elapsed_secs, use_now_playing};
use crate::types::NowPlayingState;
use crate::util::format_elapsed_short;

#[component]
pub fn NowPlayingIndicator() -> impl IntoView {
    let now_playing = use_now_playing();

    view! {
        <Show
            when=move || matches!(now_playing.get(), NowPlayingState::Playing { .. })
            fallback=|| ()
        >
            {move || match now_playing.get() {
                NowPlayingState::Playing {
                    system,
                    filename,
                    display_name,
                    started_at_unix_secs,
                    ..
                } => {
                    let elapsed = use_live_elapsed_secs(started_at_unix_secs);
                    // Memo so the 1 Hz clock signal can't propagate to the text
                    // node every second — only fires the subscriber when the
                    // minute-granularity output actually changes.
                    let elapsed_text = Memo::new(move |_| {
                        format_elapsed_short(elapsed.get()).unwrap_or_default()
                    });
                    let title = display_name.clone();
                    let href = format!(
                        "/games/{}/{}",
                        urlencoding::encode(&system),
                        urlencoding::encode(&filename),
                    );
                    view! {
                        <A href=href attr:class="now-playing-indicator" attr:title=title>
                            <span class="now-playing-dot" aria-hidden="true"></span>
                            <span class="now-playing-name">{display_name}</span>
                            <span class="now-playing-elapsed">{move || elapsed_text.get()}</span>
                        </A>
                    }
                        .into_any()
                }
                _ => ().into_any(),
            }}
        </Show>
    }
}
