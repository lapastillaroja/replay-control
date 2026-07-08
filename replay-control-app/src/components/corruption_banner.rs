use leptos::prelude::*;

use crate::i18n::{Key, t, use_i18n};
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
    let i18n = use_i18n();
    let status = expect_context::<RwSignal<CorruptionStatus>>();
    let hydrated = RwSignal::new(false);

    Effect::new(move || {
        hydrated.set(true);
    });

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
            <div class=move || {
                if hydrated.get() {
                    "corruption-banner is-hydrated"
                } else {
                    "corruption-banner"
                }
            }>
                <Show when=move || library_corrupt() fallback=|| ()>
                    <div class="corruption-banner-row">
                        <span>{move || t(i18n.locale.get(), Key::CorruptionLibraryCorrupt)}</span>
                        <button
                            class="corruption-banner-btn"
                            on:click=move |_| { rebuild_action.dispatch(()); }
                        >
                            {move || t(i18n.locale.get(), Key::CorruptionRebuild)}
                        </button>
                    </div>
                </Show>
                <Show when=move || user_data_corrupt() fallback=|| ()>
                    <div class="corruption-banner-row">
                        <span>{move || t(i18n.locale.get(), Key::CorruptionUserDataCorrupt)}</span>
                        <Show when=move || backup_exists() fallback=|| ()>
                            <button
                                class="corruption-banner-btn"
                                on:click=move |_| { restore_action.dispatch(()); }
                            >
                                {move || t(i18n.locale.get(), Key::CorruptionRestoreBackup)}
                            </button>
                        </Show>
                        <button
                            class="corruption-banner-btn corruption-banner-btn-danger"
                            on:click=move |_| { repair_action.dispatch(()); }
                        >
                            {move || {
                                t(
                                    i18n.locale.get(),
                                    if backup_exists() {
                                        Key::CorruptionReset
                                    } else {
                                        Key::CorruptionResetLoseData
                                    },
                                )
                            }}
                        </button>
                    </div>
                </Show>
            </div>
        </Show>
    }
}
