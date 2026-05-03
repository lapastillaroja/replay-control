use leptos::prelude::*;

use crate::i18n::{Key, t, use_i18n};
use crate::server_fns::{self, Activity};

/// A thin banner shown at the top of the page when any activity is running
/// (import, thumbnail update, rebuild, startup scan, maintenance).
///
/// Reads from a context-provided `RwSignal<Activity>` fed by the App-root
/// `SseActivityListener`. No polling, no resources.
#[component]
pub fn MetadataBusyBanner() -> impl IntoView {
    let i18n = use_i18n();
    let activity = expect_context::<RwSignal<Activity>>();

    let is_busy = move || !matches!(activity.get(), Activity::Idle);

    let busy_label = move || match activity.get() {
        Activity::Idle => String::new(),
        Activity::Startup { phase, system } => {
            use server_fns::StartupPhase;
            match phase {
                StartupPhase::Scanning => {
                    if system.is_empty() {
                        "Scanning game library...".to_string()
                    } else {
                        format!("Scanning game library ({system})...")
                    }
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
            let idle_key = if progress.is_rescan {
                Key::MetadataBannerRescanningLibrary
            } else {
                Key::MetadataBannerRebuildingLibrary
            };
            let phase_verb = match progress.phase {
                server_fns::RebuildPhase::Scanning => "Scanning",
                server_fns::RebuildPhase::Enriching => "Enriching",
                _ if progress.is_rescan => "Rescanning",
                _ => "Rebuilding",
            };
            match (progress.current_system.as_str(), progress.systems_total) {
                ("", _) => t(i18n.locale.get(), idle_key).to_string(),
                (sys, 0) => format!("{phase_verb} {sys}..."),
                (sys, total) => format!(
                    "{phase_verb} {sys} ({}/{total})...",
                    progress.systems_done + 1
                ),
            }
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
