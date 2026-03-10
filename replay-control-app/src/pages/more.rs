use leptos::prelude::*;
use leptos_router::components::A;
use server_fn::ServerFnError;

use crate::i18n::{use_i18n, t};
use crate::pages::ErrorDisplay;
use crate::server_fns;
use crate::util::format_size;

#[component]
pub fn MorePage() -> impl IntoView {
    let i18n = use_i18n();
    let info = Resource::new(|| (), |_| server_fns::get_info());

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

            <h3 class="section-title">{move || t(i18n.locale.get(), "more.system_info")}</h3>
            <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }>
                <Suspense fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), "common.loading")}</div> }>
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
                </Suspense>
            </ErrorBoundary>

        </div>
    }
}

#[component]
fn MenuItem(icon: &'static str, label_key: &'static str, href: Option<&'static str>) -> impl IntoView {
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
        }.into_any()
    } else {
        view! {
            <div class="menu-item menu-item-disabled">
                {content}
            </div>
        }.into_any()
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
