use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_params_map;

use crate::components::filter_chips::{FilterChips, FilterState};
use crate::i18n::{t, use_i18n};
use crate::server_fns::{self, DeveloperSystem, RomListEntry, PAGE_SIZE};

/// `/developer/:name` — Game list for a specific developer with system filter chips.
#[component]
pub fn DeveloperPage() -> impl IntoView {
    let i18n = use_i18n();
    let params = use_params_map();
    let developer = params
        .read_untracked()
        .get("name")
        .unwrap_or_default();

    let dev = StoredValue::new(developer.clone());

    // Active system filter signal (empty = all systems).
    let system_filter = RwSignal::new(String::new());

    // Content filter state (shared with FilterChips component).
    let filters = FilterState {
        hide_hacks: RwSignal::new(false),
        hide_translations: RwSignal::new(false),
        hide_betas: RwSignal::new(false),
        hide_clones: RwSignal::new(false),
        multiplayer_only: RwSignal::new(false),
        genre: RwSignal::new(String::new()),
        min_rating: RwSignal::new(None),
    };
    let debounced_genre = RwSignal::new(String::new());

    // Genre list resource — depends on developer and system filter.
    let genres_resource = Resource::new(
        move || (dev.get_value(), system_filter.get()),
        move |(developer, system)| server_fns::get_developer_genres(developer, system),
    );

    // Debounce genre changes on hydrate.
    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::prelude::*;

        let genre_timer: StoredValue<Option<i32>> = StoredValue::new(None);
        Effect::new(move || {
            let val = filters.genre.get();
            if let Some(handle) = genre_timer.get_value()
                && let Some(w) = web_sys::window()
            {
                w.clear_timeout_with_handle(handle);
            }
            let cb = Closure::<dyn Fn()>::new(move || {
                debounced_genre.set(val.clone());
            });
            if let Some(window) = web_sys::window()
                && let Ok(handle) = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                    cb.as_ref().unchecked_ref(),
                    300,
                )
            {
                genre_timer.set_value(Some(handle));
            }
            cb.forget();
        });

        on_cleanup(move || {
            if let Some(handle) = genre_timer.get_value()
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

    // First page resource — depends on all filter signals.
    let first_page = Resource::new(
        move || {
            (
                dev.get_value(),
                system_filter.get(),
                filters.hide_hacks.get(),
                filters.hide_translations.get(),
                filters.hide_clones.get(),
                debounced_genre.get(),
                filters.multiplayer_only.get(),
                filters.min_rating.get(),
            )
        },
        move |(developer, system, hh, ht, hc, gf, mp, mr)| {
            server_fns::get_developer_games(
                developer, system, 0, PAGE_SIZE, hh, ht, hc, mp, gf, mr,
            )
        },
    );

    // When first page changes, reset extra roms and update has_more.
    Effect::new(move || {
        if let Some(Ok(page)) = first_page.get() {
            set_has_more.set(page.has_more);
            set_extra_roms.set(Vec::new());
            set_offset.set(PAGE_SIZE);
        }
    });

    // Load more function.
    let load_more = move || {
        if loading_more.get() || !has_more.get() {
            return;
        }
        set_loading_more.set(true);
        let developer = dev.get_value();
        let system = system_filter.get_untracked();
        let current_offset = offset.get_untracked();
        let hh = filters.hide_hacks.get_untracked();
        let ht = filters.hide_translations.get_untracked();
        let hc = filters.hide_clones.get_untracked();
        let gf = debounced_genre.get_untracked();
        let mp = filters.multiplayer_only.get_untracked();
        let mr = filters.min_rating.get_untracked();
        leptos::task::spawn_local(async move {
            if let Ok(page) = server_fns::get_developer_games(
                developer,
                system,
                current_offset,
                PAGE_SIZE,
                hh,
                ht,
                hc,
                mp,
                gf,
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

    view! {
        <div class="page games-page developer-page">
            <div class="search-filters rom-list-filters">
                <FilterChips
                    filters
                    show_clones=Signal::derive(move || true)
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
            <Transition fallback=move || view! {
                <div class="loading">{move || t(i18n.locale.get(), "common.loading")}</div>
            }>
                {move || Suspend::new(async move {
                    let locale = i18n.locale.get();
                    match first_page.await {
                        Ok(page) => {
                            set_has_more.set(page.has_more);
                            let total = page.total;
                            let first_page_len = page.roms.len();
                            let developer_name = page.developer.clone();
                            let systems = page.systems.clone();
                            let count_text = move || {
                                let loaded = first_page_len + extra_roms.read().len();
                                if loaded < total {
                                    format!("{loaded} / {total} {}", t(locale, "stats.games").to_lowercase())
                                } else {
                                    format!("{total} {}", t(locale, "stats.games").to_lowercase())
                                }
                            };
                            let is_empty = page.roms.is_empty() && total == 0;

                            if is_empty {
                                view! {
                                    <div class="rom-header">
                                        <A href="/search" attr:class="back-btn">
                                            {t(locale, "games.back")}
                                        </A>
                                        <h2 class="page-title">{developer_name}</h2>
                                    </div>
                                    <p class="empty-state">{t(locale, "developer.no_games")}</p>
                                }.into_any()
                            } else {
                                view! {
                                    <div class="rom-header">
                                        <A href="/search" attr:class="back-btn">
                                            {t(locale, "games.back")}
                                        </A>
                                        <h2 class="page-title">{developer_name}</h2>
                                    </div>
                                    <p class="rom-count">{count_text}</p>
                                    <SystemFilterChips systems system_filter locale />
                                    <div class="rom-list">
                                        {page.roms.into_iter().map(|rom| {
                                            view! { <DeveloperRomItem rom /> }
                                        }).collect::<Vec<_>>()}

                                        {move || {
                                            extra_roms.get().into_iter().map(|rom| {
                                                view! { <DeveloperRomItem rom /> }
                                            }).collect::<Vec<_>>()
                                        }}

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
                        }
                        Err(e) => {
                            view! { <p class="error">{format!("{}: {e}", t(locale, "common.error"))}</p> }.into_any()
                        }
                    }
                })}
            </Transition>
        </div>
    }
}

/// System filter chip row for the developer page.
#[component]
fn SystemFilterChips(
    systems: Vec<DeveloperSystem>,
    system_filter: RwSignal<String>,
    locale: crate::i18n::Locale,
) -> impl IntoView {
    if systems.len() <= 1 {
        // No point showing filter chips if there's only one system.
        return view! { <div /> }.into_any();
    }

    let total_count: usize = systems.iter().map(|s| s.game_count).sum();
    let all_label = format!("{} ({})", t(locale, "developer.all_systems"), total_count);

    view! {
        <div class="system-filter-chips">
            <button
                class=move || if system_filter.read().is_empty() {
                    "system-chip system-chip-active"
                } else {
                    "system-chip"
                }
                on:click=move |_| system_filter.set(String::new())
            >
                {all_label}
            </button>
            {systems.into_iter().map(|sys| {
                let sys_id = sys.system.clone();
                let label = format!("{} ({})", sys.system_display, sys.game_count);
                let sys_for_check = sys_id.clone();
                view! {
                    <button
                        class=move || if *system_filter.read() == sys_for_check {
                            "system-chip system-chip-active"
                        } else {
                            "system-chip"
                        }
                        on:click=move |_| system_filter.set(sys_id.clone())
                    >
                        {label}
                    </button>
                }
            }).collect::<Vec<_>>()}
        </div>
    }.into_any()
}

/// A single ROM row for the developer page.
/// Simplified version of the system page RomItem -- no rename/delete actions,
/// since this is a cross-system view.
#[component]
fn DeveloperRomItem(rom: RomListEntry) -> impl IntoView {
    let filename = StoredValue::new(rom.rom_filename.clone());
    let display_name = StoredValue::new(rom.display_name.clone());
    let system = StoredValue::new(rom.system.clone());
    let box_art_url = StoredValue::new(rom.box_art_url.clone());
    let has_box_art = rom.box_art_url.is_some();
    let genre = StoredValue::new(rom.genre.clone());
    let has_genre = !rom.genre.is_empty();
    let rating = rom.rating;

    let game_href = format!(
        "/games/{}/{}",
        rom.system,
        urlencoding::encode(&rom.rom_filename)
    );
    let game_href = StoredValue::new(game_href);

    // Resolve system display name.
    let system_display = StoredValue::new({
        #[cfg(feature = "ssr")]
        {
            replay_control_core::systems::find_system(&rom.system)
                .map(|s| s.display_name.to_string())
                .unwrap_or_else(|| rom.system.clone())
        }
        #[cfg(not(feature = "ssr"))]
        {
            rom.system.clone()
        }
    });

    // Quick-favorite toggle.
    let is_fav = RwSignal::new(rom.is_favorite);
    let rom_path = StoredValue::new(rom.rom_path.clone());
    let on_toggle_fav = move |_| {
        let fav = is_fav.get();
        is_fav.set(!fav);
        let fname = filename.get_value();
        let sys = system.get_value();
        let rp = rom_path.get_value();
        if fav {
            let fav_filename = format!("{sys}@{fname}.fav");
            leptos::task::spawn_local(async move {
                let _ = server_fns::remove_favorite(fav_filename, None).await;
            });
        } else {
            leptos::task::spawn_local(async move {
                let _ = server_fns::add_favorite(sys, rp, false).await;
            });
        }
    };

    let star = move || if is_fav.get() { "\u{2605}" } else { "\u{2606}" };

    view! {
        <div class="rom-item">
            <button class="rom-fav-btn" on:click=on_toggle_fav>{star}</button>

            <A href=game_href.get_value() attr:class="rom-thumb-link">
                {if has_box_art {
                    view! { <img class="rom-thumb" src=box_art_url.get_value() loading="lazy" width="56" height="40" /> }.into_any()
                } else {
                    view! { <div class="rom-thumb-placeholder"></div> }.into_any()
                }}
            </A>

            <div class="rom-info">
                <div class="rom-name-row">
                    <A href=game_href.get_value() attr:class="rom-name rom-name-link">
                        {display_name.get_value()}
                    </A>
                </div>
                <div class="rom-badges">
                    <span class="rom-path">{system_display.get_value()}</span>
                    <Show when=move || has_genre>
                        <span class="search-badge search-badge-genre">{genre.get_value()}</span>
                    </Show>
                    {rating.filter(|&r| r > 0.0).map(|r| {
                        let label = format!("\u{2605} {:.1}", r);
                        view! { <span class="search-badge search-badge-rating">{label}</span> }
                    })}
                </div>
            </div>
        </div>
    }
}
