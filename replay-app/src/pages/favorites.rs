use leptos::prelude::*;
use leptos_router::components::A;
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
            <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }>
                <Suspense fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), "common.loading")}</div> }>
                    {move || Suspend::new(async move {
                        let favs = favorites.await?;
                        Ok::<_, ServerFnError>(view! { <FavoritesContent favs grouped_view toggle_label /> })
                    })}
                </Suspense>
            </ErrorBoundary>
        </div>
    }
}

/// Inner content — full favorites page with hero, recent scroll, system cards, and full list.
#[component]
fn FavoritesContent<F>(
    favs: Vec<Favorite>,
    grouped_view: RwSignal<bool>,
    toggle_label: F,
) -> impl IntoView
where
    F: Fn() -> &'static str + Clone + Send + Sync + 'static,
{
    let i18n = use_i18n();
    let favorites = RwSignal::new(favs);

    // Track which favorite is pending removal confirmation.
    let confirm_remove = RwSignal::new(Option::<String>::None);

    let remove_fav = move |fav_filename: String, subfolder: String| {
        // Optimistically remove from local state.
        favorites.update(|list| {
            list.retain(|f| f.marker_filename != fav_filename);
        });
        confirm_remove.set(None);
        // Call server to persist.
        let sub = if subfolder.is_empty() { None } else { Some(subfolder) };
        leptos::task::spawn_local(async move {
            let _ = server_fns::remove_favorite(fav_filename, sub).await;
        });
    };

    let is_empty = move || favorites.read().is_empty();
    let total_count = move || favorites.read().len();
    let system_count = move || {
        let favs = favorites.read();
        let mut systems = std::collections::HashSet::new();
        for fav in favs.iter() {
            systems.insert(fav.game.system.clone());
        }
        systems.len()
    };

    // Sort by date_added descending to find the most recently added favorites.
    let by_date = move || {
        let favs = favorites.read();
        let mut sorted: Vec<_> = favs.iter().cloned().collect();
        sorted.sort_by(|a, b| b.date_added.cmp(&a.date_added));
        sorted
    };

    // The latest added favorite for the hero card.
    let featured = move || by_date().into_iter().next();

    // Recently added favorites (~10, excluding the featured one), newest-first.
    let recent_items = move || {
        let sorted = by_date();
        sorted.into_iter().skip(1).take(10).collect::<Vec<_>>()
    };

    // System summary: for each system, count and the most recently added favorite.
    let system_cards = move || {
        let favs = favorites.read();
        let mut map: std::collections::BTreeMap<String, (String, String, usize, String, u64)> =
            std::collections::BTreeMap::new();
        for fav in favs.iter() {
            let entry = map
                .entry(fav.game.system.clone())
                .or_insert_with(|| (fav.game.system_display.clone(), fav.game.system.clone(), 0, String::new(), 0));
            entry.2 += 1;
            // Track the most recently added favorite for this system.
            if fav.date_added >= entry.4 {
                entry.3 = fav.game.display_name.clone().unwrap_or_else(|| fav.game.rom_filename.clone());
                entry.4 = fav.date_added;
            }
        }
        map.into_values().collect::<Vec<_>>()
    };

    view! {
        <Show when=move || !is_empty() fallback=move || view! {
            <div class="page-header">
                <h2 class="page-title">{move || t(i18n.locale.get(), "favorites.title")}</h2>
            </div>
            <p class="empty-state">{t(i18n.locale.get(), "favorites.empty")}</p>
        }>
            // Featured / Latest Added — hero card
            <section class="section">
                <h2 class="section-title">{move || t(i18n.locale.get(), "favorites.latest_added")}</h2>
                {move || featured().map(|fav| {
                    let href = format!("/games/{}/{}", fav.game.system, urlencoding::encode(&fav.game.rom_filename));
                    view! {
                        <A href=href attr:class="hero-card rom-name-link">
                            <div class="hero-info">
                                <h3 class="hero-title">{fav.game.display_name.clone().unwrap_or_else(|| fav.game.rom_filename.clone())}</h3>
                                <p class="hero-system">{fav.game.system_display.clone()}</p>
                            </div>
                        </A>
                    }
                })}
            </section>

            // Recently Added — horizontal scroll
            <Show when=move || !recent_items().is_empty()>
                <section class="section">
                    <h2 class="section-title">{move || t(i18n.locale.get(), "favorites.recently_added")}</h2>
                    <div class="recent-scroll">
                        {move || recent_items().into_iter().map(|fav| {
                            let href = format!("/games/{}/{}", fav.game.system, urlencoding::encode(&fav.game.rom_filename));
                            view! {
                                <A href=href attr:class="recent-item rom-name-link">
                                    <div class="recent-name">{fav.game.display_name.clone().unwrap_or_else(|| fav.game.rom_filename.clone())}</div>
                                    <div class="recent-system">{fav.game.system_display.clone()}</div>
                                </A>
                            }
                        }).collect::<Vec<_>>()}
                    </div>
                </section>
            </Show>

            // Stats
            <section class="section">
                <div class="stats-grid">
                    <div class="stat-card">
                        <div class="stat-value">{total_count}</div>
                        <div class="stat-label">{move || t(i18n.locale.get(), "stats.favorites")}</div>
                    </div>
                    <div class="stat-card">
                        <div class="stat-value">{system_count}</div>
                        <div class="stat-label">{move || t(i18n.locale.get(), "stats.systems")}</div>
                    </div>
                </div>
            </section>

            // By System — system cards
            <Show when=move || { system_cards().len() > 1 }>
                <section class="section">
                    <h2 class="section-title">{move || t(i18n.locale.get(), "favorites.by_system")}</h2>
                    <div class="systems-grid">
                        {move || system_cards().into_iter().map(|(display_name, system, count, latest, _)| {
                            let href = format!("/games/{system}");
                            let count_label = move || {
                                let locale = i18n.locale.get();
                                format!("{count} {}", t(locale, "stats.favorites").to_lowercase())
                            };
                            view! {
                                <A href=href attr:class="system-card">
                                    <div class="system-card-name">{display_name}</div>
                                    <div class="system-card-count">{count_label}</div>
                                    <div class="system-card-size">{latest}</div>
                                </A>
                            }
                        }).collect::<Vec<_>>()}
                    </div>
                </section>
            </Show>

            // All Favorites — full list with grouped/flat toggle
            <section class="section">
                <div class="page-header">
                    <h2 class="section-title">{move || t(i18n.locale.get(), "favorites.all")}</h2>
                    <button class="toggle-btn" on:click=move |_| grouped_view.update(|v| *v = !*v)>
                        {toggle_label.clone()}
                    </button>
                </div>

                <Show when=move || grouped_view.get() fallback=move || view! {
                    <FlatFavorites favorites confirm_remove remove_fav />
                }>
                    <GroupedFavorites favorites confirm_remove remove_fav />
                </Show>
            </section>
        </Show>
    }
}

#[component]
fn FlatFavorites<F>(
    favorites: RwSignal<Vec<Favorite>>,
    confirm_remove: RwSignal<Option<String>>,
    remove_fav: F,
) -> impl IntoView
where
    F: Fn(String, String) + Clone + Send + Sync + 'static,
{
    view! {
        <div class="fav-list">
            <For
                each=move || favorites.get()
                key=|fav| fav.marker_filename.clone()
                let:fav
            >
                <FavItem fav show_system=true confirm_remove remove_fav=remove_fav.clone() />
            </For>
        </div>
    }
}

#[component]
fn GroupedFavorites<F>(
    favorites: RwSignal<Vec<Favorite>>,
    confirm_remove: RwSignal<Option<String>>,
    remove_fav: F,
) -> impl IntoView
where
    F: Fn(String, String) + Clone + Send + Sync + 'static,
{
    let groups = move || {
        let favs = favorites.get();
        let mut map: std::collections::BTreeMap<String, Vec<Favorite>> =
            std::collections::BTreeMap::new();
        for fav in favs {
            map.entry(fav.game.system_display.clone()).or_default().push(fav);
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
                                view! { <FavItem fav show_system=false confirm_remove remove_fav /> }
                            }).collect::<Vec<_>>()}
                        </div>
                    }
                }
            </For>
        </div>
    }
}

#[component]
fn FavItem<F>(
    fav: Favorite,
    show_system: bool,
    confirm_remove: RwSignal<Option<String>>,
    remove_fav: F,
) -> impl IntoView
where
    F: Fn(String, String) + Clone + Send + Sync + 'static,
{
    let game_href = format!("/games/{}/{}", fav.game.system, urlencoding::encode(&fav.game.rom_filename));

    let fav_filename = StoredValue::new(fav.marker_filename.clone());
    let subfolder = StoredValue::new(fav.subfolder.clone());
    let rom_name = fav.game.display_name.unwrap_or(fav.game.rom_filename);
    let system_display = if show_system { Some(fav.game.system_display) } else { None };

    let is_confirming = move || {
        confirm_remove.read().as_deref() == Some(&*fav_filename.get_value())
    };

    let on_star_click = move |_| {
        confirm_remove.set(Some(fav_filename.get_value()));
    };

    let remove_fav = StoredValue::new(remove_fav);

    let on_confirm = move |_| {
        let rf = remove_fav.get_value();
        rf(fav_filename.get_value(), subfolder.get_value());
    };

    let on_cancel = move |_| {
        confirm_remove.set(None);
    };

    view! {
        <div class="fav-item">
            <div class="fav-info">
                <A href=game_href attr:class="fav-name rom-name-link">{rom_name}</A>
                {system_display.map(|s| view! { <span class="fav-system">{s}</span> })}
            </div>
            <Show when=is_confirming fallback=move || view! {
                <button class="fav-star-btn" title="Remove from favorites" on:click=on_star_click>
                    {"\u{2605}"}
                </button>
            }>
                <div class="fav-confirm-actions">
                    <button class="rom-action-btn rom-action-confirm-delete" on:click=on_confirm>
                        {"Remove?"}
                    </button>
                    <button class="rom-action-btn" on:click=on_cancel>
                        {"\u{2715}"}
                    </button>
                </div>
            </Show>
        </div>
    }
}
