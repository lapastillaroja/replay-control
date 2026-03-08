use leptos::prelude::*;
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
                <MenuItem icon="\u{1F4BE}" label_key="more.backup" />
                <MenuItem icon="\u{1F4F6}" label_key="more.wifi" />
                <MenuItem icon="\u{1F4C1}" label_key="more.nfs" />
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
                            </div>
                        })
                    })}
                </Suspense>
            </ErrorBoundary>
        </div>
    }
}

#[component]
fn MenuItem(icon: &'static str, label_key: &'static str) -> impl IntoView {
    let i18n = use_i18n();
    view! {
        <div class="menu-item">
            <span class="menu-icon">{icon}</span>
            <span class="menu-label">{move || t(i18n.locale.get(), label_key)}</span>
        </div>
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
