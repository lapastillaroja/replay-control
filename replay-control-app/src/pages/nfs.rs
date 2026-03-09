use leptos::prelude::*;
use leptos_router::components::A;
use server_fn::ServerFnError;

use crate::i18n::{use_i18n, t};
use crate::pages::ErrorDisplay;
use crate::server_fns;

#[component]
pub fn NfsPage() -> impl IntoView {
    let i18n = use_i18n();
    let nfs = Resource::new(|| (), |_| server_fns::get_nfs_config());

    view! {
        <div class="page settings-page">
            <div class="rom-header">
                <A href="/more" attr:class="back-btn">
                    {move || t(i18n.locale.get(), "games.back")}
                </A>
                <h2 class="page-title">{move || t(i18n.locale.get(), "nfs.title")}</h2>
            </div>

            <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }>
                <Suspense fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), "common.loading")}</div> }>
                    {move || Suspend::new(async move {
                        let config = nfs.await?;
                        Ok::<_, ServerFnError>(view! { <NfsForm config /> })
                    })}
                </Suspense>
            </ErrorBoundary>
        </div>
    }
}

#[component]
fn NfsForm(config: server_fns::NfsConfig) -> impl IntoView {
    let i18n = use_i18n();

    let server = RwSignal::new(config.server);
    let share = RwSignal::new(config.share);
    let version = RwSignal::new(config.version);

    let saving = RwSignal::new(false);
    let status = RwSignal::new(Option::<(bool, String)>::None);

    let on_save = move |_| {
        saving.set(true);
        status.set(None);
        let server = server.get();
        let share = share.get();
        let version = version.get();

        leptos::task::spawn_local(async move {
            match server_fns::save_nfs_config(server, share, version).await {
                Ok(()) => {
                    let locale = use_i18n().locale.get_untracked();
                    status.set(Some((true, t(locale, "settings.saved").to_string())));
                }
                Err(e) => {
                    status.set(Some((false, e.to_string())));
                }
            }
            saving.set(false);
        });
    };

    view! {
        <div class="settings-form">
            <div class="form-field">
                <label class="form-label">{move || t(i18n.locale.get(), "nfs.server")}</label>
                <input type="text"
                    class="form-input"
                    bind:value=server
                    placeholder="192.168.1.100 or hostname"
                />
            </div>

            <div class="form-field">
                <label class="form-label">{move || t(i18n.locale.get(), "nfs.share")}</label>
                <input type="text"
                    class="form-input"
                    bind:value=share
                    placeholder="/export/share"
                />
            </div>

            <div class="form-field">
                <label class="form-label">{move || t(i18n.locale.get(), "nfs.version")}</label>
                <select class="form-input" bind:value=version>
                    <option value="4">"NFSv4"</option>
                    <option value="3">"NFSv3"</option>
                </select>
            </div>

            {move || status.get().map(|(ok, msg)| {
                let class = if ok { "status-msg status-ok" } else { "status-msg status-err" };
                view! { <div class=class>{msg}</div> }
            })}

            <button
                class="form-btn"
                on:click=on_save
                disabled=move || saving.get()
            >
                {move || {
                    let locale = i18n.locale.get();
                    if saving.get() { t(locale, "settings.saving") } else { t(locale, "settings.save") }
                }}
            </button>

            <RebootButton />
        </div>
    }
}

#[component]
fn RebootButton() -> impl IntoView {
    let i18n = use_i18n();
    let rebooting = RwSignal::new(false);
    let result = RwSignal::new(Option::<(bool, String)>::None);

    let on_reboot = move |_| {
        rebooting.set(true);
        result.set(None);
        leptos::task::spawn_local(async move {
            match server_fns::reboot_system().await {
                Ok(msg) => result.set(Some((true, msg))),
                Err(e) => result.set(Some((false, e.to_string()))),
            }
            rebooting.set(false);
        });
    };

    view! {
        <div class="apply-section">
            <p class="form-hint">{move || t(i18n.locale.get(), "nfs.hint")}</p>
            <button
                class="form-btn form-btn-secondary"
                on:click=on_reboot
                disabled=move || rebooting.get()
            >
                {move || {
                    let locale = i18n.locale.get();
                    if rebooting.get() { t(locale, "settings.rebooting") } else { t(locale, "settings.reboot") }
                }}
            </button>
            {move || result.get().map(|(ok, msg)| {
                let class = if ok { "status-msg status-ok" } else { "status-msg status-err" };
                view! { <div class=class>{msg}</div> }
            })}
        </div>
    }
}
