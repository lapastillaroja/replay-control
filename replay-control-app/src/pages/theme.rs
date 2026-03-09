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

    let cards = skins
        .into_iter()
        .map(|skin| {
            let is_current = skin.index == current;
            view! { <SkinCard skin is_current /> }
        })
        .collect::<Vec<_>>();

    view! {
        <p class="form-hint">{move || t(i18n.locale.get(), "theme.synced")}</p>
        <div class="skin-grid">
            {cards}
        </div>
    }
}

#[component]
fn SkinCard(skin: server_fns::SkinInfo, is_current: bool) -> impl IntoView {
    let i18n = use_i18n();
    let card_class = if is_current {
        "skin-card skin-card-active"
    } else {
        "skin-card"
    };

    view! {
        <div
            class=card_class
            style=format!(
                "background:{};border-color:{};color:{}",
                skin.surface, skin.border, skin.text
            )
        >
            <div class="skin-preview" style=format!("background:{}", skin.bg)>
                <div class="skin-preview-bar" style=format!("background:{}", skin.accent)></div>
                <div class="skin-preview-bar skin-preview-bar-hover" style=format!("background:{}", skin.accent_hover)></div>
                <div class="skin-preview-text" style=format!("color:{}", skin.text_secondary)>"Aa"</div>
            </div>
            <div class="skin-name">{skin.name}</div>
            <Show when=move || is_current>
                <span class="skin-badge">{move || t(i18n.locale.get(), "theme.current")}</span>
            </Show>
        </div>
    }
}
