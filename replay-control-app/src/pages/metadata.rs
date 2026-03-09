use leptos::prelude::*;
use leptos_router::components::A;
use server_fn::ServerFnError;

use crate::i18n::{use_i18n, t};
use crate::pages::ErrorDisplay;
use crate::server_fns::{self, ImportState};
use crate::util::format_size;

#[component]
pub fn MetadataPage() -> impl IntoView {
    let i18n = use_i18n();
    let stats = Resource::new(|| (), |_| server_fns::get_metadata_stats());
    let coverage = Resource::new(|| (), |_| server_fns::get_system_coverage());

    // Import state
    let xml_path = RwSignal::new(String::new());
    let importing = RwSignal::new(false);
    let import_message = RwSignal::new(Option::<String>::None);
    let progress = RwSignal::new(Option::<server_fns::ImportProgress>::None);

    // Clear state
    let confirming_clear = RwSignal::new(false);
    let clearing = RwSignal::new(false);
    let clear_result = RwSignal::new(Option::<String>::None);

    // Check for in-progress import on page load (client-side only).
    Effect::new(move || {
        leptos::task::spawn_local(async move {
            if let Ok(Some(p)) = server_fns::get_import_progress().await {
                if p.state == ImportState::BuildingIndex || p.state == ImportState::Parsing {
                    progress.set(Some(p));
                    importing.set(true);
                    poll_progress(importing, progress, import_message, stats, coverage).await;
                }
            }
        });
    });

    let on_import = move |_| {
        let path = xml_path.get();
        if path.is_empty() {
            return;
        }
        importing.set(true);
        import_message.set(None);
        progress.set(None);

        leptos::task::spawn_local(async move {
            match server_fns::import_launchbox_metadata(path).await {
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

    let on_clear = move |_| {
        clearing.set(true);
        clear_result.set(None);
        leptos::task::spawn_local(async move {
            match server_fns::clear_metadata().await {
                Ok(()) => {
                    clear_result.set(Some("Metadata cleared".to_string()));
                    stats.refetch();
                    coverage.refetch();
                }
                Err(e) => {
                    clear_result.set(Some(format!("Error: {e}")));
                }
            }
            clearing.set(false);
            confirming_clear.set(false);
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

            // Status section
            <section class="section">
                <h2 class="section-title">{move || t(i18n.locale.get(), "metadata.status")}</h2>
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
            </section>

            // Import section
            <section class="section">
                <h2 class="section-title">{move || t(i18n.locale.get(), "metadata.import")}</h2>
                <p class="settings-hint">{move || t(i18n.locale.get(), "metadata.import_hint")}</p>
                <p class="settings-hint">{move || t(i18n.locale.get(), "metadata.auto_import_hint")}</p>
                <div class="metadata-import-form">
                    <input
                        type="text"
                        class="rename-input"
                        placeholder="/path/to/Metadata.xml"
                        bind:value=xml_path
                    />
                    <button
                        class="game-action-btn"
                        on:click=on_import
                        disabled=move || importing.get() || xml_path.read().is_empty()
                    >
                        {move || if importing.get() {
                            t(i18n.locale.get(), "metadata.importing")
                        } else {
                            t(i18n.locale.get(), "metadata.import_launchbox")
                        }}
                    </button>
                </div>

                // Progress display
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

            // Cache management section
            <section class="section">
                <h2 class="section-title">{move || t(i18n.locale.get(), "metadata.clear")}</h2>
                <Show when=move || confirming_clear.get()
                    fallback=move || view! {
                        <button
                            class="game-action-btn game-action-delete"
                            on:click=move |_| confirming_clear.set(true)
                        >
                            {move || t(i18n.locale.get(), "metadata.clear")}
                        </button>
                    }
                >
                    <p class="settings-hint">{move || t(i18n.locale.get(), "metadata.confirm_clear")}</p>
                    <div class="game-delete-confirm">
                        <button
                            class="game-action-btn game-action-delete-confirm"
                            on:click=on_clear
                            disabled=move || clearing.get()
                        >
                            {move || if clearing.get() {
                                t(i18n.locale.get(), "metadata.clearing")
                            } else {
                                t(i18n.locale.get(), "metadata.clear")
                            }}
                        </button>
                        <button class="game-action-btn" on:click=move |_| confirming_clear.set(false)>
                            {move || t(i18n.locale.get(), "games.cancel")}
                        </button>
                    </div>
                </Show>
                <Show when=move || clear_result.read().is_some()>
                    <p class="settings-saved">{move || clear_result.get().unwrap_or_default()}</p>
                </Show>
            </section>

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
    loop {
        // Sleep 1 second between polls.
        #[cfg(target_arch = "wasm32")]
        gloo_timers::future::TimeoutFuture::new(1_000).await;
        #[cfg(not(target_arch = "wasm32"))]
        break;

        match server_fns::get_import_progress().await {
            Ok(Some(p)) => {
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
            _ => break,
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
