use leptos::prelude::*;
use leptos_router::components::A;

use crate::server_fns;

/// Unified game list row component used across search results, developer pages,
/// system ROM lists, and other game lists. Shows box art, game name, optional
/// system badge, genre/rating badges, and a favorite toggle.
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
    #[prop(default = true)]
    show_favorite: bool,
    #[prop(default = false)]
    is_favorite: bool,
    #[prop(default = None)]
    genre: Option<String>,
    #[prop(default = None)]
    rating: Option<f32>,
    #[prop(default = None)]
    driver_status: Option<String>,
    /// Pre-resolved system display name. When provided, avoids a server-side lookup.
    #[prop(default = None)]
    system_display: Option<String>,
) -> impl IntoView {
    let system = StoredValue::new(system);
    let rom_filename = StoredValue::new(rom_filename);
    let rom_path = StoredValue::new(rom_path);
    let has_box_art = box_art_url.is_some();
    let box_art_url = StoredValue::new(box_art_url);

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
        <div class="game-list-item">
            {show_favorite.then(|| view! {
                <button class="rom-fav-btn" on:click=on_toggle_fav>{star}</button>
            })}

            <A href=game_href.get_value() attr:class="rom-thumb-link">
                {if has_box_art {
                    view! {
                        <img
                            class="rom-thumb"
                            src=box_art_url.get_value()
                            loading="lazy"
                            width="56"
                            height="40"
                        />
                    }.into_any()
                } else {
                    view! { <div class="rom-thumb-placeholder"></div> }.into_any()
                }}
            </A>

            <div class="game-list-info">
                <div class="rom-name-row">
                    <A href=game_href.get_value() attr:class="rom-name rom-name-link">
                        {display_name}
                    </A>
                    {driver_status.as_ref().map(|status| {
                        let class = match status.as_str() {
                            "Working" => "driver-dot driver-dot-working",
                            "Imperfect" => "driver-dot driver-dot-imperfect",
                            "Preliminary" => "driver-dot driver-dot-preliminary",
                            _ => "driver-dot driver-dot-unknown",
                        };
                        let title = format!("Driver: {status}");
                        view! { <span class=class title=title></span> }
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
        </div>
    }
}
