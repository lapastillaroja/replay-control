use leptos::prelude::*;
use leptos_router::components::A;
use server_fn::ServerFnError;

use crate::i18n::{Key, t, use_i18n};
use crate::server_fns::{self, Activity, ImportState, RebuildPhase, ThumbnailPhase};
use crate::util::{format_number, format_size};

#[component]
pub fn MetadataPage() -> impl IntoView {
    let i18n = use_i18n();
    // Non-blocking: each section is wrapped in Suspense with skeleton fallbacks.
    let stats = Resource::new(|| (), |_| server_fns::get_metadata_stats());
    let coverage = Resource::new(|| (), |_| server_fns::get_system_coverage());
    let data_source = Resource::new(|| (), |_| server_fns::get_thumbnail_data_source());
    let image_stats = Resource::new(|| (), |_| server_fns::get_image_stats());
    let builtin_stats = Resource::new(|| (), |_| server_fns::get_builtin_db_stats());

    // Single activity signal (replaces importing + thumb_updating + rebuilding).
    let activity = RwSignal::new(Activity::Idle);

    // Per-operation result messages — prevents a rebuild message from showing in LaunchBox/Thumbnail sections.
    let import_result = RwSignal::new(None::<String>);
    let thumb_result = RwSignal::new(None::<String>);
    let rebuild_result = RwSignal::new(None::<String>);

    // Progress signals derived from activity for display components.
    let import_progress = Memo::new(move |_| match activity.get() {
        Activity::Import { progress } => Some(progress),
        _ => None,
    });
    let thumb_progress = Memo::new(move |_| match activity.get() {
        Activity::ThumbnailUpdate { progress, .. } => Some(progress),
        _ => None,
    });

    // Derived helpers.
    let is_busy = Memo::new(move |_| !matches!(activity.get(), Activity::Idle));
    let is_importing = Memo::new(move |_| matches!(activity.get(), Activity::Import { .. }));
    let is_thumb_updating =
        Memo::new(move |_| matches!(activity.get(), Activity::ThumbnailUpdate { .. }));
    let can_cancel = Memo::new(move |_| matches!(activity.get(), Activity::ThumbnailUpdate { .. }));

    // Thumbnail cancel UI state (local, not derived from server).
    let thumb_cancelling = RwSignal::new(false);

    // Close any leaked EventSource connections when this component unmounts.
    #[cfg(target_arch = "wasm32")]
    {
        on_cleanup(move || {
            close_activity_sse();
        });
    }

    // Check for in-progress operations on page load.
    Effect::new(move || {
        leptos::task::spawn_local(async move {
            if let Ok(act) = server_fns::get_activity().await
                && !matches!(act, Activity::Idle)
            {
                activity.set(act);
                watch_activity(
                    activity,
                    import_result,
                    thumb_result,
                    rebuild_result,
                    thumb_cancelling,
                    stats,
                    coverage,
                    data_source,
                    image_stats,
                );
            }
        });
    });

    let on_download = move |_| {
        if is_busy.get() {
            return;
        }
        import_result.set(None);
        leptos::task::spawn_local(async move {
            match server_fns::download_metadata().await {
                Ok(()) => {
                    // Set a placeholder activity so buttons disable immediately.
                    activity.set(Activity::Import {
                        progress: server_fns::ImportProgress {
                            state: ImportState::Downloading,
                            processed: 0,
                            matched: 0,
                            inserted: 0,
                            elapsed_secs: 0,
                            error: None,
                            download_bytes: 0,
                            download_total: None,
                        },
                    });
                    watch_activity(
                        activity,
                        import_result,
                        thumb_result,
                        rebuild_result,
                        thumb_cancelling,
                        stats,
                        coverage,
                        data_source,
                        image_stats,
                    );
                }
                Err(e) => {
                    import_result.set(Some(format!("Error: {e}")));
                }
            }
        });
    };

    let on_thumb_update = move |_| {
        if is_busy.get() {
            return;
        }
        thumb_cancelling.set(false);
        thumb_result.set(None);
        leptos::task::spawn_local(async move {
            match server_fns::update_thumbnails().await {
                Ok(()) => {
                    activity.set(server_fns::make_thumbnail_update_activity(
                        server_fns::ThumbnailProgress {
                            phase: ThumbnailPhase::Indexing,
                            current_label: String::new(),
                            step_done: 0,
                            step_total: 0,
                            downloaded: 0,
                            entries_indexed: 0,
                            elapsed_secs: 0,
                            error: None,
                        },
                    ));
                    watch_activity(
                        activity,
                        import_result,
                        thumb_result,
                        rebuild_result,
                        thumb_cancelling,
                        stats,
                        coverage,
                        data_source,
                        image_stats,
                    );
                }
                Err(e) => {
                    thumb_result.set(Some(format!("Error: {e}")));
                }
            }
        });
    };

    let on_thumb_cancel = move |_| {
        thumb_cancelling.set(true);
        leptos::task::spawn_local(async move {
            let _ = server_fns::cancel_thumbnail_update().await;
        });
    };

    view! {
        <div class="page metadata-page">
            <div class="rom-header">
                <A href="/settings" attr:class="back-btn">
                    {move || t(i18n.locale.get(), Key::GamesBack)}
                </A>
                <h2 class="page-title">{move || t(i18n.locale.get(), Key::MetadataTitle)}</h2>
            </div>

            // ── System Overview ───────────────────────────────────────
            <section class="section">
                <h2 class="section-title">{move || t(i18n.locale.get(), Key::MetadataSystemOverview)}</h2>
                <Suspense fallback=move || view! { <MetadataTableSkeleton /> }>
                    {move || Suspend::new(async move {
                        let locale = i18n.locale.get();
                        let data = coverage.await?;

                        let has_any_data = data.iter().any(|c| c.with_metadata > 0 || c.with_thumbnail > 0);

                        Ok::<_, ServerFnError>(if !has_any_data {
                            view! {
                                <p class="game-section-empty">{t(locale, Key::MetadataNoSystems)}</p>
                            }.into_any()
                        } else {
                            let rows = data.into_iter()
                                .filter(|c| c.with_metadata > 0 || c.with_thumbnail > 0)
                                .map(|c| {
                                    let desc_pct = if c.total_games > 0 && c.with_metadata > 0 {
                                        format!("{}%", (c.with_metadata as f64 / c.total_games as f64 * 100.0) as u32)
                                    } else {
                                        "--".to_string()
                                    };
                                    let thumb_pct = if c.total_games > 0 && c.with_thumbnail > 0 {
                                        format!("{}%", (c.with_thumbnail as f64 / c.total_games as f64 * 100.0) as u32)
                                    } else {
                                        "--".to_string()
                                    };
                                    view! {
                                        <tr>
                                            <td class="overview-system">{c.display_name}</td>
                                            <td class="overview-num">{c.total_games}</td>
                                            <td class="overview-num">{desc_pct}</td>
                                            <td class="overview-num">{thumb_pct}</td>
                                        </tr>
                                    }
                                })
                                .collect::<Vec<_>>();
                            view! {
                                <div class="overview-table-wrap">
                                    <table class="overview-table">
                                        <thead>
                                            <tr>
                                                <th class="overview-system">{t(locale, Key::MetadataColSystem)}</th>
                                                <th class="overview-num">{t(locale, Key::MetadataColGames)}</th>
                                                <th class="overview-num">{t(locale, Key::MetadataColDesc)}</th>
                                                <th class="overview-num">{t(locale, Key::MetadataColThumb)}</th>
                                            </tr>
                                        </thead>
                                        <tbody>{rows}</tbody>
                                    </table>
                                </div>
                            }.into_any()
                        })
                    })}
                </Suspense>
            </section>

            // ── Data Sources ──────────────────────────────────────────
            <section class="section">
                <h2 class="section-title">{move || t(i18n.locale.get(), Key::MetadataDataSources)}</h2>

                // Built-in data info block
                <Suspense fallback=move || view! { <MetadataCardSkeleton /> }>
                    {move || Suspend::new(async move {
                        let locale = i18n.locale.get();
                        let bs = builtin_stats.await?;
                        Ok::<_, ServerFnError>(view! {
                            <div class="data-source-card builtin-info">
                                <div class="data-source-header">
                                    <span class="data-source-name">{t(locale, Key::MetadataBuiltin)}</span>
                                </div>
                                <p class="data-source-summary">
                                    {format!(
                                        "{} {} {} — {} {} {} {} — {} {} {} {}",
                                        format_number(bs.arcade_entries),
                                        t(locale, Key::MetadataBuiltinArcadeSummary),
                                        bs.arcade_mame_version,
                                        format_number(bs.game_rom_entries),
                                        t(locale, Key::MetadataBuiltinConsoleSummaryEntries),
                                        bs.game_system_count,
                                        t(locale, Key::MetadataBuiltinConsoleSummarySystems),
                                        format_number(bs.wikidata_series_entries),
                                        t(locale, Key::MetadataBuiltinWikidataEntries),
                                        bs.wikidata_series_count,
                                        t(locale, Key::MetadataBuiltinWikidataSeries),
                                    )}
                                </p>
                                <p class="settings-hint">{t(locale, Key::MetadataBuiltinHint)}</p>
                            </div>
                        })
                    })}
                </Suspense>

                // Descriptions & Ratings (LaunchBox)
                <div class="data-source-card">
                    <div class="data-source-header">
                        <span class="data-source-name">{move || t(i18n.locale.get(), Key::MetadataDescriptionsLaunchbox)}</span>
                    </div>
                    <Suspense fallback=move || view! { <MetadataLineSkeleton /> }>
                        {move || Suspend::new(async move {
                            let locale = i18n.locale.get();
                            let data = stats.await?;
                            Ok::<_, ServerFnError>(if data.total_entries == 0 {
                                view! {
                                    <p class="data-source-summary dim">{t(locale, Key::MetadataNoData)}</p>
                                }.into_any()
                            } else {
                                let updated = if data.last_updated_text.is_empty() {
                                    String::new()
                                } else {
                                    format!(" — {}", data.last_updated_text)
                                };
                                view! {
                                    <p class="data-source-summary">
                                        {format!("{} {}{}", data.total_entries, t(locale, Key::MetadataEntriesSummary), updated)}
                                    </p>
                                }.into_any()
                            })
                        })}
                    </Suspense>
                    <div class="data-source-actions">
                        <button
                            class="metadata-download-btn"
                            on:click=on_download
                            disabled=move || is_busy.get()
                        >
                            {move || if is_importing.get() {
                                t(i18n.locale.get(), Key::CommonUpdating)
                            } else {
                                t(i18n.locale.get(), Key::CommonUpdate)
                            }}
                        </button>
                    </div>
                    <Show when=move || import_progress.get().is_some()>
                        <ImportProgressDisplay progress=import_progress />
                    </Show>
                    <Show when=move || import_result.read().is_some()>
                        <p class="settings-saved">{move || import_result.get().unwrap_or_default()}</p>
                    </Show>
                </div>

                // Thumbnails (libretro)
                <div class="data-source-card">
                    <div class="data-source-header">
                        <span class="data-source-name">{move || t(i18n.locale.get(), Key::MetadataThumbnailsLibretro)}</span>
                    </div>
                    <Suspense fallback=move || view! { <MetadataLineSkeleton /> }>
                        {move || Suspend::new(async move {
                            let locale = i18n.locale.get();
                            let ds = data_source.await?;
                            let (with_boxart, with_snap, media_size) = image_stats.await?;

                            Ok::<_, ServerFnError>(if ds.entry_count == 0 && with_boxart == 0 {
                                view! {
                                    <p class="data-source-summary dim">{t(locale, Key::MetadataNoData)}</p>
                                }.into_any()
                            } else {
                                let images_line = if with_boxart > 0 || with_snap > 0 {
                                    format!(
                                        "{} {}, {} {} — {} {}",
                                        with_boxart,
                                        t(locale, Key::MetadataThumbnailSummary),
                                        with_snap,
                                        t(locale, Key::MetadataThumbnailSnaps),
                                        format_size(media_size),
                                        t(locale, Key::MetadataThumbnailOnDisk),
                                    )
                                } else {
                                    String::new()
                                };
                                let index_line = if ds.entry_count > 0 {
                                    let updated = if ds.last_updated_text.is_empty() {
                                        String::new()
                                    } else {
                                        format!(" — {}", ds.last_updated_text)
                                    };
                                    format!(
                                        "Index: {} {} {} {}{}",
                                        ds.entry_count,
                                        t(locale, Key::MetadataThumbnailIndexSummary),
                                        ds.repo_count,
                                        t(locale, Key::MetadataThumbnailSystems),
                                        updated,
                                    )
                                } else {
                                    String::new()
                                };
                                let has_images = !images_line.is_empty();
                                let has_index = !index_line.is_empty();
                                view! {
                                    <div class="data-source-details">
                                        {has_images.then(|| view! {
                                            <p class="data-source-summary">{images_line}</p>
                                        })}
                                        {has_index.then(|| view! {
                                            <p class="data-source-summary">{index_line}</p>
                                        })}
                                    </div>
                                }.into_any()
                            })
                        })}
                    </Suspense>
                    <div class="data-source-actions">
                        <button
                            class="metadata-download-btn"
                            on:click=on_thumb_update
                            disabled=move || is_busy.get()
                        >
                            {move || if is_thumb_updating.get() {
                                t(i18n.locale.get(), Key::CommonUpdating)
                            } else {
                                t(i18n.locale.get(), Key::CommonUpdate)
                            }}
                        </button>
                        <Show when=move || can_cancel.get()>
                            <button
                                class="form-btn form-btn-secondary"
                                on:click=on_thumb_cancel
                                disabled=move || thumb_cancelling.get()
                            >
                                {move || if thumb_cancelling.get() {
                                    t(i18n.locale.get(), Key::MetadataThumbnailCancelling)
                                } else {
                                    t(i18n.locale.get(), Key::MetadataThumbnailStop)
                                }}
                            </button>
                        </Show>
                    </div>
                    <Show when=move || thumb_progress.get().is_some()>
                        <ThumbnailProgressDisplay progress=thumb_progress />
                    </Show>
                    <Show when=move || thumb_result.read().is_some()>
                        <p class="settings-saved">{move || thumb_result.get().unwrap_or_default()}</p>
                    </Show>
                </div>
            </section>

            // ── Data Management ───────────────────────────────────────
            <DataManagementSection stats coverage activity result_message=rebuild_result is_busy />

            // ── Attribution ───────────────────────────────────────────
            <section class="section">
                <h2 class="section-title">{move || t(i18n.locale.get(), Key::MetadataAttribution)}</h2>
                <p class="settings-hint">{move || t(i18n.locale.get(), Key::MetadataAttributionText)}</p>
            </section>
        </div>
    }
}

// ── EventSource lifecycle management ─────────────────────────────────────

#[cfg(target_arch = "wasm32")]
thread_local! {
    static ACTIVITY_ES: std::cell::RefCell<Option<web_sys::EventSource>> = const { std::cell::RefCell::new(None) };
}

#[cfg(target_arch = "wasm32")]
fn close_activity_sse() {
    ACTIVITY_ES.with(|cell| {
        if let Some(es) = cell.borrow_mut().take() {
            es.close();
        }
    });
}

/// Watches activity progress via single SSE endpoint.
///
/// On SSR this is a no-op; the real work happens client-side via EventSource.
#[allow(clippy::too_many_arguments)] // SSE watcher needs activity + 3 result signals + resources
fn watch_activity(
    activity: RwSignal<Activity>,
    import_result: RwSignal<Option<String>>,
    thumb_result: RwSignal<Option<String>>,
    rebuild_result: RwSignal<Option<String>>,
    thumb_cancelling: RwSignal<bool>,
    stats: Resource<Result<server_fns::MetadataStats, ServerFnError>>,
    coverage: Resource<Result<Vec<server_fns::SystemCoverage>, ServerFnError>>,
    data_source: Resource<Result<server_fns::DataSourceSummary, ServerFnError>>,
    image_stats: Resource<Result<(usize, usize, u64), ServerFnError>>,
) {
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (
        &activity,
        &import_result,
        &thumb_result,
        &rebuild_result,
        &thumb_cancelling,
        &stats,
        &coverage,
        &data_source,
        &image_stats,
    );

    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::prelude::*;

        // Close any existing SSE connection before opening a new one.
        close_activity_sse();

        let es = match web_sys::EventSource::new("/sse/activity") {
            Ok(es) => es,
            Err(_) => return,
        };

        // Track this connection so on_cleanup or a future call can close it.
        ACTIVITY_ES.with(|cell| {
            *cell.borrow_mut() = Some(es.clone());
        });

        let es_clone = es.clone();
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

                if act.is_terminal() {
                    // Extract completion text before resetting to Idle.
                    let message = act.terminal_message();

                    // Reset cancelling state if this was a thumbnail operation.
                    if matches!(act, Activity::ThumbnailUpdate { .. }) {
                        thumb_cancelling.set(false);
                    }

                    // 1. Set activity to Idle immediately (buttons re-enable).
                    activity.set(Activity::Idle);

                    // 2. Set result message on the correct per-operation signal.
                    if !message.is_empty() {
                        let target = match &act {
                            Activity::Import { .. } => import_result,
                            Activity::ThumbnailUpdate { .. } => thumb_result,
                            Activity::Rebuild { .. } => rebuild_result,
                            _ => import_result, // fallback
                        };
                        target.set(Some(message));

                        // 3. Start client-side timer to clear the message after 5s.
                        gloo_timers::callback::Timeout::new(5_000, move || {
                            target.set(None);
                        })
                        .forget();
                    }

                    // Refetch relevant resources.
                    stats.refetch();
                    coverage.refetch();
                    data_source.refetch();
                    image_stats.refetch();

                    // Close SSE — operation is done.
                    es_clone.close();
                    ACTIVITY_ES.with(|cell| {
                        cell.borrow_mut().take();
                    });
                } else if matches!(act, Activity::Idle) {
                    // Server went Idle (guard dropped). Update activity signal.
                    activity.set(Activity::Idle);
                    // Refetch in case we missed a terminal event.
                    stats.refetch();
                    coverage.refetch();
                    data_source.refetch();
                    image_stats.refetch();

                    // Close SSE.
                    es_clone.close();
                    ACTIVITY_ES.with(|cell| {
                        cell.borrow_mut().take();
                    });
                } else {
                    activity.set(act);
                }
            });

        es.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        on_message.forget();

        // When the server closes the stream (idle timeout), close our side
        // to prevent EventSource auto-reconnect spam.
        let es_err = es.clone();
        let on_error = Closure::<dyn Fn()>::new(move || {
            es_err.close();
            activity.set(Activity::Idle);
            ACTIVITY_ES.with(|cell| {
                cell.borrow_mut().take();
            });
        });
        es.set_onerror(Some(on_error.as_ref().unchecked_ref()));
        on_error.forget();
    }
}

/// Displays real-time import progress.
#[component]
fn ImportProgressDisplay(progress: Memo<Option<server_fns::ImportProgress>>) -> impl IntoView {
    let i18n = use_i18n();

    view! {
        <div class="import-progress">
            {move || {
                let locale = i18n.locale.get();
                match progress.get() {
                    Some(p) => {
                        let state_text = match p.state {
                            ImportState::Downloading => {
                                if p.download_bytes > 0 {
                                    match p.download_total {
                                        Some(total) if total > 0 => format!(
                                            "{} {} / {}",
                                            t(locale, Key::MetadataDownloadingFile),
                                            format_size(p.download_bytes),
                                            format_size(total),
                                        ),
                                        _ => format!(
                                            "{} {}",
                                            t(locale, Key::MetadataDownloadingFile),
                                            format_size(p.download_bytes),
                                        ),
                                    }
                                } else {
                                    t(locale, Key::MetadataDownloadingFile).to_string()
                                }
                            }
                            ImportState::BuildingIndex => t(locale, Key::MetadataBuildingIndex).to_string(),
                            ImportState::Parsing => format!(
                                "{} ({} {}, {} {})",
                                t(locale, Key::MetadataParsingXml),
                                p.processed,
                                t(locale, Key::MetadataProcessed),
                                p.matched,
                                t(locale, Key::MetadataMatched),
                            ),
                            ImportState::Complete => t(locale, Key::MetadataImportComplete).to_string(),
                            ImportState::Failed => format!(
                                "{}: {}",
                                t(locale, Key::MetadataImportFailed),
                                p.error.as_deref().unwrap_or(""),
                            ),
                        };
                        let elapsed = format!("{}s", p.elapsed_secs);
                        view! {
                            <div class="import-progress-bar">
                                <span class="import-progress-text">{state_text}</span>
                                <span class="import-progress-time">{elapsed}</span>
                            </div>
                        }.into_any()
                    }
                    None => view! { <span></span> }.into_any(),
                }
            }}
        </div>
    }
}

/// Displays thumbnail update progress.
#[component]
fn ThumbnailProgressDisplay(
    progress: Memo<Option<server_fns::ThumbnailProgress>>,
) -> impl IntoView {
    let i18n = use_i18n();

    view! {
        <div class="import-progress">
            {move || {
                let locale = i18n.locale.get();
                match progress.get() {
                    Some(p) => {
                        let state_text = match p.phase {
                            ThumbnailPhase::Indexing => {
                                if p.step_total > 0 {
                                    format!(
                                        "{} {}/{} done — {}",
                                        t(locale, Key::MetadataThumbnailPhaseIndexing),
                                        p.step_done,
                                        p.step_total,
                                        p.current_label,
                                    )
                                } else {
                                    t(locale, Key::MetadataThumbnailPhaseIndexing).to_string()
                                }
                            }
                            ThumbnailPhase::Downloading => {
                                format!(
                                    "{} {} ({} {})",
                                    t(locale, Key::MetadataThumbnailPhaseDownloading),
                                    p.current_label,
                                    p.downloaded,
                                    t(locale, Key::MetadataThumbnailDownloaded),
                                )
                            }
                            ThumbnailPhase::Complete => format!(
                                "{}: {} {}, {} {}",
                                t(locale, Key::MetadataThumbnailComplete),
                                p.entries_indexed,
                                t(locale, Key::MetadataThumbnailIndexed),
                                p.downloaded,
                                t(locale, Key::MetadataThumbnailDownloaded),
                            ),
                            ThumbnailPhase::Failed => format!(
                                "{}: {}",
                                t(locale, Key::MetadataThumbnailFailed),
                                p.error.as_deref().unwrap_or(""),
                            ),
                            ThumbnailPhase::Cancelled => format!(
                                "{}: {} {}",
                                t(locale, Key::MetadataThumbnailCancelled),
                                p.downloaded,
                                t(locale, Key::MetadataThumbnailDownloaded),
                            ),
                        };
                        let elapsed = format!("{}s", p.elapsed_secs);
                        view! {
                            <div class="import-progress-bar">
                                <span class="import-progress-text">{state_text}</span>
                                <span class="import-progress-time">{elapsed}</span>
                            </div>
                        }.into_any()
                    }
                    None => view! { <span></span> }.into_any(),
                }
            }}
        </div>
    }
}

/// Data Management section with main and advanced actions.
#[component]
fn DataManagementSection(
    stats: Resource<Result<server_fns::MetadataStats, ServerFnError>>,
    coverage: Resource<Result<Vec<server_fns::SystemCoverage>, ServerFnError>>,
    activity: RwSignal<Activity>,
    result_message: RwSignal<Option<String>>,
    is_busy: Memo<bool>,
) -> impl IntoView {
    let i18n = use_i18n();
    let show_advanced = RwSignal::new(false);

    // Rebuild state
    let rebuilding = Memo::new(move |_| matches!(activity.get(), Activity::Rebuild { .. }));
    let rebuild_display = Memo::new(move |_| match activity.get() {
        Activity::Rebuild { progress } => match progress.phase {
            RebuildPhase::Scanning => {
                if progress.systems_total > 0 {
                    Some(format!(
                        "Scanning {}... ({}/{})",
                        progress.current_system, progress.systems_done, progress.systems_total,
                    ))
                } else if progress.current_system.is_empty() {
                    Some("Scanning...".to_string())
                } else {
                    Some(format!("Scanning {}...", progress.current_system))
                }
            }
            RebuildPhase::Enriching => {
                if progress.systems_total > 0 {
                    Some(format!(
                        "Enriching {}... ({}/{})",
                        progress.current_system, progress.systems_done, progress.systems_total,
                    ))
                } else if progress.current_system.is_empty() {
                    Some("Enriching...".to_string())
                } else {
                    Some(format!("Enriching {}...", progress.current_system))
                }
            }
            _ => None,
        },
        _ => None,
    });

    // Rebuild Game Library
    let confirming_rebuild = RwSignal::new(false);

    // Clear Downloaded Images
    let confirming_images = RwSignal::new(false);
    let clearing_images = RwSignal::new(false);
    let images_result = RwSignal::new(Option::<String>::None);

    // Cleanup Orphaned Images
    let confirming_orphans = RwSignal::new(false);
    let cleaning_orphans = RwSignal::new(false);
    let orphans_result = RwSignal::new(Option::<String>::None);

    // Clear Thumbnail Index
    let confirming_index = RwSignal::new(false);
    let clearing_index = RwSignal::new(false);
    let index_result = RwSignal::new(Option::<String>::None);

    // Clear Metadata
    let confirming_metadata = RwSignal::new(false);
    let clearing_metadata = RwSignal::new(false);
    let metadata_result = RwSignal::new(Option::<String>::None);

    let on_rebuild = Callback::new(move |_: leptos::ev::MouseEvent| {
        if is_busy.get() {
            return;
        }
        result_message.set(None);
        confirming_rebuild.set(false);
        leptos::task::spawn_local(async move {
            match server_fns::rebuild_game_library().await {
                Ok(()) => {
                    if let Ok(act) = server_fns::get_activity().await {
                        activity.set(act);
                    }
                    watch_activity(
                        activity,
                        RwSignal::new(None), // import_result (unused for rebuild)
                        RwSignal::new(None), // thumb_result (unused for rebuild)
                        result_message,      // rebuild_result
                        RwSignal::new(false), // no cancelling for rebuild
                        stats,
                        coverage,
                        Resource::new_blocking(|| (), |_| server_fns::get_thumbnail_data_source()),
                        Resource::new_blocking(|| (), |_| server_fns::get_image_stats()),
                    );
                }
                Err(e) => {
                    result_message.set(Some(format!("Error: {e}")));
                }
            }
        });
    });

    let on_clear_images = Callback::new(move |_: leptos::ev::MouseEvent| {
        if is_busy.get() {
            return;
        }
        clearing_images.set(true);
        images_result.set(None);
        leptos::task::spawn_local(async move {
            match server_fns::clear_images().await {
                Ok(()) => {
                    images_result.set(Some(
                        t(i18n.locale.get(), Key::MetadataClearedImages).to_string(),
                    ));
                }
                Err(e) => {
                    images_result.set(Some(format!("Error: {e}")));
                }
            }
            clearing_images.set(false);
            confirming_images.set(false);
        });
    });

    let on_cleanup_orphans = Callback::new(move |_: leptos::ev::MouseEvent| {
        if is_busy.get() {
            return;
        }
        cleaning_orphans.set(true);
        orphans_result.set(None);
        leptos::task::spawn_local(async move {
            match server_fns::cleanup_orphaned_images().await {
                Ok((metadata_deleted, files_deleted, bytes_freed)) => {
                    let size = format_size(bytes_freed);
                    orphans_result.set(Some(format!(
                        "Cleaned up {files_deleted} images ({size}), {metadata_deleted} metadata rows"
                    )));
                }
                Err(e) => {
                    orphans_result.set(Some(format!("Error: {e}")));
                }
            }
            cleaning_orphans.set(false);
            confirming_orphans.set(false);
        });
    });

    let on_clear_index = Callback::new(move |_: leptos::ev::MouseEvent| {
        if is_busy.get() {
            return;
        }
        clearing_index.set(true);
        index_result.set(None);
        leptos::task::spawn_local(async move {
            match server_fns::clear_thumbnail_index().await {
                Ok(()) => {
                    index_result.set(Some(
                        t(i18n.locale.get(), Key::MetadataIndexCleared).to_string(),
                    ));
                }
                Err(e) => {
                    index_result.set(Some(format!("Error: {e}")));
                }
            }
            clearing_index.set(false);
            confirming_index.set(false);
        });
    });

    let on_clear_metadata = Callback::new(move |_: leptos::ev::MouseEvent| {
        if is_busy.get() {
            return;
        }
        clearing_metadata.set(true);
        metadata_result.set(None);
        leptos::task::spawn_local(async move {
            match server_fns::clear_metadata().await {
                Ok(()) => {
                    metadata_result.set(Some(
                        t(i18n.locale.get(), Key::MetadataMetadataCleared).to_string(),
                    ));
                    stats.refetch();
                    coverage.refetch();
                }
                Err(e) => {
                    metadata_result.set(Some(format!("Error: {e}")));
                }
            }
            clearing_metadata.set(false);
            confirming_metadata.set(false);
        });
    });

    view! {
        <section class="section">
            <h2 class="section-title">{move || t(i18n.locale.get(), Key::MetadataDataManagement)}</h2>
            <div class="manage-actions">
                // Main actions (always visible)
                <ClearActionCard
                    confirming=confirming_rebuild
                    clearing=rebuilding
                    result=result_message
                    label_key=Key::MetadataRebuildGameLibrary
                    clearing_key=Key::MetadataRebuildingGameLibrary
                    confirm_key=Key::MetadataConfirmRebuildGameLibrary
                    on_confirm=on_rebuild
                    disabled=is_busy
                    progress_text=rebuild_display
                />
                <ClearActionCard
                    confirming=confirming_orphans
                    clearing=cleaning_orphans
                    result=orphans_result
                    label_key=Key::MetadataCleanupOrphans
                    clearing_key=Key::MetadataCleaningOrphans
                    confirm_key=Key::MetadataConfirmCleanupOrphans
                    on_confirm=on_cleanup_orphans
                    disabled=is_busy
                />
            </div>

            // Advanced actions (collapsed by default) — destructive or costly operations
            <div class="advanced-toggle">
                <button
                    class="advanced-toggle-btn"
                    on:click=move |_| show_advanced.update(|v| *v = !*v)
                >
                    <span class="advanced-toggle-icon">{move || if show_advanced.get() { "\u{25BC}" } else { "\u{25B6}" }}</span>
                    {move || t(i18n.locale.get(), Key::MetadataAdvancedActions)}
                </button>
            </div>
            <Show when=move || show_advanced.get()>
                <div class="manage-actions">
                    <ClearActionCard
                        confirming=confirming_images
                        clearing=clearing_images
                        result=images_result
                        label_key=Key::MetadataClearImages
                        clearing_key=Key::CommonClearing
                        confirm_key=Key::MetadataConfirmClearImages
                        on_confirm=on_clear_images
                        disabled=is_busy
                    />
                    <ClearActionCard
                        confirming=confirming_metadata
                        clearing=clearing_metadata
                        result=metadata_result
                        label_key=Key::MetadataClearMetadata
                        clearing_key=Key::CommonClearing
                        confirm_key=Key::MetadataConfirmClearMetadata
                        on_confirm=on_clear_metadata
                        disabled=is_busy
                    />
                    <ClearActionCard
                        confirming=confirming_index
                        clearing=clearing_index
                        result=index_result
                        label_key=Key::MetadataClearIndex
                        clearing_key=Key::CommonClearing
                        confirm_key=Key::MetadataConfirmClearIndex
                        on_confirm=on_clear_index
                        disabled=is_busy
                    />
                </div>
            </Show>
        </section>
    }
}

/// Reusable card for a destructive action with confirmation.
#[component]
fn ClearActionCard(
    confirming: RwSignal<bool>,
    #[prop(into)] clearing: Signal<bool>,
    result: RwSignal<Option<String>>,
    label_key: Key,
    clearing_key: Key,
    confirm_key: Key,
    on_confirm: Callback<leptos::ev::MouseEvent>,
    #[prop(optional)] disabled: Option<Memo<bool>>,
    /// Live progress text shown while the operation runs (e.g., rebuild per-system progress).
    #[prop(optional)]
    progress_text: Option<Memo<Option<String>>>,
) -> impl IntoView {
    let i18n = use_i18n();
    let externally_disabled = move || disabled.is_some_and(|d| d.get());

    view! {
        <div class="manage-action-card">
            <Show when=move || confirming.get()
                fallback=move || view! {
                    <button
                        class="game-action-btn game-action-delete"
                        on:click=move |_| confirming.set(true)
                        disabled=externally_disabled
                    >
                        {move || t(i18n.locale.get(), label_key)}
                    </button>
                }
            >
                <p class="manage-action-hint">{move || t(i18n.locale.get(), confirm_key)}</p>
                <div class="game-delete-confirm">
                    <button
                        class="game-action-btn game-action-delete-confirm"
                        on:click=move |ev| on_confirm.run(ev)
                        disabled=move || clearing.get() || externally_disabled()
                    >
                        {move || if clearing.get() {
                            t(i18n.locale.get(), clearing_key)
                        } else {
                            t(i18n.locale.get(), label_key)
                        }}
                    </button>
                    <button class="game-action-btn" on:click=move |_| confirming.set(false)>
                        {move || t(i18n.locale.get(), Key::CommonCancel)}
                    </button>
                </div>
            </Show>
            {move || progress_text.and_then(|pt| pt.get()).map(|text| view! {
                <p class="manage-action-progress">{text}</p>
            })}
            <Show when=move || result.read().is_some()>
                <p class="manage-action-result">{move || result.get().unwrap_or_default()}</p>
            </Show>
        </div>
    }
}

// ── Skeleton components ──────────────────────────────────────────────────

/// Skeleton for the system overview table (4 shimmer rows).
#[component]
fn MetadataTableSkeleton() -> impl IntoView {
    view! {
        <div class="meta-skeleton-table">
            {(0..4).map(|_| view! {
                <div class="meta-skeleton-row skeleton-shimmer">
                    <div class="meta-skeleton-cell-wide"></div>
                    <div class="meta-skeleton-cell"></div>
                    <div class="meta-skeleton-cell"></div>
                    <div class="meta-skeleton-cell"></div>
                </div>
            }).collect::<Vec<_>>()}
        </div>
    }
}

/// Skeleton for a data-source card (builtin info block).
#[component]
fn MetadataCardSkeleton() -> impl IntoView {
    view! {
        <div class="data-source-card">
            <div class="meta-skeleton-bar-wide skeleton-shimmer"></div>
            <div class="meta-skeleton-bar skeleton-shimmer"></div>
        </div>
    }
}

/// Skeleton for a single summary line inside a data-source card.
#[component]
fn MetadataLineSkeleton() -> impl IntoView {
    view! {
        <div class="meta-skeleton-bar skeleton-shimmer"></div>
    }
}
