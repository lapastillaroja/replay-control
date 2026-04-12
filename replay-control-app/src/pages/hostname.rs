use leptos::prelude::*;
use leptos_router::components::A;
use server_fn::ServerFnError;

use crate::i18n::{Key, t, use_i18n};
use crate::server_fns;

#[component]
pub fn HostnamePage() -> impl IntoView {
    let i18n = use_i18n();
    let hostname = Resource::new_blocking(|| (), |_| server_fns::get_hostname());

    view! {
        <div class="page settings-page">
            <div class="rom-header">
                <A href="/settings" attr:class="back-btn">
                    {move || t(i18n.locale.get(), Key::GamesBack)}
                </A>
                <h2 class="page-title">{move || t(i18n.locale.get(), Key::HostnameTitle)}</h2>
            </div>

            <Suspense fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), Key::CommonLoading)}</div> }>
                {move || Suspend::new(async move {
                    let current = hostname.await?;
                    Ok::<_, ServerFnError>(view! { <HostnameForm current /> })
                })}
            </Suspense>
        </div>
    }
}

#[component]
fn HostnameForm(current: String) -> impl IntoView {
    let i18n = use_i18n();

    let hostname = RwSignal::new(current);
    let saving = RwSignal::new(false);
    let status = RwSignal::new(Option::<(bool, String)>::None);

    let on_save = move |_| {
        saving.set(true);
        status.set(None);
        let value = hostname.get();

        leptos::task::spawn_local(async move {
            match server_fns::save_hostname(value).await {
                Ok(_) => {
                    let locale = use_i18n().locale.get_untracked();
                    status.set(Some((true, t(locale, Key::HostnameSaved).to_string())));
                }
                Err(e) => {
                    let locale = use_i18n().locale.get_untracked();
                    let raw = e.to_string();
                    let msg = if raw.contains("Invalid") || raw.contains("invalid") {
                        t(locale, Key::HostnameInvalid).to_string()
                    } else {
                        raw
                    };
                    status.set(Some((false, msg)));
                }
            }
            saving.set(false);
        });
    };

    view! {
        <div class="settings-form">
            <div class="form-field">
                <label class="form-label">{move || t(i18n.locale.get(), Key::HostnameLabel)}</label>
                <input type="text"
                    class="form-input"
                    bind:value=hostname
                    placeholder="replay"
                    maxlength=63
                />
                <p class="form-hint">{move || t(i18n.locale.get(), Key::HostnameHint)}</p>
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
                    if saving.get() { t(locale, Key::SettingsSaving) } else { t(locale, Key::SettingsSave) }
                }}
            </button>
        </div>
    }
}
