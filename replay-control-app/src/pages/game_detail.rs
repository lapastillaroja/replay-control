use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::{use_navigate, use_params_map};
use server_fn::ServerFnError;

use crate::components::boxart_picker::BoxArtPicker;
use crate::components::boxart_placeholder::BoxArtPlaceholder;
use crate::components::captures::{ImageLightbox, LightboxImage};
use crate::components::game_status_section::GameStatusSection;
use crate::components::hero_card::GameScrollCard;
use crate::components::manual_section::ManualSection;
use crate::components::video_section::GameVideoSection;
use crate::i18n::{Key, t, tf, use_i18n};
use crate::server_fns::{self, RecommendedGame, RomDetail};
use crate::util::format_size_for_system;

/// Maximum number of capture thumbnails shown before "View all".
const INITIAL_CAPTURE_COUNT: usize = 12;

/// Split a filename into `(stem, extension)` using `std::path::Path`.
///
/// Returns `("file", "zip")` for `"file.zip"`, or `("file", "")` if no extension.
fn split_filename(filename: &str) -> (String, String) {
    let path = std::path::Path::new(filename);
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(filename)
        .to_string();
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    (stem, ext)
}

#[component]
pub fn GameDetailPage() -> impl IntoView {
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
            <Suspense fallback=move || view! { <GameDetailSkeleton /> }>
                {move || Suspend::new(async move {
                    let data = detail.await?;
                    Ok::<_, ServerFnError>(view! {
                        <GameDetailContent detail=data system=system() />
                    })
                })}
            </Suspense>
        </div>
    }
}

/// Skeleton placeholder for the game detail page while data loads.
#[component]
fn GameDetailSkeleton() -> impl IntoView {
    view! {
        // Header skeleton
        <div class="rom-header">
            <div class="skeleton-detail-back skeleton-shimmer"></div>
            <div class="skeleton-detail-title skeleton-shimmer"></div>
        </div>

        // Cover art skeleton
        <section class="section">
            <div class="game-cover skeleton-detail-cover skeleton-shimmer"></div>
        </section>

        // Launch CTA skeleton
        <section class="game-launch-cta">
            <div class="skeleton-detail-launch skeleton-shimmer"></div>
        </section>

        // Info grid skeleton
        <section class="section">
            <div class="skeleton-detail-section-title skeleton-shimmer"></div>
            <div class="game-meta-grid">
                {(0..4).map(|_| view! {
                    <div class="game-meta-item">
                        <div class="skeleton-detail-meta-label skeleton-shimmer"></div>
                        <div class="skeleton-detail-meta-value skeleton-shimmer"></div>
                    </div>
                }).collect::<Vec<_>>()}
            </div>
        </section>
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
    let base_title_sv = StoredValue::new(detail.base_title.clone());
    let system_display = game.system_display.clone();
    let size_display = format_size_for_system(detail.size_bytes, &system);
    let has_arcade = game.rotation.is_some();
    let (_, ext_lower) = split_filename(&game.rom_filename);
    let ext = ext_lower.to_uppercase();
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
    // Prefer the full ISO `release_date` + `release_precision`;
    // fall back to `year` (derived from legacy sources).
    let release_display = match game.release_date.as_deref() {
        Some(date) => crate::util::format_release_date(
            date,
            game.release_precision,
            i18n.locale.get_untracked(),
        )
        .unwrap_or_else(|| game.year.clone()),
        None => game.year.clone(),
    };
    let has_year = !release_display.is_empty();
    let has_developer = !game.developer.is_empty();
    let has_genre = !game.genre.is_empty();
    let has_players = game.players > 0;
    let year = StoredValue::new(release_display);
    let developer = StoredValue::new(game.developer.clone());
    let genre = StoredValue::new(game.genre.clone());
    let players_str = if game.players > 0 {
        if game.cooperative && game.players > 1 {
            format!("{} (Co-op)", game.players)
        } else {
            game.players.to_string()
        }
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
    let title_url = StoredValue::new(game.title_url.clone());
    let has_title = game.title_url.is_some();

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

    // Combined lightbox image list: box art (reactive — picker can swap it),
    // then title screen, in-game screenshot, then user captures. Indices below
    // are derived from the same offsets so the click handlers stay in sync.
    let title_offset = move || usize::from(box_art_url.read().is_some());
    let screenshot_offset = move || title_offset() + usize::from(has_title);
    let captures_offset = move || screenshot_offset() + usize::from(has_screenshot);
    let lightbox_images = Memo::new(move |_| {
        let mut imgs: Vec<LightboxImage> = Vec::new();
        if let Some(url) = box_art_url.get() {
            imgs.push(LightboxImage {
                url,
                pixelated: false,
            });
        }
        if let Some(url) = title_url.get_value() {
            imgs.push(LightboxImage {
                url,
                pixelated: true,
            });
        }
        if let Some(url) = screenshot_url.get_value() {
            imgs.push(LightboxImage {
                url,
                pixelated: true,
            });
        }
        for s in user_screenshots.get_value() {
            imgs.push(LightboxImage {
                url: s.url,
                pixelated: true,
            });
        }
        imgs
    });

    // Delete confirmation state
    let confirming_delete = RwSignal::new(false);

    // Rename state
    let is_renaming = RwSignal::new(false);
    // Pre-fill rename with stem only (extension is displayed separately).
    let (file_stem, file_ext) = split_filename(&game.rom_filename);
    let rename_value = RwSignal::new(file_stem);
    let file_ext_sv = StoredValue::new(file_ext);
    let rename_allowed = detail.rename_allowed;
    let rename_reason = StoredValue::new(detail.rename_reason.clone());

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
            t(locale, Key::GameDetailUnfavorite)
        } else {
            t(locale, Key::GameDetailFavorite)
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
                {move || t(i18n.locale.get(), Key::GamesBack)}
            </button>
            <h2 class="page-title">{game_name.clone()}</h2>
        </div>

        // Hero / Cover Art
        <section class="section">
            <div class="game-cover">
                <Show when=move || box_art_url.read().is_some()
                    fallback=move || view! {
                        <BoxArtPlaceholder
                            system=system_sv.get_value()
                            name=game_name_sv.get_value()
                            size="detail".to_string()
                        />
                    }
                >
                    <img
                        class="game-cover-img game-cover-tappable"
                        src=move || box_art_url.get().unwrap_or_default()
                        alt=game_name_sv.get_value()
                        on:click=move |_| lightbox_index.set(Some(0))
                    />
                </Show>
            </div>
            <Show when=move || has_variants>
                <div class="change-cover-link" on:click=move |_| show_picker.set(true)>
                    {move || t(i18n.locale.get(), Key::GameDetailChangeCover)}
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
            <h2 class="section-title">{move || t(i18n.locale.get(), Key::GameDetailInfo)}</h2>
            <div class="game-meta-grid">
                <div class="game-meta-item">
                    <span class="game-meta-label">{move || t(i18n.locale.get(), Key::GameDetailSystem)}</span>
                    <span class="game-meta-value">{system_display.clone()}</span>
                </div>
                <div class="game-meta-item">
                    <span class="game-meta-label">{move || t(i18n.locale.get(), Key::GameDetailFilename)}</span>
                    <span class="game-meta-value">{relative_path_sv.get_value()}</span>
                </div>
                <div class="game-meta-item">
                    <span class="game-meta-label">{move || t(i18n.locale.get(), Key::GameDetailFileSize)}</span>
                    <span class="game-meta-value">{size_display}</span>
                </div>
                <Show when=move || !has_arcade>
                    <div class="game-meta-item">
                        <span class="game-meta-label">{move || t(i18n.locale.get(), Key::GameDetailFormat)}</span>
                        <span class="game-meta-value">{ext.clone()}</span>
                    </div>
                </Show>
                <Show when=move || has_year>
                    <div class="game-meta-item">
                        <span class="game-meta-label">{move || t(i18n.locale.get(), Key::GameDetailReleased)}</span>
                        <span class="game-meta-value">{year.get_value()}</span>
                    </div>
                </Show>
                <Show when=move || has_developer>
                    <div class="game-meta-item">
                        <span class="game-meta-label">{move || t(i18n.locale.get(), Key::GameDetailDeveloper)}</span>
                        <span class="game-meta-value">{developer.get_value()}</span>
                    </div>
                </Show>
                <Show when=move || has_publisher>
                    <div class="game-meta-item">
                        <span class="game-meta-label">{move || t(i18n.locale.get(), Key::GameDetailPublisher)}</span>
                        <span class="game-meta-value">{publisher.get_value()}</span>
                    </div>
                </Show>
                <Show when=move || has_genre>
                    <div class="game-meta-item">
                        <span class="game-meta-label">{move || t(i18n.locale.get(), Key::GameDetailGenre)}</span>
                        <span class="game-meta-value">{genre.get_value()}</span>
                    </div>
                </Show>
                <Show when=move || has_players>
                    <div class="game-meta-item">
                        <span class="game-meta-label">{move || t(i18n.locale.get(), Key::GameDetailPlayers)}</span>
                        <span class="game-meta-value">{players_str.clone()}</span>
                    </div>
                </Show>
                <Show when=move || has_rating>
                    <div class="game-meta-item">
                        <span class="game-meta-label">{move || t(i18n.locale.get(), Key::GameDetailRating)}</span>
                        <span class="game-meta-value">{rating_display.get_value()}</span>
                    </div>
                </Show>

                // Arcade-specific fields
                {rotation.map(|r| view! {
                    <div class="game-meta-item">
                        <span class="game-meta-label">{move || t(i18n.locale.get(), Key::GameDetailRotation)}</span>
                        <span class="game-meta-value">{r}</span>
                    </div>
                })}
                {driver_status.map(|s| {
                    let (dot_class, label) = match s.as_str() {
                        "Working" => (
                            "driver-dot driver-dot-working",
                            "Works perfectly",
                        ),
                        "Imperfect" => (
                            "driver-dot driver-dot-imperfect",
                            "Playable with minor issues",
                        ),
                        "Preliminary" => (
                            "driver-dot driver-dot-preliminary",
                            "Not fully playable",
                        ),
                        _ => (
                            "driver-dot driver-dot-unknown",
                            "Unknown",
                        ),
                    };
                    view! {
                        <div class="game-meta-item">
                            <span class="game-meta-label">{move || t(i18n.locale.get(), Key::GameDetailEmulation)}</span>
                            <span class="game-meta-value game-meta-status">
                                <span class=dot_class></span>
                                {label}
                            </span>
                        </div>
                    }
                })}
                <Show when=move || has_category>
                    <div class="game-meta-item">
                        <span class="game-meta-label">{move || t(i18n.locale.get(), Key::GameDetailRawCategory)}</span>
                        <span class="game-meta-value">{arcade_category.get_value()}</span>
                    </div>
                </Show>
                <Show when=move || is_clone>
                    <div class="game-meta-item">
                        <span class="game-meta-label">{move || t(i18n.locale.get(), Key::GameDetailParentRom)}</span>
                        <span class="game-meta-value">{parent_rom.clone()}</span>
                    </div>
                </Show>

                // Console-specific fields
                {region.map(|r| view! {
                    <div class="game-meta-item">
                        <span class="game-meta-label">{move || t(i18n.locale.get(), Key::GameDetailRegion)}</span>
                        <span class="game-meta-value">{r}</span>
                    </div>
                })}
            </div>
        </section>

        // Description (hidden when no description available)
        <Show when=move || has_description>
            <section class="section game-section">
                <h2 class="game-section-title">{move || t(i18n.locale.get(), Key::GameDetailDescription)}</h2>
                <p class="game-description">{move || description.get_value()}</p>
            </section>
        </Show>

        // Screenshots Gallery (hidden when no screenshots)
        <Show when=move || has_screenshot || has_title>
            <section class="section game-section">
                <h2 class="game-section-title">{move || t(i18n.locale.get(), Key::GameDetailScreenshots)}</h2>
                <div class="game-screenshots">
                    {title_url.get_value().map(|url| view! {
                        <div class="game-screenshot-item">
                            <img
                                class="game-screenshot-img game-screenshot-tappable"
                                src=url
                                alt="Title screen"
                                on:click=move |_| lightbox_index.set(Some(title_offset()))
                            />
                            <span class="game-screenshot-label">{move || t(i18n.locale.get(), Key::GameDetailTitleScreen)}</span>
                        </div>
                    })}
                    {screenshot_url.get_value().map(|url| view! {
                        <div class="game-screenshot-item">
                            <img
                                class="game-screenshot-img game-screenshot-tappable"
                                src=url
                                alt="In-game screenshot"
                                on:click=move |_| lightbox_index.set(Some(screenshot_offset()))
                            />
                            <span class="game-screenshot-label">{move || t(i18n.locale.get(), Key::GameDetailInGame)}</span>
                        </div>
                    })}
                </div>
            </section>
        </Show>

        // User Captures (hidden when none, with helpful prompt)
        <section class="section game-section">
            <h2 class="game-section-title">{move || t(i18n.locale.get(), Key::GameDetailUserCaptures)}</h2>
            <Show when=move || has_user_screenshots
                fallback=move || view! { <p class="game-section-empty">{move || t(i18n.locale.get(), Key::GameDetailNoCaptures)}</p> }
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
                                    on:click=move |_| lightbox_index.set(Some(captures_offset() + i))
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
                        {move || t(i18n.locale.get(), Key::GameDetailViewAllCaptures)}
                        {move || format!(" ({})", user_screenshots.get_value().len())}
                    </button>
                </Show>
            </Show>
        </section>

        <ImageLightbox images=lightbox_images current_index=lightbox_index />

        // Videos — base_title enables cross-variant video sharing
        <GameVideoSection
            system=system_sv
            rom_filename=filename_sv
            display_name=game_name_sv
            base_title=base_title_sv
        />

        // Manual / Documents — base_title enables cross-variant manual sharing.
        <ManualSection
            system=system_sv
            rom_filename=filename_sv
            display_name=game_name_sv
            base_title=base_title_sv
        />

        // Game Status — user-defined play progress.
        <GameStatusSection
            system=system_sv
            rom_filename=filename_sv
        />

        // Related Games (lazy-loaded)
        <RelatedGamesSection
            system=system_sv
            rom_filename=filename_sv
        />

        // Actions
        <section class="section">
            <h2 class="section-title">{move || t(i18n.locale.get(), Key::CommonActions)}</h2>
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
                    file_ext=file_ext_sv
                    rename_allowed
                />

                <GameDeleteAction
                    confirming_delete
                    relative_path=relative_path_sv
                    system=system_sv
                />
            </div>
            {rename_reason.get_value().map(|reason| view! {
                <p class="game-action-note">{reason}</p>
            })}
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
            t(locale, Key::GameDetailLaunching)
        } else if is_launched() {
            t(locale, Key::GameDetailLaunched)
        } else if is_simulated() {
            t(locale, Key::GameDetailLaunchNotReplayos)
        } else if is_error() {
            t(locale, Key::GameDetailLaunchError)
        } else {
            t(locale, Key::GameDetailLaunch)
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
///
/// When `rename_allowed` is false, the button is hidden. The reason is shown
/// separately below the actions row by the parent component.
/// The rename input shows only the stem; the extension is displayed as a fixed suffix.
#[component]
fn GameRenameAction(
    is_renaming: RwSignal<bool>,
    rename_value: RwSignal<String>,
    filename: StoredValue<String>,
    relative_path: StoredValue<String>,
    system: StoredValue<String>,
    file_ext: StoredValue<String>,
    rename_allowed: bool,
) -> impl IntoView {
    let i18n = use_i18n();
    let navigate = use_navigate();

    let do_rename = StoredValue::new(move || {
        let rp = relative_path.get_value();
        let stem = rename_value.get();
        let ext = file_ext.get_value();
        let new_name = if ext.is_empty() {
            stem
        } else {
            format!("{stem}.{ext}")
        };
        let sys = system.get_value();
        is_renaming.set(false);
        let nav = navigate.clone();
        leptos::task::spawn_local(async move {
            if server_fns::rename_rom(sys.clone(), rp, new_name.clone())
                .await
                .is_ok()
            {
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
        <Show when=move || rename_allowed>
            <Show when=move || is_renaming.get() fallback=move || view! {
                <button class="game-action-btn" on:click=move |_| {
                    // Pre-fill with stem (no extension).
                    let (stem, _) = split_filename(&filename.get_value());
                    rename_value.set(stem);
                    is_renaming.set(true);
                }>
                    <span class="game-action-icon">{"\u{270F}"}</span>
                    {move || t(i18n.locale.get(), Key::CommonRename)}
                </button>
            }>
                <div class="game-rename-inline">
                    <div class="rename-input-group">
                        <input
                            type="text"
                            class="rename-input"
                            prop:value=move || rename_value.get()
                            on:input=move |ev| rename_value.set(event_target_value(&ev))
                            on:keydown=on_keydown
                        />
                        <span class="rename-ext">{move || {
                            let ext = file_ext.get_value();
                            if ext.is_empty() { String::new() } else { format!(".{ext}") }
                        }}</span>
                    </div>
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
        </Show>
    }
}

/// Delete action: shows file count and total size in confirmation.
///
/// When the user clicks "Delete", a file group is fetched to show what
/// will be deleted, then the user confirms or cancels.
#[component]
fn GameDeleteAction(
    confirming_delete: RwSignal<bool>,
    relative_path: StoredValue<String>,
    system: StoredValue<String>,
) -> impl IntoView {
    let i18n = use_i18n();
    let navigate = use_navigate();

    // File group info for the delete confirmation.
    let delete_info = RwSignal::new(Option::<(usize, String)>::None);

    let on_start_delete = move |_| {
        confirming_delete.set(true);
        delete_info.set(None);

        let sys = system.get_value();
        let rp = relative_path.get_value();
        leptos::task::spawn_local(async move {
            if let Ok(group) = server_fns::get_rom_file_group(sys, rp).await {
                let size_display = crate::util::format_size(group.total_size);
                delete_info.set(Some((group.file_count, size_display)));
            }
        });
    };

    let nav_sv = StoredValue::new(navigate);
    let on_delete = move |_| {
        let rp = relative_path.get_value();
        let sys = system.get_value();
        let nav = nav_sv.get_value();
        leptos::task::spawn_local(async move {
            if server_fns::delete_rom(sys.clone(), rp).await.is_ok() {
                let href = format!("/games/{sys}");
                nav(&href, Default::default());
            }
        });
    };

    view! {
        <Show when=move || confirming_delete.get() fallback=move || view! {
            <button class="game-action-btn game-action-delete" on:click=on_start_delete>
                <span class="game-action-icon">{"\u{2715}"}</span>
                {move || t(i18n.locale.get(), Key::CommonDelete)}
            </button>
        }>
            <div class="game-delete-confirm">
                {move || {
                    delete_info.get().map(|(count, size)| {
                        if count > 1 {
                            view! {
                                <p class="delete-info">
                                    {format!("{count} files ({size})")}
                                </p>
                            }.into_any()
                        } else {
                            view! {
                                <p class="delete-info">{size}</p>
                            }.into_any()
                        }
                    })
                }}
                <button class="game-action-btn game-action-delete-confirm" on:click=on_delete>
                    {move || t(i18n.locale.get(), Key::GameDetailConfirmDelete)}
                </button>
                <button class="game-action-btn" on:click=move |_| confirming_delete.set(false)>
                    {move || t(i18n.locale.get(), Key::CommonCancel)}
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
                        let has_alternates = !data.alternate_versions.is_empty();
                        let has_specials = !data.specials.is_empty();
                        let has_arcade_versions = !data.arcade_versions.is_empty();
                        let has_aliases = !data.alias_variants.is_empty();
                        let has_cross_system = !data.cross_system.is_empty();
                        let has_series = !data.series_siblings.is_empty();
                        let has_similar = !data.similar_games.is_empty();
                        let has_sequel_nav = data.sequel_prev.is_some() || data.sequel_next.is_some();
                        if !has_variants && !has_translations && !has_hacks && !has_alternates && !has_specials && !has_arcade_versions && !has_aliases && !has_cross_system && !has_series && !has_similar && !has_sequel_nav {
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
                            let alternate_chips: Vec<ChipItem> = data.alternate_versions.iter().map(|v| {
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
                                        title_key=Key::GameDetailRegionalVariants
                                        chips=variant_chips.clone()
                                    />
                                </Show>
                                <Show when=move || has_translations>
                                    <GameChipRow
                                        title_key=Key::GameDetailTranslations
                                        chips=translation_chips.clone()
                                    />
                                </Show>
                                <Show when=move || has_hacks>
                                    <GameChipRow
                                        title_key=Key::GameDetailHacks
                                        chips=hack_chips.clone()
                                    />
                                </Show>
                                <Show when=move || has_alternates>
                                    <GameChipRow
                                        title_key=Key::GameDetailAlternateVersions
                                        chips=alternate_chips.clone()
                                    />
                                </Show>
                                <Show when=move || has_specials>
                                    <GameChipRow
                                        title_key=Key::GameDetailSpecialVersions
                                        chips=special_chips.clone()
                                    />
                                </Show>
                                <Show when=move || has_arcade_versions>
                                    <GameChipRow
                                        title_key=Key::GameDetailArcadeVersions
                                        chips=arcade_version_chips.clone()
                                    />
                                </Show>
                                <Show when=move || has_cross_system>
                                    <SimilarGamesRow
                                        games=data.cross_system.clone()
                                        title_key=Key::GameDetailAlsoAvailableOn
                                    />
                                </Show>
                                <Show when=move || has_aliases>
                                    <SimilarGamesRow
                                        games=data.alias_variants.clone()
                                        title_key=Key::GameDetailOtherVersions
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
                                        title_key=Key::GameDetailMoreInSeries
                                        custom_title=data.series_name.clone()
                                    />
                                </Show>
                                <Show when=move || has_similar>
                                    <SimilarGamesRow
                                        games=data.similar_games.clone()
                                        title_key=Key::GameDetailMoreLikeThis
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
fn GameChipRow(title_key: Key, chips: Vec<ChipItem>) -> impl IntoView {
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
    #[prop(default = Key::GameDetailMoreLikeThis)] title_key: Key,
    /// Optional series name from Wikidata. When non-empty, interpolated into
    /// `GameDetailMoreOfSeries` (e.g. "More of Kirby"), overriding `title_key`.
    #[prop(default = String::new())]
    custom_title: String,
) -> impl IntoView {
    let i18n = use_i18n();
    let has_custom = !custom_title.is_empty();

    view! {
        <section class="section game-section">
            <h2 class="game-section-title">
                {move || {
                    let locale = i18n.locale.get();
                    if has_custom {
                        tf(locale, Key::GameDetailMoreOfSeries, &[&custom_title])
                    } else {
                        t(locale, title_key).to_string()
                    }
                }}
            </h2>
            <div class="scroll-card-row">
                {games.into_iter().map(|game| {
                    let name = game.label.unwrap_or(game.display_name);
                    view! {
                        <GameScrollCard
                            href=game.href
                            name=name
                            system=game.system_display
                            system_folder=game.system
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
            <h2 class="game-section-title">{move || t(i18n.locale.get(), Key::GameDetailPlayOrder)}</h2>
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
                                <span class="play-order-subtitle">{move || t(i18n.locale.get(), Key::GameDetailNotInLibrary)}</span>
                            </div>
                        }.into_any()
                    }
                    None => view! { <div class="play-order-spacer" /> }.into_any(),
                }}
                // Position indicator (center)
                {position.map(|(n, m)| {
                    view! { <span class="play-order-position">{move || crate::i18n::tf(i18n.locale.get(), Key::GameDetailNOfM, &[&n.to_string(), &m.to_string()])}</span> }
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
                                <span class="play-order-subtitle">{move || t(i18n.locale.get(), Key::GameDetailNotInLibrary)}</span>
                            </div>
                        }.into_any()
                    }
                    None => view! { <div class="play-order-spacer" /> }.into_any(),
                }}
            </div>
        </section>
    }
}
