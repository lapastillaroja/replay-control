use leptos::prelude::*;
use leptos_router::components::A;
use server_fn::ServerFnError;

use crate::i18n::{t, use_i18n};
use crate::pages::ErrorDisplay;
use crate::server_fns;

#[component]
pub fn GithubPage() -> impl IntoView {
    let i18n = use_i18n();
    let api_key = Resource::new_blocking(|| (), |_| server_fns::get_github_api_key());

    view! {
        <div class="page settings-page">
            <div class="rom-header">
                <A href="/more" attr:class="back-btn">
                    {move || t(i18n.locale.get(), "games.back")}
                </A>
                <h2 class="page-title">{move || t(i18n.locale.get(), "github.title")}</h2>
            </div>

            <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }>
                <Suspense fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), "common.loading")}</div> }>
                    {move || Suspend::new(async move {
                        let current = api_key.await?;
                        Ok::<_, ServerFnError>(view! { <ApiKeyForm current /> })
                    })}
                </Suspense>
            </ErrorBoundary>
        </div>
    }
}

#[component]
fn ApiKeyForm(current: String) -> impl IntoView {
    let i18n = use_i18n();

    let key = RwSignal::new(current);
    let saving = RwSignal::new(false);
    let status = RwSignal::new(Option::<(bool, String)>::None);

    let on_save = move |_| {
        saving.set(true);
        status.set(None);
        let value = key.get();

        leptos::task::spawn_local(async move {
            match server_fns::save_github_api_key(value).await {
                Ok(()) => {
                    let locale = use_i18n().locale.get_untracked();
                    status.set(Some((true, t(locale, "settings.saved").to_string())));
                }
                Err(e) => {
                    status.set(Some((false, e.to_string())));
                }
            }
            saving.set(false);
        });
    };

    view! {
        <div class="settings-form">
            <div class="form-field">
                <label class="form-label">{move || t(i18n.locale.get(), "github.label")}</label>
                <input type="password"
                    class="form-input"
                    bind:value=key
                    placeholder="ghp_..."
                    autocomplete="off"
                />
                <p class="form-hint">{move || t(i18n.locale.get(), "github.hint")}</p>
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
                    if saving.get() { t(locale, "settings.saving") } else { t(locale, "settings.save") }
                }}
            </button>
        </div>
    }
}
