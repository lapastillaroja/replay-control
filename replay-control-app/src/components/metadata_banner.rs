use leptos::prelude::*;

use crate::i18n::{t, use_i18n};
use crate::server_fns::{self, Activity};

/// A thin banner shown at the top of the page when any activity is running
/// (import, thumbnail update, rebuild, startup scan, maintenance).
/// Polls every ~3 seconds and auto-hides when the activity returns to Idle.
#[component]
pub fn MetadataBusyBanner() -> impl IntoView {
    let i18n = use_i18n();

    // A signal that ticks every ~3 seconds to trigger re-polling.
    let tick = RwSignal::new(0u32);

    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::prelude::*;

        Effect::new(move || {
            let window = match web_sys::window() {
                Some(w) => w,
                None => return,
            };
            let cb = Closure::<dyn Fn()>::new(move || {
                tick.update(|n| *n = n.wrapping_add(1));
            });
            let _ = window.set_interval_with_callback_and_timeout_and_arguments_0(
                cb.as_ref().unchecked_ref(),
                3000,
            );
            // The banner lives at the App root and never unmounts, so forget is fine.
            cb.forget();
        });
    }

    // LocalResource avoids the hydration mismatch warning: this is a
    // client-only runtime status check, not SSR-rendered content.
    let activity_res = LocalResource::new(move || {
        // Re-run whenever the tick signal changes (every ~3s on the client).
        let _ = tick.get();
        async move { server_fns::get_activity().await.unwrap_or(Activity::Idle) }
    });

    let is_busy = move || {
        activity_res
            .get()
            .is_some_and(|v| !matches!(*v, Activity::Idle))
    };

    let busy_label = move || {
        let act = match activity_res.get() {
            Some(v) => (*v).clone(),
            None => return String::new(),
        };
        match act {
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
                    StartupPhase::RebuildingIndex => {
                        "Rebuilding thumbnail index...".to_string()
                    }
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
                    format!("Updating thumbnails ({})...", progress.current_label)
                }
            }
            Activity::Rebuild { progress } => {
                if progress.current_system.is_empty() {
                    "Rebuilding library...".to_string()
                } else {
                    format!("Rebuilding library ({})...", progress.current_system)
                }
            }
            Activity::Maintenance { kind } => {
                use server_fns::MaintenanceKind;
                match kind {
                    MaintenanceKind::ClearMetadata => "Clearing metadata...".to_string(),
                    MaintenanceKind::ClearImages => "Clearing images...".to_string(),
                    MaintenanceKind::ClearThumbnailIndex => {
                        "Clearing thumbnail index...".to_string()
                    }
                    MaintenanceKind::CleanupOrphans => "Cleaning up orphaned images...".to_string(),
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
                        t(i18n.locale.get(), "metadata.busy_banner").to_string()
                    } else {
                        lbl
                    }
                }}
            </div>
        </Show>
    }
}
