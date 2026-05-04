use leptos::prelude::*;

use crate::i18n::{Key, t, use_i18n};
use crate::server_fns::{self, GameStatus};

/// Status selector component shown on the game detail page.
/// Accepts a shared `current_status` signal so other sections (e.g. achievements)
/// can also update the displayed status without a page reload.
#[component]
pub fn GameStatusSection(
    system: StoredValue<String>,
    rom_filename: StoredValue<String>,
    current_status: RwSignal<Option<GameStatus>>,
) -> impl IntoView {
    let i18n = use_i18n();

    let status_resource = Resource::new(
        move || (system.get_value(), rom_filename.get_value()),
        move |(sys, fname)| server_fns::get_game_status(sys, fname),
    );

    let _sync = Effect::new(move || {
        if let Some(Ok(status)) = status_resource.get() {
            current_status.set(status);
        }
    });

    let on_toggle_status = move |status: GameStatus| {
        let sys = system.get_value();
        let fname = rom_filename.get_value();
        if current_status.get_untracked() == Some(status) {
            current_status.set(None);
            leptos::task::spawn_local(async move {
                let _ = server_fns::clear_game_status(sys, fname).await;
            });
        } else {
            current_status.set(Some(status));
            leptos::task::spawn_local(async move {
                let _ = server_fns::set_game_status(sys, fname, status).await;
            });
        }
    };

    view! {
        <section class="section game-section">
            <h2 class="game-section-title">{move || t(i18n.locale.get(), Key::GameStatusTitle)}</h2>
            <div class="game-status-selector">
                <div class="game-status-buttons">
                    <button
                        class="game-status-btn"
                        class:active=move || current_status.get() == Some(GameStatus::WantToPlay)
                        on:click=move |_| on_toggle_status(GameStatus::WantToPlay)
                    >
                        "\u{1F4CB}"
                        {move || t(i18n.locale.get(), Key::GameStatusWantToPlay)}
                    </button>
                    <button
                        class="game-status-btn"
                        class:active=move || current_status.get() == Some(GameStatus::InProgress)
                        on:click=move |_| on_toggle_status(GameStatus::InProgress)
                    >
                        "\u{1F3AE}"
                        {move || t(i18n.locale.get(), Key::GameStatusInProgress)}
                    </button>
                    <button
                        class="game-status-btn"
                        class:active=move || current_status.get() == Some(GameStatus::Completed)
                        on:click=move |_| on_toggle_status(GameStatus::Completed)
                    >
                        "\u{2705}"
                        {move || t(i18n.locale.get(), Key::GameStatusCompleted)}
                    </button>
                    <Show when=move || current_status.get() == Some(GameStatus::Platinum)>
                        <span class="game-status-btn game-status-platinum-badge active">
                            "\u{1F3C6}"
                            {move || t(i18n.locale.get(), Key::GameStatusPlatinum)}
                        </span>
                    </Show>
                </div>
            </div>
        </section>
    }
}
