use leptos::prelude::*;

use crate::i18n::{I18nContext, Key, t, use_i18n};
use crate::server_fns;

#[component]
pub fn PasswordForm(#[prop(optional)] i18n: Option<I18nContext>) -> impl IntoView {
    let i18n = i18n.unwrap_or_else(use_i18n);
    let locale_signal = i18n.locale;

    let current_password = RwSignal::new(String::new());
    let new_password = RwSignal::new(String::new());
    let confirm_password = RwSignal::new(String::new());
    let saving = RwSignal::new(false);
    let status = RwSignal::new(Option::<(bool, String)>::None);

    let on_save = move |_| {
        let locale = i18n.locale.get();
        let current = current_password.get();
        let new_pw = new_password.get();
        let confirm = confirm_password.get();

        // Client-side validation.
        if current.is_empty() || new_pw.is_empty() || confirm.is_empty() {
            status.set(Some((false, t(locale, Key::PasswordEmpty).to_string())));
            return;
        }
        if new_pw != confirm {
            status.set(Some((false, t(locale, Key::PasswordMismatch).to_string())));
            return;
        }

        saving.set(true);
        status.set(None);

        leptos::task::spawn_local(async move {
            match server_fns::change_root_password(current, new_pw).await {
                Ok(server_msg) => {
                    // The server returns either the success message or a dev-mode skip message.
                    // Map both to appropriate translated keys.
                    let locale = locale_signal.get_untracked();
                    let msg = if server_msg.contains("skipped") {
                        t(locale, Key::PasswordDevSkip).to_string()
                    } else {
                        t(locale, Key::PasswordSuccess).to_string()
                    };
                    status.set(Some((true, msg)));
                    // Clear fields on success.
                    current_password.set(String::new());
                    new_password.set(String::new());
                    confirm_password.set(String::new());
                }
                Err(e) => {
                    let locale = locale_signal.get_untracked();
                    let raw = server_fns::format_error(e);
                    // Map known server-side error strings to translated keys.
                    let msg = if raw.contains("incorrect") {
                        t(locale, Key::PasswordWrongCurrent).to_string()
                    } else if raw.contains("empty") {
                        t(locale, Key::PasswordEmpty).to_string()
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
                <label class="form-label">{move || t(i18n.locale.get(), Key::PasswordCurrent)}</label>
                <input type="password"
                    class="form-input"
                    bind:value=current_password
                    autocomplete="current-password"
                />
            </div>

            <div class="form-field">
                <label class="form-label">{move || t(i18n.locale.get(), Key::PasswordNew)}</label>
                <input type="password"
                    class="form-input"
                    bind:value=new_password
                    autocomplete="new-password"
                />
            </div>

            <div class="form-field">
                <label class="form-label">{move || t(i18n.locale.get(), Key::PasswordConfirm)}</label>
                <input type="password"
                    class="form-input"
                    bind:value=confirm_password
                    autocomplete="new-password"
                />
            </div>

            <p class="form-hint">{move || t(i18n.locale.get(), Key::PasswordDeployHint)}</p>

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
                    if saving.get() { t(locale, Key::SettingsSaving) } else { t(locale, Key::PasswordSave) }
                }}
            </button>
        </div>
    }
}
