use leptos::prelude::*;
#[cfg(feature = "hydrate")]
use leptos_router::NavigateOptions;
#[cfg(feature = "hydrate")]
use leptos_router::hooks::use_navigate;
use leptos_router::hooks::use_query_map;
use replay_control_core::replay_api::ReplayApiStatus;

use crate::i18n::{Key, t, use_i18n};
use crate::server_fns::{self, Activity, RefreshMetadataPhase, SetupStatus, ThumbnailPhase};

/// Dismissible first-run setup checklist shown at the top of the Home page.
///
/// Uses `Resource::new` (non-blocking) so it doesn't delay SSR TTFB.
/// Auto-hides when all available tasks are complete or the user dismisses it.
/// Append `?setup` to the URL to force-show the card (for testing/screenshots).
#[component]
pub fn SetupChecklist() -> impl IntoView {
    let query = use_query_map();
    let force = Memo::new(move |_| query.read().get_str("setup").is_some());
    let status = Resource::new(move || force.get(), server_fns::get_setup_status);

    view! {
        <Suspense fallback=|| ()>
            {move || Suspend::new(async move {
                let status = status.await?;
                let force = force.get_untracked();
                Ok::<_, server_fn::ServerFnError>(if status.show_setup {
                    view! { <SetupCard status force /> }.into_any()
                } else {
                    ().into_any()
                })
            })}
        </Suspense>
    }
}

/// The actual setup card with setup task rows and dismiss controls.
#[component]
fn SetupCard(status: SetupStatus, force: bool) -> impl IntoView {
    let i18n = use_i18n();

    let metadata_done = RwSignal::new(status.has_metadata);
    let thumbnail_done = RwSignal::new(status.has_thumbnail_index);
    let replay_api_done_local = RwSignal::new(status.replay_api_active);
    let password_done = RwSignal::new(!status.default_root_password_active);
    let replay_api_status = use_context::<RwSignal<ReplayApiStatus>>();
    let replay_api_done = Signal::derive(move || {
        replay_api_done_local.get()
            || replay_api_status
                .as_ref()
                .is_some_and(|status| status.read().is_active())
    });
    let has_replayos_step = status.is_device;
    let has_password_step = status.is_device;
    let admin_access = status.admin_access;
    let metadata_error = RwSignal::new(None::<String>);
    let replay_api_error = RwSignal::new(None::<String>);
    let password_error = RwSignal::new(None::<String>);
    // Local "pending" flags give immediate visual feedback on click — the
    // server fn round-trip + SSE delivery takes a beat, and without these
    // the row sits on the Start button for that beat (looks like the click
    // didn't register). Cleared when SSE confirms the activity (Effect below)
    // or when the server fn returns Err (handler).
    let metadata_pending = RwSignal::new(false);
    let replay_api_pending = RwSignal::new(false);
    let dismissed = RwSignal::new(false);

    // "Setup complete!" view is only shown when the user landed here with
    // all available tasks already done (organic visit). Re-runs from `?setup` or
    // tasks completed in-page leave the checklist visible so the user
    // can see the result and re-run if needed.
    let metadata_sources_done = Signal::derive(move || metadata_done.get() && thumbnail_done.get());
    let setup_progress = Signal::derive(move || {
        let total = 1 + usize::from(has_replayos_step) + usize::from(has_password_step);
        let completed = usize::from(metadata_sources_done.get())
            + usize::from(has_replayos_step && replay_api_done.get())
            + usize::from(has_password_step && password_done.get());
        format!("{completed}/{total}")
    });
    let show_complete_view = !force
        && status.has_metadata
        && status.has_thumbnail_index
        && (!status.is_device
            || (status.replay_api_active && !status.default_root_password_active));

    // Read the app-level activity signal (populated by SseEventsListener at
    // the App root). Per-row busy flags derive from it, so activity from
    // another tab/process is reflected here without a second SSE connection.
    let activity = use_context::<RwSignal<Activity>>().expect("Activity context");
    let is_busy = Memo::new(move |_| {
        metadata_pending.get()
            || replay_api_pending.get()
            || activity.with(|a| !matches!(a, Activity::Idle))
    });
    let metadata_busy = Memo::new(move |_| {
        metadata_pending.get()
            || activity.with(|a| {
                matches!(
                    a,
                    Activity::RefreshExternalMetadata { .. } | Activity::ThumbnailUpdate { .. }
                )
            })
    });
    let replay_api_busy = Memo::new(move |_| replay_api_pending.get());
    let password_busy = Memo::new(move |_| false);

    // Clear pending once SSE confirms the matching activity is running. Once
    // confirmed, the global signal drives `..._busy` and `pending` is no
    // longer needed.
    Effect::new(move |_| {
        activity.with(|a| match a {
            Activity::RefreshExternalMetadata { .. } | Activity::ThumbnailUpdate { .. }
                if metadata_pending.get_untracked() =>
            {
                metadata_pending.set(false);
            }
            _ => {}
        });
    });

    // Latch the per-row "done" flag only when the activity reports a
    // successful Complete phase. Failures and cancellations leave `done`
    // false so the user can see the Start button and retry.
    let metadata_completed = Memo::new(move |_| {
        activity.with(|a| {
            matches!(a, Activity::RefreshExternalMetadata { progress } if matches!(progress.phase, RefreshMetadataPhase::Complete))
        })
    });
    let thumbnail_completed = Memo::new(move |_| {
        activity.with(|a| {
            matches!(a, Activity::ThumbnailUpdate { progress, .. } if matches!(progress.phase, ThumbnailPhase::Complete))
        })
    });

    Effect::new(move |prev: Option<(bool, bool)>| {
        let met = metadata_completed.get();
        let thm = thumbnail_completed.get();
        if let Some((prev_met, prev_thm)) = prev {
            if !prev_met && met {
                metadata_done.set(true);
            }
            if !prev_thm && thm {
                thumbnail_done.set(true);
            }
        }
        (met, thm)
    });

    // Surface terminal failure/cancellation messages so the user sees why an
    // action didn't complete. The corresponding click handler clears these
    // when a new action is started.
    Effect::new(move |_| {
        activity.with(|a| match a {
            Activity::RefreshExternalMetadata { progress }
                if matches!(progress.phase, RefreshMetadataPhase::Failed) =>
            {
                metadata_error.set(Some(format_error("Failed", progress.error.as_deref())));
            }
            Activity::ThumbnailUpdate { progress, .. }
                if matches!(progress.phase, ThumbnailPhase::Failed) =>
            {
                metadata_error.set(Some(format_error("Failed", progress.error.as_deref())));
            }
            Activity::ThumbnailUpdate { progress, .. }
                if matches!(progress.phase, ThumbnailPhase::Cancelled) =>
            {
                metadata_error.set(Some(format_error("Cancelled", progress.error.as_deref())));
            }
            _ => {}
        });
    });

    #[cfg(feature = "hydrate")]
    let navigate = StoredValue::new(use_navigate());

    let on_setup_metadata = move |_: leptos::ev::MouseEvent| {
        if is_busy.get() || metadata_sources_done.get() {
            return;
        }
        if has_replayos_step && !admin_access {
            #[cfg(feature = "hydrate")]
            navigate.get_value()(
                "/settings/access?next=%2F%3Fsetup",
                NavigateOptions {
                    replace: false,
                    ..Default::default()
                },
            );
            return;
        }
        metadata_error.set(None);
        metadata_pending.set(true);
        leptos::task::spawn_local(async move {
            if let Err(e) = server_fns::start_setup_metadata_downloads().await {
                metadata_pending.set(false);
                metadata_error.set(Some(format!("Error: {e}")));
            }
        });
    };

    let on_setup_replayos_token = move |_: leptos::ev::MouseEvent| {
        if is_busy.get() || !has_replayos_step || replay_api_done.get() {
            return;
        }
        if !admin_access {
            #[cfg(feature = "hydrate")]
            navigate.get_value()(
                "/settings/access?next=%2F%3Fsetup",
                NavigateOptions {
                    replace: false,
                    ..Default::default()
                },
            );
            return;
        }
        replay_api_error.set(None);
        replay_api_pending.set(true);
        leptos::task::spawn_local(async move {
            match server_fns::enable_replay_api_assisted().await {
                Ok(status) if status.is_active() => {
                    replay_api_pending.set(false);
                    replay_api_done_local.set(true);
                }
                Ok(status) => {
                    replay_api_pending.set(false);
                    replay_api_error.set(Some(replay_api_status_message(status)));
                }
                Err(e) => {
                    replay_api_pending.set(false);
                    replay_api_error.set(Some(format!("Error: {e}")));
                }
            }
        });
    };
    let on_setup_password = move |_: leptos::ev::MouseEvent| {
        if !password_done.get() {
            #[cfg(feature = "hydrate")]
            navigate.get_value()(
                "/settings/access",
                NavigateOptions {
                    replace: false,
                    ..Default::default()
                },
            );
        }
    };

    let on_dismiss = move |_: leptos::ev::MouseEvent| {
        dismissed.set(true);
        leptos::task::spawn_local(async move {
            let _ = server_fns::dismiss_setup().await;
        });
    };

    // Need a second reference for the completion-state dismiss button.
    let on_dismiss_complete = on_dismiss;

    view! {
        <Show when=move || !dismissed.get() fallback=|| ()>
            <div class="setup-checklist">
                {move || {
                    if show_complete_view {
                        view! {
                            <p class="setup-complete">{t(i18n.locale.get(), Key::SetupComplete)}</p>
                            <button class="btn btn-text" on:click=on_dismiss_complete>
                                {t(i18n.locale.get(), Key::SetupDismiss)}
                            </button>
                        }.into_any()
                    } else {
                        view! {
                            <div class="setup-header">
                                <div class="setup-heading">
                                    <span class="setup-kicker">{"SETUP"}</span>
                                    <h3 class="setup-welcome">{t(i18n.locale.get(), Key::SetupWelcome)}</h3>
                                    <p class="setup-intro">{t(i18n.locale.get(), Key::SetupIntro)}</p>
                                </div>
                                <div class="setup-progress" aria-label="Setup progress">
                                    {move || setup_progress.get()}
                                </div>
                            </div>

                            <div class="setup-tasks">
                                <SetupTaskRow
                                    step="1"
                                    done=metadata_sources_done
                                    busy=metadata_busy
                                    global_busy=is_busy
                                    ignore_global_busy=false
                                    enabled=Signal::derive(move || !metadata_sources_done.get())
                                    error=metadata_error
                                    title_key=Key::SetupMetadataTitle
                                    hint_key=Key::SetupMetadataHint
                                    on_go=on_setup_metadata
                                />
                                <Show when=move || has_replayos_step>
                                    <SetupTaskRow
                                    step="2"
                                    done=replay_api_done
                                    busy=replay_api_busy
                                    global_busy=is_busy
                                    ignore_global_busy=false
                                    enabled=has_replayos_step
                                        error=replay_api_error
                                        title_key=Key::SetupReplayosTitle
                                        hint_key=Key::SetupReplayosHint
                                        on_go=on_setup_replayos_token
                                    />
                                    <Show when=move || !replay_api_done.get()>
                                        <a
                                            class="setup-manual-link"
                                            href="/settings/replayos"
                                        >
                                            {move || {
                                                t(i18n.locale.get(), Key::SetupReplayosManualLink)
                                            }}
                                        </a>
                                    </Show>
                                </Show>
                                <Show when=move || has_password_step>
                                    <SetupTaskRow
                                        step="3"
                                        done=password_done
                                        busy=password_busy
                                        global_busy=is_busy
                                        ignore_global_busy=true
                                        enabled=Signal::derive(move || !password_done.get())
                                        error=password_error
                                        title_key=Key::SetupPasswordTitle
                                        hint_key=Key::SetupPasswordHint
                                        on_go=on_setup_password
                                    />
                                </Show>
                            </div>
                            <Show when=move || is_busy.get()>
                                <p class="setup-busy-note">
                                    {t(i18n.locale.get(), Key::SetupBusyHint)}
                                </p>
                            </Show>

                            <div class="setup-actions">
                                <button class="btn btn-text" on:click=on_dismiss>
                                    {t(i18n.locale.get(), Key::SetupSkip)}
                                </button>
                            </div>
                        }.into_any()
                    }
                }}
            </div>
        </Show>
    }
}

fn replay_api_status_message(status: ReplayApiStatus) -> String {
    match status {
        ReplayApiStatus::NotConfigured => "Error: RePlayOS Net Control is not configured".into(),
        ReplayApiStatus::PendingRestart => "Error: RePlayOS is still restarting".into(),
        ReplayApiStatus::Active { .. } => String::new(),
        ReplayApiStatus::Unauthorized => "Error: RePlayOS rejected the Net Control code".into(),
        ReplayApiStatus::Unsupported { .. } => {
            "Error: This RePlayOS version does not support remote control".into()
        }
        ReplayApiStatus::Error { reason } => format!("Error: {reason}"),
    }
}

fn format_error(prefix: &str, detail: Option<&str>) -> String {
    match detail {
        Some(msg) if !msg.is_empty() => format!("{prefix}: {msg}"),
        _ => prefix.to_string(),
    }
}

/// A single task row in the setup checklist.
#[component]
fn SetupTaskRow(
    step: &'static str,
    #[prop(into)] done: Signal<bool>,
    busy: Memo<bool>,
    global_busy: Memo<bool>,
    ignore_global_busy: bool,
    #[prop(into)] enabled: Signal<bool>,
    error: RwSignal<Option<String>>,
    title_key: Key,
    hint_key: Key,
    on_go: impl Fn(leptos::ev::MouseEvent) + Send + Sync + 'static,
) -> impl IntoView {
    let i18n = use_i18n();
    let on_go = StoredValue::new(on_go);

    let status_class = move || {
        if busy.get() {
            "setup-task in-progress"
        } else if done.get() {
            "setup-task done"
        } else {
            "setup-task"
        }
    };

    view! {
        <div class="setup-task-row">
            <div class=status_class>
                <div class="setup-task-marker">
                    {move || {
                        if done.get() {
                            view! { <span class="setup-task-check">{"OK"}</span> }.into_any()
                        } else {
                            view! { <span class="setup-task-step">{step}</span> }.into_any()
                        }
                    }}
                </div>
                <div class="setup-task-info">
                    <span class="setup-task-title">{move || t(i18n.locale.get(), title_key)}</span>
                    <span class="setup-task-hint">{move || t(i18n.locale.get(), hint_key)}</span>
                </div>
                <div class="setup-task-action">
                    {move || {
                        if busy.get() {
                            view! {
                                <span class="setup-task-status">
                                    <span class="busy-spinner"></span>
                                    {t(i18n.locale.get(), Key::SetupInProgress)}
                                </span>
                            }.into_any()
                        } else if done.get() {
                            view! {
                                <span class="setup-task-done">{t(i18n.locale.get(), Key::SetupDone)}</span>
                            }.into_any()
                        } else {
                            view! {
                                <button
                                    class="btn btn-accent btn-sm"
                                    on:click=move |ev| on_go.with_value(|f| f(ev))
                                    disabled=move || (!ignore_global_busy && global_busy.get()) || !enabled.get()
                                >
                                    {t(i18n.locale.get(), Key::SetupStart)}
                                </button>
                            }.into_any()
                        }
                    }}
                </div>
            </div>
            <Show when=move || error.read().is_some()>
                <p class="setup-task-error">{move || error.get().unwrap_or_default()}</p>
            </Show>
        </div>
    }
}
