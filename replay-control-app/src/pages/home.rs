use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_navigate;
use server_fn::ServerFnError;

use crate::components::system_card::SystemCard;
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

    let home_search = RwSignal::new(String::new());

    let on_search_submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let q = home_search.get_untracked();
        if !q.is_empty() {
            let navigate = use_navigate();
            navigate(
                &format!("/search?q={}", urlencoding::encode(&q)),
                Default::default(),
            );
        }
    };

    let on_search_keydown = move |ev: leptos::ev::KeyboardEvent| {
        if ev.key() == "Enter" {
            let q = home_search.get_untracked();
            if !q.is_empty() {
                let navigate = use_navigate();
                navigate(
                    &format!("/search?q={}", urlencoding::encode(&q)),
                    Default::default(),
                );
            }
        }
    };

    view! {
        <div class="page home-page">
            <section class="section home-search-section">
                <form class="home-search-form" on:submit=on_search_submit>
                    <input
                        type="text"
                        class="home-search-input"
                        placeholder=move || t(i18n.locale.get(), "search.placeholder")
                        bind:value=home_search
                        on:keydown=on_search_keydown
                    />
                </form>
            </section>

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
                            let storage_value = {
                                let used = format_size(info.disk_used_bytes);
                                let total = format_size(info.disk_total_bytes);
                                let kind = info.storage_kind.to_uppercase();
                                format!("{used} / {total} {kind}")
                            };
                            Ok::<_, ServerFnError>(view! {
                                <div class="stats-grid">
                                    <StatCard value=info.total_games.to_string() label=t(locale, "stats.games") />
                                    <StatCard value=info.systems_with_games.to_string() label=t(locale, "stats.systems") />
                                    <StatCard value=info.total_favorites.to_string() label=t(locale, "stats.favorites") />
                                    <StatCard value=storage_value label=t(locale, "stats.storage") compact=true />
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
                    </Suspense>
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
