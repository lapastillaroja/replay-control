use leptos::prelude::*;
use leptos_router::components::A;

use crate::components::boxart_placeholder::BoxArtPlaceholder;
use crate::i18n::{Key, t, use_i18n};
use crate::server_fns::{self, GameStatus, StatusGameEntry};

#[derive(Clone, Copy, PartialEq, Eq)]
enum StatusTab {
    All,
    WantToPlay,
    InProgress,
    Completed,
    Platinum,
}

impl StatusTab {
    fn to_status(&self) -> Option<GameStatus> {
        match self {
            StatusTab::All => None,
            StatusTab::WantToPlay => Some(GameStatus::WantToPlay),
            StatusTab::InProgress => Some(GameStatus::InProgress),
            StatusTab::Completed => Some(GameStatus::Completed),
            StatusTab::Platinum => Some(GameStatus::Platinum),
        }
    }

    fn key(&self) -> Key {
        match self {
            StatusTab::All => Key::MyGamesAll,
            StatusTab::WantToPlay => Key::MyGamesWantToPlay,
            StatusTab::InProgress => Key::MyGamesInProgress,
            StatusTab::Completed => Key::MyGamesCompleted,
            StatusTab::Platinum => Key::MyGamesPlatinum,
        }
    }
}

#[component]
pub fn MyGamesPage() -> impl IntoView {
    let i18n = use_i18n();
    let active_tab = RwSignal::new(StatusTab::All);

    let games_resource = Resource::new(
        move || active_tab.get(),
        |tab| async move {
            if let Some(status) = tab.to_status() {
                let result = server_fns::get_games_by_status(status).await;
                match result {
                    Ok(games) => Ok((status, games)),
                    Err(e) => Err(e),
                }
            } else {
                let mut all = Vec::new();
                for status in [
                    GameStatus::WantToPlay,
                    GameStatus::InProgress,
                    GameStatus::Completed,
                    GameStatus::Platinum,
                ] {
                    if let Ok(games) = server_fns::get_games_by_status(status).await {
                        all.extend(games);
                    }
                }
                Ok((GameStatus::WantToPlay, all))
            }
        },
    );

    let tab_icon = |tab: StatusTab| match tab {
        StatusTab::All => "\u{1F4DA}",
        StatusTab::WantToPlay => "\u{1F4CB}",
        StatusTab::InProgress => "\u{1F3AE}",
        StatusTab::Completed => "\u{2705}",
        StatusTab::Platinum => "\u{1F3C6}",
    };

    view! {
        <div class="page my-games-page">
            <h1 class="page-title">{move || t(i18n.locale.get(), Key::MyGamesTitle)}</h1>

            <div class="my-games-tabs">
                {vec![
                    StatusTab::All,
                    StatusTab::WantToPlay,
                    StatusTab::InProgress,
                    StatusTab::Completed,
                    StatusTab::Platinum,
                ].into_iter().map(|tab| {
                    let tab_sv = StoredValue::new(tab);
                    view! {
                        <button
                            class="my-games-tab"
                            class:active=move || active_tab.get() == tab
                            on:click=move |_| active_tab.set(tab_sv.get_value())
                        >
                            <span class="my-games-tab-icon">{tab_icon(tab)}</span>
                            <span class="my-games-tab-label">{move || t(i18n.locale.get(), tab.key())}</span>
                        </button>
                    }
                }).collect::<Vec<_>>()}
            </div>

            <Suspense fallback=move || view! { <div class="my-games-loading">{move || t(i18n.locale.get(), Key::CommonLoading)}</div> }>
                {move || {
                    let locale = i18n.locale.get();
                    Suspend::new(async move {
                        let (_, game_list) = games_resource.await?;
                        Ok::<_, server_fn::ServerFnError>(if game_list.is_empty() {
                            view! {
                                <div class="my-games-empty">{t(locale, Key::MyGamesEmpty)}</div>
                            }.into_any()
                        } else {
                            view! {
                                <div class="my-games-grid">
                                    {game_list.into_iter().map(|entry| view! { <GameStatusCard entry=entry /> }).collect::<Vec<_>>()}
                                </div>
                            }.into_any()
                        })
                    })
                }}
            </Suspense>
        </div>
    }
}

#[component]
fn GameStatusCard(entry: StatusGameEntry) -> impl IntoView {
    let encoded_filename = urlencoding::encode(&entry.rom_filename);
    let href = format!("/games/{}/{}", entry.system, encoded_filename);

    let system_sv = StoredValue::new(entry.system.clone());
    let display_name_sv = StoredValue::new(entry.display_name.clone());
    let box_art_url_sv = StoredValue::new(entry.box_art_url.clone());
    let genre_sv = StoredValue::new(entry.genre.clone());
    let status_sv = StoredValue::new(entry.status);

    view! {
        <A href=href attr:class="my-games-card">
            <div class="my-games-card-cover">
                <Show when=move || box_art_url_sv.get_value().is_some()
                    fallback=move || view! {
                        <BoxArtPlaceholder
                            system=system_sv.get_value()
                            name=display_name_sv.get_value()
                            size="card".to_string()
                        />
                    }
                >
                    <img
                        src=box_art_url_sv.get_value().unwrap_or_default()
                        alt=display_name_sv.get_value()
                        loading="lazy"
                    />
                </Show>
            </div>
            <div class="my-games-card-info">
                <div class="my-games-card-title">{display_name_sv.get_value()}</div>
                <div class="my-games-card-meta">
                    <span class="my-games-card-system">{system_sv.get_value()}</span>
                    <Show when=move || genre_sv.get_value().is_some()>
                        <span class="my-games-card-sep">{"\u{00B7}"}</span>
                        <span class="my-games-card-genre">{genre_sv.get_value().unwrap_or_default()}</span>
                    </Show>
                </div>
            </div>
            <div class="my-games-card-status-icon">
                {match status_sv.get_value() {
                    GameStatus::WantToPlay => "\u{1F4CB}",
                    GameStatus::InProgress => "\u{1F3AE}",
                    GameStatus::Completed => "\u{2705}",
                    GameStatus::Platinum => "\u{1F3C6}",
                }}
            </div>
        </A>
    }
}
