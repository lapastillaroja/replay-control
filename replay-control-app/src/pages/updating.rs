use leptos::prelude::*;
use replay_control_core::update::UpdateState;

use crate::i18n::{Key, t, use_i18n};
use crate::server_fns;

/// Full-screen updating page. No nav bar.
/// Owns the entire update lifecycle: dispatch, progress, restart countdown.
#[component]
pub fn UpdatingPage() -> impl IntoView {
    let i18n = use_i18n();
    let update_state =
        use_context::<RwSignal<UpdateState>>().unwrap_or_else(|| RwSignal::new(UpdateState::None));

    // Local UI phase signal for this page.
    let phase = RwSignal::new(UpdatingPhase::Init);
    let error_msg = RwSignal::new(Option::<String>::None);
    let countdown = RwSignal::new(18i32);

    // On mount (hydrate only), check state and decide what to do.
    #[cfg(feature = "hydrate")]
    {
        Effect::new(move || {
            // Only run once on mount.
            let current_state = update_state.get_untracked();

            match current_state {
                UpdateState::Restarting { .. } => {
                    // Resume countdown.
                    phase.set(UpdatingPhase::Restarting);
                    start_countdown(countdown, phase);
                }
                UpdateState::Available(ref available) => {
                    let tag = available.tag.clone();
                    phase.set(UpdatingPhase::Downloading);

                    // Use replaceState so back button doesn't return here.
                    if let Some(window) = web_sys::window() {
                        let _ = window.history().ok().and_then(|h| {
                            h.replace_state_with_url(
                                &wasm_bindgen::JsValue::NULL,
                                "",
                                Some("/updating"),
                            )
                            .ok()
                        });
                    }

                    // Dispatch StartUpdate.
                    leptos::task::spawn_local(async move {
                        match server_fns::start_update(tag).await {
                            Ok(()) => {
                                // Success — set Restarting and start countdown.
                                let version = match update_state.get_untracked() {
                                    UpdateState::Available(a) => a.version,
                                    _ => String::new(),
                                };
                                update_state.set(UpdateState::Restarting {
                                    expected_version: version,
                                });
                                phase.set(UpdatingPhase::Restarting);
                                start_countdown(countdown, phase);
                            }
                            Err(e) => {
                                error_msg.set(Some(server_fns::format_error(e)));
                                phase.set(UpdatingPhase::Failed);
                            }
                        }
                    });
                }
                UpdateState::None => {
                    phase.set(UpdatingPhase::NothingToDo);
                }
            }
        });
    }

    view! {
        <div class="updating-page">
            <h1>{move || t(i18n.locale.get(), Key::UpdatePageTitle)}</h1>

            {move || {
                let locale = i18n.locale.get();
                match phase.get() {
                    UpdatingPhase::Init => {
                        view! { <p>{t(locale, Key::CommonLoading)}</p> }.into_any()
                    }
                    UpdatingPhase::Downloading => {
                        view! {
                            <p>{t(locale, Key::UpdateDownloading)}</p>
                            <p class="update-do-not-navigate">{t(locale, Key::UpdateDoNotNavigate)}</p>
                        }.into_any()
                    }
                    UpdatingPhase::Installing => {
                        view! {
                            <p>{t(locale, Key::UpdateInstalling)}</p>
                            <p class="update-do-not-navigate">{t(locale, Key::UpdateDoNotNavigate)}</p>
                        }.into_any()
                    }
                    UpdatingPhase::Restarting => {
                        let c = countdown.get();
                        let text = if c > 0 {
                            t(locale, Key::UpdateReloadingIn).replace("{0}", &c.to_string())
                        } else {
                            t(locale, Key::UpdateWaitingForServer).to_string()
                        };
                        view! {
                            <p>{t(locale, Key::UpdateRestarting)}</p>
                            <p>{text}</p>
                        }.into_any()
                    }
                    UpdatingPhase::Failed => {
                        view! {
                            <p class="error">{t(locale, Key::UpdateFailed)}</p>
                            {move || error_msg.get().map(|msg| view! { <p class="error">{msg}</p> })}
                            <a href="/more" class="form-btn form-btn-secondary">{t(locale, Key::UpdateBackToSettings)}</a>
                        }.into_any()
                    }
                    UpdatingPhase::Busy => {
                        view! {
                            <p>{t(locale, Key::UpdateSystemBusy)}</p>
                            <a href="/more" class="form-btn form-btn-secondary">{t(locale, Key::UpdateBackToSettings)}</a>
                        }.into_any()
                    }
                    UpdatingPhase::NothingToDo => {
                        view! {
                            <p>{t(locale, Key::UpdateUpToDate)}</p>
                            <a href="/more" class="form-btn form-btn-secondary">{t(locale, Key::UpdateBackToSettings)}</a>
                        }.into_any()
                    }
                }
            }}
        </div>
    }
}

#[derive(Clone, Copy, PartialEq)]
enum UpdatingPhase {
    Init,
    Downloading,
    Installing,
    Restarting,
    Failed,
    Busy,
    NothingToDo,
}

/// Start an 18-second countdown, then ping /api/version and reload.
#[cfg(feature = "hydrate")]
fn start_countdown(countdown: RwSignal<i32>, phase: RwSignal<UpdatingPhase>) {
    use wasm_bindgen::prelude::*;

    let interval_id = RwSignal::new(0i32);

    let cb = Closure::<dyn Fn()>::new(move || {
        let c = countdown.get_untracked();
        if c > 0 {
            countdown.set(c - 1);
        } else {
            // Clear the interval to stop repeated calls.
            if let Some(window) = web_sys::window() {
                window.clear_interval_with_handle(interval_id.get_untracked());
            }
            ping_and_reload();
        }
    });

    if let Some(window) = web_sys::window() {
        let id = window
            .set_interval_with_callback_and_timeout_and_arguments_0(
                cb.as_ref().unchecked_ref(),
                1000,
            )
            .unwrap_or(0);
        interval_id.set(id);
    }
    cb.forget();

    // Ensure phase is Restarting.
    phase.set(UpdatingPhase::Restarting);
}

/// Ping /api/version; if it responds, reload. Otherwise retry in 3s.
/// Uses inline JS via wasm_bindgen since XmlHttpRequest web-sys feature is not enabled.
#[cfg(feature = "hydrate")]
fn ping_and_reload() {
    #[wasm_bindgen::prelude::wasm_bindgen(inline_js = "
        export function ping_version_and_reload() {
            fetch('/api/version', { method: 'GET' })
                .then(resp => { if (resp.ok) window.location.replace('/'); else setTimeout(ping_version_and_reload, 3000); })
                .catch(() => setTimeout(ping_version_and_reload, 3000));
        }
    ")]
    extern "C" {
        fn ping_version_and_reload();
    }

    ping_version_and_reload();
}
