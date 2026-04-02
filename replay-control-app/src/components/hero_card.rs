use leptos::prelude::*;
use leptos_router::components::A;

use crate::components::boxart_placeholder::BoxArtPlaceholder;

/// Reusable hero card — prominent game entry with box art, title, and system name.
/// Used on the home page (last played) and favorites page (latest added).
#[component]
pub fn HeroCard(
    href: String,
    name: String,
    system: String,
    /// System folder name for placeholder rendering (e.g., "nintendo_snes").
    #[prop(default = String::new())]
    system_folder: String,
    box_art_url: Option<String>,
) -> impl IntoView {
    let has_art = box_art_url.is_some();
    let placeholder_name = name.clone();
    let placeholder_folder = system_folder;

    view! {
        <A href=href attr:class="hero-card rom-name-link">
            {if has_art {
                view! { <img class="hero-thumb" src=box_art_url loading="lazy" /> }.into_any()
            } else {
                view! {
                    <div class="hero-thumb-placeholder">
                        <BoxArtPlaceholder system=placeholder_folder name=placeholder_name size="hero".to_string() />
                    </div>
                }.into_any()
            }}
            <div class="hero-info">
                <h3 class="hero-title">{name}</h3>
                <p class="hero-system">{system}</p>
            </div>
        </A>
    }
}

/// Reusable card for horizontal game scroll strips.
/// Used on the home page (recently played) and favorites page (recently added).
#[component]
pub fn GameScrollCard(
    href: String,
    name: String,
    system: String,
    /// System folder name for placeholder rendering (e.g., "nintendo_snes").
    #[prop(default = String::new())]
    system_folder: String,
    box_art_url: Option<String>,
) -> impl IntoView {
    let has_art = box_art_url.is_some();
    let placeholder_name = name.clone();
    let placeholder_folder = system_folder;

    view! {
        <A href=href attr:class="scroll-card-item rom-name-link">
            {if has_art {
                view! { <img class="scroll-card-thumb" src=box_art_url loading="lazy" /> }.into_any()
            } else {
                view! {
                    <div class="scroll-card-thumb-placeholder">
                        <BoxArtPlaceholder system=placeholder_folder name=placeholder_name size="card".to_string() />
                    </div>
                }.into_any()
            }}
            <div class="scroll-card-name">{name}</div>
            <div class="scroll-card-system">{system}</div>
        </A>
    }
}
