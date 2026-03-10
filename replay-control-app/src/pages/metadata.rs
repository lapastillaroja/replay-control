use leptos::prelude::*;
use leptos_router::components::A;
use server_fn::ServerFnError;

use crate::i18n::{use_i18n, t};
use crate::pages::ErrorDisplay;
use crate::server_fns::{self, ImportState, ImageImportState};
use crate::util::format_size;

#[component]
pub fn MetadataPage() -> impl IntoView {
    let i18n = use_i18n();
    let stats = Resource::new(|| (), |_| server_fns::get_metadata_stats());
    let coverage = Resource::new(|| (), |_| server_fns::get_system_coverage());

    // Import state (downloads and auto-imports)
    let importing = RwSignal::new(false);
    let import_message = RwSignal::new(Option::<String>::None);
    let progress = RwSignal::new(Option::<server_fns::ImportProgress>::None);

    // Check for in-progress import on page load (client-side only).
    Effect::new(move || {
        leptos::task::spawn_local(async move {
            if let Ok(Some(p)) = server_fns::get_import_progress().await {
                if matches!(p.state, ImportState::Downloading | ImportState::BuildingIndex | ImportState::Parsing) {
                    progress.set(Some(p));
                    importing.set(true);
                    poll_progress(importing, progress, import_message, stats, coverage).await;
                }
            }
        });
    });

    let on_download = move |_| {
        if importing.get() { return; }
        importing.set(true);
        import_message.set(None);
        progress.set(None);
        leptos::task::spawn_local(async move {
            match server_fns::download_metadata().await {
                Ok(()) => {
                    poll_progress(importing, progress, import_message, stats, coverage).await;
                }
                Err(e) => {
                    import_message.set(Some(format!("Error: {e}")));
                    importing.set(false);
                }
            }
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

            // Descriptions & Ratings section
            <section class="section">
                <h2 class="section-title">{move || t(i18n.locale.get(), "metadata.descriptions")}</h2>
                <p class="settings-hint">{move || t(i18n.locale.get(), "metadata.descriptions_hint")}</p>
                <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }>
                    <Suspense fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), "common.loading")}</div> }>
                        {move || Suspend::new(async move {
                            let locale = i18n.locale.get();
                            let data = stats.await?;
                            Ok::<_, ServerFnError>(if data.total_entries == 0 {
                                view! {
                                    <p class="game-section-empty">{t(locale, "metadata.no_data")}</p>
                                }.into_any()
                            } else {
                                view! {
                                    <div class="info-grid">
                                        <div class="info-row">
                                            <span class="info-label">{t(locale, "metadata.total_entries")}</span>
                                            <span class="info-value">{data.total_entries.to_string()}</span>
                                        </div>
                                        <div class="info-row">
                                            <span class="info-label">{t(locale, "metadata.with_description")}</span>
                                            <span class="info-value">{data.with_description.to_string()}</span>
                                        </div>
                                        <div class="info-row">
                                            <span class="info-label">{t(locale, "metadata.with_rating")}</span>
                                            <span class="info-value">{data.with_rating.to_string()}</span>
                                        </div>
                                        <div class="info-row">
                                            <span class="info-label">{t(locale, "metadata.db_size")}</span>
                                            <span class="info-value">{format_size(data.db_size_bytes)}</span>
                                        </div>
                                    </div>
                                }.into_any()
                            })
                        })}
                    </Suspense>
                </ErrorBoundary>

                // Download/Update button
                <button
                    class="metadata-download-btn"
                    on:click=on_download
                    disabled=move || importing.get()
                >
                    {move || if importing.get() {
                        t(i18n.locale.get(), "metadata.downloading_metadata")
                    } else {
                        t(i18n.locale.get(), "metadata.download_metadata")
                    }}
                </button>

                // Import progress
                <Show when=move || progress.read().is_some()>
                    <ImportProgressDisplay progress />
                </Show>
                <Show when=move || import_message.read().is_some()>
                    <p class="settings-saved">{move || import_message.get().unwrap_or_default()}</p>
                </Show>
            </section>

            // Per-system coverage
            <section class="section">
                <h2 class="section-title">{move || t(i18n.locale.get(), "metadata.coverage")}</h2>
                <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }>
                    <Suspense fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), "common.loading")}</div> }>
                        {move || Suspend::new(async move {
                            let locale = i18n.locale.get();
                            let data = coverage.await?;
                            Ok::<_, ServerFnError>(if data.is_empty() || data.iter().all(|c| c.with_metadata == 0) {
                                view! {
                                    <p class="game-section-empty">{t(locale, "metadata.no_coverage")}</p>
                                }.into_any()
                            } else {
                                let rows = data.into_iter()
                                    .filter(|c| c.with_metadata > 0)
                                    .map(|c| {
                                        let pct = if c.total_games > 0 {
                                            (c.with_metadata as f64 / c.total_games as f64 * 100.0) as u32
                                        } else { 0 };
                                        view! {
                                            <div class="info-row">
                                                <span class="info-label">{c.display_name}</span>
                                                <span class="info-value">
                                                    {format!("{}/{} ({}%)", c.with_metadata, c.total_games, pct)}
                                                </span>
                                            </div>
                                        }
                                    })
                                    .collect::<Vec<_>>();
                                view! {
                                    <div class="info-grid">
                                        {rows}
                                    </div>
                                }.into_any()
                            })
                        })}
                    </Suspense>
                </ErrorBoundary>
            </section>

            // Images section
            <section class="section">
                <h2 class="section-title">{move || t(i18n.locale.get(), "metadata.images")}</h2>
                <p class="settings-hint">{move || t(i18n.locale.get(), "metadata.images_hint")}</p>
                <ImageSection />
            </section>

            // Data management section (clear images only)
            <ClearImagesSection />

            // Attribution section
            <section class="section">
                <h2 class="section-title">{move || t(i18n.locale.get(), "metadata.attribution")}</h2>
                <p class="settings-hint">{move || t(i18n.locale.get(), "metadata.attribution_text")}</p>
            </section>
        </div>
    }
}

/// Polls the server for import progress until complete or failed.
#[allow(unused_variables, unreachable_code)]
async fn poll_progress(
    importing: RwSignal<bool>,
    progress: RwSignal<Option<server_fns::ImportProgress>>,
    import_message: RwSignal<Option<String>>,
    stats: Resource<Result<server_fns::MetadataStats, ServerFnError>>,
    coverage: Resource<Result<Vec<server_fns::SystemCoverage>, ServerFnError>>,
) {
    let mut empty_polls = 0u32;
    loop {
        #[cfg(target_arch = "wasm32")]
        gloo_timers::future::TimeoutFuture::new(500).await;
        #[cfg(not(target_arch = "wasm32"))]
        break;

        match server_fns::get_import_progress().await {
            Ok(Some(p)) => {
                empty_polls = 0;
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
                    break;
                }
            }
            Ok(None) => {
                // Background task hasn't written progress yet — keep trying.
                empty_polls += 1;
                if empty_polls > 30 {
                    importing.set(false);
                    break;
                }
            }
            Err(_) => break,
        }
    }
}

/// Displays real-time import progress.
#[component]
fn ImportProgressDisplay(
    progress: RwSignal<Option<server_fns::ImportProgress>>,
) -> impl IntoView {
    let i18n = use_i18n();

    view! {
        <div class="import-progress">
            {move || {
                let locale = i18n.locale.get();
                let p = progress.get();
                match p {
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


/// Images section: shows per-system image coverage and download buttons.
#[component]
fn ImageSection() -> impl IntoView {
    let i18n = use_i18n();
    let image_coverage = Resource::new(|| (), |_| server_fns::get_image_coverage());
    let image_stats = Resource::new(|| (), |_| server_fns::get_image_stats());
    let img_importing = RwSignal::new(false);
    let img_progress = RwSignal::new(Option::<server_fns::ImageImportProgress>::None);
    let img_message = RwSignal::new(Option::<String>::None);

    let img_cancelling = RwSignal::new(false);

    // Check for in-progress image import on load.
    Effect::new(move || {
        leptos::task::spawn_local(async move {
            if let Ok(Some(p)) = server_fns::get_image_import_progress().await {
                if matches!(p.state, ImageImportState::Cloning | ImageImportState::Copying) {
                    img_progress.set(Some(p));
                    img_importing.set(true);
                    watch_image_progress(img_importing, img_progress, img_message, img_cancelling, image_coverage, image_stats);
                }
            }
        });
    });

    view! {
        // Image stats
        <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }>
            <Suspense fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), "common.loading")}</div> }>
                {move || Suspend::new(async move {
                    let locale = i18n.locale.get();
                    let (with_boxart, with_snap, media_size) = image_stats.await?;
                    Ok::<_, ServerFnError>(if with_boxart == 0 && with_snap == 0 {
                        view! {
                            <p class="game-section-empty">{t(locale, "metadata.no_images")}</p>
                        }.into_any()
                    } else {
                        view! {
                            <div class="info-grid">
                                <div class="info-row">
                                    <span class="info-label">{t(locale, "metadata.with_boxart")}</span>
                                    <span class="info-value">{with_boxart.to_string()}</span>
                                </div>
                                <div class="info-row">
                                    <span class="info-label">{t(locale, "metadata.with_snap")}</span>
                                    <span class="info-value">{with_snap.to_string()}</span>
                                </div>
                                <div class="info-row">
                                    <span class="info-label">{t(locale, "metadata.media_size")}</span>
                                    <span class="info-value">{format_size(media_size)}</span>
                                </div>
                            </div>
                        }.into_any()
                    })
                })}
            </Suspense>
        </ErrorBoundary>

        // Download All / Stop buttons
        {
            let on_download_all = move |_| {
                if img_importing.get() { return; }
                img_importing.set(true);
                img_cancelling.set(false);
                img_message.set(None);
                // Optimistic: show progress bar immediately
                img_progress.set(Some(server_fns::ImageImportProgress {
                    state: ImageImportState::Cloning,
                    system: String::new(),
                    system_display: String::new(),
                    processed: 0,
                    total: 0,
                    boxart_copied: 0,
                    snap_copied: 0,
                    elapsed_secs: 0,
                    error: None,
                    current_system: 0,
                    total_systems: 0,
                }));
                leptos::task::spawn_local(async move {
                    match server_fns::import_all_images().await {
                        Ok(()) => {
                            watch_image_progress(img_importing, img_progress, img_message, img_cancelling, image_coverage, image_stats);
                        }
                        Err(e) => {
                            img_message.set(Some(format!("Error: {e}")));
                            img_importing.set(false);
                        }
                    }
                });
            };
            let on_cancel = move |_| {
                img_cancelling.set(true);
                leptos::task::spawn_local(async move {
                    let _ = server_fns::cancel_image_import().await;
                });
            };
            view! {
                <div class="image-action-row">
                    <button
                        class="metadata-download-btn"
                        on:click=on_download_all
                        disabled=move || img_importing.get()
                    >
                        {move || if img_importing.get() {
                            t(i18n.locale.get(), "metadata.downloading_all")
                        } else {
                            t(i18n.locale.get(), "metadata.download_all")
                        }}
                    </button>
                    <Show when=move || img_importing.get()>
                        <button
                            class="form-btn form-btn-secondary"
                            on:click=on_cancel
                            disabled=move || img_cancelling.get()
                        >
                            {move || if img_cancelling.get() {
                                t(i18n.locale.get(), "metadata.cancelling")
                            } else {
                                t(i18n.locale.get(), "metadata.stop")
                            }}
                        </button>
                    </Show>
                </div>
            }
        }

        // Image import progress
        <Show when=move || img_progress.read().is_some()>
            <ImageProgressDisplay progress=img_progress />
        </Show>
        <Show when=move || img_message.read().is_some()>
            <p class="settings-saved">{move || img_message.get().unwrap_or_default()}</p>
        </Show>

        // Per-system image coverage with download buttons
        <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }>
            <Suspense fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), "common.loading")}</div> }>
                {move || Suspend::new(async move {
                    let locale = i18n.locale.get();
                    let data = image_coverage.await?;

                    let systems_with_repo: Vec<_> = data.into_iter().filter(|c| c.has_repo).collect();
                    Ok::<_, ServerFnError>(if systems_with_repo.is_empty() {
                        view! {
                            <p class="game-section-empty">{t(locale, "metadata.no_image_systems")}</p>
                        }.into_any()
                    } else {
                        let rows = systems_with_repo.into_iter().map(|c| {
                            let system = StoredValue::new(c.system.clone());
                            let has_images = c.with_boxart > 0 || c.with_snap > 0;
                            let display_name = StoredValue::new(c.display_name.clone());
                            let on_download = move |_| {
                                if img_importing.get() { return; }
                                img_importing.set(true);
                                img_message.set(None);
                                // Optimistic: show progress bar immediately
                                img_progress.set(Some(server_fns::ImageImportProgress {
                                    state: ImageImportState::Cloning,
                                    system: system.get_value(),
                                    system_display: display_name.get_value(),
                                    processed: 0,
                                    total: 0,
                                    boxart_copied: 0,
                                    snap_copied: 0,
                                    elapsed_secs: 0,
                                    error: None,
                                    current_system: 1,
                                    total_systems: 1,
                                }));
                                let sys = system.get_value();
                                leptos::task::spawn_local(async move {
                                    match server_fns::import_system_images(sys).await {
                                        Ok(()) => {
                                            watch_image_progress(img_importing, img_progress, img_message, img_cancelling, image_coverage, image_stats);
                                        }
                                        Err(e) => {
                                            img_message.set(Some(format!("Error: {e}")));
                                            img_importing.set(false);
                                        }
                                    }
                                });
                            };
                            view! {
                                <div class="info-row image-system-row">
                                    <span class="info-label">{c.display_name}</span>
                                    <span class="info-value">
                                        <Show when=move || has_images
                                            fallback=move || view! { <span class="image-count-none">{t(locale, "metadata.no_images_short")}</span> }
                                        >
                                            <span>{format!("{}/{}", c.with_boxart, c.total_games)}</span>
                                        </Show>
                                        <button
                                            class="game-action-btn image-download-btn"
                                            on:click=on_download
                                            disabled=move || img_importing.get()
                                        >
                                            {move || if has_images {
                                                t(locale, "metadata.update_images")
                                            } else {
                                                t(locale, "metadata.download_images")
                                            }}
                                        </button>
                                    </span>
                                </div>
                            }
                        }).collect::<Vec<_>>();
                        view! {
                            <div class="info-grid image-systems-grid">
                                {rows}
                            </div>
                        }.into_any()
                    })
                })}
            </Suspense>
        </ErrorBoundary>
    }
}

/// Watches image import progress via SSE, falling back to polling.
#[allow(unused_variables, unreachable_code)]
fn watch_image_progress(
    importing: RwSignal<bool>,
    progress: RwSignal<Option<server_fns::ImageImportProgress>>,
    message: RwSignal<Option<String>>,
    cancelling: RwSignal<bool>,
    coverage: Resource<Result<Vec<server_fns::ImageCoverage>, ServerFnError>>,
    stats: Resource<Result<(usize, usize, u64), ServerFnError>>,
) {
    #[cfg(not(target_arch = "wasm32"))]
    return;

    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::prelude::*;

        let es = match web_sys::EventSource::new("/sse/image-progress") {
            Ok(es) => es,
            Err(_) => return,
        };

        let es_clone = es.clone();
        let on_message = Closure::<dyn Fn(web_sys::MessageEvent)>::new(move |event: web_sys::MessageEvent| {
            let data = event.data().as_string().unwrap_or_default();
            if data == "null" || data.is_empty() {
                return;
            }
            let p: server_fns::ImageImportProgress = match serde_json::from_str(&data) {
                Ok(p) => p,
                Err(_) => return,
            };

            let is_multi = p.total_systems > 1;
            let is_last = p.current_system >= p.total_systems;
            let done = match p.state {
                ImageImportState::Failed | ImageImportState::Cancelled => true,
                ImageImportState::Complete => !is_multi || is_last,
                _ => false,
            };

            if done {
                cancelling.set(false);
                if p.state == ImageImportState::Complete {
                    if is_multi {
                        message.set(Some(format!(
                            "All {} systems done ({}s)",
                            p.total_systems, p.elapsed_secs,
                        )));
                    } else {
                        message.set(Some(format!(
                            "{}: {} boxart, {} snaps ({}s)",
                            p.system_display, p.boxart_copied, p.snap_copied, p.elapsed_secs,
                        )));
                    }
                } else if p.state == ImageImportState::Cancelled {
                    message.set(Some(format!(
                        "Cancelled after {}s ({} boxart, {} snaps imported)",
                        p.elapsed_secs, p.boxart_copied, p.snap_copied,
                    )));
                } else {
                    message.set(Some(format!(
                        "Failed: {}",
                        p.error.as_deref().unwrap_or("unknown error"),
                    )));
                }
                progress.set(Some(p));
                importing.set(false);
                coverage.refetch();
                stats.refetch();
                es_clone.close();
                return;
            }

            progress.set(Some(p));
        });

        es.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        on_message.forget(); // intentional: prevent drop while EventSource is alive
    }
}

/// Displays image import progress.
#[component]
fn ImageProgressDisplay(
    progress: RwSignal<Option<server_fns::ImageImportProgress>>,
) -> impl IntoView {
    let i18n = use_i18n();

    view! {
        <div class="import-progress">
            {move || {
                let locale = i18n.locale.get();
                match progress.get() {
                    Some(p) => {
                        let sys_prefix = if p.total_systems > 1 {
                            format!("[{}/{}] ", p.current_system, p.total_systems)
                        } else {
                            String::new()
                        };
                        let state_text = match p.state {
                            ImageImportState::Cloning => format!(
                                "{}{}: {}",
                                sys_prefix,
                                t(locale, "metadata.cloning_repo"),
                                p.system_display,
                            ),
                            ImageImportState::Copying => format!(
                                "{}{} {}: {}/{} ({}+{} {})",
                                sys_prefix,
                                p.system_display,
                                t(locale, "metadata.copying_images"),
                                p.processed,
                                p.total,
                                p.boxart_copied,
                                p.snap_copied,
                                t(locale, "metadata.images_found"),
                            ),
                            ImageImportState::Complete => format!(
                                "{}{}: {} boxart, {} snaps",
                                sys_prefix,
                                t(locale, "metadata.import_complete"),
                                p.boxart_copied,
                                p.snap_copied,
                            ),
                            ImageImportState::Failed => format!(
                                "{}{}: {}",
                                sys_prefix,
                                t(locale, "metadata.import_failed"),
                                p.error.as_deref().unwrap_or(""),
                            ),
                            ImageImportState::Cancelled => format!(
                                "{}{}: {} boxart, {} snaps",
                                sys_prefix,
                                t(locale, "metadata.import_cancelled"),
                                p.boxart_copied,
                                p.snap_copied,
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

/// Clear images section with confirmation.
#[component]
fn ClearImagesSection() -> impl IntoView {
    let i18n = use_i18n();
    let confirming = RwSignal::new(false);
    let clearing = RwSignal::new(false);
    let result = RwSignal::new(Option::<String>::None);

    let on_clear = move |_| {
        clearing.set(true);
        result.set(None);
        leptos::task::spawn_local(async move {
            match server_fns::clear_images().await {
                Ok(()) => {
                    result.set(Some(t(i18n.locale.get(), "metadata.cleared_images").to_string()));
                }
                Err(e) => {
                    result.set(Some(format!("Error: {e}")));
                }
            }
            clearing.set(false);
            confirming.set(false);
        });
    };

    view! {
        <section class="section">
            <h2 class="section-title">{move || t(i18n.locale.get(), "metadata.data_management")}</h2>
            <div class="manage-actions">
                <div class="manage-action-card">
                    <Show when=move || confirming.get()
                        fallback=move || view! {
                            <button
                                class="game-action-btn game-action-delete"
                                on:click=move |_| confirming.set(true)
                            >
                                {move || t(i18n.locale.get(), "metadata.clear_images")}
                            </button>
                        }
                    >
                        <p class="manage-action-hint">{move || t(i18n.locale.get(), "metadata.confirm_clear_images")}</p>
                        <div class="game-delete-confirm">
                            <button
                                class="game-action-btn game-action-delete-confirm"
                                on:click=on_clear
                                disabled=move || clearing.get()
                            >
                                {move || if clearing.get() {
                                    t(i18n.locale.get(), "metadata.clearing_images")
                                } else {
                                    t(i18n.locale.get(), "metadata.clear_images")
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
            </div>
        </section>
    }
}
