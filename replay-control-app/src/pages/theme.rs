use leptos::prelude::*;
use leptos_router::components::A;
use server_fn::ServerFnError;

use crate::i18n::{use_i18n, t};
use crate::pages::ErrorDisplay;
use crate::server_fns;

#[component]
pub fn ThemePage() -> impl IntoView {
    let i18n = use_i18n();
    let skins = Resource::new(|| (), |_| server_fns::get_skins());

    view! {
        <div class="page settings-page">
            <div class="rom-header">
                <A href="/more" attr:class="back-btn">
                    {move || t(i18n.locale.get(), "games.back")}
                </A>
                <h2 class="page-title">{move || t(i18n.locale.get(), "theme.title")}</h2>
            </div>

            <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }>
                <Suspense fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), "common.loading")}</div> }>
                    {move || Suspend::new(async move {
                        let (current, skins) = skins.await?;
                        Ok::<_, ServerFnError>(view! { <SkinGrid current skins /> })
                    })}
                </Suspense>
            </ErrorBoundary>
        </div>
    }
}

#[component]
fn SkinGrid(current: u32, skins: Vec<server_fns::SkinInfo>) -> impl IntoView {
    let i18n = use_i18n();
    let active = RwSignal::new(current);
    let saving = RwSignal::new(false);
    let status = RwSignal::new(Option::<(bool, String)>::None);

    let cards = skins
        .into_iter()
        .map(|skin| {
            view! { <SkinCard skin active saving status /> }
        })
        .collect::<Vec<_>>();

    view! {
        <p class="form-hint">{move || t(i18n.locale.get(), "theme.synced")}</p>
        {move || status.get().map(|(ok, msg)| {
            let class = if ok { "status-msg status-ok" } else { "status-msg status-err" };
            view! { <div class=class>{msg}</div> }
        })}
        <div class="skin-grid">
            {cards}
        </div>
    }
}

#[component]
fn SkinCard(
    skin: server_fns::SkinInfo,
    active: RwSignal<u32>,
    saving: RwSignal<bool>,
    status: RwSignal<Option<(bool, String)>>,
) -> impl IntoView {
    let i18n = use_i18n();
    let index = skin.index;

    let is_active = move || active.get() == index;
    let card_class = move || {
        if is_active() {
            "skin-card skin-card-active"
        } else {
            "skin-card"
        }
    };

    let bg = skin.bg.clone();
    let surface = skin.surface.clone();
    let border = skin.border.clone();
    let text = skin.text.clone();
    let accent = skin.accent.clone();
    let accent_hover = skin.accent_hover.clone();
    let text_secondary = skin.text_secondary.clone();

    let style = format!(
        "background:{surface};border-color:{border};color:{text}"
    );

    let on_click = move |_| {
        if saving.get_untracked() || active.get_untracked() == index {
            return;
        }
        saving.set(true);
        status.set(None);
        leptos::task::spawn_local(async move {
            match server_fns::set_skin(index).await {
                Ok(()) => {
                    active.set(index);
                    let locale = use_i18n().locale.get_untracked();
                    status.set(Some((true, t(locale, "theme.applied").to_string())));
                }
                Err(e) => {
                    status.set(Some((false, e.to_string())));
                }
            }
            saving.set(false);
        });
    };

    view! {
        <button
            class=card_class
            style=style
            on:click=on_click
            disabled=move || saving.get()
        >
            <div class="skin-preview" style=format!("background:{bg}")>
                <div class="skin-preview-bar" style=format!("background:{accent}")></div>
                <div class="skin-preview-bar skin-preview-bar-hover" style=format!("background:{accent_hover}")></div>
                <div class="skin-preview-text" style=format!("color:{text_secondary}")>"Aa"</div>
            </div>
            <div class="skin-name">{skin.name}</div>
            <Show when=is_active>
                <span class="skin-badge">{move || t(i18n.locale.get(), "theme.current")}</span>
            </Show>
        </button>
    }
}
