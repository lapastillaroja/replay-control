use leptos::prelude::*;
use leptos_router::components::A;

use crate::components::boxart_placeholder::BoxArtPlaceholder;
use crate::i18n::{Key, t, use_i18n};
use crate::server_fns;

#[component]
pub fn GameListItem(
    system: String,
    rom_filename: String,
    display_name: String,
    rom_path: String,
    box_art_url: Option<String>,
    #[prop(default = false)]
    show_system: bool,
    #[prop(default = true)] show_favorite: bool,
    #[prop(default = false)] is_favorite: bool,
    #[prop(default = None)] genre: Option<String>,
    #[prop(default = None)] rating: Option<f32>,
    #[prop(default = None)] driver_status: Option<String>,
    #[prop(default = None)]
    system_display: Option<String>,
    #[prop(default = false)] has_manual: bool,
    #[prop(default = None)] base_title: Option<String>,
) -> impl IntoView {
    let i18n = use_i18n();
    let system = StoredValue::new(system);
    let rom_filename = StoredValue::new(rom_filename);
    let rom_path = StoredValue::new(rom_path);
    let has_box_art = box_art_url.is_some();
    let box_art_url = StoredValue::new(box_art_url);
    let placeholder_name = StoredValue::new(display_name.clone());
    let placeholder_system = StoredValue::new(system.get_value());
    let game_href = StoredValue::new(format!(
        "/games/{}/{}",
        system.get_value(),
        urlencoding::encode(&rom_filename.get_value())
    ));

    let system_label = StoredValue::new(if show_system {
        system_display.unwrap_or_else(|| system.get_value())
    } else {
        String::new()
    });

    let has_genre = genre.as_ref().is_some_and(|g| !g.is_empty());
    let genre = StoredValue::new(genre.unwrap_or_default());

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

    let _base_title_sv = StoredValue::new(base_title.unwrap_or_default());
    let _display_name_sv = StoredValue::new(display_name.clone());

    let on_open_manual = move |_| {
        #[cfg(target_arch = "wasm32")]
        {
            let sys = system.get_value();
            let bt = _base_title_sv.get_value();
            let title = _display_name_sv.get_value();
            leptos::task::spawn_local(async move {
                let url = if let Ok(manuals) = server_fns::get_local_manuals(sys.clone(), bt.clone()).await
                    && let Some(manual) = manuals.into_iter().next()
                {
                    Some(manual.url)
                } else if let Ok(results) = server_fns::search_game_manuals(sys, bt, title).await
                    && let Some(rec) = results.into_iter().next()
                {
                    Some(rec.url)
                } else {
                    None
                };
                if let Some(url) = url {
                    if let Some(window) = web_sys::window() {
                        let _ = window.open_with_url_and_target(&url, "_blank");
                    }
                }
            });
        }
    };

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
                    view! {
                        <div class="rom-thumb-placeholder">
                            <BoxArtPlaceholder system=placeholder_system.get_value() name=placeholder_name.get_value() size="list".to_string() />
                        </div>
                    }.into_any()
                }}
            </A>

            <div class="game-list-info">
                <div class="rom-name-row">
                    <A href=game_href.get_value() attr:class="rom-name rom-name-link">
                        {display_name}
                    </A>
                    {driver_status.as_ref().and_then(|status| {
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
                    <Show when=move || has_manual>
                        <span
                            class="manual-badge"
                            on:click=on_open_manual
                            title={move || t(i18n.locale.get(), Key::GameDetailOpenManual)}
                        >
                            "\u{1F4C4}"
                        </span>
                    </Show>
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
