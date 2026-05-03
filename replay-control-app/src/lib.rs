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
use pages::ErrorDisplay;
use pages::developer::DeveloperPage;
use pages::favorites::{FavoritesPage, SystemFavoritesPage};
use pages::game_detail::GameDetailPage;
use pages::games::SystemRomView;
use pages::github::GithubPage;
use pages::home::HomePage;
use pages::hostname::HostnamePage;
use pages::logs::LogsPage;
use pages::metadata::MetadataPage;
use pages::nfs::NfsPage;
use pages::password::PasswordPage;
use pages::search::SearchPage;
use pages::settings::SettingsPage;
use pages::skin::SkinPage;
use pages::stats::StatsDashboardPage;
use pages::updating::UpdatingPage;
use pages::wifi::WifiPage;
use server_fns::{Activity, CorruptionStatus};

/// The HTML shell wrapping the App component for SSR.
#[cfg(feature = "ssr")]
#[component]
pub fn Shell(options: leptos::config::LeptosOptions) -> impl IntoView {
    use crate::api::AppState;
    use crate::i18n::InitialLocale;
    use replay_control_core::skins;

    let state = expect_context::<AppState>();
    let skin_index = state.effective_skin();
    let theme_color = skins::theme_color(skin_index);
    let skin_css = skins::theme_css(skin_index).unwrap_or_default();
    let font_size = state
        .prefs
        .read()
        .expect("prefs lock poisoned")
        .font_size
        .clone();
    let html_class = if font_size == "large" {
        "font-large"
    } else {
        ""
    };
    // SSR lang attribute: use the resolved locale (injected from settings/Accept-Language).
    // After hydration, the App's reactive signal takes over via the <html lang> attribute.
    let initial_lang = use_context::<InitialLocale>()
        .map(|il| il.0.code())
        .unwrap_or("en");

    view! {
        <!DOCTYPE html>
        <html lang=initial_lang class=html_class>
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

    let update_state = RwSignal::new(replay_control_core::update::UpdateState::None);
    provide_context(update_state);

    // Push-based status signals fed by SSE listeners below.
    // Banners and other consumers subscribe to these via use_context.
    provide_context(RwSignal::new(Activity::Idle));
    provide_context(RwSignal::new(CorruptionStatus::default()));
    // Fed by SseConfigListener; the skin page subscribes so its "current"
    // badge follows external skin changes (e.g. changed from the Pi).
    provide_context(RwSignal::<Option<u32>>::new(None));

    view! {
        <Router>
            <SseConfigListener />
            <SseActivityListener />
            <SearchShortcut />
            <div class="app">
                <header class="top-bar">
                    <h1 class="app-title">
                        <A href="/" attr:class="app-title-link">
                            <img
                                class="top-bar-icon"
                                src="/static/branding/app-icon.png"
                                alt=""
                                aria-hidden="true"
                            />
                            <span class="app-logo" aria-label="Replay Control"></span>
                        </A>
                    </h1>
                </header>

                <CorruptionBanner />
                <MetadataBusyBanner />

                <main class="content">
                    <Routes fallback=|| view! { <p class="error">"Page not found"</p> }>
                        <Route path=path!("/") view=|| view! { <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }><HomePage /></ErrorBoundary> } />
                        <Route path=path!("/developer/:name") view=|| view! { <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }><DeveloperPage /></ErrorBoundary> } />
                        <Route path=path!("/games/:system") view=|| view! { <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }><SystemRomView /></ErrorBoundary> } />
                        <Route path=path!("/games/:system/:filename") view=|| view! { <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }><GameDetailPage /></ErrorBoundary> } />
                        <Route path=path!("/favorites") view=|| view! { <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }><FavoritesPage /></ErrorBoundary> } />
                        <Route path=path!("/favorites/:system") view=|| view! { <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }><SystemFavoritesPage /></ErrorBoundary> } />
                        <Route path=path!("/my-games") view=|| view! { <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }><pages::my_games::MyGamesPage /></ErrorBoundary> } />
                        <Route path=path!("/search") view=|| view! { <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }><SearchPage /></ErrorBoundary> } />
                        <Route path=path!("/stats") view=|| view! { <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }><StatsDashboardPage /></ErrorBoundary> } />
                        <Route path=path!("/settings") view=|| view! { <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }><SettingsPage /></ErrorBoundary> } />
                        <Route path=path!("/settings/wifi") view=|| view! { <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }><WifiPage /></ErrorBoundary> } />
                        <Route path=path!("/settings/nfs") view=|| view! { <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }><NfsPage /></ErrorBoundary> } />
                        <Route path=path!("/settings/hostname") view=|| view! { <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }><HostnamePage /></ErrorBoundary> } />
                        <Route path=path!("/settings/password") view=|| view! { <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }><PasswordPage /></ErrorBoundary> } />
                        <Route path=path!("/settings/metadata") view=|| view! { <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }><MetadataPage /></ErrorBoundary> } />
                        <Route path=path!("/settings/skin") view=|| view! { <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }><SkinPage /></ErrorBoundary> } />
                        <Route path=path!("/settings/logs") view=|| view! { <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }><LogsPage /></ErrorBoundary> } />
                        <Route path=path!("/settings/github") view=|| view! { <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }><GithubPage /></ErrorBoundary> } />
                        <Route path=path!("/updating") view=|| view! { <UpdatingPage /> } />
                    </Routes>
                </main>

                <BottomNav />
            </div>
        </Router>
    }
}

#[cfg(feature = "hydrate")]
fn corruption_status_from_payload(payload: &serde_json::Value) -> CorruptionStatus {
    let bool_field = |key: &str| payload.get(key).and_then(|v| v.as_bool()).unwrap_or(false);
    CorruptionStatus {
        library_corrupt: bool_field("library_corrupt"),
        user_data_corrupt: bool_field("user_data_corrupt"),
        user_data_backup_exists: bool_field("user_data_backup_exists"),
    }
}

/// SSE listener for config changes (skin, storage, update, corruption).
///
/// Connects to `/sse/config` on hydration. This is a broadcast-based endpoint
/// (no polling) — the server pushes events only when state actually changes.
///
/// Handles:
/// - `init`: records current skin/storage state and corruption flags from server
/// - `SkinChanged`: updates the `<style id="skin-theme">` element in-place
/// - `StorageChanged`: reloads the page so all data is re-fetched
/// - `UpdateAvailable`: sets the update-state signal
/// - `CorruptionChanged`: writes to the `RwSignal<CorruptionStatus>` context
#[component]
fn SseConfigListener() -> impl IntoView {
    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::prelude::*;

        // `None` until the `init` event below seeds it.
        let current_skin = expect_context::<RwSignal<Option<u32>>>();
        // Track the last storage kind to detect real transitions.
        let last_storage_kind = RwSignal::new(String::new());

        // Capture signals before closures.
        let update_state_signal =
            use_context::<RwSignal<replay_control_core::update::UpdateState>>();
        let corruption_signal = use_context::<RwSignal<CorruptionStatus>>();

        Effect::new(move || {
            let es = match web_sys::EventSource::new("/sse/config") {
                Ok(es) => es,
                Err(_) => return,
            };

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
                                current_skin.set(Some(idx as u32));
                            }
                            if let Some(kind) = payload.get("storage_kind").and_then(|v| v.as_str())
                            {
                                last_storage_kind.set(kind.to_string());
                            }
                            // Set available update from init payload.
                            if let Some(signal) = update_state_signal
                                && let Some(update_val) = payload.get("available_update")
                                && let Ok(available) =
                                    serde_json::from_value::<
                                        replay_control_core::update::AvailableUpdate,
                                    >(update_val.clone())
                            {
                                let current = signal.get_untracked();
                                if !matches!(
                                    current,
                                    replay_control_core::update::UpdateState::Restarting { .. }
                                ) {
                                    signal.set(
                                        replay_control_core::update::UpdateState::Available(
                                            available,
                                        ),
                                    );
                                }
                            }
                            if let Some(sig) = corruption_signal {
                                sig.set(corruption_status_from_payload(&payload));
                            }
                            // Version-based reload for stale tabs.
                            if let Some(server_version) =
                                payload.get("version").and_then(|v| v.as_str())
                                && server_version != crate::VERSION
                                && let Some(window) = web_sys::window()
                            {
                                let _ = window.location().reload();
                            }
                        }
                        "SkinChanged" => {
                            if let Some(idx) = payload.get("skin_index").and_then(|v| v.as_u64()) {
                                let idx = idx as u32;
                                let prev = current_skin.get_untracked();
                                if prev != Some(idx) {
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
                                    current_skin.set(Some(idx));
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
                        "UpdateAvailable" => {
                            if let Some(signal) = update_state_signal
                                && let Some(update_val) = payload.get("update")
                                && let Ok(available) =
                                    serde_json::from_value::<
                                        replay_control_core::update::AvailableUpdate,
                                    >(update_val.clone())
                            {
                                let current = signal.get_untracked();
                                if !matches!(
                                    current,
                                    replay_control_core::update::UpdateState::Restarting { .. }
                                ) {
                                    signal.set(
                                        replay_control_core::update::UpdateState::Available(
                                            available,
                                        ),
                                    );
                                }
                            }
                        }
                        "CorruptionChanged" => {
                            if let Some(sig) = corruption_signal {
                                sig.set(corruption_status_from_payload(&payload));
                            }
                        }
                        _ => {}
                    }
                },
            );

            es.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
            on_message.forget();

            // No onerror handler: rely on EventSource's built-in retry so the
            // listener reconnects after a server restart (e.g. auto-update).
            // The fresh `init` payload that follows is what triggers the
            // version-mismatch reload for stale tabs.
            //
            // Leak the EventSource so its wbindgen wrapper isn't dropped at the
            // end of this Effect — the listener is mounted at the App root and
            // never unmounts.
            std::mem::forget(es);
        });
    }
}

/// SSE listener for activity state (import, thumbnail, rebuild, maintenance, etc.).
///
/// Connects to `/sse/activity` once at hydration and stays open for the
/// lifetime of the tab. Each event payload is a JSON-encoded `Activity`;
/// this writes it into the `RwSignal<Activity>` provided by `App`. Banners
/// and other consumers subscribe via `use_context::<RwSignal<Activity>>()`.
///
/// No `onerror` handler — the browser's built-in EventSource retry reconnects
/// after server restarts, the same as `SseConfigListener`.
#[component]
fn SseActivityListener() -> impl IntoView {
    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::prelude::*;

        let activity_signal = use_context::<RwSignal<Activity>>();

        Effect::new(move || {
            let Some(signal) = activity_signal else {
                return;
            };
            let es = match web_sys::EventSource::new("/sse/activity") {
                Ok(es) => es,
                Err(_) => return,
            };

            let on_message = Closure::<dyn Fn(web_sys::MessageEvent)>::new(
                move |event: web_sys::MessageEvent| {
                    let data = event.data().as_string().unwrap_or_default();
                    if data.is_empty() {
                        return;
                    }
                    if let Ok(activity) = serde_json::from_str::<Activity>(&data) {
                        signal.set(activity);
                    }
                },
            );

            es.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
            on_message.forget();

            // Listener mounted at App root, never unmounts — leaking the
            // EventSource keeps its wbindgen wrapper alive past Effect end.
            std::mem::forget(es);
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
