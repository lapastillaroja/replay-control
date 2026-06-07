//! RePlayOS Net Control settings page (`/settings/replay-net-control`).
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
use crate::server_fns;

#[component]
pub fn ReplayNetControlPage() -> impl IntoView {
    let i18n = use_i18n();
    let mode = Resource::new_blocking(|| (), |_| server_fns::get_mode());
    let initial_status = Resource::new_blocking(|| (), |_| server_fns::get_replay_api_status());

    view! {
        <div class="page settings-page">
            <div class="rom-header">
                <A href="/settings" attr:class="back-btn">
                    {move || t(i18n.locale.get(), Key::GamesBack)}
                </A>
                <h2 class="page-title">{move || t(i18n.locale.get(), Key::ReplayNetControlTitle)}</h2>
            </div>

            <Suspense fallback=move || {
                view! { <div class="loading">{move || t(i18n.locale.get(), Key::CommonLoading)}</div> }
            }>
                {move || Suspend::new(async move {
                    if !mode.await.map(|p| p.is_device()).unwrap_or(false) {
                        return Ok::<_, ServerFnError>(view! { <DeviceOnlyNotice /> }.into_any());
                    }
                    let initial = initial_status.await?;
                    Ok::<_, ServerFnError>(view! { <NetControlContent initial /> }.into_any())
                })}
            </Suspense>
        </div>
    }
}

#[component]
fn NetControlContent(initial: ReplayApiStatus) -> impl IntoView {
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
    // "Re-enter code" affordance: re-open the setup sections while Active
    // (covers a TV-side code reset without waiting for a 401).
    let reenter = RwSignal::new(false);

    let is_active = Memo::new(move |_| status.read().is_active());
    let show_setup = Memo::new(move |_| !is_active.get() || reenter.get());
    let unsupported =
        Memo::new(move |_| matches!(*status.read(), ReplayApiStatus::Unsupported { .. }));

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
            <NetControlStatusCard status busy on_reprobe />

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
