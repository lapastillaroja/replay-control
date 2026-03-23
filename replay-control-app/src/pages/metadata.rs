use leptos::prelude::*;
use leptos_router::components::A;
use server_fn::ServerFnError;

use crate::i18n::{t, use_i18n};
use crate::pages::ErrorDisplay;
use crate::server_fns::{self, ImportState, ThumbnailPhase};
use crate::util::{format_number, format_size};

#[component]
pub fn MetadataPage() -> impl IntoView {
    let i18n = use_i18n();
    let stats = Resource::new_blocking(|| (), |_| server_fns::get_metadata_stats());
    let coverage = Resource::new_blocking(|| (), |_| server_fns::get_system_coverage());
    let data_source = Resource::new_blocking(|| (), |_| server_fns::get_thumbnail_data_source());
    let image_stats = Resource::new_blocking(|| (), |_| server_fns::get_image_stats());
    let builtin_stats = Resource::new_blocking(|| (), |_| server_fns::get_builtin_db_stats());

    // LaunchBox import state
    let importing = RwSignal::new(false);
    let import_message = RwSignal::new(Option::<String>::None);
    let progress = RwSignal::new(Option::<server_fns::ImportProgress>::None);

    // Thumbnail update state
    let thumb_updating = RwSignal::new(false);
    let thumb_progress = RwSignal::new(Option::<server_fns::ThumbnailProgress>::None);
    let thumb_message = RwSignal::new(Option::<String>::None);
    let thumb_cancelling = RwSignal::new(false);

    // Close any leaked EventSource connections when this component unmounts.
    #[cfg(target_arch = "wasm32")]
    {
        on_cleanup(move || {
            close_metadata_sse();
            close_thumbnail_sse();
        });
    }

    // Check for in-progress operations on page load.
    Effect::new(move || {
        leptos::task::spawn_local(async move {
            if let Ok(Some(p)) = server_fns::get_import_progress().await {
                match p.state {
                    ImportState::Downloading
                    | ImportState::BuildingIndex
                    | ImportState::Parsing => {
                        progress.set(Some(p));
                        importing.set(true);
                        watch_metadata_progress(
                            importing,
                            progress,
                            import_message,
                            stats,
                            coverage,
                        );
                    }
                    ImportState::Complete => {
                        import_message.set(Some(format!(
                            "Import complete: {} matched, {} inserted ({}s)",
                            p.matched, p.inserted, p.elapsed_secs,
                        )));
                    }
                    ImportState::Failed => {
                        import_message.set(Some(format!(
                            "Import failed: {}",
                            p.error.as_deref().unwrap_or("unknown error"),
                        )));
                    }
                }
            }
        });
    });

    Effect::new(move || {
        leptos::task::spawn_local(async move {
            if let Ok(Some(p)) = server_fns::get_thumbnail_progress().await {
                match p.phase {
                    ThumbnailPhase::Indexing | ThumbnailPhase::Downloading => {
                        thumb_progress.set(Some(p));
                        thumb_updating.set(true);
                        watch_thumbnail_progress(
                            thumb_updating,
                            thumb_progress,
                            thumb_message,
                            thumb_cancelling,
                            data_source,
                            image_stats,
                            coverage,
                        );
                    }
                    ThumbnailPhase::Complete => {
                        thumb_message.set(Some(format!(
                            "Complete: {} indexed, {} downloaded ({}s)",
                            p.entries_indexed, p.downloaded, p.elapsed_secs,
                        )));
                    }
                    ThumbnailPhase::Cancelled => {
                        thumb_message.set(Some(format!(
                            "Cancelled after {}s ({} downloaded)",
                            p.elapsed_secs, p.downloaded,
                        )));
                    }
                    ThumbnailPhase::Failed => {
                        thumb_message.set(Some(format!(
                            "Failed: {}",
                            p.error.as_deref().unwrap_or("unknown error"),
                        )));
                    }
                }
            }
        });
    });

    let on_download = move |_| {
        if importing.get() || thumb_updating.get() {
            return;
        }
        leptos::task::spawn_local(async move {
            if let Ok(busy) = server_fns::is_metadata_busy().await {
                if busy {
                    import_message.set(Some("Another operation is already running".to_string()));
                    return;
                }
            }
            importing.set(true);
            import_message.set(None);
            progress.set(None);
            match server_fns::download_metadata().await {
                Ok(()) => {
                    watch_metadata_progress(importing, progress, import_message, stats, coverage);
                }
                Err(e) => {
                    import_message.set(Some(format!("Error: {e}")));
                    importing.set(false);
                }
            }
        });
    };

    let on_thumb_update = move |_| {
        if thumb_updating.get() || importing.get() {
            return;
        }
        // Check server-side busy state before showing progress UI.
        // Prevents "Fetching index..." flash followed by error when
        // another operation (e.g., LaunchBox import) is already running.
        leptos::task::spawn_local(async move {
            if let Ok(busy) = server_fns::is_metadata_busy().await {
                if busy {
                    thumb_message.set(Some("Another operation is already running".to_string()));
                    return;
                }
            }
            thumb_updating.set(true);
            thumb_cancelling.set(false);
            thumb_message.set(None);
            thumb_progress.set(Some(server_fns::ThumbnailProgress {
                phase: ThumbnailPhase::Indexing,
                current_label: String::new(),
                step_done: 0,
                step_total: 0,
                downloaded: 0,
                entries_indexed: 0,
                elapsed_secs: 0,
                error: None,
            }));
            match server_fns::update_thumbnails().await {
                Ok(()) => {
                    watch_thumbnail_progress(
                        thumb_updating,
                        thumb_progress,
                        thumb_message,
                        thumb_cancelling,
                        data_source,
                        image_stats,
                        coverage,
                    );
                }
                Err(e) => {
                    thumb_message.set(Some(format!("Error: {e}")));
                    thumb_updating.set(false);
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
                <A href="/more" attr:class="back-btn">
                    {move || t(i18n.locale.get(), "games.back")}
                </A>
                <h2 class="page-title">{move || t(i18n.locale.get(), "metadata.title")}</h2>
            </div>

            // ── System Overview ───────────────────────────────────────
            <section class="section">
                <h2 class="section-title">{move || t(i18n.locale.get(), "metadata.system_overview")}</h2>
                <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }>
                    <Suspense fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), "common.loading")}</div> }>
                        {move || Suspend::new(async move {
                            let locale = i18n.locale.get();
                            let data = coverage.await?;

                            let has_any_data = data.iter().any(|c| c.with_metadata > 0 || c.with_thumbnail > 0);

                            Ok::<_, ServerFnError>(if !has_any_data {
                                view! {
                                    <p class="game-section-empty">{t(locale, "metadata.no_systems")}</p>
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
                                                    <th class="overview-system">{t(locale, "metadata.col_system")}</th>
                                                    <th class="overview-num">{t(locale, "metadata.col_games")}</th>
                                                    <th class="overview-num">{t(locale, "metadata.col_desc")}</th>
                                                    <th class="overview-num">{t(locale, "metadata.col_thumb")}</th>
                                                </tr>
                                            </thead>
                                            <tbody>{rows}</tbody>
                                        </table>
                                    </div>
                                }.into_any()
                            })
                        })}
                    </Suspense>
                </ErrorBoundary>
            </section>

            // ── Data Sources ──────────────────────────────────────────
            <section class="section">
                <h2 class="section-title">{move || t(i18n.locale.get(), "metadata.data_sources")}</h2>

                // Built-in data info block
                <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }>
                    <Suspense fallback=move || ()>
                        {move || Suspend::new(async move {
                            let locale = i18n.locale.get();
                            let bs = builtin_stats.await?;
                            Ok::<_, ServerFnError>(view! {
                                <div class="data-source-card builtin-info">
                                    <div class="data-source-header">
                                        <span class="data-source-name">{t(locale, "metadata.builtin")}</span>
                                    </div>
                                    <p class="data-source-summary">
                                        {format!(
                                            "{} {} {} — {} {} {} {} — {} {} {} {}",
                                            format_number(bs.arcade_entries),
                                            t(locale, "metadata.builtin_arcade_summary"),
                                            bs.arcade_mame_version,
                                            format_number(bs.game_rom_entries),
                                            t(locale, "metadata.builtin_console_summary_entries"),
                                            bs.game_system_count,
                                            t(locale, "metadata.builtin_console_summary_systems"),
                                            format_number(bs.wikidata_series_entries),
                                            t(locale, "metadata.builtin_wikidata_entries"),
                                            bs.wikidata_series_count,
                                            t(locale, "metadata.builtin_wikidata_series"),
                                        )}
                                    </p>
                                    <p class="settings-hint">{t(locale, "metadata.builtin_hint")}</p>
                                </div>
                            })
                        })}
                    </Suspense>
                </ErrorBoundary>

                // Descriptions & Ratings (LaunchBox)
                <div class="data-source-card">
                    <div class="data-source-header">
                        <span class="data-source-name">{move || t(i18n.locale.get(), "metadata.descriptions_launchbox")}</span>
                    </div>
                    <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }>
                        <Suspense fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), "common.loading")}</div> }>
                            {move || Suspend::new(async move {
                                let locale = i18n.locale.get();
                                let data = stats.await?;
                                Ok::<_, ServerFnError>(if data.total_entries == 0 {
                                    view! {
                                        <p class="data-source-summary dim">{t(locale, "metadata.no_data")}</p>
                                    }.into_any()
                                } else {
                                    let updated = if data.last_updated_text.is_empty() {
                                        String::new()
                                    } else {
                                        format!(" — {}", data.last_updated_text)
                                    };
                                    view! {
                                        <p class="data-source-summary">
                                            {format!("{} {}{}", data.total_entries, t(locale, "metadata.entries_summary"), updated)}
                                        </p>
                                    }.into_any()
                                })
                            })}
                        </Suspense>
                    </ErrorBoundary>
                    <div class="data-source-actions">
                        <button
                            class="metadata-download-btn"
                            on:click=on_download
                            disabled=move || importing.get() || thumb_updating.get()
                        >
                            {move || if importing.get() {
                                t(i18n.locale.get(), "metadata.downloading_metadata")
                            } else {
                                t(i18n.locale.get(), "metadata.download_metadata")
                            }}
                        </button>
                    </div>
                    <Show when=move || progress.read().is_some()>
                        <ImportProgressDisplay progress />
                    </Show>
                    <Show when=move || import_message.read().is_some()>
                        <p class="settings-saved">{move || import_message.get().unwrap_or_default()}</p>
                    </Show>
                </div>

                // Thumbnails (libretro)
                <div class="data-source-card">
                    <div class="data-source-header">
                        <span class="data-source-name">{move || t(i18n.locale.get(), "metadata.thumbnails_libretro")}</span>
                    </div>
                    <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }>
                        <Suspense fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), "common.loading")}</div> }>
                            {move || Suspend::new(async move {
                                let locale = i18n.locale.get();
                                let ds = data_source.await?;
                                let (with_boxart, with_snap, media_size) = image_stats.await?;

                                Ok::<_, ServerFnError>(if ds.entry_count == 0 && with_boxart == 0 {
                                    view! {
                                        <p class="data-source-summary dim">{t(locale, "metadata.thumbnail_no_data")}</p>
                                    }.into_any()
                                } else {
                                    let images_line = if with_boxart > 0 || with_snap > 0 {
                                        format!(
                                            "{} {}, {} {} — {} {}",
                                            with_boxart,
                                            t(locale, "metadata.thumbnail_summary"),
                                            with_snap,
                                            t(locale, "metadata.thumbnail_snaps"),
                                            format_size(media_size),
                                            t(locale, "metadata.thumbnail_on_disk"),
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
                                            t(locale, "metadata.thumbnail_index_summary"),
                                            ds.repo_count,
                                            t(locale, "metadata.thumbnail_systems"),
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
                    </ErrorBoundary>
                    <div class="data-source-actions">
                        <button
                            class="metadata-download-btn"
                            on:click=on_thumb_update
                            disabled=move || thumb_updating.get() || importing.get()
                        >
                            {move || if thumb_updating.get() {
                                t(i18n.locale.get(), "metadata.thumbnail_updating")
                            } else {
                                t(i18n.locale.get(), "metadata.thumbnail_update")
                            }}
                        </button>
                        <Show when=move || thumb_updating.get()>
                            <button
                                class="form-btn form-btn-secondary"
                                on:click=on_thumb_cancel
                                disabled=move || thumb_cancelling.get()
                            >
                                {move || if thumb_cancelling.get() {
                                    t(i18n.locale.get(), "metadata.thumbnail_cancelling")
                                } else {
                                    t(i18n.locale.get(), "metadata.thumbnail_stop")
                                }}
                            </button>
                        </Show>
                    </div>
                    <Show when=move || thumb_progress.read().is_some()>
                        <ThumbnailProgressDisplay progress=thumb_progress />
                    </Show>
                    <Show when=move || thumb_message.read().is_some()>
                        <p class="settings-saved">{move || thumb_message.get().unwrap_or_default()}</p>
                    </Show>
                </div>
            </section>

            // ── Data Management ───────────────────────────────────────
            <DataManagementSection stats coverage />

            // ── Attribution ───────────────────────────────────────────
            <section class="section">
                <h2 class="section-title">{move || t(i18n.locale.get(), "metadata.attribution")}</h2>
                <p class="settings-hint">{move || t(i18n.locale.get(), "metadata.attribution_text")}</p>
            </section>
        </div>
    }
}

// ── EventSource lifecycle management ─────────────────────────────────────
//
// We track active SSE connections in thread-locals so we can close old ones
// before opening new ones (prevents leaked connections on SPA navigation)
// and close them on component unmount via on_cleanup.

#[cfg(target_arch = "wasm32")]
thread_local! {
    static METADATA_ES: std::cell::RefCell<Option<web_sys::EventSource>> = const { std::cell::RefCell::new(None) };
    static THUMBNAIL_ES: std::cell::RefCell<Option<web_sys::EventSource>> = const { std::cell::RefCell::new(None) };
}

#[cfg(target_arch = "wasm32")]
fn close_metadata_sse() {
    METADATA_ES.with(|cell| {
        if let Some(es) = cell.borrow_mut().take() {
            es.close();
        }
    });
}

#[cfg(target_arch = "wasm32")]
fn close_thumbnail_sse() {
    THUMBNAIL_ES.with(|cell| {
        if let Some(es) = cell.borrow_mut().take() {
            es.close();
        }
    });
}

#[cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)]
fn close_metadata_sse() {}

#[cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)]
fn close_thumbnail_sse() {}

/// Watches metadata import progress via SSE.
#[allow(unused_variables, unreachable_code)]
fn watch_metadata_progress(
    importing: RwSignal<bool>,
    progress: RwSignal<Option<server_fns::ImportProgress>>,
    import_message: RwSignal<Option<String>>,
    stats: Resource<Result<server_fns::MetadataStats, ServerFnError>>,
    coverage: Resource<Result<Vec<server_fns::SystemCoverage>, ServerFnError>>,
) {
    #[cfg(not(target_arch = "wasm32"))]
    return;

    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::prelude::*;

        // Close any existing metadata SSE connection before opening a new one.
        close_metadata_sse();

        let es = match web_sys::EventSource::new("/sse/metadata-progress") {
            Ok(es) => es,
            Err(_) => return,
        };

        // Track this connection so on_cleanup or a future call can close it.
        METADATA_ES.with(|cell| {
            *cell.borrow_mut() = Some(es.clone());
        });

        let es_clone = es.clone();
        let on_message =
            Closure::<dyn Fn(web_sys::MessageEvent)>::new(move |event: web_sys::MessageEvent| {
                let data = event.data().as_string().unwrap_or_default();
                if data == "null" || data.is_empty() {
                    return;
                }
                let p: server_fns::ImportProgress = match serde_json::from_str(&data) {
                    Ok(p) => p,
                    Err(_) => return,
                };

                let done = matches!(p.state, ImportState::Complete | ImportState::Failed);

                if p.state == ImportState::Complete {
                    import_message.set(Some(format!(
                        "Import complete: {} matched, {} inserted ({}s)",
                        p.matched, p.inserted, p.elapsed_secs,
                    )));
                } else if p.state == ImportState::Failed {
                    import_message.set(Some(format!(
                        "Import failed: {}",
                        p.error.as_deref().unwrap_or("unknown error"),
                    )));
                }

                progress.set(Some(p));

                if done {
                    importing.set(false);
                    stats.refetch();
                    coverage.refetch();
                    es_clone.close();
                    METADATA_ES.with(|cell| {
                        cell.borrow_mut().take();
                    });
                }
            });

        es.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        on_message.forget();

        // When the server closes the stream (idle timeout), close our side
        // to prevent EventSource auto-reconnect spam.
        let es_err = es.clone();
        let on_error = Closure::<dyn Fn()>::new(move || {
            es_err.close();
            METADATA_ES.with(|cell| {
                cell.borrow_mut().take();
            });
        });
        es.set_onerror(Some(on_error.as_ref().unchecked_ref()));
        on_error.forget();
    }
}

/// Watches thumbnail update progress via SSE.
#[allow(unused_variables, unreachable_code)]
fn watch_thumbnail_progress(
    updating: RwSignal<bool>,
    progress: RwSignal<Option<server_fns::ThumbnailProgress>>,
    message: RwSignal<Option<String>>,
    cancelling: RwSignal<bool>,
    data_source: Resource<Result<server_fns::DataSourceSummary, ServerFnError>>,
    image_stats: Resource<Result<(usize, usize, u64), ServerFnError>>,
    coverage: Resource<Result<Vec<server_fns::SystemCoverage>, ServerFnError>>,
) {
    #[cfg(not(target_arch = "wasm32"))]
    return;

    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::prelude::*;

        // Close any existing thumbnail SSE connection before opening a new one.
        close_thumbnail_sse();

        let es = match web_sys::EventSource::new("/sse/thumbnail-progress") {
            Ok(es) => es,
            Err(_) => return,
        };

        // Track this connection so on_cleanup or a future call can close it.
        THUMBNAIL_ES.with(|cell| {
            *cell.borrow_mut() = Some(es.clone());
        });

        let es_clone = es.clone();
        let on_message =
            Closure::<dyn Fn(web_sys::MessageEvent)>::new(move |event: web_sys::MessageEvent| {
                let data = event.data().as_string().unwrap_or_default();
                if data == "null" || data.is_empty() {
                    return;
                }
                let p: server_fns::ThumbnailProgress = match serde_json::from_str(&data) {
                    Ok(p) => p,
                    Err(_) => return,
                };

                let done = matches!(
                    p.phase,
                    ThumbnailPhase::Complete | ThumbnailPhase::Failed | ThumbnailPhase::Cancelled
                );

                if done {
                    cancelling.set(false);
                    match p.phase {
                        ThumbnailPhase::Complete => {
                            message.set(Some(format!(
                                "Complete: {} indexed, {} downloaded ({}s)",
                                p.entries_indexed, p.downloaded, p.elapsed_secs,
                            )));
                        }
                        ThumbnailPhase::Cancelled => {
                            message.set(Some(format!(
                                "Cancelled after {}s ({} downloaded)",
                                p.elapsed_secs, p.downloaded,
                            )));
                        }
                        ThumbnailPhase::Failed => {
                            message.set(Some(format!(
                                "Failed: {}",
                                p.error.as_deref().unwrap_or("unknown error"),
                            )));
                        }
                        _ => {}
                    }
                    progress.set(Some(p));
                    updating.set(false);
                    data_source.refetch();
                    image_stats.refetch();
                    coverage.refetch();
                    es_clone.close();
                    THUMBNAIL_ES.with(|cell| {
                        cell.borrow_mut().take();
                    });
                    return;
                }

                progress.set(Some(p));
            });

        es.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        on_message.forget();

        // When the server closes the stream (idle timeout), close our side
        // to prevent EventSource auto-reconnect spam.
        let es_err = es.clone();
        let on_error = Closure::<dyn Fn()>::new(move || {
            es_err.close();
            THUMBNAIL_ES.with(|cell| {
                cell.borrow_mut().take();
            });
        });
        es.set_onerror(Some(on_error.as_ref().unchecked_ref()));
        on_error.forget();
    }
}

/// Polls `is_metadata_busy()` every 2 seconds until the rebuild completes.
/// On completion, shows a success message and refetches stats/coverage.
#[allow(unused_variables, unreachable_code)]
async fn poll_rebuild_completion(
    rebuilding: RwSignal<bool>,
    rebuild_result: RwSignal<Option<String>>,
    stats: Resource<Result<server_fns::MetadataStats, ServerFnError>>,
    coverage: Resource<Result<Vec<server_fns::SystemCoverage>, ServerFnError>>,
    i18n: crate::i18n::I18nContext,
) {
    #[cfg(not(target_arch = "wasm32"))]
    return;

    #[cfg(target_arch = "wasm32")]
    {
        loop {
            gloo_timers::future::TimeoutFuture::new(2_000).await;
            match server_fns::is_metadata_busy().await {
                Ok(true) => {
                    // Still running, keep polling.
                }
                Ok(false) => {
                    // Rebuild complete.
                    rebuild_result.set(Some(
                        t(i18n.locale.get(), "metadata.game_library_rebuilt").to_string(),
                    ));
                    rebuilding.set(false);
                    stats.refetch();
                    coverage.refetch();
                    break;
                }
                Err(_) => {
                    // Server error — stop polling, show generic success
                    // since the rebuild was accepted.
                    rebuild_result.set(Some(
                        t(i18n.locale.get(), "metadata.game_library_rebuilt").to_string(),
                    ));
                    rebuilding.set(false);
                    break;
                }
            }
        }
    }
}

/// Displays real-time import progress.
#[component]
fn ImportProgressDisplay(progress: RwSignal<Option<server_fns::ImportProgress>>) -> impl IntoView {
    let i18n = use_i18n();

    view! {
        <div class="import-progress">
            {move || {
                let locale = i18n.locale.get();
                match progress.get() {
                    Some(p) => {
                        let state_text = match p.state {
                            ImportState::Downloading => t(locale, "metadata.downloading_file").to_string(),
                            ImportState::BuildingIndex => t(locale, "metadata.building_index").to_string(),
                            ImportState::Parsing => format!(
                                "{} ({} {}, {} {})",
                                t(locale, "metadata.parsing_xml"),
                                p.processed,
                                t(locale, "metadata.processed"),
                                p.matched,
                                t(locale, "metadata.matched"),
                            ),
                            ImportState::Complete => t(locale, "metadata.import_complete").to_string(),
                            ImportState::Failed => format!(
                                "{}: {}",
                                t(locale, "metadata.import_failed"),
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
    progress: RwSignal<Option<server_fns::ThumbnailProgress>>,
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
                                        "{} {}/{} {}",
                                        t(locale, "metadata.thumbnail_phase_indexing"),
                                        p.step_done,
                                        p.step_total,
                                        p.current_label,
                                    )
                                } else {
                                    t(locale, "metadata.thumbnail_phase_indexing").to_string()
                                }
                            }
                            ThumbnailPhase::Downloading => {
                                format!(
                                    "{} {} ({} {})",
                                    t(locale, "metadata.thumbnail_phase_downloading"),
                                    p.current_label,
                                    p.downloaded,
                                    t(locale, "metadata.thumbnail_downloaded"),
                                )
                            }
                            ThumbnailPhase::Complete => format!(
                                "{}: {} {}, {} {}",
                                t(locale, "metadata.thumbnail_complete"),
                                p.entries_indexed,
                                t(locale, "metadata.thumbnail_indexed"),
                                p.downloaded,
                                t(locale, "metadata.thumbnail_downloaded"),
                            ),
                            ThumbnailPhase::Failed => format!(
                                "{}: {}",
                                t(locale, "metadata.thumbnail_failed"),
                                p.error.as_deref().unwrap_or(""),
                            ),
                            ThumbnailPhase::Cancelled => format!(
                                "{}: {} {}",
                                t(locale, "metadata.thumbnail_cancelled"),
                                p.downloaded,
                                t(locale, "metadata.thumbnail_downloaded"),
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
///
/// Main actions (always visible):
/// - Rebuild Game Library
/// - Clear Downloaded Images
///
/// Advanced actions (collapsed by default):
/// - Clear Metadata
/// - Clear Thumbnail Index
#[component]
fn DataManagementSection(
    stats: Resource<Result<server_fns::MetadataStats, ServerFnError>>,
    coverage: Resource<Result<Vec<server_fns::SystemCoverage>, ServerFnError>>,
) -> impl IntoView {
    let i18n = use_i18n();
    let show_advanced = RwSignal::new(false);

    // Rebuild Game Library
    let confirming_rebuild = RwSignal::new(false);
    let rebuilding = RwSignal::new(false);
    let rebuild_result = RwSignal::new(Option::<String>::None);

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
        rebuilding.set(true);
        rebuild_result.set(None);
        confirming_rebuild.set(false);
        leptos::task::spawn_local(async move {
            match server_fns::rebuild_game_library().await {
                Ok(()) => {
                    // The rebuild runs in the background. Poll is_metadata_busy()
                    // every 2 seconds until it completes.
                    poll_rebuild_completion(rebuilding, rebuild_result, stats, coverage, i18n)
                        .await;
                }
                Err(e) => {
                    rebuild_result.set(Some(format!("Error: {e}")));
                    rebuilding.set(false);
                }
            }
        });
    });

    let on_clear_images = Callback::new(move |_: leptos::ev::MouseEvent| {
        clearing_images.set(true);
        images_result.set(None);
        leptos::task::spawn_local(async move {
            match server_fns::clear_images().await {
                Ok(()) => {
                    images_result.set(Some(
                        t(i18n.locale.get(), "metadata.cleared_images").to_string(),
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
        clearing_index.set(true);
        index_result.set(None);
        leptos::task::spawn_local(async move {
            match server_fns::clear_thumbnail_index().await {
                Ok(()) => {
                    index_result.set(Some(
                        t(i18n.locale.get(), "metadata.index_cleared").to_string(),
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
        clearing_metadata.set(true);
        metadata_result.set(None);
        leptos::task::spawn_local(async move {
            match server_fns::clear_metadata().await {
                Ok(()) => {
                    metadata_result.set(Some(
                        t(i18n.locale.get(), "metadata.metadata_cleared").to_string(),
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
            <h2 class="section-title">{move || t(i18n.locale.get(), "metadata.data_management")}</h2>
            <div class="manage-actions">
                // Main actions (always visible)
                <ClearActionCard
                    confirming=confirming_rebuild
                    clearing=rebuilding
                    result=rebuild_result
                    label_key="metadata.rebuild_game_library"
                    clearing_key="metadata.rebuilding_game_library"
                    confirm_key="metadata.confirm_rebuild_game_library"
                    on_confirm=on_rebuild
                />
                <ClearActionCard
                    confirming=confirming_images
                    clearing=clearing_images
                    result=images_result
                    label_key="metadata.clear_images"
                    clearing_key="metadata.clearing_images"
                    confirm_key="metadata.confirm_clear_images"
                    on_confirm=on_clear_images
                />
                <ClearActionCard
                    confirming=confirming_orphans
                    clearing=cleaning_orphans
                    result=orphans_result
                    label_key="metadata.cleanup_orphans"
                    clearing_key="metadata.cleaning_orphans"
                    confirm_key="metadata.confirm_cleanup_orphans"
                    on_confirm=on_cleanup_orphans
                />
            </div>

            // Advanced actions (collapsed by default)
            <div class="advanced-toggle">
                <button
                    class="advanced-toggle-btn"
                    on:click=move |_| show_advanced.update(|v| *v = !*v)
                >
                    <span class="advanced-toggle-icon">{move || if show_advanced.get() { "\u{25BC}" } else { "\u{25B6}" }}</span>
                    {move || t(i18n.locale.get(), "metadata.advanced_actions")}
                </button>
            </div>
            <Show when=move || show_advanced.get()>
                <div class="manage-actions">
                    <ClearActionCard
                        confirming=confirming_metadata
                        clearing=clearing_metadata
                        result=metadata_result
                        label_key="metadata.clear_metadata"
                        clearing_key="metadata.clearing_metadata"
                        confirm_key="metadata.confirm_clear_metadata"
                        on_confirm=on_clear_metadata
                    />
                    <ClearActionCard
                        confirming=confirming_index
                        clearing=clearing_index
                        result=index_result
                        label_key="metadata.clear_index"
                        clearing_key="metadata.clearing_index"
                        confirm_key="metadata.confirm_clear_index"
                        on_confirm=on_clear_index
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
    clearing: RwSignal<bool>,
    result: RwSignal<Option<String>>,
    #[prop(into)] label_key: &'static str,
    #[prop(into)] clearing_key: &'static str,
    #[prop(into)] confirm_key: &'static str,
    on_confirm: Callback<leptos::ev::MouseEvent>,
) -> impl IntoView {
    let i18n = use_i18n();

    view! {
        <div class="manage-action-card">
            <Show when=move || confirming.get()
                fallback=move || view! {
                    <button
                        class="game-action-btn game-action-delete"
                        on:click=move |_| confirming.set(true)
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
                        disabled=move || clearing.get()
                    >
                        {move || if clearing.get() {
                            t(i18n.locale.get(), clearing_key)
                        } else {
                            t(i18n.locale.get(), label_key)
                        }}
                    </button>
                    <button class="game-action-btn" on:click=move |_| confirming.set(false)>
                        {move || t(i18n.locale.get(), "games.cancel")}
                    </button>
                </div>
            </Show>
            <Show when=move || result.read().is_some()>
                <p class="manage-action-result">{move || result.get().unwrap_or_default()}</p>
            </Show>
        </div>
    }
}
