use crate::components::status_message::StatusMessage;
use leptos::prelude::*;
#[cfg(feature = "hydrate")]
use leptos_router::NavigateOptions;
use leptos_router::components::A;
#[cfg(feature = "hydrate")]
use leptos_router::hooks::use_navigate;
#[cfg(feature = "hydrate")]
use leptos_router::hooks::use_query_map;
use server_fn::ServerFnError;

use crate::components::confirm_dialog::use_confirm_dialog;
use crate::components::device_only_notice::DeviceOnlyNotice;
use crate::i18n::{Key, t, use_i18n};
use crate::pages::password::PasswordForm;
use crate::server_fns;
use crate::server_fns::TlsCertificateInfo;
#[cfg(feature = "hydrate")]
use crate::util::sanitize_next_path;
use replay_control_core::auth::{AuthRole, AuthStatus};

#[component]
pub fn AccessSecurityPage() -> impl IntoView {
    let i18n = use_i18n();
    let snapshot = Resource::new_blocking(
        || (),
        |_| async {
            let status = server_fns::get_auth_status().await?;
            let on_device = server_fns::get_mode()
                .await
                .map(|profile| profile.is_device())
                .unwrap_or(false);
            let admin_timeout = if admin_settings_unlocked(&status) {
                server_fns::get_admin_session_timeout().await.ok()
            } else {
                None
            };
            let certificate = server_fns::get_tls_certificate_info().await.ok();
            Ok::<_, ServerFnError>((status, on_device, admin_timeout, certificate))
        },
    );

    view! {
        <div class="page settings-page">
            <div class="rom-header">
                <A href="/settings" attr:class="back-btn">
                    {move || t(i18n.locale.get(), Key::GamesBack)}
                </A>
                <h2 class="page-title">{move || t(i18n.locale.get(), Key::AccessSecurityTitle)}</h2>
            </div>

            <div class="settings-form">
                <Suspense fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), Key::CommonLoading)}</div> }>
                    {move || match snapshot.get() {
                        Some(Ok((status, on_device, admin_timeout, certificate))) => view! {
                            <AccessSecurityContent
                                initial_status=status
                                initial_admin_timeout=admin_timeout
                                initial_certificate=certificate
                                on_device
                            />
                        }
                            .into_any(),
                        Some(Err(err)) => view! { <p class="error">{err.to_string()}</p> }.into_any(),
                        None => view! { <div class="loading">{move || t(i18n.locale.get(), Key::CommonLoading)}</div> }
                            .into_any(),
                    }}
                </Suspense>
            </div>
        </div>
    }
}

#[component]
fn AccessSecurityContent(
    initial_status: AuthStatus,
    initial_admin_timeout: Option<String>,
    initial_certificate: Option<TlsCertificateInfo>,
    on_device: bool,
) -> impl IntoView {
    let i18n = use_i18n();
    let status = RwSignal::new(initial_status);
    let admin_unlocked = move || admin_settings_unlocked(&status.read());
    let auth_active = move || status.read().auth_required;

    let certificate = RwSignal::new(initial_certificate);
    // The certificate info is admin-only, so a page that loaded as a non-admin
    // fetched nothing. Re-fetch once the session is elevated to admin.
    Effect::new(move |_| {
        if admin_unlocked() && certificate.get_untracked().is_none() {
            leptos::task::spawn_local(async move {
                if let Ok(info) = server_fns::get_tls_certificate_info().await {
                    certificate.set(Some(info));
                }
            });
        }
    });

    view! {
        <section class="apply-section">
            <h3 class="form-label">{move || t(i18n.locale.get(), Key::LoginCurrentRole)}</h3>
            <div class="info-grid">
                <div class="info-row">
                    <span class="info-label">{move || t(i18n.locale.get(), Key::LoginCurrentRole)}</span>
                    <span class="info-value">{move || t(i18n.locale.get(), role_label_key(&status.read()))}</span>
                </div>
                <Show when=move || {
                    let status = status.read();
                    status.auth_required
                        && status.role == AuthRole::Admin
                        && status.admin_seconds_remaining.is_some()
                }>
                    <div class="info-row">
                        <span class="info-label">{move || t(i18n.locale.get(), Key::LoginAdminTimeRemaining)}</span>
                        <span class="info-value">{move || {
                            status
                                .read()
                                .admin_seconds_remaining
                                .map(compact_duration)
                                .unwrap_or_default()
                        }}</span>
                    </div>
                </Show>
                <Show when=move || {
                    let status = status.read();
                    status.auth_required
                        && status.role == AuthRole::User
                        && status.session_seconds_remaining.is_some()
                }>
                    <div class="info-row">
                        <span class="info-label">{move || t(i18n.locale.get(), Key::LoginSessionTimeRemaining)}</span>
                        <span class="info-value">{move || {
                            status
                                .read()
                                .session_seconds_remaining
                                .map(compact_duration)
                                .unwrap_or_default()
                        }}</span>
                    </div>
                </Show>
            </div>
            <SessionActions status />
        </section>

        {move || {
            if !auth_active() {
                view! {
                    <section class="apply-section">
                        <h3 class="form-label">{move || t(i18n.locale.get(), Key::LoginStandaloneOpenTitle)}</h3>
                        <p class="form-hint">{move || t(i18n.locale.get(), Key::LoginStandaloneOpenHint)}</p>
                    </section>
                }.into_any()
            } else if status.read().role == AuthRole::Admin {
                view! {
                    <section class="apply-section">
                        <h3 class="form-label">{move || t(i18n.locale.get(), Key::AccessNormalUserTitle)}</h3>
                        <p class="form-hint">{move || t(i18n.locale.get(), Key::AccessNormalUserReplayOs)}</p>
                        <A href="/settings/replayos" attr:class="form-btn form-btn-secondary">
                            {move || t(i18n.locale.get(), Key::AccessManageReplayOs)}
                        </A>
                    </section>
                }.into_any()
            } else {
                view! {
                    <section class="apply-section">
                        <AdminLoginInline status />
                    </section>
                }.into_any()
            }
        }}

        <Show when=move || admin_unlocked()>
            <section class="apply-section">
                <h3 class="form-label">{move || t(i18n.locale.get(), Key::AccessAdminTimeoutTitle)}</h3>
                <p class="form-hint">{move || t(i18n.locale.get(), Key::AccessAdminTimeoutHint)}</p>
                <AdminSessionTimeoutForm
                    initial=initial_admin_timeout
                        .clone()
                        .filter(|value| is_admin_timeout_value(value))
                        .unwrap_or_else(|| "1h".to_string())
                    status
                />
            </section>
        </Show>

        <section class="apply-section">
            <h3 class="form-label">{move || t(i18n.locale.get(), Key::AccessDevicePasswordTitle)}</h3>
            <p class="form-hint">{move || t(i18n.locale.get(), Key::AccessDevicePasswordHint)}</p>
            {move || {
                if !on_device {
                    view! { <DeviceOnlyNotice /> }.into_any()
                } else if admin_unlocked() {
                    view! { <PasswordForm /> }.into_any()
                } else {
                    view! { <p class="form-hint">{move || t(i18n.locale.get(), Key::SettingsAdminOnlyDisabled)}</p> }.into_any()
                }
            }}
        </section>

        <section class="apply-section">
            <h3 class="form-label">{move || t(i18n.locale.get(), Key::AccessHttpsTitle)}</h3>
            <p class="form-hint">{move || t(i18n.locale.get(), Key::AccessCertificateTrustHint)}</p>
            <CertificateSection certificate can_regenerate=Signal::derive(admin_unlocked) />
        </section>
    }
}

#[component]
fn AdminSessionTimeoutForm(initial: String, status: RwSignal<AuthStatus>) -> impl IntoView {
    let i18n = use_i18n();
    let selected = RwSignal::new(if is_admin_timeout_value(&initial) {
        initial
    } else {
        "1h".to_string()
    });
    let saving = RwSignal::new(false);
    let error = RwSignal::new(Option::<String>::None);

    let on_change = move |ev: leptos::ev::Event| {
        if saving.get_untracked() {
            return;
        }
        let value = leptos::prelude::event_target_value(&ev);
        if !is_admin_timeout_value(&value) {
            return;
        }
        let previous = selected.get_untracked();
        selected.set(value.clone());
        saving.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            match server_fns::set_admin_session_timeout(value).await {
                Ok(updated) => status.set(updated),
                Err(err) => {
                    selected.set(previous);
                    error.set(Some(server_fns::format_error(err)));
                }
            }
            saving.set(false);
        });
    };

    view! {
        <select class="form-input" on:change=on_change disabled=move || saving.get()>
            <option value="1h" selected=move || selected.get() == "1h">
                {move || t(i18n.locale.get(), Key::AccessAdminTimeoutOneHour)}
            </option>
            <option value="3h" selected=move || selected.get() == "3h">
                {move || t(i18n.locale.get(), Key::AccessAdminTimeoutThreeHours)}
            </option>
            <option value="12h" selected=move || selected.get() == "12h">
                {move || t(i18n.locale.get(), Key::AccessAdminTimeoutTwelveHours)}
            </option>
        </select>
        {move || error.get().map(|message| view! {
            <div class="status-msg status-err">{message}</div>
        })}
    }
}

fn is_admin_timeout_value(value: &str) -> bool {
    matches!(value, "1h" | "3h" | "12h")
}

fn role_label_key(status: &AuthStatus) -> Key {
    if !status.auth_required {
        return Key::AuthRoleOpen;
    }
    match status.role {
        AuthRole::Anonymous => Key::AuthRoleAnonymous,
        AuthRole::User => Key::AuthRoleUser,
        AuthRole::Admin => Key::AuthRoleAdmin,
    }
}

fn admin_settings_unlocked(status: &AuthStatus) -> bool {
    !status.auth_required || status.role == AuthRole::Admin
}

fn compact_duration(seconds: u64) -> String {
    if seconds < 60 {
        "<1m".to_string()
    } else if seconds < 60 * 60 {
        format!("{}m", seconds.div_ceil(60))
    } else if seconds < 24 * 60 * 60 {
        format!("{}h", seconds.div_ceil(60 * 60))
    } else {
        format!("{}d", seconds.div_ceil(24 * 60 * 60))
    }
}

#[component]
fn SessionActions(status: RwSignal<AuthStatus>) -> impl IntoView {
    let i18n = use_i18n();
    let locale = i18n.locale;
    let saving = RwSignal::new(false);
    let error = RwSignal::new(Option::<String>::None);
    #[cfg(feature = "hydrate")]
    let navigate = StoredValue::new(use_navigate());

    let on_downgrade = move |_| {
        saving.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            match server_fns::downgrade_admin_to_user().await {
                Ok(updated) => {
                    status.set(updated);
                    saving.set(false);
                }
                Err(err) => {
                    error.set(Some(server_fns::format_error(err)));
                    saving.set(false);
                }
            }
        });
    };
    let on_logout = move |_| {
        saving.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            match server_fns::logout().await {
                Ok(updated) => {
                    status.set(updated);
                    saving.set(false);
                    #[cfg(feature = "hydrate")]
                    navigate.get_value()(
                        "/login",
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
    let confirm_dialog = use_confirm_dialog();
    let logout_all = Callback::new(move |()| {
        saving.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            match server_fns::logout_all_browsers().await {
                Ok(updated) => {
                    status.set(updated);
                    saving.set(false);
                    #[cfg(feature = "hydrate")]
                    navigate.get_value()(
                        "/login",
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
    });
    let on_logout_all = move |_| {
        let current_locale = locale.get_untracked();
        confirm_dialog.confirm(
            t(current_locale, Key::LoginLogoutAll),
            t(current_locale, Key::LoginLogoutAllConfirm),
            t(current_locale, Key::LoginLogoutAll),
            true,
            logout_all,
        );
    };

    view! {
        <div class="access-action-stack">
            {move || error.get().map(|message| view! {
                <div class="status-msg status-err">{message}</div>
            })}
            <Show when=move || status.read().auth_required && status.read().role == AuthRole::Admin>
                <button
                    class="form-btn form-btn-secondary"
                    on:click=on_downgrade
                    disabled=move || saving.get() || !status.read().can_downgrade
                >
                    {move || {
                        let locale = locale.get();
                        if saving.get() {
                            t(locale, Key::SettingsSaving)
                        } else {
                            t(locale, Key::LoginDowngrade)
                        }
                    }}
                </button>
                <Show when=move || !status.read().can_downgrade>
                    <p class="form-hint">
                        {move || t(locale.get(), Key::AccessDowngradeUnavailableHint)}
                    </p>
                </Show>
            </Show>
            <Show when=move || status.read().auth_required && status.read().role != AuthRole::Anonymous>
                <button
                    class="form-btn form-btn-secondary"
                    on:click=on_logout
                    disabled=move || saving.get()
                >
                    {move || {
                        let locale = locale.get();
                        if saving.get() {
                            t(locale, Key::SettingsSaving)
                        } else {
                            t(locale, Key::LoginLogout)
                        }
                    }}
                </button>
            </Show>
            <Show when=move || status.read().auth_required && status.read().role == AuthRole::Admin>
                <button
                    class="form-btn form-btn-danger"
                    on:click=on_logout_all
                    disabled=move || saving.get()
                >
                    {move || {
                        let locale = locale.get();
                        if saving.get() {
                            t(locale, Key::SettingsSaving)
                        } else {
                            t(locale, Key::LoginLogoutAll)
                        }
                    }}
                </button>
            </Show>
        </div>
    }
}

#[component]
fn AdminLoginInline(status: RwSignal<AuthStatus>) -> impl IntoView {
    let i18n = use_i18n();
    let locale = i18n.locale;
    let admin_password = RwSignal::new(String::new());
    let saving = RwSignal::new(false);
    let error = RwSignal::new(Option::<String>::None);
    #[cfg(feature = "hydrate")]
    let next_after_unlock = {
        let query = use_query_map();
        StoredValue::new({
            let next = sanitize_next_path(query.get_untracked().get("next"));
            (next != "/").then_some(next)
        })
    };
    #[cfg(feature = "hydrate")]
    let navigate = StoredValue::new(use_navigate());

    let login_admin = move || {
        if saving.get_untracked() {
            return;
        }
        let password = admin_password.get();
        saving.set(true);
        error.set(None);
        leptos::task::spawn_local(async move {
            match server_fns::login_admin(password).await {
                Ok(updated) => {
                    status.set(updated);
                    admin_password.set(String::new());
                    #[cfg(feature = "hydrate")]
                    if let Some(target) = next_after_unlock.get_value() {
                        navigate.get_value()(
                            &target,
                            NavigateOptions {
                                replace: true,
                                ..Default::default()
                            },
                        );
                    }
                }
                Err(err) => error.set(Some(server_fns::format_error(err))),
            }
            saving.set(false);
        });
    };
    let on_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        login_admin();
    };

    view! {
        <form class="access-inline-admin" on:submit=on_submit>
            <h3 class="form-label">{move || t(i18n.locale.get(), Key::LoginAdminTitle)}</h3>
            <p class="form-hint">{move || t(i18n.locale.get(), Key::LoginAdminHint)}</p>
            <div class="form-field">
                <label class="form-label" for="access-admin-password">
                    {move || t(i18n.locale.get(), Key::LoginAdminPasswordLabel)}
                </label>
                <input
                    id="access-admin-password"
                    class="form-input"
                    type="password"
                    autocomplete="current-password"
                    enterkeyhint="go"
                    bind:value=admin_password
                />
            </div>
            {move || error.get().map(|message| view! {
                <div class="status-msg status-err">{message}</div>
            })}
            <button class="form-btn" type="submit" disabled=move || saving.get()>
                {move || {
                    let locale = locale.get();
                    if saving.get() {
                        t(locale, Key::SettingsSaving)
                    } else {
                        t(locale, Key::LoginAdminSubmit)
                    }
                }}
            </button>
        </form>
    }
}

#[component]
fn CertificateSection(
    certificate: RwSignal<Option<TlsCertificateInfo>>,
    #[prop(into)] can_regenerate: Signal<bool>,
) -> impl IntoView {
    let i18n = use_i18n();
    let locale = i18n.locale;
    let confirm_dialog = use_confirm_dialog();
    let saving = RwSignal::new(false);
    let status = RwSignal::new(Option::<(bool, String)>::None);

    let regenerate = Callback::new(move |()| {
        saving.set(true);
        status.set(None);
        leptos::task::spawn_local(async move {
            match server_fns::regenerate_tls_certificate_info().await {
                Ok(updated) => {
                    certificate.set(Some(updated));
                    status.set(Some((
                        true,
                        t(locale.get_untracked(), Key::AccessCertificateRegenerated).to_string(),
                    )));
                    // The service restarts with a new-fingerprint cert; reload so
                    // the browser re-prompts to accept it instead of failing every
                    // later fetch. Delay covers the restart-to-ready time (~3.3s on
                    // the Pi); reloading sooner would hit a not-yet-listening server.
                    crate::util::reload_after_ms(4000);
                }
                Err(error) => {
                    status.set(Some((false, server_fns::format_error(error))));
                }
            }
            saving.set(false);
        });
    });
    let on_regenerate = move |_| {
        let current_locale = locale.get_untracked();
        confirm_dialog.confirm(
            t(current_locale, Key::AccessCertificateTitle),
            t(current_locale, Key::AccessCertificateRegenerateConfirm),
            t(current_locale, Key::AccessCertificateRegenerate),
            false,
            regenerate,
        );
    };

    view! {
        // Certificate details are admin-only — render them only when we have the
        // info (a non-admin fetch returns nothing), never leaking paths/SANs.
        {move || certificate.get().map(|info| view! { <CertificateDetails info /> })}

        <StatusMessage status=status />

        // Always visible; disabled (with a hint) for non-admins.
        <button
            class="form-btn form-btn-secondary"
            on:click=on_regenerate
            disabled=move || saving.get() || !can_regenerate.get()
        >
            {move || {
                let locale = i18n.locale.get();
                if saving.get() {
                    t(locale, Key::SettingsSaving)
                } else {
                    t(locale, Key::AccessCertificateRegenerate)
                }
            }}
        </button>
        <Show when=move || !can_regenerate.get()>
            <p class="form-hint">{move || t(i18n.locale.get(), Key::SettingsAdminOnlyDisabled)}</p>
        </Show>
    }
}

#[component]
fn CertificateDetails(info: TlsCertificateInfo) -> impl IntoView {
    let i18n = use_i18n();
    let info = RwSignal::new(info);
    let missing_coverage = Signal::derive(move || {
        info.with(|info| {
            if info.has_missing_coverage() {
                join_values(
                    info.missing_dns_names
                        .iter()
                        .chain(info.missing_ip_addresses.iter()),
                )
            } else {
                t(i18n.locale.get(), Key::AccessCertificateCovered).to_string()
            }
        })
    });

    view! {
        <div class="info-grid">
            <AccessValueRow label_key=Key::AccessCertificateMode value=Signal::derive(move || t(i18n.locale.get(), Key::AccessCertificateLocal).to_string()) />
            <AccessValueRow label_key=Key::AccessCertificateGenerated value=Signal::derive(move || info.with(|i| i.generated_at.clone().unwrap_or_else(|| "-".to_string()))) />
            <AccessValueRow label_key=Key::AccessCertificateExpires value=Signal::derive(move || info.with(|i| i.expires_at.clone().unwrap_or_else(|| "-".to_string()))) />
            <AccessValueRow label_key=Key::AccessCertificateFingerprint value=Signal::derive(move || info.with(|i| i.fingerprint_sha256.clone().unwrap_or_else(|| "-".to_string()))) />
            <AccessValueRow label_key=Key::AccessCertificateCoveredNames value=Signal::derive(move || info.with(|i| joined_names(&i.covered_dns_names, &i.covered_ip_addresses))) />
            <AccessValueRow label_key=Key::AccessCertificateCurrentNames value=Signal::derive(move || info.with(|i| joined_names(&i.current_dns_names, &i.current_ip_addresses))) />
            <AccessValueRow label_key=Key::AccessCertificateMissingCoverage value=missing_coverage />
        </div>
    }
}

#[component]
fn AccessValueRow(label_key: Key, #[prop(into)] value: Signal<String>) -> impl IntoView {
    let i18n = use_i18n();
    view! {
        <div class="info-row">
            <span class="info-label">{move || t(i18n.locale.get(), label_key)}</span>
            <span class="info-value">{move || value.get()}</span>
        </div>
    }
}

fn joined_names(dns_names: &[String], ip_addresses: &[String]) -> String {
    let joined = join_values(dns_names.iter().chain(ip_addresses.iter()));
    if joined.is_empty() {
        "-".to_string()
    } else {
        joined
    }
}

fn join_values<'a>(values: impl Iterator<Item = &'a String>) -> String {
    values.cloned().collect::<Vec<_>>().join(", ")
}
