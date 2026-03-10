use leptos::prelude::*;
use leptos_router::components::A;
use server_fn::ServerFnError;

use crate::i18n::{t, use_i18n};
use crate::pages::ErrorDisplay;
use crate::server_fns;
use crate::util::format_size;

#[component]
pub fn HomePage() -> impl IntoView {
    let i18n = use_i18n();
    let info = Resource::new(|| (), |_| server_fns::get_info());
    let recents = Resource::new(|| (), |_| server_fns::get_recents());
    let systems = Resource::new(|| (), |_| server_fns::get_systems());

    view! {
        <div class="page home-page">
            <section class="section">
                <h2 class="section-title">{move || t(i18n.locale.get(), "home.last_played")}</h2>
                <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }>
                    <Suspense fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), "common.loading")}</div> }>
                        {move || Suspend::new(async move {
                            let locale = i18n.locale.get();
                            let entries = recents.await?;
                            Ok::<_, ServerFnError>(if let Some(last) = entries.first() {
                                let name = last.game.display_name.clone().unwrap_or_else(|| last.game.rom_filename.clone());
                                let sys = last.game.system_display.clone();
                                let href = format!("/games/{}/{}", last.game.system, urlencoding::encode(&last.game.rom_filename));
                                view! {
                                    <A href=href attr:class="hero-card rom-name-link">
                                        <div class="hero-info">
                                            <h3 class="hero-title">{name}</h3>
                                            <p class="hero-system">{sys}</p>
                                        </div>
                                    </A>
                                }.into_any()
                            } else {
                                view! { <p class="empty-state">{t(locale, "home.no_games_played")}</p> }.into_any()
                            })
                        })}
                    </Suspense>
                </ErrorBoundary>
            </section>

            <section class="section">
                <h2 class="section-title">{move || t(i18n.locale.get(), "home.recently_played")}</h2>
                <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }>
                    <Suspense fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), "common.loading")}</div> }>
                        {move || Suspend::new(async move {
                            let locale = i18n.locale.get();
                            let entries = recents.await?;
                            let items: Vec<_> = entries.iter().skip(1).take(10).cloned().collect();
                            Ok::<_, ServerFnError>(if items.is_empty() {
                                view! { <p class="empty-state">{t(locale, "home.no_recent")}</p> }.into_any()
                            } else {
                                view! {
                                    <div class="recent-scroll">
                                        {items.into_iter().map(|entry| {
                                            let name = entry.game.display_name.clone().unwrap_or_else(|| entry.game.rom_filename.clone());
                                            let href = format!("/games/{}/{}", entry.game.system, urlencoding::encode(&entry.game.rom_filename));
                                            view! {
                                                <A href=href attr:class="recent-item rom-name-link">
                                                    <div class="recent-name">{name}</div>
                                                    <div class="recent-system">{entry.game.system_display.clone()}</div>
                                                </A>
                                            }
                                        }).collect::<Vec<_>>()}
                                    </div>
                                }.into_any()
                            })
                        })}
                    </Suspense>
                </ErrorBoundary>
            </section>

            <section class="section">
                <h2 class="section-title">{move || t(i18n.locale.get(), "home.library")}</h2>
                <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }>
                    <Suspense fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), "common.loading")}</div> }>
                        {move || Suspend::new(async move {
                            let locale = i18n.locale.get();
                            let info = info.await?;
                            Ok::<_, ServerFnError>(view! {
                                <div class="stats-grid">
                                    <StatCard value=info.total_games.to_string() label=t(locale, "stats.games") />
                                    <StatCard value=info.systems_with_games.to_string() label=t(locale, "stats.systems") />
                                    <StatCard value=info.total_favorites.to_string() label=t(locale, "stats.favorites") />
                                    <StatCard value=format_size(info.disk_used_bytes) label=t(locale, "stats.used") />
                                    <StatCard value=info.storage_kind.to_uppercase() label=t(locale, "stats.storage") />
                                </div>
                            })
                        })}
                    </Suspense>
                </ErrorBoundary>
            </section>

            <section class="section">
                <h2 class="section-title">{move || t(i18n.locale.get(), "home.systems")}</h2>
                <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }>
                    <Suspense fallback=move || view! { <div class="loading">{move || t(i18n.locale.get(), "common.loading")}</div> }>
                        {move || Suspend::new(async move {
                            let locale = i18n.locale.get();
                            let systems = systems.await?;
                            let with_games: Vec<_> = systems.into_iter().filter(|s| s.game_count > 0).collect();
                            Ok::<_, ServerFnError>(if with_games.is_empty() {
                                view! { <p class="empty-state">{t(locale, "home.no_systems")}</p> }.into_any()
                            } else {
                                view! {
                                    <div class="systems-grid">
                                        {with_games.into_iter().map(|sys| {
                                            let href = format!("/games/{}", sys.folder_name);
                                            let name = sys.display_name.clone();
                                            let count = format!("{} {}", sys.game_count, t(locale, "stats.games").to_lowercase());
                                            view! {
                                                <A href=href attr:class="system-card">
                                                    <div class="system-card-name">{name}</div>
                                                    <div class="system-card-count">{count}</div>
                                                </A>
                                            }
                                        }).collect::<Vec<_>>()}
                                    </div>
                                }.into_any()
                            })
                        })}
                    </Suspense>
                </ErrorBoundary>
            </section>
        </div>
    }
}

#[component]
fn StatCard(value: String, label: &'static str) -> impl IntoView {
    view! {
        <div class="stat-card">
            <div class="stat-value">{value}</div>
            <div class="stat-label">{label}</div>
        </div>
    }
}
