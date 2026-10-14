use leptos::prelude::*;

use crate::i18n::{Key, t, use_i18n};
#[cfg(feature = "hydrate")]
use {
    crate::hooks::use_update_state, crate::i18n::Locale, crate::server_fns,
    replay_control_core::update::UpdateState,
};

#[cfg(feature = "hydrate")]
const PENDING_VERSION_KEY: &str = "replay-control-pending-update-version";

#[cfg(feature = "hydrate")]
fn pending_version() -> Option<String> {
    web_sys::window()?
        .session_storage()
        .ok()??
        .get_item(PENDING_VERSION_KEY)
        .ok()?
}

#[cfg(feature = "hydrate")]
fn set_pending_version(version: &str) {
    if let Some(storage) =
        web_sys::window().and_then(|window| window.session_storage().ok().flatten())
    {
        let _ = storage.set_item(PENDING_VERSION_KEY, version);
    }
}

#[cfg(feature = "hydrate")]
fn clear_pending_version() {
    if let Some(storage) =
        web_sys::window().and_then(|window| window.session_storage().ok().flatten())
    {
        let _ = storage.remove_item(PENDING_VERSION_KEY);
    }
}

/// Full-screen updating page. No nav bar.
/// Owns the entire update lifecycle: dispatch, progress, restart countdown.
#[component]
pub fn UpdatingPage() -> impl IntoView {
    let i18n = use_i18n();
    #[cfg(feature = "hydrate")]
    let update_state = use_update_state();

    // Local UI phase signal for this page.
    let phase = RwSignal::new(UpdatingPhase::Init);
    #[cfg(feature = "hydrate")]
    let error_msg = RwSignal::new(Option::<String>::None);
    #[cfg(feature = "hydrate")]
    let countdown = RwSignal::new(18i32);

    // On mount (hydrate only), check state and decide what to do.
    #[cfg(feature = "hydrate")]
    {
        Effect::new(move || {
            // Only run once on mount.
            let current_state = update_state.get_untracked();

            match current_state {
                UpdateState::Restarting { expected_version } => {
                    // Resume countdown.
                    set_pending_version(&expected_version);
                    phase.set(UpdatingPhase::Restarting);
                    start_countdown(
                        countdown,
                        phase,
                        expected_version,
                        i18n.locale.get_untracked(),
                    );
                }
                UpdateState::Available(ref available) => {
                    let tag = available.tag.clone();
                    let expected_version = available.version.clone();
                    set_pending_version(&expected_version);
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
                                let version = expected_version;
                                update_state.set(UpdateState::Restarting {
                                    expected_version: version.clone(),
                                });
                                phase.set(UpdatingPhase::Restarting);
                                start_countdown(
                                    countdown,
                                    phase,
                                    version,
                                    i18n.locale.get_untracked(),
                                );
                            }
                            Err(e) => {
                                clear_pending_version();
                                error_msg.set(Some(server_fns::format_error(e)));
                                phase.set(UpdatingPhase::Failed);
                            }
                        }
                    });
                }
                UpdateState::None => {
                    if let Some(version) = pending_version() {
                        phase.set(UpdatingPhase::Restarting);
                        start_countdown(countdown, phase, version, i18n.locale.get_untracked());
                    } else {
                        phase.set(UpdatingPhase::NothingToDo);
                    }
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
                    #[cfg(feature = "hydrate")]
                    UpdatingPhase::Downloading => {
                        view! {
                            <p>{t(locale, Key::UpdateDownloading)}</p>
                            <p class="update-do-not-navigate">{t(locale, Key::UpdateDoNotNavigate)}</p>
                        }.into_any()
                    }
                    #[cfg(feature = "hydrate")]
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
                    #[cfg(feature = "hydrate")]
                    UpdatingPhase::Failed => {
                        view! {
                            <p class="error">{t(locale, Key::UpdateFailed)}</p>
                            {move || error_msg.get().map(|msg| view! { <p class="error">{msg}</p> })}
                            <a href="/settings" class="form-btn form-btn-secondary">{t(locale, Key::UpdateBackToSettings)}</a>
                        }.into_any()
                    }
                    #[cfg(feature = "hydrate")]
                    UpdatingPhase::NothingToDo => {
                        view! {
                            <p>{t(locale, Key::UpdateUpToDate)}</p>
                            <a href="/settings" class="form-btn form-btn-secondary">{t(locale, Key::UpdateBackToSettings)}</a>
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
    #[cfg(feature = "hydrate")]
    Downloading,
    #[cfg(feature = "hydrate")]
    Restarting,
    #[cfg(feature = "hydrate")]
    Failed,
    #[cfg(feature = "hydrate")]
    NothingToDo,
}

/// Start an 18-second countdown, then ping /api/version and reload.
#[cfg(feature = "hydrate")]
fn start_countdown(
    countdown: RwSignal<i32>,
    phase: RwSignal<UpdatingPhase>,
    expected_version: String,
    locale: Locale,
) {
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
            ping_and_reload(&expected_version, locale);
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
fn ping_and_reload(expected_version: &str, locale: Locale) {
    #[wasm_bindgen::prelude::wasm_bindgen(inline_js = "
        export async function ping_version_and_reload(expected, complete, continueLabel, backLabel) {
            const showResult = (message, failed) => {
                const page = document.querySelector('.updating-page');
                if (!page) return;
                page.querySelectorAll('p, a').forEach(node => node.remove());
                const result = document.createElement('p');
                result.className = failed ? 'error' : 'success';
                result.textContent = message;
                const link = document.createElement('a');
                link.href = failed ? '/settings' : '/';
                link.className = 'form-btn form-btn-secondary';
                link.textContent = failed ? backLabel : continueLabel;
                page.append(result, link);
            };
            try {
                const result = await fetch('/api/update-result', { cache: 'no-store' }).then(r => r.text());
                if (result.startsWith('failed:')) {
                    try { sessionStorage.removeItem('replay-control-pending-update-version'); } catch (_) {}
                    showResult(result.slice(7), true);
                    return;
                }
                const response = await fetch('/api/version', { cache: 'no-store' });
                if (response.ok && (await response.json()).version === expected && (result.startsWith('ok:') || result === '')) {
                    try { sessionStorage.removeItem('replay-control-pending-update-version'); } catch (_) {}
                    showResult(complete, false);
                    setTimeout(() => window.location.replace('/'), 3000);
                    return;
                }
            } catch (_) {}
            setTimeout(() => ping_version_and_reload(expected, complete, continueLabel, backLabel), 3000);
        }
    ")]
    extern "C" {
        fn ping_version_and_reload(
            expected_version: &str,
            complete: &str,
            continue_label: &str,
            back_label: &str,
        );
    }

    ping_version_and_reload(
        expected_version,
        t(locale, Key::UpdateComplete),
        t(locale, Key::UpdateContinue),
        t(locale, Key::UpdateBackToSettings),
    );
}
