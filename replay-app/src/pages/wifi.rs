use leptos::prelude::*;
use leptos_router::components::A;
use server_fn::ServerFnError;

use crate::i18n::{use_i18n, t};
use crate::pages::ErrorDisplay;
use crate::server_fns;

#[component]
pub fn WifiPage() -> impl IntoView {
    let i18n = use_i18n();
    let wifi = Resource::new(|| (), |_| server_fns::get_wifi_config());

    view! {
        <div class="page settings-page">
            <div class="rom-header">
                <A href="/more" attr:class="back-btn">
                    {move || t(i18n.locale.get(), "games.back")}
                </A>
                <h2 class="page-title">{move || t(i18n.locale.get(), "wifi.title")}</h2>
            </div>

            <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }>
                <Suspense fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), "common.loading")}</div> }>
                    {move || Suspend::new(async move {
                        let config = wifi.await?;
                        Ok::<_, ServerFnError>(view! { <WifiForm config /> })
                    })}
                </Suspense>
            </ErrorBoundary>
        </div>
    }
}

#[component]
fn WifiForm(config: server_fns::WifiConfig) -> impl IntoView {
    let i18n = use_i18n();

    let ssid = RwSignal::new(config.ssid);
    let password = RwSignal::new(String::new());
    let country = RwSignal::new(config.country);
    let mode = RwSignal::new(config.mode);
    let hidden = RwSignal::new(config.hidden);
    let has_password = config.has_password;

    let saving = RwSignal::new(false);
    let status = RwSignal::new(Option::<(bool, String)>::None);

    let on_save = move |_| {
        saving.set(true);
        status.set(None);
        let ssid = ssid.get();
        let password = password.get();
        let country = country.get();
        let mode = mode.get();
        let hidden = hidden.get();

        leptos::task::spawn_local(async move {
            match server_fns::save_wifi_config(ssid, password, country, mode, hidden).await {
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

    let password_placeholder = if has_password {
        "settings.password_keep"
    } else {
        "settings.password_enter"
    };

    view! {
        <div class="settings-form">
            <div class="form-field">
                <label class="form-label">{move || t(i18n.locale.get(), "wifi.ssid")}</label>
                <input type="text"
                    class="form-input"
                    bind:value=ssid
                    placeholder="Network name"
                />
            </div>

            <div class="form-field">
                <label class="form-label">{move || t(i18n.locale.get(), "wifi.password")}</label>
                <input type="password"
                    class="form-input"
                    bind:value=password
                    placeholder=move || t(i18n.locale.get(), password_placeholder)
                />
            </div>

            <div class="form-field">
                <label class="form-label">{move || t(i18n.locale.get(), "wifi.country")}</label>
                <input type="text"
                    class="form-input"
                    bind:value=country
                    placeholder="US, GB, ES, DE..."
                    maxlength=2
                />
            </div>

            <div class="form-field">
                <label class="form-label">{move || t(i18n.locale.get(), "wifi.mode")}</label>
                <select class="form-input" bind:value=mode>
                    <option value="transition">"WPA2/WPA3 (Transition)"</option>
                    <option value="wpa2">"WPA2 Only"</option>
                    <option value="wpa3">"WPA3 Only"</option>
                </select>
            </div>

            <div class="form-field form-field-check">
                <label class="form-label">{move || t(i18n.locale.get(), "wifi.hidden")}</label>
                <input type="checkbox"
                    class="form-checkbox"
                    bind:checked=hidden
                />
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
            <p class="form-hint">{move || t(i18n.locale.get(), "settings.reboot_hint")}</p>
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
