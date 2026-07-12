use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::{use_location, use_navigate, use_params_map};
use server_fn::ServerFnError;

use crate::components::boxart_picker::BoxArtPicker;
use crate::components::boxart_placeholder::BoxArtPlaceholder;
use crate::components::captures::{ImageLightbox, LightboxImage};
use crate::components::confirm_dialog::use_confirm_dialog;
use crate::components::hero_card::GameScrollCard;
use crate::components::resources_section::GameResourcesSection;
use crate::hooks::confirm_replace_running_game;
use crate::i18n::{Key, t, tf, use_i18n};
#[cfg(feature = "hydrate")]
use crate::server_fns::PlaytimeAvailability;
use crate::server_fns::{self, RecommendedGame, RomDetail, VariantChip};
use crate::types::NowPlayingState;
#[cfg(feature = "hydrate")]
use crate::util::format_elapsed_short;
use crate::util::format_game_size;
use replay_control_core::replay_api::ReplayApiStatus;
use replay_control_core::systems;

/// Maximum number of capture thumbnails shown before "View all".
const INITIAL_CAPTURE_COUNT: usize = 8;

/// URL fragment that scrolls the page to the manual section. Set when
/// navigating from the home "Now Playing" hero card and read back here to
/// drive `use_focus_scroll` on `<ManualSection>`.
pub const MANUALS_FRAGMENT: &str = "manuals";

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
    let location = use_location();
    let system = move || params.read().get("system").unwrap_or_default();
    let filename = move || {
        let raw = params.read().get("filename").unwrap_or_default();
        // URL-decode the filename
        urlencoding::decode(&raw)
            .map(|s| s.into_owned())
            .unwrap_or(raw)
    };

    let detail = Resource::new_blocking(
        move || (system(), filename()),
        |(sys, fname)| server_fns::get_rom_detail(sys, fname),
    );
    let focus_manuals = move || location.hash.read().trim_start_matches('#') == MANUALS_FRAGMENT;

    view! {
        <div class="page game-detail">
            <Suspense fallback=move || view! { <GameDetailSkeleton /> }>
                {move || Suspend::new(async move {
                    let data = detail.await?;
                    Ok::<_, ServerFnError>(view! {
                        <GameDetailContent
                            detail=data
                            system=system()
                            focus_manuals=Signal::derive(focus_manuals)
                        />
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
fn GameDetailContent(
    detail: RomDetail,
    system: String,
    focus_manuals: Signal<bool>,
) -> impl IntoView {
    let i18n = use_i18n();
    let confirm_dialog = use_confirm_dialog();
    let now_playing = crate::hooks::use_now_playing();

    let game = &detail.game;
    let game_name = game.display_name.clone();
    let game_name_sv = StoredValue::new(game_name.clone());
    let filename_sv = StoredValue::new(game.rom_filename.clone());
    let relative_path_sv = StoredValue::new(game.rom_path.clone());
    let return_to_sv = StoredValue::new(format!(
        "/games/{}/{}",
        system,
        urlencoding::encode(&game.rom_filename)
    ));
    let system_sv = StoredValue::new(system.clone());
    let base_title_sv = StoredValue::new(detail.base_title.clone());

    // Total play time for this game. Browser-only fetch so a slow or
    // unimplemented `get_playtime` endpoint never blocks the SSR response.
    #[cfg(feature = "hydrate")]
    let game_playtime = LocalResource::new(move || {
        server_fns::get_game_playtime(system_sv.get_value(), filename_sv.get_value())
    });
    #[cfg(feature = "hydrate")]
    let playtime_display = move || {
        let locale = i18n.locale.get();
        match game_playtime.get() {
            None => "\u{2026}".to_string(),
            Some(result) => match result {
                Ok(p) if p.availability == PlaytimeAvailability::Tracked => {
                    format_elapsed_short(p.seconds)
                }
                _ => t(locale, Key::PlaytimeUnavailable).to_string(),
            },
        }
    };
    #[cfg(not(feature = "hydrate"))]
    let playtime_display = move || t(i18n.locale.get(), Key::PlaytimeUnavailable).to_string();
    let system_display = game.system_display.clone();
    let size_display = format_game_size(detail.size_bytes, &system);
    let storage_size_display = StoredValue::new(size_display.storage);
    let rom_capacity_display = StoredValue::new(size_display.rom_capacity);
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
    let players_str = StoredValue::new(players_str);
    let summary_players_str = StoredValue::new(if game.players == 1 {
        t(i18n.locale.get_untracked(), Key::GameDetailOnePlayer).to_string()
    } else if game.players > 1 {
        tf(
            i18n.locale.get_untracked(),
            Key::GameDetailPlayerRange,
            &[&game.players.to_string()],
        )
    } else {
        String::new()
    });

    // Arcade-specific fields
    let rotation = game.rotation.clone();
    let driver_status = game.driver_status.clone();
    let is_clone = game.is_clone.unwrap_or(false);
    let parent_rom = game.parent_rom.clone();
    let arcade_category = StoredValue::new(game.arcade_category.clone());
    let has_category = game.arcade_category.is_some();
    let is_mature = game.is_mature;
    let arcade_board = game.arcade_board.clone();
    let arcade_board_tag = game.arcade_board_tag.clone();

    // Console-specific fields
    let region = game.region.clone();

    // RetroAchievements support flag (non-empty id means a known RA set exists).
    let has_achievements = !game.ra_id.is_empty();
    // Number of achievements in the set, shown next to the trophy when known.
    let ra_count = game.ra_count;
    // We always show the trophy when an `ra_id` exists (trusting the match), but the
    // game can't actually earn achievements on RePlay when the system's core doesn't
    // support RA at all (PSX, PCE-CD, MAME, ST-V) — in which case we add a note.
    let ra_blocked_by_core =
        has_achievements && !systems::system_core_supports_retroachievements(&system);

    // External metadata
    let description = StoredValue::new(game.description.clone());
    let has_description = game.description.is_some();
    let description_expanded = RwSignal::new(false);
    let description_needs_toggle = game
        .description
        .as_ref()
        .is_some_and(|text| text.lines().count() > 6 || text.chars().count() > 540);
    let description_class = move || {
        if description_expanded.get() || !description_needs_toggle {
            "game-description"
        } else {
            "game-description game-description-clamped"
        }
    };
    let description_toggle_label = move || {
        let key = if description_expanded.get() {
            Key::GameDetailShowLess
        } else {
            Key::GameDetailShowMore
        };
        t(i18n.locale.get(), key)
    };
    let has_rating = game.rating.is_some();
    let rating_display = StoredValue::new(game.rating.map(|r| format!("{:.1} / 5.0", r)));
    let has_publisher = game.publisher.as_ref().is_some_and(|p| !p.is_empty());
    let publisher = StoredValue::new(game.publisher.clone().unwrap_or_default());

    let library_resources = StoredValue::new(detail.library_resources.clone());
    // Resources-section data bundled into the detail payload (see RomDetail).
    // Passed in so the section renders from this one request instead of issuing
    // six separate per-section fetches on a client-side navigation.
    let documents_sv = StoredValue::new(detail.documents.clone());
    let local_manuals_sv = StoredValue::new(detail.local_manuals.clone());
    let saved_videos_sv = StoredValue::new(detail.saved_videos.clone());
    let saved_resource_links_sv = StoredValue::new(detail.saved_resource_links.clone());
    let manual_suggestions_sv = StoredValue::new(detail.manual_suggestions.clone());
    let video_suggestions_sv = StoredValue::new(detail.video_suggestions.clone());

    // Images — box_art_url is an RwSignal so the picker can update it reactively.
    let box_art_url = RwSignal::new(game.box_art_url.clone());
    let screenshot_url = StoredValue::new(game.screenshot_url.clone());
    let has_screenshot = game.screenshot_url.is_some();
    let title_url = StoredValue::new(game.title_url.clone());
    let has_title = game.title_url.is_some();

    let active_started_at = move || match now_playing.get() {
        crate::types::NowPlayingState::Playing {
            ref system,
            ref filename,
            started_at_unix_secs,
            ..
        } if *system == system_sv.get_value() && *filename == filename_sv.get_value() => {
            Some(started_at_unix_secs)
        }
        _ => None,
    };
    let is_now_playing = Memo::new(move |_| active_started_at().is_some());

    // Box art variant picker state.
    // Suppress "Change cover" for hack and special ROMs — they should inherit the base ROM's cover.
    let variant_count = detail.variant_count;
    let has_variants = variant_count > 1 && !detail.is_hack && !detail.is_special;
    let show_picker = RwSignal::new(false);

    // User captures
    let user_screenshots = RwSignal::new(detail.user_screenshots.clone());
    let has_user_screenshots = move || !user_screenshots.read().is_empty();
    let captures_show_all = RwSignal::new(false);
    let lightbox_index = RwSignal::new(Option::<usize>::None);
    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::JsCast;
        use wasm_bindgen::prelude::Closure;

        let refresh_in_flight = RwSignal::new(false);
        let sys = system_sv.get_value();
        let rom = filename_sv.get_value();
        let interval_callback = Closure::<dyn FnMut()>::new(move || {
            if refresh_in_flight.get_untracked() {
                return;
            }
            refresh_in_flight.set(true);
            let sys = sys.clone();
            let rom = rom.clone();
            leptos::task::spawn_local(async move {
                if let Ok(latest) = server_fns::get_user_captures(sys, rom).await {
                    user_screenshots.update(|current| {
                        if *current != latest {
                            *current = latest;
                        }
                    });
                }
                refresh_in_flight.set(false);
            });
        });
        if let Ok(interval_id) = window().set_interval_with_callback_and_timeout_and_arguments_0(
            interval_callback.as_ref().unchecked_ref(),
            2_500,
        ) {
            interval_callback.forget();
            on_cleanup(move || {
                window().clear_interval_with_handle(interval_id);
            });
        }
    }

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
                can_delete: false,
            });
        }
        if let Some(url) = title_url.get_value() {
            imgs.push(LightboxImage {
                url,
                pixelated: true,
                can_delete: false,
            });
        }
        if let Some(url) = screenshot_url.get_value() {
            imgs.push(LightboxImage {
                url,
                pixelated: true,
                can_delete: false,
            });
        }
        for s in user_screenshots.read().iter() {
            imgs.push(LightboxImage {
                url: s.url.clone(),
                pixelated: true,
                can_delete: true,
            });
        }
        imgs
    });

    let delete_capture_at = move |capture_index: usize| {
        let capture = user_screenshots.read().get(capture_index).cloned();
        let Some(capture) = capture else {
            return;
        };
        let locale = i18n.locale.get_untracked();
        confirm_dialog.confirm(
            t(locale, Key::GameDetailDeleteCapture),
            t(locale, Key::GameDetailDeleteCaptureConfirm),
            t(locale, Key::CommonDelete),
            true,
            Callback::new(move |()| {
                let capture = capture.clone();
                // Defer the optimistic removal: removing the capture and closing
                // the lightbox unmount the element this Callback is owned by,
                // which would drop it mid-run (wasm "closure invoked recursively
                // or after being dropped"). Run it after this returns.
                leptos::task::spawn_local(async move {
                    user_screenshots.update(|captures| {
                        if captures
                            .get(capture_index)
                            .is_some_and(|item| item.filename == capture.filename)
                        {
                            captures.remove(capture_index);
                        } else if let Some(pos) = captures
                            .iter()
                            .position(|item| item.filename == capture.filename)
                        {
                            captures.remove(pos);
                        }
                    });
                    lightbox_index.set(None);

                    let sys = system_sv.get_value();
                    let rom = filename_sv.get_value();
                    if server_fns::delete_user_capture(sys, rom, capture.filename.clone())
                        .await
                        .is_err()
                    {
                        user_screenshots.update(|captures| {
                            if !captures
                                .iter()
                                .any(|item| item.filename == capture.filename)
                            {
                                let insert_at = capture_index.min(captures.len());
                                captures.insert(insert_at, capture);
                            }
                        });
                    }
                });
            }),
        );
    };

    // Rename state
    let is_renaming = RwSignal::new(false);
    // Pre-fill rename with stem only (extension is displayed separately).
    let (file_stem, file_ext) = split_filename(&game.rom_filename);
    let rename_value = RwSignal::new(file_stem);
    let file_ext_sv = StoredValue::new(file_ext);
    let rename_allowed = detail.rename_allowed;
    let rename_reason = StoredValue::new(detail.rename_reason.clone());

    // Toggle favorite
    let remove_favorite = Callback::new(move |()| {
        is_favorite.set(false);
        let sys = system_sv.get_value();
        let fname = filename_sv.get_value();
        let fav_filename = format!("{sys}@{fname}.fav");
        leptos::task::spawn_local(async move {
            let _ = server_fns::remove_favorite(fav_filename, None).await;
        });
    });
    let on_toggle_fav = move |_| {
        if is_favorite.get() {
            // Removing a favorite is cheap and reversible in this page — no confirmation.
            remove_favorite.run(());
            return;
        }

        is_favorite.set(true);
        let sys = system_sv.get_value();
        let rp = relative_path_sv.get_value();
        leptos::task::spawn_local(async move {
            let _ = server_fns::add_favorite(sys, rp, false).await;
        });
    };

    let fav_label = Signal::derive(move || {
        let locale = i18n.locale.get();
        if is_favorite.get() {
            t(locale, Key::GameDetailUnfavorite).to_string()
        } else {
            t(locale, Key::GameDetailFavorite).to_string()
        }
    });

    let fav_icon = Signal::derive(move || {
        if is_favorite.get() {
            "\u{2605}"
        } else {
            "\u{2606}"
        }
    });

    view! {
        // Header
        <div class="rom-header">
            <button class="back-btn" on:click=go_back>
                {move || t(i18n.locale.get(), Key::GamesBack)}
            </button>
            <div class="game-detail-title-wrap">
                <h2 class="page-title" title=game_name.clone()>{game_name.clone()}</h2>
            </div>
        </div>

        // Hero / Cover Art
        <section class="section">
            <div class="game-cover" class:game-cover-playing=move || is_now_playing.get()>
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
            <div class="game-launch-row">
                <GameLaunchAction
                    system=system_sv
                    filename=filename_sv
                    display_name=game_name_sv
                    relative_path=relative_path_sv
                    return_to=return_to_sv
                    already_playing=is_now_playing
                />
                <button
                    type="button"
                    class="game-action-btn game-action-fav game-action-fav-cta"
                    class:game-action-fav-active=move || is_favorite.get()
                    aria-label=fav_label
                    title=fav_label
                    on:click=on_toggle_fav
                >
                    <span class="game-action-icon">{move || fav_icon.get()}</span>
                </button>
            </div>
        </section>

        // Game Info Card
        <section class="section">
            <h2 class="section-title">{move || t(i18n.locale.get(), Key::GameDetailInfo)}</h2>
            <div class="game-info-strip">
                <span><strong>{system_display.clone()}</strong></span>
                <Show when=move || has_year>
                    <span>{year.get_value()}</span>
                </Show>
                <Show when=move || has_genre>
                    <span>{genre.get_value()}</span>
                </Show>
                <Show when=move || has_players>
                    <span>{summary_players_str.get_value()}</span>
                </Show>
            </div>

            <Show when=move || has_description>
                <div class="game-description-block">
                    <p class=description_class>{move || description.get_value()}</p>
                    <Show when=move || description_needs_toggle>
                        <button
                            type="button"
                            class="game-description-toggle"
                            on:click=move |_| description_expanded.update(|expanded| *expanded = !*expanded)
                        >
                            {description_toggle_label}
                        </button>
                    </Show>
                </div>
            </Show>

            <div class="info-grid game-info-table">
                <div class="info-row game-info-row game-info-row-filename">
                    <span class="info-label">{move || t(i18n.locale.get(), Key::GameDetailFilename)}</span>
                    <span class="info-value" title=relative_path_sv.get_value()>{relative_path_sv.get_value()}</span>
                </div>
                <div class="info-row game-info-row">
                    <span class="info-label">{move || t(i18n.locale.get(), Key::GameDetailStorageSize)}</span>
                    <span class="info-value">{storage_size_display.get_value()}</span>
                </div>
                <Show when=move || rom_capacity_display.get_value().is_some()>
                    <div class="info-row game-info-row">
                        <span class="info-label">{move || t(i18n.locale.get(), Key::GameDetailRomCapacity)}</span>
                        <span class="info-value">{move || rom_capacity_display.get_value().unwrap_or_default()}</span>
                    </div>
                </Show>
                <Show when=move || !has_arcade>
                    <div class="info-row game-info-row">
                        <span class="info-label">{move || t(i18n.locale.get(), Key::GameDetailFormat)}</span>
                        <span class="info-value">{ext.clone()}</span>
                    </div>
                </Show>
                <Show when=move || has_year>
                    <div class="info-row game-info-row">
                        <span class="info-label">{move || t(i18n.locale.get(), Key::GameDetailReleased)}</span>
                        <span class="info-value">{year.get_value()}</span>
                    </div>
                </Show>
                <Show when=move || has_developer>
                    <div class="info-row game-info-row">
                        <span class="info-label">{move || t(i18n.locale.get(), Key::GameDetailDeveloper)}</span>
                        <span class="info-value">{developer.get_value()}</span>
                    </div>
                </Show>
                <Show when=move || has_publisher>
                    <div class="info-row game-info-row">
                        <span class="info-label">{move || t(i18n.locale.get(), Key::GameDetailPublisher)}</span>
                        <span class="info-value">{publisher.get_value()}</span>
                    </div>
                </Show>
                <Show when=move || has_genre>
                    <div class="info-row game-info-row">
                        <span class="info-label">{move || t(i18n.locale.get(), Key::GameDetailGenre)}</span>
                        <span class="info-value">{genre.get_value()}</span>
                    </div>
                </Show>
                <Show when=move || has_players>
                    <div class="info-row game-info-row">
                        <span class="info-label">{move || t(i18n.locale.get(), Key::GameDetailPlayers)}</span>
                        <span class="info-value">{players_str.get_value()}</span>
                    </div>
                </Show>
                <Show when=move || has_rating>
                    <div class="info-row game-info-row">
                        <span class="info-label">{move || t(i18n.locale.get(), Key::GameDetailRating)}</span>
                        <span class="info-value">{rating_display.get_value()}</span>
                    </div>
                </Show>
                <Show when=move || has_achievements>
                    <div class="info-row game-info-row">
                        <span class="info-label">{move || t(i18n.locale.get(), Key::GameDetailRetroAchievements)}</span>
                        <span class="info-value">
                            "\u{1F3C6}"
                            <Show when=move || { ra_count > 0 }>
                                <span class="ra-count">{format!("\u{00A0}{ra_count}")}</span>
                            </Show>
                            <Show when=move || ra_blocked_by_core>
                                <span class="game-meta-note">
                                    {move || t(i18n.locale.get(), Key::GameDetailRetroAchievementsNoCore)}
                                </span>
                            </Show>
                        </span>
                    </div>
                </Show>

                // Arcade-specific fields
                {arcade_board.zip(arcade_board_tag).map(|(label, tag)| {
                    let href = format!("/board/{}", urlencoding::encode(&tag));
                    view! {
                        <div class="info-row game-info-row">
                            <span class="info-label">{move || t(i18n.locale.get(), Key::GameDetailBoard)}</span>
                            <A href=href attr:class="info-value game-meta-link">{label}</A>
                        </div>
                    }
                })}
                {rotation.map(|r| view! {
                    <div class="info-row game-info-row">
                        <span class="info-label">{move || t(i18n.locale.get(), Key::GameDetailRotation)}</span>
                        <span class="info-value">{r}</span>
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
                        <div class="info-row game-info-row">
                            <span class="info-label">{move || t(i18n.locale.get(), Key::GameDetailEmulation)}</span>
                            <span class="info-value game-meta-status">
                                <span class=dot_class></span>
                                {label}
                            </span>
                        </div>
                    }
                })}
                <Show when=move || has_category>
                    <div class="info-row game-info-row">
                        <span class="info-label">{move || t(i18n.locale.get(), Key::GameDetailRawCategory)}</span>
                        <span class="info-value">{arcade_category.get_value()}</span>
                    </div>
                </Show>
                <Show when=move || is_mature>
                    <div class="info-row game-info-row">
                        <span class="info-label">{move || t(i18n.locale.get(), Key::GameDetailMatureCategory)}</span>
                        <span class="info-value">{move || t(i18n.locale.get(), Key::CommonYes)}</span>
                    </div>
                </Show>
                <Show when=move || is_clone>
                    <div class="info-row game-info-row">
                        <span class="info-label">{move || t(i18n.locale.get(), Key::GameDetailParentRom)}</span>
                        <span class="info-value">{parent_rom.clone()}</span>
                    </div>
                </Show>

                // Console-specific fields
                {region.map(|r| view! {
                    <div class="info-row game-info-row">
                        <span class="info-label">{move || t(i18n.locale.get(), Key::GameDetailRegion)}</span>
                        <span class="info-value">{r}</span>
                    </div>
            })}

                // Total play time, always shown (placeholder when tracking is
                // off or the device doesn't report play time yet).
                <div class="info-row game-info-row">
                    <span class="info-label">{move || t(i18n.locale.get(), Key::GameDetailPlaytime)}</span>
                    <span class="info-value">{playtime_display}</span>
                </div>
            </div>
        </section>

        // Screenshots and user captures.
        <section class="section game-section">
            <h2 class="game-section-title">{move || t(i18n.locale.get(), Key::GameDetailScreenshots)}</h2>

            <Show when=move || has_screenshot || has_title>
                <div class="screenshot-group screenshot-group-provided">
                    <div class="screenshot-thumb-grid">
                        {title_url.get_value().map(|url| view! {
                            <div class="screenshot-card">
                                <img
                                    class="screenshot-thumb screenshot-thumb-tappable"
                                    src=url
                                    alt="Title screen"
                                    on:click=move |_| lightbox_index.set(Some(title_offset()))
                                />
                                <span class="screenshot-card-label">{move || t(i18n.locale.get(), Key::GameDetailTitleScreen)}</span>
                            </div>
                        })}
                        {screenshot_url.get_value().map(|url| view! {
                            <div class="screenshot-card">
                                <img
                                    class="screenshot-thumb screenshot-thumb-tappable"
                                    src=url
                                    alt="In-game screenshot"
                                    on:click=move |_| lightbox_index.set(Some(screenshot_offset()))
                                />
                                <span class="screenshot-card-label">{move || t(i18n.locale.get(), Key::GameDetailInGame)}</span>
                            </div>
                        })}
                    </div>
                </div>
            </Show>

            <div class="screenshot-group screenshot-group-captures">
                <div class="screenshot-group-label">{move || t(i18n.locale.get(), Key::GameDetailUserCaptures)}</div>
            <Show when=has_user_screenshots
                fallback=move || view! { <p class="game-section-empty">{move || t(i18n.locale.get(), Key::GameDetailNoCaptures)}</p> }
            >
                <div class="screenshot-thumb-grid">
                    {move || {
                        let all = user_screenshots.get();
                        let show_all = captures_show_all.get();
                        let visible = if show_all || all.len() <= INITIAL_CAPTURE_COUNT {
                            all.clone()
                        } else {
                            all[..INITIAL_CAPTURE_COUNT].to_vec()
                        };
                        visible.into_iter().enumerate().map(|(i, s)| {
                            let url = s.url.clone();
                            view! {
                                <div class="screenshot-card screenshot-card-capture">
                                    <img
                                        class="screenshot-thumb screenshot-thumb-tappable"
                                        src=url
                                        alt="Capture"
                                        on:click=move |_| lightbox_index.set(Some(captures_offset() + i))
                                    />
                                    <button
                                        type="button"
                                        class="capture-delete-btn"
                                        aria-label=move || t(i18n.locale.get(), Key::GameDetailDeleteCapture)
                                        title=move || t(i18n.locale.get(), Key::GameDetailDeleteCapture)
                                        on:click=move |ev| {
                                            ev.stop_propagation();
                                            delete_capture_at(i);
                                        }
                                    >
                                        "x"
                                    </button>
                                </div>
                            }
                        }).collect::<Vec<_>>()
                    }}
                </div>
                <Show when=move || { user_screenshots.read().len() > INITIAL_CAPTURE_COUNT && !captures_show_all.get() }>
                    <button
                        class="game-action-btn captures-show-all"
                        on:click=move |_| captures_show_all.set(true)
                    >
                        {move || t(i18n.locale.get(), Key::GameDetailViewAllCaptures)}
                        {move || format!(" ({})", user_screenshots.read().len())}
                    </button>
                </Show>
            </Show>
            </div>
        </section>

        <ImageLightbox
            images=lightbox_images
            current_index=lightbox_index
            delete_label=Signal::derive(move || t(i18n.locale.get(), Key::GameDetailDeleteCapture).to_string())
            on_delete=Callback::new(move |image_index: usize| {
                let offset = captures_offset();
                if image_index >= offset {
                    delete_capture_at(image_index - offset);
                }
            })
        />

        <GameResourcesSection
            system=system_sv
            rom_filename=filename_sv
            base_title=base_title_sv
            display_name=game_name_sv
            library_resources
            initial_documents=documents_sv
            initial_local_manuals=local_manuals_sv
            initial_saved_videos=saved_videos_sv
            initial_saved_resource_links=saved_resource_links_sv
            initial_manual_suggestions=manual_suggestions_sv
            initial_video_suggestions=video_suggestions_sv
            section_id="manuals"
            focus_on_mount=focus_manuals
        />

        // Related Games (lazy-loaded)
        <RelatedGamesSection
            system=system_sv
            rom_filename=filename_sv
        />

        // Actions
        <section class="section">
            <h2 class="section-title">{move || t(i18n.locale.get(), Key::GameDetailMoreActions)}</h2>
            <div class="game-actions">
                <GameRenameAction
                    is_renaming rename_value
                    filename=filename_sv
                    relative_path=relative_path_sv
                    system=system_sv
                    file_ext=file_ext_sv
                    rename_allowed
                />

                <GameDeleteAction
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
fn GameLaunchAction(
    system: StoredValue<String>,
    filename: StoredValue<String>,
    display_name: StoredValue<String>,
    relative_path: StoredValue<String>,
    return_to: StoredValue<String>,
    #[prop(into)] already_playing: Signal<bool>,
) -> impl IntoView {
    let i18n = use_i18n();
    let confirm_dialog = use_confirm_dialog();
    let now_playing = crate::hooks::use_now_playing();
    let _ = return_to;
    let launch = ServerAction::<server_fns::LaunchGame>::new();
    let mode = Resource::new_blocking(|| (), |_| server_fns::get_mode());
    let initial_replay_api_status =
        Resource::new_blocking(|| (), |_| server_fns::get_replay_api_status());
    let replay_api_status = use_context::<RwSignal<ReplayApiStatus>>();
    let launching = launch.pending();
    let launch_result = launch.value();
    let launch_clicked = RwSignal::new(false);
    let launch_requires_setup = Memo::new(move |_| {
        let on_device = mode
            .get()
            .and_then(Result::ok)
            .is_some_and(|mode| mode.is_device());
        if !on_device {
            return false;
        }

        let live_status = replay_api_status
            .map(|status| status.get())
            .unwrap_or_default();
        let status = if live_status == ReplayApiStatus::default() {
            initial_replay_api_status
                .get()
                .and_then(Result::ok)
                .unwrap_or(live_status)
        } else {
            live_status
        };
        !status.is_active()
    });

    Effect::new(move |_| {
        if launch_result.get().is_some() {
            launch_clicked.set(false);
        }
    });

    let is_launched =
        move || matches!(launch_result.get(), Some(Ok(ref m)) if !m.contains("simulated"));
    let is_simulated =
        move || matches!(launch_result.get(), Some(Ok(ref m)) if m.contains("simulated"));
    let is_error = move || matches!(launch_result.get(), Some(Err(_)));
    let is_disabled = move || launching.get() || is_launched() || already_playing.get();
    let is_launching =
        move || launching.get() || (launch_clicked.get() && launch_result.get().is_none());
    let error_message = move || match launch_result.get() {
        Some(Err(error)) => Some(error.to_string()),
        _ => None,
    };

    let label = move || {
        let locale = i18n.locale.get();
        if already_playing.get() {
            t(locale, Key::GameDetailAlreadyPlaying)
        } else if is_launching() {
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
    let dispatch_launch = Callback::new(move |()| {
        launch_clicked.set(true);
        launch.dispatch(server_fns::LaunchGame {
            rom_path: relative_path.get_value(),
            return_to: String::new(),
        });
    });
    let on_launch = move |_| {
        if is_disabled() || launch_requires_setup.get() {
            return;
        }
        if let NowPlayingState::Playing {
            system: cur_system,
            filename: cur_filename,
            display_name: cur_name,
            ..
        } = now_playing.get_untracked()
            && (cur_system != system.get_value() || cur_filename != filename.get_value())
        {
            let locale = i18n.locale.get_untracked();
            let next_name = display_name.get_value();
            confirm_replace_running_game(
                confirm_dialog,
                locale,
                &next_name,
                &cur_name,
                dispatch_launch,
            );
            return;
        }
        dispatch_launch.run(());
    };

    view! {
        <div class="game-launch-action">
            <Show
                when=move || launch_requires_setup.get()
                fallback=move || view! {
                    <button
                        type="button"
                        class="game-action-launch"
                        class:game-action-launch-success=is_launched
                        class:game-action-launch-simulated=is_simulated
                        class:game-action-launch-playing=move || already_playing.get()
                        prop:disabled=is_disabled
                        on:click=on_launch
                    >
                        <span class="game-action-icon">{"\u{25B6}"}</span>
                        {label}
                    </button>
                }
            >
                <A
                    href="/settings/replayos"
                    attr:class="game-action-launch game-action-launch-setup"
                >
                    <span class="game-action-icon">{"\u{25B6}"}</span>
                    {move || t(i18n.locale.get(), Key::SetupReplayosTitle)}
                </A>
            </Show>
            <Show when=move || error_message().is_some() && !launch_requires_setup.get()>
                <p class="game-action-error">{move || error_message().unwrap_or_default()}</p>
            </Show>
        </div>
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

/// Build the confirm-dialog message for a ROM delete: the standard shared
/// `ConfirmDialog` only takes a plain string, so a multi-file group (e.g. a
/// ZIP plus a MAME CHD companion folder) is rendered as one file per line
/// rather than as its own dedicated dialog layout — kept consistent with
/// every other confirmation in the app instead of a one-off custom panel.
fn delete_confirm_message(locale: crate::i18n::Locale, group: &server_fns::RomFileGroup) -> String {
    // Directory summary rows stand in for `dir_file_count` files each.
    let total_files: usize = group
        .files
        .iter()
        .map(|f| f.dir_file_count.unwrap_or(1))
        .sum();
    if total_files <= 1 {
        return tf(
            locale,
            Key::GameDetailConfirmDeleteSingle,
            &[&crate::util::format_storage_size(group.total_size)],
        );
    }
    let lines: String = group
        .files
        .iter()
        .map(|f| {
            let size = crate::util::format_storage_size(f.size_bytes);
            match f.dir_file_count {
                Some(n) => format!(
                    "\u{2022} {} ({}, {size})",
                    f.filename,
                    tf(locale, Key::GameDetailDeleteDirFiles, &[&n.to_string()]),
                ),
                None => format!("\u{2022} {} ({size})", f.filename),
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    tf(
        locale,
        Key::GameDetailConfirmDeleteMultiple,
        &[
            &total_files.to_string(),
            &crate::util::format_storage_size(group.total_size),
            &lines,
        ],
    )
}

/// Delete action: fetches the file group (ZIP, any companion directory
/// contents) and shows it via the shared confirm dialog before deleting.
#[component]
fn GameDeleteAction(
    relative_path: StoredValue<String>,
    system: StoredValue<String>,
) -> impl IntoView {
    let i18n = use_i18n();
    let navigate = use_navigate();
    let confirm_dialog = use_confirm_dialog();
    let nav_sv = StoredValue::new(navigate);

    let on_click_delete = move |_| {
        let sys = system.get_value();
        let rp = relative_path.get_value();
        let locale = i18n.locale.get_untracked();
        leptos::task::spawn_local(async move {
            let Ok(group) = server_fns::get_rom_file_group(sys.clone(), rp.clone()).await else {
                return;
            };
            let message = delete_confirm_message(locale, &group);
            let delete_label = t(locale, Key::CommonDelete).to_string();
            confirm_dialog.confirm(
                delete_label.clone(),
                message,
                delete_label,
                true,
                Callback::new(move |()| {
                    let sys = sys.clone();
                    let rp = rp.clone();
                    let nav = nav_sv.get_value();
                    leptos::task::spawn_local(async move {
                        if server_fns::delete_rom(sys.clone(), rp).await.is_ok() {
                            let href = format!("/games/{sys}");
                            nav(&href, Default::default());
                        }
                    });
                }),
            );
        });
    };

    view! {
        <button class="game-action-btn game-action-delete" on:click=on_click_delete>
            <span class="game-action-icon">{"\u{2715}"}</span>
            {move || t(i18n.locale.get(), Key::CommonDelete)}
        </button>
    }
}

/// Related games section: regional variants and "More Like This" (genre-based).
/// Loads lazily via its own Resource so it never blocks the main page render.
#[component]
fn RelatedGamesSection(
    system: StoredValue<String>,
    rom_filename: StoredValue<String>,
) -> impl IntoView {
    let i18n = use_i18n();
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
                        let has_same_board = !data.same_board.is_empty();
                        let has_sequel_nav = data.sequel_prev.is_some() || data.sequel_next.is_some();
                        if !has_variants && !has_translations && !has_hacks && !has_alternates && !has_specials && !has_arcade_versions && !has_aliases && !has_cross_system && !has_series && !has_similar && !has_same_board && !has_sequel_nav {
                            view! { <div /> }.into_any()
                        } else {
                            let variant_chips: Vec<ChipItem> = data.regional_variants.iter().map(|v| {
                                ChipItem { label: v.region.clone(), href: v.href.clone(), is_current: v.is_current }
                            }).collect();
                            let translation_chips: Vec<ChipItem> = data.translations.iter().map(ChipItem::from).collect();
                            let hack_chips: Vec<ChipItem> = data.hacks.iter().map(ChipItem::from).collect();
                            let alternate_chips: Vec<ChipItem> = data.alternate_versions.iter().map(ChipItem::from).collect();
                            let special_chips: Vec<ChipItem> = data.specials.iter().map(ChipItem::from).collect();
                            let arcade_version_chips: Vec<ChipItem> = data.arcade_versions.iter().map(ChipItem::from).collect();
                            view! {
                                <section class="section recommendations-section">
                                    <h2 class="section-title">{move || t(i18n.locale.get(), Key::GameDetailRecommendations)}</h2>
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
                                    <Show when=move || has_same_board>
                                        <SimilarGamesRow
                                            games=data.same_board.clone()
                                            title_key=Key::GameDetailMoreOnBoard
                                            see_all_href=data.same_board_href.clone()
                                        />
                                    </Show>
                                </section>
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

impl From<&VariantChip> for ChipItem {
    fn from(v: &VariantChip) -> Self {
        ChipItem {
            label: v.label.clone(),
            href: v.href.clone(),
            is_current: v.is_current,
        }
    }
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
    /// Optional "see all" target (e.g. `/board/<tag>`). Renders a link in the
    /// section header when non-empty.
    #[prop(default = String::new())]
    see_all_href: String,
) -> impl IntoView {
    let i18n = use_i18n();
    let has_custom = !custom_title.is_empty();
    let see_all = StoredValue::new(see_all_href);
    let has_see_all = move || !see_all.get_value().is_empty();

    view! {
        <section class="section game-section">
            <div class="game-section-header">
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
                <Show when=has_see_all>
                    <A href=see_all.get_value() attr:class="search-see-all">
                        {move || t(i18n.locale.get(), Key::CommonSeeAll)} " \u{2192}"
                    </A>
                </Show>
            </div>
            <div class="scroll-card-row">
                {games.into_iter().map(|game| {
                    let name = game.label.unwrap_or(game.display_name);
                    let system = systems::system_abbreviation(&game.system);
                    view! {
                        <GameScrollCard
                            href=game.href
                            name=name
                            system=system
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
