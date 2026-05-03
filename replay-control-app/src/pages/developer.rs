use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::{use_params_map, use_query_map};

use crate::components::filter_chips::{FilterChips, FilterState};
use crate::components::game_list_item::GameListItem;
use crate::hooks::use_infinite_scroll;
use crate::i18n::{Key, t, tf, use_i18n};
use crate::server_fns::{self, DeveloperSystem, PAGE_SIZE, RomListEntry};

/// `/developer/:name` — Game list for a specific developer with system filter chips.
#[component]
pub fn DeveloperPage() -> impl IntoView {
    let i18n = use_i18n();
    let params = use_params_map();
    let developer = params.read_untracked().get("name").unwrap_or_default();

    // Wrap in a Memo so it's reactive (tracked in Resource source closures).
    // StoredValue::get_value() is not tracked by the reactive graph, which can
    // prevent Resource from re-subscribing to other signals in the source tuple
    // after hydration.
    let dev = Memo::new({
        let d = developer.clone();
        move |_| d.clone()
    });

    // Read filter params from URL query (persisted across reloads).
    let query_map = use_query_map();
    let qm = query_map.read_untracked();

    // Active system filter signal — read initial value from URL param `system`.
    let system_filter = RwSignal::new(qm.get("system").unwrap_or_default());

    // Content filter state (shared with FilterChips component).
    let filters = FilterState::from_query_map(&qm);
    drop(qm);
    let debounced_genre = RwSignal::new(filters.genre_untracked());

    // Sync filter changes to URL (hydrate-only).
    #[cfg(feature = "hydrate")]
    {
        let dev_for_url = dev;
        let filters_initialized = StoredValue::new(false);
        Effect::new(move || {
            let sys = system_filter.get();
            let hh = filters.hide_hacks.get();
            let ht = filters.hide_translations.get();
            let hb = filters.hide_betas.get();
            let hc = filters.hide_clones.get();
            let mp = filters.multiplayer_only.get();
            let g = filters.genre.get();
            let mr = filters.min_rating.get();
            let miny = filters.min_year.get();
            let maxy = filters.max_year.get();
            debounced_genre.set(g.clone());
            if !filters_initialized.get_value() {
                filters_initialized.set_value(true);
                return;
            }
            update_developer_url(
                &dev_for_url.get_untracked(),
                &DeveloperUrlParams {
                    system: &sys,
                    hide_hacks: hh,
                    hide_translations: ht,
                    hide_betas: hb,
                    hide_clones: hc,
                    multiplayer_only: mp,
                    genre: &g,
                    min_rating: mr,
                    min_year: miny,
                    max_year: maxy,
                },
            );
        });
    }

    // Genre list resource — depends on developer and system filter.
    let genres_resource = Resource::new(
        move || (dev.get(), system_filter.get()),
        move |(developer, system)| server_fns::get_developer_genres(developer, system),
    );

    // Extra ROMs loaded after the first page.
    let (extra_roms, set_extra_roms) = signal(Vec::<RomListEntry>::new());
    let (has_more, set_has_more) = signal(false);
    let (loading_more, set_loading_more) = signal(false);
    let (offset, set_offset) = signal(PAGE_SIZE);

    // First page resource — depends on all filter signals.
    let first_page = Resource::new(
        move || {
            (
                dev.get(),
                system_filter.get(),
                filters.hide_hacks.get(),
                filters.hide_translations.get(),
                filters.hide_betas.get(),
                filters.hide_clones.get(),
                debounced_genre.get(),
                filters.multiplayer_only.get(),
                filters.min_rating.get(),
                filters.min_year.get(),
                filters.max_year.get(),
            )
        },
        move |(developer, system, hh, ht, hb, hc, gf, mp, mr, miny, maxy)| {
            server_fns::get_developer_games(
                developer, system, 0, PAGE_SIZE, hh, ht, hb, hc, mp, gf, mr, miny, maxy,
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
        if loading_more.get_untracked() || !has_more.get_untracked() {
            return;
        }
        set_loading_more.set(true);
        let developer = dev.get_untracked();
        let system = system_filter.get_untracked();
        let current_offset = offset.get_untracked();
        let hh = filters.hide_hacks.get_untracked();
        let ht = filters.hide_translations.get_untracked();
        let hb = filters.hide_betas.get_untracked();
        let hc = filters.hide_clones.get_untracked();
        let gf = debounced_genre.get_untracked();
        let mp = filters.multiplayer_only.get_untracked();
        let mr = filters.min_rating.get_untracked();
        let miny = filters.min_year.get_untracked();
        let maxy = filters.max_year.get_untracked();
        leptos::task::spawn_local(async move {
            if let Ok(page) = server_fns::get_developer_games(
                developer,
                system,
                current_offset,
                PAGE_SIZE,
                hh,
                ht,
                hb,
                hc,
                mp,
                gf,
                mr,
                miny,
                maxy,
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
    use_infinite_scroll(sentinel_ref, load_more);

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
                <div class="loading">{move || t(i18n.locale.get(), Key::CommonLoading)}</div>
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
                            // Build a lookup map from system folder -> display name
                            // so GameListItem can show the correct system name on
                            // client-side navigation (where SSR lookup is unavailable).
                            let system_display_map: std::collections::HashMap<String, String> =
                                systems.iter().map(|s| (s.system.clone(), s.system_display.clone())).collect();
                            let system_display_map = StoredValue::new(system_display_map);
                            let count_text = move || {
                                let loaded = first_page_len + extra_roms.read().len();
                                if loaded < total {
                                    tf(locale, Key::CountGamesPartial, &[&loaded.to_string(), &total.to_string()])
                                } else {
                                    tf(locale, Key::CountGames, &[&total.to_string()])
                                }
                            };
                            let is_empty = page.roms.is_empty() && total == 0;

                            if is_empty {
                                view! {
                                    <div class="rom-header">
                                        <A href="/search" attr:class="back-btn">
                                            {t(locale, Key::GamesBack)}
                                        </A>
                                        <h2 class="page-title">{developer_name}</h2>
                                    </div>
                                    <p class="empty-state">{t(locale, Key::DeveloperNoGames)}</p>
                                }.into_any()
                            } else {
                                view! {
                                    <div class="rom-header">
                                        <A href="/search" attr:class="back-btn">
                                            {t(locale, Key::GamesBack)}
                                        </A>
                                        <h2 class="page-title">{developer_name}</h2>
                                    </div>
                                    <p class="rom-count">{count_text}</p>
                                    <SystemFilterChips systems system_filter locale />
                                    <div class="rom-list">
                                        {page.roms.into_iter().map(|rom| {
                                            {
                                                let genre = (!rom.genre.is_empty()).then(|| rom.genre.clone());
                                                let sys_display = system_display_map.get_value().get(&rom.system).cloned();
                                                let base_title = replay_control_core::title_utils::base_title(&rom.display_name);
                                                view! { <GameListItem
                                                    system=rom.system.clone()
                                                    rom_filename=rom.rom_filename.clone()
                                                    display_name=rom.display_name.clone()
                                                    rom_path=rom.rom_path.clone()
                                                    box_art_url=rom.box_art_url.clone()
                                                    show_system=true
                                                    is_favorite=rom.is_favorite
                                                    genre=genre
                                                    rating=rom.rating
                                                    driver_status=rom.driver_status.clone()
                                                    system_display=sys_display
                                                    has_manual=rom.has_manual
                                                    base_title=Some(base_title)
                                                /> }
                                            }
                                        }).collect::<Vec<_>>()}

                                        {move || {
                                            extra_roms.get().into_iter().map(|rom| {
                                                let genre = (!rom.genre.is_empty()).then(|| rom.genre.clone());
                                                let sys_display = system_display_map.get_value().get(&rom.system).cloned();
                                                let base_title = replay_control_core::title_utils::base_title(&rom.display_name);
                                                view! { <GameListItem
                                                    system=rom.system.clone()
                                                    rom_filename=rom.rom_filename.clone()
                                                    display_name=rom.display_name.clone()
                                                    rom_path=rom.rom_path.clone()
                                                    box_art_url=rom.box_art_url.clone()
                                                    show_system=true
                                                    is_favorite=rom.is_favorite
                                                    genre=genre
                                                    rating=rom.rating
                                                    driver_status=rom.driver_status.clone()
                                                    system_display=sys_display
                                                    has_manual=rom.has_manual
                                                    base_title=Some(base_title)
                                                /> }
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
                                                        t(i18n.locale.get(), Key::CommonLoading)
                                                    } else {
                                                        t(i18n.locale.get(), Key::GamesLoadMore)
                                                    }}
                                                </button>
                                            </div>
                                        </Show>
                                    </div>
                                }.into_any()
                            }
                        }
                        Err(e) => {
                            view! { <p class="error">{format!("{}: {e}", t(locale, Key::CommonError))}</p> }.into_any()
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
    let all_label = format!("{} ({})", t(locale, Key::DeveloperAllSystems), total_count);

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
    }
    .into_any()
}

/// Parameters for updating the developer page URL query string.
#[cfg(feature = "hydrate")]
struct DeveloperUrlParams<'a> {
    system: &'a str,
    hide_hacks: bool,
    hide_translations: bool,
    hide_betas: bool,
    hide_clones: bool,
    multiplayer_only: bool,
    genre: &'a str,
    min_rating: Option<f32>,
    min_year: Option<u16>,
    max_year: Option<u16>,
}

/// Update the URL query params for the developer page (replace, no navigation).
#[cfg(feature = "hydrate")]
fn update_developer_url(developer: &str, p: &DeveloperUrlParams<'_>) {
    if let Some(window) = web_sys::window() {
        let mut params = Vec::new();
        if !p.system.is_empty() {
            params.push(format!("system={}", urlencoding::encode(p.system)));
        }
        if p.hide_hacks {
            params.push("hide_hacks=true".to_string());
        }
        if p.hide_translations {
            params.push("hide_translations=true".to_string());
        }
        if p.hide_betas {
            params.push("hide_betas=true".to_string());
        }
        if p.hide_clones {
            params.push("hide_clones=true".to_string());
        }
        if p.multiplayer_only {
            params.push("multiplayer=true".to_string());
        }
        if !p.genre.is_empty() {
            params.push(format!("genre={}", urlencoding::encode(p.genre)));
        }
        if let Some(mr) = p.min_rating {
            params.push(format!("min_rating={mr}"));
        }
        if let Some(y) = p.min_year {
            params.push(format!("min_year={y}"));
        }
        if let Some(y) = p.max_year {
            params.push(format!("max_year={y}"));
        }
        let qs = if params.is_empty() {
            String::new()
        } else {
            format!("?{}", params.join("&"))
        };
        let url = format!("/developer/{}{qs}", urlencoding::encode(developer));
        let _ = window
            .history()
            .and_then(|h| h.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&url)));
    }
}
