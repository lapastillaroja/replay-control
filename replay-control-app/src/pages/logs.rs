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

    let logs = Resource::new_blocking(
        move || (source.get(), version.get()),
        |(src, _)| server_fns::get_system_logs(src, LOG_LINES),
    );

    let on_refresh = move |_| version.update(|v| *v += 1);

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
                >
                    {LOG_SOURCES.iter().map(|(value, label_key)| {
                        let value = *value;
                        let label_key = *label_key;
                        view! {
                            <option value=value selected=move || *source.read() == value>
                                {move || t(i18n.locale.get(), label_key)}
                            </option>
                        }
                    }).collect::<Vec<_>>()}
                </select>
                <button class="form-btn form-btn-secondary" on:click=on_refresh>
                    {move || t(i18n.locale.get(), Key::LogsRefresh)}
                </button>
            </div>

            <Suspense fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), Key::CommonLoading)}</div> }>
                {move || Suspend::new(async move {
                    let text = logs.await?;
                    Ok::<_, ServerFnError>(view! {
                        <pre class="logs-output">{text}</pre>
                    })
                })}
            </Suspense>
        </div>
    }
}
