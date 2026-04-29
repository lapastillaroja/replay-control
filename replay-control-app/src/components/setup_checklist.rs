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
    let status = Resource::new(
        move || query.read().get_str("setup").is_some(),
        server_fns::get_setup_status,
    );

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

    // Read the app-level activity signal (populated by SseActivityListener at
    // the App root). Per-row busy flags derive from it, so activity from
    // another tab/process is reflected here without a second SSE connection.
    let activity = use_context::<RwSignal<Activity>>().expect("Activity context");
    let is_busy = Memo::new(move |_| !matches!(activity.get(), Activity::Idle));
    let metadata_busy = Memo::new(move |_| matches!(activity.get(), Activity::Import { .. }));
    let thumbnail_busy =
        Memo::new(move |_| matches!(activity.get(), Activity::ThumbnailUpdate { .. }));

    // Latch the per-row "done" flag on a busy → not-busy transition.
    // Treats both Complete and Failed as "done"; the next page load re-derives
    // truth from the DB via get_setup_status.
    Effect::new(move |prev: Option<(bool, bool)>| {
        let met = metadata_busy.get();
        let thm = thumbnail_busy.get();
        if let Some((prev_met, prev_thm)) = prev {
            if prev_met && !met {
                metadata_done.set(true);
            }
            if prev_thm && !thm {
                thumbnail_done.set(true);
            }
        }
        (met, thm)
    });

    let all_done = Memo::new(move |_| metadata_done.get() && thumbnail_done.get());

    let on_download_metadata = move |_: leptos::ev::MouseEvent| {
        if is_busy.get() {
            return;
        }
        leptos::task::spawn_local(async move {
            let _ = server_fns::download_metadata().await;
        });
    };

    let on_update_thumbnails = move |_: leptos::ev::MouseEvent| {
        if is_busy.get() {
            return;
        }
        leptos::task::spawn_local(async move {
            let _ = server_fns::update_thumbnails().await;
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
    busy: Memo<bool>,
    global_busy: Memo<bool>,
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
    }
}
