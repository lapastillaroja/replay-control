use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::{use_params_map, use_query_map};

use crate::components::filter_chips::{FilterChips, FilterState};
use crate::components::game_list_item::GameListItem;
use crate::hooks::use_infinite_scroll;
use crate::i18n::{Key, t, tf, use_i18n};
use crate::server_fns::{self, BoardSystem, PAGE_SIZE, RomListEntry};

/// `/board/:tag` — Game list for an arcade board, mirroring `/developer/:name`.
///
/// The page shares the same filter / system-chip / genre-dropdown / infinite
/// scroll surface as the developer page; the only structural difference is
/// the implicit filter is `SearchFilter::board` instead of `developer`.
#[component]
pub fn BoardPage() -> impl IntoView {
    let i18n = use_i18n();
    let params = use_params_map();
    let board_tag = params.read_untracked().get("tag").unwrap_or_default();

    let tag = Memo::new({
        let t = board_tag.clone();
        move |_| t.clone()
    });

    let query_map = use_query_map();
    let qm = query_map.read_untracked();

    let system_filter = RwSignal::new(qm.get("system").unwrap_or_default());

    let filters = FilterState::from_query_map(&qm);
    drop(qm);
    let debounced_genre = RwSignal::new(filters.genre_untracked());

    #[cfg(feature = "hydrate")]
    {
        let tag_for_url = tag;
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
            let ha = filters.has_achievements.get();
            debounced_genre.set(g.clone());
            if !filters_initialized.get_value() {
                filters_initialized.set_value(true);
                return;
            }
            update_board_url(
                &tag_for_url.get_untracked(),
                &BoardUrlParams {
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
                    has_achievements: ha,
                },
            );
        });
    }

    let genres_resource = Resource::new(
        move || (tag.get(), system_filter.get()),
        move |(board_tag, system)| server_fns::get_board_genres(board_tag, system),
    );

    let (extra_roms, set_extra_roms) = signal(Vec::<RomListEntry>::new());
    let (has_more, set_has_more) = signal(false);
    let (loading_more, set_loading_more) = signal(false);
    let (offset, set_offset) = signal(PAGE_SIZE);

    let first_page = Resource::new(
        move || {
            (
                tag.get(),
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
                filters.has_achievements.get(),
            )
        },
        move |(board_tag, system, hh, ht, hb, hc, gf, mp, mr, miny, maxy, ha)| {
            server_fns::get_board_games(
                board_tag, system, 0, PAGE_SIZE, hh, ht, hb, hc, mp, gf, mr, miny, maxy, ha,
            )
        },
    );

    Effect::new(move || {
        if let Some(Ok(page)) = first_page.get() {
            set_has_more.set(page.has_more);
            set_extra_roms.set(Vec::new());
            set_offset.set(PAGE_SIZE);
        }
    });

    let load_more = move || {
        if loading_more.get_untracked() || !has_more.get_untracked() {
            return;
        }
        set_loading_more.set(true);
        let board_tag = tag.get_untracked();
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
        let ha = filters.has_achievements.get_untracked();
        leptos::task::spawn_local(async move {
            if let Ok(page) = server_fns::get_board_games(
                board_tag,
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
                ha,
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

    let sentinel_ref = NodeRef::<leptos::html::Div>::new();
    use_infinite_scroll(sentinel_ref, load_more);

    view! {
        <div class="page games-page board-page">
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
                            let board_display = page.board_display_name.clone();
                            let systems = page.systems.clone();
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
                                        <h2 class="page-title">{board_display}</h2>
                                    </div>
                                    <p class="empty-state">{t(locale, Key::BoardNoGames)}</p>
                                }.into_any()
                            } else {
                                view! {
                                    <div class="rom-header">
                                        <A href="/search" attr:class="back-btn">
                                            {t(locale, Key::GamesBack)}
                                        </A>
                                        <h2 class="page-title">{board_display}</h2>
                                    </div>
                                    <p class="rom-count">{count_text}</p>
                                    <SystemFilterChips systems system_filter locale />
                                    <div class="rom-list">
                                        {page.roms.into_iter().map(|rom| {
                                            let genre = (!rom.genre.is_empty()).then(|| rom.genre.clone());
                                            let sys_display = system_display_map.get_value().get(&rom.system).cloned();
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
                                            /> }
                                        }).collect::<Vec<_>>()}

                                        {move || {
                                            extra_roms.get().into_iter().map(|rom| {
                                                let genre = (!rom.genre.is_empty()).then(|| rom.genre.clone());
                                                let sys_display = system_display_map.get_value().get(&rom.system).cloned();
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

#[component]
fn SystemFilterChips(
    systems: Vec<BoardSystem>,
    system_filter: RwSignal<String>,
    locale: crate::i18n::Locale,
) -> impl IntoView {
    if systems.len() <= 1 {
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

#[cfg_attr(not(feature = "hydrate"), allow(dead_code))]
struct BoardUrlParams<'a> {
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
    has_achievements: bool,
}

#[cfg(feature = "hydrate")]
fn update_board_url(board_tag: &str, p: &BoardUrlParams<'_>) {
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
        if p.has_achievements {
            params.push("has_achievements=true".to_string());
        }
        let qs = if params.is_empty() {
            String::new()
        } else {
            format!("?{}", params.join("&"))
        };
        let url = format!("/board/{}{qs}", urlencoding::encode(board_tag));
        let _ = window
            .history()
            .and_then(|h| h.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&url)));
    }
}
