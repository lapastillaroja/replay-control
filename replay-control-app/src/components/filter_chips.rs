use leptos::prelude::*;

use crate::components::genre_dropdown::GenreDropdown;
use crate::i18n::{Key, t, use_i18n};

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
    let value = move || min_rating.get().map(|v| v.to_string()).unwrap_or_default();

    // Capture initial value for `selected` attributes (same reason as GenreDropdown).
    let initial = min_rating
        .get_untracked()
        .map(|v| v.to_string())
        .unwrap_or_default();

    let select_class = move || {
        if min_rating.read().is_some() {
            "filter-genre-select filter-select-active"
        } else {
            "filter-genre-select"
        }
    };

    view! {
        <select
            class=select_class
            on:change=on_change
            prop:value=value
        >
            <option value="" selected=initial.is_empty()>{move || t(i18n.locale.get(), Key::FilterRatingAny)}</option>
            <option value="3" selected={initial == "3"}>"3+"</option>
            <option value="3.5" selected={initial == "3.5"}>"3.5+"</option>
            <option value="4" selected={initial == "4"}>"4+"</option>
            <option value="4.5" selected={initial == "4.5"}>"4.5+"</option>
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
    pub min_year: RwSignal<Option<u16>>,
    pub max_year: RwSignal<Option<u16>>,
}

impl FilterState {
    /// Build a `FilterState` from URL query parameters.
    ///
    /// Reads `hide_hacks`, `hide_translations`, `hide_betas`, `hide_clones`,
    /// `genre`, `multiplayer`, `min_rating`, `min_year`, and `max_year` from
    /// the provided `ParamsMap`.
    pub fn from_query_map(qm: &leptos_router::params::ParamsMap) -> Self {
        let bool_param = |key: &str| qm.get(key).is_some_and(|v| v == "true");
        let genre = qm.get("genre").unwrap_or_default();
        let min_rating = qm.get("min_rating").and_then(|v| v.parse::<f32>().ok());
        let min_year = qm.get("min_year").and_then(|v| v.parse::<u16>().ok());
        let max_year = qm.get("max_year").and_then(|v| v.parse::<u16>().ok());

        Self {
            hide_hacks: RwSignal::new(bool_param("hide_hacks")),
            hide_translations: RwSignal::new(bool_param("hide_translations")),
            hide_betas: RwSignal::new(bool_param("hide_betas")),
            hide_clones: RwSignal::new(bool_param("hide_clones")),
            multiplayer_only: RwSignal::new(bool_param("multiplayer")),
            genre: RwSignal::new(genre.clone()),
            min_rating: RwSignal::new(min_rating),
            min_year: RwSignal::new(min_year),
            max_year: RwSignal::new(max_year),
        }
    }

    /// Return the initial genre value (convenience for creating a debounced_genre signal).
    pub fn genre_untracked(&self) -> String {
        self.genre.get_untracked()
    }
}

/// A single toggle filter chip.
#[component]
fn FilterChip(signal: RwSignal<bool>, label_key: Key) -> impl IntoView {
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
        <FilterChip signal=filters.hide_hacks label_key=Key::FilterHideHacks />
        <FilterChip signal=filters.hide_translations label_key=Key::FilterHideTranslations />
        <FilterChip signal=filters.hide_betas label_key=Key::FilterHideBetas />
        <Show when=move || show_clones.get()>
            <FilterChip signal=filters.hide_clones label_key=Key::FilterHideClones />
        </Show>
        <FilterChip signal=filters.multiplayer_only label_key=Key::FilterMultiplayer />
        {genre_list.map(|gl| {
            view! { <GenreDropdown genre=filters.genre genre_list=gl /> }
        })}
        <RatingDropdown min_rating=filters.min_rating />
    }
}
