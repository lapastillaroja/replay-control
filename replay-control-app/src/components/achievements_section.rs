use leptos::prelude::*;

use crate::i18n::{Key, t, use_i18n};
use crate::server_fns::{self, RaGameInfo};

const INITIAL_ACHIEVEMENT_COUNT: usize = 6;

#[component]
pub fn AchievementsSection(
    system: StoredValue<String>,
    rom_filename: StoredValue<String>,
    display_name: StoredValue<String>,
) -> impl IntoView {
    let i18n = use_i18n();

    let achievements_resource = Resource::new(
        || (),
        move |_| {
            let sys = system.get_value();
            let fname = rom_filename.get_value();
            let title = display_name.get_value();
            server_fns::get_game_achievements(sys, fname, title)
        },
    );

    let game_info = RwSignal::new(Option::<RaGameInfo>::None);
    let loaded = RwSignal::new(false);
    let show_all = RwSignal::new(false);

    let _sync = Effect::new(move || {
        if let Some(result) = achievements_resource.get() {
            match result {
                Ok(info) => {
                    game_info.set(info);
                    loaded.set(true);
                }
                Err(_) => {
                    loaded.set(true);
                }
            }
        }
    });

    let achievement_count = move || {
        game_info
            .get()
            .map(|g| g.achievements.len())
            .unwrap_or(0)
    };

    let total_points = move || game_info.get().map(|g| g.total_points).unwrap_or(0);

    view! {
        <Show when=move || loaded.get() && game_info.get().is_some()>
            <section class="section game-section">
                <h2 class="section-title">
                    {move || t(i18n.locale.get(), Key::AchievementsTitle)}
                    <span class="achievement-count">
                        {move || format!("({})", achievement_count())}
                    </span>
                    <span class="achievement-points">
                        {move || format!("{} {}", total_points(), t(i18n.locale.get(), Key::AchievementsPoints))}
                    </span>
                </h2>

                <div class="achievements-grid">
                    {move || {
                        let info = game_info.get();
                        let achievements = match info {
                            Some(g) => {
                                if show_all.get() || g.achievements.len() <= INITIAL_ACHIEVEMENT_COUNT {
                                    g.achievements.clone()
                                } else {
                                    g.achievements[..INITIAL_ACHIEVEMENT_COUNT].to_vec()
                                }
                            }
                            None => Vec::new(),
                        };

                        achievements
                            .into_iter()
                            .map(|a| {
                                let badge_url = a.badge_url.clone();
                                let title_sv = StoredValue::new(a.title);
                                let description_sv = StoredValue::new(a.description);
                                let points = a.points;
                                let type_sv = StoredValue::new(a.r#type);

                                view! {
                                    <div class="achievement-card">
                                        <div class="achievement-badge-wrapper">
                                            <img
                                                class="achievement-badge"
                                                src=badge_url
                                                alt=title_sv.get_value()
                                                loading="lazy"
                                            />
                                            <Show when=move || type_sv.get_value().is_some()>
                                                <span class="achievement-type-badge">
                                                    {move || {
                                                        type_sv.get_value()
                                                            .and_then(|t| t.chars().next())
                                                            .map(|c| c.to_uppercase().to_string())
                                                            .unwrap_or_default()
                                                    }}
                                                </span>
                                            </Show>
                                        </div>
                                        <div class="achievement-info">
                                            <div class="achievement-header">
                                                <h3 class="achievement-title">{title_sv.get_value()}</h3>
                                                <span class="achievement-points-badge">{points}</span>
                                            </div>
                                            <p class="achievement-description">{description_sv.get_value()}</p>
                                        </div>
                                    </div>
                                }
                            })
                            .collect::<Vec<_>>()
                    }}
                </div>

                <Show when=move || {
                    achievement_count() > INITIAL_ACHIEVEMENT_COUNT && !show_all.get()
                }>
                    <button
                        class="game-action-btn achievements-show-all"
                        on:click=move |_| show_all.set(true)
                    >
                        {move || t(i18n.locale.get(), Key::AchievementsShowAll)}
                    </button>
                </Show>
            </section>
        </Show>
    }
}
