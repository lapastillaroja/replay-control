use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::{use_navigate, use_params_map};
use server_fn::ServerFnError;

use crate::components::boxart_picker::BoxArtPicker;
use crate::components::captures::{CapturesLightbox, INITIAL_CAPTURE_COUNT};
use crate::components::hero_card::GameScrollCard;
use crate::components::video_section::GameVideoSection;
use crate::i18n::{t, use_i18n};
use crate::pages::ErrorDisplay;
use crate::server_fns::{self, RecommendedGame, RomDetail};
use crate::util::format_size_for_system;

#[component]
pub fn GameDetailPage() -> impl IntoView {
    let i18n = use_i18n();
    let params = use_params_map();
    let system = move || params.read().get("system").unwrap_or_default();
    let filename = move || {
        let raw = params.read().get("filename").unwrap_or_default();
        // URL-decode the filename
        urlencoding::decode(&raw)
            .map(|s| s.into_owned())
            .unwrap_or(raw)
    };

    let detail = Resource::new(
        move || (system(), filename()),
        |(sys, fname)| server_fns::get_rom_detail(sys, fname),
    );

    view! {
        <div class="page game-detail">
            <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }>
                <Suspense fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), "common.loading")}</div> }>
                    {move || Suspend::new(async move {
                        let data = detail.await?;
                        Ok::<_, ServerFnError>(view! {
                            <GameDetailContent detail=data system=system() />
                        })
                    })}
                </Suspense>
            </ErrorBoundary>
        </div>
    }
}

#[component]
fn GameDetailContent(detail: RomDetail, system: String) -> impl IntoView {
    let i18n = use_i18n();

    let game = &detail.game;
    let game_name = game.display_name.clone();
    let game_name_sv = StoredValue::new(game_name.clone());
    let filename_sv = StoredValue::new(game.rom_filename.clone());
    let relative_path_sv = StoredValue::new(game.rom_path.clone());
    let system_sv = StoredValue::new(system.clone());
    let system_display = game.system_display.clone();
    let size_display = format_size_for_system(detail.size_bytes, &system);
    let has_arcade = game.rotation.is_some();
    let ext = game
        .rom_filename
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_uppercase();
    // Use browser history for back navigation when available (preserves scroll position
    // and works correctly regardless of where the user came from — home, favorites, etc.)
    #[cfg(feature = "hydrate")]
    let go_back = {
        let back_href = format!("/games/{system}");
        move |ev: leptos::ev::MouseEvent| {
            ev.prevent_default();
            if let Some(window) = web_sys::window() {
                let history = window.history().ok();
                // Only use history.back() if there's actual history to go back to
                if history
                    .as_ref()
                    .is_some_and(|h| h.length().unwrap_or(0) > 1)
                {
                    let _ = history.unwrap().back();
                } else {
                    // Fallback: navigate to the system page
                    let nav = leptos_router::hooks::use_navigate();
                    nav(&back_href, Default::default());
                }
            }
        }
    };
    #[cfg(not(feature = "hydrate"))]
    let go_back = move |_: leptos::ev::MouseEvent| {};

    let is_favorite = RwSignal::new(detail.is_favorite);

    // Metadata fields
    let has_year = !game.year.is_empty();
    let has_developer = !game.developer.is_empty();
    let has_genre = !game.genre.is_empty();
    let has_players = game.players > 0;
    let year = StoredValue::new(game.year.clone());
    let developer = StoredValue::new(game.developer.clone());
    let genre = StoredValue::new(game.genre.clone());
    let players_str = if game.players > 0 {
        game.players.to_string()
    } else {
        String::new()
    };

    // Arcade-specific fields
    let rotation = game.rotation.clone();
    let driver_status = game.driver_status.clone();
    let is_clone = game.is_clone.unwrap_or(false);
    let parent_rom = game.parent_rom.clone();
    let arcade_category = StoredValue::new(game.arcade_category.clone());
    let has_category = game.arcade_category.is_some();

    // Console-specific fields
    let region = game.region.clone();

    // External metadata
    let description = StoredValue::new(game.description.clone());
    let has_description = game.description.is_some();
    let has_rating = game.rating.is_some();
    let rating_display = StoredValue::new(game.rating.map(|r| format!("{:.1} / 5.0", r)));
    let has_publisher = game.publisher.as_ref().is_some_and(|p| !p.is_empty());
    let publisher = StoredValue::new(game.publisher.clone().unwrap_or_default());

    // Images — box_art_url is an RwSignal so the picker can update it reactively.
    let box_art_url = RwSignal::new(game.box_art_url.clone());
    let screenshot_url = StoredValue::new(game.screenshot_url.clone());
    let has_screenshot = game.screenshot_url.is_some();

    // Box art variant picker state.
    // Suppress "Change cover" for hack and special ROMs — they should inherit the base ROM's cover.
    let variant_count = detail.variant_count;
    let has_variants = variant_count > 1 && !detail.is_hack && !detail.is_special;
    let show_picker = RwSignal::new(false);

    // User captures
    let user_screenshots = StoredValue::new(detail.user_screenshots.clone());
    let has_user_screenshots = !detail.user_screenshots.is_empty();
    let captures_show_all = RwSignal::new(false);
    let lightbox_index = RwSignal::new(Option::<usize>::None);

    // Delete confirmation state
    let confirming_delete = RwSignal::new(false);

    // Rename state
    let is_renaming = RwSignal::new(false);
    let rename_value = RwSignal::new(game.rom_filename.clone());

    // Toggle favorite
    let on_toggle_fav = move |_| {
        let fav = is_favorite.get();
        is_favorite.set(!fav);

        let sys = system_sv.get_value();
        let fname = filename_sv.get_value();
        let rp = relative_path_sv.get_value();

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

    let fav_label = move || {
        let locale = i18n.locale.get();
        if is_favorite.get() {
            t(locale, "game_detail.unfavorite")
        } else {
            t(locale, "game_detail.favorite")
        }
    };

    let fav_icon = move || {
        if is_favorite.get() {
            "\u{2605}"
        } else {
            "\u{2606}"
        }
    };

    view! {
        // Header
        <div class="rom-header">
            <button class="back-btn" on:click=go_back>
                {move || t(i18n.locale.get(), "games.back")}
            </button>
            <h2 class="page-title">{game_name.clone()}</h2>
        </div>

        // Hero / Cover Art
        <section class="section">
            <div class="game-cover">
                <Show when=move || box_art_url.read().is_some()
                    fallback=move || view! { <span class="game-cover-text">{game_name_sv.get_value()}</span> }
                >
                    <img
                        class="game-cover-img"
                        class:game-cover-tappable=has_variants
                        src=move || box_art_url.get().unwrap_or_default()
                        alt=game_name_sv.get_value()
                        on:click=move |_| {
                            if has_variants {
                                show_picker.set(true);
                            }
                        }
                    />
                </Show>
            </div>
            <Show when=move || has_variants>
                <div class="change-cover-link" on:click=move |_| show_picker.set(true)>
                    {move || t(i18n.locale.get(), "game_detail.change_cover")}
                    " \u{203A}"
                </div>
            </Show>
            <Show when=move || show_picker.get()>
                <BoxArtPicker
                    system=system_sv
                    rom_filename=filename_sv
                    on_close=Callback::new(move |()| show_picker.set(false))
                    on_change=Callback::new(move |new_url: String| {
                        show_picker.set(false);
                        if new_url.is_empty() {
                            // Reset: reload the page to get the default box art.
                            // For simplicity, just reload the resource.
                            #[cfg(feature = "hydrate")]
                            {
                                if let Some(window) = web_sys::window() {
                                    let _ = window.location().reload();
                                }
                            }
                        } else {
                            box_art_url.set(Some(new_url));
                        }
                    })
                />
            </Show>
        </section>

        // Launch on TV (prominent CTA)
        <section class="game-launch-cta">
            <GameLaunchAction relative_path=relative_path_sv />
        </section>

        // Game Info Card
        <section class="section">
            <h2 class="section-title">{move || t(i18n.locale.get(), "game_detail.info")}</h2>
            <div class="game-meta-grid">
                <div class="game-meta-item">
                    <span class="game-meta-label">{move || t(i18n.locale.get(), "game_detail.system")}</span>
                    <span class="game-meta-value">{system_display.clone()}</span>
                </div>
                <div class="game-meta-item">
                    <span class="game-meta-label">{move || t(i18n.locale.get(), "game_detail.filename")}</span>
                    <span class="game-meta-value">{relative_path_sv.get_value()}</span>
                </div>
                <div class="game-meta-item">
                    <span class="game-meta-label">{move || t(i18n.locale.get(), "game_detail.file_size")}</span>
                    <span class="game-meta-value">{size_display}</span>
                </div>
                <Show when=move || !has_arcade>
                    <div class="game-meta-item">
                        <span class="game-meta-label">{move || t(i18n.locale.get(), "game_detail.format")}</span>
                        <span class="game-meta-value">{ext.clone()}</span>
                    </div>
                </Show>
                <Show when=move || has_year>
                    <div class="game-meta-item">
                        <span class="game-meta-label">{move || t(i18n.locale.get(), "game_detail.year")}</span>
                        <span class="game-meta-value">{year.get_value()}</span>
                    </div>
                </Show>
                <Show when=move || has_developer>
                    <div class="game-meta-item">
                        <span class="game-meta-label">{move || t(i18n.locale.get(), "game_detail.developer")}</span>
                        <span class="game-meta-value">{developer.get_value()}</span>
                    </div>
                </Show>
                <Show when=move || has_publisher>
                    <div class="game-meta-item">
                        <span class="game-meta-label">{move || t(i18n.locale.get(), "game_detail.publisher")}</span>
                        <span class="game-meta-value">{publisher.get_value()}</span>
                    </div>
                </Show>
                <Show when=move || has_genre>
                    <div class="game-meta-item">
                        <span class="game-meta-label">{move || t(i18n.locale.get(), "game_detail.genre")}</span>
                        <span class="game-meta-value">{genre.get_value()}</span>
                    </div>
                </Show>
                <Show when=move || has_players>
                    <div class="game-meta-item">
                        <span class="game-meta-label">{move || t(i18n.locale.get(), "game_detail.players")}</span>
                        <span class="game-meta-value">{players_str.clone()}</span>
                    </div>
                </Show>
                <Show when=move || has_rating>
                    <div class="game-meta-item">
                        <span class="game-meta-label">{move || t(i18n.locale.get(), "game_detail.rating")}</span>
                        <span class="game-meta-value">{rating_display.get_value()}</span>
                    </div>
                </Show>

                // Arcade-specific fields
                {rotation.map(|r| view! {
                    <div class="game-meta-item">
                        <span class="game-meta-label">{move || t(i18n.locale.get(), "game_detail.rotation")}</span>
                        <span class="game-meta-value">{r}</span>
                    </div>
                })}
                {driver_status.map(|s| view! {
                    <div class="game-meta-item">
                        <span class="game-meta-label">{move || t(i18n.locale.get(), "game_detail.status")}</span>
                        <span class="game-meta-value">{s}</span>
                    </div>
                })}
                <Show when=move || has_category>
                    <div class="game-meta-item">
                        <span class="game-meta-label">{move || t(i18n.locale.get(), "game_detail.raw_category")}</span>
                        <span class="game-meta-value">{arcade_category.get_value()}</span>
                    </div>
                </Show>
                <Show when=move || is_clone>
                    <div class="game-meta-item">
                        <span class="game-meta-label">{move || t(i18n.locale.get(), "game_detail.parent_rom")}</span>
                        <span class="game-meta-value">{parent_rom.clone()}</span>
                    </div>
                </Show>

                // Console-specific fields
                {region.map(|r| view! {
                    <div class="game-meta-item">
                        <span class="game-meta-label">{move || t(i18n.locale.get(), "game_detail.region")}</span>
                        <span class="game-meta-value">{r}</span>
                    </div>
                })}
            </div>
        </section>

        // Description (hidden when no description available)
        <Show when=move || has_description>
            <section class="section game-section">
                <h2 class="game-section-title">{move || t(i18n.locale.get(), "game_detail.description")}</h2>
                <p class="game-description">{move || description.get_value()}</p>
            </section>
        </Show>

        // Screenshots Gallery (hidden when no screenshots)
        <Show when=move || has_screenshot>
            <section class="section game-section">
                <h2 class="game-section-title">{move || t(i18n.locale.get(), "game_detail.screenshots")}</h2>
                <div class="game-screenshots">
                    <img class="game-screenshot-img" src=screenshot_url.get_value() alt="Screenshot" />
                </div>
            </section>
        </Show>

        // User Captures (hidden when none, with helpful prompt)
        <section class="section game-section">
            <h2 class="game-section-title">{move || t(i18n.locale.get(), "game_detail.user_captures")}</h2>
            <Show when=move || has_user_screenshots
                fallback=move || view! { <p class="game-section-empty">{move || t(i18n.locale.get(), "game_detail.no_captures")}</p> }
            >
                <div class="user-captures-gallery">
                    {move || {
                        let all = user_screenshots.get_value();
                        let show_all = captures_show_all.get();
                        let visible = if show_all || all.len() <= INITIAL_CAPTURE_COUNT {
                            all.clone()
                        } else {
                            all[..INITIAL_CAPTURE_COUNT].to_vec()
                        };
                        visible.into_iter().enumerate().map(|(i, s)| {
                            let url = s.url.clone();
                            view! {
                                <img
                                    class="user-capture-thumb"
                                    src=url
                                    alt="Capture"
                                    on:click=move |_| lightbox_index.set(Some(i))
                                />
                            }
                        }).collect::<Vec<_>>()
                    }}
                </div>
                <Show when=move || { user_screenshots.get_value().len() > INITIAL_CAPTURE_COUNT && !captures_show_all.get() }>
                    <button
                        class="game-action-btn captures-show-all"
                        on:click=move |_| captures_show_all.set(true)
                    >
                        {move || t(i18n.locale.get(), "game_detail.view_all_captures")}
                        {move || format!(" ({})", user_screenshots.get_value().len())}
                    </button>
                </Show>
                <CapturesLightbox
                    screenshots=user_screenshots.get_value()
                    current_index=lightbox_index
                />
            </Show>
        </section>

        // Videos
        <GameVideoSection
            system=system_sv
            rom_filename=filename_sv
            display_name=game_name_sv
        />

        // Related Games (lazy-loaded)
        <RelatedGamesSection
            system=system_sv
            rom_filename=filename_sv
        />

        // Actions
        <section class="section">
            <h2 class="section-title">{move || t(i18n.locale.get(), "game_detail.actions")}</h2>
            <div class="game-actions">
                <button class="game-action-btn game-action-fav" on:click=on_toggle_fav>
                    <span class="game-action-icon">{fav_icon}</span>
                    {fav_label}
                </button>

                <GameRenameAction
                    is_renaming rename_value
                    filename=filename_sv
                    relative_path=relative_path_sv
                    system=system_sv
                />

                <GameDeleteAction
                    confirming_delete
                    relative_path=relative_path_sv
                    system=system_sv
                />
            </div>
        </section>
    }
}

/// Launch action: "Launch on TV" button with launching/launched/error states.
#[component]
fn GameLaunchAction(relative_path: StoredValue<String>) -> impl IntoView {
    let i18n = use_i18n();
    let launching = RwSignal::new(false);
    let launch_result = RwSignal::new(Option::<Result<String, String>>::None);

    // Schedule a 3-second reset timer. Only runs client-side (WASM).
    let schedule_reset = move || {
        #[cfg(target_arch = "wasm32")]
        {
            gloo_timers::callback::Timeout::new(3_000, move || {
                launch_result.set(None);
            })
            .forget();
        }
    };

    let on_launch = move |_| {
        launching.set(true);
        launch_result.set(None);

        let rp = relative_path.get_value();
        leptos::task::spawn_local(async move {
            let result = server_fns::launch_game(rp).await;
            launching.set(false);
            match result {
                Ok(msg) => {
                    launch_result.set(Some(Ok(msg)));
                    schedule_reset();
                }
                Err(e) => {
                    launch_result.set(Some(Err(e.to_string())));
                    schedule_reset();
                }
            }
        });
    };

    let is_launched =
        move || matches!(launch_result.get(), Some(Ok(ref m)) if !m.contains("simulated"));
    let is_simulated =
        move || matches!(launch_result.get(), Some(Ok(ref m)) if m.contains("simulated"));
    let is_error = move || matches!(launch_result.get(), Some(Err(_)));
    let is_disabled = move || launching.get() || is_launched();

    let label = move || {
        let locale = i18n.locale.get();
        if launching.get() {
            t(locale, "game_detail.launching")
        } else if is_launched() {
            t(locale, "game_detail.launched")
        } else if is_simulated() {
            t(locale, "game_detail.launch_not_replayos")
        } else if is_error() {
            t(locale, "game_detail.launch_error")
        } else {
            t(locale, "game_detail.launch")
        }
    };

    view! {
        <button
            class="game-action-launch"
            class:game-action-launch-success=is_launched
            class:game-action-launch-simulated=is_simulated
            prop:disabled=is_disabled
            on:click=on_launch
        >
            <span class="game-action-icon">{"\u{25B6}"}</span>
            {label}
        </button>
    }
}

/// Rename action: shows a button that toggles to an inline rename form.
#[component]
fn GameRenameAction(
    is_renaming: RwSignal<bool>,
    rename_value: RwSignal<String>,
    filename: StoredValue<String>,
    relative_path: StoredValue<String>,
    system: StoredValue<String>,
) -> impl IntoView {
    let i18n = use_i18n();
    let navigate = use_navigate();

    let do_rename = StoredValue::new(move || {
        let rp = relative_path.get_value();
        let new_name = rename_value.get();
        let sys = system.get_value();
        is_renaming.set(false);
        let nav = navigate.clone();
        leptos::task::spawn_local(async move {
            if server_fns::rename_rom(rp, new_name.clone()).await.is_ok() {
                let encoded = urlencoding::encode(&new_name);
                let href = format!("/games/{sys}/{encoded}");
                nav(&href, Default::default());
            }
        });
    });

    let on_keydown = move |ev: leptos::ev::KeyboardEvent| {
        if ev.key() == "Enter" {
            (do_rename.get_value())();
        } else if ev.key() == "Escape" {
            is_renaming.set(false);
        }
    };

    let on_click = move |_: leptos::ev::MouseEvent| {
        (do_rename.get_value())();
    };

    view! {
        <Show when=move || is_renaming.get() fallback=move || view! {
            <button class="game-action-btn" on:click=move |_| {
                rename_value.set(filename.get_value());
                is_renaming.set(true);
            }>
                <span class="game-action-icon">{"\u{270F}"}</span>
                {move || t(i18n.locale.get(), "game_detail.rename")}
            </button>
        }>
            <div class="game-rename-inline">
                <input
                    type="text"
                    class="rename-input"
                    prop:value=move || rename_value.get()
                    on:input=move |ev| rename_value.set(event_target_value(&ev))
                    on:keydown=on_keydown
                />
                <div class="game-rename-btns">
                    <button class="rom-action-btn" on:click=on_click>
                        {"\u{2713}"}
                    </button>
                    <button class="rom-action-btn" on:click=move |_| is_renaming.set(false)>
                        {"\u{2715}"}
                    </button>
                </div>
            </div>
        </Show>
    }
}

/// Delete action: shows a button that toggles to a confirm/cancel pair.
#[component]
fn GameDeleteAction(
    confirming_delete: RwSignal<bool>,
    relative_path: StoredValue<String>,
    system: StoredValue<String>,
) -> impl IntoView {
    let i18n = use_i18n();
    let navigate = use_navigate();

    let nav_sv = StoredValue::new(navigate);
    let on_delete = move |_| {
        let rp = relative_path.get_value();
        let sys = system.get_value();
        let nav = nav_sv.get_value();
        leptos::task::spawn_local(async move {
            if server_fns::delete_rom(rp).await.is_ok() {
                let href = format!("/games/{sys}");
                nav(&href, Default::default());
            }
        });
    };

    view! {
        <Show when=move || confirming_delete.get() fallback=move || view! {
            <button class="game-action-btn game-action-delete" on:click=move |_| confirming_delete.set(true)>
                <span class="game-action-icon">{"\u{2715}"}</span>
                {move || t(i18n.locale.get(), "game_detail.delete")}
            </button>
        }>
            <div class="game-delete-confirm">
                <button class="game-action-btn game-action-delete-confirm" on:click=on_delete>
                    {move || t(i18n.locale.get(), "game_detail.confirm_delete")}
                </button>
                <button class="game-action-btn" on:click=move |_| confirming_delete.set(false)>
                    {move || t(i18n.locale.get(), "games.cancel")}
                </button>
            </div>
        </Show>
    }
}

/// Related games section: regional variants and "More Like This" (genre-based).
/// Loads lazily via its own Resource so it never blocks the main page render.
#[component]
fn RelatedGamesSection(
    system: StoredValue<String>,
    rom_filename: StoredValue<String>,
) -> impl IntoView {
    let related = Resource::new(
        move || (system.get_value(), rom_filename.get_value()),
        |(sys, fname)| server_fns::get_related_games(sys, fname),
    );

    view! {
        <Transition fallback=|| ()>
            {move || Suspend::new(async move {
                let data = related.await;
                Ok::<_, ServerFnError>(match data {
                    Ok(data) => {
                        let has_variants = data.regional_variants.len() > 1;
                        let has_translations = !data.translations.is_empty();
                        let has_hacks = !data.hacks.is_empty();
                        let has_specials = !data.specials.is_empty();
                        let has_arcade_versions = !data.arcade_versions.is_empty();
                        let has_aliases = !data.alias_variants.is_empty();
                        let has_series = !data.series_siblings.is_empty();
                        let has_similar = !data.similar_games.is_empty();
                        let has_sequel_nav = data.sequel_prev.is_some() || data.sequel_next.is_some();
                        if !has_variants && !has_translations && !has_hacks && !has_specials && !has_arcade_versions && !has_aliases && !has_series && !has_similar && !has_sequel_nav {
                            view! { <div /> }.into_any()
                        } else {
                            let variant_chips: Vec<ChipItem> = data.regional_variants.iter().map(|v| {
                                ChipItem { label: v.region.clone(), href: v.href.clone(), is_current: v.is_current }
                            }).collect();
                            let translation_chips: Vec<ChipItem> = data.translations.iter().map(|v| {
                                ChipItem { label: v.label.clone(), href: v.href.clone(), is_current: v.is_current }
                            }).collect();
                            let hack_chips: Vec<ChipItem> = data.hacks.iter().map(|v| {
                                ChipItem { label: v.label.clone(), href: v.href.clone(), is_current: v.is_current }
                            }).collect();
                            let special_chips: Vec<ChipItem> = data.specials.iter().map(|v| {
                                ChipItem { label: v.label.clone(), href: v.href.clone(), is_current: v.is_current }
                            }).collect();
                            let arcade_version_chips: Vec<ChipItem> = data.arcade_versions.iter().map(|v| {
                                ChipItem { label: v.label.clone(), href: v.href.clone(), is_current: v.is_current }
                            }).collect();
                            view! {
                                <Show when=move || has_variants>
                                    <GameChipRow
                                        title_key="game_detail.regional_variants"
                                        chips=variant_chips.clone()
                                    />
                                </Show>
                                <Show when=move || has_translations>
                                    <GameChipRow
                                        title_key="game_detail.translations"
                                        chips=translation_chips.clone()
                                    />
                                </Show>
                                <Show when=move || has_hacks>
                                    <GameChipRow
                                        title_key="game_detail.hacks"
                                        chips=hack_chips.clone()
                                    />
                                </Show>
                                <Show when=move || has_specials>
                                    <GameChipRow
                                        title_key="game_detail.special_versions"
                                        chips=special_chips.clone()
                                    />
                                </Show>
                                <Show when=move || has_arcade_versions>
                                    <GameChipRow
                                        title_key="game_detail.arcade_versions"
                                        chips=arcade_version_chips.clone()
                                    />
                                </Show>
                                <Show when=move || has_aliases>
                                    <SimilarGamesRow
                                        games=data.alias_variants.clone()
                                        title_key="game_detail.other_versions"
                                    />
                                </Show>
                                <Show when=move || has_sequel_nav>
                                    <PlayOrderNav
                                        prev=data.sequel_prev.clone()
                                        next=data.sequel_next.clone()
                                        position=data.series_position
                                    />
                                </Show>
                                <Show when=move || has_series>
                                    <SimilarGamesRow
                                        games=data.series_siblings.clone()
                                        title_key="game_detail.more_in_series"
                                        custom_title=data.series_name.clone()
                                    />
                                </Show>
                                <Show when=move || has_similar>
                                    <SimilarGamesRow
                                        games=data.similar_games.clone()
                                        title_key="game_detail.more_like_this"
                                    />
                                </Show>
                            }.into_any()
                        }
                    }
                    Err(_) => view! { <div /> }.into_any(),
                })
            })}
        </Transition>
    }
}

/// A single chip item for the generic chip row.
#[derive(Debug, Clone)]
struct ChipItem {
    label: String,
    href: String,
    is_current: bool,
}

/// Generic horizontal chip row showing clickable links with a section title.
/// Reuses `.regional-variants` and `.region-chip` CSS classes.
#[component]
fn GameChipRow(title_key: &'static str, chips: Vec<ChipItem>) -> impl IntoView {
    let i18n = use_i18n();

    view! {
        <section class="section game-section">
            <h2 class="game-section-title">{move || t(i18n.locale.get(), title_key)}</h2>
            <div class="regional-variants">
                {chips.into_iter().map(|chip| {
                    let class = if chip.is_current { "region-chip active" } else { "region-chip" };
                    view! {
                        <A href=chip.href attr:class=class>{chip.label}</A>
                    }
                }).collect::<Vec<_>>()}
            </div>
        </section>
    }
}

/// Horizontal scrollable row of similar games, reusing GameScrollCard.
#[component]
fn SimilarGamesRow(
    games: Vec<RecommendedGame>,
    #[prop(default = "game_detail.more_like_this")] title_key: &'static str,
    /// Optional custom title (e.g., series name from Wikidata). When non-empty, displayed
    /// instead of the i18n title_key.
    #[prop(default = String::new())]
    custom_title: String,
) -> impl IntoView {
    let i18n = use_i18n();
    let has_custom = !custom_title.is_empty();

    view! {
        <section class="section game-section">
            <h2 class="game-section-title">
                {move || {
                    if has_custom {
                        custom_title.clone()
                    } else {
                        t(i18n.locale.get(), title_key).to_string()
                    }
                }}
            </h2>
            <div class="recent-scroll">
                {games.into_iter().map(|game| {
                    let name = game.label.unwrap_or(game.display_name);
                    view! {
                        <GameScrollCard
                            href=game.href
                            name=name
                            system=game.system_display
                            box_art_url=game.box_art_url
                        />
                    }
                }).collect::<Vec<_>>()}
            </div>
        </section>
    }
}

/// Breadcrumb-style play order navigation showing prev/next sequel links.
///
/// Layout: `[< Prev Game] [N of M] [Next Game >]`
/// Games not in library shown dimmed with "not in library" subtitle.
#[component]
fn PlayOrderNav(
    prev: Option<server_fns::SequelLink>,
    next: Option<server_fns::SequelLink>,
    position: Option<(i32, i32)>,
) -> impl IntoView {
    let i18n = use_i18n();

    view! {
        <section class="section game-section">
            <h2 class="game-section-title">{move || t(i18n.locale.get(), "game_detail.play_order")}</h2>
            <div class="play-order-nav">
                // Previous game (left side)
                {match &prev {
                    Some(link) if link.in_library => {
                        let href = link.href.clone().unwrap_or_default();
                        let title = link.title.clone();
                        view! {
                            <A href=href attr:class="play-order-link prev">
                                {"\u{2190} "}{title}
                            </A>
                        }.into_any()
                    }
                    Some(link) => {
                        let title = link.title.clone();
                        view! {
                            <div class="play-order-dimmed prev">
                                <span>{"\u{2190} "}{title}</span>
                                <span class="play-order-subtitle">{move || t(i18n.locale.get(), "game_detail.not_in_library")}</span>
                            </div>
                        }.into_any()
                    }
                    None => view! { <div class="play-order-spacer" /> }.into_any(),
                }}
                // Position indicator (center)
                {position.map(|(n, m)| view! {
                    <span class="play-order-position">{format!("{n} / {m}")}</span>
                })}
                // Next game (right side)
                {match &next {
                    Some(link) if link.in_library => {
                        let href = link.href.clone().unwrap_or_default();
                        let title = link.title.clone();
                        view! {
                            <A href=href attr:class="play-order-link next">
                                {title}{" \u{2192}"}
                            </A>
                        }.into_any()
                    }
                    Some(link) => {
                        let title = link.title.clone();
                        view! {
                            <div class="play-order-dimmed next">
                                <span>{title}{" \u{2192}"}</span>
                                <span class="play-order-subtitle">{move || t(i18n.locale.get(), "game_detail.not_in_library")}</span>
                            </div>
                        }.into_any()
                    }
                    None => view! { <div class="play-order-spacer" /> }.into_any(),
                }}
            </div>
        </section>
    }
}
