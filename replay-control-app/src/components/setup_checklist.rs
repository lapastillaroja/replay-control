use leptos::prelude::*;
use leptos_router::hooks::use_query_map;

use crate::i18n::{Key, t, use_i18n};
use crate::server_fns::{self, Activity, SetupStatus};

/// Dismissible first-run setup checklist shown at the top of the Home page.
///
/// Uses `Resource::new` (non-blocking) so it doesn't delay SSR TTFB.
/// Auto-hides when both tasks are complete or the user dismisses it.
/// Append `?setup` to the URL to force-show the card (for testing/screenshots).
#[component]
pub fn SetupChecklist() -> impl IntoView {
    let query = use_query_map();
    let force = query.read().get_str("setup").is_some();
    let status = Resource::new(move || force, |force| server_fns::get_setup_status(force));

    view! {
        <Suspense fallback=|| ()>
            {move || Suspend::new(async move {
                let status = status.await?;
                Ok::<_, server_fn::ServerFnError>(if status.show_setup {
                    view! { <SetupCard status /> }.into_any()
                } else {
                    ().into_any()
                })
            })}
        </Suspense>
    }
}

/// The actual setup card with two task rows and dismiss controls.
#[component]
fn SetupCard(status: SetupStatus) -> impl IntoView {
    let i18n = use_i18n();

    let metadata_done = RwSignal::new(status.has_metadata);
    let thumbnail_done = RwSignal::new(status.has_thumbnail_index);
    let dismissed = RwSignal::new(false);
    let metadata_busy = RwSignal::new(false);
    let thumbnail_busy = RwSignal::new(false);
    let activity = RwSignal::new(Activity::Idle);

    // Check if a background task is already running when the component mounts.
    #[cfg(target_arch = "wasm32")]
    Effect::new(move || {
        leptos::task::spawn_local(async move {
            if let Ok(act) = server_fns::get_activity().await {
                if matches!(act, Activity::Import { .. }) {
                    metadata_busy.set(true);
                } else if matches!(act, Activity::ThumbnailUpdate { .. }) {
                    thumbnail_busy.set(true);
                }
                if !matches!(act, Activity::Idle) {
                    activity.set(act);
                    watch_setup_activity(
                        activity,
                        metadata_done,
                        thumbnail_done,
                        metadata_busy,
                        thumbnail_busy,
                    );
                }
            }
        });
    });

    let is_busy = Memo::new(move |_| !matches!(activity.get(), Activity::Idle));
    let all_done = Memo::new(move |_| metadata_done.get() && thumbnail_done.get());

    let on_download_metadata = move |_: leptos::ev::MouseEvent| {
        if is_busy.get() {
            return;
        }
        leptos::task::spawn_local(async move {
            if server_fns::download_metadata().await.is_ok() {
                metadata_busy.set(true);
                activity.set(Activity::Import {
                    progress: server_fns::ImportProgress {
                        state: server_fns::ImportState::Downloading,
                        processed: 0,
                        matched: 0,
                        inserted: 0,
                        elapsed_secs: 0,
                        error: None,
                        download_bytes: 0,
                        download_total: None,
                    },
                });
                #[cfg(target_arch = "wasm32")]
                watch_setup_activity(
                    activity,
                    metadata_done,
                    thumbnail_done,
                    metadata_busy,
                    thumbnail_busy,
                );
            }
        });
    };

    let on_update_thumbnails = move |_: leptos::ev::MouseEvent| {
        if is_busy.get() {
            return;
        }
        leptos::task::spawn_local(async move {
            if server_fns::update_thumbnails().await.is_ok() {
                thumbnail_busy.set(true);
                activity.set(server_fns::make_thumbnail_update_activity(
                    server_fns::ThumbnailProgress {
                        phase: server_fns::ThumbnailPhase::Indexing,
                        current_label: String::new(),
                        step_done: 0,
                        step_total: 0,
                        downloaded: 0,
                        entries_indexed: 0,
                        elapsed_secs: 0,
                        error: None,
                    },
                ));
                #[cfg(target_arch = "wasm32")]
                watch_setup_activity(
                    activity,
                    metadata_done,
                    thumbnail_done,
                    metadata_busy,
                    thumbnail_busy,
                );
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
                    if all_done.get() {
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
                                    title_key=Key::SetupMetadataTitle
                                    hint_key=Key::SetupMetadataHint
                                    on_go=on_download_metadata
                                />
                                <SetupTaskRow
                                    done=thumbnail_done
                                    busy=thumbnail_busy
                                    global_busy=is_busy
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

/// A single task row in the setup checklist.
#[component]
fn SetupTaskRow(
    done: RwSignal<bool>,
    busy: RwSignal<bool>,
    global_busy: Memo<bool>,
    title_key: Key,
    hint_key: Key,
    on_go: impl Fn(leptos::ev::MouseEvent) + Send + Sync + 'static,
) -> impl IntoView {
    let i18n = use_i18n();
    let on_go = StoredValue::new(on_go);

    let status_class = move || {
        if done.get() {
            "setup-task done"
        } else if busy.get() {
            "setup-task in-progress"
        } else {
            "setup-task"
        }
    };

    view! {
        <div class=status_class>
            <div class="setup-task-info">
                <span class="setup-task-title">{move || t(i18n.locale.get(), title_key)}</span>
                <span class="setup-task-hint">{move || t(i18n.locale.get(), hint_key)}</span>
            </div>
            <div class="setup-task-action">
                {move || {
                    if done.get() {
                        view! {
                            <span class="setup-task-done">{t(i18n.locale.get(), Key::SetupTaskDone)}</span>
                        }.into_any()
                    } else if busy.get() {
                        view! {
                            <span class="setup-task-status">
                                <span class="metadata-busy-spinner"></span>
                                {t(i18n.locale.get(), Key::SetupInProgress)}
                            </span>
                        }.into_any()
                    } else {
                        view! {
                            <button
                                class="btn btn-accent btn-sm"
                                on:click=move |ev| on_go.with_value(|f| f(ev))
                                disabled=move || global_busy.get()
                            >
                                {t(i18n.locale.get(), Key::SetupStart)}
                            </button>
                        }.into_any()
                    }
                }}
            </div>
        </div>
    }
}

/// Watch activity via SSE (client-side only).
/// Closes the connection when activity returns to Idle.
#[cfg(target_arch = "wasm32")]
fn watch_setup_activity(
    activity: RwSignal<Activity>,
    metadata_done: RwSignal<bool>,
    thumbnail_done: RwSignal<bool>,
    metadata_busy: RwSignal<bool>,
    thumbnail_busy: RwSignal<bool>,
) {
    use wasm_bindgen::prelude::*;

    let es = match web_sys::EventSource::new("/sse/activity") {
        Ok(es) => es,
        Err(_) => return,
    };

    let es_for_idle = es.clone();
    let on_message =
        Closure::<dyn Fn(web_sys::MessageEvent)>::new(move |event: web_sys::MessageEvent| {
            let data = event.data().as_string().unwrap_or_default();
            if data.is_empty() {
                return;
            }
            let act: Activity = match serde_json::from_str(&data) {
                Ok(act) => act,
                Err(_) => return,
            };

            let is_done = act.is_terminal() || matches!(act, Activity::Idle);
            if is_done {
                if metadata_busy.get_untracked() {
                    metadata_done.set(true);
                    metadata_busy.set(false);
                }
                if thumbnail_busy.get_untracked() {
                    thumbnail_done.set(true);
                    thumbnail_busy.set(false);
                }
                activity.set(Activity::Idle);
                es_for_idle.close();
            } else {
                activity.set(act);
            }
        });

    es.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    on_message.forget();

    // Close on server-side stream end to prevent auto-reconnect.
    let es_for_err = es.clone();
    let on_error = Closure::<dyn Fn()>::new(move || {
        es_for_err.close();
        activity.set(Activity::Idle);
    });
    es.set_onerror(Some(on_error.as_ref().unchecked_ref()));
    on_error.forget();
}
