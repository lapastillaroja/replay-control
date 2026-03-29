use leptos::prelude::*;
use leptos_router::components::A;

use crate::i18n::{t, use_i18n};
use crate::server_fns::SystemSummary;
use crate::util::format_size;

#[component]
pub fn SystemCard(system: SystemSummary, href: String) -> impl IntoView {
    let i18n = use_i18n();
    let has_games = system.game_count > 0;
    let size_display = format_size(system.total_size_bytes);
    let game_count = system.game_count;

    let card_class = if has_games {
        "system-card"
    } else {
        "system-card empty"
    };

    let icon = match system.category.as_str() {
        "arcade" => "\u{1F579}\u{FE0F}", // 🕹️
        "console" => "\u{1F3AE}",         // 🎮
        "handheld" => "\u{1F4F1}",        // 📱
        "computer" => "\u{1F4BB}",        // 💻
        _ => "\u{1F3AE}",                 // 🎮 default
    };

    view! {
        <A href=href attr:class=card_class>
            <div class="system-card-name"><span class="system-card-icon">{icon}</span>{system.display_name.clone()}</div>
            <div class="system-card-manufacturer">{system.manufacturer.clone()}</div>
            <div class="system-card-count">
                {move || {
                    let locale = i18n.locale.get();
                    if has_games {
                        format!("{} {}", game_count, t(locale, "stats.games").to_lowercase())
                    } else {
                        t(locale, "games.no_games").to_string()
                    }
                }}
            </div>
            <Show when=move || has_games>
                <div class="system-card-size">{size_display.clone()}</div>
            </Show>
        </A>
    }
}
