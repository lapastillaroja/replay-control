use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_location;

use crate::i18n::{t, use_i18n};

#[component]
pub fn BottomNav() -> impl IntoView {
    let i18n = use_i18n();
    let location = use_location();

    let tabs = [
        ("/", "nav.games", "\u{1F3AE}"),
        ("/favorites", "nav.favorites", "\u{2B50}"),
        ("/more", "nav.more", "\u{2261}"),
    ];

    view! {
        <nav class="bottom-nav">
            {tabs.into_iter().map(|(href, label_key, icon)| {
                let class = move || {
                    let path = location.pathname.get();
                    let active = if href == "/" {
                        path == "/" || path.starts_with("/games")
                    } else {
                        path.starts_with(href)
                    };
                    if active { "nav-tab active" } else { "nav-tab" }
                };

                view! {
                    <A href=href attr:class=class>
                        <span class="nav-icon">{icon}</span>
                        <span class="nav-label">{move || t(i18n.locale.get(), label_key)}</span>
                    </A>
                }
            }).collect::<Vec<_>>()}
        </nav>
    }
}
