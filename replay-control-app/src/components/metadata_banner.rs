use leptos::prelude::*;

use crate::i18n::{Key, Locale, t, use_i18n};
use crate::server_fns::{self, Activity, RebuildPhase, RebuildProgress};

/// Format the per-system progress label for a rebuild/rescan. Shared
/// between the top banner and the rebuild/rescan card hint so both
/// surfaces show the same text. Returns `None` when no progress text
/// applies (terminal phases).
pub fn format_rebuild_progress_label(locale: Locale, p: &RebuildProgress) -> Option<String> {
    if matches!(p.phase, RebuildPhase::MediaStats) {
        return Some(t(locale, Key::MetadataBannerUpdatingMediaStats).to_string());
    }
    if !matches!(p.phase, RebuildPhase::Scanning | RebuildPhase::Enriching) {
        return None;
    }
    let enriching = matches!(p.phase, RebuildPhase::Enriching) || p.enriching;
    let verb_key = if enriching {
        Key::MetadataProgressVerbEnriching
    } else if p.is_rescan {
        Key::MetadataProgressVerbRescanning
    } else {
        Key::MetadataProgressVerbRebuilding
    };
    let idle_key = if enriching {
        Key::MetadataBannerEnrichingLibrary
    } else if p.is_rescan {
        Key::MetadataBannerRescanningLibrary
    } else {
        Key::MetadataBannerRebuildingLibrary
    };
    let verb = t(locale, verb_key);
    Some(match (p.current_system.as_str(), p.systems_total) {
        ("", _) => t(locale, idle_key).to_string(),
        (sys, 0) => format!("{verb} {sys}..."),
        (sys, total) => format!("{verb} {sys} ({}/{total})...", p.systems_done + 1),
    })
}

/// A thin banner shown at the top of the page when any activity is running
/// (import, thumbnail update, rebuild, startup scan, maintenance).
///
/// Reads from a context-provided `RwSignal<Activity>` fed by the App-root
/// `SseEventsListener`. No polling, no resources.
#[component]
pub fn MetadataBusyBanner() -> impl IntoView {
    let i18n = use_i18n();
    let activity = expect_context::<RwSignal<Activity>>();

    let is_busy = move || !matches!(activity.get(), Activity::Idle);

    let busy_label = move || match activity.get() {
        Activity::Idle => String::new(),
        Activity::Startup {
            phase,
            system,
            enriching,
        } => {
            use server_fns::StartupPhase;
            match phase {
                StartupPhase::FetchingMetadata => {
                    t(i18n.locale.get(), Key::MetadataBannerFetchingGameMetadata).to_string()
                }
                StartupPhase::Scanning => {
                    let phase_text = t(
                        i18n.locale.get(),
                        if enriching {
                            Key::MetadataProgressLibraryEnriching
                        } else {
                            Key::MetadataProgressLibraryScanning
                        },
                    );
                    if system.is_empty() {
                        format!("{phase_text}...")
                    } else {
                        format!("{phase_text} ({system})...")
                    }
                }
                StartupPhase::MediaStats => {
                    t(i18n.locale.get(), Key::MetadataBannerUpdatingMediaStats).to_string()
                }
                StartupPhase::RebuildingIndex => "Rebuilding thumbnail index...".to_string(),
            }
        }
        Activity::Import { progress } => {
            use server_fns::ImportState;
            match progress.state {
                ImportState::Downloading => "Downloading metadata...".to_string(),
                ImportState::BuildingIndex => "Building ROM index...".to_string(),
                ImportState::Parsing => {
                    format!("Importing metadata ({} matched)...", progress.matched)
                }
                _ => "Importing metadata...".to_string(),
            }
        }
        Activity::ThumbnailUpdate { progress, .. } => {
            if progress.current_label.is_empty() {
                "Updating thumbnails...".to_string()
            } else {
                format!("Updating thumbnails: {}", progress.current_label)
            }
        }
        Activity::Rebuild { progress } => {
            format_rebuild_progress_label(i18n.locale.get(), &progress).unwrap_or_default()
        }
        Activity::Identity { progress } => {
            let pct = progress
                .rows_done
                .saturating_mul(100)
                .checked_div(progress.rows_total)
                .unwrap_or_default();
            format!(
                "Matching ROMs ({pct}%, {}/{})...",
                progress.rows_done, progress.rows_total
            )
        }
        Activity::Maintenance { kind } => {
            use server_fns::MaintenanceKind;
            match kind {
                MaintenanceKind::ClearMetadata => "Clearing metadata...".to_string(),
                MaintenanceKind::ClearImages => "Clearing images...".to_string(),
                MaintenanceKind::ClearThumbnailIndex => "Clearing thumbnail index...".to_string(),
                MaintenanceKind::CleanupOrphans => "Cleaning up orphaned images...".to_string(),
            }
        }
        Activity::Update { progress } => {
            if progress.phase_detail.is_empty() {
                "Updating software...".to_string()
            } else {
                progress.phase_detail.clone()
            }
        }
        Activity::RefreshExternalMetadata { progress } => {
            use server_fns::RefreshMetadataPhase;
            match progress.phase {
                RefreshMetadataPhase::Checking => "Checking metadata...".to_string(),
                RefreshMetadataPhase::Downloading => {
                    if progress.downloaded_bytes > 0 {
                        format!(
                            "Downloading metadata ({} MB)...",
                            progress.downloaded_bytes / (1024 * 1024)
                        )
                    } else {
                        "Downloading metadata...".to_string()
                    }
                }
                RefreshMetadataPhase::Parsing => "Parsing metadata...".to_string(),
                RefreshMetadataPhase::Enriching => "Re-enriching library...".to_string(),
                RefreshMetadataPhase::Complete => String::new(),
                RefreshMetadataPhase::Failed => "Metadata refresh failed".to_string(),
                RefreshMetadataPhase::UpToDate => {
                    t(i18n.locale.get(), Key::MetadataBannerAlreadyUpToDate).to_string()
                }
            }
        }
    };

    view! {
        <Show when=move || is_busy() fallback=|| ()>
            <div class="metadata-busy-banner">
                <span class="metadata-busy-spinner"></span>
                {move || {
                    let lbl = busy_label();
                    if lbl.is_empty() {
                        t(i18n.locale.get(), Key::MetadataBusyBanner).to_string()
                    } else {
                        lbl
                    }
                }}
            </div>
        </Show>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_stats_phase_has_dedicated_label() {
        let progress = RebuildProgress {
            phase: RebuildPhase::MediaStats,
            current_system: "PlayStation".to_string(),
            systems_done: 42,
            systems_total: 42,
            elapsed_secs: 12,
            error: None,
            is_rescan: true,
            enriching: false,
        };

        assert_eq!(
            format_rebuild_progress_label(Locale::En, &progress).as_deref(),
            Some("Updating media stats...")
        );
    }
}
