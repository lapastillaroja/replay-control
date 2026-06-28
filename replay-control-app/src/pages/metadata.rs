use leptos::prelude::*;
use leptos_router::components::A;
use server_fn::ServerFnError;

use crate::components::stat_card::StatCard;
use crate::i18n::{Key, t, use_i18n};
use crate::server_fns::{
    self, Activity, CountBucket, DriverStatusCounts, ImportState, LibrarySummary,
    MetadataLibraryOverview, MetadataPageSnapshot, PlaytimeAvailability, RebuildProgress,
    SystemCoverage, SystemStatsRefreshState, ThumbnailPhase,
};
use crate::util::{format_elapsed_short, format_number, format_size, format_year_range, pct};
use replay_control_core::systems::{
    system_core_supports_retroachievements, system_has_retroachievements,
};
use std::collections::HashMap;

type SnapshotRes = Resource<Result<MetadataPageSnapshot, ServerFnError>>;
type OverviewRes = Resource<Result<MetadataLibraryOverview, ServerFnError>>;

/// Client-only play time state, provided via context to the summary card and
/// the per-system accordion rows. `Disabled` (tracking off on the TV) and the
/// unavailable cases both collapse to `Placeholder`; `Loading` is the transient
/// pre-fetch state.
#[derive(Clone, PartialEq)]
enum PlaytimeUi {
    Loading,
    Placeholder,
    Ready {
        all_seconds: u64,
        by_system: HashMap<String, u64>,
    },
}

/// Render one play time figure from the shared state: an ellipsis while
/// loading, the i18n placeholder when unavailable/disabled, else the formatted
/// duration. `pick` selects the relevant seconds from the loaded totals (the
/// grand total for the summary card, a per-system lookup for an accordion row).
fn playtime_value(
    ui: &PlaytimeUi,
    pick: impl FnOnce(u64, &HashMap<String, u64>) -> u64,
    locale: crate::i18n::Locale,
) -> String {
    match ui {
        PlaytimeUi::Loading => "\u{2026}".to_string(),
        PlaytimeUi::Placeholder => t(locale, Key::PlaytimeUnavailable).to_string(),
        PlaytimeUi::Ready {
            all_seconds,
            by_system,
        } => format_elapsed_short(pick(*all_seconds, by_system)),
    }
}

fn format_rebuild_progress(locale: crate::i18n::Locale, p: &RebuildProgress) -> Option<String> {
    crate::components::metadata_banner::format_rebuild_progress_label(locale, p)
}

#[component]
pub fn MetadataPage() -> impl IntoView {
    let i18n = use_i18n();
    // Keep the library overview separate from slower media/data-source stats.
    // Both read durable DB state, but the overview only touches
    // game_library_system_stats so it can render during rescan.
    let overview: OverviewRes =
        Resource::new(|| (), |_| server_fns::get_metadata_library_overview());
    let snapshot: SnapshotRes = Resource::new(|| (), |_| server_fns::get_metadata_page_snapshot());

    // Play time is fetched client-side only — a `LocalResource` never runs
    // during SSR, so a slow or unimplemented `get_playtime` endpoint can't
    // block or delay the page. One fetch per page load; no polling. The summary
    // card and the per-system accordion rows read the result via context.
    let playtime = LocalResource::new(server_fns::get_library_playtime);
    let playtime_ui = Memo::new(move |_| {
        let Some(result) = playtime.get() else {
            return PlaytimeUi::Loading;
        };
        match result.take() {
            Ok(summary) if summary.availability == PlaytimeAvailability::Tracked => {
                PlaytimeUi::Ready {
                    all_seconds: summary.all_seconds,
                    by_system: summary
                        .systems
                        .into_iter()
                        .map(|s| (s.system, s.seconds))
                        .collect(),
                }
            }
            _ => PlaytimeUi::Placeholder,
        }
    });
    provide_context(playtime_ui);

    // App-level activity signal (populated by SseEventsListener at the App
    // root). Shared with banners, the setup checklist, and other consumers
    // — activity from another tab/process is reflected here too.
    let activity = use_context::<RwSignal<Activity>>().expect("Activity context");

    // Per-operation result messages — prevents a rebuild message from showing in LaunchBox/Thumbnail sections.
    let import_result = RwSignal::new(None::<String>);
    let thumb_result = RwSignal::new(None::<String>);
    let rebuild_result = RwSignal::new(None::<String>);

    // Progress signals derived from activity for display components.
    let import_progress = Memo::new(move |_| match activity.get() {
        Activity::Import { progress } => Some(progress),
        _ => None,
    });
    let refresh_progress = Memo::new(move |_| match activity.get() {
        Activity::RefreshExternalMetadata { progress } => Some(progress),
        _ => None,
    });
    let thumb_progress = Memo::new(move |_| match activity.get() {
        Activity::ThumbnailUpdate { progress, .. } => Some(progress),
        _ => None,
    });

    // Derived helpers.
    let is_busy = Memo::new(move |_| activity.with(|a| !matches!(a, Activity::Idle)));
    let is_importing = Memo::new(move |_| {
        activity.with(|a| {
            matches!(
                a,
                Activity::Import { .. } | Activity::RefreshExternalMetadata { .. }
            )
        })
    });
    let is_thumb_updating =
        Memo::new(move |_| activity.with(|a| matches!(a, Activity::ThumbnailUpdate { .. })));
    let can_cancel =
        Memo::new(move |_| activity.with(|a| matches!(a, Activity::ThumbnailUpdate { .. })));

    // Thumbnail cancel UI state (local, not derived from server).
    let thumb_cancelling = RwSignal::new(false);

    // When the activity transitions back to Idle, dispatch the result message
    // and refetch the relevant resources based on what was running. The last
    // non-Idle activity is captured in a StoredValue so its terminal_message
    // and kind survive past the Idle transition.
    let last_active: StoredValue<Option<Activity>> = StoredValue::new(None);
    Effect::new(move |_| {
        let act = activity.get();
        if matches!(act, Activity::Idle) {
            let prev = last_active.get_value();
            last_active.set_value(None);
            let Some(prev) = prev else {
                return;
            };
            dispatch_terminal(
                prev,
                import_result,
                thumb_result,
                rebuild_result,
                thumb_cancelling,
                overview,
                snapshot,
            );
        } else {
            last_active.set_value(Some(act));
        }
    });

    let on_download = move |_| {
        if is_busy.get() {
            return;
        }
        import_result.set(None);
        leptos::task::spawn_local(async move {
            if let Err(e) = server_fns::download_metadata().await {
                import_result.set(Some(format!("Error: {e}")));
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
            if let Err(e) = server_fns::update_thumbnails().await {
                thumb_result.set(Some(format!("Error: {e}")));
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

            // ── Library Summary Cards ────────────────────────────────
            <section class="section">
                <Transition fallback=move || view! { <SummaryCardsSkeleton /> }>
                    {move || Suspend::new(async move {
                        let locale = i18n.locale.get();
                        let overview = overview.await?;
                        let s = overview.library_summary;
                        let storage_kind = overview.storage_kind;
                        Ok::<_, ServerFnError>(if s.total_games == 0 {
                            // Empty library: still show the storage-type card —
                            // it's infrastructure info, not derived from games.
                            view! { <StorageOnlyCard storage_kind locale /> }.into_any()
                        } else {
                            view! { <SummaryCards summary=s storage_kind locale /> }.into_any()
                        })
                    })}
                </Transition>
            </section>

            <SystemOverviewSection overview />

            // ── Data Sources ──────────────────────────────────────────
            <section class="section">
                <h2 class="section-title">{move || t(i18n.locale.get(), Key::MetadataDataSources)}</h2>

                // Built-in data info block
                <Transition fallback=move || view! { <MetadataCardSkeleton /> }>
                    {move || Suspend::new(async move {
                        let locale = i18n.locale.get();
                        let snap = snapshot.await?;
                        let bs = snap.builtin_stats;
                        Ok::<_, ServerFnError>(view! {
                            <div class="data-source-card builtin-info">
                                <div class="data-source-header">
                                    <span class="data-source-name">{t(locale, Key::MetadataBuiltin)}</span>
                                </div>
                                <p class="data-source-summary">
                                    {format!(
                                        "{} {} {} — {} {} {} {} — {} {} {} {} — {} {} — {} {}",
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
                                        format_number(bs.manual_resource_entries),
                                        t(locale, Key::MetadataBuiltinManualLinks),
                                        format_number(bs.shmups_wiki_resource_entries),
                                        t(locale, Key::MetadataBuiltinGuideLinks),
                                    )}
                                </p>
                                <p class="settings-hint">{t(locale, Key::MetadataBuiltinHint)}</p>
                            </div>
                        })
                    })}
                </Transition>

                // Descriptions & Ratings
                <div class="data-source-card">
                    <div class="data-source-header">
                        <span class="data-source-name">{move || t(i18n.locale.get(), Key::MetadataDescriptionsRatings)}</span>
                    </div>
                    <Transition fallback=move || view! { <MetadataLineSkeleton /> }>
                        {move || Suspend::new(async move {
                            let locale = i18n.locale.get();
                            let snap = snapshot.await?;
                            let data = snap.stats;
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
                    </Transition>
                    <div class="data-source-actions">
                        <button
                            class="form-btn"
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
                    <Show when=move || refresh_progress.get().is_some()>
                        <RefreshMetadataProgressDisplay progress=refresh_progress />
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
                    <Transition fallback=move || view! { <MetadataLineSkeleton /> }>
                        {move || Suspend::new(async move {
                            let locale = i18n.locale.get();
                            let snap = snapshot.await?;
                            let ds = snap.data_source;
                            let image_stats = snap.image_stats;

                            Ok::<_, ServerFnError>(if ds.entry_count == 0 && image_stats.total_files == 0 {
                                view! {
                                    <p class="data-source-summary dim">{t(locale, Key::MetadataNoData)}</p>
                                }.into_any()
                            } else {
                                let images_line = if image_stats.total_files > 0 {
                                    format!(
                                        "{} {} · {} {}, {} {}, {} {} — {} {}",
                                        format_number(image_stats.total_files),
                                        t(locale, Key::MetadataSummaryDownloadedArt).to_lowercase(),
                                        format_number(image_stats.boxart_files),
                                        t(locale, Key::MetadataThumbnailSummary),
                                        format_number(image_stats.snap_files),
                                        t(locale, Key::MetadataThumbnailSnaps),
                                        format_number(image_stats.title_files),
                                        t(locale, Key::MetadataRowTitleScreens).to_lowercase(),
                                        format_size(image_stats.total_size_bytes),
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
                    </Transition>
                    <div class="data-source-actions">
                        <button
                            class="form-btn"
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
            <DataManagementSection overview snapshot activity result_message=rebuild_result is_busy />

            // ── Attribution ───────────────────────────────────────────
            <section class="section">
                <h2 class="section-title">{move || t(i18n.locale.get(), Key::MetadataAttribution)}</h2>
                <p class="settings-hint">{move || t(i18n.locale.get(), Key::MetadataAttributionText)}</p>
            </section>
        </div>
    }
}

/// React to a non-Idle → Idle transition: surface the activity's terminal
/// message in the matching result signal, refetch the resources whose values
/// the operation could have changed, and clear thumbnail-cancel UI state if
/// it was a thumbnail update.
#[allow(clippy::too_many_arguments)]
fn dispatch_terminal(
    prev: Activity,
    import_result: RwSignal<Option<String>>,
    thumb_result: RwSignal<Option<String>>,
    rebuild_result: RwSignal<Option<String>>,
    thumb_cancelling: RwSignal<bool>,
    overview: OverviewRes,
    snapshot: SnapshotRes,
) {
    let target = match &prev {
        Activity::Import { .. } | Activity::RefreshExternalMetadata { .. } => Some(import_result),
        Activity::ThumbnailUpdate { .. } => {
            thumb_cancelling.set(false);
            Some(thumb_result)
        }
        Activity::Rebuild { .. } | Activity::Identity { .. } => Some(rebuild_result),
        _ => None,
    };

    if let Some(target) = target {
        let msg = prev.terminal_message();
        if !msg.is_empty() {
            target.set(Some(msg));
            #[cfg(target_arch = "wasm32")]
            gloo_timers::callback::Timeout::new(5_000, move || target.set(None)).forget();
        }
    }

    overview.refetch();
    snapshot.refetch();
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

/// Displays LaunchBox metadata refresh progress under the Update button.
#[component]
fn RefreshMetadataProgressDisplay(
    progress: Memo<Option<server_fns::RefreshMetadataProgress>>,
) -> impl IntoView {
    use server_fns::RefreshMetadataPhase;
    view! {
        <div class="import-progress">
            {move || {
                let Some(p) = progress.get() else {
                    return view! { <span></span> }.into_any();
                };
                let state_text = match p.phase {
                    RefreshMetadataPhase::Checking => "Checking for updates...".to_string(),
                    RefreshMetadataPhase::Downloading => match (p.downloaded_bytes, p.total_bytes) {
                        (0, _) => "Downloading...".to_string(),
                        (bytes, Some(total)) => format!(
                            "Downloading {} / {}",
                            format_size(bytes),
                            format_size(total),
                        ),
                        (bytes, None) => format!("Downloading {}", format_size(bytes)),
                    },
                    RefreshMetadataPhase::Parsing => {
                        if p.source_entries > 0 {
                            format!("Parsing ({} entries)...", p.source_entries)
                        } else {
                            "Parsing...".to_string()
                        }
                    }
                    RefreshMetadataPhase::Enriching => "Re-enriching library...".to_string(),
                    RefreshMetadataPhase::Failed => format!(
                        "Failed: {}",
                        p.error.as_deref().unwrap_or("unknown error"),
                    ),
                    RefreshMetadataPhase::Complete | RefreshMetadataPhase::UpToDate => {
                        return view! { <span></span> }.into_any();
                    }
                };
                let elapsed = format!("{}s", p.elapsed_secs);
                view! {
                    <div class="import-progress-bar">
                        <span class="import-progress-text">{state_text}</span>
                        <span class="import-progress-time">{elapsed}</span>
                    </div>
                }.into_any()
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
                                if p.current_label.is_empty() {
                                    t(locale, Key::MetadataThumbnailPhaseDownloading).to_string()
                                } else {
                                    p.current_label.clone()
                                }
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
    overview: OverviewRes,
    snapshot: SnapshotRes,
    activity: RwSignal<Activity>,
    result_message: RwSignal<Option<String>>,
    is_busy: Memo<bool>,
) -> impl IntoView {
    let i18n = use_i18n();
    let controls_hydrated = RwSignal::new(false);
    let show_advanced = RwSignal::new(false);

    Effect::new(move |_| {
        controls_hydrated.set(true);
    });

    let rebuilding = Memo::new(
        move |_| matches!(activity.get(), Activity::Rebuild { progress } if !progress.is_rescan),
    );
    let rescanning = Memo::new(
        move |_| matches!(activity.get(), Activity::Rebuild { progress } if progress.is_rescan),
    );

    // Each card gets its own display memo so that during a rescan the
    // (advanced-section) Rebuild card stays blank, and vice versa.
    let rebuild_display = Memo::new(move |_| match activity.get() {
        Activity::Rebuild { progress } if !progress.is_rescan => {
            format_rebuild_progress(i18n.locale.get(), &progress)
        }
        _ => None,
    });
    let rescan_display = Memo::new(move |_| match activity.get() {
        Activity::Rebuild { progress } if progress.is_rescan => {
            format_rebuild_progress(i18n.locale.get(), &progress)
        }
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
            if let Err(e) = server_fns::rebuild_game_library().await {
                result_message.set(Some(format!("Error: {e}")));
            }
        });
    });

    let on_rescan = Callback::new(move |_: leptos::ev::MouseEvent| {
        if is_busy.get() {
            return;
        }
        result_message.set(None);
        leptos::task::spawn_local(async move {
            if let Err(e) = server_fns::rescan_game_library().await {
                result_message.set(Some(format!("Error: {e}")));
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
                    let locale = i18n.locale.get_untracked();
                    images_result.set(Some(t(locale, Key::MetadataClearedImages).to_string()));
                    overview.refetch();
                    snapshot.refetch();
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
                    overview.refetch();
                    snapshot.refetch();
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
                        t(i18n.locale.get_untracked(), Key::MetadataIndexCleared).to_string(),
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
                        t(i18n.locale.get_untracked(), Key::MetadataMetadataCleared).to_string(),
                    ));
                    snapshot.refetch();
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
            <div class=move || {
                if controls_hydrated.get() {
                    "manage-actions is-hydrated"
                } else {
                    "manage-actions"
                }
            }>
                <RescanActionCard
                    rescanning=rescanning
                    result=result_message
                    label_key=Key::MetadataRescanGameLibrary
                    rescanning_key=Key::MetadataRescanningGameLibrary
                    hint_key=Key::MetadataRescanGameLibraryHint
                    on_click=on_rescan
                    disabled=is_busy
                    progress_text=rescan_display
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
                <div class="manage-actions manage-actions-grid">
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
                    <ExportCoverageCard />
                </div>
            </Show>
        </section>
    }
}

/// Export the per-ROM metadata coverage as a CSV — the entire library by
/// default, or a single system. A native GET form so the download works without
/// JS/hydration; the `system` select serializes to `?system=<folder>` (empty =
/// all systems). The system list is fetched lazily; "All systems" always works
/// even before it resolves.
#[component]
fn ExportCoverageCard() -> impl IntoView {
    let i18n = use_i18n();
    let systems = Resource::new(
        || (),
        |_| async move { server_fns::get_systems().await.unwrap_or_default() },
    );

    let all_systems_option = move || {
        view! {
            <option value="" selected=true>
                {move || t(i18n.locale.get(), Key::MetadataExportCsvAllSystems)}
            </option>
        }
    };

    view! {
        <div class="manage-action-card export-coverage-card">
            <span class="export-coverage-label">
                {move || t(i18n.locale.get(), Key::MetadataExportCsv)}
            </span>
            <form class="export-coverage-form" action="/api/export/library.csv" method="get">
                <Suspense fallback=move || {
                    view! {
                        <select name="system" class="form-input export-coverage-select">
                            {all_systems_option()}
                        </select>
                    }
                }>
                    {move || Suspend::new(async move {
                        let mut list = systems.await;
                        list.retain(|s| s.game_count > 0);
                        list.sort_by(|a, b| a.display_name.cmp(&b.display_name));
                        view! {
                            <select name="system" class="form-input export-coverage-select">
                                {all_systems_option()}
                                {list
                                    .into_iter()
                                    .map(|s| view! { <option value=s.folder_name>{s.display_name}</option> })
                                    .collect_view()}
                            </select>
                        }
                    })}
                </Suspense>
                <button type="submit" class="form-btn">
                    {move || t(i18n.locale.get(), Key::MetadataExportCsvDownload)}
                </button>
            </form>
            <p class="manage-action-hint">{move || t(i18n.locale.get(), Key::MetadataExportCsvHint)}</p>
        </div>
    }
}

/// Card for a non-destructive, single-click action like rescan. Always shows
/// the hint underneath so the user understands what they're triggering — no
/// two-step confirm because the operation can't lose data.
#[component]
fn RescanActionCard(
    #[prop(into)] rescanning: Signal<bool>,
    result: RwSignal<Option<String>>,
    label_key: Key,
    rescanning_key: Key,
    hint_key: Key,
    on_click: Callback<leptos::ev::MouseEvent>,
    #[prop(optional)] disabled: Option<Memo<bool>>,
    #[prop(optional)] progress_text: Option<Memo<Option<String>>>,
) -> impl IntoView {
    let i18n = use_i18n();
    let externally_disabled = move || disabled.is_some_and(|d| d.get());

    view! {
        <div class="manage-action-card">
            <button
                class="form-btn"
                on:click=move |ev| on_click.run(ev)
                disabled=move || rescanning.get() || externally_disabled()
            >
                {move || if rescanning.get() {
                    t(i18n.locale.get(), rescanning_key)
                } else {
                    t(i18n.locale.get(), label_key)
                }}
            </button>
            <p class="manage-action-hint">{move || t(i18n.locale.get(), hint_key)}</p>
            {move || progress_text.and_then(|pt| pt.get()).map(|text| view! {
                <p class="manage-action-progress">{text}</p>
            })}
            <Show when=move || result.read().is_some()>
                <p class="manage-action-result">{move || result.get().unwrap_or_default()}</p>
            </Show>
        </div>
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
                        class="form-btn form-btn-secondary"
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
                        class="form-btn form-btn-danger"
                        on:click=move |ev| on_confirm.run(ev)
                        disabled=move || clearing.get() || externally_disabled()
                    >
                        {move || if clearing.get() {
                            t(i18n.locale.get(), clearing_key)
                        } else {
                            t(i18n.locale.get(), label_key)
                        }}
                    </button>
                    <button class="form-btn form-btn-secondary" on:click=move |_| confirming.set(false)>
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

/// User-facing label for a `StorageKind` tag (`"sd"` / `"usb"` / `"nvme"` /
/// `"nfs"`). Returns `"--"` for empty input.
fn storage_label(kind: &str) -> String {
    match kind {
        "sd" => "SD".to_string(),
        "usb" => "USB".to_string(),
        "nvme" => "NVMe".to_string(),
        "nfs" => "NFS".to_string(),
        other if !other.is_empty() => other.to_uppercase(),
        _ => "--".to_string(),
    }
}

/// Single stat card showing only the storage type. Rendered when the library
/// is empty so users still see infrastructure info.
#[component]
fn StorageOnlyCard(storage_kind: String, locale: crate::i18n::Locale) -> impl IntoView {
    view! {
        <div class="stats-grid">
            <StatCard compact=true
                value=storage_label(&storage_kind)
                label=t(locale, Key::MetadataSummaryStorage) />
        </div>
    }
}

/// Renders the library summary stat cards.
#[component]
fn SummaryCards(
    summary: LibrarySummary,
    storage_kind: String,
    locale: crate::i18n::Locale,
) -> impl IntoView {
    let s = summary;

    // Enrichment: weighted average of core metadata coverage fields.
    let enrichment_value = if s.total_games > 0 {
        let avg = (pct(s.with_genre, s.total_games)
            + pct(s.with_developer, s.total_games)
            + pct(s.with_publisher, s.total_games)
            + pct(s.with_release_date, s.total_games)
            + pct(s.with_rating, s.total_games)
            + pct(s.with_manual, s.total_games)
            + pct(s.with_box_art, s.total_games))
            / 7;
        format!("{avg}%")
    } else {
        "--".to_string()
    };

    let year_value = format_year_range(s.min_year, s.max_year).unwrap_or_else(|| "--".to_string());

    let storage_label = storage_label(&storage_kind);

    view! {
        <div class="stats-grid">
            <StatCard compact=true
                value=format_number(s.total_games)
                label=t(locale, Key::MetadataSummaryTotalGames) />
            <StatCard compact=true
                value=enrichment_value
                label=t(locale, Key::MetadataSummaryEnrichment) />
            <StatCard compact=true
                value=format_number(s.system_count)
                label=t(locale, Key::MetadataSummarySystems) />
            <StatCard compact=true
                value=format_number(s.coop_games)
                label=t(locale, Key::MetadataSummaryCoOp) />
            <StatCard compact=true
                value=year_value
                label=t(locale, Key::MetadataSummaryYearSpan) />
            <StatCard compact=true
                value=format_size(s.total_size_bytes)
                label=t(locale, Key::MetadataSummaryLibrarySize) />
            <StatCard compact=true
                value=format_number(s.downloaded_thumbnail_files)
                label=t(locale, Key::MetadataSummaryDownloadedArt) />
            <StatCard compact=true
                value=storage_label
                label=t(locale, Key::MetadataSummaryStorage) />
            <PlaytimeStatCard />
        </div>
    }
}

/// Total library play time card. Reads the client-only [`PlaytimeUi`] context,
/// so it shows the placeholder on standalone builds and on firmware without the
/// `get_playtime` endpoint, and the formatted total once tracking data loads.
#[component]
fn PlaytimeStatCard() -> impl IntoView {
    let i18n = use_i18n();
    let playtime = use_context::<Memo<PlaytimeUi>>().expect("PlaytimeUi context");
    let value = move || playtime.with(|ui| playtime_value(ui, |all, _| all, i18n.locale.get()));
    view! {
        <div class="stat-card compact">
            <div class="stat-value">{value}</div>
            <div class="stat-label">
                {move || t(i18n.locale.get(), Key::MetadataSummaryPlaytime)}
            </div>
        </div>
    }
}

// ── System overview accordion ────────────────────────────────────────────

#[component]
fn SystemOverviewSection(overview: OverviewRes) -> impl IntoView {
    let i18n = use_i18n();
    let expand_all = RwSignal::new(false);
    let toggle_all = move |_: leptos::ev::MouseEvent| expand_all.update(|v| *v = !*v);

    view! {
        <section class="section">
            <div class="system-overview-header">
                <h2 class="section-title" style="margin:0">
                    {move || t(i18n.locale.get(), Key::MetadataSystemOverview)}
                </h2>
                <button class="system-overview-toggle-all" on:click=toggle_all>
                    {move || if expand_all.get() {
                        t(i18n.locale.get(), Key::MetadataCollapseAll)
                    } else {
                        t(i18n.locale.get(), Key::MetadataExpandAll)
                    }}
                </button>
            </div>
            <Transition fallback=move || view! { <AccordionSkeleton /> }>
                {move || Suspend::new(async move {
                    let overview = overview.await?;
                    let data = overview.coverage;
                    let rows = data
                        .into_iter()
                        .filter(|c| c.total_games > 0)
                        .map(|c| view! {
                            <SystemAccordionRow coverage=c expand_all />
                        })
                        .collect::<Vec<_>>();
                    Ok::<_, ServerFnError>(view! {
                        <div class="system-accordion-list">{rows}</div>
                    })
                })}
            </Transition>
        </section>
    }
}

#[component]
fn SystemAccordionRow(coverage: SystemCoverage, expand_all: RwSignal<bool>) -> impl IntoView {
    let cov = StoredValue::new(coverage);
    let expanded = RwSignal::new(false);

    Effect::new(move |prev: Option<bool>| {
        let v = expand_all.get();
        if prev.is_some() && prev != Some(v) {
            expanded.set(v);
        }
        v
    });

    let toggle = move |_| expanded.update(|e| *e = !*e);

    view! {
        <div
            class="system-accordion-row"
            class:expanded=move || expanded.get()
            on:click=toggle
        >
            <SystemRowHeader cov />
            <Show when=move || expanded.get()>
                <SystemRowDetails cov />
            </Show>
        </div>
    }
}

#[component]
fn SystemRowHeader(cov: StoredValue<SystemCoverage>) -> impl IntoView {
    let i18n = use_i18n();

    let (display_name, size_bytes, overall, games_text) = cov.with_value(|c| {
        let g = pct(c.with_genre, c.total_games);
        let d = pct(c.with_developer, c.total_games);
        let p = pct(c.with_publisher, c.total_games);
        let date = pct(c.with_release_date, c.total_games);
        let r = pct(c.with_rating, c.total_games);
        let desc = pct(c.with_description, c.total_games);
        let art = pct(c.with_thumbnail, c.total_games);
        let overall = (g + d + p + date + r + desc + art) / 7;
        (
            c.display_name.clone(),
            c.size_bytes,
            overall,
            format_number(c.total_games),
        )
    });

    let width = format!("width:{overall}%");
    let size_text = format_size(size_bytes);

    view! {
        <>
            <div class="system-row-header">
                <span class="system-row-name">{display_name}</span>
                <span class="system-row-chevron" aria-hidden="true">"▸"</span>
            </div>
            <div class="system-row-summary">
                {move || {
                    let locale = i18n.locale.get();
                    format!(
                        "{} {} \u{00B7} {} \u{00B7} {}% {}",
                        games_text,
                        t(locale, Key::StatsGames).to_lowercase(),
                        size_text,
                        overall,
                        t(locale, Key::MetadataSystemCoverage),
                    )
                }}
            </div>
            <div class="system-row-overall-bar">
                <div class="system-row-overall-bar-fill" style=width></div>
            </div>
        </>
    }
}

#[component]
fn SystemRowDetails(cov: StoredValue<SystemCoverage>) -> impl IntoView {
    let i18n = use_i18n();

    // Per-system play time, from the client-only context. Placeholder until the
    // fetch resolves, or when tracking is off / unavailable.
    let playtime = use_context::<Memo<PlaytimeUi>>().expect("PlaytimeUi context");
    let system_key = StoredValue::new(cov.with_value(|c| c.system.clone()));
    let playtime_line = move || {
        let locale = i18n.locale.get();
        let value = playtime.with(|ui| {
            playtime_value(
                ui,
                |_, by_system| {
                    system_key.with_value(|key| by_system.get(key).copied().unwrap_or(0))
                },
                locale,
            )
        });
        format!("{} \u{00B7} {}", t(locale, Key::MetadataRowPlaytime), value)
    };

    view! {
        <div class="system-row-details" on:click=|ev| ev.stop_propagation()>
            <CoverageBarRow cov field=CoverageField::Genre />
            <CoverageBarRow cov field=CoverageField::Developer />
            <CoverageBarRow cov field=CoverageField::Publisher />
            <CoverageBarRow cov field=CoverageField::ReleaseDate />
            <CoverageBarRow cov field=CoverageField::Rating />
            <CoverageBarRow cov field=CoverageField::Description />
            <CoverageBarRow cov field=CoverageField::BoxArt />
            <CoverageBarRow cov field=CoverageField::Manuals />
            <CoverageBarRow cov field=CoverageField::Videos />
            // Verified identity — CRC match or runtime RA hash match.
            <Show when=move || cov.with_value(|c| c.verified_count > 0) fallback=|| ()>
                <CoverageBarRow cov field=CoverageField::Verified />
            </Show>
            // RetroAchievements — shown for every RA-supported system (even at
            // 0%, e.g. discs pre-resolution). Systems RA doesn't cover get a
            // footer note instead (see footer_row_view), never a bar.
            <Show
                when=move || cov.with_value(|c| system_has_retroachievements(&c.system))
                fallback=|| ()
            >
                <CoverageBarRow cov field=CoverageField::RaId />
            </Show>

            <div class="composition-row">
                {move || composition_text(cov, i18n.locale.get())}
            </div>

            <div class="footer-row">{playtime_line}</div>

            {move || media_row_view(cov, i18n.locale.get())}

            {move || distribution_rows_view(cov, i18n.locale.get())}

            {move || driver_row_view(cov, i18n.locale.get())}

            {move || footer_row_view(cov, i18n.locale.get())}
        </div>
    }
}

#[derive(Copy, Clone)]
enum CoverageField {
    Genre,
    Developer,
    Publisher,
    ReleaseDate,
    Rating,
    Description,
    BoxArt,
    Manuals,
    Videos,
    Verified,
    RaId,
}

#[component]
fn CoverageBarRow(cov: StoredValue<SystemCoverage>, field: CoverageField) -> impl IntoView {
    let i18n = use_i18n();
    let (count, total, label_key) = cov.with_value(|c| match field {
        CoverageField::Genre => (c.with_genre, c.total_games, Key::MetadataRowGenre),
        CoverageField::Developer => (c.with_developer, c.total_games, Key::MetadataRowDeveloper),
        CoverageField::Publisher => (c.with_publisher, c.total_games, Key::MetadataRowPublisher),
        CoverageField::ReleaseDate => (
            c.with_release_date,
            c.total_games,
            Key::MetadataRowReleaseDate,
        ),
        CoverageField::Rating => (c.with_rating, c.total_games, Key::MetadataRowRating),
        CoverageField::Description => (
            c.with_description,
            c.total_games,
            Key::MetadataRowDescription,
        ),
        CoverageField::BoxArt => (c.with_thumbnail, c.total_games, Key::MetadataRowBoxArt),
        CoverageField::Manuals => (c.with_manual, c.total_games, Key::MetadataRowManuals),
        CoverageField::Videos => (c.with_video, c.total_games, Key::MetadataRowVideos),
        CoverageField::Verified => (c.verified_count, c.total_games, Key::MetadataRowVerified),
        CoverageField::RaId => (
            c.with_ra_id,
            c.total_games,
            Key::MetadataRowRetroAchievements,
        ),
    });
    let value = pct(count, total);
    let width = format!("width:{value}%");
    let pct_text = format!("{value}%");

    view! {
        <div class="coverage-row">
            <span class="coverage-row-label">{move || t(i18n.locale.get(), label_key)}</span>
            <div class="coverage-bar">
                <div class="coverage-bar-fill" style=width></div>
            </div>
            <span class="coverage-row-pct">{pct_text}</span>
        </div>
    }
}

fn composition_text(cov: StoredValue<SystemCoverage>, locale: crate::i18n::Locale) -> String {
    cov.with_value(|c| {
        let total = c.total_games.max(1);
        let unique = total
            .saturating_sub(c.clone_count + c.hack_count + c.translation_count + c.special_count);
        let optional = [
            (c.clone_count, Key::MetadataRowClones),
            (c.hack_count, Key::MetadataRowHacks),
            (c.translation_count, Key::MetadataRowTranslations),
            (c.homebrew_count, Key::MetadataRowHomebrew),
            (c.unlicensed_count, Key::MetadataRowUnlicensed),
            (c.special_count, Key::MetadataRowSpecial),
        ];
        std::iter::once(format!(
            "{}% {}",
            pct(unique, total),
            t(locale, Key::MetadataRowUnique)
        ))
        .chain(
            optional
                .into_iter()
                .filter(|(n, _)| *n > 0)
                .map(|(n, key)| format!("{}% {}", pct(n, total), t(locale, key))),
        )
        .collect::<Vec<_>>()
        .join(" \u{00B7} ")
    })
}

fn media_row_view(
    cov: StoredValue<SystemCoverage>,
    locale: crate::i18n::Locale,
) -> Option<leptos::prelude::AnyView> {
    let text = cov.with_value(|c| {
        let mut parts = Vec::new();
        if c.downloaded_boxart_files > 0 {
            parts.push(format!(
                "{} {}",
                format_number(c.downloaded_boxart_files),
                t(locale, Key::MetadataRowBoxArt).to_lowercase()
            ));
        }
        if c.downloaded_snap_files > 0 {
            parts.push(format!(
                "{} {}",
                format_number(c.downloaded_snap_files),
                t(locale, Key::MetadataRowScreenshots).to_lowercase()
            ));
        }
        if c.downloaded_title_files > 0 {
            parts.push(format!(
                "{} {}",
                format_number(c.downloaded_title_files),
                t(locale, Key::MetadataRowTitleScreens).to_lowercase()
            ));
        }
        if c.downloaded_thumbnail_bytes > 0 {
            parts.push(format_size(c.downloaded_thumbnail_bytes));
        }
        (!parts.is_empty()).then(|| {
            format!(
                "{} {}",
                t(locale, Key::MetadataRowDownloadedMedia),
                parts.join(" \u{00B7} ")
            )
        })
    })?;
    Some(view! { <div class="footer-row">{text}</div> }.into_any())
}

fn distribution_rows_view(
    cov: StoredValue<SystemCoverage>,
    locale: crate::i18n::Locale,
) -> Option<leptos::prelude::AnyView> {
    let rows = cov.with_value(|c| {
        [
            distribution_text(
                Key::MetadataRowRegions,
                &c.region_counts,
                c.total_games,
                locale,
            ),
            distribution_text(
                Key::MetadataRowGenreGroups,
                &c.genre_group_counts,
                c.total_games,
                locale,
            ),
            distribution_text(
                Key::MetadataRowPlayers,
                &c.player_count_distribution,
                c.total_games,
                locale,
            ),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
    });
    if rows.is_empty() {
        return None;
    }
    Some(
        view! {
            <div class="footer-row">
                {rows.join(" \u{00B7} ")}
            </div>
        }
        .into_any(),
    )
}

fn distribution_text(
    label_key: Key,
    buckets: &[CountBucket],
    total_games: usize,
    locale: crate::i18n::Locale,
) -> Option<String> {
    if buckets.is_empty() {
        return None;
    }
    let total = total_games.max(1);
    let values = buckets
        .iter()
        .take(3)
        .map(|bucket| {
            format!(
                "{} {}%",
                distribution_label(&bucket.label),
                pct(bucket.count, total)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!("{} {}", t(locale, label_key), values))
}

fn distribution_label(label: &str) -> String {
    if label.is_empty() {
        return "--".to_string();
    }
    label
        .split(['_', '-'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn driver_row_view(
    cov: StoredValue<SystemCoverage>,
    locale: crate::i18n::Locale,
) -> Option<leptos::prelude::AnyView> {
    let text = cov.with_value(|c| {
        c.driver_status.as_ref().map(|d: &DriverStatusCounts| {
            format!(
                "{} {} {} \u{00B7} {} {} \u{00B7} {} {} \u{00B7} {} {}",
                t(locale, Key::MetadataRowDrivers),
                format_number(d.working),
                t(locale, Key::MetadataDriverWorking),
                format_number(d.imperfect),
                t(locale, Key::MetadataDriverImperfect),
                format_number(d.preliminary),
                t(locale, Key::MetadataDriverPreliminary),
                format_number(d.unknown),
                t(locale, Key::MetadataDriverUnknown),
            )
        })
    })?;
    Some(view! { <div class="driver-row">{text}</div> }.into_any())
}

fn footer_row_view(
    cov: StoredValue<SystemCoverage>,
    locale: crate::i18n::Locale,
) -> Option<leptos::prelude::AnyView> {
    let text = cov.with_value(|c| {
        let mut parts: Vec<String> = Vec::new();
        if let Some(yr) = format_year_range(c.min_year, c.max_year) {
            parts.push(yr);
        }
        // RA-supported systems show the % bar above (even at 0%). Systems RA
        // doesn't cover (Amiga, C64, DOS, …) get an explicit "not supported"
        // note instead. Systems RA covers but whose RePlay core can't earn them
        // (PlayStation via pcsx_rearmed, PC Engine CD, MAME, …) keep the bar
        // (the matches are real) but add a note that they aren't earnable here.
        if !system_has_retroachievements(&c.system) {
            parts.push(t(locale, Key::MetadataRowNoRetroAchievements).to_string());
        } else if !system_core_supports_retroachievements(&c.system) {
            parts.push(t(locale, Key::MetadataRowRetroAchievementsNoCore).to_string());
        }
        if c.coop_count > 0 {
            parts.push(format!(
                "{} {}",
                format_number(c.coop_count),
                t(locale, Key::MetadataRowCoOp)
            ));
        }
        match c.stats_refresh_state {
            SystemStatsRefreshState::Refreshing => {
                parts.push(format!(
                    "{} {}",
                    t(locale, Key::MetadataRowStats),
                    t(locale, Key::MetadataStatsRefreshing)
                ));
            }
            SystemStatsRefreshState::Stale => {
                parts.push(format!(
                    "{} {}",
                    t(locale, Key::MetadataRowStats),
                    t(locale, Key::MetadataStatsStale)
                ));
            }
            SystemStatsRefreshState::Failed => {
                parts.push(format!(
                    "{} {}",
                    t(locale, Key::MetadataRowStats),
                    t(locale, Key::MetadataStatsFailed)
                ));
            }
            SystemStatsRefreshState::Unknown | SystemStatsRefreshState::Fresh => {}
        }
        if parts.is_empty() {
            None
        } else {
            Some(parts.join(" \u{00B7} "))
        }
    })?;
    Some(view! { <div class="footer-row">{text}</div> }.into_any())
}

// ── Skeletons ────────────────────────────────────────────────────────────

#[component]
fn SummaryCardsSkeleton() -> impl IntoView {
    view! {
        <div class="meta-skeleton-cards">
            {(0..6).map(|_| view! {
                <div class="meta-skeleton-card skeleton-shimmer"></div>
            }).collect::<Vec<_>>()}
        </div>
    }
}

#[component]
fn AccordionSkeleton() -> impl IntoView {
    view! {
        <div class="meta-skeleton-table">
            {(0..4).map(|_| view! {
                <div class="meta-skeleton-row skeleton-shimmer">
                    <div class="meta-skeleton-cell-wide"></div>
                    <div class="meta-skeleton-cell"></div>
                </div>
            }).collect::<Vec<_>>()}
        </div>
    }
}

#[component]
fn MetadataCardSkeleton() -> impl IntoView {
    view! {
        <div class="data-source-card">
            <div class="meta-skeleton-bar-wide skeleton-shimmer"></div>
            <div class="meta-skeleton-bar skeleton-shimmer"></div>
        </div>
    }
}

#[component]
fn MetadataLineSkeleton() -> impl IntoView {
    view! {
        <div class="meta-skeleton-bar skeleton-shimmer"></div>
    }
}
