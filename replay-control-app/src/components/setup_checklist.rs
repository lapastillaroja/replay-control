use leptos::prelude::*;
use leptos_router::hooks::use_query_map;

use crate::i18n::{Key, t, use_i18n};
use crate::server_fns::{self, Activity, ImportState, SetupStatus, ThumbnailPhase};

/// Dismissible first-run setup checklist shown at the top of the Home page.
///
/// Uses `Resource::new` (non-blocking) so it doesn't delay SSR TTFB.
/// Auto-hides when both tasks are complete or the user dismisses it.
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

/// The actual setup card with two task rows and dismiss controls.
#[component]
fn SetupCard(status: SetupStatus, force: bool) -> impl IntoView {
    let i18n = use_i18n();

    let metadata_done = RwSignal::new(status.has_metadata);
    let thumbnail_done = RwSignal::new(status.has_thumbnail_index);
    let metadata_error = RwSignal::new(None::<String>);
    let thumbnail_error = RwSignal::new(None::<String>);
    // Local "pending" flags give immediate visual feedback on click — the
    // server fn round-trip + SSE delivery takes a beat, and without these
    // the row sits on the Start button for that beat (looks like the click
    // didn't register). Cleared when SSE confirms the activity (Effect below)
    // or when the server fn returns Err (handler).
    let metadata_pending = RwSignal::new(false);
    let thumbnail_pending = RwSignal::new(false);
    let dismissed = RwSignal::new(false);

    // "Setup complete!" view is only shown when the user landed here with
    // both tasks already done (organic visit). Re-runs from `?setup` or
    // tasks completed in-page leave the checklist visible so the user
    // can see the result and re-run if needed.
    let show_complete_view = !force && status.has_metadata && status.has_thumbnail_index;

    // Read the app-level activity signal (populated by SseActivityListener at
    // the App root). Per-row busy flags derive from it, so activity from
    // another tab/process is reflected here without a second SSE connection.
    let activity = use_context::<RwSignal<Activity>>().expect("Activity context");
    let is_busy = Memo::new(move |_| {
        metadata_pending.get()
            || thumbnail_pending.get()
            || activity.with(|a| !matches!(a, Activity::Idle))
    });
    let metadata_busy = Memo::new(move |_| {
        metadata_pending.get() || activity.with(|a| matches!(a, Activity::Import { .. }))
    });
    let thumbnail_busy = Memo::new(move |_| {
        thumbnail_pending.get() || activity.with(|a| matches!(a, Activity::ThumbnailUpdate { .. }))
    });

    // Clear pending once SSE confirms the matching activity is running. Once
    // confirmed, the global signal drives `..._busy` and `pending` is no
    // longer needed.
    Effect::new(move |_| {
        activity.with(|a| match a {
            Activity::Import { .. } if metadata_pending.get_untracked() => {
                metadata_pending.set(false);
            }
            Activity::ThumbnailUpdate { .. } if thumbnail_pending.get_untracked() => {
                thumbnail_pending.set(false);
            }
            _ => {}
        });
    });

    // Latch the per-row "done" flag only when the activity reports a
    // successful Complete phase. Failures and cancellations leave `done`
    // false so the user can see the Start button and retry.
    let metadata_completed = Memo::new(move |_| {
        activity.with(|a| {
            matches!(a, Activity::Import { progress } if matches!(progress.state, ImportState::Complete))
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
            Activity::Import { progress } if matches!(progress.state, ImportState::Failed) => {
                metadata_error.set(Some(format_error("Failed", progress.error.as_deref())));
            }
            Activity::ThumbnailUpdate { progress, .. }
                if matches!(progress.phase, ThumbnailPhase::Failed) =>
            {
                thumbnail_error.set(Some(format_error("Failed", progress.error.as_deref())));
            }
            Activity::ThumbnailUpdate { progress, .. }
                if matches!(progress.phase, ThumbnailPhase::Cancelled) =>
            {
                thumbnail_error.set(Some(format_error("Cancelled", progress.error.as_deref())));
            }
            _ => {}
        });
    });

    let on_download_metadata = move |_: leptos::ev::MouseEvent| {
        if is_busy.get() {
            return;
        }
        metadata_error.set(None);
        metadata_pending.set(true);
        leptos::task::spawn_local(async move {
            if let Err(e) = server_fns::download_metadata().await {
                metadata_pending.set(false);
                metadata_error.set(Some(format!("Error: {e}")));
            }
        });
    };

    let on_update_thumbnails = move |_: leptos::ev::MouseEvent| {
        if is_busy.get() {
            return;
        }
        thumbnail_error.set(None);
        thumbnail_pending.set(true);
        leptos::task::spawn_local(async move {
            if let Err(e) = server_fns::update_thumbnails().await {
                thumbnail_pending.set(false);
                thumbnail_error.set(Some(format!("Error: {e}")));
            }
        });
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
                            <h3 class="setup-welcome">{t(i18n.locale.get(), Key::SetupWelcome)}</h3>
                            <p class="setup-intro">{t(i18n.locale.get(), Key::SetupIntro)}</p>

                            <div class="setup-tasks">
                                <SetupTaskRow
                                    done=metadata_done
                                    busy=metadata_busy
                                    global_busy=is_busy
                                    error=metadata_error
                                    title_key=Key::SetupMetadataTitle
                                    hint_key=Key::SetupMetadataHint
                                    on_go=on_download_metadata
                                />
                                <SetupTaskRow
                                    done=thumbnail_done
                                    busy=thumbnail_busy
                                    global_busy=is_busy
                                    error=thumbnail_error
                                    title_key=Key::SetupThumbnailTitle
                                    hint_key=Key::SetupThumbnailHint
                                    on_go=on_update_thumbnails
                                />
                            </div>

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

fn format_error(prefix: &str, detail: Option<&str>) -> String {
    match detail {
        Some(msg) if !msg.is_empty() => format!("{prefix}: {msg}"),
        _ => prefix.to_string(),
    }
}

/// A single task row in the setup checklist.
#[component]
fn SetupTaskRow(
    done: RwSignal<bool>,
    busy: Memo<bool>,
    global_busy: Memo<bool>,
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
                <div class="setup-task-info">
                    <span class="setup-task-title">{move || t(i18n.locale.get(), title_key)}</span>
                    <span class="setup-task-hint">{move || t(i18n.locale.get(), hint_key)}</span>
                </div>
                <div class="setup-task-action">
                    {move || {
                        if busy.get() {
                            view! {
                                <span class="setup-task-status">
                                    <span class="metadata-busy-spinner"></span>
                                    {t(i18n.locale.get(), Key::SetupInProgress)}
                                </span>
                            }.into_any()
                        } else {
                            let label_key = if done.get() { Key::SetupUpdate } else { Key::SetupStart };
                            view! {
                                <button
                                    class="btn btn-accent btn-sm"
                                    on:click=move |ev| on_go.with_value(|f| f(ev))
                                    disabled=move || global_busy.get()
                                >
                                    {t(i18n.locale.get(), label_key)}
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
