//! RePlayOS settings page (`/settings/replayos`).
//!
//! Onboarding + status surface for the RePlayOS API integration. Two setup
//! paths (see the integration plan): assisted (one button; the action copy
//! itself warns that RePlayOS restarts) and manual (enable Net Control on the
//! TV, type the code it shows). The status card is driven live by the
//! SSE-fed `RwSignal<ReplayApiStatus>` context.

use leptos::prelude::*;
use leptos_router::components::A;
use replay_control_core::replay_api::ReplayApiStatus;
use server_fn::ServerFnError;

use crate::components::device_only_notice::DeviceOnlyNotice;
use crate::i18n::{Key, t, tf, use_i18n};
use crate::server_fns::{self, ReplayOsSettings};
use crate::util::confirm_action;

#[component]
pub fn ReplayNetControlPage() -> impl IntoView {
    let i18n = use_i18n();
    let mode = Resource::new_blocking(|| (), |_| server_fns::get_mode());
    let initial_status = Resource::new_blocking(|| (), |_| server_fns::get_replay_api_status());
    let initial_settings = Resource::new_blocking(|| (), |_| server_fns::get_replayos_settings());

    view! {
        <div class="page settings-page">
            <div class="rom-header">
                <A href="/settings" attr:class="back-btn">
                    {move || t(i18n.locale.get(), Key::GamesBack)}
                </A>
                <h2 class="page-title">{move || t(i18n.locale.get(), Key::ReplayOsSettingsTitle)}</h2>
            </div>

            <Suspense fallback=move || {
                view! { <div class="loading">{move || t(i18n.locale.get(), Key::CommonLoading)}</div> }
            }>
                {move || Suspend::new(async move {
                    if !mode.await.map(|p| p.is_device()).unwrap_or(false) {
                        return Ok::<_, ServerFnError>(view! { <DeviceOnlyNotice /> }.into_any());
                    }
                    let initial = initial_status.await?;
                    let settings = initial_settings.await?;
                    Ok::<_, ServerFnError>(view! { <NetControlContent initial settings /> }.into_any())
                })}
            </Suspense>
        </div>
    }
}

#[component]
fn NetControlContent(initial: ReplayApiStatus, settings: ReplayOsSettings) -> impl IntoView {
    let i18n = use_i18n();

    // Live status: the SSE-driven app context, seeded with the SSR-fetched
    // value so the card is correct before the SSE init event lands.
    let status = expect_context::<RwSignal<ReplayApiStatus>>();
    if status.get_untracked() == ReplayApiStatus::default() && initial != ReplayApiStatus::default()
    {
        status.set(initial);
    }

    let busy = RwSignal::new(false);
    let error = RwSignal::new(Option::<String>::None);
    let code = RwSignal::new(String::new());
    let action_busy = RwSignal::new(false);
    let message_result = RwSignal::new(Option::<(bool, String)>::None);
    let restart_result = RwSignal::new(Option::<(bool, String)>::None);
    let device_result = RwSignal::new(Option::<(bool, String)>::None);
    let mode_result = RwSignal::new(Option::<(bool, String)>::None);
    let message_text = RwSignal::new(String::new());
    let message_duration = RwSignal::new("3".to_string());
    let kiosk_mode = RwSignal::new(settings.kiosk_mode);
    // "Re-enter code" affordance: re-open the setup sections while Active
    // (covers a TV-side code reset without waiting for a 401).
    let reenter = RwSignal::new(false);

    let is_active = Memo::new(move |_| status.read().is_active());
    let show_setup = Memo::new(move |_| !is_active.get() || reenter.get());
    let unsupported =
        Memo::new(move |_| matches!(*status.read(), ReplayApiStatus::Unsupported { .. }));
    let api_action_disabled = Memo::new(move |_| action_busy.get() || !is_active.get());

    let on_auto = move |_| {
        run_status_action(
            busy,
            error,
            status,
            server_fns::enable_replay_api_assisted(),
            || (),
        )
    };
    let on_connect = move |_| {
        let entered = code.get_untracked();
        run_status_action(
            busy,
            error,
            status,
            server_fns::verify_replay_api_token(entered),
            move || {
                code.set(String::new());
                reenter.set(false);
            },
        );
    };
    let on_reprobe =
        move |_| run_status_action(busy, error, status, server_fns::reprobe_replay_api(), || ());
    let on_send_message = move |_| {
        let text = message_text.get_untracked();
        let duration = message_duration.get_untracked().parse::<u8>().unwrap_or(3);
        run_string_action(
            action_busy,
            message_result,
            server_fns::send_replayos_message(text, duration),
            || (),
        );
    };
    let on_clear_message = move |_| {
        message_text.set(String::new());
        message_result.set(None);
    };
    let on_restart_game = move |_| {
        if !confirm_action(t(
            i18n.locale.get_untracked(),
            Key::ReplayOsRestartGameConfirm,
        )) {
            return;
        }
        run_string_action(
            action_busy,
            restart_result,
            server_fns::restart_replayos_game(),
            || (),
        );
    };
    let on_power_off = move |_| {
        if !confirm_action(t(i18n.locale.get_untracked(), Key::ReplayOsPowerOffConfirm)) {
            return;
        }
        run_string_action(
            action_busy,
            device_result,
            server_fns::power_off_replayos_device(),
            || (),
        );
    };
    let on_reboot = move |_| {
        run_string_action(
            action_busy,
            device_result,
            server_fns::reboot_system(),
            || (),
        );
    };
    let on_save_kiosk = move |_| {
        let enabled = kiosk_mode.get_untracked();
        run_string_action(
            action_busy,
            mode_result,
            server_fns::save_replayos_kiosk_mode(enabled),
            || (),
        );
    };

    // One label rule for both action buttons: the idle key, or "Connecting…"
    // while any action is in flight.
    let action_label = move |idle: Key| {
        let locale = i18n.locale.get();
        if busy.get() {
            t(locale, Key::ReplayApiConnecting)
        } else {
            t(locale, idle)
        }
    };

    view! {
        <div class="settings-form">
            <section class="replayos-settings-section">
                <h3 class="form-label">{move || t(i18n.locale.get(), Key::ReplayOsConnectionTitle)}</h3>
                <NetControlStatusCard status busy on_reprobe />
            </section>

            <Show when=move || error.read().is_some() fallback=|| ()>
                <p class="form-hint form-error">{move || error.get().unwrap_or_default()}</p>
            </Show>

            <Show
                when=move || show_setup.get()
                fallback=move || {
                    view! {
                        <button class="form-btn form-btn-secondary" on:click=move |_| reenter.set(true)>
                            {move || t(i18n.locale.get(), Key::ReplayApiReenterCode)}
                        </button>
                    }
                }
            >
                <section class="apply-section">
                    <h3 class="form-label">{move || t(i18n.locale.get(), Key::ReplayApiAutoTitle)}</h3>
                    <button
                        class="form-btn"
                        disabled=move || busy.get() || unsupported.get()
                        on:click=on_auto
                    >
                        {move || action_label(Key::ReplayApiAutoButton)}
                    </button>
                    <p class="form-hint">{move || t(i18n.locale.get(), Key::ReplayApiAutoHint)}</p>
                </section>

                <section class="apply-section">
                    <h3 class="form-label">{move || t(i18n.locale.get(), Key::ReplayApiManualTitle)}</h3>
                    <ol class="form-hint net-control-steps">
                        <li>{move || t(i18n.locale.get(), Key::ReplayApiManualStep1)}</li>
                        <li>{move || t(i18n.locale.get(), Key::ReplayApiManualStep2)}</li>
                        <li>{move || t(i18n.locale.get(), Key::ReplayApiManualStep3)}</li>
                    </ol>
                    <div class="form-field net-control-code-row">
                        <input
                            class="form-input net-control-code-input"
                            type="text"
                            inputmode="numeric"
                            autocomplete="off"
                            maxlength="6"
                            placeholder="123456"
                            bind:value=code
                        />
                        <button
                            class="form-btn"
                            disabled=move || busy.get() || code.read().trim().is_empty()
                            on:click=on_connect
                        >
                            {move || action_label(Key::ReplayApiConnect)}
                        </button>
                    </div>
                </section>
            </Show>

            <section class="apply-section">
                <h3 class="form-label">{move || t(i18n.locale.get(), Key::ReplayOsActionsTitle)}</h3>
                <p class="form-hint">{move || t(i18n.locale.get(), Key::ReplayOsConnectedHint)}</p>

                <div class="form-field">
                    <label class="form-label" for="replayos-message">
                        {move || t(i18n.locale.get(), Key::ReplayOsMessageTitle)}
                    </label>
                    <textarea
                        id="replayos-message"
                        class="form-input replayos-message-input"
                        maxlength="120"
                        rows="3"
                        placeholder=move || t(i18n.locale.get(), Key::ReplayOsMessagePlaceholder)
                        bind:value=message_text
                    ></textarea>
                    <div class="replayos-inline-controls">
                        <select class="form-input replayos-duration-select" bind:value=message_duration>
                            <option value="1">"1s"</option>
                            <option value="3">"3s"</option>
                            <option value="5">"5s"</option>
                            <option value="10">"10s"</option>
                        </select>
                        <button
                            type="button"
                            class="form-btn"
                            disabled=move || api_action_disabled.get() || message_text.read().trim().is_empty()
                            on:click=on_send_message
                        >
                            {move || t(i18n.locale.get(), Key::ReplayOsMessageSend)}
                        </button>
                        <button
                            type="button"
                            class="form-btn form-btn-secondary"
                            disabled=move || action_busy.get() || message_text.read().is_empty()
                            on:click=on_clear_message
                        >
                            {move || t(i18n.locale.get(), Key::ReplayOsMessageClear)}
                        </button>
                    </div>
                    <ActionResultMessage result=message_result />
                </div>

                <div class="replayos-action-block">
                    <button
                        type="button"
                        class="form-btn form-btn-secondary"
                        disabled=move || api_action_disabled.get()
                        on:click=on_restart_game
                    >
                        {move || t(i18n.locale.get(), Key::ReplayOsRestartGame)}
                    </button>
                    <ActionResultMessage result=restart_result />
                </div>
            </section>

            <section class="apply-section">
                <h3 class="form-label">{move || t(i18n.locale.get(), Key::ReplayOsDeviceTitle)}</h3>
                <p class="form-hint">{move || t(i18n.locale.get(), Key::ReplayOsDeviceHint)}</p>
                <div class="replayos-button-stack">
                    <button
                        type="button"
                        class="form-btn form-btn-secondary"
                        disabled=move || action_busy.get()
                        on:click=on_reboot
                    >
                        {move || t(i18n.locale.get(), Key::SettingsReboot)}
                    </button>
                    <button
                        type="button"
                        class="form-btn form-btn-secondary"
                        disabled=move || api_action_disabled.get()
                        on:click=on_power_off
                    >
                        {move || t(i18n.locale.get(), Key::ReplayOsPowerOff)}
                    </button>
                </div>
                <ActionResultMessage result=device_result />
            </section>

            <section class="apply-section">
                <h3 class="form-label">{move || t(i18n.locale.get(), Key::ReplayOsModeTitle)}</h3>
                <div class="form-field form-field-check">
                    <label class="form-label" for="replayos-kiosk-mode">
                        {move || t(i18n.locale.get(), Key::ReplayOsKioskMode)}
                    </label>
                    <input
                        id="replayos-kiosk-mode"
                        type="checkbox"
                        class="form-checkbox"
                        prop:checked=move || kiosk_mode.get()
                        disabled=move || api_action_disabled.get()
                        on:change=move |ev| kiosk_mode.set(event_target_checked(&ev))
                    />
                </div>
                <p class="form-hint">{move || t(i18n.locale.get(), Key::ReplayOsKioskHint)}</p>
                <button
                    type="button"
                    class="form-btn"
                    disabled=move || api_action_disabled.get()
                    on:click=on_save_kiosk
                >
                    {move || t(i18n.locale.get(), Key::SettingsSave)}
                </button>
                <ActionResultMessage result=mode_result />
            </section>
        </div>
    }
}

#[component]
fn ActionResultMessage(result: RwSignal<Option<(bool, String)>>) -> impl IntoView {
    view! {
        <Show when=move || result.read().is_some() fallback=|| ()>
            {move || result.get().map(|(ok, msg)| {
                let class = if ok {
                    "status-msg status-ok replayos-action-status"
                } else {
                    "status-msg status-err replayos-action-status"
                };
                view! { <div class=class>{msg}</div> }
            })}
        </Show>
    }
}

/// The always-visible status card: one line per `ReplayApiStatus`, with
/// "Check again" / retry on the states a re-probe can move.
#[component]
fn NetControlStatusCard(
    status: RwSignal<ReplayApiStatus>,
    busy: RwSignal<bool>,
    on_reprobe: impl Fn(leptos::ev::MouseEvent) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let i18n = use_i18n();

    let line = Memo::new(move |_| {
        let locale = i18n.locale.get();
        match status.get() {
            ReplayApiStatus::Active { version } => (
                "ok",
                tf(locale, Key::ReplayApiStatusConnected, &[&version]),
                None,
            ),
            ReplayApiStatus::NotConfigured => (
                "idle",
                t(locale, Key::ReplayApiStatusNotConnected).to_string(),
                None,
            ),
            ReplayApiStatus::PendingRestart => (
                "busy",
                t(locale, Key::ReplayApiStatusRestarting).to_string(),
                None,
            ),
            ReplayApiStatus::Unauthorized => (
                "warn",
                t(locale, Key::ReplayApiStatusUnauthorized).to_string(),
                None,
            ),
            ReplayApiStatus::Unsupported { version } => (
                "info",
                t(locale, Key::ReplayApiStatusUnsupported).to_string(),
                version,
            ),
            ReplayApiStatus::Error { reason } => (
                "warn",
                t(locale, Key::ReplayApiStatusError).to_string(),
                Some(reason),
            ),
        }
    });
    // Re-probe makes sense where connectivity might have changed.
    let show_reprobe = Memo::new(move |_| {
        matches!(
            *status.read(),
            ReplayApiStatus::Unsupported { .. }
                | ReplayApiStatus::Error { .. }
                | ReplayApiStatus::NotConfigured
        )
    });

    view! {
        <div class=move || format!("net-control-status net-control-status--{}", line.read().0)>
            <span class="net-control-status-dot"></span>
            <span>{move || line.read().1.clone()}</span>
            <Show when=move || line.read().2.is_some() fallback=|| ()>
                <small class="form-hint">{move || line.read().2.clone().unwrap_or_default()}</small>
            </Show>
            <Show when=move || show_reprobe.get() fallback=|| ()>
                <button
                    class="form-btn form-btn-secondary net-control-reprobe"
                    disabled=move || busy.get()
                    on:click=on_reprobe
                >
                    {move || t(i18n.locale.get(), Key::ReplayApiCheckAgain)}
                </button>
            </Show>
        </div>
    }
}

/// Shared busy/error/status protocol for the page's three actions: guard
/// against double-fire, clear the error, run the server fn, publish the
/// resulting status, run the success hook, release busy.
fn run_status_action<Fut>(
    busy: RwSignal<bool>,
    error: RwSignal<Option<String>>,
    status: RwSignal<ReplayApiStatus>,
    fut: Fut,
    on_success: impl FnOnce() + 'static,
) where
    Fut: std::future::Future<Output = Result<ReplayApiStatus, ServerFnError>> + 'static,
{
    if busy.get_untracked() {
        return;
    }
    busy.set(true);
    error.set(None);
    leptos::task::spawn_local(async move {
        match fut.await {
            Ok(new_status) => {
                status.set(new_status);
                on_success();
            }
            Err(e) => error.set(Some(e.to_string())),
        }
        busy.set(false);
    });
}

fn run_string_action<Fut>(
    busy: RwSignal<bool>,
    result: RwSignal<Option<(bool, String)>>,
    fut: Fut,
    on_success: impl FnOnce() + 'static,
) where
    Fut: std::future::Future<Output = Result<String, ServerFnError>> + 'static,
{
    if busy.get_untracked() {
        return;
    }
    busy.set(true);
    result.set(None);
    leptos::task::spawn_local(async move {
        match fut.await {
            Ok(message) => {
                on_success();
                result.set(Some((true, message)));
            }
            Err(error) => result.set(Some((false, error.to_string()))),
        }
        busy.set(false);
    });
}
