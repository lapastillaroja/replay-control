use leptos::prelude::*;

use crate::i18n::{Key, t, use_i18n};

/// Genre dropdown filter — shared between ROM list and global search pages.
#[component]
pub fn GenreDropdown(genre: RwSignal<String>, genre_list: Vec<String>) -> impl IntoView {
    let i18n = use_i18n();

    // Capture the initial value so we can mark the matching <option> as
    // `selected` in the SSR-rendered HTML. Without this, the browser defaults
    // to the first option because `prop:value` only runs on the client and
    // its RenderEffect fires before children are in the DOM.
    let initial = genre.get_untracked();

    let select_class = move || {
        if genre.read().is_empty() {
            "filter-genre-select"
        } else {
            "filter-genre-select filter-select-active"
        }
    };

    view! {
        <select
            class=select_class
            on:change=move |ev| genre.set(event_target_value(&ev))
            prop:value=move || genre.get()
        >
            <option value="" selected=initial.is_empty()>
                {move || t(i18n.locale.get(), Key::FilterGenreAll)}
            </option>
            {genre_list
                .into_iter()
                .map(|g| {
                    let is_selected = g == initial;
                    let g2 = g.clone();
                    view! { <option value=g selected=is_selected>{g2}</option> }
                })
                .collect::<Vec<_>>()}
        </select>
    }
}
