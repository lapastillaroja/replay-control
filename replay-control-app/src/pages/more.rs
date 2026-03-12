use leptos::prelude::*;
use leptos_router::components::A;
use server_fn::ServerFnError;

use crate::i18n::{t, use_i18n};
use crate::pages::ErrorDisplay;
use crate::server_fns;
use crate::util::format_size;

#[component]
pub fn MorePage() -> impl IntoView {
    let i18n = use_i18n();
    let info = Resource::new(|| (), |_| server_fns::get_info());
    let region = Resource::new(|| (), |_| server_fns::get_region_preference());

    view! {
        <div class="page more-page">
            <h2 class="page-title">{move || t(i18n.locale.get(), "more.title")}</h2>

            <div class="menu-list">
                <MenuItem icon="\u{1F3A8}" label_key="more.skin" href=Some("/more/skin") />
                <MenuItem icon="\u{1F4F6}" label_key="more.wifi" href=Some("/more/wifi") />
                <MenuItem icon="\u{1F4C1}" label_key="more.nfs" href=Some("/more/nfs") />
                <MenuItem icon="\u{1F4BB}" label_key="more.hostname" href=Some("/more/hostname") />
                <MenuItem icon="\u{1F4DA}" label_key="more.metadata" href=Some("/more/metadata") />
                <MenuItem icon="\u{1F4DC}" label_key="more.logs" href=Some("/more/logs") />
            </div>

            <h3 class="section-title">{move || t(i18n.locale.get(), "region.title")}</h3>
            <p class="form-hint">{move || t(i18n.locale.get(), "region.hint")}</p>
            <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }>
                <Transition fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), "common.loading")}</div> }>
                    {move || Suspend::new(async move {
                        let current = region.await?;
                        Ok::<_, ServerFnError>(view! { <RegionSelector current /> })
                    })}
                </Transition>
            </ErrorBoundary>

            <h3 class="section-title">{move || t(i18n.locale.get(), "more.system_info")}</h3>
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

        </div>
    }
}

#[component]
fn RegionSelector(current: String) -> impl IntoView {
    let i18n = use_i18n();
    let active = RwSignal::new(current);
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

    let option_views = options
        .iter()
        .map(|(value, label_key)| {
            let value = *value;
            let label_key = *label_key;
            let is_selected = {
                let active = active;
                move || active.read().as_str() == value
            };
            view! {
                <option value=value selected=is_selected>
                    {move || t(i18n.locale.get(), label_key)}
                </option>
            }
        })
        .collect::<Vec<_>>();

    view! {
        <div class="form-field">
            <select
                class="form-input"
                on:change=on_change
                disabled=move || saving.get()
            >
                {option_views}
            </select>
        </div>
        {move || status.get().map(|(ok, msg)| {
            let class = if ok { "status-msg status-ok" } else { "status-msg status-err" };
            view! { <div class=class>{msg}</div> }
        })}
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
fn InfoRow(label: &'static str, value: String) -> impl IntoView {
    view! {
        <div class="info-row">
            <span class="info-label">{label}</span>
            <span class="info-value">{value}</span>
        </div>
    }
}
