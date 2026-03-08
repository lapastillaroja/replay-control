use leptos::prelude::*;

use crate::i18n::{use_i18n, t};
use crate::server_fns;
use crate::server_fns::Favorite;

#[component]
pub fn FavoritesPage() -> impl IntoView {
    let i18n = use_i18n();
    let favorites = Resource::new(|| (), |_| server_fns::get_favorites());
    let (grouped_view, set_grouped_view) = signal(false);

    let toggle_label = move || {
        let locale = i18n.locale.get();
        if grouped_view.get() {
            t(locale, "favorites.view_grouped")
        } else {
            t(locale, "favorites.view_flat")
        }
    };

    view! {
        <div class="page favorites-page">
            <div class="page-header">
                <h2 class="page-title">{move || t(i18n.locale.get(), "favorites.title")}</h2>
                <button class="toggle-btn" on:click=move |_| set_grouped_view.update(|v| *v = !*v)>
                    {toggle_label}
                </button>
            </div>

            <Suspense fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), "common.loading")}</div> }>
                {move || Suspend::new(async move {
                    let locale = i18n.locale.get();
                    match favorites.await {
                        Ok(favs) => {
                            if favs.is_empty() {
                                view! { <p class="empty-state">{t(locale, "favorites.empty")}</p> }.into_any()
                            } else if grouped_view.get() {
                                view! { <GroupedFavorites favorites=favs /> }.into_any()
                            } else {
                                view! { <FlatFavorites favorites=favs /> }.into_any()
                            }
                        }
                        Err(e) => {
                            view! { <p class="error">{format!("{}: {e}", t(locale, "common.error"))}</p> }.into_any()
                        }
                    }
                })}
            </Suspense>
        </div>
    }
}

#[component]
fn FlatFavorites(favorites: Vec<Favorite>) -> impl IntoView {
    view! {
        <div class="fav-list">
            {favorites.into_iter().map(|fav| {
                view! { <FavItem name=fav.rom_filename.clone() system=Some(fav.system_display.clone()) /> }
            }).collect::<Vec<_>>()}
        </div>
    }
}

#[component]
fn GroupedFavorites(favorites: Vec<Favorite>) -> impl IntoView {
    let mut groups: std::collections::BTreeMap<String, Vec<Favorite>> =
        std::collections::BTreeMap::new();
    for fav in favorites {
        groups.entry(fav.system_display.clone()).or_default().push(fav);
    }

    view! {
        <div class="fav-grouped">
            {groups.into_iter().map(|(system_name, favs)| {
                let count = format!("({})", favs.len());
                view! {
                    <div class="fav-group">
                        <h3 class="fav-group-title">
                            {system_name} " " <span class="fav-group-count">{count}</span>
                        </h3>
                        {favs.into_iter().map(|fav| {
                            view! { <FavItem name=fav.rom_filename.clone() system=None /> }
                        }).collect::<Vec<_>>()}
                    </div>
                }
            }).collect::<Vec<_>>()}
        </div>
    }
}

#[component]
fn FavItem(name: String, system: Option<String>) -> impl IntoView {
    view! {
        <div class="fav-item">
            <div class="fav-info">
                <span class="fav-name">{name}</span>
                {system.map(|s| view! { <span class="fav-system">{s}</span> })}
            </div>
            <span class="fav-star">{"\u{2605}"}</span>
        </div>
    }
}
