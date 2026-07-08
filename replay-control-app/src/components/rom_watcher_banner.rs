use leptos::prelude::*;

use crate::i18n::{Key, t, use_i18n};
use crate::types::RomWatcherStatus;

/// App-shell banner shown when the local ROM watcher failed to start.
///
/// `Skipped` (NFS storage, missing dir) is informational and does not
/// surface a banner — those are designed limitations, not failures.
#[component]
pub fn RomWatcherBanner() -> impl IntoView {
    let i18n = use_i18n();
    let status = expect_context::<RwSignal<RomWatcherStatus>>();
    let reason = move || banner_reason(&status.get());

    view! {
        <Show when=move || reason().is_some() fallback=|| ()>
            <div class="rom-watcher-banner">
                <div class="rom-watcher-banner-row">
                    <span>{move || t(i18n.locale.get(), Key::RomWatcherStopped)}</span>
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
