use leptos::prelude::*;

use crate::i18n::{t, use_i18n};

/// Genre dropdown filter — shared between ROM list and global search pages.
#[component]
pub fn GenreDropdown(genre: RwSignal<String>, genre_list: Vec<String>) -> impl IntoView {
    let i18n = use_i18n();

    view! {
        <select
            class="filter-genre-select"
            on:change=move |ev| genre.set(event_target_value(&ev))
            prop:value=move || genre.get()
        >
            <option value="">{move || t(i18n.locale.get(), "filter.genre_all")}</option>
            {genre_list
                .into_iter()
                .map(|g| {
                    let g2 = g.clone();
                    view! { <option value=g>{g2}</option> }
                })
                .collect::<Vec<_>>()}
        </select>
    }
}
