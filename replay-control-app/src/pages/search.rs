use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_query_map;

use crate::i18n::{t, use_i18n};
use crate::server_fns::{self, GlobalSearchResult, GlobalSearchResults, SystemSearchGroup};

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

    // Signals for user input.
    let search_input = RwSignal::new(initial_query.clone());
    let hide_hacks = RwSignal::new(initial_hide_hacks);
    let hide_translations = RwSignal::new(initial_hide_translations);
    let hide_betas = RwSignal::new(initial_hide_betas);
    let hide_clones = RwSignal::new(initial_hide_clones);
    let genre = RwSignal::new(initial_genre.clone());

    // Debounced search query that drives the resource.
    let debounced_query = RwSignal::new(initial_query.clone());
    let debounced_genre = RwSignal::new(initial_genre);

    // Recent searches (client-side, loaded from localStorage).
    // Initialize synchronously during hydrate to avoid empty flash.
    #[cfg(feature = "hydrate")]
    let recent_searches: RwSignal<Vec<String>> = RwSignal::new(load_recent_searches());
    #[cfg(not(feature = "hydrate"))]
    let recent_searches: RwSignal<Vec<String>> = RwSignal::new(Vec::new());

    // Random game navigation state.
    let random_loading = RwSignal::new(false);

    // Genre list resource.
    let genres_resource = Resource::new(|| (), |_| server_fns::get_all_genres());

    // Debounce the search input (400ms) + save to recent searches.
    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::prelude::*;

        let timer_handle: StoredValue<Option<i32>> = StoredValue::new(None);
        Effect::new(move || {
            let val = search_input.get();
            if let Some(handle) = timer_handle.get_value() {
                if let Some(w) = web_sys::window() {
                    w.clear_timeout_with_handle(handle);
                }
            }
            let cb = Closure::<dyn Fn()>::new(move || {
                debounced_query.set(val.clone());
                update_url_params(
                    &val,
                    hide_hacks.get_untracked(),
                    hide_translations.get_untracked(),
                    hide_betas.get_untracked(),
                    hide_clones.get_untracked(),
                    &genre.get_untracked(),
                );
                // Save to recent searches if non-empty.
                if !val.trim().is_empty() {
                    save_recent_search(&val);
                    // Refresh the signal so UI stays in sync.
                    recent_searches.set(load_recent_searches());
                }
            });
            if let Some(window) = web_sys::window() {
                if let Ok(handle) =
                    window.set_timeout_with_callback_and_timeout_and_arguments_0(
                        cb.as_ref().unchecked_ref(),
                        400,
                    )
                {
                    timer_handle.set_value(Some(handle));
                }
            }
            cb.forget();
        });

        // Immediate update for filter changes (no debounce needed).
        Effect::new(move || {
            let hh = hide_hacks.get();
            let ht = hide_translations.get();
            let hb = hide_betas.get();
            let hc = hide_clones.get();
            let g = genre.get();
            debounced_genre.set(g.clone());
            update_url_params(&debounced_query.get_untracked(), hh, ht, hb, hc, &g);
        });

        on_cleanup(move || {
            if let Some(handle) = timer_handle.get_value() {
                if let Some(w) = web_sys::window() {
                    w.clear_timeout_with_handle(handle);
                }
            }
        });
    }

    // Search results resource.
    let results = Resource::new(
        move || {
            (
                debounced_query.get(),
                hide_hacks.get(),
                hide_translations.get(),
                hide_betas.get(),
                hide_clones.get(),
                debounced_genre.get(),
            )
        },
        |(q, hh, ht, hb, hc, g)| server_fns::global_search(q, hh, ht, hb, hc, g, 3),
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
            hide_hacks.get_untracked(),
            hide_translations.get_untracked(),
            hide_betas.get_untracked(),
            hide_clones.get_untracked(),
            &genre.get_untracked(),
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
                        let href = format!(
                            "/games/{}/{}",
                            system,
                            urlencoding::encode(&rom_filename)
                        );
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
                <button
                    class=move || {
                        if hide_hacks.get() {
                            "filter-chip filter-chip-active"
                        } else {
                            "filter-chip"
                        }
                    }
                    on:click=move |_| hide_hacks.update(|v| *v = !*v)
                >
                    {move || t(i18n.locale.get(), "filter.hide_hacks")}
                    {move || if hide_hacks.get() { " \u{2715}" } else { "" }}
                </button>

                <button
                    class=move || {
                        if hide_translations.get() {
                            "filter-chip filter-chip-active"
                        } else {
                            "filter-chip"
                        }
                    }
                    on:click=move |_| hide_translations.update(|v| *v = !*v)
                >
                    {move || t(i18n.locale.get(), "filter.hide_translations")}
                    {move || if hide_translations.get() { " \u{2715}" } else { "" }}
                </button>

                <button
                    class=move || {
                        if hide_betas.get() {
                            "filter-chip filter-chip-active"
                        } else {
                            "filter-chip"
                        }
                    }
                    on:click=move |_| hide_betas.update(|v| *v = !*v)
                >
                    {move || t(i18n.locale.get(), "filter.hide_betas")}
                    {move || if hide_betas.get() { " \u{2715}" } else { "" }}
                </button>

                <button
                    class=move || {
                        if hide_clones.get() {
                            "filter-chip filter-chip-active"
                        } else {
                            "filter-chip"
                        }
                    }
                    on:click=move |_| hide_clones.update(|v| *v = !*v)
                >
                    {move || t(i18n.locale.get(), "filter.hide_clones")}
                    {move || if hide_clones.get() { " \u{2715}" } else { "" }}
                </button>

                {move || {
                    genres_resource.get().and_then(|res| res.ok()).map(|genre_list| {
                        view! { <GenreDropdown genre genre_list /> }
                    })
                }}
            </div>

            <Transition fallback=move || view! {
                <div class="loading">{move || t(i18n.locale.get(), "common.loading")}</div>
            }>
                {move || Suspend::new(async move {
                    let locale = i18n.locale.get();
                    let data = results.await?;
                    let q = debounced_query.get_untracked();
                    let hh = hide_hacks.get_untracked();
                    let ht = hide_translations.get_untracked();
                    let hb = hide_betas.get_untracked();
                    let hc = hide_clones.get_untracked();
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

/// Genre dropdown filter.
#[component]
fn GenreDropdown(genre: RwSignal<String>, genre_list: Vec<String>) -> impl IntoView {
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

/// A single search result row.
#[component]
fn SearchResultItem(result: GlobalSearchResult) -> impl IntoView {
    let href = format!(
        "/games/{}/{}",
        result.system,
        urlencoding::encode(&result.rom_filename)
    );
    let href = StoredValue::new(href);
    let has_box_art = result.box_art_url.is_some();
    let box_art = StoredValue::new(result.box_art_url.clone());
    let star = if result.is_favorite {
        "\u{2605}"
    } else {
        ""
    };
    let genre = StoredValue::new(result.genre.clone());
    let has_genre = !result.genre.is_empty();
    let display_name = result.display_name.clone();

    view! {
        <div class="search-result-item">
            <Show when=move || has_box_art>
                <A href=href.get_value() attr:class="search-result-thumb-link">
                    <img class="search-result-thumb" src=box_art.get_value() loading="lazy" />
                </A>
            </Show>
            <div class="search-result-info">
                <A href=href.get_value() attr:class="search-result-name rom-name-link">
                    {display_name}
                </A>
                <div class="search-result-badges">
                    <Show when=move || has_genre>
                        <span class="search-badge search-badge-genre">{genre.get_value()}</span>
                    </Show>
                    <Show when=move || !star.is_empty()>
                        <span class="search-badge search-badge-fav">{star}</span>
                    </Show>
                </div>
            </div>
        </div>
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
    genre: &str,
) {
    #[cfg(feature = "hydrate")]
    update_url_params(query, hide_hacks, hide_translations, hide_betas, hide_clones, genre);
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
