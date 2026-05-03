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
    let has_user_progress = RwSignal::new(false);

    let _sync = Effect::new(move || {
        if let Some(result) = achievements_resource.get() {
            match result {
                Ok(info) => {
                    let has_progress = info.is_some() && info.as_ref().map(|g| g.earned_count > 0).unwrap_or(false);
                    game_info.set(info);
                    loaded.set(true);
                    has_user_progress.set(has_progress);
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
    let earned_points = move || game_info.get().map(|g| g.earned_points).unwrap_or(0);
    let completion_pct = move || game_info.get().map(|g| g.completion_percentage).unwrap_or(0.0);

    view! {
        <Show when=move || loaded.get() && game_info.get().is_some()>
            <section class="section game-section">
                <h2 class="section-title">
                    {move || t(i18n.locale.get(), Key::AchievementsTitle)}
                    <span class="achievement-count">
                        {move || format!("({})", achievement_count())}
                    </span>
                    <Show when=move || has_user_progress.get()>
                        <span class="achievement-complete-badge">
                            {move || t(i18n.locale.get(), Key::AchievementsCompleted)}
                        </span>
                    </Show>
                </h2>

                <Show when=move || has_user_progress.get()>
                    <div class="achievement-progress-section">
                        <div class="achievement-progress-header">
                            <span class="achievement-progress-label">
                                {move || format!(
                                    "{} / {} ({:.0}%)",
                                    game_info.get().map(|g| g.earned_count).unwrap_or(0),
                                    achievement_count(),
                                    completion_pct()
                                )}
                            </span>
                            <span class="achievement-progress-points">
                                {move || format!(
                                    "{} / {} {}",
                                    earned_points(),
                                    total_points(),
                                    t(i18n.locale.get(), Key::AchievementsPoints)
                                )}
                            </span>
                        </div>
                        <div class="achievement-progress-bar">
                            <div
                                class="achievement-progress-fill"
                                style=move || format!("width: {}%", completion_pct())
                            ></div>
                        </div>
                    </div>
                </Show>

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
                                let unlocked = a.unlocked;
                                let unlocked_date_sv = StoredValue::new(a.unlocked_date);
                                let unlocked_hardcore = a.unlocked_hardcore;

                                let badge_class = if unlocked {
                                    "achievement-badge"
                                } else {
                                    "achievement-badge achievement-badge-locked"
                                };

                                let card_class = if unlocked {
                                    "achievement-card achievement-card-earned"
                                } else {
                                    "achievement-card"
                                };

                                view! {
                                    <div class=card_class>
                                        <div class="achievement-badge-wrapper">
                                            <img
                                                class=badge_class
                                                src=badge_url
                                                alt=title_sv.get_value()
                                                loading="lazy"
                                            />
                                            <Show when=move || unlocked_hardcore>
                                                <span class="achievement-hardcore-badge">HC</span>
                                            </Show>
                                            <Show when=move || !unlocked && type_sv.get_value().is_some()>
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
                                            <Show when=move || unlocked>
                                                <span class="achievement-unlock-date">
                                                    {move || {
                                                        unlocked_date_sv.get_value()
                                                            .map(|d| format!("✓ {d}"))
                                                            .unwrap_or_else(|| "✓".to_string())
                                                    }}
                                                </span>
                                            </Show>
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
