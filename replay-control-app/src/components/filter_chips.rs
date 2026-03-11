use leptos::prelude::*;

use crate::components::genre_dropdown::GenreDropdown;
use crate::i18n::{t, use_i18n};

/// Rating filter dropdown — "Any", "3+", "3.5+", "4+", "4.5+".
#[component]
fn RatingDropdown(min_rating: RwSignal<Option<f32>>) -> impl IntoView {
    let i18n = use_i18n();
    let on_change = move |ev: leptos::ev::Event| {
        let val = event_target_value(&ev);
        min_rating.set(if val.is_empty() {
            None
        } else {
            val.parse::<f32>().ok()
        });
    };
    let value = move || {
        min_rating.get().map(|v| v.to_string()).unwrap_or_default()
    };

    view! {
        <select
            class="filter-genre-select"
            on:change=on_change
            prop:value=value
        >
            <option value="">{move || t(i18n.locale.get(), "filter.rating_any")}</option>
            <option value="3">"3+"</option>
            <option value="3.5">"3.5+"</option>
            <option value="4">"4+"</option>
            <option value="4.5">"4.5+"</option>
        </select>
    }
}

/// Shared filter state used by both the ROM list and global search pages.
#[derive(Clone, Copy)]
pub struct FilterState {
    pub hide_hacks: RwSignal<bool>,
    pub hide_translations: RwSignal<bool>,
    pub hide_betas: RwSignal<bool>,
    pub hide_clones: RwSignal<bool>,
    pub multiplayer_only: RwSignal<bool>,
    pub genre: RwSignal<String>,
    pub min_rating: RwSignal<Option<f32>>,
}

/// A single toggle filter chip.
#[component]
fn FilterChip(
    signal: RwSignal<bool>,
    label_key: &'static str,
) -> impl IntoView {
    let i18n = use_i18n();

    view! {
        <button
            class=move || {
                if signal.get() {
                    "filter-chip filter-chip-active"
                } else {
                    "filter-chip"
                }
            }
            on:click=move |_| signal.update(|v| *v = !*v)
        >
            {move || t(i18n.locale.get(), label_key)}
            {move || if signal.get() { " \u{2715}" } else { "" }}
        </button>
    }
}

/// Renders the shared set of filter chips (hacks, translations, betas, clones, multiplayer)
/// plus an optional genre dropdown.
///
/// - `show_clones`: controls whether the "hide clones" chip is shown (typically
///   gated on `is_arcade` in rom_list, always shown in search).
/// - `genre_list`: when `Some`, renders a `GenreDropdown` at the end.
#[component]
pub fn FilterChips(
    filters: FilterState,
    #[prop(into)] show_clones: Signal<bool>,
    #[prop(optional)] genre_list: Option<Vec<String>>,
) -> impl IntoView {
    view! {
        <FilterChip signal=filters.hide_hacks label_key="filter.hide_hacks" />
        <FilterChip signal=filters.hide_translations label_key="filter.hide_translations" />
        <FilterChip signal=filters.hide_betas label_key="filter.hide_betas" />
        <Show when=move || show_clones.get()>
            <FilterChip signal=filters.hide_clones label_key="filter.hide_clones" />
        </Show>
        <FilterChip signal=filters.multiplayer_only label_key="filter.multiplayer" />
        {genre_list.map(|gl| {
            view! { <GenreDropdown genre=filters.genre genre_list=gl /> }
        })}
        <RatingDropdown min_rating=filters.min_rating />
    }
}
