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
                    system_display,
                    filename,
                    display_name,
                    started_at_unix_secs,
                    ..
                } => {
                    let elapsed = use_live_elapsed_secs(started_at_unix_secs);
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
                            <span class="now-playing-elapsed">
                                {move || format!(
                                    "{system_display} \u{00B7} {}",
                                    format_elapsed_short(elapsed.get()),
                                )}
                            </span>
                        </A>
                    }
                        .into_any()
                }
                _ => ().into_any(),
            }}
        </Show>
    }
}
