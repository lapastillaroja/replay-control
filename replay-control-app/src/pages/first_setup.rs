use leptos::prelude::*;
#[cfg(feature = "hydrate")]
use leptos_router::NavigateOptions;
#[cfg(feature = "hydrate")]
use leptos_router::hooks::use_navigate;

use crate::i18n::{Key, t, use_i18n};
use crate::server_fns;

#[component]
pub fn FirstSetupPage() -> impl IntoView {
    let i18n = use_i18n();
    let password = RwSignal::new(String::new());
    let saving = RwSignal::new(false);
    let error = RwSignal::new(Option::<String>::None);

    #[cfg(feature = "hydrate")]
    let navigate = StoredValue::new(use_navigate());

    let complete_setup = move || {
        if saving.get_untracked() {
            return;
        }
        let current_password = password.get();
        saving.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            match server_fns::complete_first_setup(current_password).await {
                Ok(_) => {
                    password.set(String::new());
                    saving.set(false);
                    #[cfg(feature = "hydrate")]
                    navigate.get_value()(
                        "/",
                        NavigateOptions {
                            replace: true,
                            ..Default::default()
                        },
                    );
                }
                Err(err) => {
                    error.set(Some(server_fns::format_error(err)));
                    saving.set(false);
                }
            }
        });
    };

    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        complete_setup();
    };

    view! {
        <div class="login-page first-setup-page">
            <div class="login-brand">
                <img
                    class="login-logo"
                    src="/static/branding/logo-oneline-transparent.png"
                    alt="Replay Control"
                />
                <img
                    class="login-character"
                    src="/static/branding/login-character.png"
                    alt=""
                    aria-hidden="true"
                />
            </div>
            <h1 class="login-title">{move || t(i18n.locale.get(), Key::FirstSetupTitle)}</h1>
            <p class="login-intro">{move || t(i18n.locale.get(), Key::FirstSetupBody)}</p>

            <form class="login-form" on:submit=on_submit>
                <section class="apply-section">
                    <h3 class="form-label">{move || t(i18n.locale.get(), Key::FirstSetupPasswordTitle)}</h3>
                    <p class="form-hint">{move || t(i18n.locale.get(), Key::FirstSetupPasswordHint)}</p>
                    <div class="form-field">
                        <label class="form-label" for="first-setup-password">
                            {move || t(i18n.locale.get(), Key::LoginAdminPasswordLabel)}
                        </label>
                        <input
                            id="first-setup-password"
                            class="form-input"
                            type="password"
                            autocomplete="current-password"
                            enterkeyhint="go"
                            disabled=move || saving.get()
                            bind:value=password
                        />
                    </div>
                    {move || error.get().map(|message| {
                        view! { <div class="status-msg status-err login-field-error">{message}</div> }
                    })}
                    <button
                        class="form-btn"
                        type="button"
                        on:click=move |_| complete_setup()
                        disabled=move || saving.get()
                    >
                        {move || {
                            let locale = i18n.locale.get();
                            if saving.get() {
                                t(locale, Key::SettingsSaving)
                            } else {
                                t(locale, Key::FirstSetupSubmit)
                            }
                        }}
                    </button>
                </section>
            </form>
        </div>
    }
}
