use leptos::prelude::*;
use leptos_router::components::A;
use server_fn::ServerFnError;

use crate::components::hero_card::{GameScrollCard, HeroCard};
use crate::components::system_card::SystemCard;
use crate::i18n::{t, use_i18n};
use crate::pages::ErrorDisplay;
use crate::server_fns;
use crate::util::format_size_short;

#[component]
pub fn HomePage() -> impl IntoView {
    let i18n = use_i18n();
    let info = Resource::new(|| (), |_| server_fns::get_info());
    let recents = Resource::new(|| (), |_| server_fns::get_recents());
    let systems = Resource::new(|| (), |_| server_fns::get_systems());
    let (reco_version, set_reco_version) = signal(0u32);
    let recommendations = Resource::new(
        move || reco_version.get(),
        |_| server_fns::get_recommendations(6),
    );

    let on_refresh = move |_| {
        set_reco_version.update(|v| *v += 1);
    };

    view! {
        <div class="page home-page">
            <section class="section home-search-section">
                <A href="/search" attr:class="home-search-link">
                    <span class="home-search-placeholder">
                        {move || t(i18n.locale.get(), "search.placeholder")}
                    </span>
                </A>
            </section>

            <section class="section">
                <h2 class="section-title">{move || t(i18n.locale.get(), "home.last_played")}</h2>
                <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }>
                    <Transition fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), "common.loading")}</div> }>
                        {move || Suspend::new(async move {
                            let locale = i18n.locale.get();
                            let entries = recents.await?;
                            Ok::<_, ServerFnError>(if let Some(last) = entries.first() {
                                let name = last.entry.game.display_name.clone().unwrap_or_else(|| last.entry.game.rom_filename.clone());
                                let sys = last.entry.game.system_display.clone();
                                let href = format!("/games/{}/{}", last.entry.game.system, urlencoding::encode(&last.entry.game.rom_filename));
                                let art_url = last.box_art_url.clone();
                                view! {
                                    <HeroCard href name system=sys box_art_url=art_url />
                                }.into_any()
                            } else {
                                view! { <p class="empty-state">{t(locale, "home.no_games_played")}</p> }.into_any()
                            })
                        })}
                    </Transition>
                </ErrorBoundary>
            </section>

            <section class="section">
                <h2 class="section-title">{move || t(i18n.locale.get(), "home.recently_played")}</h2>
                <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }>
                    <Transition fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), "common.loading")}</div> }>
                        {move || Suspend::new(async move {
                            let locale = i18n.locale.get();
                            let entries = recents.await?;
                            let items: Vec<_> = entries.iter().skip(1).take(10).cloned().collect();
                            Ok::<_, ServerFnError>(if items.is_empty() {
                                view! { <p class="empty-state">{t(locale, "home.no_recent")}</p> }.into_any()
                            } else {
                                view! {
                                    <div class="recent-scroll">
                                        {items.into_iter().map(|item| {
                                            let name = item.entry.game.display_name.clone().unwrap_or_else(|| item.entry.game.rom_filename.clone());
                                            let href = format!("/games/{}/{}", item.entry.game.system, urlencoding::encode(&item.entry.game.rom_filename));
                                            let art_url = item.box_art_url.clone();
                                            let system = item.entry.game.system_display.clone();
                                            view! {
                                                <GameScrollCard href name system box_art_url=art_url />
                                            }
                                        }).collect::<Vec<_>>()}
                                    </div>
                                }.into_any()
                            })
                        })}
                    </Transition>
                </ErrorBoundary>
            </section>

            // Recommendations: random picks + discover quick links
            <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }>
                <Transition fallback=|| ()>
                    {move || Suspend::new(async move {
                        let locale = i18n.locale.get();
                        let data = recommendations.await?;
                        let has_picks = !data.random_picks.is_empty();
                        let has_discover = !data.top_genres.is_empty() || data.multiplayer_count > 0;
                        let has_multiplayer = data.multiplayer_count > 0;
                        Ok::<_, ServerFnError>(view! {
                            {if has_picks {
                                let picks_view: Vec<_> = data.random_picks.iter().map(|game| {
                                    let href = game.href.clone();
                                    let name = game.display_name.clone();
                                    let system = game.system_display.clone();
                                    let box_art_url = game.box_art_url.clone();
                                    view! {
                                        <GameScrollCard href name system box_art_url />
                                    }
                                }).collect();
                                view! {
                                    <section class="section">
                                        <div class="section-header">
                                            <h2 class="section-title">{t(locale, "home.discover_random")}</h2>
                                            <button class="refresh-btn" on:click=on_refresh title="Refresh">
                                                <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none"
                                                     stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                                                    <path d="M21.5 2v6h-6M2.5 22v-6h6M2 11.5a10 10 0 0 1 18.8-4.3M22 12.5a10 10 0 0 1-18.8 4.2"/>
                                                </svg>
                                            </button>
                                        </div>
                                        <div class="recent-scroll">
                                            {picks_view}
                                        </div>
                                    </section>
                                }.into_any()
                            } else {
                                view! { <div></div> }.into_any()
                            }}

                            {if has_discover {
                                let multiplayer_card = if has_multiplayer {
                                    Some(view! {
                                        <DiscoverCard
                                            href=format!("/search?multiplayer=true")
                                            label=t(locale, "home.discover_multiplayer").to_string()
                                            count=data.multiplayer_count
                                        />
                                    })
                                } else {
                                    None
                                };
                                let genre_cards: Vec<_> = data.top_genres.iter().map(|gc| {
                                    let href = format!("/search?genre={}", urlencoding::encode(&gc.genre));
                                    view! {
                                        <DiscoverCard
                                            href
                                            label=gc.genre.clone()
                                            count=gc.count
                                        />
                                    }
                                }).collect();
                                view! {
                                    <section class="section">
                                        <h2 class="section-title">{t(locale, "home.discover")}</h2>
                                        <div class="discover-grid">
                                            {multiplayer_card}
                                            {genre_cards}
                                        </div>
                                    </section>
                                }.into_any()
                            } else {
                                view! { <div></div> }.into_any()
                            }}
                        })
                    })}
                </Transition>
            </ErrorBoundary>

            <section class="section">
                <h2 class="section-title">{move || t(i18n.locale.get(), "home.library")}</h2>
                <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }>
                    <Transition fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), "common.loading")}</div> }>
                        {move || Suspend::new(async move {
                            let locale = i18n.locale.get();
                            let info = info.await?;
                            let storage_pct = if info.disk_total_bytes > 0 {
                                ((info.disk_used_bytes as f64 / info.disk_total_bytes as f64) * 100.0).round() as u8
                            } else {
                                0
                            };
                            let storage_label = {
                                let kind = info.storage_kind.to_uppercase();
                                let (used_num, used_unit) = format_size_short(info.disk_used_bytes);
                                let (total_num, total_unit) = format_size_short(info.disk_total_bytes);
                                if used_unit == total_unit {
                                    format!("{used_num} / {total_num} {total_unit} {kind}")
                                } else {
                                    format!("{used_num} {used_unit} / {total_num} {total_unit} {kind}")
                                }
                            };
                            Ok::<_, ServerFnError>(view! {
                                <div class="stats-grid">
                                    <StatCard value=info.total_games.to_string() label=t(locale, "stats.games") />
                                    <StatCard value=info.systems_with_games.to_string() label=t(locale, "stats.systems") />
                                    <StatCard value=info.total_favorites.to_string() label=t(locale, "stats.favorites") />
                                    <StorageBarCard pct=storage_pct detail=storage_label />
                                </div>
                            })
                        })}
                    </Transition>
                </ErrorBoundary>
            </section>

            <section class="section">
                <h2 class="section-title">{move || t(i18n.locale.get(), "home.systems")}</h2>
                <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }>
                    <Transition fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), "common.loading")}</div> }>
                        {move || Suspend::new(async move {
                            let systems = systems.await?;
                            Ok::<_, ServerFnError>(view! {
                                <div class="systems-grid">
                                    {systems.iter().map(|sys| {
                                        let href = format!("/games/{}", sys.folder_name);
                                        if sys.game_count > 0 {
                                            view! { <SystemCard system=sys.clone() href /> }.into_any()
                                        } else {
                                            view! { <EmptySystemCard system=sys.clone() /> }.into_any()
                                        }
                                    }).collect::<Vec<_>>()}
                                </div>
                            })
                        })}
                    </Transition>
                </ErrorBoundary>
            </section>
        </div>
    }
}

#[component]
fn StatCard(
    value: String,
    label: &'static str,
    #[prop(optional)] compact: bool,
) -> impl IntoView {
    let class = if compact { "stat-card compact" } else { "stat-card" };
    view! {
        <div class=class>
            <div class="stat-value">{value}</div>
            <div class="stat-label">{label}</div>
        </div>
    }
}

#[component]
fn StorageBarCard(pct: u8, detail: String) -> impl IntoView {
    let width = format!("width:{}%", pct);
    view! {
        <div class="stat-card">
            <div class="storage-bar">
                <div class="storage-bar-fill" style=width></div>
            </div>
            <div class="stat-value">{format!("{}%", pct)}</div>
            <div class="stat-label">{detail}</div>
        </div>
    }
}

/// An inert, greyed-out system card for systems with no games.
/// Not clickable — just a plain div with the `.empty` class.
#[component]
fn EmptySystemCard(system: crate::server_fns::SystemSummary) -> impl IntoView {
    let i18n = use_i18n();

    view! {
        <div class="system-card empty">
            <div class="system-card-name">{system.display_name.clone()}</div>
            <div class="system-card-manufacturer">{system.manufacturer.clone()}</div>
            <div class="system-card-count">
                {move || t(i18n.locale.get(), "games.no_games").to_string()}
            </div>
        </div>
    }
}

/// A quick-discover card that links to a pre-filtered search.
#[component]
fn DiscoverCard(href: String, label: String, count: usize) -> impl IntoView {
    let i18n = use_i18n();
    let count_label = move || {
        let games = t(i18n.locale.get(), "home.discover_games");
        format!("{count} {games}")
    };

    view! {
        <A href=href attr:class="discover-card">
            <span class="discover-card-label">{label}</span>
            <span class="discover-card-count">{count_label}</span>
        </A>
    }
}
