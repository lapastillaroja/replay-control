use leptos::prelude::*;
use leptos_router::hooks::{use_navigate, use_params_map};
use server_fn::ServerFnError;

use crate::i18n::{t, use_i18n};
use crate::pages::ErrorDisplay;
use crate::server_fns::{self, RomDetail, ScreenshotUrl, VideoEntry, VideoRecommendation};
use crate::util::format_size;

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
                <Transition fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), "common.loading")}</div> }>
                    {move || Suspend::new(async move {
                        let data = detail.await?;
                        Ok::<_, ServerFnError>(view! {
                            <GameDetailContent detail=data system=system() />
                        })
                    })}
                </Transition>
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
    let size_display = format_size(detail.size_bytes);
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

    // Images
    let box_art_url = StoredValue::new(game.box_art_url.clone());
    let has_box_art = game.box_art_url.is_some();
    let screenshot_url = StoredValue::new(game.screenshot_url.clone());
    let has_screenshot = game.screenshot_url.is_some();

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
                <Show when=move || has_box_art
                    fallback=move || view! { <span class="game-cover-text">{game_name_sv.get_value()}</span> }
                >
                    <img class="game-cover-img" src=box_art_url.get_value() alt=game_name_sv.get_value() />
                </Show>
            </div>
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

        // Description
        <section class="section game-section">
            <h2 class="game-section-title">{move || t(i18n.locale.get(), "game_detail.description")}</h2>
            <Show when=move || has_description
                fallback=move || view! { <p class="game-section-empty">{move || t(i18n.locale.get(), "game_detail.no_description")}</p> }
            >
                <p class="game-description">{move || description.get_value()}</p>
            </Show>
        </section>

        // Screenshots Gallery
        <section class="section game-section">
            <h2 class="game-section-title">{move || t(i18n.locale.get(), "game_detail.screenshots")}</h2>
            <Show when=move || has_screenshot
                fallback=move || view! { <p class="game-section-empty">{move || t(i18n.locale.get(), "game_detail.no_screenshots")}</p> }
            >
                <div class="game-screenshots">
                    <img class="game-screenshot-img" src=screenshot_url.get_value() alt="Screenshot" />
                </div>
            </Show>
        </section>

        // User Captures
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

        // Manual
        <section class="section game-section">
            <h2 class="game-section-title">{move || t(i18n.locale.get(), "game_detail.manual")}</h2>
            <p class="game-section-empty">{move || t(i18n.locale.get(), "game_detail.no_manual")}</p>
        </section>

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

    let is_launched = move || {
        matches!(launch_result.get(), Some(Ok(ref m)) if !m.contains("simulated"))
    };
    let is_simulated = move || {
        matches!(launch_result.get(), Some(Ok(ref m)) if m.contains("simulated"))
    };
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

// ── User Captures Section ───────────────────────────────────────

/// Maximum number of capture thumbnails shown before "View all".
const INITIAL_CAPTURE_COUNT: usize = 12;

/// Fullscreen lightbox for browsing user captures.
#[component]
fn CapturesLightbox(
    screenshots: Vec<ScreenshotUrl>,
    current_index: RwSignal<Option<usize>>,
) -> impl IntoView {
    let count = screenshots.len();
    let screenshots_sv = StoredValue::new(screenshots);

    let on_prev = move |ev: leptos::ev::MouseEvent| {
        ev.stop_propagation();
        current_index.update(|idx| {
            if let Some(i) = idx {
                *i = if *i == 0 { count - 1 } else { *i - 1 };
            }
        });
    };

    let on_next = move |ev: leptos::ev::MouseEvent| {
        ev.stop_propagation();
        current_index.update(|idx| {
            if let Some(i) = idx {
                *i = if *i + 1 >= count { 0 } else { *i + 1 };
            }
        });
    };

    let on_close = move |_: leptos::ev::MouseEvent| {
        current_index.set(None);
    };

    let on_close_btn = move |ev: leptos::ev::MouseEvent| {
        ev.stop_propagation();
        current_index.set(None);
    };

    // Keyboard navigation (hydrate-only)
    #[cfg(feature = "hydrate")]
    {
        use leptos::ev;
        let handle = leptos::prelude::window_event_listener(ev::keydown, move |ev: ev::KeyboardEvent| {
            if current_index.get().is_none() {
                return;
            }
            match ev.key().as_str() {
                "Escape" => current_index.set(None),
                "ArrowLeft" => current_index.update(|idx| {
                    if let Some(i) = idx {
                        *i = if *i == 0 { count - 1 } else { *i - 1 };
                    }
                }),
                "ArrowRight" => current_index.update(|idx| {
                    if let Some(i) = idx {
                        *i = if *i + 1 >= count { 0 } else { *i + 1 };
                    }
                }),
                _ => {}
            }
        });
        on_cleanup(move || drop(handle));
    }

    let current_url = move || {
        current_index.get().and_then(|i| {
            screenshots_sv.get_value().get(i).map(|s| s.url.clone())
        })
    };

    view! {
        <Show when=move || current_index.get().is_some()>
            <div class="lightbox-overlay" on:click=on_close>
                <button class="lightbox-close" on:click=on_close_btn>
                    {"\u{2715}"}
                </button>
                <Show when=move || { count > 1 }>
                    <button class="lightbox-nav lightbox-prev" on:click=on_prev>
                        {"\u{2039}"}
                    </button>
                </Show>
                <img class="lightbox-img" src=current_url alt="Capture" />
                <Show when=move || { count > 1 }>
                    <button class="lightbox-nav lightbox-next" on:click=on_next>
                        {"\u{203A}"}
                    </button>
                </Show>
            </div>
        </Show>
    }
}

// ── Game Videos Section ─────────────────────────────────────────

/// Maximum number of embedded videos shown before "Show all".
const INITIAL_VIDEO_COUNT: usize = 3;

/// Full video section: saved videos, add input, search buttons, and results.
#[component]
fn GameVideoSection(
    system: StoredValue<String>,
    rom_filename: StoredValue<String>,
    display_name: StoredValue<String>,
) -> impl IntoView {
    let i18n = use_i18n();

    // Saved videos signal — starts from SSR resource, then updated locally.
    let saved_videos = RwSignal::new(Vec::<VideoEntry>::new());
    let show_all = RwSignal::new(false);

    // Load saved videos on mount.
    let videos_resource = Resource::new(
        move || (system.get_value(), rom_filename.get_value()),
        |(sys, fname)| server_fns::get_game_videos(sys, fname),
    );

    // Sync resource into signal when it resolves.
    let _sync = Effect::new(move || {
        if let Some(Ok(vids)) = videos_resource.get() {
            saved_videos.set(vids);
        }
    });

    // Add video state
    let add_url = RwSignal::new(String::new());
    let add_error = RwSignal::new(Option::<String>::None);
    let add_success = RwSignal::new(false);
    let adding = RwSignal::new(false);

    let do_add_video = move || {
        let url = add_url.get();
        if url.trim().is_empty() {
            return;
        }
        adding.set(true);
        add_error.set(None);
        add_success.set(false);

        let sys = system.get_value();
        let fname = rom_filename.get_value();
        leptos::task::spawn_local(async move {
            match server_fns::add_game_video(sys, fname, url, None, false, None).await {
                Ok(entry) => {
                    saved_videos.update(|vids| vids.insert(0, entry));
                    add_url.set(String::new());
                    add_success.set(true);
                    add_error.set(None);
                }
                Err(e) => {
                    let msg = e.to_string();
                    // Detect duplicate error
                    if msg.contains("already saved") {
                        add_error.set(Some("game_detail.add_video_duplicate".to_string()));
                    } else {
                        add_error.set(Some("game_detail.add_video_error".to_string()));
                    }
                    add_success.set(false);
                }
            }
            adding.set(false);
        });
    };

    // Remove video handler
    let on_remove = move |video_id: String| {
        let sys = system.get_value();
        let fname = rom_filename.get_value();
        let vid = video_id.clone();
        saved_videos.update(|vids| vids.retain(|v| v.id != vid));
        leptos::task::spawn_local(async move {
            let _ = server_fns::remove_game_video(sys, fname, video_id).await;
        });
    };

    // Search state
    let trailer_results = RwSignal::new(Vec::<VideoRecommendation>::new());
    let gameplay_results = RwSignal::new(Vec::<VideoRecommendation>::new());
    let onecc_results = RwSignal::new(Vec::<VideoRecommendation>::new());
    let trailer_searching = RwSignal::new(false);
    let gameplay_searching = RwSignal::new(false);
    let onecc_searching = RwSignal::new(false);
    let trailer_error = RwSignal::new(false);
    let gameplay_error = RwSignal::new(false);
    let onecc_error = RwSignal::new(false);
    let trailer_searched = RwSignal::new(false);
    let gameplay_searched = RwSignal::new(false);
    let onecc_searched = RwSignal::new(false);

    let on_search_trailers = move |_| {
        trailer_searching.set(true);
        trailer_error.set(false);
        trailer_searched.set(true);
        trailer_results.set(vec![]);
        let sys = system.get_value();
        let dn = display_name.get_value();
        leptos::task::spawn_local(async move {
            match server_fns::search_game_videos(sys, dn, "trailer".to_string()).await {
                Ok(results) => trailer_results.set(results),
                Err(_) => trailer_error.set(true),
            }
            trailer_searching.set(false);
        });
    };

    let on_search_gameplay = move |_| {
        gameplay_searching.set(true);
        gameplay_error.set(false);
        gameplay_searched.set(true);
        gameplay_results.set(vec![]);
        let sys = system.get_value();
        let dn = display_name.get_value();
        leptos::task::spawn_local(async move {
            match server_fns::search_game_videos(sys, dn, "gameplay".to_string()).await {
                Ok(results) => gameplay_results.set(results),
                Err(_) => gameplay_error.set(true),
            }
            gameplay_searching.set(false);
        });
    };

    let on_search_onecc = move |_| {
        onecc_searching.set(true);
        onecc_error.set(false);
        onecc_searched.set(true);
        onecc_results.set(vec![]);
        let sys = system.get_value();
        let dn = display_name.get_value();
        leptos::task::spawn_local(async move {
            match server_fns::search_game_videos(sys, dn, "1cc".to_string()).await {
                Ok(results) => onecc_results.set(results),
                Err(_) => onecc_error.set(true),
            }
            onecc_searching.set(false);
        });
    };

    // Pin handler — adds a recommendation to saved videos
    let pin_video = move |rec: VideoRecommendation, tag: String| {
        let sys = system.get_value();
        let fname = rom_filename.get_value();
        let url = rec.url.clone();
        let title = Some(rec.title.clone());
        leptos::task::spawn_local(async move {
            if let Ok(entry) =
                server_fns::add_game_video(sys, fname, url, title, true, Some(tag)).await
            {
                saved_videos.update(|vids| vids.insert(0, entry));
            }
        });
    };

    let has_videos = move || !saved_videos.read().is_empty();
    let visible_videos = move || {
        let vids = saved_videos.get();
        if show_all.get() || vids.len() <= INITIAL_VIDEO_COUNT {
            vids
        } else {
            vids[..INITIAL_VIDEO_COUNT].to_vec()
        }
    };
    let has_more = move || saved_videos.read().len() > INITIAL_VIDEO_COUNT && !show_all.get();

    view! {
        <section class="section game-section">
            <h2 class="game-section-title">{move || t(i18n.locale.get(), "game_detail.videos")}</h2>

            // Saved videos list
            <Show when=has_videos fallback=move || view! {
                <p class="game-section-empty">{move || t(i18n.locale.get(), "game_detail.no_videos")}</p>
            }>
                <div class="video-list">
                    <For
                        each=visible_videos
                        key=|v| v.id.clone()
                        let:video
                    >
                        <VideoEmbed video=video.clone() on_remove=on_remove />
                    </For>
                    <Show when=has_more>
                        <button
                            class="game-action-btn"
                            style="margin-top: 4px"
                            on:click=move |_| show_all.set(true)
                        >
                            {move || t(i18n.locale.get(), "game_detail.show_all_videos")}
                            {move || format!(" ({})", saved_videos.read().len())}
                        </button>
                    </Show>
                </div>
            </Show>

            // Add video input
            <div class="video-add-form">
                <input
                    type="text"
                    class="form-input"
                    placeholder=move || t(i18n.locale.get(), "game_detail.add_video_placeholder")
                    prop:value=move || add_url.get()
                    on:input=move |ev| {
                        add_url.set(event_target_value(&ev));
                        add_error.set(None);
                        add_success.set(false);
                    }
                    on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                        if ev.key() == "Enter" {
                            do_add_video();
                        }
                    }
                />
                <button
                    class="game-action-btn"
                    prop:disabled=move || adding.get() || add_url.read().trim().is_empty()
                    on:click=move |_| do_add_video()
                >
                    {move || t(i18n.locale.get(), "game_detail.add_video")}
                </button>
            </div>
            <Show when=move || add_error.get().is_some()>
                <p class="video-add-error">{move || add_error.get().map(|k| t(i18n.locale.get(), &k)).unwrap_or("")}</p>
            </Show>
            <Show when=move || add_success.get()>
                <p class="video-add-success">{move || t(i18n.locale.get(), "game_detail.video_added")}</p>
            </Show>

            // Search buttons
            <div class="video-search-buttons">
                <button
                    class="game-action-btn"
                    prop:disabled=move || trailer_searching.get()
                    on:click=on_search_trailers
                >
                    {move || {
                        if trailer_searching.get() {
                            t(i18n.locale.get(), "game_detail.searching")
                        } else {
                            t(i18n.locale.get(), "game_detail.find_trailers")
                        }
                    }}
                </button>
                <button
                    class="game-action-btn"
                    prop:disabled=move || gameplay_searching.get()
                    on:click=on_search_gameplay
                >
                    {move || {
                        if gameplay_searching.get() {
                            t(i18n.locale.get(), "game_detail.searching")
                        } else {
                            t(i18n.locale.get(), "game_detail.find_gameplay")
                        }
                    }}
                </button>
                <button
                    class="game-action-btn"
                    prop:disabled=move || onecc_searching.get()
                    on:click=on_search_onecc
                >
                    {move || {
                        if onecc_searching.get() {
                            t(i18n.locale.get(), "game_detail.searching")
                        } else {
                            t(i18n.locale.get(), "game_detail.find_1cc")
                        }
                    }}
                </button>
            </div>

            // Trailer results
            <Show when=move || trailer_searched.get()>
                <VideoRecommendations
                    results=trailer_results
                    is_searching=trailer_searching
                    has_error=trailer_error
                    tag="trailer".to_string()
                    saved_videos=saved_videos
                    on_pin=pin_video
                />
            </Show>

            // Gameplay results
            <Show when=move || gameplay_searched.get()>
                <VideoRecommendations
                    results=gameplay_results
                    is_searching=gameplay_searching
                    has_error=gameplay_error
                    tag="gameplay".to_string()
                    saved_videos=saved_videos
                    on_pin=pin_video
                />
            </Show>

            // 1CC results
            <Show when=move || onecc_searched.get()>
                <VideoRecommendations
                    results=onecc_results
                    is_searching=onecc_searching
                    has_error=onecc_error
                    tag="1cc".to_string()
                    saved_videos=saved_videos
                    on_pin=pin_video
                />
            </Show>
        </section>
    }
}

/// A single embedded video with remove button.
#[component]
fn VideoEmbed<F>(video: VideoEntry, on_remove: F) -> impl IntoView
where
    F: Fn(String) + Clone + Send + 'static,
{
    let i18n = use_i18n();
    let video_id = video.id.clone();
    let on_remove = on_remove.clone();

    // Compute embed URL from platform and video_id
    let embed_url = match video.platform.as_str() {
        "youtube" => format!("https://www.youtube-nocookie.com/embed/{}", video.video_id),
        "twitch" => {
            // Twitch needs a parent param; use a placeholder that works
            format!(
                "https://player.twitch.tv/?video={}&parent=localhost",
                video.video_id
            )
        }
        "vimeo" => format!("https://player.vimeo.com/video/{}", video.video_id),
        "dailymotion" => format!("https://www.dailymotion.com/embed/video/{}", video.video_id),
        _ => video.url.clone(),
    };

    let title_display = video.title.clone().unwrap_or_default();

    view! {
        <div class="video-item">
            <div class="video-item-header">
                <span class="video-item-title">{title_display}</span>
                <button
                    class="video-remove-btn"
                    on:click=move |_| on_remove(video_id.clone())
                >
                    {move || t(i18n.locale.get(), "game_detail.remove_video")}
                </button>
            </div>
            <div class="video-embed">
                <iframe
                    src=embed_url
                    sandbox="allow-scripts allow-same-origin allow-popups"
                    allowfullscreen=true
                ></iframe>
            </div>
        </div>
    }
}

/// Panel showing video search results with pin buttons.
#[component]
fn VideoRecommendations<F>(
    results: RwSignal<Vec<VideoRecommendation>>,
    is_searching: RwSignal<bool>,
    has_error: RwSignal<bool>,
    tag: String,
    saved_videos: RwSignal<Vec<VideoEntry>>,
    on_pin: F,
) -> impl IntoView
where
    F: Fn(VideoRecommendation, String) + Clone + Send + 'static,
{
    let i18n = use_i18n();
    let tag_sv = StoredValue::new(tag);

    view! {
        <div class="video-recommendations">
            <Show when=move || has_error.get()>
                <p class="video-add-error">{move || t(i18n.locale.get(), "game_detail.search_error")}</p>
            </Show>
            <Show when=move || !is_searching.get() && results.read().is_empty() && !has_error.get()>
                <p class="game-section-empty">{move || t(i18n.locale.get(), "game_detail.no_results")}</p>
            </Show>
            <For
                each=move || results.get()
                key=|rec| rec.url.clone()
                let:rec
            >
                <RecommendationItem
                    rec=rec.clone()
                    tag=tag_sv
                    saved_videos=saved_videos
                    on_pin=on_pin.clone()
                />
            </For>
        </div>
    }
}

/// A single recommendation result with thumbnail, inline player, and pin button.
#[component]
fn RecommendationItem<F>(
    rec: VideoRecommendation,
    tag: StoredValue<String>,
    saved_videos: RwSignal<Vec<VideoEntry>>,
    on_pin: F,
) -> impl IntoView
where
    F: Fn(VideoRecommendation, String) + Clone + Send + 'static,
{
    let i18n = use_i18n();
    let rec_sv = StoredValue::new(rec.clone());
    let playing = RwSignal::new(false);

    // Build embed URL from the YouTube watch URL
    let embed_url = StoredValue::new({
        rec.url
            .split("v=")
            .nth(1)
            .map(|id| {
                let id = id.split('&').next().unwrap_or(id);
                format!("https://www.youtube-nocookie.com/embed/{id}?autoplay=1")
            })
            .unwrap_or_default()
    });

    // Check if this video is already saved
    let is_pinned = move || {
        let url = &rec_sv.get_value().url;
        saved_videos
            .read()
            .iter()
            .any(|v| url.contains(&v.video_id))
    };

    let on_pin = on_pin.clone();
    let on_click_pin = move |_| {
        let r = rec_sv.get_value();
        let t = tag.get_value();
        on_pin(r, t);
    };

    let on_click_play = move |_| {
        playing.update(|p| *p = !*p);
    };

    let meta_text = StoredValue::new({
        let mut parts = Vec::new();
        if let Some(ref ch) = rec.channel {
            parts.push(ch.clone());
        }
        if let Some(ref dur) = rec.duration_text {
            parts.push(dur.clone());
        }
        parts.join(" \u{00B7} ")
    });

    view! {
        <div class="recommendation-item-wrapper">
            <div class="recommendation-item">
                <div class="recommendation-thumb-wrapper" on:click=on_click_play>
                    {rec.thumbnail_url.map(|url| view! {
                        <img class="recommendation-thumb" src=url alt="" />
                    })}
                    <div class="recommendation-play-icon">"\u{25B6}"</div>
                </div>
                <div class="recommendation-info" on:click=on_click_play>
                    <div class="recommendation-title">{rec.title.clone()}</div>
                    <Show when=move || !meta_text.get_value().is_empty()>
                        <div class="recommendation-meta">{meta_text.get_value()}</div>
                    </Show>
                </div>
                <button
                    class="recommendation-pin-btn"
                    class:pinned=is_pinned
                    prop:disabled=is_pinned
                    on:click=on_click_pin
                >
                    {move || {
                        if is_pinned() {
                            t(i18n.locale.get(), "game_detail.pinned")
                        } else {
                            t(i18n.locale.get(), "game_detail.pin_video")
                        }
                    }}
                </button>
            </div>
            <Show when=move || playing.get()>
                <div class="recommendation-player">
                    <div class="video-embed">
                        <iframe
                            src=embed_url.get_value()
                            allowfullscreen=true
                            allow="autoplay; encrypted-media"
                            sandbox="allow-scripts allow-same-origin allow-popups"
                        ></iframe>
                    </div>
                </div>
            </Show>
        </div>
    }
}
