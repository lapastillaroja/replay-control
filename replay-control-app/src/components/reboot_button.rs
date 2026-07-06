use crate::components::status_message::StatusMessage;
use leptos::prelude::*;

use crate::i18n::{Key, t, use_i18n};
use crate::server_fns;

#[component]
pub fn RebootButton(
    #[prop(optional)] hint: Option<Key>,
    /// Disable the button (off-device, where rebooting RePlayOS is meaningless)
    /// and show a short device-only note in its place.
    #[prop(optional)]
    disabled: bool,
) -> impl IntoView {
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
            {hint.map(|key| view! { <p class="form-hint">{move || t(i18n.locale.get(), key)}</p> })}
            <button
                class="form-btn form-btn-secondary"
                on:click=on_reboot
                disabled=move || rebooting.get() || disabled
            >
                {move || {
                    let locale = i18n.locale.get();
                    if rebooting.get() { t(locale, Key::SettingsRebooting) } else { t(locale, Key::SettingsReboot) }
                }}
            </button>
            {disabled.then(|| view! {
                <p class="form-hint">{move || t(i18n.locale.get(), Key::SettingsDeviceOnlyDisabled)}</p>
            })}
            <StatusMessage status=result />
        </div>
    }
}
