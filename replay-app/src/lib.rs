pub mod components;
pub mod i18n;
pub mod pages;
pub mod server_fns;
pub mod types;
pub mod util;

#[cfg(feature = "ssr")]
pub mod api;

use leptos::prelude::*;
use leptos_router::components::{Route, Router, Routes, A};
use leptos_router::path;

use components::nav::BottomNav;
use i18n::{provide_i18n, use_i18n, t};
use pages::favorites::{FavoritesPage, SystemFavoritesPage};
use pages::game_detail::GameDetailPage;
use pages::games::{GamesPage, SystemRomView};
use pages::home::HomePage;
use pages::more::MorePage;

/// The HTML shell wrapping the App component for SSR.
#[cfg(feature = "ssr")]
#[component]
pub fn Shell(options: leptos::config::LeptosOptions) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="UTF-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1.0, viewport-fit=cover" />
                <meta name="theme-color" content="#0f1115" />
                <meta name="apple-mobile-web-app-capable" content="yes" />
                <meta name="apple-mobile-web-app-status-bar-style" content="black-translucent" />
                <meta name="apple-mobile-web-app-title" content="Replay" />
                <title>"Replay"</title>
                <link rel="manifest" href="/manifest.json" />
                <link rel="icon" type="image/png" sizes="192x192" href="/icons/icon-192.png" />
                <link rel="apple-touch-icon" href="/icons/icon-192.png" />
                <link rel="stylesheet" href="/style.css" />
                <HydrationScripts options=options.clone() />
                <script>
                    "if ('serviceWorker' in navigator) { navigator.serviceWorker.register('/sw.js'); }"
                </script>
            </head>
            <body>
                <App />
            </body>
        </html>
    }
}

#[component]
pub fn App() -> impl IntoView {
    provide_i18n();
    let i18n = use_i18n();

    view! {
        <Router>
            <div class="app">
                <header class="top-bar">
                    <h1 class="app-title">
                        <A href="/" attr:class="app-title-link">{move || t(i18n.locale.get(), "app.title")}</A>
                    </h1>
                    <div class="top-actions">
                        <A href="/favorites" attr:class="icon-btn" attr:title="Favorites">
                            {icon_star()}
                        </A>
                    </div>
                </header>

                <main class="content">
                    <Routes fallback=|| view! { <p class="error">"Page not found"</p> }>
                        <Route path=path!("/") view=HomePage />
                        <Route path=path!("/games") view=GamesPage />
                        <Route path=path!("/games/:system") view=SystemRomView />
                        <Route path=path!("/games/:system/:filename") view=GameDetailPage />
                        <Route path=path!("/favorites") view=FavoritesPage />
                        <Route path=path!("/favorites/:system") view=SystemFavoritesPage />
                        <Route path=path!("/more") view=MorePage />
                    </Routes>
                </main>

                <BottomNav />
            </div>
        </Router>
    }
}

fn icon_star() -> &'static str {
    "\u{2605}"
}

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    leptos::mount::hydrate_body(App);
}
