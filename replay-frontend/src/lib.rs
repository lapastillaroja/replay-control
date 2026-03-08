mod api;
mod components;
mod pages;
mod types;

use leptos::prelude::*;

use components::nav::BottomNav;
use pages::favorites::FavoritesPage;
use pages::games::GamesPage;
use pages::home::HomePage;
use pages::more::MorePage;

#[derive(Clone, Copy, PartialEq)]
pub enum Tab {
    Home,
    Games,
    Favorites,
    More,
}

#[component]
pub fn App() -> impl IntoView {
    let (active_tab, set_active_tab) = signal(Tab::Home);
    let (selected_system, set_selected_system) = signal(Option::<String>::None);

    let on_tab_change = move |tab: Tab| {
        if tab == Tab::Games {
            set_selected_system.set(None);
        }
        set_active_tab.set(tab);
    };

    view! {
        <div class="app">
            <header class="top-bar">
                <h1 class="app-title">"Replay"</h1>
                <div class="top-actions">
                    <button
                        class="icon-btn"
                        class:active=move || active_tab.get() == Tab::Favorites
                        on:click=move |_| set_active_tab.set(Tab::Favorites)
                        title="Favorites"
                    >
                        {icon_star()}
                    </button>
                </div>
            </header>

            <main class="content">
                <Show when=move || active_tab.get() == Tab::Home>
                    <HomePage
                        set_selected_system=set_selected_system
                        set_active_tab=set_active_tab
                    />
                </Show>
                <Show when=move || active_tab.get() == Tab::Games>
                    <GamesPage
                        selected_system=selected_system
                        set_selected_system=set_selected_system
                    />
                </Show>
                <Show when=move || active_tab.get() == Tab::Favorites>
                    <FavoritesPage />
                </Show>
                <Show when=move || active_tab.get() == Tab::More>
                    <MorePage />
                </Show>
            </main>

            <BottomNav active_tab=active_tab on_tab_change=on_tab_change />
        </div>
    }
}

fn icon_star() -> &'static str {
    "\u{2605}"
}
