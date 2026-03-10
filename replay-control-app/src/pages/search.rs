use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_query_map;

use crate::i18n::{t, use_i18n};
use crate::server_fns::{self, GlobalSearchResult, GlobalSearchResults, SystemSearchGroup};

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
    let initial_genre = query_map
        .read_untracked()
        .get("genre")
        .map(|s| s.to_string())
        .unwrap_or_default();

    // Signals for user input.
    let search_input = RwSignal::new(initial_query.clone());
    let hide_hacks = RwSignal::new(initial_hide_hacks);
    let genre = RwSignal::new(initial_genre.clone());

    // Debounced search query that drives the resource.
    let debounced_query = RwSignal::new(initial_query);
    let debounced_genre = RwSignal::new(initial_genre);

    // Genre list resource.
    let genres_resource = Resource::new(|| (), |_| server_fns::get_all_genres());

    // Debounce the search input (400ms).
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
                update_url_params(&val, hide_hacks.get_untracked(), &genre.get_untracked());
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
            let g = genre.get();
            debounced_genre.set(g.clone());
            update_url_params(&debounced_query.get_untracked(), hh, &g);
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
                debounced_genre.get(),
            )
        },
        |(q, hh, g)| server_fns::global_search(q, hh, g, 3),
    );

    view! {
        <div class="page search-page">
            <div class="search-page-bar">
                <input
                    type="text"
                    class="search-page-input"
                    placeholder=move || t(i18n.locale.get(), "search.placeholder")
                    prop:value=move || search_input.get()
                    on:input=move |ev| search_input.set(event_target_value(&ev))
                    autofocus=true
                />
            </div>

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

                <ErrorBoundary fallback=|_| ()>
                    <Suspense fallback=|| ()>
                        {move || Suspend::new(async move {
                            let genre_list = genres_resource.await?;
                            Ok::<_, server_fn::ServerFnError>(view! {
                                <GenreDropdown genre genre_list />
                            })
                        })}
                    </Suspense>
                </ErrorBoundary>
            </div>

            <Transition fallback=move || view! {
                <div class="loading">{move || t(i18n.locale.get(), "common.loading")}</div>
            }>
                {move || Suspend::new(async move {
                    let locale = i18n.locale.get();
                    let data = results.await?;
                    let q = debounced_query.get_untracked();
                    let hh = hide_hacks.get_untracked();
                    let g = debounced_genre.get_untracked();
                    Ok::<_, server_fn::ServerFnError>(view! {
                        <SearchResults data locale query=q hide_hacks=hh genre=g />
                    })
                })}
            </Transition>
        </div>
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
    genre: String,
) -> impl IntoView {
    let has_results = !data.groups.is_empty();

    if !has_results {
        return view! {
            <p class="empty-state">{t(locale, "search.no_results")}</p>
        }
        .into_any();
    }

    let summary = format!(
        "{} {} {} {}",
        data.total_results,
        t(locale, "search.results_summary"),
        data.total_systems,
        t(locale, "search.systems")
    );

    // Build query string for "See all" links.
    let filter_qs = StoredValue::new({
        let mut params = Vec::new();
        if !query.is_empty() {
            params.push(format!("search={}", urlencoding::encode(&query)));
        }
        if hide_hacks {
            params.push("hide_hacks=true".to_string());
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
fn update_url_params(query: &str, hide_hacks: bool, genre: &str) {
    if let Some(window) = web_sys::window() {
        let mut params = Vec::new();
        if !query.is_empty() {
            params.push(format!("q={}", urlencoding::encode(query)));
        }
        if hide_hacks {
            params.push("hide_hacks=true".to_string());
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
