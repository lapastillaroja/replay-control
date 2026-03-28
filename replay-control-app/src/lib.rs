#![recursion_limit = "512"]

/// App version from Cargo.toml.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Git commit hash, embedded at build time.
pub const GIT_HASH: &str = env!("GIT_HASH");

pub mod components;
pub mod hooks;
pub mod i18n;
pub mod pages;
pub mod server_fns;
pub mod types;
pub mod util;

#[cfg(feature = "ssr")]
pub mod api;

use leptos::prelude::*;
use leptos_router::components::{A, Route, Router, Routes};
use leptos_router::path;

use components::corruption_banner::CorruptionBanner;
use components::metadata_banner::MetadataBusyBanner;
use components::nav::BottomNav;
use i18n::{provide_i18n, t, use_i18n};
use pages::developer::DeveloperPage;
use pages::favorites::{FavoritesPage, SystemFavoritesPage};
use pages::game_detail::GameDetailPage;
use pages::games::SystemRomView;
use pages::github::GithubPage;
use pages::home::HomePage;
use pages::hostname::HostnamePage;
use pages::logs::LogsPage;
use pages::metadata::MetadataPage;
use pages::more::MorePage;
use pages::nfs::NfsPage;
use pages::search::SearchPage;
use pages::skin::SkinPage;
use pages::wifi::WifiPage;

/// The HTML shell wrapping the App component for SSR.
#[cfg(feature = "ssr")]
#[component]
pub fn Shell(options: leptos::config::LeptosOptions) -> impl IntoView {
    use crate::api::AppState;
    use replay_control_core::skins;

    let state = expect_context::<AppState>();
    let skin_index = state.effective_skin();
    let theme_color = skins::theme_color(skin_index);
    let skin_css = skins::theme_css(skin_index).unwrap_or_default();
    let font_size = replay_control_core::settings::read_font_size(&state.storage().root);
    let html_class = if font_size == "large" {
        "font-large"
    } else {
        ""
    };

    view! {
        <!DOCTYPE html>
        <html lang="en" class=html_class>
            <head>
                <meta charset="UTF-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1.0, viewport-fit=cover" />
                <meta name="theme-color" content=theme_color />
                <meta name="apple-mobile-web-app-capable" content="yes" />
                <meta name="apple-mobile-web-app-status-bar-style" content="black-translucent" />
                <meta name="apple-mobile-web-app-title" content="Replay Control" />
                <meta name="version" content=format!("{}-{}", VERSION, GIT_HASH) />
                <title>"Replay Control"</title>
                <link rel="manifest" href="/manifest.json" />
                <link rel="icon" type="image/png" sizes="192x192" href="/icons/icon-192.png" />
                <link rel="apple-touch-icon" href="/icons/icon-192.png" />
                <link rel="stylesheet" href="/style.css" />
                <style id="skin-theme">{skin_css}</style>
                <HydrationScripts options=options.clone() />
                <script>
                    "if ('serviceWorker' in navigator) { navigator.serviceWorker.register('/sw.js'); }"
                </script>
                <script defer src="/ptr-init.js"></script>
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
            <SseConfigListener />
            <SearchShortcut />
            <div class="app">
                <header class="top-bar">
                    <h1 class="app-title">
                        <A href="/" attr:class="app-title-link">{move || t(i18n.locale.get(), "app.title")}</A>
                    </h1>
                    <div class="top-actions">
                        <A href="/search" attr:class="icon-btn" attr:title="Search">
                            {icon_search()}
                        </A>
                        <A href="/favorites" attr:class="icon-btn" attr:title="Favorites">
                            {icon_star()}
                        </A>
                    </div>
                </header>

                <CorruptionBanner />
                <MetadataBusyBanner />

                <main class="content">
                    <Routes fallback=|| view! { <p class="error">"Page not found"</p> }>
                        <Route path=path!("/") view=HomePage />
                        <Route path=path!("/developer/:name") view=DeveloperPage />
                        <Route path=path!("/games/:system") view=SystemRomView />
                        <Route path=path!("/games/:system/:filename") view=GameDetailPage />
                        <Route path=path!("/favorites") view=FavoritesPage />
                        <Route path=path!("/favorites/:system") view=SystemFavoritesPage />
                        <Route path=path!("/search") view=SearchPage />
                        <Route path=path!("/more") view=MorePage />
                        <Route path=path!("/more/wifi") view=WifiPage />
                        <Route path=path!("/more/nfs") view=NfsPage />
                        <Route path=path!("/more/hostname") view=HostnamePage />
                        <Route path=path!("/more/metadata") view=MetadataPage />
                        <Route path=path!("/more/skin") view=SkinPage />
                        <Route path=path!("/more/logs") view=LogsPage />
                        <Route path=path!("/more/github") view=GithubPage />
                    </Routes>
                </main>

                <BottomNav />
            </div>
        </Router>
    }
}

/// SSE listener for config changes (skin, storage).
///
/// Connects to `/sse/config` on hydration. This is a broadcast-based endpoint
/// (no polling) — the server pushes events only when skin or storage actually
/// changes.
///
/// Handles:
/// - `init`: records current skin/storage state from server
/// - `SkinChanged`: updates the `<style id="skin-theme">` element in-place
/// - `StorageChanged`: reloads the page so all data is re-fetched
#[component]
fn SseConfigListener() -> impl IntoView {
    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::prelude::*;

        // Track the last skin index to avoid unnecessary CSS updates on init.
        // u32::MAX means "not yet initialized". These are internal tracking
        // signals — nothing subscribes to them reactively.
        let last_skin = RwSignal::new(u32::MAX);
        // Track the last storage kind to detect real transitions.
        let last_storage_kind = RwSignal::new(String::new());

        Effect::new(move || {
            let es = match web_sys::EventSource::new("/sse/config") {
                Ok(es) => es,
                Err(_) => return,
            };

            let es_ref = es.clone();
            let on_message = Closure::<dyn Fn(web_sys::MessageEvent)>::new(
                move |event: web_sys::MessageEvent| {
                    let data = event.data().as_string().unwrap_or_default();
                    if data.is_empty() {
                        return;
                    }

                    let Ok(payload) = serde_json::from_str::<serde_json::Value>(&data) else {
                        return;
                    };

                    let event_type = payload
                        .get("type")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default();

                    match event_type {
                        "init" => {
                            // Record initial state from server.
                            if let Some(idx) = payload.get("skin_index").and_then(|v| v.as_u64()) {
                                last_skin.set(idx as u32);
                            }
                            if let Some(kind) = payload.get("storage_kind").and_then(|v| v.as_str())
                            {
                                last_storage_kind.set(kind.to_string());
                            }
                        }
                        "SkinChanged" => {
                            if let Some(idx) = payload.get("skin_index").and_then(|v| v.as_u64()) {
                                let idx = idx as u32;
                                let prev = last_skin.get_untracked();
                                if prev != idx {
                                    // Update the <style id="skin-theme"> element.
                                    if let Some(doc) = web_sys::window().and_then(|w| w.document())
                                    {
                                        if let Some(style_el) = doc.get_element_by_id("skin-theme")
                                        {
                                            let css = payload
                                                .get("skin_css")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("");
                                            style_el.set_text_content(Some(css));
                                        }
                                        // Update the theme-color meta tag.
                                        if let Ok(Some(meta)) =
                                            doc.query_selector("meta[name='theme-color']")
                                        {
                                            let bg = payload
                                                .get("skin_css")
                                                .and_then(|v| v.as_str())
                                                .and_then(|css| {
                                                    css.find("--bg:")
                                                        .map(|i| &css[i + 5..])
                                                        .and_then(|s| {
                                                            s.find(';').map(|j| s[..j].trim())
                                                        })
                                                })
                                                .unwrap_or("#1a1a2e");
                                            let _ = meta.set_attribute("content", bg);
                                        }
                                    }
                                    last_skin.set(idx);
                                }
                            }
                        }
                        "StorageChanged" => {
                            if let Some(new_kind) =
                                payload.get("storage_kind").and_then(|v| v.as_str())
                            {
                                let prev = last_storage_kind.get_untracked();
                                if !prev.is_empty() && prev != new_kind {
                                    // Storage changed — reload to re-fetch all data.
                                    if let Some(window) = web_sys::window() {
                                        let _ = window.location().reload();
                                    }
                                }
                                last_storage_kind.set(new_kind.to_string());
                            }
                        }
                        _ => {}
                    }
                },
            );

            es.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
            on_message.forget();

            // On error (server restart, network issue) just close.
            // EventSource auto-reconnects by default; closing prevents
            // rapid reconnect loops if the server is truly down.
            let on_error = Closure::<dyn Fn()>::new(move || {
                es_ref.close();
            });
            es.set_onerror(Some(on_error.as_ref().unchecked_ref()));
            on_error.forget();

            // Keep the EventSource alive for the app lifetime (this component
            // is mounted at the App root and never unmounts).
        });
    }
}

/// Invisible component that installs the "/" keyboard shortcut for search.
/// Must be rendered inside `<Router>` so `use_navigate` has access to the
/// router context.
#[component]
fn SearchShortcut() -> impl IntoView {
    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::JsCast;
        use wasm_bindgen::prelude::*;

        let navigate = leptos_router::hooks::use_navigate();
        Effect::new(move || {
            let navigate = navigate.clone();
            let window = match web_sys::window() {
                Some(w) => w,
                None => return,
            };
            let cb = Closure::<dyn Fn(web_sys::KeyboardEvent)>::new(
                move |ev: web_sys::KeyboardEvent| {
                    if ev.key() != "/" {
                        return;
                    }
                    // Don't intercept if user is typing in an input or textarea.
                    if let Some(doc) = web_sys::window().and_then(|w| w.document())
                        && let Some(active) = doc.active_element()
                    {
                        let tag = active.tag_name().to_uppercase();
                        if tag == "INPUT" || tag == "TEXTAREA" || tag == "SELECT" {
                            return;
                        }
                    }
                    ev.prevent_default();
                    // Navigate to /search (or focus input if already there).
                    if let Some(w) = web_sys::window() {
                        let href = w.location().pathname().unwrap_or_default();
                        if href == "/search" {
                            // Already on search page -- focus the input.
                            if let Some(doc) = w.document()
                                && let Some(el) =
                                    doc.query_selector(".search-page-input").ok().flatten()
                                && let Some(input) = el.dyn_ref::<web_sys::HtmlInputElement>()
                            {
                                let _ = input.focus();
                            }
                        } else {
                            navigate("/search", Default::default());
                        }
                    }
                },
            );
            let _ = window.add_event_listener_with_callback("keydown", cb.as_ref().unchecked_ref());
            // This component is mounted once at the App root and never unmounts,
            // so `forget()` is acceptable — the listener lives for the app lifetime.
            cb.forget();
        });
    }
}

fn icon_search() -> impl leptos::IntoView {
    leptos::prelude::view! {
        <span inner_html="<svg xmlns='http://www.w3.org/2000/svg' width='18' height='18' viewBox='0 0 24 24' fill='none' stroke='currentColor' stroke-width='2.5' stroke-linecap='round' stroke-linejoin='round'><circle cx='11' cy='11' r='8'/><line x1='21' y1='21' x2='16.65' y2='16.65'/></svg>" />
    }
}

fn icon_star() -> &'static str {
    "\u{2605}"
}

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    console_error_panic_hook::set_once();
    leptos::mount::hydrate_body(App);
}
