use leptos::prelude::*;
use leptos_router::components::A;
use server_fn::ServerFnError;

use crate::components::stat_card::StatCard;
use crate::i18n::{Key, t, use_i18n};
use crate::server_fns::{
    self, Activity, DriverStatusCounts, ImportState, LibrarySummary, MetadataPageSnapshot,
    RebuildPhase, SystemCoverage, ThumbnailPhase,
};
use crate::util::{format_number, format_size, format_year_range, pct};

type SnapshotRes = Resource<Result<MetadataPageSnapshot, ServerFnError>>;

#[component]
pub fn MetadataPage() -> impl IntoView {
    let i18n = use_i18n();
    // Single snapshot resource replaces six per-stat server fns. SSR fan-out
    // collapses to one DB pool acquisition and one closure regardless of how
    // many sections render. See `api/library/metadata_snapshot.rs`.
    let snapshot: SnapshotRes = Resource::new(|| (), |_| server_fns::get_metadata_page_snapshot());

    // App-level activity signal (populated by SseActivityListener at the App
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
    let thumb_progress = Memo::new(move |_| match activity.get() {
        Activity::ThumbnailUpdate { progress, .. } => Some(progress),
        _ => None,
    });

    // Derived helpers.
    let is_busy = Memo::new(move |_| activity.with(|a| !matches!(a, Activity::Idle)));
    let is_importing = Memo::new(move |_| activity.with(|a| matches!(a, Activity::Import { .. })));
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
                <Suspense fallback=move || view! { <SummaryCardsSkeleton /> }>
                    {move || Suspend::new(async move {
                        let locale = i18n.locale.get();
                        let snap = snapshot.await?;
                        let s = snap.library_summary;
                        let storage_kind = snap.storage_kind;
                        Ok::<_, ServerFnError>(if s.total_games == 0 {
                            // Empty library: still show the storage-type card —
                            // it's infrastructure info, not derived from games.
                            view! { <StorageOnlyCard storage_kind locale /> }.into_any()
                        } else {
                            view! { <SummaryCards summary=s storage_kind locale /> }.into_any()
                        })
                    })}
                </Suspense>
            </section>

            <SystemOverviewSection snapshot />

            // ── Data Sources ──────────────────────────────────────────
            <section class="section">
                <h2 class="section-title">{move || t(i18n.locale.get(), Key::MetadataDataSources)}</h2>

                // Built-in data info block
                <Suspense fallback=move || view! { <MetadataCardSkeleton /> }>
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

                // Descriptions & Ratings
                <div class="data-source-card">
                    <div class="data-source-header">
                        <span class="data-source-name">{move || t(i18n.locale.get(), Key::MetadataDescriptionsRatings)}</span>
                    </div>
                    <Suspense fallback=move || view! { <MetadataLineSkeleton /> }>
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
                            let snap = snapshot.await?;
                            let ds = snap.data_source;
                            let (with_boxart, with_snap, media_size) = snap.image_stats;

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
            <DataManagementSection snapshot activity result_message=rebuild_result is_busy />

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
    snapshot: SnapshotRes,
) {
    let target = match &prev {
        Activity::Import { .. } => Some(import_result),
        Activity::ThumbnailUpdate { .. } => {
            thumb_cancelling.set(false);
            Some(thumb_result)
        }
        Activity::Rebuild { .. } => Some(rebuild_result),
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
    snapshot: SnapshotRes,
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
            if let Err(e) = server_fns::rebuild_game_library().await {
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

    // Enrichment: weighted average of genre/developer/rating/art coverage.
    let enrichment_value = if s.total_games > 0 {
        let avg = (pct(s.with_genre, s.total_games)
            + pct(s.with_developer, s.total_games)
            + pct(s.with_rating, s.total_games)
            + pct(s.with_box_art, s.total_games))
            / 4;
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
                value=storage_label
                label=t(locale, Key::MetadataSummaryStorage) />
        </div>
    }
}

// ── System overview accordion ────────────────────────────────────────────

#[component]
fn SystemOverviewSection(snapshot: SnapshotRes) -> impl IntoView {
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
            <Suspense fallback=move || view! { <AccordionSkeleton /> }>
                {move || Suspend::new(async move {
                    let snap = snapshot.await?;
                    let data = snap.coverage;
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
            </Suspense>
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
        let r = pct(c.with_rating, c.total_games);
        let desc = pct(c.with_description, c.total_games);
        let art = pct(c.with_thumbnail, c.total_games);
        let overall = (g + d + r + desc + art) / 5;
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

    view! {
        <div class="system-row-details" on:click=|ev| ev.stop_propagation()>
            <CoverageBarRow cov field=CoverageField::Genre />
            <CoverageBarRow cov field=CoverageField::Developer />
            <CoverageBarRow cov field=CoverageField::Rating />
            <CoverageBarRow cov field=CoverageField::Description />
            <CoverageBarRow cov field=CoverageField::BoxArt />

            <div class="composition-row">
                {move || composition_text(cov, i18n.locale.get())}
            </div>

            {move || driver_row_view(cov, i18n.locale.get())}

            {move || footer_row_view(cov, i18n.locale.get())}
        </div>
    }
}

#[derive(Copy, Clone)]
enum CoverageField {
    Genre,
    Developer,
    Rating,
    Description,
    BoxArt,
}

#[component]
fn CoverageBarRow(cov: StoredValue<SystemCoverage>, field: CoverageField) -> impl IntoView {
    let i18n = use_i18n();
    let (count, total, label_key) = cov.with_value(|c| match field {
        CoverageField::Genre => (c.with_genre, c.total_games, Key::MetadataRowGenre),
        CoverageField::Developer => (c.with_developer, c.total_games, Key::MetadataRowDeveloper),
        CoverageField::Rating => (c.with_rating, c.total_games, Key::MetadataRowRating),
        CoverageField::Description => (
            c.with_description,
            c.total_games,
            Key::MetadataRowDescription,
        ),
        CoverageField::BoxArt => (c.with_thumbnail, c.total_games, Key::MetadataRowBoxArt),
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
        if c.verified_count > 0 {
            parts.push(format!(
                "{}/{} {}",
                format_number(c.verified_count),
                format_number(c.total_games),
                t(locale, Key::MetadataRowVerified),
            ));
        }
        if c.coop_count > 0 {
            parts.push(format!(
                "{} {}",
                format_number(c.coop_count),
                t(locale, Key::MetadataRowCoOp)
            ));
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
