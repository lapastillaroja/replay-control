use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_params_map;
use server_fn::ServerFnError;

use crate::components::hero_card::{GameScrollCard, HeroCard};
use crate::i18n::{t, use_i18n};
use crate::pages::ErrorDisplay;
use crate::server_fns;
use crate::server_fns::{FavoriteWithArt, OrganizeCriteria};

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
    favs: Vec<FavoriteWithArt>,
    grouped_view: RwSignal<bool>,
    toggle_label: F,
) -> impl IntoView
where
    F: Fn() -> &'static str + Clone + Send + Sync + 'static,
{
    let i18n = use_i18n();
    let favorites = RwSignal::new(favs);

    // Filter input for the "All Favorites" section.
    let filter_text = RwSignal::new(String::new());

    let filtered_favorites = move || {
        let query = filter_text.get().to_lowercase();
        if query.is_empty() {
            return favorites.get();
        }
        favorites
            .get()
            .into_iter()
            .filter(|f| {
                let name = f
                    .fav
                    .game
                    .display_name
                    .as_deref()
                    .unwrap_or(&f.fav.game.rom_filename);
                name.to_lowercase().contains(&query)
            })
            .collect()
    };

    // Track which favorite is pending removal confirmation.
    let confirm_remove = RwSignal::new(Option::<String>::None);

    let remove_fav = move |fav_filename: String, subfolder: String| {
        // Optimistically remove from local state.
        favorites.update(|list| {
            list.retain(|f| f.fav.marker_filename != fav_filename);
        });
        confirm_remove.set(None);
        // Call server to persist.
        let sub = if subfolder.is_empty() {
            None
        } else {
            Some(subfolder)
        };
        leptos::task::spawn_local(async move {
            let _ = server_fns::remove_favorite(fav_filename, sub).await;
        });
    };

    let is_empty = move || favorites.read().is_empty();
    let total_count = move || favorites.read().len();
    let system_count = move || {
        let favs = favorites.read();
        let mut systems = std::collections::HashSet::new();
        for f in favs.iter() {
            systems.insert(f.fav.game.system.clone());
        }
        systems.len()
    };

    // Sort by date_added descending to find the most recently added favorites.
    let by_date = move || {
        let favs = favorites.read();
        let mut sorted: Vec<_> = favs.iter().cloned().collect();
        sorted.sort_by(|a, b| b.fav.date_added.cmp(&a.fav.date_added));
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
        for f in favs.iter() {
            let entry = map.entry(f.fav.game.system.clone()).or_insert_with(|| {
                (
                    f.fav.game.system_display.clone(),
                    f.fav.game.system.clone(),
                    0,
                    String::new(),
                    0,
                )
            });
            entry.2 += 1;
            // Track the most recently added favorite for this system.
            if f.fav.date_added >= entry.4 {
                entry.3 = f.fav
                    .game
                    .display_name
                    .clone()
                    .unwrap_or_else(|| f.fav.game.rom_filename.clone());
                entry.4 = f.fav.date_added;
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
                {move || featured().map(|f| {
                    let href = format!("/games/{}/{}", f.fav.game.system, urlencoding::encode(&f.fav.game.rom_filename));
                    let name = f.fav.game.display_name.clone().unwrap_or_else(|| f.fav.game.rom_filename.clone());
                    let system = f.fav.game.system_display.clone();
                    let box_art_url = f.box_art_url.clone();
                    view! {
                        <HeroCard href name system box_art_url />
                    }
                })}
            </section>

            // Recently Added — horizontal scroll
            <Show when=move || !recent_items().is_empty()>
                <section class="section">
                    <h2 class="section-title">{move || t(i18n.locale.get(), "favorites.recently_added")}</h2>
                    <div class="recent-scroll">
                        {move || recent_items().into_iter().map(|f| {
                            let href = format!("/games/{}/{}", f.fav.game.system, urlencoding::encode(&f.fav.game.rom_filename));
                            let name = f.fav.game.display_name.clone().unwrap_or_else(|| f.fav.game.rom_filename.clone());
                            let system = f.fav.game.system_display.clone();
                            let box_art_url = f.box_art_url.clone();
                            view! {
                                <GameScrollCard href name system box_art_url />
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

            // Organize panel
            <OrganizePanel favorites />

            // By System — system cards
            <Show when=move || { system_cards().len() > 1 }>
                <section class="section">
                    <h2 class="section-title">{move || t(i18n.locale.get(), "favorites.by_system")}</h2>
                    <div class="systems-grid">
                        {move || system_cards().into_iter().map(|(display_name, system, count, latest, _)| {
                            let href = format!("/favorites/{system}");
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

                <div class="fav-filter-bar">
                    <div class="fav-filter-input-wrap">
                        <input
                            type="text"
                            class="fav-filter-input"
                            placeholder="Filter favorites..."
                            prop:value=move || filter_text.get()
                            on:input=move |ev| filter_text.set(event_target_value(&ev))
                        />
                        <Show when=move || !filter_text.get().is_empty()>
                            <button
                                class="fav-filter-clear"
                                on:click=move |_| filter_text.set(String::new())
                            >
                                {"\u{2715}"}
                            </button>
                        </Show>
                    </div>
                    <span class="fav-filter-count">
                        {
                            let filtered_for_count = filtered_favorites.clone();
                            move || {
                                let filtered = filtered_for_count().len();
                                let total = total_count();
                                if filter_text.read().is_empty() {
                                    format!("{total} {}", t(i18n.locale.get(), "stats.favorites").to_lowercase())
                                } else {
                                    format!("{filtered} / {total} {}", t(i18n.locale.get(), "stats.favorites").to_lowercase())
                                }
                            }
                        }
                    </span>
                </div>

                {
                    let filtered_signal = Signal::derive(filtered_favorites.clone());
                    let filtered_signal2 = Signal::derive(filtered_favorites.clone());
                    view! {
                        <Show when=move || grouped_view.get() fallback=move || view! {
                            <FlatFavorites favorites=filtered_signal confirm_remove remove_fav />
                        }>
                            <GroupedFavorites favorites=filtered_signal2 confirm_remove remove_fav />
                        </Show>
                    }
                }
            </section>
        </Show>
    }
}

#[component]
fn FlatFavorites<F>(
    #[prop(into)] favorites: Signal<Vec<FavoriteWithArt>>,
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
                key=|f| f.fav.marker_filename.clone()
                let:f
            >
                <FavItem fav=f.fav box_art_url=f.box_art_url show_system=true confirm_remove remove_fav=remove_fav.clone() />
            </For>
        </div>
    }
}

#[component]
fn GroupedFavorites<F>(
    #[prop(into)] favorites: Signal<Vec<FavoriteWithArt>>,
    confirm_remove: RwSignal<Option<String>>,
    remove_fav: F,
) -> impl IntoView
where
    F: Fn(String, String) + Clone + Send + Sync + 'static,
{
    let groups = move || {
        let favs = favorites.get();
        let mut map: std::collections::BTreeMap<String, Vec<FavoriteWithArt>> =
            std::collections::BTreeMap::new();
        for f in favs {
            map.entry(f.fav.game.system_display.clone())
                .or_default()
                .push(f);
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
                            {favs.into_iter().map(|f| {
                                let remove_fav = remove_fav.clone();
                                view! { <FavItem fav=f.fav box_art_url=f.box_art_url show_system=false confirm_remove remove_fav /> }
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
    fav: crate::server_fns::Favorite,
    box_art_url: Option<String>,
    show_system: bool,
    confirm_remove: RwSignal<Option<String>>,
    remove_fav: F,
) -> impl IntoView
where
    F: Fn(String, String) + Clone + Send + Sync + 'static,
{
    let game_href = format!(
        "/games/{}/{}",
        fav.game.system,
        urlencoding::encode(&fav.game.rom_filename)
    );
    let game_href = StoredValue::new(game_href);

    let has_box_art = box_art_url.is_some();
    let box_art = StoredValue::new(box_art_url);
    let fav_filename = StoredValue::new(fav.marker_filename.clone());
    let subfolder = StoredValue::new(fav.subfolder.clone());
    let rom_name = fav.game.display_name.unwrap_or(fav.game.rom_filename);
    let system_display = if show_system {
        Some(fav.game.system_display)
    } else {
        None
    };

    let is_confirming =
        move || confirm_remove.read().as_deref() == Some(&*fav_filename.get_value());

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
            <A href=game_href.get_value() attr:class="rom-thumb-link">
                <Show when=move || has_box_art fallback=|| view! { <div class="rom-thumb-placeholder"></div> }>
                    <img class="rom-thumb" src=box_art.get_value() loading="lazy" />
                </Show>
            </A>
            <div class="fav-info">
                <A href=game_href.get_value() attr:class="fav-name rom-name-link">{rom_name}</A>
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

/// Collapsible panel for organizing favorites into subfolders.
#[component]
fn OrganizePanel(favorites: RwSignal<Vec<FavoriteWithArt>>) -> impl IntoView {
    let i18n = use_i18n();
    let expanded = RwSignal::new(false);
    let primary = RwSignal::new("genre".to_string());
    let secondary = RwSignal::new("none".to_string());
    let keep_originals = RwSignal::new(true);
    let busy = RwSignal::new(false);
    let status = RwSignal::new(Option::<(bool, String)>::None);

    // Reset secondary when primary changes to the same value.
    Effect::new(move || {
        let p = primary.get();
        if secondary.get_untracked() == p {
            secondary.set("none".to_string());
        }
    });

    let on_organize = move |_| {
        busy.set(true);
        status.set(None);
        let p = parse_criteria(&primary.get());
        let s = parse_criteria(&secondary.get());
        let keep = keep_originals.get();

        if let Some(p) = p {
            leptos::task::spawn_local(async move {
                match server_fns::organize_favorites(p, s, keep).await {
                    Ok(result) => {
                        let locale = use_i18n().locale.get_untracked();
                        let msg = format!("{} {}", result.organized, t(locale, "organize.done"));
                        status.set(Some((true, msg)));
                        // Reload favorites.
                        if let Ok(new_favs) = server_fns::get_favorites().await {
                            favorites.set(new_favs);
                        }
                    }
                    Err(e) => status.set(Some((false, e.to_string()))),
                }
                busy.set(false);
            });
        }
    };

    let on_flatten = move |_| {
        busy.set(true);
        status.set(None);
        leptos::task::spawn_local(async move {
            match server_fns::flatten_favorites().await {
                Ok(count) => {
                    let locale = use_i18n().locale.get_untracked();
                    let msg = if count == 0 {
                        t(locale, "organize.already_flat").to_string()
                    } else {
                        format!("{count} {}", t(locale, "organize.flattened"))
                    };
                    status.set(Some((true, msg)));
                    if let Ok(new_favs) = server_fns::get_favorites().await {
                        favorites.set(new_favs);
                    }
                }
                Err(e) => status.set(Some((false, e.to_string()))),
            }
            busy.set(false);
        });
    };

    view! {
        <section class="section">
            <button
                class="toggle-btn organize-toggle"
                on:click=move |_| expanded.update(|v| *v = !*v)
            >
                <span class="organize-toggle-icon">{move || if expanded.get() { "\u{25BC}" } else { "\u{25B6}" }}</span>
                {move || t(i18n.locale.get(), "organize.title")}
            </button>

            <Show when=move || expanded.get()>
                <div class="organize-panel">
                    <div class="form-field">
                        <label class="form-label">{move || t(i18n.locale.get(), "organize.primary")}</label>
                        <select class="form-input" bind:value=primary>
                            <option value="genre">{move || t(i18n.locale.get(), "organize.genre")}</option>
                            <option value="system">{move || t(i18n.locale.get(), "organize.system")}</option>
                            <option value="players">{move || t(i18n.locale.get(), "organize.players")}</option>
                            <option value="rating">{move || t(i18n.locale.get(), "organize.rating")}</option>
                            <option value="alphabetical">{move || t(i18n.locale.get(), "organize.alphabetical")}</option>
                        </select>
                    </div>

                    <div class="form-field">
                        <label class="form-label">{move || t(i18n.locale.get(), "organize.secondary")}</label>
                        {move || {
                            let p = primary.get();
                            let options: Vec<(&str, &str)> = [
                                ("genre", "organize.genre"),
                                ("system", "organize.system"),
                                ("players", "organize.players"),
                                ("rating", "organize.rating"),
                                ("alphabetical", "organize.alphabetical"),
                            ]
                            .into_iter()
                            .filter(|(val, _)| *val != p.as_str())
                            .collect();

                            view! {
                                <select class="form-input" bind:value=secondary>
                                    <option value="none">{t(i18n.locale.get(), "organize.none")}</option>
                                    {options.into_iter().map(|(val, key)| {
                                        let label = t(i18n.locale.get(), key);
                                        view! { <option value=val>{label}</option> }
                                    }).collect::<Vec<_>>()}
                                </select>
                            }
                        }}
                    </div>

                    <div class="form-field form-field-check">
                        <div>
                            <label class="form-label">{move || t(i18n.locale.get(), "organize.keep_originals")}</label>
                            <p class="form-hint">{move || t(i18n.locale.get(), "organize.keep_hint")}</p>
                        </div>
                        <input type="checkbox"
                            class="form-checkbox"
                            bind:checked=keep_originals
                        />
                    </div>

                    {move || status.get().map(|(ok, msg)| {
                        let class = if ok { "status-msg status-ok" } else { "status-msg status-err" };
                        view! { <div class=class>{msg}</div> }
                    })}

                    <div class="organize-actions">
                        <button
                            class="form-btn"
                            on:click=on_organize
                            disabled=move || busy.get()
                        >
                            {move || {
                                let locale = i18n.locale.get();
                                if busy.get() { t(locale, "organize.organizing") } else { t(locale, "organize.apply") }
                            }}
                        </button>
                        <button
                            class="form-btn form-btn-secondary"
                            on:click=on_flatten
                            disabled=move || busy.get()
                        >
                            {move || {
                                let locale = i18n.locale.get();
                                if busy.get() { t(locale, "organize.flattening") } else { t(locale, "organize.flatten") }
                            }}
                        </button>
                    </div>
                </div>
            </Show>
        </section>
    }
}

fn parse_criteria(value: &str) -> Option<OrganizeCriteria> {
    match value {
        "system" => Some(OrganizeCriteria::System),
        "genre" => Some(OrganizeCriteria::Genre),
        "players" => Some(OrganizeCriteria::Players),
        "rating" => Some(OrganizeCriteria::Rating),
        "alphabetical" => Some(OrganizeCriteria::Alphabetical),
        _ => None,
    }
}

/// `/favorites/:system` — favorites list filtered to a single system.
#[component]
pub fn SystemFavoritesPage() -> impl IntoView {
    let i18n = use_i18n();
    let params = use_params_map();
    let system = move || params.read().get("system").unwrap_or_default();

    let favorites = Resource::new(
        move || system(),
        |sys| server_fns::get_system_favorites(sys),
    );

    view! {
        <div class="page favorites-page">
            <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }>
                <Suspense fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), "common.loading")}</div> }>
                    {move || Suspend::new(async move {
                        let favs = favorites.await?;
                        Ok::<_, ServerFnError>(view! { <SystemFavoritesContent favs /> })
                    })}
                </Suspense>
            </ErrorBoundary>
        </div>
    }
}

/// Inner content for the system-specific favorites page.
#[component]
fn SystemFavoritesContent(favs: Vec<FavoriteWithArt>) -> impl IntoView {
    let i18n = use_i18n();
    let favorites = RwSignal::new(favs);
    let confirm_remove = RwSignal::new(Option::<String>::None);

    // Derive the system display name from the first favorite.
    let system_display = favorites
        .read()
        .first()
        .map(|f| f.fav.game.system_display.clone())
        .unwrap_or_default();

    let total_count = move || favorites.read().len();
    let is_empty = move || favorites.read().is_empty();

    let remove_fav = move |fav_filename: String, subfolder: String| {
        favorites.update(|list| {
            list.retain(|f| f.fav.marker_filename != fav_filename);
        });
        confirm_remove.set(None);
        let sub = if subfolder.is_empty() {
            None
        } else {
            Some(subfolder)
        };
        leptos::task::spawn_local(async move {
            let _ = server_fns::remove_favorite(fav_filename, sub).await;
        });
    };

    view! {
        <div class="rom-header">
            <A href="/favorites" attr:class="back-btn">
                {move || t(i18n.locale.get(), "games.back")}
            </A>
            <h2 class="page-title">{system_display}</h2>
        </div>
        <p class="rom-count">{move || {
            let count = total_count();
            let locale = i18n.locale.get();
            format!("{count} {}", t(locale, "stats.favorites").to_lowercase())
        }}</p>
        <Show when=move || !is_empty() fallback=move || view! {
            <p class="empty-state">{t(i18n.locale.get(), "favorites.empty")}</p>
        }>
            <div class="fav-list">
                <For
                    each=move || favorites.get()
                    key=|f| f.fav.marker_filename.clone()
                    let:f
                >
                    <FavItem fav=f.fav box_art_url=f.box_art_url show_system=false confirm_remove remove_fav=remove_fav.clone() />
                </For>
            </div>
        </Show>
    }
}
