use leptos::prelude::*;

use crate::server_fns;

/// Banner shown on every page when a database is flagged as corrupt.
/// Polls corruption status every ~5 seconds (less frequent than the
/// activity banner since corruption is a rare, persistent state).
///
/// Shows appropriate actions based on which DB is corrupt:
/// - library.db: [Rebuild] (no data loss -- rebuildable)
/// - user_data.db with backup: [Restore from backup] [Repair]
/// - user_data.db without backup: [Repair (lose data)]
#[component]
pub fn CorruptionBanner() -> impl IntoView {
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
                5000,
            );
            cb.forget();
        });
    }

    let status_res = LocalResource::new(move || {
        let _ = tick.get();
        async move { server_fns::get_corruption_status().await.ok() }
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

    let get_status = move || -> Option<server_fns::CorruptionStatus> {
        let wrapper = status_res.get()?;
        (*wrapper).clone()
    };

    let library_corrupt = move || get_status().is_some_and(|s| s.library_corrupt);
    let user_data_corrupt = move || get_status().is_some_and(|s| s.user_data_corrupt);
    let backup_exists = move || get_status().is_some_and(|s| s.user_data_backup_exists);
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
                            {move || if backup_exists() { "Repair" } else { "Repair (lose data)" }}
                        </button>
                    </div>
                </Show>
            </div>
        </Show>
    }
}
