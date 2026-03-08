use leptos::prelude::*;

use crate::Tab;

#[component]
pub fn BottomNav(
    active_tab: ReadSignal<Tab>,
    on_tab_change: impl Fn(Tab) + 'static + Copy,
) -> impl IntoView {
    let tabs = [
        (Tab::Home, "Home", "\u{1F3E0}"),
        (Tab::Games, "Games", "\u{1F3AE}"),
        (Tab::Favorites, "Favs", "\u{2B50}"),
        (Tab::More, "More", "\u{2261}"),
    ];

    view! {
        <nav class="bottom-nav">
            {tabs
                .into_iter()
                .map(|(tab, label, icon)| {
                    view! {
                        <button
                            class="nav-tab"
                            class:active=move || active_tab.get() == tab
                            on:click=move |_| on_tab_change(tab)
                        >
                            <span class="nav-icon">{icon}</span>
                            <span class="nav-label">{label}</span>
                        </button>
                    }
                })
                .collect::<Vec<_>>()}
        </nav>
    }
}
