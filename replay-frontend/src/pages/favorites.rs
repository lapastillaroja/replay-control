use leptos::prelude::*;

use crate::api;
use crate::types::Favorite;

#[component]
pub fn FavoritesPage() -> impl IntoView {
    let favorites = LocalResource::new(|| api::fetch_favorites());
    let (grouped_view, set_grouped_view) = signal(false);

    view! {
        <div class="page favorites-page">
            <div class="page-header">
                <h2 class="page-title">"Favorites"</h2>
                <button
                    class="toggle-btn"
                    on:click=move |_| set_grouped_view.update(|v| *v = !*v)
                >
                    {move || if grouped_view.get() { "View: Grouped" } else { "View: Flat" }}
                </button>
            </div>

            <Suspense fallback=|| view! { <div class="loading">"Loading..."</div> }>
                {move || {
                    favorites
                        .get()
                        .map(|result| {
                            match &*result {
                                Ok(favs) => {
                                    if favs.is_empty() {
                                        view! {
                                            <p class="empty-state">"No favorites yet"</p>
                                        }
                                            .into_any()
                                    } else if grouped_view.get() {
                                        view! {
                                            <GroupedFavorites favorites=favs.clone() />
                                        }
                                            .into_any()
                                    } else {
                                        view! { <FlatFavorites favorites=favs.clone() /> }
                                            .into_any()
                                    }
                                }
                                Err(e) => {
                                    view! { <p class="error">{format!("Error: {e}")}</p> }
                                        .into_any()
                                }
                            }
                        })
                }}
            </Suspense>
        </div>
    }
}

#[component]
fn FlatFavorites(favorites: Vec<Favorite>) -> impl IntoView {
    view! {
        <div class="fav-list">
            {favorites
                .into_iter()
                .map(|fav| {
                    let name = fav.rom_filename.clone();
                    let sys = fav.system_display.clone();
                    view! {
                        <div class="fav-item">
                            <div class="fav-info">
                                <span class="fav-name">{name}</span>
                                <span class="fav-system">{sys}</span>
                            </div>
                            <span class="fav-star">{"\u{2605}"}</span>
                        </div>
                    }
                })
                .collect::<Vec<_>>()}
        </div>
    }
}

#[component]
fn GroupedFavorites(favorites: Vec<Favorite>) -> impl IntoView {
    let mut groups: std::collections::BTreeMap<String, Vec<Favorite>> =
        std::collections::BTreeMap::new();
    for fav in favorites {
        groups
            .entry(fav.system_display.clone())
            .or_default()
            .push(fav);
    }

    view! {
        <div class="fav-grouped">
            {groups
                .into_iter()
                .map(|(system_name, favs)| {
                    let count = format!("({})", favs.len());
                    view! {
                        <div class="fav-group">
                            <h3 class="fav-group-title">
                                {system_name}
                                " "
                                <span class="fav-group-count">{count}</span>
                            </h3>
                            {favs
                                .into_iter()
                                .map(|fav| {
                                    let name = fav.rom_filename.clone();
                                    view! {
                                        <div class="fav-item">
                                            <div class="fav-info">
                                                <span class="fav-name">{name}</span>
                                            </div>
                                            <span class="fav-star">{"\u{2605}"}</span>
                                        </div>
                                    }
                                })
                                .collect::<Vec<_>>()}
                        </div>
                    }
                })
                .collect::<Vec<_>>()}
        </div>
    }
}
