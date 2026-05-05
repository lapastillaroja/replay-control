use leptos::prelude::*;
use leptos_router::components::A;
use server_fn::ServerFnError;

use crate::i18n::{Key, t, use_i18n};
use crate::server_fns;

/// Available log sources that map to journalctl unit filters.
const LOG_SOURCES: &[(&str, Key)] = &[
    ("all", Key::LogsSourceAll),
    ("replay-control", Key::LogsSourceCompanion),
    ("replay", Key::LogsSourceReplay),
];

const LOG_LINES: usize = 200;

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
            {move || copy_status.get().map(|(ok, msg)| {
                let class = if ok { "status-msg status-ok" } else { "status-msg status-err" };
                view! { <div class=class>{msg}</div> }
            })}

            <Suspense fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), Key::CommonLoading)}</div> }>
                {move || Suspend::new(async move {
                    let text = logs.await?;
                    Ok::<_, ServerFnError>(view! {
                        <pre id="logs-output" class="logs-output">{text}</pre>
                    })
                })}
            </Suspense>

            <Suspense fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), Key::CommonLoading)}</div> }>
                {move || Suspend::new(async move {
                    let config = log_level.await?;
                    Ok::<_, ServerFnError>(view! { <LogLevelForm config /> })
                })}
            </Suspense>
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
            match server_fns::save_log_level_config(value).await {
                Ok(()) => {
                    let locale = use_i18n().locale.get_untracked();
                    status.set(Some((true, t(locale, Key::SettingsSaved).to_string())));
                }
                Err(e) => {
                    status.set(Some((false, e.to_string())));
                }
            }
            saving.set(false);
        });
    };

    view! {
        <div class="settings-form apply-section logs-level-form">
            <div class="form-field">
                <label class="form-label">{move || t(i18n.locale.get(), Key::LogsLevelTitle)}</label>
                <select class="form-input logs-level-select" bind:value=level>
                    <option value="info">{move || t(i18n.locale.get(), Key::LogsLevelInfo)}</option>
                    <option value="debug">{move || t(i18n.locale.get(), Key::LogsLevelDebug)}</option>
                </select>
                <p class="form-hint">{move || t(i18n.locale.get(), Key::LogsLevelRebootHint)}</p>
            </div>

            {move || status.get().map(|(ok, msg)| {
                let class = if ok { "status-msg status-ok" } else { "status-msg status-err" };
                view! { <div class=class>{msg}</div> }
            })}

            <button
                class="form-btn"
                on:click=on_save
                disabled=move || saving.get()
            >
                {move || {
                    let locale = i18n.locale.get();
                    if saving.get() { t(locale, Key::SettingsSaving) } else { t(locale, Key::SettingsSave) }
                }}
            </button>
        </div>
    }
}
