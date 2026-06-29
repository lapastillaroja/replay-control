use leptos::prelude::*;

use crate::i18n::{Key, t, use_i18n};
use crate::server_fns;

/// Format bytes to human-readable string.
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Stat card showing a single metric.
#[component]
fn StatCard(label: String, value: String, icon: &'static str) -> impl IntoView {
    view! {
        <div class="stat-card">
            <div class="stat-icon">{icon}</div>
            <div class="stat-value">{value}</div>
            <div class="stat-label">{label}</div>
        </div>
    }
}

/// Horizontal bar chart row.
#[component]
fn BarRow(label: String, count: usize, max: usize, suffix: Option<String>) -> impl IntoView {
    let pct = if max > 0 {
        (count as f64 / max as f64) * 100.0
    } else {
        0.0
    };
    let width = format!("{:.1}%", pct);
    let suffix = suffix.unwrap_or_default();

    view! {
        <div class="bar-row">
            <div class="bar-label">
                <span class="bar-name">{label}</span>
                <span class="bar-count">{count} {suffix}</span>
            </div>
            <div class="bar-track">
                <div class="bar-fill" style:width=width></div>
            </div>
        </div>
    }
}

/// Donut-style segment bar (horizontal multi-color).
#[component]
fn SegmentBar(segments: Vec<(String, usize, &'static str)>) -> impl IntoView {
    let total: usize = segments.iter().map(|(_, count, _)| count).sum();
    let segments_with_pct: Vec<_> = segments
        .into_iter()
        .filter(|(_, count, _)| *count > 0)
        .map(|(label, count, color)| {
            let pct = if total > 0 {
                (count as f64 / total as f64) * 100.0
            } else {
                0.0
            };
            (label, count, color, pct)
        })
        .collect();

    view! {
        <div class="segment-bar-container">
            <div class="segment-bar">
                {segments_with_pct.iter().map(|(_, _, color, pct)| {
                    let width = format!("{:.1}%", pct);
                    view! { <div class="segment-fill" style:width=width style=("--segment-color", *color)></div> }
                }).collect::<Vec<_>>()}
            </div>
            <div class="segment-legend">
                {segments_with_pct.iter().map(|(label, count, color, pct)| {
                    let label = label.clone();
                    let count = *count;
                    let color = *color;
                    let pct_val = *pct;
                    view! {
                        <div class="legend-item">
                            <span class="legend-dot" style:background=color></span>
                            <span class="legend-label">{label}</span>
                            <span class="legend-value">{count} {move || format!("{:.0}%", pct_val)}</span>
                        </div>
                    }
                }).collect::<Vec<_>>()}
            </div>
        </div>
    }
}

/// Coverage progress bar.
#[component]
fn CoverageRow(label: String, count: usize, percentage: f64) -> impl IntoView {
    let width = format!("{:.1}%", percentage);
    let pct_str = format!("{:.1}%", percentage);

    view! {
        <div class="coverage-row">
            <div class="coverage-label">
                <span>{label}</span>
                <span class="coverage-count">{count}</span>
            </div>
            <div class="coverage-track">
                <div class="coverage-fill" style:width=width></div>
            </div>
            <span class="coverage-pct">{pct_str}</span>
        </div>
    }
}

/// Stats dashboard page showing library overview with charts.
#[component]
pub fn StatsDashboardPage() -> impl IntoView {
    let i18n = use_i18n();

    let stats = Resource::new(|| (), |_| server_fns::get_stats_dashboard());

    view! {
        <div class="stats-page">
            <h1 class="stats-title">{move || t(i18n.locale.get(), Key::StatsTitle)}</h1>
            <Suspense fallback=move || view! { <div class="stats-loading">{move || t(i18n.locale.get(), Key::StatsLoading)}</div> }>
                {move || Suspend::new(async move {
                    let dashboard = stats.await?;

                    let max_system_count = dashboard.systems.iter().map(|s| s.game_count).max().unwrap_or(1);
                    let max_genre_count = dashboard.genres.iter().map(|g| g.count).max().unwrap_or(1);
                    let max_dev_count = dashboard.developers.iter().map(|d| d.count).max().unwrap_or(1);
                    let max_decade_count = dashboard.decades.iter().map(|d| d.count).max().unwrap_or(1);

                    let arcade_count = dashboard.summary.arcade_count;
                    let console_count = dashboard.summary.total_games.saturating_sub(arcade_count);

                    let locale = i18n.locale.get();

                    Ok::<_, server_fn::ServerFnError>(view! {
                        <div class="stats-content">
                            <div class="stat-cards-grid">
                                <StatCard
                                    label=t(locale, Key::StatsTotalGames).to_string()
                                    value=dashboard.summary.total_games.to_string()
                                    icon="\u{1F3AE}"
                                />
                                <StatCard
                                    label=t(locale, Key::StatsTotalSystems).to_string()
                                    value=dashboard.summary.total_systems.to_string()
                                    icon="\u{1F4BE}"
                                />
                                <StatCard
                                    label=t(locale, Key::StatsTotalSize).to_string()
                                    value=format_bytes(dashboard.summary.total_size_bytes)
                                    icon="\u{1F4C1}"
                                />
                                <StatCard
                                    label=t(locale, Key::StatsFavorites).to_string()
                                    value=dashboard.summary.total_favorites.to_string()
                                    icon="\u{2B50}"
                                />
                            </div>

                            <section class="stats-section">
                                <h2 class="stats-section-title">{t(locale, Key::StatsPlayerModes)}</h2>
                                <SegmentBar segments=vec![
                                    (t(locale, Key::StatsSinglePlayer).to_string(), dashboard.player_modes.single_player, "#4a90d9"),
                                    (t(locale, Key::StatsCooperative).to_string(), dashboard.player_modes.cooperative, "#50c878"),
                                    (t(locale, Key::StatsMultiplayer).to_string(), dashboard.player_modes.multiplayer, "#f39c12"),
                                    (t(locale, Key::StatsUnknown).to_string(), dashboard.player_modes.unknown, "#95a5a6"),
                                ] />
                            </section>

                            <Show when=move || { arcade_count > 0 }>
                                <section class="stats-section">
                                    <h2 class="stats-section-title">{t(locale, Key::StatsArcadeConsole)}</h2>
                                    <SegmentBar segments=vec![
                                        (t(locale, Key::StatsConsole).to_string(), console_count, "#4a90d9"),
                                        (t(locale, Key::StatsArcade).to_string(), arcade_count, "#e74c3c"),
                                    ] />
                                </section>
                            </Show>

                            <section class="stats-section">
                                <h2 class="stats-section-title">{t(locale, Key::StatsBySystem)}</h2>
                                {dashboard.systems.iter().map(|sys| {
                                    let name = sys.display_name.clone();
                                    let count = sys.game_count;
                                    let max = max_system_count;
                                    view! {
                                        <BarRow label=name count=count max=max suffix=None />
                                    }
                                }).collect::<Vec<_>>()}
                            </section>

                            <section class="stats-section">
                                <h2 class="stats-section-title">{t(locale, Key::StatsByGenre)}</h2>
                                {dashboard.genres.iter().map(|genre| {
                                    let name = genre.genre.clone();
                                    let count = genre.count;
                                    let max = max_genre_count;
                                    view! {
                                        <BarRow label=name count=count max=max suffix=None />
                                    }
                                }).collect::<Vec<_>>()}
                            </section>

                            <section class="stats-section">
                                <h2 class="stats-section-title">{t(locale, Key::StatsByDecade)}</h2>
                                {dashboard.decades.iter().map(|decade| {
                                    let label = format!("{}s", decade.decade);
                                    let count = decade.count;
                                    let max = max_decade_count;
                                    view! {
                                        <BarRow label=label count=count max=max suffix=None />
                                    }
                                }).collect::<Vec<_>>()}
                            </section>

                            <section class="stats-section">
                                <h2 class="stats-section-title">{t(locale, Key::StatsByDeveloper)}</h2>
                                {dashboard.developers.iter().map(|dev| {
                                    let name = dev.developer.clone();
                                    let count = dev.count;
                                    let max = max_dev_count;
                                    view! {
                                        <BarRow label=name count=count max=max suffix=None />
                                    }
                                }).collect::<Vec<_>>()}
                            </section>

                            <section class="stats-section">
                                <h2 class="stats-section-title">{t(locale, Key::StatsVariants)}</h2>
                                <SegmentBar segments=vec![
                                    (t(locale, Key::StatsVerified).to_string(), dashboard.variants.verified, "#27ae60"),
                                    (t(locale, Key::StatsClones).to_string(), dashboard.variants.clones, "#3498db"),
                                    (t(locale, Key::StatsHacks).to_string(), dashboard.variants.hacks, "#e67e22"),
                                    (t(locale, Key::StatsTranslations).to_string(), dashboard.variants.translations, "#9b59b6"),
                                    (t(locale, Key::StatsSpecial).to_string(), dashboard.variants.special, "#95a5a6"),
                                ] />
                            </section>

                            <section class="stats-section">
                                <h2 class="stats-section-title">{t(locale, Key::StatsMetadataCoverage)}</h2>
                                <CoverageRow
                                    label=t(locale, Key::StatsGenreCoverage).to_string()
                                    count=dashboard.metadata_coverage.with_genre
                                    percentage=dashboard.metadata_coverage.genre_pct
                                />
                                <CoverageRow
                                    label=t(locale, Key::StatsDeveloperCoverage).to_string()
                                    count=dashboard.metadata_coverage.with_developer
                                    percentage=dashboard.metadata_coverage.developer_pct
                                />
                                <CoverageRow
                                    label=t(locale, Key::StatsRatingCoverage).to_string()
                                    count=dashboard.metadata_coverage.with_rating
                                    percentage=dashboard.metadata_coverage.rating_pct
                                />
                                <CoverageRow
                                    label=t(locale, Key::StatsBoxartCoverage).to_string()
                                    count=dashboard.metadata_coverage.with_boxart
                                    percentage=dashboard.metadata_coverage.boxart_pct
                                />
                                <CoverageRow
                                    label=t(locale, Key::StatsScreenshotCoverage).to_string()
                                    count=dashboard.metadata_coverage.with_screenshot
                                    percentage=dashboard.metadata_coverage.screenshot_pct
                                />
                            </section>
                        </div>
                    })
                })}
            </Suspense>
        </div>
    }
}
