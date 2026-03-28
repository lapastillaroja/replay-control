use leptos::prelude::*;
use leptos_router::components::A;
use server_fn::ServerFnError;

use crate::components::reboot_button::RebootButton;
use crate::i18n::{t, use_i18n};
use crate::pages::ErrorDisplay;
use crate::server_fns;
use crate::util::format_size;

#[component]
pub fn MorePage() -> impl IntoView {
    let i18n = use_i18n();
    let info = Resource::new(|| (), |_| server_fns::get_info());
    let region = Resource::new(|| (), |_| server_fns::get_region_preference());
    let region_secondary = Resource::new(|| (), |_| server_fns::get_region_preference_secondary());
    let language = Resource::new(|| (), |_| server_fns::get_language_preference());
    let font_size = Resource::new(|| (), |_| server_fns::get_font_size());

    view! {
        <div class="page more-page">
            <h2 class="page-title">{move || t(i18n.locale.get(), "more.title")}</h2>

            // ── Preferences section ──────────────────────────────
            <section class="more-section">
                <h3 class="more-section-header">{move || t(i18n.locale.get(), "more.section_preferences")}</h3>

                <div class="more-section-body">
                    <div class="menu-list">
                        <MenuItem icon="\u{1F3A8}" label_key="more.skin" href=Some("/more/skin") />
                    </div>

                    <div class="more-inline-setting">
                        <h4 class="more-setting-title">{move || t(i18n.locale.get(), "region.title")}</h4>
                        <p class="form-hint">{move || t(i18n.locale.get(), "region.hint")}</p>
                        <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }>
                            <Transition fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), "common.loading")}</div> }>
                                {move || Suspend::new(async move {
                                    let current = region.await?;
                                    let current_secondary = region_secondary.await?;
                                    Ok::<_, ServerFnError>(view! { <RegionSelector current current_secondary /> })
                                })}
                            </Transition>
                        </ErrorBoundary>
                    </div>

                    <div class="more-inline-setting">
                        <h4 class="more-setting-title">{move || t(i18n.locale.get(), "language.title")}</h4>
                        <p class="form-hint">{move || t(i18n.locale.get(), "language.hint")}</p>
                        <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }>
                            <Transition fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), "common.loading")}</div> }>
                                {move || Suspend::new(async move {
                                    let (primary, secondary) = language.await?;
                                    Ok::<_, ServerFnError>(view! { <LanguageSelector current_primary=primary current_secondary=secondary /> })
                                })}
                            </Transition>
                        </ErrorBoundary>
                    </div>

                    <div class="more-inline-setting">
                        <h4 class="more-setting-title">{move || t(i18n.locale.get(), "more.text_size")}</h4>
                        <p class="form-hint">{move || t(i18n.locale.get(), "more.text_size_hint")}</p>
                        <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }>
                            <Transition fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), "common.loading")}</div> }>
                                {move || Suspend::new(async move {
                                    let current = font_size.await?;
                                    Ok::<_, ServerFnError>(view! { <TextSizeToggle current /> })
                                })}
                            </Transition>
                        </ErrorBoundary>
                    </div>
                </div>
            </section>

            // ── Game Data section ────────────────────────────────
            <section class="more-section">
                <h3 class="more-section-header">{move || t(i18n.locale.get(), "more.section_game_data")}</h3>

                <div class="more-section-body">
                    <div class="menu-list">
                        <MenuItem icon="\u{1F4DA}" label_key="more.metadata" href=Some("/more/metadata") />
                        <MenuItem icon="\u{1F511}" label_key="more.github" href=Some("/more/github") />
                    </div>
                </div>
            </section>

            // ── System section ───────────────────────────────────
            <section class="more-section">
                <h3 class="more-section-header">{move || t(i18n.locale.get(), "more.section_system")}</h3>

                <div class="more-section-body">
                    <div class="menu-list">
                        <MenuItem icon="\u{1F4F6}" label_key="more.wifi" href=Some("/more/wifi") />
                        <MenuItem icon="\u{1F4C1}" label_key="more.nfs" href=Some("/more/nfs") />
                        <MenuItem icon="\u{1F4BB}" label_key="more.hostname" href=Some("/more/hostname") />
                        <MenuItem icon="\u{1F4DC}" label_key="more.logs" href=Some("/more/logs") />
                    </div>

                    <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }>
                        <Transition fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), "common.loading")}</div> }>
                            {move || Suspend::new(async move {
                                let locale = i18n.locale.get();
                                let info = info.await?;
                                Ok::<_, ServerFnError>(view! {
                                    <div class="info-grid">
                                        <InfoRow label=t(locale, "more.storage") value=info.storage_kind.to_uppercase() />
                                        <InfoRow label=t(locale, "more.path") value=info.storage_root.clone() />
                                        <InfoRow label=t(locale, "more.disk_total") value=format_size(info.disk_total_bytes) />
                                        <InfoRow label=t(locale, "more.disk_used") value=format_size(info.disk_used_bytes) />
                                        <InfoRow label=t(locale, "more.disk_available") value=format_size(info.disk_available_bytes) />
                                        <InfoRow label=t(locale, "more.ethernet_ip") value=info.ethernet_ip.unwrap_or_else(|| t(locale, "more.not_connected").to_string()) />
                                        <InfoRow label=t(locale, "more.wifi_ip") value=info.wifi_ip.unwrap_or_else(|| t(locale, "more.not_connected").to_string()) />
                                    </div>
                                })
                            })}
                        </Transition>
                    </ErrorBoundary>

                    <RebootButton />
                </div>
            </section>

            <div class="more-version">
                {format!("v{} ({})", crate::VERSION, crate::GIT_HASH)}
            </div>
        </div>
    }
}

#[component]
fn RegionSelector(current: String, current_secondary: String) -> impl IntoView {
    let i18n = use_i18n();
    let active = RwSignal::new(current);
    let active_secondary = RwSignal::new(current_secondary);
    let saving = RwSignal::new(false);
    let status = RwSignal::new(Option::<(bool, String)>::None);

    let options: &[(&str, &str)] = &[
        ("usa", "region.usa"),
        ("europe", "region.europe"),
        ("japan", "region.japan"),
        ("world", "region.world"),
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
                    // If secondary was set to the new primary value, reset secondary.
                    if active_secondary.get_untracked() == v {
                        active_secondary.set(String::new());
                        let _ = server_fns::save_region_preference_secondary(String::new()).await;
                    }
                    active.set(v);
                    let locale = use_i18n().locale.get_untracked();
                    status.set(Some((true, t(locale, "region.saved").to_string())));
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
                    status.set(Some((true, t(locale, "region.saved").to_string())));
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

    // Secondary dropdown: "None" + all regions except the currently selected primary.
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
            <label class="form-label">{move || t(i18n.locale.get(), "region.primary_label")}</label>
            <select
                class="form-input"
                on:change=on_change
                disabled=move || saving.get()
            >
                {option_views}
            </select>
        </div>

        <div class="form-field">
            <label class="form-label">{move || t(i18n.locale.get(), "region.secondary_label")}</label>
            <select
                class="form-input"
                on:change=on_change_secondary
                disabled=move || saving.get()
            >
                <option value="" selected=move || active_secondary.read().is_empty()>
                    {move || t(i18n.locale.get(), "region.none")}
                </option>
                {secondary_option_views}
            </select>
        </div>
        {move || status.get().map(|(ok, msg)| {
            let class = if ok { "status-msg status-ok" } else { "status-msg status-err" };
            view! { <div class=class>{msg}</div> }
        })}
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
                // Reload the page so the SSR body class updates.
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
fn MenuItem(
    icon: &'static str,
    label_key: &'static str,
    href: Option<&'static str>,
) -> impl IntoView {
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

    // Language options: (value, i18n_key)
    let options: &[(&str, &str)] = &[
        ("", "language.auto"),
        ("en", "language.en"),
        ("es", "language.es"),
        ("fr", "language.fr"),
        ("de", "language.de"),
        ("it", "language.it"),
        ("ja", "language.ja"),
        ("pt", "language.pt"),
    ];

    let secondary_options: &[(&str, &str)] = &[
        ("", "region.none"),
        ("en", "language.en"),
        ("es", "language.es"),
        ("fr", "language.fr"),
        ("de", "language.de"),
        ("it", "language.it"),
        ("ja", "language.ja"),
        ("pt", "language.pt"),
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
                    status.set(Some((true, t(locale, "language.saved").to_string())));
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
                    status.set(Some((true, t(locale, "language.saved").to_string())));
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
            <label class="form-label">{move || t(i18n.locale.get(), "language.primary_label")}</label>
            <select
                class="form-input"
                on:change=on_change_primary
                disabled=move || saving.get()
            >
                {primary_option_views}
            </select>
        </div>

        <div class="form-field">
            <label class="form-label">{move || t(i18n.locale.get(), "language.secondary_label")}</label>
            <select
                class="form-input"
                on:change=on_change_secondary
                disabled=move || saving.get()
            >
                {secondary_option_views}
            </select>
        </div>
        {move || status.get().map(|(ok, msg)| {
            let class = if ok { "status-msg status-ok" } else { "status-msg status-err" };
            view! { <div class=class>{msg}</div> }
        })}
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
