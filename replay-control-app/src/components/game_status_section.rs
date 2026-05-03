use leptos::prelude::*;

use crate::i18n::{Key, t, use_i18n};
use crate::server_fns::{self, GameStatus};

/// Status selector component shown on the game detail page.
#[component]
pub fn GameStatusSection(
    system: StoredValue<String>,
    rom_filename: StoredValue<String>,
) -> impl IntoView {
    let i18n = use_i18n();

    let status_resource = Resource::new(
        move || (system.get_value(), rom_filename.get_value()),
        move |(sys, fname)| server_fns::get_game_status(sys, fname),
    );

    let current_status = RwSignal::new(Option::<GameStatus>::None);

    let _sync = Effect::new(move || {
        if let Some(Ok(status)) = status_resource.get() {
            current_status.set(status);
        }
    });

    let on_set_status = move |status: GameStatus| {
        let sys = system.get_value();
        let fname = rom_filename.get_value();
        current_status.set(Some(status));
        leptos::task::spawn_local(async move {
            let _ = server_fns::set_game_status(sys, fname, status).await;
        });
    };

    let on_clear = move |_| {
        let sys = system.get_value();
        let fname = rom_filename.get_value();
        current_status.set(None);
        leptos::task::spawn_local(async move {
            let _ = server_fns::clear_game_status(sys, fname).await;
        });
    };

    let status_label = move || match current_status.get() {
        Some(GameStatus::WantToPlay) => t(i18n.locale.get(), Key::GameStatusWantToPlay),
        Some(GameStatus::InProgress) => t(i18n.locale.get(), Key::GameStatusInProgress),
        Some(GameStatus::Completed) => t(i18n.locale.get(), Key::GameStatusCompleted),
        Some(GameStatus::Platinum) => t(i18n.locale.get(), Key::GameStatusPlatinum),
        None => t(i18n.locale.get(), Key::GameStatusNone),
    };

    let status_icon = move || match current_status.get() {
        Some(GameStatus::WantToPlay) => "\u{1F4CB}",
        Some(GameStatus::InProgress) => "\u{1F3AE}",
        Some(GameStatus::Completed) => "\u{2705}",
        Some(GameStatus::Platinum) => "\u{1F3C6}",
        None => "\u{2753}",
    };

    view! {
        <section class="section game-section">
            <h2 class="game-section-title">{move || t(i18n.locale.get(), Key::GameStatusTitle)}</h2>
            <div class="game-status-selector">
                <div class="game-status-current">
                    <span class="game-status-icon">{status_icon}</span>
                    <span class="game-status-label">{status_label}</span>
                </div>
                <div class="game-status-buttons">
                    <button
                        class="game-status-btn"
                        class:active=move || current_status.get() == Some(GameStatus::WantToPlay)
                        on:click=move |_| on_set_status(GameStatus::WantToPlay)
                    >
                        "\u{1F4CB}"
                        {move || t(i18n.locale.get(), Key::GameStatusWantToPlay)}
                    </button>
                    <button
                        class="game-status-btn"
                        class:active=move || current_status.get() == Some(GameStatus::InProgress)
                        on:click=move |_| on_set_status(GameStatus::InProgress)
                    >
                        "\u{1F3AE}"
                        {move || t(i18n.locale.get(), Key::GameStatusInProgress)}
                    </button>
                    <button
                        class="game-status-btn"
                        class:active=move || current_status.get() == Some(GameStatus::Completed)
                        on:click=move |_| on_set_status(GameStatus::Completed)
                    >
                        "\u{2705}"
                        {move || t(i18n.locale.get(), Key::GameStatusCompleted)}
                    </button>
                    <button
                        class="game-status-btn"
                        class:active=move || current_status.get() == Some(GameStatus::Platinum)
                        on:click=move |_| on_set_status(GameStatus::Platinum)
                    >
                        "\u{1F3C6}"
                        {move || t(i18n.locale.get(), Key::GameStatusPlatinum)}
                    </button>
                    <Show when=move || current_status.get().is_some()>
                        <button
                            class="game-status-btn game-status-clear"
                            on:click=on_clear
                        >
                            {move || t(i18n.locale.get(), Key::GameStatusClear)}
                        </button>
                    </Show>
                </div>
            </div>
        </section>
    }
}
