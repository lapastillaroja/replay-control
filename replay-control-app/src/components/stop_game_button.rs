use leptos::prelude::*;

use crate::i18n::{Key, t, use_i18n};
use crate::server_fns;
use crate::types::NowPlayingState;

#[component]
pub fn StopGameButton(class: &'static str) -> impl IntoView {
    let i18n = use_i18n();
    let stopping = RwSignal::new(false);
    let failed = RwSignal::new(false);
    let now_playing = use_context::<RwSignal<NowPlayingState>>();

    let on_stop = move |_| {
        if stopping.get() {
            return;
        }

        stopping.set(true);
        failed.set(false);

        leptos::task::spawn_local(async move {
            match server_fns::stop_current_game().await {
                Ok(_) => {
                    if let Some(now_playing) = now_playing {
                        now_playing.set(NowPlayingState::Menu);
                    }
                }
                Err(_) => failed.set(true),
            }
            stopping.set(false);
        });
    };

    let label = move || {
        let locale = i18n.locale.get();
        if stopping.get() {
            t(locale, Key::GameDetailStoppingGame)
        } else if failed.get() {
            t(locale, Key::GameDetailStopGameFailed)
        } else {
            t(locale, Key::GameDetailStopGame)
        }
    };

    view! {
        <button class=class prop:disabled=move || stopping.get() on:click=on_stop>
            <span class="game-action-icon">{"\u{25A0}"}</span>
            {label}
        </button>
    }
}
