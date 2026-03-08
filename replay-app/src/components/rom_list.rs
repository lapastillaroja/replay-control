use std::collections::HashSet;

use leptos::prelude::*;
use leptos_router::components::A;

use crate::i18n::{use_i18n, t};
use crate::server_fns::{self, RomEntry, PAGE_SIZE};
use crate::util::format_size;

/// ROM list with built-in search, pagination, and infinite scroll.
#[component]
pub fn RomList(system: String) -> impl IntoView {
    let i18n = use_i18n();
    let sys = StoredValue::new(system.clone());

    // Search: raw input updates immediately, debounced_search drives the Resource.
    // On SSR the initial empty string is correct; debounce only matters after hydration.
    let (search_input, set_search_input) = signal(String::new());
    let debounced_search = RwSignal::new(String::new());
    // search_input is read in the #[cfg(feature = "hydrate")] block below.
    let _ = &search_input;

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
                debounced_search.set(val.clone());
            });
            if let Some(window) = web_sys::window() {
                if let Ok(handle) = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                    cb.as_ref().unchecked_ref(),
                    300,
                ) {
                    timer_handle.set_value(Some(handle));
                }
            }
            cb.forget();
        });

        // Clean up pending timer on unmount.
        on_cleanup(move || {
            if let Some(handle) = timer_handle.get_value() {
                if let Some(w) = web_sys::window() {
                    w.clear_timeout_with_handle(handle);
                }
            }
        });
    }

    // Extra ROMs loaded after the first page.
    let (extra_roms, set_extra_roms) = signal(Vec::<RomEntry>::new());
    let (has_more, set_has_more) = signal(false);
    let (loading_more, set_loading_more) = signal(false);
    let (offset, set_offset) = signal(PAGE_SIZE);

    // Version bump to trigger re-fetch after delete/rename.
    let (version, set_version) = signal(0u32);

    // First page — resolves during SSR.
    let first_page = Resource::new(
        move || (sys.get_value(), debounced_search.get(), version.get()),
        |(system, query, _)| server_fns::get_roms_page(system, 0, PAGE_SIZE, query),
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
        let system = sys.get_value();
        let query = debounced_search.get_untracked();
        let current_offset = offset.get_untracked();
        leptos::task::spawn_local(async move {
            if let Ok(page) = server_fns::get_roms_page(system, current_offset, PAGE_SIZE, query).await {
                set_extra_roms.update(|roms| roms.extend(page.roms));
                set_has_more.set(page.has_more);
                set_offset.update(|o| *o += PAGE_SIZE);
            }
            set_loading_more.set(false);
        });
    };

    // Favorites — re-fetches when version changes (after delete/rename).
    let fav_filenames = Resource::new(
        move || (sys.get_value(), version.get()),
        |(s, _)| server_fns::get_system_favorites(s),
    );
    let (local_favs, set_local_favs) = signal(HashSet::<String>::new());

    // Seed/re-seed local_favs when server data arrives.
    Effect::new(move || {
        if let Some(Ok(server_favs)) = fav_filenames.get() {
            set_local_favs.set(server_favs.into_iter().collect());
        }
    });

    // Action states.
    let (confirm_delete, set_confirm_delete) = signal(Option::<String>::None);
    let (renaming, set_renaming) = signal(Option::<String>::None);
    let (rename_value, set_rename_value) = signal(String::new());

    // Sentinel ref for infinite scroll.
    let sentinel_ref = NodeRef::<leptos::html::Div>::new();

    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::prelude::*;
        use web_sys::js_sys;

        let load_more_for_observer = load_more.clone();
        Effect::new(move || {
            let Some(el) = sentinel_ref.get() else { return };

            let cb = Closure::<dyn Fn(js_sys::Array)>::new(move |entries: js_sys::Array| {
                for entry in entries.iter() {
                    if let Ok(entry) = entry.dyn_into::<web_sys::IntersectionObserverEntry>() {
                        if entry.is_intersecting() {
                            load_more_for_observer();
                        }
                    }
                }
            });

            let opts = web_sys::IntersectionObserverInit::new();
            opts.set_root_margin("200px");

            if let Ok(observer) = web_sys::IntersectionObserver::new_with_options(
                cb.as_ref().unchecked_ref(),
                &opts,
            ) {
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
        <div class="search-bar">
            <input
                type="text"
                placeholder=move || t(i18n.locale.get(), "games.search_placeholder")
                class="search-input"
                on:input=move |ev| set_search_input.set(event_target_value(&ev))
            />
        </div>

        <Suspense fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), "games.loading_roms")}</div> }>
            {move || Suspend::new(async move {
                let locale = i18n.locale.get();
                match first_page.await {
                    Ok(page) => {
                        set_has_more.set(page.has_more);
                        let total = page.total;
                        let first_page_len = page.roms.len();
                        let count_text = move || {
                            let loaded = first_page_len + extra_roms.read().len();
                            if loaded < total {
                                format!("{loaded} / {total} {}", t(locale, "stats.games").to_lowercase())
                            } else {
                                format!("{total} {}", t(locale, "stats.games").to_lowercase())
                            }
                        };

                        view! {
                            <p class="rom-count">{count_text}</p>
                            <div class="rom-list">
                                // First page ROMs (from SSR).
                                {page.roms.into_iter().map(|rom| {
                                    view! {
                                        <RomItem rom local_favs set_local_favs
                                            confirm_delete set_confirm_delete
                                            renaming set_renaming
                                            rename_value set_rename_value
                                            set_version
                                        />
                                    }
                                }).collect::<Vec<_>>()}

                                // Extra ROMs from subsequent pages.
                                {move || {
                                    extra_roms.get().into_iter().map(|rom| {
                                        view! {
                                            <RomItem rom local_favs set_local_favs
                                                confirm_delete set_confirm_delete
                                                renaming set_renaming
                                                rename_value set_rename_value
                                                set_version
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
        </Suspense>
    }
}

/// A single ROM row with favorite toggle, rename, and delete actions.
#[component]
fn RomItem(
    rom: RomEntry,
    local_favs: ReadSignal<HashSet<String>>,
    set_local_favs: WriteSignal<HashSet<String>>,
    confirm_delete: ReadSignal<Option<String>>,
    set_confirm_delete: WriteSignal<Option<String>>,
    renaming: ReadSignal<Option<String>>,
    set_renaming: WriteSignal<Option<String>>,
    rename_value: ReadSignal<String>,
    set_rename_value: WriteSignal<String>,
    set_version: WriteSignal<u32>,
) -> impl IntoView {
    let filename = StoredValue::new(rom.game.rom_filename.clone());
    let display_name = StoredValue::new(rom.game.display_name.clone());
    let relative_path = StoredValue::new(rom.game.rom_path.clone());
    let system = StoredValue::new(rom.game.system.clone());
    let size = format_size(rom.size_bytes);
    let ext = format!(".{}", rom.game.rom_filename.rsplit('.').next().unwrap_or(""));
    let path_display = rom.game.rom_path.clone();

    let game_href = {
        let sys = rom.game.system.clone();
        let fname = rom.game.rom_filename.clone();
        format!("/games/{}/{}", sys, urlencoding::encode(&fname))
    };
    let game_href = StoredValue::new(game_href);

    let shown_name = move || {
        display_name.get_value().unwrap_or_else(|| filename.get_value())
    };

    let is_deleting = move || confirm_delete.get().as_deref() == Some(&*relative_path.get_value());
    let is_renaming = move || renaming.get().as_deref() == Some(&*relative_path.get_value());

    view! {
        <div class="rom-item">
            <FavButton filename system rom_path=relative_path local_favs set_local_favs />

            <div class="rom-info">
                <Show when=is_renaming fallback=move || view! {
                    <A href=game_href.get_value() attr:class="rom-name rom-name-link">{shown_name()}</A>
                    <span class="rom-path">{path_display.clone()}</span>
                }>
                    <RenameInput rename_value set_rename_value set_renaming
                        relative_path set_version
                    />
                </Show>
            </div>

            <div class="rom-meta">
                <span class="rom-size">{size}</span>
                <span class="rom-ext">{ext}</span>
            </div>

            <div class="rom-actions">
                <Show when=is_deleting fallback=move || view! {
                    <button class="rom-action-btn" title="Rename"
                        on:click=move |_| {
                            set_rename_value.set(filename.get_value());
                            set_renaming.set(Some(relative_path.get_value()));
                        }
                    >{"\u{270F}"}</button>
                    <button class="rom-action-btn rom-action-delete" title="Delete"
                        on:click=move |_| set_confirm_delete.set(Some(relative_path.get_value()))
                    >{"\u{2715}"}</button>
                }>
                    <DeleteConfirm relative_path set_confirm_delete set_version />
                </Show>
            </div>
        </div>
    }
}

#[component]
fn FavButton(
    filename: StoredValue<String>,
    system: StoredValue<String>,
    rom_path: StoredValue<String>,
    local_favs: ReadSignal<HashSet<String>>,
    set_local_favs: WriteSignal<HashSet<String>>,
) -> impl IntoView {
    let on_toggle = move |_| {
        let fname = filename.get_value();
        let sys = system.get_value();
        let rp = rom_path.get_value();
        let is_fav = local_favs.get().contains(&fname);

        set_local_favs.update(|set| {
            if is_fav { set.remove(&fname); } else { set.insert(fname.clone()); }
        });

        if is_fav {
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

    let star = move || {
        if local_favs.get().contains(&filename.get_value()) { "\u{2605}" } else { "\u{2606}" }
    };

    view! { <button class="rom-fav-btn" on:click=on_toggle>{star}</button> }
}

#[component]
fn RenameInput(
    rename_value: ReadSignal<String>,
    set_rename_value: WriteSignal<String>,
    set_renaming: WriteSignal<Option<String>>,
    relative_path: StoredValue<String>,
    set_version: WriteSignal<u32>,
) -> impl IntoView {
    let on_keydown = move |ev: leptos::ev::KeyboardEvent| {
        if ev.key() == "Enter" {
            let rp = relative_path.get_value();
            let new_name = rename_value.get_untracked();
            set_renaming.set(None);
            leptos::task::spawn_local(async move {
                if server_fns::rename_rom(rp, new_name).await.is_ok() {
                    set_version.update(|v| *v += 1);
                }
            });
        } else if ev.key() == "Escape" {
            set_renaming.set(None);
        }
    };

    view! {
        <div class="rename-inline">
            <input
                type="text"
                class="rename-input"
                prop:value=move || rename_value.get()
                on:input=move |ev| set_rename_value.set(event_target_value(&ev))
                on:keydown=on_keydown
            />
        </div>
    }
}

#[component]
fn DeleteConfirm(
    relative_path: StoredValue<String>,
    set_confirm_delete: WriteSignal<Option<String>>,
    set_version: WriteSignal<u32>,
) -> impl IntoView {
    let on_confirm = move |_| {
        let rp = relative_path.get_value();
        set_confirm_delete.set(None);
        leptos::task::spawn_local(async move {
            if server_fns::delete_rom(rp).await.is_ok() {
                set_version.update(|v| *v += 1);
            }
        });
    };

    view! {
        <button class="rom-action-btn rom-action-confirm-delete" on:click=on_confirm>
            {"\u{2713} Del"}
        </button>
        <button class="rom-action-btn" on:click=move |_| set_confirm_delete.set(None)>
            {"\u{2715}"}
        </button>
    }
}
