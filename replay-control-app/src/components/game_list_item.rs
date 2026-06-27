use leptos::either::Either;
use leptos::prelude::*;
use leptos_router::components::A;

use crate::components::boxart_placeholder::BoxArtPlaceholder;
use crate::hooks::{LaunchControl, use_launch_control};
use crate::i18n::{Key, t, use_i18n};
use crate::server_fns;

/// Unified game list row component used across search results, developer pages,
/// system ROM lists, and other game lists. Shows box art, game name, optional
/// system badge, genre/rating badges, a favorite toggle, and a launch button.
#[component]
pub fn GameListItem(
    system: String,
    rom_filename: String,
    display_name: String,
    rom_path: String,
    box_art_url: Option<String>,
    /// Show the system name (e.g., for cross-system lists like developer page).
    #[prop(default = false)]
    show_system: bool,
    #[prop(default = true)] show_favorite: bool,
    #[prop(default = false)] is_favorite: bool,
    #[prop(default = None)] genre: Option<String>,
    #[prop(default = None)] rating: Option<f32>,
    #[prop(default = None)] driver_status: Option<String>,
    /// Pre-resolved system display name. When provided, avoids a server-side lookup.
    #[prop(default = None)]
    system_display: Option<String>,
) -> impl IntoView {
    let system = StoredValue::new(system);
    let rom_filename = StoredValue::new(rom_filename);
    let rom_path = StoredValue::new(rom_path);
    let has_box_art = box_art_url.is_some();
    let box_art_url = StoredValue::new(box_art_url);
    let placeholder_name = StoredValue::new(display_name.clone());
    let placeholder_system = StoredValue::new(system.get_value());
    let row_label = StoredValue::new(display_name.clone());

    let game_href = StoredValue::new(format!(
        "/games/{}/{}",
        system.get_value(),
        urlencoding::encode(&rom_filename.get_value())
    ));

    // Resolve system display name for the badge.
    // All callers that set show_system=true pass system_display, so the
    // fallback to raw folder name only exists as a safety net.
    let system_label = StoredValue::new(if show_system {
        system_display.unwrap_or_else(|| system.get_value())
    } else {
        String::new()
    });

    let has_genre = genre.as_ref().is_some_and(|g| !g.is_empty());
    let genre = StoredValue::new(genre.unwrap_or_default());

    let i18n = use_i18n();

    // Favorite toggle.
    let is_fav = RwSignal::new(is_favorite);
    let on_toggle_fav = move |_| {
        let fav = is_fav.get();
        is_fav.set(!fav);
        let fname = rom_filename.get_value();
        let sys = system.get_value();
        let rp = rom_path.get_value();
        if fav {
            let fav_filename = format!("{sys}@{fname}.fav");
            leptos::task::spawn_local(async move {
                is_fav.set(
                    server_fns::remove_favorite(fav_filename, None)
                        .await
                        .is_err(),
                );
            });
        } else {
            leptos::task::spawn_local(async move {
                is_fav.set(server_fns::add_favorite(sys, rp, false).await.is_ok());
            });
        }
    };

    // Launch state + handler from the shared hook. The <button> markup stays
    // inline below (a shared child component lost taps on iOS Safari after a
    // swipe-back); only the handler logic is shared.
    let LaunchControl {
        launching,
        launch_failed,
        on_launch,
    } = use_launch_control(system, rom_filename, rom_path, row_label);

    view! {
        <div class="game-list-item">
            <A href=game_href.get_value() attr:class="game-list-row-link">
                <span class="visually-hidden">{row_label.get_value()}</span>
            </A>

            {show_favorite.then(|| view! {
                <button
                    class="rom-fav-btn"
                    class:rom-fav-active=move || is_fav.get()
                    on:click=on_toggle_fav
                >
                    {move || if is_fav.get() { "\u{2605}" } else { "\u{2606}" }}
                </button>
            })}

            <div class="rom-thumb-link">
                <div class="rom-thumb-frame">
                    {if has_box_art {
                        Either::Left(view! {
                            <img
                                class="rom-thumb"
                                src=box_art_url.get_value()
                                loading="lazy"
                                width="56"
                                height="40"
                            />
                        })
                    } else {
                        Either::Right(view! {
                            <div class="rom-thumb-placeholder">
                                <BoxArtPlaceholder system=placeholder_system.get_value() name=placeholder_name.get_value() size="list".to_string() />
                            </div>
                        })
                    }}
                </div>
            </div>

            <div class="game-list-info">
                <div class="rom-name-row">
                    <span class="rom-name">
                        {display_name}
                    </span>
                    {driver_status.as_ref().and_then(|status| {
                        // Only show the dot for non-Working statuses — "Working" is the
                        // default/expected state and showing a green dot for every working
                        // game adds noise without value.
                        let (class, title) = match status.as_str() {
                            "Working" => return None,
                            "Imperfect" => (
                                "driver-dot driver-dot-imperfect",
                                "Emulation: minor issues",
                            ),
                            "Preliminary" => (
                                "driver-dot driver-dot-preliminary",
                                "Emulation: not fully playable",
                            ),
                            _ => (
                                "driver-dot driver-dot-unknown",
                                "Emulation: unknown status",
                            ),
                        };
                        Some(view! { <span class=class title=title></span> })
                    })}
                </div>
                <div class="game-list-badges">
                    <Show when=move || show_system>
                        <span class="game-list-system">{system_label.get_value()}</span>
                    </Show>
                    <Show when=move || has_genre>
                        <span class="search-badge search-badge-genre">{genre.get_value()}</span>
                    </Show>
                    {rating.filter(|&r| r > 0.0).map(|r| {
                        let label = format!("\u{2605} {:.1}", r);
                        view! { <span class="search-badge search-badge-rating">{label}</span> }
                    })}
                </div>
            </div>

            <button
                type="button"
                class="game-action-launch game-list-launch-btn"
                class:game-list-launch-pending=move || launching.get()
                class:game-list-launch-error=move || launch_failed.get()
                aria-label=move || t(i18n.locale.get(), Key::GameDetailLaunch)
                on:click=move |_| on_launch.run(())
            >
                <span class="game-action-icon">{"\u{25B6}"}</span>
            </button>
        </div>
    }
}
