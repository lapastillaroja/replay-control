use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_params_map;
use server_fn::ServerFnError;

use crate::components::boxart_placeholder::BoxArtPlaceholder;
use crate::components::game_section_row::GameSectionRow;
use crate::components::hero_card::{GameScrollCard, HeroCard};
use crate::i18n::{t, tf, use_i18n, Key};
use crate::server_fns;
use crate::server_fns::{FavoriteWithArt, GameSection, OrganizeCriteria};

#[component]
pub fn FavoritesPage() -> impl IntoView {
    let i18n = use_i18n();
    let favorites = Resource::new(|| (), |_| server_fns::get_favorites());
    let recommendations = Resource::new(|| (), |_| server_fns::get_favorites_recommendations());
    let grouped_view = RwSignal::new(false);

    let toggle_label = move || {
        let locale = i18n.locale.get();
        if grouped_view.get() {
            t(locale, Key::FavoritesViewFlat)
        } else {
            t(locale, Key::FavoritesViewGrouped)
        }
    };

    view! {
        <div class="page favorites-page">
            <Suspense fallback=move || view! { <FavoritesPageSkeleton /> }>
                {move || Suspend::new(async move {
                    let favs = favorites.await?;
                    Ok::<_, ServerFnError>(view! { <FavoritesContent favs grouped_view toggle_label recommendations /> })
                })}
            </Suspense>
        </div>
    }
}

/// Inner content — full favorites page with hero, recent scroll, system cards, and full list.
#[component]
fn FavoritesContent<F>(
    favs: Vec<FavoriteWithArt>,
    grouped_view: RwSignal<bool>,
    toggle_label: F,
    recommendations: Resource<Result<Vec<GameSection>, ServerFnError>>,
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
                entry.3 = f
                    .fav
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
                <h2 class="page-title">{move || t(i18n.locale.get(), Key::FavoritesTitle)}</h2>
            </div>
            <p class="empty-state">{t(i18n.locale.get(), Key::FavoritesEmpty)}</p>
        }>
            // Featured / Latest Added — hero card
            <section class="section">
                <h2 class="section-title">{move || t(i18n.locale.get(), Key::FavoritesLatestAdded)}</h2>
                {move || featured().map(|f| {
                    let href = format!("/games/{}/{}", f.fav.game.system, urlencoding::encode(&f.fav.game.rom_filename));
                    let name = f.fav.game.display_name.clone().unwrap_or_else(|| f.fav.game.rom_filename.clone());
                    let system = f.fav.game.system_display.clone();
                    let system_folder = f.fav.game.system.clone();
                    let box_art_url = f.box_art_url.clone();
                    view! {
                        <HeroCard href name system system_folder box_art_url />
                    }
                })}
            </section>

            // Recently Added — horizontal scroll
            <Show when=move || !recent_items().is_empty()>
                <section class="section">
                    <h2 class="section-title">{move || t(i18n.locale.get(), Key::FavoritesRecentlyAdded)}</h2>
                    <div class="scroll-card-row">
                        {move || recent_items().into_iter().map(|f| {
                            let href = format!("/games/{}/{}", f.fav.game.system, urlencoding::encode(&f.fav.game.rom_filename));
                            let name = f.fav.game.display_name.clone().unwrap_or_else(|| f.fav.game.rom_filename.clone());
                            let system = f.fav.game.system_display.clone();
                            let system_folder = f.fav.game.system.clone();
                            let box_art_url = f.box_art_url.clone();
                            view! {
                                <GameScrollCard href name system system_folder box_art_url />
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
                        <div class="stat-label">{move || t(i18n.locale.get(), Key::StatsFavorites)}</div>
                    </div>
                    <div class="stat-card">
                        <div class="stat-value">{system_count}</div>
                        <div class="stat-label">{move || t(i18n.locale.get(), Key::CommonSystems)}</div>
                    </div>
                </div>
            </section>

            // Organize panel
            <OrganizePanel favorites />

            // Personalized recommendations (loaded in parallel, non-blocking)
            // Uses skeleton from home page while streaming.
            <Suspense fallback=move || view! { <FavRecommendationsSkeleton /> }>
                {move || Suspend::new(async move {
                    let recs = recommendations.await;
                    let sections = recs.ok().unwrap_or_default();
                    Ok::<_, ServerFnError>(view! {
                        {sections.into_iter().map(|section| {
                            view! { <GameSectionRow section /> }
                        }).collect::<Vec<_>>()}
                    })
                })}
            </Suspense>

            // By System — system cards
            <Show when=move || { system_cards().len() > 1 }>
                <section class="section">
                    <h2 class="section-title">{move || t(i18n.locale.get(), Key::FavoritesBySystem)}</h2>
                    <div class="systems-grid">
                        {move || system_cards().into_iter().map(|(display_name, system, count, latest, _)| {
                            let href = format!("/favorites/{system}");
                            let icon_src = format!("/static/icons/systems/{system}.png");
                            let count_label = move || {
                                let locale = i18n.locale.get();
                                tf(locale, Key::CountFavorites, &[&count.to_string()])
                            };
                            view! {
                                <A href=href attr:class="system-card">
                                    <div class="system-card-name">{display_name}</div>
                                    <div class="system-card-body">
                                        <img
                                            class="system-card-icon"
                                            src=icon_src
                                            alt=""
                                            onerror="this.style.display='none'"
                                            loading="lazy"
                                        />
                                        <div class="system-card-text">
                                            <div class="system-card-count">{count_label}</div>
                                            <div class="system-card-size">{latest}</div>
                                        </div>
                                    </div>
                                </A>
                            }
                        }).collect::<Vec<_>>()}
                    </div>
                </section>
            </Show>

            // All Favorites — full list with grouped/flat toggle
            <section class="section">
                <div class="page-header">
                    <h2 class="section-title">{move || t(i18n.locale.get(), Key::FavoritesAll)}</h2>
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
                            let filtered_for_count = filtered_favorites;
                            move || {
                                let filtered = filtered_for_count().len();
                                let total = total_count();
                                if filter_text.read().is_empty() {
                                    tf(i18n.locale.get(), Key::CountFavorites, &[&total.to_string()])
                                } else {
                                    tf(i18n.locale.get(), Key::CountFavoritesPartial, &[&filtered.to_string(), &total.to_string()])
                                }
                            }
                        }
                    </span>
                </div>

                {
                    let filtered_signal = Signal::derive(filtered_favorites);
                    let filtered_signal2 = Signal::derive(filtered_favorites);
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
                <FavItem fav=f.fav box_art_url=f.box_art_url genre=f.genre show_system=true confirm_remove remove_fav=remove_fav.clone() />
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
                                view! { <FavItem fav=f.fav box_art_url=f.box_art_url genre=f.genre show_system=false confirm_remove remove_fav /> }
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
    #[prop(default = None)] genre: Option<String>,
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
    let system_folder = StoredValue::new(fav.game.system.clone());
    let rom_name = fav.game.display_name.unwrap_or(fav.game.rom_filename);
    let placeholder_name = StoredValue::new(rom_name.clone());
    let system_display = if show_system {
        Some(fav.game.system_display)
    } else {
        None
    };
    let has_genre = genre.as_ref().is_some_and(|g| !g.is_empty());
    let genre = StoredValue::new(genre.unwrap_or_default());

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
                <Show when=move || has_box_art fallback=move || view! {
                    <div class="rom-thumb-placeholder">
                        <BoxArtPlaceholder system=system_folder.get_value() name=placeholder_name.get_value() size="list".to_string() />
                    </div>
                }>
                    <img class="rom-thumb" src=box_art.get_value() loading="lazy" />
                </Show>
            </A>
            <div class="fav-info">
                <A href=game_href.get_value() attr:class="fav-name rom-name-link">{rom_name}</A>
                <div class="fav-badges">
                    {system_display.map(|s| view! { <span class="fav-system">{s}</span> })}
                    <Show when=move || has_genre>
                        <span class="search-badge search-badge-genre">{genre.get_value()}</span>
                    </Show>
                </div>
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

    // Build a preview of the folder structure from current favorites data.
    // Returns (primary_folder, sub_folders) tuples for nested preview.
    let preview_folders = move || {
        let favs = favorites.read();
        let pri = primary.get();
        let sec = secondary.get();
        let unknown = t(i18n.locale.get(), Key::OrganizePreviewUnknown).to_string();

        // Helper: extract folder name for a given criterion from a favorite.
        let folder_name = |criteria: &str, f: &FavoriteWithArt| -> Option<String> {
            match criteria {
                "genre" => Some(
                    f.genre
                        .as_deref()
                        .filter(|g| !g.is_empty())
                        .unwrap_or(&unknown)
                        .to_string(),
                ),
                "system" => Some(f.fav.game.system_display.clone()),
                "alphabetical" => {
                    let display = f
                        .fav
                        .game
                        .display_name
                        .as_deref()
                        .unwrap_or(&f.fav.game.rom_filename);
                    Some(
                        display
                            .chars()
                            .next()
                            .map(|c| {
                                let upper = c.to_uppercase().to_string();
                                if upper
                                    .chars()
                                    .next()
                                    .is_some_and(|ch| ch.is_ascii_alphabetic())
                                {
                                    upper
                                } else {
                                    "#".to_string()
                                }
                            })
                            .unwrap_or_else(|| "#".to_string()),
                    )
                }
                // players/rating/developer: no client-side data
                _ => None,
            }
        };

        // Static examples for criteria we can't derive client-side.
        let static_examples = |criteria: &str| -> Vec<&str> {
            match criteria {
                "players" => vec!["1 Player", "2 Players", "Unknown"],
                "rating" => vec!["★★★★★", "★★★★", "Not Rated"],
                "developer" => vec!["Capcom", "Konami", "Sega"],
                _ => vec![],
            }
        };

        // Determine whether each criterion has client-side data.
        let pri_is_real = static_examples(&pri).is_empty();
        let sec_is_real = sec != "none" && static_examples(&sec).is_empty();
        let sec_is_static = sec != "none" && !sec_is_real;

        // Build the preview map based on what data is available.
        let mut map: std::collections::HashMap<String, std::collections::BTreeSet<String>> =
            std::collections::HashMap::new();

        if pri_is_real {
            // Primary from real data: gather primary folders (and secondary if also real).
            for f in favs.iter() {
                if let Some(pri_name) = folder_name(&pri, f) {
                    let subs = map.entry(pri_name).or_default();
                    if sec_is_real && let Some(sec_name) = folder_name(&sec, f) {
                        subs.insert(sec_name);
                    }
                }
            }
            // If secondary is static, inject static examples into each primary.
            if sec_is_static {
                let examples: std::collections::BTreeSet<String> = static_examples(&sec)
                    .iter()
                    .map(|s| s.to_string())
                    .collect();
                for subs in map.values_mut() {
                    *subs = examples.clone();
                }
            }
        } else {
            // Primary is static: use static primary folder names.
            let pri_examples = static_examples(&pri);
            if sec_is_real {
                // Collect all unique real secondary values as a representative sample.
                let mut all_secs = std::collections::BTreeSet::new();
                for f in favs.iter() {
                    if let Some(sec_name) = folder_name(&sec, f) {
                        all_secs.insert(sec_name);
                    }
                }
                // Show a few secondary values under each static primary.
                let sample: Vec<String> = all_secs.into_iter().take(5).collect();
                for name in pri_examples {
                    map.insert(name.to_string(), sample.iter().cloned().collect());
                }
            } else {
                // Both static (or secondary is "none").
                let sec_examples = if sec_is_static {
                    static_examples(&sec)
                } else {
                    vec![]
                };
                for name in pri_examples {
                    map.entry(name.to_string())
                        .or_default()
                        .extend(sec_examples.iter().map(|s| s.to_string()));
                }
            }
        }

        // Convert to sorted Vec of tuples.
        let mut result: Vec<(String, Vec<String>)> = map
            .into_iter()
            .map(|(k, v)| (k, v.into_iter().collect()))
            .collect();
        result.sort_by(|a, b| a.0.cmp(&b.0));
        result
    };

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
                        let msg = format!("{} {}", result.organized, t(locale, Key::OrganizeDone));
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
                        t(locale, Key::OrganizeAlreadyFlat).to_string()
                    } else {
                        format!("{count} {}", t(locale, Key::OrganizeFlattened))
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
                <span class="organize-toggle-icon">{move || if expanded.get() { "\u{1F4C2}" } else { "\u{1F4C1}" }}</span>
                <span class="organize-toggle-text">
                    <span class="organize-toggle-title">{move || t(i18n.locale.get(), Key::OrganizeTitle)}</span>
                    <span class="organize-toggle-desc">{move || t(i18n.locale.get(), Key::OrganizeDescription)}</span>
                </span>
            </button>

            <Show when=move || expanded.get()>
                <div class="organize-panel">
                    <div class="form-field">
                        <label class="form-label">{move || t(i18n.locale.get(), Key::OrganizePrimary)}</label>
                        <select class="form-input" bind:value=primary>
                            <option value="genre">{move || t(i18n.locale.get(), Key::OrganizeGenre)}</option>
                            <option value="system">{move || t(i18n.locale.get(), Key::OrganizeSystem)}</option>
                            <option value="players">{move || t(i18n.locale.get(), Key::OrganizePlayers)}</option>
                            <option value="rating">{move || t(i18n.locale.get(), Key::OrganizeRating)}</option>
                            <option value="developer">{move || t(i18n.locale.get(), Key::OrganizeDeveloper)}</option>
                            <option value="alphabetical">{move || t(i18n.locale.get(), Key::OrganizeAlphabetical)}</option>
                        </select>
                    </div>

                    <div class="form-field">
                        <label class="form-label">{move || t(i18n.locale.get(), Key::OrganizeSecondary)}</label>
                        {move || {
                            let p = primary.get();
                            let options: Vec<(&str, Key)> = [
                                ("genre", Key::OrganizeGenre),
                                ("system", Key::OrganizeSystem),
                                ("players", Key::OrganizePlayers),
                                ("rating", Key::OrganizeRating),
                                ("developer", Key::OrganizeDeveloper),
                                ("alphabetical", Key::OrganizeAlphabetical),
                            ]
                            .into_iter()
                            .filter(|(val, _)| *val != p.as_str())
                            .collect();

                            view! {
                                <select class="form-input" bind:value=secondary>
                                    <option value="none">{t(i18n.locale.get(), Key::OrganizeNone)}</option>
                                    {options.into_iter().map(|(val, key)| {
                                        let label = t(i18n.locale.get(), key);
                                        view! { <option value=val>{label}</option> }
                                    }).collect::<Vec<_>>()}
                                </select>
                            }
                        }}
                    </div>

                    <OrganizePreview preview_folders />

                    <div class="form-field form-field-check">
                        <div>
                            <label class="form-label">{move || t(i18n.locale.get(), Key::OrganizeKeepOriginals)}</label>
                            <p class="form-hint">{move || t(i18n.locale.get(), Key::OrganizeKeepHint)}</p>
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
                                if busy.get() { t(locale, Key::OrganizeOrganizing) } else { t(locale, Key::OrganizeApply) }
                            }}
                        </button>
                        <button
                            class="form-btn form-btn-secondary"
                            on:click=on_flatten
                            disabled=move || busy.get()
                        >
                            {move || {
                                let locale = i18n.locale.get();
                                if busy.get() { t(locale, Key::OrganizeFlattening) } else { t(locale, Key::OrganizeFlatten) }
                            }}
                        </button>
                    </div>
                </div>
            </Show>
        </section>
    }
}

/// Folder structure preview showing what subfolders will be created.
#[component]
fn OrganizePreview<F>(preview_folders: F) -> impl IntoView
where
    F: Fn() -> Vec<(String, Vec<String>)> + Clone + Send + Sync + 'static,
{
    let i18n = use_i18n();

    view! {
        {move || {
            let folders = preview_folders();
            if folders.is_empty() {
                return None;
            }
            let total_primary = folders.len();
            // Cap at 6 primary folders shown.
            let show_primary = total_primary.min(6);
            let remaining_primary = total_primary.saturating_sub(6);
            let has_subfolders = folders.iter().any(|(_, subs)| !subs.is_empty());

            let mut lines = Vec::new();
            for (i, (name, subs)) in folders.iter().take(show_primary).enumerate() {
                let is_last_primary = i == show_primary - 1 && remaining_primary == 0;
                let connector = if is_last_primary {
                    "\u{2514}\u{2500}\u{2500} "
                } else {
                    "\u{251C}\u{2500}\u{2500} "
                };
                lines.push(format!("{connector}\u{1F4C1} {name}/"));

                if !subs.is_empty() {
                    let total_subs = subs.len();
                    // Cap at 3 sub-folders per parent.
                    let show_subs = total_subs.min(3);
                    let remaining_subs = total_subs.saturating_sub(3);
                    let continuation = if is_last_primary { "    " } else { "\u{2502}   " };
                    for (j, sub) in subs.iter().take(show_subs).enumerate() {
                        let sub_connector = if j == show_subs - 1 && remaining_subs == 0 {
                            "\u{2514}\u{2500}\u{2500} "
                        } else {
                            "\u{251C}\u{2500}\u{2500} "
                        };
                        lines.push(format!("{continuation}{sub_connector}\u{1F4C1} {sub}/"));
                    }
                    if remaining_subs > 0 {
                        lines.push(format!(
                            "{continuation}\u{2514}\u{2500}\u{2500} \u{2026} +{remaining_subs} more"
                        ));
                    }
                }
            }
            if remaining_primary > 0 {
                // If there are sub-folders, the overflow hint uses a folder icon too.
                if has_subfolders {
                    lines.push(format!(
                        "\u{2514}\u{2500}\u{2500} \u{2026} +{remaining_primary} more folders"
                    ));
                } else {
                    lines.push(format!(
                        "\u{2514}\u{2500}\u{2500} \u{2026} +{remaining_primary} more"
                    ));
                }
            }
            Some(view! {
                <div class="organize-preview">
                    <div class="organize-preview-label">{t(i18n.locale.get(), Key::OrganizePreview)}</div>
                    <div class="organize-preview-tree">
                        {lines.into_iter().map(|line| view! {
                            <div>{line}</div>
                        }).collect::<Vec<_>>()}
                    </div>
                </div>
            })
        }}
    }
}

fn parse_criteria(value: &str) -> Option<OrganizeCriteria> {
    match value {
        "system" => Some(OrganizeCriteria::System),
        "genre" => Some(OrganizeCriteria::Genre),
        "players" => Some(OrganizeCriteria::Players),
        "rating" => Some(OrganizeCriteria::Rating),
        "alphabetical" => Some(OrganizeCriteria::Alphabetical),
        "developer" => Some(OrganizeCriteria::Developer),
        _ => None,
    }
}

/// Skeleton for the full favorites page while the main resource streams.
#[component]
fn FavoritesPageSkeleton() -> impl IntoView {
    view! {
        // Hero card skeleton
        <section class="section">
            <div class="skeleton-title skeleton-shimmer"></div>
            <div class="skeleton-hero skeleton-shimmer">
                <div class="skeleton-hero-thumb"></div>
                <div class="skeleton-hero-info">
                    <div class="skeleton-hero-title"></div>
                    <div class="skeleton-hero-system"></div>
                </div>
            </div>
        </section>
        // Recent scroll skeleton
        <section class="section">
            <div class="skeleton-title skeleton-shimmer"></div>
            <div class="scroll-card-row">
                {(0..6).map(|_| view! {
                    <div class="skeleton-card skeleton-shimmer">
                        <div class="skeleton-card-image"></div>
                        <div class="skeleton-card-text"></div>
                        <div class="skeleton-card-subtext"></div>
                    </div>
                }).collect::<Vec<_>>()}
            </div>
        </section>
        // List items skeleton
        <section class="section">
            <div class="skeleton-title skeleton-shimmer"></div>
            <div class="fav-skeleton-list">
                {(0..5).map(|_| view! {
                    <div class="fav-skeleton-item skeleton-shimmer">
                        <div class="fav-skeleton-thumb"></div>
                        <div class="fav-skeleton-info">
                            <div class="fav-skeleton-name"></div>
                            <div class="fav-skeleton-system"></div>
                        </div>
                    </div>
                }).collect::<Vec<_>>()}
            </div>
        </section>
    }
}

/// Skeleton placeholder for favorites recommendation sections while streaming.
#[component]
fn FavRecommendationsSkeleton() -> impl IntoView {
    view! {
        <section class="section">
            <div class="skeleton-title skeleton-shimmer"></div>
            <div class="scroll-card-row">
                {(0..6).map(|_| view! {
                    <div class="skeleton-card skeleton-shimmer">
                        <div class="skeleton-card-image"></div>
                        <div class="skeleton-card-text"></div>
                        <div class="skeleton-card-subtext"></div>
                    </div>
                }).collect::<Vec<_>>()}
            </div>
        </section>
    }
}

#[component]
pub fn SystemFavoritesPage() -> impl IntoView {
    let i18n = use_i18n();
    let params = use_params_map();
    let system = move || params.read().get("system").unwrap_or_default();

    let favorites = Resource::new(system, server_fns::get_system_favorites);

    view! {
        <div class="page favorites-page">
            <Suspense fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), Key::CommonLoading)}</div> }>
                {move || Suspend::new(async move {
                    let favs = favorites.await?;
                    Ok::<_ , ServerFnError>(view! { <SystemFavoritesContent favs /> })
                })}
            </Suspense>
        </div>
    }
}

/// Inner content for the system-specific favorites page.
#[component]
fn SystemFavoritesContent(favs: Vec<FavoriteWithArt>) -> impl IntoView {
    let i18n = use_i18n();
    let favorites = RwSignal::new(favs);
    let confirm_remove = RwSignal::new(Option::<String>::None);

    // Derive system info from the first favorite (one-time read).
    let first = favorites.read_untracked();
    let system_display = first
        .first()
        .map(|f| f.fav.game.system_display.clone())
        .unwrap_or_default();
    let system_folder = first
        .first()
        .map(|f| f.fav.game.system.clone())
        .unwrap_or_default();
    drop(first);
    let icon_src = format!("/static/icons/systems/{system_folder}.png");

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
                {move || t(i18n.locale.get(), Key::GamesBack)}
            </A>
            <h2 class="page-title">
                <img
                    class="rom-header-icon"
                    src=icon_src
                    alt=""
                    onerror="this.style.display='none'"
                    loading="lazy"
                />
                {system_display}
            </h2>
        </div>
        <p class="rom-count">{move || {
            let count = total_count();
            let locale = i18n.locale.get();
            tf(locale, Key::CountFavorites, &[&count.to_string()])
        }}</p>
        <Show when=move || !is_empty() fallback=move || view! {
            <p class="empty-state">{t(i18n.locale.get(), Key::FavoritesEmpty)}</p>
        }>
            <div class="fav-list">
                <For
                    each=move || favorites.get()
                    key=|f| f.fav.marker_filename.clone()
                    let:f
                >
                    <FavItem fav=f.fav box_art_url=f.box_art_url genre=f.genre show_system=false confirm_remove remove_fav />
                </For>
            </div>
        </Show>
    }
}
