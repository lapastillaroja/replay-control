use leptos::prelude::*;

use crate::i18n::{t, use_i18n};
use crate::server_fns;

#[component]
pub fn RebootButton() -> impl IntoView {
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
