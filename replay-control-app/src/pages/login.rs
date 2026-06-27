use leptos::prelude::*;
#[cfg(feature = "hydrate")]
use leptos_router::hooks::use_query_map;
use server_fn::ServerFnError;

use crate::i18n::{Key, t, use_i18n};
use crate::server_fns;
use crate::util::numeric_code;
#[cfg(feature = "hydrate")]
use crate::util::sanitize_next_path;
use replay_control_core::auth::{AuthRole, AuthStatus};

#[component]
pub fn LoginPage() -> impl IntoView {
    let i18n = use_i18n();
    let status_resource = Resource::new(|| (), |_| server_fns::get_auth_status());

    view! {
        <div class="login-page">
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
            <h1 class="login-title">{move || t(i18n.locale.get(), Key::LoginWelcomeTitle)}</h1>
            <p class="login-intro">{move || t(i18n.locale.get(), Key::LoginWelcomeBody)}</p>
            <Suspense fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), Key::CommonLoading)}</div> }>
                {move || Suspend::new(async move {
                    let status = status_resource.await?;
                    Ok::<_, ServerFnError>(view! { <LoginForm initial=status /> })
                })}
            </Suspense>
        </div>
    }
}

#[component]
fn LoginForm(initial: AuthStatus) -> impl IntoView {
    let i18n = use_i18n();
    #[cfg(feature = "hydrate")]
    let next_after_login = {
        let query = use_query_map();
        StoredValue::new(sanitize_next_path(query.get_untracked().get("next")))
    };
    let auth_status = RwSignal::new(initial);
    let user_code_ref = NodeRef::<leptos::html::Input>::new();
    let admin_password_ref = NodeRef::<leptos::html::Input>::new();
    let saving_user = RwSignal::new(false);
    let saving_admin = RwSignal::new(false);
    let user_error = RwSignal::new(Option::<String>::None);
    let admin_error = RwSignal::new(Option::<String>::None);

    let continue_after_login = move || {
        #[cfg(feature = "hydrate")]
        {
            let target = next_after_login.get_value();
            let _ = web_sys::window().and_then(|window| window.location().assign(&target).ok());
        }
    };

    let login_user = move || {
        if saving_user.get_untracked() {
            return;
        }
        let code = user_code_ref
            .get()
            .map(|input| numeric_code(&input.value(), 6))
            .unwrap_or_default();
        saving_user.set(true);
        user_error.set(None);
        leptos::task::spawn_local(async move {
            match server_fns::login_with_replay_code(code).await {
                Ok(status) => {
                    auth_status.set(status);
                    if let Some(input) = user_code_ref.get_untracked() {
                        input.set_value("");
                    }
                    saving_user.set(false);
                    continue_after_login();
                }
                Err(error) => {
                    user_error.set(Some(server_fns::format_error(error)));
                    saving_user.set(false);
                }
            }
        });
    };
    let on_user_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        login_user();
    };
    let on_user_code_input = move |ev| {
        let value = numeric_code(&event_target_value(&ev), 6);
        if let Some(input) = user_code_ref.get_untracked() {
            input.set_value(&value);
        }
    };

    let login_admin = move || {
        if saving_admin.get_untracked() {
            return;
        }
        let password = admin_password_ref
            .get()
            .map(|input| input.value())
            .unwrap_or_default();
        saving_admin.set(true);
        admin_error.set(None);
        leptos::task::spawn_local(async move {
            match server_fns::login_admin(password).await {
                Ok(status) => {
                    auth_status.set(status);
                    if let Some(input) = admin_password_ref.get_untracked() {
                        input.set_value("");
                    }
                    saving_admin.set(false);
                    continue_after_login();
                }
                Err(error) => {
                    admin_error.set(Some(server_fns::format_error(error)));
                    saving_admin.set(false);
                }
            }
        });
    };
    let on_admin_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        login_admin();
    };

    view! {
        <div class="login-form">
            <Show when=move || {
                let status = auth_status.read();
                status.auth_required && status.role != AuthRole::Anonymous
            }>
                <AuthenticatedRedirect />
            </Show>

            <Show when=move || !auth_status.read().auth_required>
                <section class="apply-section">
                    <h3 class="form-label">{move || t(i18n.locale.get(), Key::LoginStandaloneOpenTitle)}</h3>
                    <p class="form-hint">{move || t(i18n.locale.get(), Key::LoginStandaloneOpenHint)}</p>
                    <a class="form-btn" href="/">{move || t(i18n.locale.get(), Key::LoginContinue)}</a>
                </section>
            </Show>

            <Show when=move || {
                let status = auth_status.read();
                status.auth_required && status.role == AuthRole::Anonymous
            }>
                <form class="apply-section" on:submit=on_user_submit>
                    <h3 class="form-label">{move || t(i18n.locale.get(), Key::LoginUserTitle)}</h3>
                    <p class="form-hint">{move || t(i18n.locale.get(), Key::LoginUserCodeHint)}</p>
                    <div class="form-field">
                        <label class="form-label" for="login-net-control-code">
                            {move || t(i18n.locale.get(), Key::LoginUserCodeLabel)}
                        </label>
                        <input
                            node_ref=user_code_ref
                            id="login-net-control-code"
                            class="form-input login-code-input"
                            type="text"
                            inputmode="numeric"
                            pattern="[0-9]*"
                            maxlength=6
                            placeholder="123456"
                            autocomplete="one-time-code"
                            enterkeyhint="go"
                            disabled=move || saving_user.get()
                            on:input=on_user_code_input
                        />
                    </div>
                    {move || user_error.get().map(|message| {
                        view! { <div class="status-msg status-err login-field-error">{message}</div> }
                    })}
                    <button
                        class="form-btn"
                        type="button"
                        on:click=move |_| login_user()
                        disabled=move || saving_user.get()
                    >
                        {move || {
                            let locale = i18n.locale.get();
                            if saving_user.get() {
                                t(locale, Key::SettingsSaving)
                            } else {
                                t(locale, Key::LoginUserSubmit)
                            }
                        }}
                    </button>
                </form>
            </Show>

            <Show when=move || {
                let status = auth_status.read();
                status.auth_required && status.role == AuthRole::Anonymous
            }>
                <form class="apply-section" on:submit=on_admin_submit>
                    <h3 class="form-label">{move || t(i18n.locale.get(), Key::LoginAdminTitle)}</h3>
                    <p class="form-hint">{move || t(i18n.locale.get(), Key::LoginAdminHint)}</p>
                    <div class="form-field">
                        <label class="form-label" for="login-admin-password">
                            {move || t(i18n.locale.get(), Key::LoginAdminPasswordLabel)}
                        </label>
                        <input
                            node_ref=admin_password_ref
                            id="login-admin-password"
                            class="form-input"
                            type="password"
                            autocomplete="current-password"
                            enterkeyhint="go"
                            disabled=move || saving_admin.get()
                        />
                    </div>
                    {move || admin_error.get().map(|message| {
                        view! { <div class="status-msg status-err login-field-error">{message}</div> }
                    })}
                    <button
                        class="form-btn"
                        type="button"
                        on:click=move |_| login_admin()
                        disabled=move || saving_admin.get()
                    >
                        {move || {
                            let locale = i18n.locale.get();
                            if saving_admin.get() {
                                t(locale, Key::SettingsSaving)
                            } else {
                                t(locale, Key::LoginAdminSubmit)
                            }
                        }}
                    </button>
                </form>
            </Show>

        </div>
    }
}

#[component]
fn AuthenticatedRedirect() -> impl IntoView {
    #[cfg(feature = "hydrate")]
    {
        Effect::new(move || {
            let _ = web_sys::window().and_then(|window| window.location().assign("/").ok());
        });
    }
}
