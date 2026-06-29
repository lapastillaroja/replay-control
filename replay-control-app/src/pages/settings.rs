use leptos::prelude::*;
use leptos_router::components::A;
#[cfg(feature = "hydrate")]
use leptos_router::hooks::use_location;
use serde::{Deserialize, Serialize};
use server_fn::ServerFnError;

use crate::components::theme_selector::ThemeSelector;
use crate::i18n::{Key, Locale, t, use_i18n};
use crate::server_fns;
use crate::server_fns::SystemLiveStats;
use crate::util::{format_elapsed_short, format_size};
use replay_control_core::auth::{AuthRole, AuthStatus};
use replay_control_core::update::UpdateState;

/// Section definitions: (ID, i18n key) — single source of truth for sidebar + scroll-spy.
const SECTIONS: [(&str, Key); 7] = [
    ("settings-appearance", Key::SettingsSectionAppearance),
    ("settings-library-games", Key::SettingsSectionLibraryGames),
    ("settings-device-network", Key::SettingsSectionDeviceNetwork),
    ("settings-access", Key::SettingsSectionAccess),
    ("settings-replayos", Key::ReplayOsSettingsTitle),
    ("settings-updates", Key::MoreSectionUpdates),
    ("settings-system", Key::SettingsSectionSystem),
];

const SECTION_APPEARANCE: &str = SECTIONS[0].0;
const SECTION_LIBRARY_GAMES: &str = SECTIONS[1].0;
const SECTION_DEVICE_NETWORK: &str = SECTIONS[2].0;
const SECTION_ACCESS: &str = SECTIONS[3].0;
const SECTION_REPLAYOS: &str = SECTIONS[4].0;
const SECTION_UPDATES: &str = SECTIONS[5].0;
const SECTION_SYSTEM: &str = SECTIONS[6].0;

#[derive(Clone, Serialize, Deserialize)]
struct AppearanceValues {
    font_size: String,
    locale: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct LibraryGamesValues {
    admin_unlocked: bool,
    on_device: bool,
    region: String,
    region_secondary: String,
    language_primary: String,
    language_secondary: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct GateValues {
    admin_unlocked: bool,
    on_device: bool,
}

#[derive(Clone, Serialize, Deserialize)]
struct SystemValues {
    admin_unlocked: bool,
    analytics_enabled: Option<bool>,
}

async fn load_appearance_values() -> Result<AppearanceValues, ServerFnError> {
    Ok(AppearanceValues {
        font_size: server_fns::get_font_size().await?,
        locale: server_fns::get_locale().await.unwrap_or_default(),
    })
}

async fn load_library_games_values() -> Result<LibraryGamesValues, ServerFnError> {
    let mode = server_fns::get_mode().await?;
    let auth_status = server_fns::get_auth_status().await?;
    let admin_unlocked = admin_settings_unlocked(&auth_status);
    let (language_primary, language_secondary) = server_fns::get_language_preference().await?;

    Ok(LibraryGamesValues {
        admin_unlocked,
        on_device: mode.is_device(),
        region: server_fns::get_region_preference().await?,
        region_secondary: server_fns::get_region_preference_secondary().await?,
        language_primary,
        language_secondary,
    })
}

async fn load_gate_values() -> Result<GateValues, ServerFnError> {
    let mode = server_fns::get_mode().await?;
    let auth_status = server_fns::get_auth_status().await?;

    Ok(GateValues {
        admin_unlocked: admin_settings_unlocked(&auth_status),
        on_device: mode.is_device(),
    })
}

async fn load_system_values() -> Result<SystemValues, ServerFnError> {
    let auth_status = server_fns::get_auth_status().await?;
    let admin_unlocked = admin_settings_unlocked(&auth_status);
    let analytics_enabled = if admin_unlocked {
        Some(server_fns::get_analytics_preference().await.unwrap_or(true))
    } else {
        None
    };

    Ok(SystemValues {
        admin_unlocked,
        analytics_enabled,
    })
}

#[component]
pub fn SettingsPage() -> impl IntoView {
    let i18n = use_i18n();
    let active_section = RwSignal::new(SECTION_APPEARANCE.to_string());

    view! {
        <div class="page settings-page">
            <h2 class="page-title">{move || t(i18n.locale.get(), Key::SettingsTitle)}</h2>

            <div class="settings-layout">
                <SettingsSidebar active_section />
                <div class="settings-content">
                    <AppearanceSection />
                    <LibraryGamesSection />
                    <DeviceNetworkSection />
                    <AccessSection />
                    <ReplayOsSection />
                    <UpdatesSection />
                    <SystemSection />
                </div>
            </div>
        </div>

        <ScrollSpy active_section />
    }
}

#[component]
fn AppearanceSection() -> impl IntoView {
    let i18n = use_i18n();
    let values = Resource::new_blocking(|| (), |_| load_appearance_values());

    view! {
        <section class="settings-section" id=SECTION_APPEARANCE>
            <h3 class="section-title">{move || t(i18n.locale.get(), Key::SettingsSectionAppearance)}</h3>
            <div class="settings-section-body">
                <div class="menu-list">
                    {menu_item("\u{1F3A8}", Key::MoreSkin, Some("/settings/skin"))}
                </div>
                <Suspense fallback=|| ()>
                    {move || Suspend::new(async move {
                        match values.await {
                            Ok(values) => view! {
                                <div class="settings-inline-setting">
                                    <h4 class="settings-setting-title">{move || t(i18n.locale.get(), Key::ThemeTitle)}</h4>
                                    <ThemeSelector />
                                </div>
                                <div class="settings-inline-setting">
                                    <h4 class="settings-setting-title">{move || t(i18n.locale.get(), Key::MoreTextSize)}</h4>
                                    <p class="form-hint">{move || t(i18n.locale.get(), Key::MoreTextSizeHint)}</p>
                                    <TextSizeToggle current=values.font_size />
                                </div>
                                <div class="settings-inline-setting">
                                    <h4 class="settings-setting-title">{move || t(i18n.locale.get(), Key::LocaleTitle)}</h4>
                                    <LocaleSelector current=values.locale />
                                </div>
                            }
                                .into_any(),
                            Err(err) => view! { <p class="error">{err.to_string()}</p> }.into_any(),
                        }
                    })}
                </Suspense>
            </div>
        </section>
    }
}

#[component]
fn LibraryGamesSection() -> impl IntoView {
    let i18n = use_i18n();
    let values = Resource::new_blocking(|| (), |_| load_library_games_values());

    view! {
        <section class="settings-section" id=SECTION_LIBRARY_GAMES>
            <h3 class="section-title">{move || t(i18n.locale.get(), Key::SettingsSectionLibraryGames)}</h3>
            <div class="settings-section-body">
                <Suspense fallback=|| ()>
                    {move || Suspend::new(async move {
                        match values.await {
                            Ok(values) => {
                                let device_hint_key = if values.on_device {
                                    Key::SettingsAdminOnlyDisabled
                                } else {
                                    Key::SettingsDeviceOnlyDisabled
                                };
                                view! {
                                    <div class="menu-list">
                                        {menu_item("\u{1F4DA}", Key::MoreMetadata, values.admin_unlocked.then_some("/settings/game-library"))}
                                        {menu_item("\u{1F3C6}", Key::MoreRetroAchievements, (values.on_device && values.admin_unlocked).then_some("/settings/retroachievements"))}
                                    </div>
                                    <Show when=move || !values.on_device || !values.admin_unlocked>
                                        <p class="form-hint">{move || t(i18n.locale.get(), device_hint_key)}</p>
                                    </Show>

                                    <div class="settings-inline-setting">
                                        <h4 class="settings-setting-title">{move || t(i18n.locale.get(), Key::RegionTitle)}</h4>
                                        <p class="form-hint">{move || t(i18n.locale.get(), Key::RegionHint)}</p>
                                        <RegionSelector current=values.region current_secondary=values.region_secondary disabled=!values.admin_unlocked />
                                        <Show when=move || !values.admin_unlocked>
                                            <p class="form-hint">{move || t(i18n.locale.get(), Key::SettingsAdminOnlyDisabled)}</p>
                                        </Show>
                                    </div>

                                    <div class="settings-inline-setting">
                                        <h4 class="settings-setting-title">{move || t(i18n.locale.get(), Key::LanguageTitle)}</h4>
                                        <p class="form-hint">{move || t(i18n.locale.get(), Key::LanguageHint)}</p>
                                        <LanguageSelector
                                            current_primary=values.language_primary
                                            current_secondary=values.language_secondary
                                            disabled=!values.admin_unlocked
                                        />
                                        <Show when=move || !values.admin_unlocked>
                                            <p class="form-hint">{move || t(i18n.locale.get(), Key::SettingsAdminOnlyDisabled)}</p>
                                        </Show>
                                    </div>
                                }
                                    .into_any()
                            }
                            Err(err) => view! { <p class="error">{err.to_string()}</p> }.into_any(),
                        }
                    })}
                </Suspense>
            </div>
        </section>
    }
}

#[component]
fn DeviceNetworkSection() -> impl IntoView {
    let i18n = use_i18n();
    let values = Resource::new_blocking(|| (), |_| load_gate_values());

    view! {
        <section class="settings-section" id=SECTION_DEVICE_NETWORK>
            <h3 class="section-title">{move || t(i18n.locale.get(), Key::SettingsSectionDeviceNetwork)}</h3>
            <div class="settings-section-body">
                <Suspense fallback=|| ()>
                    {move || Suspend::new(async move {
                        match values.await {
                            Ok(values) => {
                                let device_href = move |href: &'static str| (values.on_device && values.admin_unlocked).then_some(href);
                                let device_hint_key = if values.on_device {
                                    Key::SettingsAdminOnlyDisabled
                                } else {
                                    Key::SettingsDeviceOnlyDisabled
                                };
                                view! {
                                    <div class="menu-list">
                                        {menu_item("\u{1F4F6}", Key::MoreWifi, device_href("/settings/wifi"))}
                                        {menu_item("\u{1F4C1}", Key::MoreNfs, device_href("/settings/nfs"))}
                                        {menu_item("\u{1F4BB}", Key::MoreHostname, device_href("/settings/hostname"))}
                                    </div>
                                    <Show when=move || !values.on_device || !values.admin_unlocked>
                                        <p class="form-hint">{move || t(i18n.locale.get(), device_hint_key)}</p>
                                    </Show>
                                }
                                    .into_any()
                            }
                            Err(err) => view! { <p class="error">{err.to_string()}</p> }.into_any(),
                        }
                    })}
                </Suspense>
            </div>
        </section>
    }
}

#[component]
fn AccessSection() -> impl IntoView {
    let i18n = use_i18n();
    let auth_status = Resource::new_blocking(|| (), |_| server_fns::get_auth_status());

    view! {
        <section class="settings-section" id=SECTION_ACCESS>
            <h3 class="section-title">{move || t(i18n.locale.get(), Key::SettingsSectionAccess)}</h3>
            <div class="settings-section-body">
                <Suspense fallback=|| ()>
                    {move || Suspend::new(async move {
                        match auth_status.await {
                            Ok(auth_status) => view! {
                                <div class="menu-list">
                                    {if auth_status.auth_required && auth_status.role == AuthRole::Anonymous {
                                        menu_item("\u{1F511}", Key::LoginTitle, Some("/login")).into_any()
                                    } else {
                                        menu_item("\u{1F512}", Key::AccessSecurityTitle, Some("/settings/access")).into_any()
                                    }}
                                </div>
                            }
                                .into_any(),
                            Err(err) => view! { <p class="error">{err.to_string()}</p> }.into_any(),
                        }
                    })}
                </Suspense>
            </div>
        </section>
    }
}

#[component]
fn ReplayOsSection() -> impl IntoView {
    let i18n = use_i18n();
    let values = Resource::new_blocking(|| (), |_| load_gate_values());

    view! {
        <section class="settings-section" id=SECTION_REPLAYOS>
            <h3 class="section-title">{move || t(i18n.locale.get(), Key::ReplayOsSettingsTitle)}</h3>
            <div class="settings-section-body">
                <Suspense fallback=|| ()>
                    {move || Suspend::new(async move {
                        match values.await {
                            Ok(values) => {
                                let device_hint_key = if values.on_device {
                                    Key::SettingsAdminOnlyDisabled
                                } else {
                                    Key::SettingsDeviceOnlyDisabled
                                };
                                view! {
                                    <div class="menu-list">
                                        {menu_item("\u{1F4E1}", Key::ReplayOsSettingsTitle, (values.on_device && values.admin_unlocked).then_some("/settings/replayos"))}
                                    </div>
                                    <Show when=move || !values.on_device || !values.admin_unlocked>
                                        <p class="form-hint">{move || t(i18n.locale.get(), device_hint_key)}</p>
                                    </Show>
                                }
                                    .into_any()
                            }
                            Err(err) => view! { <p class="error">{err.to_string()}</p> }.into_any(),
                        }
                    })}
                </Suspense>
            </div>
        </section>
    }
}

#[component]
fn SystemSection() -> impl IntoView {
    let i18n = use_i18n();
    let system_values = Resource::new_blocking(|| (), |_| load_system_values());
    let live_stats = Resource::new(|| (), |_| server_fns::get_live_stats());

    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::prelude::*;
        Effect::new(move || {
            let cb = Closure::<dyn Fn()>::new(move || {
                live_stats.refetch();
            });
            let id = web_sys::window()
                .unwrap()
                .set_interval_with_callback_and_timeout_and_arguments_0(
                    cb.as_ref().unchecked_ref(),
                    1000,
                )
                .unwrap();
            cb.forget();
            on_cleanup(move || {
                if let Some(w) = web_sys::window() {
                    w.clear_interval_with_handle(id);
                }
            });
        });
    }

    view! {
        <section class="settings-section" id=SECTION_SYSTEM>
            <h3 class="section-title">{move || t(i18n.locale.get(), Key::SettingsSectionSystem)}</h3>
            <div class="settings-section-body">
                <Transition fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), Key::CommonLoading)}</div> }>
                    {move || Suspend::new(async move {
                        match live_stats.await {
                            Ok(stats) => view! { <SystemStatsGrid stats /> }.into_any(),
                            Err(err) => view! { <p class="error">{err.to_string()}</p> }.into_any(),
                        }
                    })}
                </Transition>
                <Suspense fallback=|| ()>
                    {move || Suspend::new(async move {
                        match system_values.await {
                            Ok(values) => view! {
                                <div class="menu-list">
                                    {menu_item("\u{1F4DC}", Key::MoreLogs, values.admin_unlocked.then_some("/settings/logs"))}
                                    {menu_item("\u{1F511}", Key::MoreGithub, values.admin_unlocked.then_some("/settings/github"))}
                                </div>
                                <Show when=move || !values.admin_unlocked>
                                    <p class="form-hint">{move || t(i18n.locale.get(), Key::SettingsAdminOnlyDisabled)}</p>
                                </Show>
                                {values.analytics_enabled.map(|current| view! { <AnalyticsInline current /> })}
                            }
                                .into_any(),
                            Err(err) => view! { <p class="error">{err.to_string()}</p> }.into_any(),
                        }
                    })}
                </Suspense>
            </div>
        </section>
    }
}

#[component]
fn SystemStatsGrid(stats: SystemLiveStats) -> impl IntoView {
    let i18n = use_i18n();
    view! {
        <div class="info-grid">
            <InfoRow label_key=Key::MoreStorage value=stats.storage_kind.to_uppercase() />
            <InfoRow label_key=Key::MorePath value=stats.storage_root />
            <InfoRow label_key=Key::MoreDiskTotal value=format_size(stats.disk_total_bytes) />
            <InfoRow label_key=Key::MoreDiskUsed value=format_size(stats.disk_used_bytes) />
            <InfoRow label_key=Key::MoreDiskAvailable value=format_size(stats.disk_available_bytes) />
            <InfoRow label_key=Key::MoreEthernetIp value=stats.ethernet_ip.unwrap_or_else(|| t(i18n.locale.get_untracked(), Key::MoreNotConnected).to_string()) />
            <InfoRow label_key=Key::MoreWifiIp value=stats.wifi_ip.unwrap_or_else(|| t(i18n.locale.get_untracked(), Key::MoreNotConnected).to_string()) />
            <InfoRow label_key=Key::MoreEthernetMac value=stats.ethernet_mac.unwrap_or_else(|| "—".to_string()) />
            <InfoRow label_key=Key::MoreWifiMac value=stats.wifi_mac.unwrap_or_else(|| "—".to_string()) />
            {stats.model.map(|model| view! { <InfoRow label_key=Key::MoreModel value=model /> })}
            {stats.cpu_temperature_c.map(|temp| view! { <InfoRow label_key=Key::MoreCpuTemperature value=format!("{temp:.0} °C") /> })}
            {stats.available_ram_mb.map(|mb| view! { <InfoRow label_key=Key::MoreAvailableRam value=format!("{mb} MB") /> })}
            <InfoRow label_key=Key::MoreUptime value=format_elapsed_short(stats.uptime_seconds) />
        </div>
    }
}

// ── Shared helpers ──────────────────────────────────────────────

/// Inline status message (ok/error) used by settings controls after save.
#[component]
fn SaveStatus(status: RwSignal<Option<(bool, String)>>) -> impl IntoView {
    move || {
        status.get().map(|(ok, msg)| {
            let class = if ok {
                "status-msg status-ok"
            } else {
                "status-msg status-err"
            };
            view! { <div class=class>{msg}</div> }
        })
    }
}

// ── Sidebar ─────────────────────────────────────────────────────

#[component]
fn SettingsSidebar(active_section: RwSignal<String>) -> impl IntoView {
    let i18n = use_i18n();

    view! {
        <nav class="settings-sidebar">
            {SECTIONS.into_iter().map(|(id, label_key)| {
                let class = move || {
                    if active_section.read().as_str() == id {
                        "settings-sidebar-item active"
                    } else {
                        "settings-sidebar-item"
                    }
                };

                let on_click = move |ev: leptos::ev::MouseEvent| {
                    ev.prevent_default();
                    #[cfg(feature = "hydrate")]
                    {
                        if let Some(el) = web_sys::window()
                            .and_then(|w| w.document())
                            .and_then(|doc| doc.get_element_by_id(id))
                        {
                            let opts = web_sys::ScrollIntoViewOptions::new();
                            opts.set_behavior(web_sys::ScrollBehavior::Smooth);
                            opts.set_block(web_sys::ScrollLogicalPosition::Start);
                            el.scroll_into_view_with_scroll_into_view_options(&opts);
                        }
                    }
                };

                view! {
                    <a href=format!("#{id}") class=class on:click=on_click>
                        {move || t(i18n.locale.get(), label_key)}
                    </a>
                }
            }).collect::<Vec<_>>()}
        </nav>
    }
}

// ── Scroll-spy (hydrate only) ───────────────────────────────────

#[component]
fn ScrollSpy(#[allow(unused_variables)] active_section: RwSignal<String>) -> impl IntoView {
    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::prelude::*;

        // Set up IntersectionObserver once on mount.
        // Deferred via requestAnimationFrame because Effect::new runs before
        // the component's DOM nodes are inserted into the document.  On SSR
        // hydration the elements already exist, but on client-side navigation
        // they don't yet — so getElementById would return None for every
        // section and the observer would observe nothing.
        Effect::new(move || {
            let Some(window) = web_sys::window() else {
                return;
            };

            let setup = Closure::<dyn FnMut()>::new(move || {
                let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
                    return;
                };

                let callback = Closure::<dyn Fn(js_sys::Array, web_sys::IntersectionObserver)>::new(
                    move |entries: js_sys::Array, _observer: web_sys::IntersectionObserver| {
                        for entry in entries.iter() {
                            let entry: web_sys::IntersectionObserverEntry =
                                JsCast::unchecked_into(entry);
                            if entry.is_intersecting()
                                && let Some(target) = entry.target().get_attribute("id")
                            {
                                active_section.set(target);
                            }
                        }
                    },
                );

                let options = web_sys::IntersectionObserverInit::new();
                options.set_threshold(&JsValue::from_f64(0.1));
                options.set_root_margin("-10% 0px -80% 0px");

                if let Ok(obs) = web_sys::IntersectionObserver::new_with_options(
                    callback.as_ref().unchecked_ref(),
                    &options,
                ) {
                    for (id, _) in SECTIONS {
                        if let Some(el) = doc.get_element_by_id(id) {
                            obs.observe(&el);
                        }
                    }
                    std::mem::forget(obs);
                }

                callback.forget();
            });

            let _ = window.request_animation_frame(setup.as_ref().unchecked_ref());
            setup.forget();
        });

        // Re-check visible section on route changes (back-navigation from sub-pages).
        // IntersectionObserver doesn't fire for elements already in the viewport.
        let pathname = use_location().pathname;
        Effect::new(move || {
            let path = pathname.get();
            if path != "/settings" {
                return;
            }

            let Some(window) = web_sys::window() else {
                return;
            };
            let Some(_doc) = window.document() else {
                return;
            };

            let raf_callback = Closure::<dyn Fn()>::new(move || {
                let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
                    return;
                };
                let vh = web_sys::window()
                    .and_then(|w| w.inner_height().ok())
                    .and_then(|v| v.as_f64())
                    .unwrap_or(800.0);
                let threshold_top = vh * 0.1;
                let threshold_bottom = vh * 0.2;
                for (id, _) in SECTIONS {
                    if let Some(el) = doc.get_element_by_id(id) {
                        let get_rect: js_sys::Function = match js_sys::Reflect::get(
                            &el,
                            &JsValue::from_str("getBoundingClientRect"),
                        )
                        .ok()
                        .and_then(|v| v.dyn_into().ok())
                        {
                            Some(f) => f,
                            None => continue,
                        };
                        let rect = get_rect.call0(&el).unwrap_or(JsValue::UNDEFINED);
                        let top = js_sys::Reflect::get(&rect, &JsValue::from_str("top"))
                            .ok()
                            .and_then(|v| v.as_f64())
                            .unwrap_or(f64::MAX);
                        let bottom = js_sys::Reflect::get(&rect, &JsValue::from_str("bottom"))
                            .ok()
                            .and_then(|v| v.as_f64())
                            .unwrap_or(f64::MIN);
                        if top < threshold_bottom && bottom > threshold_top {
                            active_section.set(id.to_string());
                            break;
                        }
                    }
                }
            });
            let _ = window.request_animation_frame(raf_callback.as_ref().unchecked_ref());
            raf_callback.forget();
        });
    }
}

// ── Updates section ─────────────────────────────────────────────

#[component]
fn UpdatesSection() -> impl IntoView {
    let i18n = use_i18n();
    let update_state =
        use_context::<RwSignal<UpdateState>>().unwrap_or_else(|| RwSignal::new(UpdateState::None));
    let system_values = Resource::new_blocking(|| (), |_| load_system_values());

    view! {
        <section class="settings-section" id=SECTION_UPDATES>
            <h3 class="section-title">{move || t(i18n.locale.get(), Key::MoreSectionUpdates)}</h3>

            <div class="settings-section-body">
                <Suspense fallback=|| ()>
                    {move || Suspend::new(async move {
                        match system_values.await {
                            Ok(values) => view! {
                                <UpdatesSectionContent admin_unlocked=values.admin_unlocked update_state />
                            }
                                .into_any(),
                            Err(err) => view! { <p class="error">{err.to_string()}</p> }.into_any(),
                        }
                    })}
                </Suspense>
            </div>
        </section>
    }
}

#[component]
fn UpdatesSectionContent(
    admin_unlocked: bool,
    update_state: RwSignal<UpdateState>,
) -> impl IntoView {
    let i18n = use_i18n();

    let on_skip = move |_| {
        if !admin_unlocked {
            return;
        }
        let state = update_state.get_untracked();
        if let UpdateState::Available(ref available) = state {
            let tag = available.tag.clone();
            leptos::task::spawn_local(async move {
                let _ = server_fns::skip_version(tag).await;
                update_state.set(UpdateState::None);
            });
        }
    };

    let version_text = move || {
        let locale = i18n.locale.get();
        let tpl = t(locale, Key::UpdateCurrentVersion);
        tpl.replace("{0}", crate::VERSION)
            .replace("{1}", crate::GIT_HASH)
    };

    view! {
        // Update banner
        {move || {
            let state = update_state.get();
            if let UpdateState::Available(ref available) = state {
                let locale = i18n.locale.get();
                let banner_text = t(locale, Key::UpdateAvailable).replace("{0}", &available.version);
                let release_url = available.release_notes_url.clone();
                Some(view! {
                    <div class="update-banner">
                        <div class="update-banner-title">{banner_text}</div>
                        <div class="update-actions">
                            <Show when=move || admin_unlocked>
                                <A href="/updating" attr:class="form-btn">
                                    {move || t(i18n.locale.get(), Key::UpdateNow)}
                                </A>
                            </Show>
                            <a href=release_url target="_blank" rel="noopener" class="form-btn form-btn-secondary">
                                {move || t(i18n.locale.get(), Key::UpdateViewRelease)}
                            </a>
                        </div>
                        <Show when=move || admin_unlocked>
                            <button class="update-skip-link" on:click=on_skip>
                                {move || t(i18n.locale.get(), Key::UpdateSkip)}
                            </button>
                        </Show>
                    </div>
                })
            } else {
                None
            }
        }}

        // Current version
        <div class="update-version">{version_text}</div>

        {move || {
            if admin_unlocked {
                view! { <AdminUpdateControls update_state /> }.into_any()
            } else {
                view! { <p class="form-hint">{move || t(i18n.locale.get(), Key::SettingsAdminOnlyDisabled)}</p> }.into_any()
            }
        }}
    }
}

#[component]
fn AdminUpdateControls(update_state: RwSignal<UpdateState>) -> impl IntoView {
    let i18n = use_i18n();
    let channel_value = RwSignal::new("stable".to_string());
    let channel = Resource::new(|| (), |_| server_fns::get_update_channel());
    Effect::new(move || {
        if let Some(Ok(val)) = channel.get() {
            channel_value.set(val);
        }
    });
    let check_error = RwSignal::new(Option::<String>::None);
    let checking = RwSignal::new(false);
    let up_to_date = RwSignal::new(false);
    let controls_hydrated = RwSignal::new(false);

    Effect::new(move || {
        controls_hydrated.set(true);
    });

    let run_check = move || {
        checking.set(true);
        check_error.set(None);
        up_to_date.set(false);
        leptos::task::spawn_local(async move {
            match server_fns::check_for_updates().await {
                Ok(Some(available)) => {
                    update_state.set(UpdateState::Available(available));
                }
                Ok(None) => {
                    if matches!(update_state.get_untracked(), UpdateState::Available(_)) {
                        update_state.set(UpdateState::None);
                    }
                    up_to_date.set(true);
                }
                Err(e) => {
                    check_error.set(Some(server_fns::format_error(e)));
                }
            }
            checking.set(false);
        });
    };

    let on_check = move |_| {
        if checking.get_untracked() {
            return;
        }
        run_check();
    };

    let on_channel_change = move |ev: leptos::ev::Event| {
        let value = leptos::prelude::event_target_value(&ev);
        let prev = channel_value.get_untracked();
        channel_value.set(value.clone());
        check_error.set(None);
        up_to_date.set(false);
        leptos::task::spawn_local(async move {
            match server_fns::save_update_channel(value).await {
                Ok(()) => run_check(),
                Err(e) => {
                    channel_value.set(prev);
                    check_error.set(Some(server_fns::format_error(e)));
                }
            }
        });
    };

    view! {
        <div class=move || {
            if controls_hydrated.get() {
                "update-controls-row is-hydrated"
            } else {
                "update-controls-row"
            }
        }>
            <select
                class="form-input"
                on:change=on_channel_change
                prop:value=move || channel_value.get()
            >
                <option value="stable">{move || t(i18n.locale.get(), Key::UpdateChannelStable)}</option>
                <option value="beta">{move || t(i18n.locale.get(), Key::UpdateChannelBeta)}</option>
            </select>
            <button
                class="form-btn form-btn-secondary"
                on:click=on_check
                disabled=move || checking.get()
            >
                {move || {
                    if checking.get() {
                        t(i18n.locale.get(), Key::UpdateChecking).to_string()
                    } else {
                        t(i18n.locale.get(), Key::UpdateCheckButton).to_string()
                    }
                }}
            </button>
        </div>

        <Show when=move || up_to_date.get()>
            <div class="status-msg status-ok">{move || t(i18n.locale.get(), Key::UpdateUpToDate)}</div>
        </Show>
        {move || check_error.get().map(|msg| view! {
            <div class="status-msg status-err">{msg}</div>
        })}
    }
}

fn admin_settings_unlocked(status: &AuthStatus) -> bool {
    !status.auth_required || status.role == AuthRole::Admin
}

// ── Analytics (inline within System section) ────────────────────

#[component]
fn AnalyticsInline(current: bool) -> impl IntoView {
    let i18n = use_i18n();
    view! {
        <div class="analytics-section">
            <h4 class="settings-setting-title">{move || t(i18n.locale.get(), Key::AnalyticsTitle)}</h4>
            <p class="form-hint">{move || t(i18n.locale.get(), Key::AnalyticsDescription)}</p>
            <AnalyticsToggle current />

            <details class="analytics-details">
                <summary>{move || t(i18n.locale.get(), Key::AnalyticsWhatSent)}</summary>
                <ul class="analytics-fields">
                    <li>{move || t(i18n.locale.get(), Key::AnalyticsFieldInstallId)}</li>
                    <li>{move || t(i18n.locale.get(), Key::AnalyticsFieldVersion)}</li>
                    <li>{move || t(i18n.locale.get(), Key::AnalyticsFieldArch)}</li>
                    <li>{move || t(i18n.locale.get(), Key::AnalyticsFieldChannel)}</li>
                </ul>
                <p class="analytics-not-collected">{move || t(i18n.locale.get(), Key::AnalyticsNotCollected)}</p>
            </details>
        </div>
    }
}

#[component]
fn AnalyticsToggle(current: bool) -> impl IntoView {
    let i18n = use_i18n();
    let active = RwSignal::new(current);
    let saving = RwSignal::new(false);
    let status = RwSignal::new(Option::<(bool, String)>::None);

    let on_change = move |_| {
        if saving.get_untracked() {
            return;
        }
        let new_value = !active.get_untracked();
        saving.set(true);
        status.set(None);
        leptos::task::spawn_local(async move {
            match server_fns::save_analytics_preference(new_value).await {
                Ok(()) => {
                    active.set(new_value);
                    let locale = i18n.locale.get_untracked();
                    status.set(Some((true, t(locale, Key::AnalyticsSaved).to_string())));
                }
                Err(e) => {
                    status.set(Some((false, e.to_string())));
                }
            }
            saving.set(false);
        });
    };

    view! {
        <div class="form-field form-field-check">
            <label class="form-label">{move || t(i18n.locale.get(), Key::AnalyticsTitle)}</label>
            <input type="checkbox"
                class="form-checkbox"
                prop:checked=move || active.get()
                on:change=on_change
                disabled=move || saving.get()
            />
        </div>
        <SaveStatus status />
    }
}

// ── Shared child components ─────────────────────────────────────

#[component]
fn RegionSelector(current: String, current_secondary: String, disabled: bool) -> impl IntoView {
    let i18n = use_i18n();
    let active = RwSignal::new(current);
    let active_secondary = RwSignal::new(current_secondary);
    let saving = RwSignal::new(false);
    let status = RwSignal::new(Option::<(bool, String)>::None);

    let options: &[(&str, Key)] = &[
        ("usa", Key::RegionUsa),
        ("europe", Key::RegionEurope),
        ("japan", Key::RegionJapan),
        ("world", Key::RegionWorld),
    ];

    let on_change = move |ev: leptos::ev::Event| {
        let value = leptos::prelude::event_target_value(&ev);
        if disabled || saving.get_untracked() || active.get_untracked() == value {
            return;
        }
        saving.set(true);
        status.set(None);
        let v = value.clone();
        leptos::task::spawn_local(async move {
            match server_fns::save_region_preference(v.clone()).await {
                Ok(()) => {
                    if active_secondary.get_untracked() == v {
                        active_secondary.set(String::new());
                        let _ = server_fns::save_region_preference_secondary(String::new()).await;
                    }
                    active.set(v);
                    let locale = use_i18n().locale.get_untracked();
                    status.set(Some((true, t(locale, Key::RegionSaved).to_string())));
                }
                Err(e) => {
                    status.set(Some((false, e.to_string())));
                }
            }
            saving.set(false);
        });
    };

    let on_change_secondary = move |ev: leptos::ev::Event| {
        let value = leptos::prelude::event_target_value(&ev);
        if disabled || saving.get_untracked() || active_secondary.get_untracked() == value {
            return;
        }
        saving.set(true);
        status.set(None);
        let v = value.clone();
        leptos::task::spawn_local(async move {
            match server_fns::save_region_preference_secondary(v.clone()).await {
                Ok(()) => {
                    active_secondary.set(v);
                    let locale = use_i18n().locale.get_untracked();
                    status.set(Some((true, t(locale, Key::RegionSaved).to_string())));
                }
                Err(e) => {
                    status.set(Some((false, e.to_string())));
                }
            }
            saving.set(false);
        });
    };

    let option_views = options
        .iter()
        .map(|(value, label_key)| {
            let value = *value;
            let label_key = *label_key;
            let is_selected = move || active.read().as_str() == value;
            view! {
                <option value=value selected=is_selected>
                    {move || t(i18n.locale.get(), label_key)}
                </option>
            }
        })
        .collect::<Vec<_>>();

    let secondary_option_views = options
        .iter()
        .map(|(value, label_key)| {
            let value = *value;
            let label_key = *label_key;
            let is_selected = move || active_secondary.read().as_str() == value;
            let is_hidden = move || active.read().as_str() == value;
            view! {
                <option value=value selected=is_selected hidden=is_hidden>
                    {move || t(i18n.locale.get(), label_key)}
                </option>
            }
        })
        .collect::<Vec<_>>();

    view! {
        <div class="form-field">
            <label class="form-label">{move || t(i18n.locale.get(), Key::RegionPrimaryLabel)}</label>
            <select
                class="form-input"
                on:change=on_change
                disabled=move || disabled || saving.get()
            >
                {option_views}
            </select>
        </div>

        <div class="form-field">
            <label class="form-label">{move || t(i18n.locale.get(), Key::RegionSecondaryLabel)}</label>
            <select
                class="form-input"
                on:change=on_change_secondary
                disabled=move || disabled || saving.get()
            >
                <option value="" selected=move || active_secondary.read().is_empty()>
                    {move || t(i18n.locale.get(), Key::RegionNone)}
                </option>
                {secondary_option_views}
            </select>
        </div>
        <SaveStatus status />
    }
}

#[component]
fn TextSizeToggle(current: String) -> impl IntoView {
    let active = RwSignal::new(current);
    let saving = RwSignal::new(false);

    let on_select = move |size: &'static str| {
        if saving.get_untracked() || active.read().as_str() == size {
            return;
        }
        saving.set(true);
        leptos::task::spawn_local(async move {
            if server_fns::save_font_size(size.to_string()).await.is_ok() {
                active.set(size.to_string());
                #[cfg(feature = "hydrate")]
                {
                    if let Some(html) = web_sys::window()
                        .and_then(|w| w.document())
                        .and_then(|d| d.document_element())
                    {
                        let _ = html
                            .class_list()
                            .toggle_with_force("font-large", size == "large");
                    }
                }
            }
            saving.set(false);
        });
    };

    let on_normal = move |_| on_select("normal");
    let on_large = move |_| on_select("large");

    let normal_class = move || {
        if active.read().as_str() == "normal" {
            "text-size-btn text-size-btn-normal active"
        } else {
            "text-size-btn text-size-btn-normal"
        }
    };
    let large_class = move || {
        if active.read().as_str() == "large" {
            "text-size-btn text-size-btn-large active"
        } else {
            "text-size-btn text-size-btn-large"
        }
    };

    view! {
        <div class="text-size-toggle">
            <button
                class=normal_class
                on:click=on_normal
                disabled=move || saving.get()
            >
                "A"
            </button>
            <button
                class=large_class
                on:click=on_large
                disabled=move || saving.get()
            >
                "A"
            </button>
        </div>
    }
}

fn menu_item(icon: &'static str, label_key: Key, href: Option<&'static str>) -> impl IntoView {
    let i18n = use_i18n();
    // Reactive so the label re-translates on locale change, like the rest of
    // the page (was get_untracked, which froze it at first render).
    let label = move || t(i18n.locale.get(), label_key);
    let disabled = href.is_none();
    let target = href.unwrap_or("#");
    let class = if disabled {
        "menu-item menu-item-disabled"
    } else {
        "menu-item"
    };
    let aria_disabled = if disabled { "true" } else { "false" };
    let tabindex = if disabled { "-1" } else { "0" };
    let on_click = move |ev: leptos::ev::MouseEvent| {
        if disabled {
            ev.prevent_default();
        }
    };

    view! {
        <a
            href=target
            class=class
            aria-disabled=aria_disabled
            tabindex=tabindex
            on:click=on_click
        >
            <span class="menu-icon">{icon}</span>
            <span class="menu-label">{label}</span>
            <span class="menu-chevron">{"\u{203A}"}</span>
        </a>
    }
}

#[component]
fn LanguageSelector(
    current_primary: String,
    current_secondary: String,
    disabled: bool,
) -> impl IntoView {
    let i18n = use_i18n();
    let active_primary = RwSignal::new(current_primary);
    let active_secondary = RwSignal::new(current_secondary);
    let saving = RwSignal::new(false);
    let status = RwSignal::new(Option::<(bool, String)>::None);

    let options: &[(&str, Key)] = &[
        ("", Key::LanguageAuto),
        ("en", Key::LanguageEn),
        ("es", Key::LanguageEs),
        ("fr", Key::LanguageFr),
        ("de", Key::LanguageDe),
        ("it", Key::LanguageIt),
        ("ja", Key::LanguageJa),
        ("pt", Key::LanguagePt),
    ];

    let secondary_options: &[(&str, Key)] = &[
        ("", Key::RegionNone),
        ("en", Key::LanguageEn),
        ("es", Key::LanguageEs),
        ("fr", Key::LanguageFr),
        ("de", Key::LanguageDe),
        ("it", Key::LanguageIt),
        ("ja", Key::LanguageJa),
        ("pt", Key::LanguagePt),
    ];

    let on_change_primary = move |ev: leptos::ev::Event| {
        let value = leptos::prelude::event_target_value(&ev);
        if disabled || saving.get_untracked() {
            return;
        }
        saving.set(true);
        status.set(None);
        let secondary = active_secondary.get_untracked();
        let v = value.clone();
        leptos::task::spawn_local(async move {
            match server_fns::save_language_preference(v.clone(), secondary).await {
                Ok(()) => {
                    active_primary.set(v);
                    let locale = use_i18n().locale.get_untracked();
                    status.set(Some((true, t(locale, Key::LanguageSaved).to_string())));
                }
                Err(e) => {
                    status.set(Some((false, e.to_string())));
                }
            }
            saving.set(false);
        });
    };

    let on_change_secondary = move |ev: leptos::ev::Event| {
        let value = leptos::prelude::event_target_value(&ev);
        if disabled || saving.get_untracked() {
            return;
        }
        saving.set(true);
        status.set(None);
        let primary = active_primary.get_untracked();
        let v = value.clone();
        leptos::task::spawn_local(async move {
            match server_fns::save_language_preference(primary, v.clone()).await {
                Ok(()) => {
                    active_secondary.set(v);
                    let locale = use_i18n().locale.get_untracked();
                    status.set(Some((true, t(locale, Key::LanguageSaved).to_string())));
                }
                Err(e) => {
                    status.set(Some((false, e.to_string())));
                }
            }
            saving.set(false);
        });
    };

    let primary_option_views = options
        .iter()
        .map(|(value, label_key)| {
            let value = *value;
            let label_key = *label_key;
            let is_selected = move || active_primary.read().as_str() == value;
            view! {
                <option value=value selected=is_selected>
                    {move || t(i18n.locale.get(), label_key)}
                </option>
            }
        })
        .collect::<Vec<_>>();

    let secondary_option_views = secondary_options
        .iter()
        .map(|(value, label_key)| {
            let value = *value;
            let label_key = *label_key;
            let is_selected = move || active_secondary.read().as_str() == value;
            view! {
                <option value=value selected=is_selected>
                    {move || t(i18n.locale.get(), label_key)}
                </option>
            }
        })
        .collect::<Vec<_>>();

    view! {
        <div class="form-field">
            <label class="form-label">{move || t(i18n.locale.get(), Key::LanguagePrimaryLabel)}</label>
            <select
                class="form-input"
                on:change=on_change_primary
                disabled=move || disabled || saving.get()
            >
                {primary_option_views}
            </select>
        </div>

        <div class="form-field">
            <label class="form-label">{move || t(i18n.locale.get(), Key::LanguageSecondaryLabel)}</label>
            <select
                class="form-input"
                on:change=on_change_secondary
                disabled=move || disabled || saving.get()
            >
                {secondary_option_views}
            </select>
        </div>
        <SaveStatus status />
    }
}

#[component]
fn InfoRow(label_key: Key, #[prop(into)] value: Signal<String>) -> impl IntoView {
    let i18n = use_i18n();

    view! {
        <div class="info-row">
            <span class="info-label">{move || t(i18n.locale.get(), label_key)}</span>
            <span class="info-value">{move || value.get()}</span>
        </div>
    }
}

/// Inline locale selector. Language names are always shown in their own script
/// so users can find their language regardless of the current UI locale.
#[component]
fn LocaleSelector(current: String) -> impl IntoView {
    let i18n = use_i18n();
    let active = RwSignal::new(current);
    let saving = RwSignal::new(false);
    let status = RwSignal::new(Option::<(bool, String)>::None);

    let options: &[(&str, Key)] = &[
        ("auto", Key::LocaleAuto),
        ("en", Key::LocaleEn),
        ("es", Key::LocaleEs),
        ("ja", Key::LocaleJa),
    ];

    let on_change = move |ev: leptos::ev::Event| {
        let value = leptos::prelude::event_target_value(&ev);
        if saving.get_untracked() || active.get_untracked() == value {
            return;
        }
        saving.set(true);
        status.set(None);
        let v = value.clone();
        leptos::task::spawn_local(async move {
            match server_fns::save_locale(v.clone()).await {
                Ok(()) => {
                    if v == "auto" {
                        #[cfg(target_arch = "wasm32")]
                        {
                            if let Some(window) = web_sys::window() {
                                let _ = window.location().reload();
                            }
                        }
                        active.set(v);
                    } else {
                        let locale = Locale::from_code(&v);
                        i18n.set_locale.set(locale);
                        active.set(v);
                        #[cfg(target_arch = "wasm32")]
                        {
                            if let Some(html) = web_sys::window()
                                .and_then(|w| w.document())
                                .and_then(|d| d.document_element())
                            {
                                let _ = html.set_attribute("lang", locale.code());
                            }
                        }
                        let new_locale = i18n.locale.get_untracked();
                        status.set(Some((true, t(new_locale, Key::LocaleSaved).to_string())));
                    }
                }
                Err(e) => {
                    status.set(Some((false, e.to_string())));
                }
            }
            saving.set(false);
        });
    };

    let option_views = options
        .iter()
        .map(|(value, label_key)| {
            let value = *value;
            let label_key = *label_key;
            let is_selected = move || active.read().as_str() == value;
            view! {
                <option value=value selected=is_selected>
                    {move || t(i18n.locale.get(), label_key)}
                </option>
            }
        })
        .collect::<Vec<_>>();

    view! {
        <div class="form-field">
            <select class="form-input" on:change=on_change disabled=move || saving.get()>
                {option_views}
            </select>
        </div>
        <SaveStatus status />
    }
}
