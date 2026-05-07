use leptos::prelude::*;

use crate::types::RomWatcherStatus;

/// App-shell banner shown when the local ROM watcher failed to start.
///
/// `Skipped` (NFS storage, missing dir) is informational and does not
/// surface a banner — those are designed limitations, not failures.
#[component]
pub fn RomWatcherBanner() -> impl IntoView {
    let status = expect_context::<RwSignal<RomWatcherStatus>>();
    let reason = move || banner_reason(&status.get());

    view! {
        <Show when=move || reason().is_some() fallback=|| ()>
            <div class="rom-watcher-banner">
                <div class="rom-watcher-banner-row">
                    <span>
                        "ROM auto-detection is not running. Use manual rescan or restart Replay Control to detect newly added ROMs."
                    </span>
                    <small class="rom-watcher-banner-reason">
                        {move || reason().unwrap_or_default()}
                    </small>
                </div>
            </div>
        </Show>
    }
}

fn banner_reason(status: &RomWatcherStatus) -> Option<String> {
    match status {
        RomWatcherStatus::Failed { reason } => Some(reason.clone()),
        RomWatcherStatus::Active | RomWatcherStatus::Skipped { .. } => None,
    }
}
