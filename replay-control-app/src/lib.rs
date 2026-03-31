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
use i18n::provide_i18n;
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
use pages::password::PasswordPage;
use pages::search::SearchPage;
use pages::skin::SkinPage;
use pages::wifi::WifiPage;

/// SVG path data for the 3 rotating top-bar icons (all 256x256 viewBox).
const TOP_BAR_ICONS: [&str; 3] = [
    // Phosphor Joystick (MIT)
    r#"<path d="M224,160v48a16,16,0,0,1-16,16H48a16,16,0,0,1-16-16V160a16,16,0,0,1,16-16h72V95.19a40,40,0,1,1,16,0V144h72A16,16,0,0,1,224,160Zm-64-40a8,8,0,0,0,8,8h32a8,8,0,0,0,0-16H168A8,8,0,0,0,160,120Z"/>"#,
    // Phosphor Game Controller (MIT)
    r#"<path d="M247.44,173.75a.68.68,0,0,0,0-.14L231.05,89.44c0-.06,0-.12,0-.18A60.08,60.08,0,0,0,172,40H83.89a59.88,59.88,0,0,0-59,49.52L8.58,173.61a.68.68,0,0,0,0,.14,36,36,0,0,0,60.9,31.71l.35-.37L109.52,160h37l39.71,45.09c.11.13.23.25.35.37A36.08,36.08,0,0,0,212,216a36,36,0,0,0,35.43-42.25ZM104,112H96v8a8,8,0,0,1-16,0v-8H72a8,8,0,0,1,0-16h8V88a8,8,0,0,1,16,0v8h8a8,8,0,0,1,0,16Zm40-8a8,8,0,0,1,8-8h24a8,8,0,0,1,0,16H152A8,8,0,0,1,144,104Zm84.37,87.47a19.84,19.84,0,0,1-12.9,8.23A20.09,20.09,0,0,1,198,194.31L167.8,160H172a60,60,0,0,0,51-28.38l8.74,45A19.82,19.82,0,0,1,228.37,191.47Z"/>"#,
    // Mega Drive 6-button fighting pad (custom)
    r#"<path fill-rule="evenodd" d="M68,68 C38,68 14,86 8,114 C4,130 6,150 14,164 C22,180 40,190 58,190 L82,190 C94,190 106,184 114,174 L126,156 L130,156 L142,174 C150,184 162,190 174,190 L198,190 C216,190 234,180 242,164 C250,150 252,130 248,114 C242,86 218,68 188,68 Z M54,120 L66,120 L66,108 L78,108 L78,120 L90,120 L90,132 L78,132 L78,144 L66,144 L66,132 L54,132 Z M162,110 A10,10 0 1,1 162,130 A10,10 0 1,1 162,110 M186,106 A10,10 0 1,1 186,126 A10,10 0 1,1 186,106 M210,110 A10,10 0 1,1 210,130 A10,10 0 1,1 210,110 M168,136 A10,10 0 1,1 168,156 A10,10 0 1,1 168,136 M192,132 A10,10 0 1,1 192,152 A10,10 0 1,1 192,132 M216,136 A10,10 0 1,1 216,156 A10,10 0 1,1 216,136 M112,134 L122,134 C124,134 126,132 126,130 L126,128 C126,126 124,124 122,124 L112,124 C110,124 108,126 108,128 L108,130 C108,132 110,134 112,134 M134,134 L144,134 C146,134 148,132 148,130 L148,128 C148,126 146,124 144,124 L134,124 C132,124 130,126 130,128 L130,130 C130,132 132,134 134,134"/>"#,
];

/// Pick a random top-bar icon (SSR: time-based, hydrate: index 0 — irrelevant
/// since the client preserves the server-rendered SVG).
fn top_bar_icon_path() -> &'static str {
    #[cfg(feature = "ssr")]
    {
        let idx = (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos() % 3) as usize;
        TOP_BAR_ICONS[idx]
    }
    #[cfg(not(feature = "ssr"))]
    {
        TOP_BAR_ICONS[0]
    }
}

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
                <link rel="manifest" href="/static/manifest.json" />
                <link rel="icon" type="image/png" sizes="192x192" href="/static/icons/icon-192.png" />
                <link rel="apple-touch-icon" href="/static/icons/icon-192.png" />
                <link rel="stylesheet" href="/static/style.css" />
                <style id="skin-theme">{skin_css}</style>
                <HydrationScripts options=options.clone() />
                <script>
                    "if ('serviceWorker' in navigator) { navigator.serviceWorker.register('/static/sw.js'); }"
                </script>
                <script defer src="/static/ptr-init.js"></script>
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

    view! {
        <Router>
            <SseConfigListener />
            <SearchShortcut />
            <div class="app">
                <header class="top-bar">
                    <h1 class="app-title">
                        <A href="/" attr:class="app-title-link">
                            <svg
                                class="top-bar-icon"
                                viewBox="0 0 256 256"
                                fill="currentColor"
                                aria-hidden="true"
                                inner_html=top_bar_icon_path()
                            ></svg>
                            <span class="app-logo" aria-label="Replay Control"></span>
                        </A>
                    </h1>
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
                        <Route path=path!("/more/password") view=PasswordPage />
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

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    console_error_panic_hook::set_once();
    leptos::mount::hydrate_body(App);
}
