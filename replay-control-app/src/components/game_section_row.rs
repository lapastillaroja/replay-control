use leptos::prelude::*;
use leptos_router::components::A;

use crate::components::hero_card::GameScrollCard;
use crate::i18n::{t, use_i18n};
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

    view! {
        <section class="section">
            <div class="section-header">
                <h2 class="section-title">{section.title}</h2>
                <Show when=move || has_see_all>
                    <A href=see_all_href.clone() attr:class="section-link">
                        {move || t(i18n.locale.get(), "common.see_all")}
                    </A>
                </Show>
            </div>
            <div class="scroll-card-row">
                {section.games.into_iter().map(|game| {
                    view! {
                        <GameScrollCard
                            href=game.href
                            name=game.display_name
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
