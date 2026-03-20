use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_query_map;

use crate::components::filter_chips::{FilterChips, FilterState};
use crate::components::game_list_item::GameListItem;
use crate::i18n::{t, use_i18n};
use crate::server_fns::{
    self, DeveloperMatch, DeveloperSearchResult, GlobalSearchResult, GlobalSearchResults,
    SystemSearchGroup,
};

#[cfg(feature = "hydrate")]
const RECENT_SEARCHES_KEY: &str = "replay_recent_searches";
#[cfg(feature = "hydrate")]
const MAX_RECENT_SEARCHES: usize = 8;

#[component]
pub fn SearchPage() -> impl IntoView {
    let i18n = use_i18n();
    let query_map = use_query_map();

    // Read initial values from URL query params.
    let initial_query = query_map
        .read_untracked()
        .get("q")
        .map(|s| s.to_string())
        .unwrap_or_default();
    let initial_hide_hacks = query_map
        .read_untracked()
        .get("hide_hacks")
        .map(|v| v == "true")
        .unwrap_or(false);
    let initial_hide_translations = query_map
        .read_untracked()
        .get("hide_translations")
        .map(|v| v == "true")
        .unwrap_or(false);
    let initial_hide_betas = query_map
        .read_untracked()
        .get("hide_betas")
        .map(|v| v == "true")
        .unwrap_or(false);
    let initial_hide_clones = query_map
        .read_untracked()
        .get("hide_clones")
        .map(|v| v == "true")
        .unwrap_or(false);
    let initial_genre = query_map
        .read_untracked()
        .get("genre")
        .map(|s| s.to_string())
        .unwrap_or_default();
    let initial_multiplayer = query_map
        .read_untracked()
        .get("multiplayer")
        .map(|v| v == "true")
        .unwrap_or(false);

    // Signals for user input.
    let search_input = RwSignal::new(initial_query.clone());
    let filters = FilterState {
        hide_hacks: RwSignal::new(initial_hide_hacks),
        hide_translations: RwSignal::new(initial_hide_translations),
        hide_betas: RwSignal::new(initial_hide_betas),
        hide_clones: RwSignal::new(initial_hide_clones),
        multiplayer_only: RwSignal::new(initial_multiplayer),
        genre: RwSignal::new(initial_genre.clone()),
        min_rating: RwSignal::new(None),
    };

    // Debounced search query that drives the resource.
    let debounced_query = RwSignal::new(initial_query.clone());
    let debounced_genre = RwSignal::new(initial_genre);

    // Recent searches (client-side, loaded from localStorage).
    // Start empty on both SSR and hydrate so the DOM matches during hydration.
    // An Effect below populates the signal post-hydration from localStorage.
    let recent_searches: RwSignal<Vec<String>> = RwSignal::new(Vec::new());

    // Random game navigation state.
    let random_loading = RwSignal::new(false);

    // Genre list resource.
    let genres_resource = Resource::new(|| (), |_| server_fns::get_all_genres());

    // Load recent searches from localStorage after hydration.
    // This runs as a one-shot effect so SSR and hydration both see an empty
    // vec (no DOM mismatch), then the UI updates reactively once loaded.
    #[cfg(feature = "hydrate")]
    Effect::new(move || {
        recent_searches.set(load_recent_searches());
    });

    // Debounce the search input (400ms) + save to recent searches.
    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::prelude::*;

        // Skip the very first update_url_params call from the filter Effect.
        // On mount, the Leptos Router's pushState hasn't fired yet (it's deferred
        // via an async channel). If we call replaceState before pushState, we
        // overwrite the *previous* page's history entry with /search, making the
        // browser back button go from /search to /search (i.e., "nothing happens").
        let filters_initialized = StoredValue::new(false);

        let timer_handle: StoredValue<Option<i32>> = StoredValue::new(None);
        Effect::new(move || {
            let val = search_input.get();
            if let Some(handle) = timer_handle.get_value()
                && let Some(w) = web_sys::window()
            {
                w.clear_timeout_with_handle(handle);
            }
            let cb = Closure::<dyn Fn()>::new(move || {
                debounced_query.set(val.clone());
                update_url_params(
                    &val,
                    filters.hide_hacks.get_untracked(),
                    filters.hide_translations.get_untracked(),
                    filters.hide_betas.get_untracked(),
                    filters.hide_clones.get_untracked(),
                    filters.multiplayer_only.get_untracked(),
                    &filters.genre.get_untracked(),
                );
                // Save to recent searches if non-empty.
                if !val.trim().is_empty() {
                    save_recent_search(&val);
                    // Refresh the signal so UI stays in sync.
                    recent_searches.set(load_recent_searches());
                }
            });
            if let Some(window) = web_sys::window()
                && let Ok(handle) = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                    cb.as_ref().unchecked_ref(),
                    400,
                )
            {
                timer_handle.set_value(Some(handle));
            }
            cb.forget();
        });

        // Immediate update for filter changes (no debounce needed).
        Effect::new(move || {
            let hh = filters.hide_hacks.get();
            let ht = filters.hide_translations.get();
            let hb = filters.hide_betas.get();
            let hc = filters.hide_clones.get();
            let mp = filters.multiplayer_only.get();
            let g = filters.genre.get();
            debounced_genre.set(g.clone());
            // Skip the first run: URL already reflects initial values and the
            // Router's pushState hasn't completed yet.
            if !filters_initialized.get_value() {
                filters_initialized.set_value(true);
                return;
            }
            update_url_params(&debounced_query.get_untracked(), hh, ht, hb, hc, mp, &g);
        });

        on_cleanup(move || {
            if let Some(handle) = timer_handle.get_value()
                && let Some(w) = web_sys::window()
            {
                w.clear_timeout_with_handle(handle);
            }
        });
    }

    // Search results resource.
    let results = Resource::new(
        move || {
            (
                debounced_query.get(),
                filters.hide_hacks.get(),
                filters.hide_translations.get(),
                filters.hide_betas.get(),
                filters.hide_clones.get(),
                filters.multiplayer_only.get(),
                debounced_genre.get(),
                filters.min_rating.get(),
            )
        },
        |(q, hh, ht, hb, hc, mp, g, mr)| server_fns::global_search(q, hh, ht, hb, hc, mp, mr, g, 3),
    );

    // Developer match resource — only fires when query is non-empty.
    let developer_results = Resource::new(
        move || debounced_query.get(),
        |q| server_fns::search_by_developer(q, 20),
    );

    // Derived: show the "empty state" panel (recent searches + random game).
    // Show whenever the search field is empty — don't gate on focus state because
    // autofocus fires before hydration so the on:focus handler never triggers,
    // and any stray blur event hides the panel with no way to recover.
    let show_empty_panel = move || {
        let q = search_input.get();
        q.trim().is_empty()
    };

    // Handler: click a recent search chip.
    let on_recent_click = move |query: String| {
        search_input.set(query.clone());
        debounced_query.set(query.clone());
        update_url_params_if_hydrate(
            &query,
            filters.hide_hacks.get_untracked(),
            filters.hide_translations.get_untracked(),
            filters.hide_betas.get_untracked(),
            filters.hide_clones.get_untracked(),
            filters.multiplayer_only.get_untracked(),
            &filters.genre.get_untracked(),
        );
    };

    // Handler: remove a single recent search.
    let on_recent_remove = move |query: String| {
        #[cfg(feature = "hydrate")]
        {
            remove_recent_search(&query);
            recent_searches.set(load_recent_searches());
        }
        let _ = query;
    };

    // Handler: random game button.
    let on_random_game = move |_| {
        random_loading.set(true);
        #[cfg(feature = "hydrate")]
        {
            leptos::task::spawn_local(async move {
                match server_fns::random_game().await {
                    Ok((system, rom_filename)) => {
                        let href =
                            format!("/games/{}/{}", system, urlencoding::encode(&rom_filename));
                        if let Some(w) = web_sys::window() {
                            let _ = w.location().set_href(&href);
                        }
                    }
                    Err(_) => {
                        random_loading.set(false);
                    }
                }
            });
        }
    };

    // Focus the search input on mount (autofocus only works on full page load,
    // not on client-side router navigation).
    let input_ref = NodeRef::<leptos::html::Input>::new();
    #[cfg(feature = "hydrate")]
    Effect::new(move || {
        if let Some(el) = input_ref.get() {
            let _ = el.focus();
        }
    });

    view! {
        <div class="page search-page">
            <div class="search-page-bar">
                <input
                    type="text"
                    class="search-page-input"
                    node_ref=input_ref
                    placeholder=move || t(i18n.locale.get(), "search.placeholder")
                    prop:value=move || search_input.get()
                    on:input=move |ev| search_input.set(event_target_value(&ev))
                    autofocus=true
                />
            </div>

            // Empty state panel: recent searches + random game.
            <Show when=show_empty_panel>
                <div class="search-empty-panel">
                    <RecentSearches
                        searches=recent_searches
                        on_click=on_recent_click
                        on_remove=on_recent_remove
                    />
                    <button
                        class="random-game-btn"
                        on:click=on_random_game
                        disabled=move || random_loading.get()
                    >
                        <span class="random-game-icon">{"\u{1F3B2}"}</span>
                        " "
                        {move || if random_loading.get() {
                            t(i18n.locale.get(), "common.loading")
                        } else {
                            t(i18n.locale.get(), "search.random_game")
                        }}
                    </button>
                </div>
            </Show>

            <div class="search-filters">
                <FilterChips
                    filters
                    show_clones=Signal::derive(|| true)
                />
                <Suspense>
                    {move || Suspend::new(async move {
                        let genre_list = genres_resource.await?;
                        Ok::<_, server_fn::ServerFnError>(if genre_list.is_empty() {
                            None
                        } else {
                            Some(view! { <crate::components::genre_dropdown::GenreDropdown genre=filters.genre genre_list /> })
                        })
                    })}
                </Suspense>
            </div>

            // Developer match block (horizontal scroll, shown above regular results).
            <Transition fallback=|| ()>
                {move || Suspend::new(async move {
                    let locale = i18n.locale.get();
                    let query = debounced_query.get();
                    let dev = developer_results.await?;
                    Ok::<_, server_fn::ServerFnError>(dev.map(|data| {
                        view! { <DeveloperBlock data locale query /> }
                    }))
                })}
            </Transition>

            <Transition fallback=move || view! {
                <div class="loading">{move || t(i18n.locale.get(), "common.loading")}</div>
            }>
                {move || Suspend::new(async move {
                    let locale = i18n.locale.get();
                    let data = results.await?;
                    let q = debounced_query.get_untracked();
                    let hh = filters.hide_hacks.get_untracked();
                    let ht = filters.hide_translations.get_untracked();
                    let hb = filters.hide_betas.get_untracked();
                    let hc = filters.hide_clones.get_untracked();
                    let g = debounced_genre.get_untracked();
                    Ok::<_, server_fn::ServerFnError>(view! {
                        <SearchResults data locale query=q hide_hacks=hh hide_translations=ht hide_betas=hb hide_clones=hc genre=g />
                    })
                })}
            </Transition>
        </div>
    }
}

/// Recent searches chip list.
#[component]
fn RecentSearches(
    searches: RwSignal<Vec<String>>,
    on_click: impl Fn(String) + Copy + Send + Sync + 'static,
    on_remove: impl Fn(String) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let i18n = use_i18n();

    let has_searches = move || !searches.read().is_empty();

    view! {
        <Show when=has_searches>
            <div class="recent-searches">
                <span class="recent-searches-label">
                    {move || t(i18n.locale.get(), "search.recent_searches")}
                </span>
                <div class="recent-searches-chips">
                    {move || {
                        searches.get().into_iter().map(|q| {
                            let q_click = q.clone();
                            let q_remove = q.clone();
                            let q_display = q.clone();
                            view! {
                                <span class="recent-chip">
                                    <button
                                        class="recent-chip-text"
                                        on:mousedown=move |ev| {
                                            ev.prevent_default();
                                            on_click(q_click.clone());
                                        }
                                    >
                                        {q_display}
                                    </button>
                                    <button
                                        class="recent-chip-remove"
                                        on:mousedown=move |ev| {
                                            ev.prevent_default();
                                            on_remove(q_remove.clone());
                                        }
                                    >
                                        {"\u{2715}"}
                                    </button>
                                </span>
                            }
                        }).collect::<Vec<_>>()
                    }}
                </div>
            </div>
        </Show>
    }
}

/// Display search results grouped by system.
#[component]
fn SearchResults(
    data: GlobalSearchResults,
    locale: crate::i18n::Locale,
    query: String,
    hide_hacks: bool,
    hide_translations: bool,
    hide_betas: bool,
    hide_clones: bool,
    genre: String,
) -> impl IntoView {
    let has_results = !data.groups.is_empty();

    if !has_results {
        return view! {
            <p class="empty-state">{t(locale, "search.no_results")}</p>
        }
        .into_any();
    }

    let count_summary = format!(
        "{} {} {} {}",
        data.total_results,
        t(locale, "search.results_summary"),
        data.total_systems,
        t(locale, "search.systems")
    );
    let summary = if query.is_empty() && !genre.is_empty() {
        format!(
            "{} {} — {}",
            t(locale, "search.browsing_genre"),
            genre,
            count_summary
        )
    } else {
        count_summary
    };

    // Build query string for "See all" links.
    let filter_qs = StoredValue::new({
        let mut params = Vec::new();
        if !query.is_empty() {
            params.push(format!("search={}", urlencoding::encode(&query)));
        }
        if hide_hacks {
            params.push("hide_hacks=true".to_string());
        }
        if hide_translations {
            params.push("hide_translations=true".to_string());
        }
        if hide_betas {
            params.push("hide_betas=true".to_string());
        }
        if hide_clones {
            params.push("hide_clones=true".to_string());
        }
        if !genre.is_empty() {
            params.push(format!("genre={}", urlencoding::encode(&genre)));
        }
        if params.is_empty() {
            String::new()
        } else {
            format!("?{}", params.join("&"))
        }
    });

    view! {
        <p class="search-summary">{summary}</p>
        <div class="search-groups">
            {data
                .groups
                .into_iter()
                .map(|group| {
                    let qs = filter_qs.get_value();
                    view! { <SystemGroup group locale filter_qs=qs /> }
                })
                .collect::<Vec<_>>()}
        </div>
    }
    .into_any()
}

/// A single system's search result group.
#[component]
fn SystemGroup(
    group: SystemSearchGroup,
    locale: crate::i18n::Locale,
    filter_qs: String,
) -> impl IntoView {
    let header_text = format!("{} ({})", group.system_display, group.total_matches);
    let see_all_href = format!("/games/{}{}", group.system, filter_qs);

    view! {
        <div class="search-group">
            <div class="search-group-header">
                <h3 class="search-group-title">{header_text}</h3>
                <A href=see_all_href attr:class="search-see-all">
                    {t(locale, "search.see_all")} " \u{2192}"
                </A>
            </div>
            <div class="search-group-results">
                {group
                    .top_results
                    .into_iter()
                    .map(|result| {
                        view! { <SearchResultItem result /> }
                    })
                    .collect::<Vec<_>>()}
            </div>
        </div>
    }
}

/// A single search result row — delegates to the shared `GameListItem`.
#[component]
fn SearchResultItem(result: GlobalSearchResult) -> impl IntoView {
    let genre = (!result.genre.is_empty()).then(|| result.genre.clone());
    view! {
        <GameListItem
            system=result.system.clone()
            rom_filename=result.rom_filename.clone()
            display_name=result.display_name.clone()
            rom_path=result.rom_path.clone()
            box_art_url=result.box_art_url.clone()
            is_favorite=result.is_favorite
            genre=genre
            rating=result.rating
        />
    }
}

/// "Games by [Developer]" compact list block.
#[component]
fn DeveloperBlock(
    data: DeveloperSearchResult,
    locale: crate::i18n::Locale,
    query: String,
) -> impl IntoView {
    let title = format!(
        "{} {} ({})",
        t(locale, "search.games_by"),
        data.developer_name,
        data.total_count
    );

    let see_all_href = format!(
        "/developer/{}",
        urlencoding::encode(&data.developer_name)
    );

    let has_other_developers = !data.other_developers.is_empty();

    view! {
        <div class="search-group">
            <div class="search-group-header">
                <h3 class="search-group-title">{title}</h3>
                <A href=see_all_href attr:class="search-see-all">
                    {t(locale, "developer.see_all")} " \u{2192}"
                </A>
            </div>
            <div class="search-group-results">
                {data.games.into_iter().take(3).map(|result| {
                    view! { <SearchResultItem result /> }
                }).collect::<Vec<_>>()}
            </div>
        </div>
        <Show when=move || has_other_developers>
            <OtherDevelopersList
                developers=data.other_developers.clone()
                query=query.clone()
                locale
            />
        </Show>
    }
}

/// List of additional developer matches below the main developer block.
#[component]
fn OtherDevelopersList(
    developers: Vec<DeveloperMatch>,
    query: String,
    locale: crate::i18n::Locale,
) -> impl IntoView {
    let heading = format!(
        "{} \"{}\"",
        t(locale, "search.other_developers"),
        query,
    );

    view! {
        <section class="developer-match-list">
            <h3 class="developer-match-heading">{heading}</h3>
            {developers.into_iter().map(|dev| {
                let href = format!("/developer/{}", urlencoding::encode(&dev.name));
                let count_label = dev.game_count.to_string();
                view! {
                    <A href=href attr:class="developer-match-item">
                        <span class="developer-match-name">{dev.name}</span>
                        <span class="developer-match-count">{count_label}</span>
                    </A>
                }
            }).collect::<Vec<_>>()}
        </section>
    }
}

/// Update URL query params without navigating (replace mode).
#[cfg(feature = "hydrate")]
fn update_url_params(
    query: &str,
    hide_hacks: bool,
    hide_translations: bool,
    hide_betas: bool,
    hide_clones: bool,
    multiplayer_only: bool,
    genre: &str,
) {
    if let Some(window) = web_sys::window() {
        let mut params = Vec::new();
        if !query.is_empty() {
            params.push(format!("q={}", urlencoding::encode(query)));
        }
        if hide_hacks {
            params.push("hide_hacks=true".to_string());
        }
        if hide_translations {
            params.push("hide_translations=true".to_string());
        }
        if hide_betas {
            params.push("hide_betas=true".to_string());
        }
        if hide_clones {
            params.push("hide_clones=true".to_string());
        }
        if multiplayer_only {
            params.push("multiplayer=true".to_string());
        }
        if !genre.is_empty() {
            params.push(format!("genre={}", urlencoding::encode(genre)));
        }
        let qs = if params.is_empty() {
            String::new()
        } else {
            format!("?{}", params.join("&"))
        };
        let url = format!("/search{qs}");
        let _ = window
            .history()
            .and_then(|h| h.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&url)));
    }
}

/// Wrapper that compiles on both targets — calls the real function only on hydrate.
#[allow(unused_variables)]
fn update_url_params_if_hydrate(
    query: &str,
    hide_hacks: bool,
    hide_translations: bool,
    hide_betas: bool,
    hide_clones: bool,
    multiplayer_only: bool,
    genre: &str,
) {
    #[cfg(feature = "hydrate")]
    update_url_params(
        query,
        hide_hacks,
        hide_translations,
        hide_betas,
        hide_clones,
        multiplayer_only,
        genre,
    );
}

// ── localStorage helpers for recent searches ──────────────────────

#[cfg(feature = "hydrate")]
fn get_local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

#[cfg(feature = "hydrate")]
fn load_recent_searches() -> Vec<String> {
    let storage = match get_local_storage() {
        Some(s) => s,
        None => return Vec::new(),
    };
    let raw = match storage.get_item(RECENT_SEARCHES_KEY).ok().flatten() {
        Some(s) => s,
        None => return Vec::new(),
    };
    serde_json::from_str::<Vec<String>>(&raw).unwrap_or_default()
}

#[cfg(feature = "hydrate")]
fn save_recent_search(query: &str) {
    let storage = match get_local_storage() {
        Some(s) => s,
        None => return,
    };
    let mut searches = load_recent_searches();
    // Remove duplicates.
    searches.retain(|s| s != query);
    // Prepend new search.
    searches.insert(0, query.to_string());
    // Trim to max.
    searches.truncate(MAX_RECENT_SEARCHES);
    if let Ok(json) = serde_json::to_string(&searches) {
        let _ = storage.set_item(RECENT_SEARCHES_KEY, &json);
    }
}

#[cfg(feature = "hydrate")]
fn remove_recent_search(query: &str) {
    let storage = match get_local_storage() {
        Some(s) => s,
        None => return,
    };
    let mut searches = load_recent_searches();
    searches.retain(|s| s != query);
    if let Ok(json) = serde_json::to_string(&searches) {
        let _ = storage.set_item(RECENT_SEARCHES_KEY, &json);
    }
}
