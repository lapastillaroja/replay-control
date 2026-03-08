use leptos::prelude::*;
use server_fn::ServerFnError;

use crate::i18n::{use_i18n, t};
use crate::pages::ErrorDisplay;
use crate::server_fns;
use crate::server_fns::Favorite;

#[component]
pub fn FavoritesPage() -> impl IntoView {
    let i18n = use_i18n();
    let favorites = Resource::new(|| (), |_| server_fns::get_favorites());
    let grouped_view = RwSignal::new(false);

    let toggle_label = move || {
        let locale = i18n.locale.get();
        if grouped_view.get() {
            t(locale, "favorites.view_flat")
        } else {
            t(locale, "favorites.view_grouped")
        }
    };

    view! {
        <div class="page favorites-page">
            <div class="page-header">
                <h2 class="page-title">{move || t(i18n.locale.get(), "favorites.title")}</h2>
                <button class="toggle-btn" on:click=move |_| grouped_view.update(|v| *v = !*v)>
                    {toggle_label}
                </button>
            </div>

            <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }>
                <Suspense fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), "common.loading")}</div> }>
                    {move || Suspend::new(async move {
                        let favs = favorites.await?;
                        Ok::<_, ServerFnError>(view! { <FavoritesContent favs grouped_view /> })
                    })}
                </Suspense>
            </ErrorBoundary>
        </div>
    }
}

/// Inner content — reactive list of favorites with remove support.
#[component]
fn FavoritesContent(favs: Vec<Favorite>, grouped_view: RwSignal<bool>) -> impl IntoView {
    let i18n = use_i18n();
    let favorites = RwSignal::new(favs);

    let remove_fav = move |fav_filename: String, subfolder: String| {
        // Optimistically remove from local state.
        favorites.update(|list| {
            list.retain(|f| f.filename != fav_filename);
        });
        // Call server to persist.
        let sub = if subfolder.is_empty() { None } else { Some(subfolder) };
        leptos::task::spawn_local(async move {
            let _ = server_fns::remove_favorite(fav_filename, sub).await;
        });
    };

    let is_empty = move || favorites.read().is_empty();

    view! {
        <Show when=move || !is_empty() fallback=move || view! {
            <p class="empty-state">{t(i18n.locale.get(), "favorites.empty")}</p>
        }>
            <Show when=move || grouped_view.get() fallback=move || view! {
                <FlatFavorites favorites remove_fav />
            }>
                <GroupedFavorites favorites remove_fav />
            </Show>
        </Show>
    }
}

#[component]
fn FlatFavorites<F>(favorites: RwSignal<Vec<Favorite>>, remove_fav: F) -> impl IntoView
where
    F: Fn(String, String) + Clone + Send + Sync + 'static,
{
    view! {
        <div class="fav-list">
            <For
                each=move || favorites.get()
                key=|fav| fav.filename.clone()
                let:fav
            >
                <FavItem fav show_system=true remove_fav=remove_fav.clone() />
            </For>
        </div>
    }
}

#[component]
fn GroupedFavorites<F>(favorites: RwSignal<Vec<Favorite>>, remove_fav: F) -> impl IntoView
where
    F: Fn(String, String) + Clone + Send + Sync + 'static,
{
    let groups = move || {
        let favs = favorites.get();
        let mut map: std::collections::BTreeMap<String, Vec<Favorite>> =
            std::collections::BTreeMap::new();
        for fav in favs {
            map.entry(fav.system_display.clone()).or_default().push(fav);
        }
        map.into_iter().collect::<Vec<_>>()
    };

    view! {
        <div class="fav-grouped">
            <For
                each=groups
                key=|(system, _)| system.clone()
                let:group
            >
                {
                    let (system_name, favs) = group;
                    let count = favs.len();
                    let remove_fav = remove_fav.clone();
                    view! {
                        <div class="fav-group">
                            <h3 class="fav-group-title">
                                {system_name} " " <span class="fav-group-count">{"("}{count}{")"}</span>
                            </h3>
                            {favs.into_iter().map(|fav| {
                                let remove_fav = remove_fav.clone();
                                view! { <FavItem fav show_system=false remove_fav /> }
                            }).collect::<Vec<_>>()}
                        </div>
                    }
                }
            </For>
        </div>
    }
}

#[component]
fn FavItem<F>(fav: Favorite, show_system: bool, remove_fav: F) -> impl IntoView
where
    F: Fn(String, String) + Clone + Send + Sync + 'static,
{
    let fav_filename = StoredValue::new(fav.filename.clone());
    let subfolder = StoredValue::new(fav.subfolder.clone());
    let rom_name = fav.rom_filename;
    let system_display = if show_system { Some(fav.system_display) } else { None };

    let on_remove = move |_| {
        remove_fav(fav_filename.get_value(), subfolder.get_value());
    };

    view! {
        <div class="fav-item">
            <div class="fav-info">
                <span class="fav-name">{rom_name}</span>
                {system_display.map(|s| view! { <span class="fav-system">{s}</span> })}
            </div>
            <button class="fav-star-btn" title="Remove from favorites" on:click=on_remove>
                {"\u{2605}"}
            </button>
        </div>
    }
}
