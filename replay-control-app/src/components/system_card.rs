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
    let icon_src = format!("/static/icons/systems/{}.png", system.folder_name);

    let card_class = if has_games {
        "system-card"
    } else {
        "system-card empty"
    };

    view! {
        <A href=href attr:class=card_class>
            <div class="system-card-name">{system.display_name.clone()}</div>
            <div class="system-card-body">
                <img
                    class="system-card-icon"
                    src=icon_src
                    alt=""
                    onerror="this.style.display='none'"
                    loading="lazy"
                />
                <div class="system-card-text">
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
                </div>
            </div>
        </A>
    }
}
