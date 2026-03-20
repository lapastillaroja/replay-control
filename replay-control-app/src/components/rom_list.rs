use leptos::prelude::*;
use leptos_router::NavigateOptions;
use leptos_router::components::A;
use leptos_router::hooks::{query_signal_with_options, use_query_map};

use crate::components::filter_chips::{FilterChips, FilterState};
use crate::components::game_list_item::GameListItem;
use crate::i18n::{t, use_i18n};
use crate::server_fns::{self, PAGE_SIZE, RomListEntry};

/// ROM list with built-in search, pagination, and infinite scroll.
#[component]
pub fn RomList(system: String) -> impl IntoView {
    let i18n = use_i18n();
    let sys = StoredValue::new(system.clone());

    // Read filter params from URL query (passed from global search "See all" links).
    let query_map = use_query_map();
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

    let filters = FilterState {
        hide_hacks: RwSignal::new(initial_hide_hacks),
        hide_translations: RwSignal::new(initial_hide_translations),
        hide_betas: RwSignal::new(initial_hide_betas),
        hide_clones: RwSignal::new(initial_hide_clones),
        multiplayer_only: RwSignal::new(initial_multiplayer),
        genre: RwSignal::new(initial_genre.clone()),
        min_rating: RwSignal::new(None),
    };
    let debounced_genre = RwSignal::new(initial_genre);

    // Track whether the system is arcade (set from first page response).
    let is_arcade = RwSignal::new(false);

    // Genre list resource for this system.
    let sys_for_genres = system.clone();
    let genres_resource = Resource::new(
        move || sys_for_genres.clone(),
        server_fns::get_system_genres,
    );

    // Search: synced with URL query param `?search=...`.
    // Use replace mode so each keystroke doesn't add a history entry.
    let (search_query, set_search_query) = query_signal_with_options::<String>(
        "search",
        NavigateOptions {
            replace: true,
            scroll: false,
            ..Default::default()
        },
    );
    // set_search_query is used in the hydrate block below.
    let _ = &set_search_query;

    // Local input signal tracks what the user is typing (immediate), while
    // debounced_search drives the Resource (delayed).
    let search_input = RwSignal::new(search_query.get_untracked().unwrap_or_default());
    let debounced_search = RwSignal::new(search_query.get_untracked().unwrap_or_default());

    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::prelude::*;

        let timer_handle: StoredValue<Option<i32>> = StoredValue::new(None);
        Effect::new(move || {
            let val = search_input.get();
            if let Some(handle) = timer_handle.get_value()
                && let Some(w) = web_sys::window()
            {
                w.clear_timeout_with_handle(handle);
            }
            let cb = Closure::<dyn Fn()>::new(move || {
                debounced_search.set(val.clone());
                // Sync to URL query param.
                if val.is_empty() {
                    set_search_query.set(None);
                } else {
                    set_search_query.set(Some(val.clone()));
                }
            });
            if let Some(window) = web_sys::window()
                && let Ok(handle) = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                    cb.as_ref().unchecked_ref(),
                    300,
                )
            {
                timer_handle.set_value(Some(handle));
            }
            cb.forget();
        });

        // Sync from URL -> input when query param changes externally (e.g., back button).
        Effect::new(move || {
            let url_val = search_query.get().unwrap_or_default();
            if url_val != search_input.get_untracked() {
                search_input.set(url_val.clone());
                debounced_search.set(url_val);
            }
        });

        // Immediate update for filter toggle changes (no debounce needed).
        // Skip the first run: URL already reflects initial values and the
        // Router's pushState hasn't completed yet. Calling replaceState before
        // pushState would overwrite the previous page's history entry.
        let filters_initialized = StoredValue::new(false);
        Effect::new(move || {
            let hh = filters.hide_hacks.get();
            let ht = filters.hide_translations.get();
            let hb = filters.hide_betas.get();
            let hc = filters.hide_clones.get();
            let mp = filters.multiplayer_only.get();
            let g = filters.genre.get();
            debounced_genre.set(g.clone());
            if !filters_initialized.get_value() {
                filters_initialized.set_value(true);
                return;
            }
            update_filter_url(
                sys.get_value(),
                hh,
                ht,
                hb,
                hc,
                mp,
                &g,
                &debounced_search.get_untracked(),
            );
        });

        // Clean up pending timer on unmount.
        on_cleanup(move || {
            if let Some(handle) = timer_handle.get_value()
                && let Some(w) = web_sys::window()
            {
                w.clear_timeout_with_handle(handle);
            }
        });
    }

    // Extra ROMs loaded after the first page.
    let (extra_roms, set_extra_roms) = signal(Vec::<RomListEntry>::new());
    let (has_more, set_has_more) = signal(false);
    let (loading_more, set_loading_more) = signal(false);
    let (offset, set_offset) = signal(PAGE_SIZE);

    // First page -- resolves during SSR.
    let first_page = Resource::new(
        move || {
            (
                sys.get_value(),
                debounced_search.get(),
                filters.hide_hacks.get(),
                filters.hide_translations.get(),
                filters.hide_betas.get(),
                filters.hide_clones.get(),
                debounced_genre.get(),
                filters.multiplayer_only.get(),
                filters.min_rating.get(),
            )
        },
        move |(system, query, hh, ht, hb, hc, gf, mp, mr)| {
            server_fns::get_roms_page(system, 0, PAGE_SIZE, query, hh, ht, hb, hc, gf, mp, mr)
        },
    );

    // When first page changes, reset extra roms and update has_more + is_arcade.
    Effect::new(move || {
        if let Some(Ok(page)) = first_page.get() {
            set_has_more.set(page.has_more);
            set_extra_roms.set(Vec::new());
            set_offset.set(PAGE_SIZE);
            is_arcade.set(page.is_arcade);
        }
    });

    // Load more function.
    let load_more = move || {
        if loading_more.get() || !has_more.get() {
            return;
        }
        set_loading_more.set(true);
        let system = sys.get_value();
        let query = debounced_search.get_untracked();
        let current_offset = offset.get_untracked();
        let hh = filters.hide_hacks.get_untracked();
        let ht = filters.hide_translations.get_untracked();
        let hb = filters.hide_betas.get_untracked();
        let hc = filters.hide_clones.get_untracked();
        let gf = debounced_genre.get_untracked();
        let mp = filters.multiplayer_only.get_untracked();
        let mr = filters.min_rating.get_untracked();
        leptos::task::spawn_local(async move {
            if let Ok(page) = server_fns::get_roms_page(
                system,
                current_offset,
                PAGE_SIZE,
                query,
                hh,
                ht,
                hb,
                hc,
                gf,
                mp,
                mr,
            )
            .await
            {
                set_extra_roms.update(|roms| roms.extend(page.roms));
                set_has_more.set(page.has_more);
                set_offset.update(|o| *o += PAGE_SIZE);
            }
            set_loading_more.set(false);
        });
    };

    // Sentinel ref for infinite scroll.
    let sentinel_ref = NodeRef::<leptos::html::Div>::new();

    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::prelude::*;
        use web_sys::js_sys;

        let load_more_for_observer = load_more;
        Effect::new(move || {
            let Some(el) = sentinel_ref.get() else { return };

            let cb = Closure::<dyn Fn(js_sys::Array)>::new(move |entries: js_sys::Array| {
                for entry in entries.iter() {
                    if let Ok(entry) = entry.dyn_into::<web_sys::IntersectionObserverEntry>()
                        && entry.is_intersecting()
                    {
                        load_more_for_observer();
                    }
                }
            });

            let opts = web_sys::IntersectionObserverInit::new();
            opts.set_root_margin("200px");

            if let Ok(observer) =
                web_sys::IntersectionObserver::new_with_options(cb.as_ref().unchecked_ref(), &opts)
            {
                let obs_for_cleanup = observer.clone();
                observer.observe(&el);
                on_cleanup(move || {
                    obs_for_cleanup.disconnect();
                });
            }

            cb.forget();
        });
    }

    // The search bar and filter bar are rendered outside the Suspense/Transition block
    // so that the input element is never recreated when search results update, which
    // preserves keyboard focus while typing.
    view! {
        <div class="search-bar">
            <input
                type="text"
                placeholder=move || t(i18n.locale.get(), "games.search_placeholder")
                class="search-input"
                prop:value=move || search_input.get()
                on:input=move |ev| search_input.set(event_target_value(&ev))
            />
        </div>
        <div class="search-filters rom-list-filters">
            <FilterChips
                filters
                show_clones=Signal::derive(move || is_arcade.get())
            />
            <Suspense>
                {move || Suspend::new(async move {
                    match genres_resource.await {
                        Ok(genre_list) if !genre_list.is_empty() => {
                            Some(view! { <crate::components::genre_dropdown::GenreDropdown genre=filters.genre genre_list /> })
                        }
                        _ => None,
                    }
                })}
            </Suspense>
        </div>
        <Transition fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), "games.loading_roms")}</div> }>
            {move || Suspend::new(async move {
                let locale = i18n.locale.get();
                match first_page.await {
                    Ok(page) => {
                        set_has_more.set(page.has_more);
                        is_arcade.set(page.is_arcade);
                        let total = page.total;
                        let first_page_len = page.roms.len();
                        let display_name = page.system_display.clone();
                        let count_text = move || {
                            let loaded = first_page_len + extra_roms.read().len();
                            if loaded < total {
                                format!("{loaded} / {total} {}", t(locale, "stats.games").to_lowercase())
                            } else {
                                format!("{total} {}", t(locale, "stats.games").to_lowercase())
                            }
                        };

                        view! {
                            <div class="rom-header">
                                <A href="/" attr:class="back-btn">
                                    {t(locale, "games.back")}
                                </A>
                                <h2 class="page-title">{display_name}</h2>
                            </div>
                            <p class="rom-count">{count_text}</p>
                            <div class="rom-list">
                                // First page ROMs (from SSR).
                                {page.roms.into_iter().map(|rom| {
                                    let genre = if rom.genre.is_empty() { None } else { Some(rom.genre.clone()) };
                                    view! {
                                        <GameListItem
                                            system=rom.system
                                            rom_filename=rom.rom_filename
                                            display_name=rom.display_name
                                            rom_path=rom.rom_path
                                            box_art_url=rom.box_art_url
                                            is_favorite=rom.is_favorite
                                            genre
                                            rating=rom.rating
                                            driver_status=rom.driver_status
                                            show_system=false
                                            show_favorite=true
                                        />
                                    }
                                }).collect::<Vec<_>>()}

                                // Extra ROMs from subsequent pages.
                                {move || {
                                    extra_roms.get().into_iter().map(|rom| {
                                        let genre = if rom.genre.is_empty() { None } else { Some(rom.genre.clone()) };
                                        view! {
                                            <GameListItem
                                                system=rom.system
                                                rom_filename=rom.rom_filename
                                                display_name=rom.display_name
                                                rom_path=rom.rom_path
                                                box_art_url=rom.box_art_url
                                                is_favorite=rom.is_favorite
                                                genre
                                                rating=rom.rating
                                                driver_status=rom.driver_status
                                                show_system=false
                                                show_favorite=true
                                            />
                                        }
                                    }).collect::<Vec<_>>()
                                }}

                                // Sentinel for infinite scroll.
                                <Show when=move || has_more.get()>
                                    <div class="load-more-sentinel" node_ref=sentinel_ref>
                                        <button
                                            class="load-more-btn"
                                            disabled=move || loading_more.get()
                                            on:click=move |_| load_more()
                                        >
                                            {move || if loading_more.get() {
                                                t(i18n.locale.get(), "common.loading")
                                            } else {
                                                t(i18n.locale.get(), "games.load_more")
                                            }}
                                        </button>
                                    </div>
                                </Show>
                            </div>
                        }.into_any()
                    }
                    Err(e) => {
                        view! { <p class="error">{format!("{}: {e}", t(locale, "common.error"))}</p> }.into_any()
                    }
                }
            })}
        </Transition>
    }
}

/// Update the URL query params for the ROM list page (replace, no navigation).
/// Keeps all filter state in sync with the URL.
#[cfg(feature = "hydrate")]
#[allow(clippy::too_many_arguments)]
fn update_filter_url(
    system: String,
    hide_hacks: bool,
    hide_translations: bool,
    hide_betas: bool,
    hide_clones: bool,
    multiplayer_only: bool,
    genre: &str,
    search: &str,
) {
    if let Some(window) = web_sys::window() {
        let mut params = Vec::new();
        if !search.is_empty() {
            params.push(format!("search={}", urlencoding::encode(search)));
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
        let url = format!("/games/{system}{qs}");
        let _ = window
            .history()
            .and_then(|h| h.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&url)));
    }
}
