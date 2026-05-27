use leptos::prelude::*;

use crate::components::filter_chips::{FilterChips, FilterState};
use crate::i18n::{Key, t, use_i18n};

#[component]
pub fn SearchControls(
    query: RwSignal<String>,
    filters: FilterState,
    #[prop(into)] placeholder: Signal<String>,
    #[prop(into)] show_clones: Signal<bool>,
    #[prop(optional)] autofocus: bool,
    #[prop(default = String::new())] filters_class: String,
    #[prop(optional)] genre_dropdown: Option<AnyView>,
    #[prop(optional)] extra_action: Option<AnyView>,
) -> impl IntoView {
    let input_ref = NodeRef::<leptos::html::Input>::new();
    let filter_classes = if filters_class.is_empty() {
        "search-filters".to_string()
    } else {
        format!("search-filters {filters_class}")
    };

    #[cfg(feature = "hydrate")]
    Effect::new(move || {
        if autofocus && let Some(el) = input_ref.get() {
            let _ = el.focus();
        }
    });

    view! {
        <div class="search-page-bar">
            <input
                type="text"
                class="search-page-input"
                node_ref=input_ref
                placeholder=move || placeholder.get()
                prop:value=move || query.get()
                on:input=move |ev| query.set(event_target_value(&ev))
                autofocus=autofocus
            />
        </div>
        <div class=filter_classes>
            <FilterChips filters show_clones />
            {genre_dropdown}
            {extra_action.map(|action| view! {
                <div class="search-controls-action">{action}</div>
            })}
        </div>
    }
}

#[component]
pub fn RandomGameButton(
    loading: RwSignal<bool>,
    on_click: impl Fn(leptos::ev::MouseEvent) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let i18n = use_i18n();

    view! {
        <button
            class="random-game-btn"
            on:click=on_click
            disabled=move || loading.get()
        >
            <span class="random-game-icon">{"\u{1F3B2}"}</span>
            " "
            {move || if loading.get() {
                t(i18n.locale.get(), Key::CommonLoading)
            } else {
                t(i18n.locale.get(), Key::SearchRandomGame)
            }}
        </button>
    }
}
