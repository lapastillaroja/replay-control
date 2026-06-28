use leptos::prelude::*;
use leptos_router::components::A;
use replay_control_core::systems::system_abbreviation;

use crate::components::hero_card::GameScrollCard;
use crate::hooks::use_scroll_memory;
use crate::i18n::{Key, key_from_str, t, tf, use_i18n};
use crate::server_fns::GameSection;

/// A horizontal scroll-card section: title + optional "See all" link + game cards.
///
/// Shared by the home page (favorites picks, curated spotlight) and favorites
/// page (personalized recommendations).
#[component]
pub fn GameSectionRow(section: GameSection) -> impl IntoView {
    let i18n = use_i18n();
    let has_see_all = section.see_all_href.is_some();
    let see_all_href = section.see_all_href.unwrap_or_default();
    let title_key = section.title_key.clone();
    let title_args = section.title_args.clone();

    // Preserve the row's horizontal scroll across Back navigation: the row
    // re-mounts with fresh DOM (scrollLeft 0). Key on route + section identity
    // (stable across navigations, unique per row), and tag the saved offset with
    // a signature of the cards so a regenerated row (the recommendations snapshot
    // is short-lived) starts at 0 rather than restoring a stale position.
    let row_ref = NodeRef::<leptos::html::Div>::new();
    use_scroll_memory(row_ref, || {
        let path = leptos_router::hooks::use_location()
            .pathname
            .get_untracked();
        let key = format!(
            "{path}|{}|{}",
            section.title_key,
            section.title_args.join(",")
        );
        let sig = section
            .games
            .iter()
            .map(|g| g.href.as_str())
            .collect::<Vec<_>>()
            .join("|");
        (key, sig)
    });

    let title = move || {
        let locale = i18n.locale.get();
        if let Some(key) = key_from_str(&title_key) {
            let args: Vec<&str> = title_args.iter().map(|s| s.as_str()).collect();
            tf(locale, key, &args)
        } else {
            title_key.clone()
        }
    };

    view! {
        <section class="section">
            <div class="section-header">
                <h2 class="section-title">{title}</h2>
                <Show when=move || has_see_all>
                    <A href=see_all_href.clone() attr:class="section-link">
                        {move || t(i18n.locale.get(), Key::CommonSeeAll)}
                    </A>
                </Show>
            </div>
            <div class="scroll-card-row" node_ref=row_ref>
                {section.games.into_iter().map(|game| {
                    let system = system_abbreviation(&game.system);
                    view! {
                        <GameScrollCard
                            href=game.href
                            name=game.display_name
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
