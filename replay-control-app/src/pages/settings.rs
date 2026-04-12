use leptos::prelude::*;
use leptos_router::components::A;
use server_fn::ServerFnError;

use crate::components::reboot_button::RebootButton;
use crate::i18n::{Key, Locale, t, use_i18n};
use crate::server_fns;
use crate::util::format_size;
use replay_control_core::update::UpdateState;

/// Section definitions: (ID, i18n key) — single source of truth for sidebar + scroll-spy.
const SECTIONS: [(&str, Key); 5] = [
    ("settings-appearance", Key::SettingsSectionAppearance),
    ("settings-game-preferences", Key::MoreSectionGamePreferences),
    ("settings-network", Key::SettingsSectionNetwork),
    ("settings-updates", Key::MoreSectionUpdates),
    ("settings-system", Key::SettingsSectionSystem),
];

const SECTION_APPEARANCE: &str = SECTIONS[0].0;
const SECTION_GAME_PREFS: &str = SECTIONS[1].0;
const SECTION_NETWORK: &str = SECTIONS[2].0;
const SECTION_UPDATES: &str = SECTIONS[3].0;
const SECTION_SYSTEM: &str = SECTIONS[4].0;

#[component]
pub fn SettingsPage() -> impl IntoView {
    let i18n = use_i18n();
    let info = Resource::new(|| (), |_| server_fns::get_info());
    let locale_res = Resource::new(|| (), |_| server_fns::get_locale());
    let region = Resource::new(|| (), |_| server_fns::get_region_preference());
    let region_secondary = Resource::new(|| (), |_| server_fns::get_region_preference_secondary());
    let language = Resource::new(|| (), |_| server_fns::get_language_preference());
    let font_size = Resource::new(|| (), |_| server_fns::get_font_size());

    let active_section = RwSignal::new(SECTION_APPEARANCE.to_string());

    view! {
        <div class="page settings-page">
            <h2 class="page-title">{move || t(i18n.locale.get(), Key::SettingsTitle)}</h2>

            <div class="settings-layout">
                <SettingsSidebar active_section />

                <div class="settings-content">
                    // ── Appearance ───────────────────────────────────
                    <section class="settings-section" id=SECTION_APPEARANCE>
                        <h3 class="settings-section-header">{move || t(i18n.locale.get(), Key::SettingsSectionAppearance)}</h3>

                        <div class="settings-section-body">
                            <div class="menu-list">
                                <MenuItem icon="\u{1F3A8}" label_key=Key::MoreSkin href=Some("/settings/skin") />
                            </div>

                            <div class="settings-inline-setting">
                                <h4 class="settings-setting-title">{move || t(i18n.locale.get(), Key::MoreTextSize)}</h4>
                                <p class="form-hint">{move || t(i18n.locale.get(), Key::MoreTextSizeHint)}</p>
                                <Transition fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), Key::CommonLoading)}</div> }>
                                    {move || Suspend::new(async move {
                                        let current = font_size.await?;
                                        Ok::<_, ServerFnError>(view! { <TextSizeToggle current /> })
                                    })}
                                </Transition>
                            </div>

                            <div class="settings-inline-setting">
                                <h4 class="settings-setting-title">{move || t(i18n.locale.get(), Key::LocaleTitle)}</h4>
                                <Transition fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), Key::CommonLoading)}</div> }>
                                    {move || Suspend::new(async move {
                                        let current = locale_res.await.unwrap_or_default();
                                        Ok::<_, ServerFnError>(view! { <LocaleSelector current /> })
                                    })}
                                </Transition>
                            </div>
                        </div>
                    </section>

                    // ── Game Preferences ─────────────────────────────
                    <section class="settings-section" id=SECTION_GAME_PREFS>
                        <h3 class="settings-section-header">{move || t(i18n.locale.get(), Key::MoreSectionGamePreferences)}</h3>

                        <div class="settings-section-body">
                            <div class="settings-inline-setting">
                                <h4 class="settings-setting-title">{move || t(i18n.locale.get(), Key::RegionTitle)}</h4>
                                <p class="form-hint">{move || t(i18n.locale.get(), Key::RegionHint)}</p>
                                <Transition fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), Key::CommonLoading)}</div> }>
                                    {move || Suspend::new(async move {
                                        let current = region.await?;
                                        let current_secondary = region_secondary.await?;
                                        Ok::<_, ServerFnError>(view! { <RegionSelector current current_secondary /> })
                                    })}
                                </Transition>
                            </div>

                            <div class="settings-inline-setting">
                                <h4 class="settings-setting-title">{move || t(i18n.locale.get(), Key::LanguageTitle)}</h4>
                                <p class="form-hint">{move || t(i18n.locale.get(), Key::LanguageHint)}</p>
                                <Transition fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), Key::CommonLoading)}</div> }>
                                    {move || Suspend::new(async move {
                                        let (primary, secondary) = language.await?;
                                        Ok::<_, ServerFnError>(view! { <LanguageSelector current_primary=primary current_secondary=secondary /> })
                                    })}
                                </Transition>
                            </div>

                            <div class="menu-list">
                                <MenuItem icon="\u{1F4DA}" label_key=Key::MoreMetadata href=Some("/settings/metadata") />
                            </div>
                        </div>
                    </section>

                    // ── Network ──────────────────────────────────────
                    <section class="settings-section" id=SECTION_NETWORK>
                        <h3 class="settings-section-header">{move || t(i18n.locale.get(), Key::SettingsSectionNetwork)}</h3>

                        <div class="settings-section-body">
                            <div class="menu-list">
                                <MenuItem icon="\u{1F4F6}" label_key=Key::MoreWifi href=Some("/settings/wifi") />
                                <MenuItem icon="\u{1F4C1}" label_key=Key::MoreNfs href=Some("/settings/nfs") />
                                <MenuItem icon="\u{1F4BB}" label_key=Key::MoreHostname href=Some("/settings/hostname") />
                                <MenuItem icon="\u{1F512}" label_key=Key::MorePassword href=Some("/settings/password") />
                            </div>
                        </div>
                    </section>

                    // ── Updates ──────────────────────────────────────
                    <UpdatesSection />

                    // ── System ───────────────────────────────────────
                    <section class="settings-section" id=SECTION_SYSTEM>
                        <h3 class="settings-section-header">{move || t(i18n.locale.get(), Key::SettingsSectionSystem)}</h3>

                        <div class="settings-section-body">
                            <Transition fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), Key::CommonLoading)}</div> }>
                                {move || Suspend::new(async move {
                                    let locale = i18n.locale.get();
                                    let info = info.await?;
                                    Ok::<_, ServerFnError>(view! {
                                        <div class="info-grid">
                                            <InfoRow label=t(locale, Key::MoreStorage) value=info.storage_kind.to_uppercase() />
                                            <InfoRow label=t(locale, Key::MorePath) value=info.storage_root.clone() />
                                            <InfoRow label=t(locale, Key::MoreDiskTotal) value=format_size(info.disk_total_bytes) />
                                            <InfoRow label=t(locale, Key::MoreDiskUsed) value=format_size(info.disk_used_bytes) />
                                            <InfoRow label=t(locale, Key::MoreDiskAvailable) value=format_size(info.disk_available_bytes) />
                                            <InfoRow label=t(locale, Key::MoreEthernetIp) value=info.ethernet_ip.unwrap_or_else(|| t(locale, Key::MoreNotConnected).to_string()) />
                                            <InfoRow label=t(locale, Key::MoreWifiIp) value=info.wifi_ip.unwrap_or_else(|| t(locale, Key::MoreNotConnected).to_string()) />
                                        </div>
                                    })
                                })}
                            </Transition>

                            <div class="menu-list">
                                <MenuItem icon="\u{1F4DC}" label_key=Key::MoreLogs href=Some("/settings/logs") />
                                <MenuItem icon="\u{1F511}" label_key=Key::MoreGithub href=Some("/settings/github") />
                            </div>

                            <AnalyticsInline />

                            <div class="settings-reboot">
                                <RebootButton />
                            </div>
                        </div>
                    </section>
                </div>
            </div>
        </div>

        <ScrollSpy active_section />
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
        Effect::new(move || {
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

        // Re-check visible section on route changes (back-navigation from sub-pages).
        // IntersectionObserver doesn't fire for elements already in the viewport.
        let pathname = leptos_router::hooks::use_location().pathname;
        Effect::new(move || {
            let path = pathname.get();
            if path != "/settings" {
                return;
            }

            let Some(window) = web_sys::window() else {
                return;
            };
            let Some(doc) = window.document() else {
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
        leptos::task::spawn_local(async move {
            if server_fns::save_update_channel(value).await.is_ok() {
                run_check();
            }
        });
    };

    let on_skip = move |_| {
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
        <section class="settings-section" id=SECTION_UPDATES>
            <h3 class="settings-section-header">{move || t(i18n.locale.get(), Key::MoreSectionUpdates)}</h3>

            <div class="settings-section-body">
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
                                    <A href="/updating" attr:class="form-btn">
                                        {move || t(i18n.locale.get(), Key::UpdateNow)}
                                    </A>
                                    <a href=release_url target="_blank" rel="noopener" class="form-btn form-btn-secondary">
                                        {move || t(i18n.locale.get(), Key::UpdateViewRelease)}
                                    </a>
                                </div>
                                <button class="update-skip-link" on:click=on_skip>
                                    {move || t(i18n.locale.get(), Key::UpdateSkip)}
                                </button>
                            </div>
                        })
                    } else {
                        None
                    }
                }}

                // Current version
                <div class="update-version">{version_text}</div>

                // Controls row: channel + check button
                <div class="update-controls-row">
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

                // Status messages
                <Show when=move || up_to_date.get()>
                    <div class="status-msg status-ok">{move || t(i18n.locale.get(), Key::UpdateUpToDate)}</div>
                </Show>
                {move || check_error.get().map(|msg| view! {
                    <div class="status-msg status-err">{msg}</div>
                })}
            </div>
        </section>
    }
}

// ── Analytics (inline within System section) ────────────────────

#[component]
fn AnalyticsInline() -> impl IntoView {
    let i18n = use_i18n();
    let analytics = Resource::new(|| (), |_| server_fns::get_analytics_preference());

    view! {
        <div class="analytics-section">
            <h4 class="settings-setting-title">{move || t(i18n.locale.get(), Key::AnalyticsTitle)}</h4>
            <p class="form-hint">{move || t(i18n.locale.get(), Key::AnalyticsDescription)}</p>
            <Transition fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), Key::CommonLoading)}</div> }>
                {move || Suspend::new(async move {
                    let current = analytics.await.unwrap_or(true);
                    Ok::<_, ServerFnError>(view! { <AnalyticsToggle current /> })
                })}
            </Transition>

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
                    let locale = use_i18n().locale.get_untracked();
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
fn RegionSelector(current: String, current_secondary: String) -> impl IntoView {
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
        if saving.get_untracked() || active.get_untracked() == value {
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
        if saving.get_untracked() || active_secondary.get_untracked() == value {
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
                disabled=move || saving.get()
            >
                {option_views}
            </select>
        </div>

        <div class="form-field">
            <label class="form-label">{move || t(i18n.locale.get(), Key::RegionSecondaryLabel)}</label>
            <select
                class="form-input"
                on:change=on_change_secondary
                disabled=move || saving.get()
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
                    let _ = web_sys::window().and_then(|w| w.location().reload().ok());
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

#[component]
fn MenuItem(icon: &'static str, label_key: Key, href: Option<&'static str>) -> impl IntoView {
    let i18n = use_i18n();
    let content = view! {
        <span class="menu-icon">{icon}</span>
        <span class="menu-label">{move || t(i18n.locale.get(), label_key)}</span>
    };

    if let Some(href) = href {
        view! {
            <A href=href attr:class="menu-item">
                {content}
                <span class="menu-chevron">{"\u{203A}"}</span>
            </A>
        }
        .into_any()
    } else {
        view! {
            <div class="menu-item menu-item-disabled">
                {content}
            </div>
        }
        .into_any()
    }
}

#[component]
fn LanguageSelector(current_primary: String, current_secondary: String) -> impl IntoView {
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
        if saving.get_untracked() {
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
        if saving.get_untracked() {
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
                disabled=move || saving.get()
            >
                {primary_option_views}
            </select>
        </div>

        <div class="form-field">
            <label class="form-label">{move || t(i18n.locale.get(), Key::LanguageSecondaryLabel)}</label>
            <select
                class="form-input"
                on:change=on_change_secondary
                disabled=move || saving.get()
            >
                {secondary_option_views}
            </select>
        </div>
        <SaveStatus status />
    }
}

#[component]
fn InfoRow(label: &'static str, value: String) -> impl IntoView {
    view! {
        <div class="info-row">
            <span class="info-label">{label}</span>
            <span class="info-value">{value}</span>
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
