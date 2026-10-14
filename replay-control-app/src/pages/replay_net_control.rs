//! RePlayOS settings page (`/settings/replayos`).
//!
//! Onboarding + status surface for the RePlayOS API integration. Two setup
//! paths (see the integration plan): assisted (one button; the action copy
//! itself warns that RePlayOS restarts) and manual (enable Net Control on the
//! TV, type the code it shows). The status card is driven live by the
//! SSE-fed `RwSignal<ReplayApiStatus>` context.

use crate::hooks::use_replay_capabilities;
use leptos::prelude::*;
use leptos_router::components::A;
use replay_control_core::replay_api::ReplayApiStatus;
use replay_control_core::replay_api::ReplayFeature;
use server_fn::ServerFnError;

use crate::components::client_only::ClientOnly;
use crate::components::confirm_dialog::use_confirm_dialog;
use crate::components::device_only_notice::DeviceOnlyNotice;
use crate::components::status_message::StatusMessage;
use crate::i18n::{Key, t, tf, use_i18n};
use crate::server_fns::{self, ReplayOsSettings};
use crate::util::numeric_code;

#[component]
pub fn ReplayNetControlPage() -> impl IntoView {
    let i18n = use_i18n();
    let status = expect_context::<RwSignal<ReplayApiStatus>>();
    let host_kind = Resource::new_blocking(|| (), |_| server_fns::get_host_kind());
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
                    if !host_kind.await.map(|host| host.is_replayos()).unwrap_or(false) {
                        return Ok::<_, ServerFnError>(view! { <DeviceOnlyNotice /> }.into_any());
                    }
                    let initial = initial_status.await?;
                    let settings = initial_settings.await?;
                    Ok::<_, ServerFnError>(
                        view! { <NetControlContent initial settings status /> }.into_any(),
                    )
                })}
            </Suspense>
        </div>
    }
}

#[component]
fn NetControlContent(
    initial: ReplayApiStatus,
    settings: ReplayOsSettings,
    status: RwSignal<ReplayApiStatus>,
) -> impl IntoView {
    let i18n = use_i18n();
    let confirm_dialog = use_confirm_dialog();

    // Live status: the SSE-driven app context, seeded with the SSR-fetched
    // value so the card is correct before the SSE init event lands.
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
    let message_duration = RwSignal::new(settings.message_duration_secs.to_string());
    let saved_message_duration = RwSignal::new(settings.message_duration_secs.to_string());
    let duration_saving = RwSignal::new(false);
    let duration_save_failed = RwSignal::new(false);
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
    let on_code_input = move |ev| {
        code.set(numeric_code(&event_target_value(&ev), 6));
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
    let on_message_duration_change = move |ev| {
        if duration_saving.get_untracked() {
            return;
        }
        let selected = event_target_value(&ev);
        let Ok(duration) = selected.parse::<u8>() else {
            return;
        };
        if selected == saved_message_duration.get_untracked() {
            return;
        }
        let previous = saved_message_duration.get_untracked();
        message_duration.set(selected.clone());
        duration_saving.set(true);
        duration_save_failed.set(false);
        leptos::task::spawn_local(async move {
            match server_fns::save_replayos_message_duration(duration).await {
                Ok(()) => saved_message_duration.set(selected),
                Err(_) => {
                    message_duration.set(previous);
                    duration_save_failed.set(true);
                }
            }
            duration_saving.set(false);
        });
    };
    let on_clear_message = move |_| {
        message_text.set(String::new());
        message_result.set(None);
    };
    let restart_game = Callback::new(move |()| {
        run_string_action(
            action_busy,
            restart_result,
            server_fns::restart_replayos_game(),
            || (),
        );
    });
    let on_restart_game = move |_| {
        let locale = i18n.locale.get_untracked();
        confirm_dialog.confirm(
            t(locale, Key::PlayerControlReset),
            t(locale, Key::ReplayOsRestartGameConfirm),
            t(locale, Key::PlayerControlReset),
            false,
            restart_game,
        );
    };
    let power_off = Callback::new(move |()| {
        run_string_action(
            action_busy,
            device_result,
            server_fns::power_off_replayos_device(),
            || (),
        );
    });
    let on_power_off = move |_| {
        let locale = i18n.locale.get_untracked();
        confirm_dialog.confirm(
            t(locale, Key::ReplayOsPowerOff),
            t(locale, Key::ReplayOsPowerOffConfirm),
            t(locale, Key::ReplayOsPowerOff),
            true,
            power_off,
        );
    };
    let reboot = Callback::new(move |()| {
        run_string_action(
            action_busy,
            device_result,
            server_fns::reboot_system(),
            || (),
        );
    });
    let on_reboot = move |_| {
        let locale = i18n.locale.get_untracked();
        confirm_dialog.confirm(
            t(locale, Key::SettingsReboot),
            t(locale, Key::ReplayOsRebootConfirm),
            t(locale, Key::SettingsReboot),
            true,
            reboot,
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
                            pattern="[0-9]*"
                            autocomplete="off"
                            placeholder="123456"
                            prop:value=move || code.get()
                            on:input=on_code_input
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
                        <select
                            class="form-input replayos-duration-select"
                            disabled=move || duration_saving.get()
                            on:change=on_message_duration_change
                        >
                            <option value="1" selected=move || message_duration.get() == "1">"1s"</option>
                            <option value="3" selected=move || message_duration.get() == "3">"3s"</option>
                            <option value="5" selected=move || message_duration.get() == "5">"5s"</option>
                            <option value="10" selected=move || message_duration.get() == "10">"10s"</option>
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
                    <Show when=move || duration_save_failed.get() fallback=|| ()>
                        <p class="form-hint form-error">{move || t(i18n.locale.get(), Key::ReplayOsDurationSaveFailed)}</p>
                    </Show>
                    <StatusMessage status=message_result />
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
                    <StatusMessage status=restart_result />
                </div>
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
                <StatusMessage status=mode_result />
            </section>

            <ClientOnly fallback=move || view! { <p class="form-hint">{move || t(i18n.locale.get(), Key::CommonLoading)}</p> }>
                <LinkPlaySection action_busy api_action_disabled=Signal::derive(move || api_action_disabled.get()) />
                <InsiderSection action_busy api_action_disabled=Signal::derive(move || api_action_disabled.get()) />
            </ClientOnly>

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
                <StatusMessage status=device_result />
            </section>
        </div>
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

#[component]
fn LinkPlaySection(
    action_busy: RwSignal<bool>,
    api_action_disabled: Signal<bool>,
) -> impl IntoView {
    let i18n = use_i18n();
    let capabilities = use_replay_capabilities();
    let restriction =
        move || capabilities.feature_restriction(ReplayFeature::LinkPlay, i18n.locale.get());
    view! {
        <Show when=move || restriction().is_none() fallback=move || view! {
            <section class="apply-section link-play-unavailable">
                <h3 class="form-label">{move || t(i18n.locale.get(), Key::LinkPlayTitle)}</h3>
                <p class="form-hint">{restriction}</p>
            </section>
        }>
            <AvailableLinkPlaySection action_busy api_action_disabled />
        </Show>
    }
}

#[component]
fn AvailableLinkPlaySection(
    action_busy: RwSignal<bool>,
    api_action_disabled: Signal<bool>,
) -> impl IntoView {
    let i18n = use_i18n();
    let refresh = RwSignal::new(0u64);
    // Optional device panels load in the browser, outside the page's streamed
    // SSR resource tree. Their failures never replace pairing/status controls.
    let config = LocalResource::new(move || {
        refresh.get();
        server_fns::get_link_play_config()
    });
    view! {
        <ErrorBoundary fallback=move |_| view! {
            <p class="status-msg status-err">{move || t(i18n.locale.get(),Key::GameSettingsUnavailable)}</p>
            <button type="button" class="compact-control" on:click=move |_| refresh.update(|value| *value += 1)>
                {move || t(i18n.locale.get(),Key::ReplayApiCheckAgain)}
            </button>
        }>
            <Suspense fallback=move || view! { <p class="form-hint">{move || t(i18n.locale.get(),Key::CommonLoading)}</p> }>
                {move || Suspend::new(async move {
                    let link_play = config.await?;
                    Ok::<_,ServerFnError>(view! { <LinkPlayForm link_play action_busy api_action_disabled /> })
                })}
            </Suspense>
        </ErrorBoundary>
    }
}

#[component]
fn LinkPlayForm(
    link_play: server_fns::LinkPlayConfig,
    action_busy: RwSignal<bool>,
    api_action_disabled: Signal<bool>,
) -> impl IntoView {
    let i18n = use_i18n();
    let link_play_result = RwSignal::new(Option::<(bool, String)>::None);
    let link_play_mode = RwSignal::new(link_play.mode);
    let link_play_address = RwSignal::new(link_play.address);
    let link_play_port = RwSignal::new(link_play.port.to_string());
    let link_play_discovery = RwSignal::new(link_play.lan_discovery);
    let on_save_link_play = move |_| {
        let Ok(port) = link_play_port.get_untracked().parse::<u16>() else {
            link_play_result.set(Some((
                false,
                t(i18n.locale.get_untracked(), Key::LinkPlayInvalidPort).to_string(),
            )));
            return;
        };
        let config = server_fns::LinkPlayConfig {
            mode: link_play_mode.get_untracked(),
            address: link_play_address.get_untracked(),
            port,
            lan_discovery: link_play_discovery.get_untracked(),
        };
        run_settings_action(
            action_busy,
            link_play_result,
            server_fns::save_link_play_config(config),
            || (),
        );
    };

    view! {
            <section class="apply-section">
                <h3 class="form-label">{move || t(i18n.locale.get(), Key::LinkPlayTitle)}</h3>
                <p class="form-hint">{move || t(i18n.locale.get(), Key::LinkPlayHint)}</p>
                <p class="form-hint">
                    <a href="https://www.replayos.com/linkplay/#configuration-file" target="_blank" rel="noopener noreferrer">
                        {move || t(i18n.locale.get(), Key::LinkPlayConfigGuide)}
                    </a>
                </p>
                <div class="form-field">
                    <label class="form-label" for="link-play-mode">
                        {move || t(i18n.locale.get(), Key::LinkPlayMode)}
                    </label>
                    <select id="link-play-mode" class="form-input" bind:value=link_play_mode>
                        <option value="disabled">{move || t(i18n.locale.get(), Key::LinkPlayDisabled)}</option>
                        <option value="host">{move || t(i18n.locale.get(), Key::LinkPlayHost)}</option>
                        <option value="client">{move || t(i18n.locale.get(), Key::LinkPlayClient)}</option>
                    </select>
                </div>
                <Show when=move || link_play_mode.read().as_str() == "client" fallback=|| ()>
                    <div class="form-field">
                        <label class="form-label" for="link-play-address">
                            {move || t(i18n.locale.get(), Key::LinkPlayAddress)}
                        </label>
                        <input
                            id="link-play-address"
                            type="text"
                            class="form-input"
                            maxlength="255"
                            bind:value=link_play_address
                            autocomplete="off"
                        />
                        <p class="form-hint">
                            {move || t(i18n.locale.get(), Key::LinkPlayAddressHint)}
                        </p>
                    </div>
                </Show>
                <div class="form-field">
                    <label class="form-label" for="link-play-port">
                        {move || t(i18n.locale.get(), Key::LinkPlayPort)}
                    </label>
                    <input
                        id="link-play-port"
                        type="number"
                        class="form-input"
                        min="1"
                        max="65535"
                        bind:value=link_play_port
                    />
                </div>
                <div class="form-field form-field-check">
                    <label class="form-label" for="link-play-discovery">
                        {move || t(i18n.locale.get(), Key::LinkPlayDiscovery)}
                    </label>
                    <input
                        id="link-play-discovery"
                        type="checkbox"
                        class="form-checkbox"
                        bind:checked=link_play_discovery
                    />
                </div>
                <p class="form-hint">
                    {move || t(i18n.locale.get(), Key::LinkPlayDiscoveryHint)}
                </p>
                <button
                    type="button"
                    class="form-btn"
                    disabled=move || api_action_disabled.get()
                    on:click=on_save_link_play
                >
                    {move || t(i18n.locale.get(), Key::LinkPlaySave)}
                </button>
                <StatusMessage status=link_play_result />
            </section>


    }
}

#[component]
fn InsiderSection(action_busy: RwSignal<bool>, api_action_disabled: Signal<bool>) -> impl IntoView {
    let i18n = use_i18n();
    let refresh = RwSignal::new(0u64);
    let config = LocalResource::new(move || {
        refresh.get();
        server_fns::get_replay_insider_config()
    });
    view! {
        <ErrorBoundary fallback=move |_| view! {
            <p class="status-msg status-err">{move || t(i18n.locale.get(),Key::GameSettingsUnavailable)}</p>
            <button type="button" class="compact-control" on:click=move |_| refresh.update(|value| *value += 1)>
                {move || t(i18n.locale.get(),Key::ReplayApiCheckAgain)}
            </button>
        }>
            <Suspense fallback=move || view! { <p class="form-hint">{move || t(i18n.locale.get(),Key::CommonLoading)}</p> }>
                {move || Suspend::new(async move {
                    let insider = config.await?;
                    Ok::<_,ServerFnError>(view! { <InsiderForm insider action_busy api_action_disabled /> })
                })}
            </Suspense>
        </ErrorBoundary>
    }
}

#[component]
fn InsiderForm(
    insider: server_fns::ReplayInsiderConfig,
    action_busy: RwSignal<bool>,
    api_action_disabled: Signal<bool>,
) -> impl IntoView {
    let i18n = use_i18n();
    let insider_result = RwSignal::new(Option::<(bool, String)>::None);
    let insider_token = RwSignal::new(String::new());
    let insider_token_configured = RwSignal::new(insider.token_configured);
    let insider_enabled = RwSignal::new(insider.enabled);
    let activation_label = move || {
        t(
            i18n.locale.get(),
            match insider_enabled.get() {
                Some(true) => Key::ReplayInsiderActive,
                Some(false) => Key::ReplayInsiderInactive,
                None => Key::ReplayInsiderUnknown,
            },
        )
    };
    let show_insider_token = RwSignal::new(false);
    let save_insider_token = move |clear: bool| {
        let token = if clear {
            String::new()
        } else {
            insider_token.get_untracked()
        };
        run_settings_action(
            action_busy,
            insider_result,
            server_fns::save_replay_insider_token_and_restart(token.clone()),
            move || {
                insider_token.set(String::new());
                insider_token_configured.set(!token.is_empty());
                insider_enabled.set(None);
            },
        );
    };
    let on_save_insider = move |_| save_insider_token(false);
    let on_clear_insider = move |_| save_insider_token(true);

    view! {
            <section class="apply-section">
                <h3 class="form-label">{move || t(i18n.locale.get(), Key::ReplayInsiderTitle)}</h3>
                <p class="form-hint">{move || t(i18n.locale.get(), Key::ReplayInsiderHint)}</p>
                <p class="form-hint insider-activation-status">{activation_label}</p>
                <div class="form-field">
                    <label class="form-label" for="replayos-insider-token">
                        {move || t(i18n.locale.get(), Key::ReplayInsiderToken)}
                    </label>
                    <div class="input-with-toggle">
                        <input
                            id="replayos-insider-token"
                            type=move || if show_insider_token.get() { "text" } else { "password" }
                            class="form-input"
                            bind:value=insider_token
                            autocomplete="off"
                            maxlength="512"
                            placeholder=move || t(i18n.locale.get(), Key::SettingsPasswordEnter)
                        />
                        <button
                            type="button"
                            class="toggle-password"
                            on:click=move |_| show_insider_token.update(|shown| *shown = !*shown)
                        >
                            {move || if show_insider_token.get() { "\u{1F648}" } else { "\u{1F441}" }}
                        </button>
                    </div>
                    <p class="form-hint">
                        {move || {
                            let key = if insider_token_configured.get() {
                                Key::ReplayInsiderTokenSaved
                            } else {
                                Key::ReplayInsiderTokenMissing
                            };
                            t(i18n.locale.get(), key)
                        }}
                    </p>
                </div>
                <p class="form-hint">
                    {move || t(i18n.locale.get(), Key::SettingsReplayRestartWarning)}
                </p>
                <StatusMessage status=insider_result />
                <div class="replayos-button-stack">
                    <button
                        type="button"
                        class="form-btn"
                        disabled=move || api_action_disabled.get() || insider_token.read().trim().is_empty()
                        on:click=on_save_insider
                    >
                        {move || t(i18n.locale.get(), Key::ReplayInsiderSaveRestart)}
                    </button>
                    <button
                        type="button"
                        class="form-btn form-btn-secondary"
                        disabled=move || api_action_disabled.get() || !insider_token_configured.get()
                        on:click=on_clear_insider
                    >
                        {move || t(i18n.locale.get(), Key::ReplayInsiderClearRestart)}
                    </button>
                </div>
            </section>
    }
}

fn run_settings_action<Fut>(
    busy: RwSignal<bool>,
    result: RwSignal<Option<(bool, String)>>,
    fut: Fut,
    on_success: impl FnOnce() + 'static,
) where
    Fut: std::future::Future<Output = Result<server_fns::SettingsOutcome, ServerFnError>> + 'static,
{
    use server_fns::SettingsOutcome;
    let i18n = use_i18n();
    if busy.get_untracked() {
        return;
    }
    busy.set(true);
    result.set(None);
    leptos::task::spawn_local(async move {
        let outcome = fut.await;
        let (ok, key) = match outcome {
            Ok(SettingsOutcome::Applied) => (true, Key::LinkPlayApplied),
            Ok(SettingsOutcome::Restarting) => (true, Key::ReplaySettingsRestarting),
            Ok(SettingsOutcome::InvalidMode) => (false, Key::LinkPlayInvalidMode),
            Ok(SettingsOutcome::InvalidAddress) => (false, Key::LinkPlayInvalidAddress),
            Ok(SettingsOutcome::InvalidPort) => (false, Key::LinkPlayInvalidPort),
            Ok(SettingsOutcome::InvalidToken) => (false, Key::ReplayInsiderInvalid),
            Err(_) => (false, Key::ReplaySettingsFailed),
        };
        if ok {
            on_success();
        }
        result.set(Some((ok, t(i18n.locale.get_untracked(), key).to_string())));
        busy.set(false);
    });
}
