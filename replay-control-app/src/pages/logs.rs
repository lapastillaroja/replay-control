use crate::components::status_message::StatusMessage;
use leptos::either::Either;
use leptos::prelude::*;
use leptos_router::components::A;
use server_fn::ServerFnError;

use replay_control_core::replay_api::ReplayLogLevel;

use crate::i18n::{Key, t, use_i18n};
use crate::server_fns;

/// Available log sources that map to journalctl unit filters.
const LOG_SOURCES: &[(&str, Key)] = &[
    ("all", Key::LogsSourceAll),
    ("replay-control", Key::LogsSourceCompanion),
    ("replay", Key::LogsSourceReplay),
];

const LOG_LINES: usize = 400;

#[component]
pub fn LogsPage() -> impl IntoView {
    let i18n = use_i18n();
    let source = RwSignal::new("all".to_string());
    let version = RwSignal::new(0u32);
    let log_level = Resource::new_blocking(|| (), |_| server_fns::get_log_level_config());
    let copying = RwSignal::new(false);
    let copy_status = RwSignal::new(Option::<(bool, String)>::None);

    let logs = Resource::new_blocking(
        move || (source.get(), version.get()),
        |(src, _)| server_fns::get_system_logs(src, LOG_LINES),
    );

    // Shown when a source has no logs. The "replay" source is empty whenever
    // RePlayOS isn't writing its frontend logs to a readable location on this
    // device, so it gets a more specific explanation than the generic case.
    let empty_msg = move || {
        let key = if source.get() == "replay" {
            Key::LogsReplayUnavailable
        } else {
            Key::LogsEmpty
        };
        t(i18n.locale.get(), key)
    };

    let on_refresh = move |_| version.update(|v| *v += 1);
    let on_copy = move |_| {
        copying.set(true);
        copy_status.set(None);

        leptos::task::spawn_local(async move {
            match copy_rendered_logs_to_clipboard().await {
                Ok(()) => {
                    let locale = use_i18n().locale.get_untracked();
                    copy_status.set(Some((true, t(locale, Key::LogsCopied).to_string())));
                }
                Err(e) => {
                    copy_status.set(Some((false, e)));
                }
            }
            copying.set(false);
        });
    };

    view! {
        <div class="page settings-page">
            <div class="rom-header">
                <A href="/settings" attr:class="back-btn">
                    {move || t(i18n.locale.get(), Key::GamesBack)}
                </A>
                <h2 class="page-title">{move || t(i18n.locale.get(), Key::LogsTitle)}</h2>
            </div>

            <div class="logs-controls">
                <select
                    class="form-input logs-source-select"
                    on:change=move |ev| source.set(event_target_value(&ev))
                    prop:value=move || source.get()
                >
                    {LOG_SOURCES.iter().map(|(value, label_key)| {
                        let value = *value;
                        let label_key = *label_key;
                        view! {
                            <option value=value>
                                {move || t(i18n.locale.get(), label_key)}
                            </option>
                        }
                    }).collect::<Vec<_>>()}
                </select>
                <button class="form-btn form-btn-secondary" on:click=on_refresh>
                    {move || t(i18n.locale.get(), Key::LogsRefresh)}
                </button>
                <button
                    class="form-btn form-btn-secondary logs-copy-btn"
                    on:click=on_copy
                    disabled=move || copying.get()
                >
                    <span class="logs-copy-icon" aria-hidden="true">{"\u{1F4CB}"}</span>
                    {move || t(i18n.locale.get(), Key::LogsCopy)}
                </button>
            </div>
            <StatusMessage status=copy_status />

            <Suspense fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), Key::CommonLoading)}</div> }>
                {move || Suspend::new(async move {
                    let text = logs.await?;
                    Ok::<_, ServerFnError>(if text.trim().is_empty() {
                        Either::Left(view! { <p class="logs-empty">{empty_msg}</p> })
                    } else {
                        Either::Right(view! { <pre id="logs-output" class="logs-output">{text}</pre> })
                    })
                })}
            </Suspense>

            <ReplayOsLogLevelRow />

            <Suspense fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), Key::CommonLoading)}</div> }>
                {move || Suspend::new(async move {
                    let config = log_level.await?;
                    Ok::<_, ServerFnError>(view! { <LogLevelForm config /> })
                })}
            </Suspense>
        </div>
    }
}

/// Map a RePlayOS UI log level (read live from the device) to its display key.
/// `None` (couldn't read it) renders as "Unavailable".
fn replay_level_key(level: Option<ReplayLogLevel>) -> Key {
    match level {
        Some(ReplayLogLevel::Debug) => Key::LogsLevelDebug,
        Some(ReplayLogLevel::Info) => Key::LogsLevelInfo,
        Some(ReplayLogLevel::Warn) => Key::LogsLevelWarn,
        Some(ReplayLogLevel::Error) => Key::LogsLevelError,
        Some(ReplayLogLevel::Disabled) => Key::LogsLevelDisabled,
        Some(ReplayLogLevel::Unknown) => Key::LogsLevelUnknown,
        None => Key::LogsReplayLevelUnavailable,
    }
}

/// Read-only display of the RePlayOS UI log level. Fetched client-side so a
/// slow or unreachable device API never blocks the page; degrades to
/// "Unavailable". The level can't be set from here — the RePlayOS API rejects
/// writes to it — so this just shows the current value and points the user at
/// the TV menu (kept in English, since RePlayOS' UI is English-only).
#[component]
fn ReplayOsLogLevelRow() -> impl IntoView {
    let i18n = use_i18n();
    let level = Resource::new(|| (), |_| server_fns::get_replayos_log_level());

    view! {
        <div class="settings-form logs-level-form logs-replay-level">
            <div class="form-field">
                <label class="form-label">
                    {move || t(i18n.locale.get(), Key::LogsReplayLevelTitle)}
                </label>
                <Transition fallback=move || view! {
                    <span class="form-static">{move || t(i18n.locale.get(), Key::CommonLoading)}</span>
                }>
                    {move || Suspend::new(async move {
                        let key = replay_level_key(level.await.unwrap_or(None));
                        view! {
                            <span class="form-static logs-replay-level-value">
                                {move || {
                                    let locale = i18n.locale.get();
                                    format!(
                                        "{}: {}",
                                        t(locale, Key::LogsReplayLevelPrefix),
                                        t(locale, key),
                                    )
                                }}
                            </span>
                        }
                    })}
                </Transition>
                <p class="form-hint">{move || t(i18n.locale.get(), Key::LogsReplayLevelHint)}</p>
            </div>
        </div>
    }
}

#[cfg(feature = "hydrate")]
async fn copy_rendered_logs_to_clipboard() -> Result<(), String> {
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen(inline_js = "
        export function replay_copy_rendered_logs_to_clipboard() {
            const output = document.getElementById('logs-output');
            if (!output) {
                return Promise.reject(new Error('logs output not found'));
            }
            const text = output.innerText || output.textContent || '';
            if (!text) {
                return Promise.reject(new Error('logs output is empty'));
            }
            if (navigator.clipboard && typeof navigator.clipboard.writeText === 'function') {
                return navigator.clipboard.writeText(text);
            }

            return new Promise((resolve, reject) => {
                const textarea = document.createElement('textarea');
                textarea.value = text;
                textarea.setAttribute('readonly', '');
                textarea.style.position = 'fixed';
                textarea.style.left = '-9999px';
                textarea.style.top = '0';
                document.body.appendChild(textarea);
                textarea.focus();
                textarea.select();

                try {
                    if (document.execCommand('copy')) {
                        resolve();
                    } else {
                        reject(new Error('copy command failed'));
                    }
                } catch (error) {
                    reject(error);
                } finally {
                    document.body.removeChild(textarea);
                }
            });
        }
    ")]
    extern "C" {
        fn replay_copy_rendered_logs_to_clipboard() -> js_sys::Promise;
    }

    wasm_bindgen_futures::JsFuture::from(replay_copy_rendered_logs_to_clipboard())
        .await
        .map_err(|_| "Clipboard copy failed.".to_string())?;
    Ok(())
}

#[cfg(not(feature = "hydrate"))]
async fn copy_rendered_logs_to_clipboard() -> Result<(), String> {
    Err("Clipboard is unavailable.".to_string())
}

#[component]
fn LogLevelForm(config: server_fns::LogLevelConfig) -> impl IntoView {
    let i18n = use_i18n();

    let level = RwSignal::new(config.level);
    let saving = RwSignal::new(false);
    let status = RwSignal::new(Option::<(bool, String)>::None);

    let on_save = move |_| {
        saving.set(true);
        status.set(None);
        let value = level.get();

        leptos::task::spawn_local(async move {
            let locale = use_i18n().locale.get_untracked();
            match server_fns::save_log_level_config(value).await {
                Ok(result) if result.restarting => {
                    // Service is bouncing to apply the new RUST_LOG. Keep the
                    // form disabled and reload once it's back so the session
                    // reconnects cleanly (mirrors the certificate-rotation flow).
                    status.set(Some((
                        true,
                        t(locale, Key::LogsLevelRestarting).to_string(),
                    )));
                    crate::util::reload_after_ms(6000);
                }
                Ok(_) => {
                    // No change (or off-device): nothing restarted.
                    status.set(Some((true, t(locale, Key::SettingsSaved).to_string())));
                    saving.set(false);
                }
                Err(e) => {
                    status.set(Some((false, e.to_string())));
                    saving.set(false);
                }
            }
        });
    };

    view! {
        <div class="settings-form apply-section logs-level-form">
            <div class="form-field">
                <label class="form-label">{move || t(i18n.locale.get(), Key::LogsLevelTitle)}</label>
                <select class="form-input logs-level-select" bind:value=level>
                    <option value="error">{move || t(i18n.locale.get(), Key::LogsLevelError)}</option>
                    <option value="info">{move || t(i18n.locale.get(), Key::LogsLevelInfo)}</option>
                    <option value="debug">{move || t(i18n.locale.get(), Key::LogsLevelDebug)}</option>
                </select>
                <p class="form-hint">{move || t(i18n.locale.get(), Key::LogsLevelRestartHint)}</p>
            </div>

            <StatusMessage status=status />

            <button
                class="form-btn"
                on:click=on_save
                disabled=move || saving.get()
            >
                {move || {
                    let locale = i18n.locale.get();
                    if saving.get() { t(locale, Key::SettingsSaving) } else { t(locale, Key::LogsLevelSaveRestart) }
                }}
            </button>
        </div>
    }
}
