use leptos::prelude::*;

use crate::server_fns::{self, CorruptionStatus};

/// Banner shown on every page when a database is flagged as corrupt.
///
/// Reads from a context-provided `RwSignal<CorruptionStatus>` seeded from the
/// `/sse/config` `init` payload and updated via `CorruptionChanged` events
/// pushed by the server whenever a pool's corrupt flag flips. No polling.
///
/// Shows appropriate actions based on which DB is corrupt:
/// - library.db: [Rebuild] (no data loss -- rebuildable)
/// - user_data.db with backup: [Restore from backup] [Reset]
/// - user_data.db without backup: [Reset (lose data)]
#[component]
pub fn CorruptionBanner() -> impl IntoView {
    let status = expect_context::<RwSignal<CorruptionStatus>>();

    let rebuild_action = Action::new(|_: &()| async {
        let _ = server_fns::rebuild_corrupt_library().await;
    });
    let repair_action = Action::new(|_: &()| async {
        let _ = server_fns::repair_corrupt_user_data().await;
    });
    let restore_action = Action::new(|_: &()| async {
        let _ = server_fns::restore_user_data_backup().await;
    });

    let library_corrupt = move || status.read().library_corrupt;
    let user_data_corrupt = move || status.read().user_data_corrupt;
    let backup_exists = move || status.read().user_data_backup_exists;
    let any_corrupt = move || library_corrupt() || user_data_corrupt();

    view! {
        <Show when=move || any_corrupt() fallback=|| ()>
            <div class="corruption-banner">
                <Show when=move || library_corrupt() fallback=|| ()>
                    <div class="corruption-banner-row">
                        <span>"Library database is corrupt."</span>
                        <button
                            class="corruption-banner-btn"
                            on:click=move |_| { rebuild_action.dispatch(()); }
                        >
                            "Rebuild"
                        </button>
                    </div>
                </Show>
                <Show when=move || user_data_corrupt() fallback=|| ()>
                    <div class="corruption-banner-row">
                        <span>"User data is corrupt. Some data may be lost."</span>
                        <Show when=move || backup_exists() fallback=|| ()>
                            <button
                                class="corruption-banner-btn"
                                on:click=move |_| { restore_action.dispatch(()); }
                            >
                                "Restore from backup"
                            </button>
                        </Show>
                        <button
                            class="corruption-banner-btn corruption-banner-btn-danger"
                            on:click=move |_| { repair_action.dispatch(()); }
                        >
                            {move || if backup_exists() { "Reset" } else { "Reset (lose data)" }}
                        </button>
                    </div>
                </Show>
            </div>
        </Show>
    }
}
