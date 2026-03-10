use leptos::prelude::*;
use leptos_router::components::A;

/// Reusable hero card — prominent game entry with box art, title, and system name.
/// Used on the home page (last played) and favorites page (latest added).
#[component]
pub fn HeroCard(
    href: String,
    name: String,
    system: String,
    box_art_url: Option<String>,
) -> impl IntoView {
    let has_art = box_art_url.is_some();

    view! {
        <A href=href attr:class="hero-card rom-name-link">
            {if has_art {
                view! { <img class="hero-thumb" src=box_art_url loading="lazy" /> }.into_any()
            } else {
                view! { <div class="hero-thumb-placeholder"></div> }.into_any()
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
    box_art_url: Option<String>,
) -> impl IntoView {
    let has_art = box_art_url.is_some();

    view! {
        <A href=href attr:class="recent-item rom-name-link">
            {if has_art {
                view! { <img class="recent-thumb" src=box_art_url loading="lazy" /> }.into_any()
            } else {
                view! { <div class="recent-thumb-placeholder"></div> }.into_any()
            }}
            <div class="recent-name">{name}</div>
            <div class="recent-system">{system}</div>
        </A>
    }
}
