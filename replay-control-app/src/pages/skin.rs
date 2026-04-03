use leptos::prelude::*;
use leptos_router::components::A;
use server_fn::ServerFnError;

use crate::i18n::{t, use_i18n, Key};
use crate::server_fns;

#[component]
pub fn SkinPage() -> impl IntoView {
    let i18n = use_i18n();
    let skins = Resource::new_blocking(|| (), |_| server_fns::get_skins());

    view! {
        <div class="page settings-page">
            <div class="rom-header">
                <A href="/more" attr:class="back-btn">
                    {move || t(i18n.locale.get(), Key::GamesBack)}
                </A>
                <h2 class="page-title">{move || t(i18n.locale.get(), Key::SkinTitle)}</h2>
            </div>

            <Suspense fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), Key::CommonLoading)}</div> }>
                {move || Suspend::new(async move {
                    let (current, sync, skins) = skins.await?;
                    Ok::<_, ServerFnError>(view! { <SkinGrid current sync skins /> })
                })}
            </Suspense>
        </div>
    }
}

#[component]
fn SkinGrid(current: u32, sync: bool, skins: Vec<server_fns::SkinInfo>) -> impl IntoView {
    let i18n = use_i18n();
    let active = RwSignal::new(current);
    let sync_enabled = RwSignal::new(sync);
    let saving = RwSignal::new(false);
    let status = RwSignal::new(Option::<(bool, String)>::None);

    // Keep a lookup so we can apply skin CSS when toggling sync.

    let on_toggle_sync = move |_| {
        let new_sync = !sync_enabled.get_untracked();
        saving.set(true);
        status.set(None);
        leptos::task::spawn_local(async move {
            match server_fns::set_skin_sync(new_sync).await {
                Ok(()) => {
                    sync_enabled.set(new_sync);
                    #[cfg(feature = "hydrate")]
                    if let Some(window) = web_sys::window() {
                        let _ = window.location().reload();
                    }
                }
                Err(e) => {
                    status.set(Some((false, e.to_string())));
                }
            }
            saving.set(false);
        });
    };

    let cards = skins
        .into_iter()
        .map(|skin| {
            view! { <SkinCard skin active sync_enabled saving status /> }
        })
        .collect::<Vec<_>>();

    view! {
        <div class="form-field form-field-check">
            <label class="form-label">{move || t(i18n.locale.get(), Key::SkinSync)}</label>
            <input type="checkbox"
                class="form-checkbox"
                prop:checked=move || sync_enabled.get()
                on:change=on_toggle_sync
                disabled=move || saving.get()
            />
        </div>
        <p class="form-hint">{move || {
            let locale = i18n.locale.get();
            if sync_enabled.get() {
                t(locale, Key::SkinSyncHint)
            } else {
                t(locale, Key::SkinHint)
            }
        }}</p>
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
    sync_enabled: RwSignal<bool>,
    saving: RwSignal<bool>,
    status: RwSignal<Option<(bool, String)>>,
) -> impl IntoView {
    let i18n = use_i18n();
    let index = skin.index;

    let is_active = move || active.get() == index;
    let is_disabled = move || saving.get() || sync_enabled.get();
    let card_class = move || {
        if is_active() {
            "skin-card skin-card-active"
        } else {
            "skin-card"
        }
    };

    let style = format!(
        "background:{};border-color:{};color:{}",
        skin.surface, skin.border, skin.text
    );

    let bg = skin.bg.clone();
    let accent = skin.accent.clone();
    let accent_hover = skin.accent_hover.clone();
    let text_secondary = skin.text_secondary.clone();

    let on_click = move |_| {
        if is_disabled() || active.get_untracked() == index {
            return;
        }
        saving.set(true);
        status.set(None);
        leptos::task::spawn_local(async move {
            match server_fns::set_skin(index).await {
                Ok(()) => {
                    // Reload the page so the SSR-rendered skin theme
                    // style tag picks up the new skin. Simpler and more
                    // reliable than replicating CSS variable logic client-side.
                    #[cfg(feature = "hydrate")]
                    if let Some(window) = web_sys::window() {
                        let _ = window.location().reload();
                    }
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
            disabled=is_disabled
        >
            <div class="skin-preview" style=format!("background:{bg}")>
                <div class="skin-preview-bar" style=format!("background:{accent}")></div>
                <div class="skin-preview-bar skin-preview-bar-hover" style=format!("background:{accent_hover}")></div>
                <div class="skin-preview-text" style=format!("color:{text_secondary}")>"Aa"</div>
            </div>
            <div class="skin-name">{skin.name}</div>
            <Show when=is_active>
                <span class="skin-badge">{move || t(i18n.locale.get(), Key::SkinCurrent)}</span>
            </Show>
        </button>
    }
}
