use leptos::prelude::*;
use replay_control_core::systems::system_abbreviation;
use server_fn::ServerFnError;

use crate::components::hero_card::GameScrollCard;
use crate::i18n::{Key, t, use_i18n};
use crate::server_fns::{self, FavoriteWithArt};

/// Browse the favorites `_favorites/` folder tree as navigable collections.
///
/// Reuses the existing `get_favorites` server fn: every favorite already carries
/// its `subfolder` (e.g. "Evercade/Capcom Collection"), so the whole tree is
/// built client-side by grouping on the subfolder path segments. No backend.
#[component]
pub fn CollectionsPage() -> impl IntoView {
    let i18n = use_i18n();
    let favs = Resource::new(|| (), |_| server_fns::get_favorites());
    let path = RwSignal::new(Vec::<String>::new());

    view! {
        <div class="collections-page">
            <div class="page-header">
                <h2 class="page-title">{move || t(i18n.locale.get(), Key::CollectionsTitle)}</h2>
            </div>
            <Suspense fallback=move || {
                view! { <p class="empty-state">{move || t(i18n.locale.get(), Key::CollectionsLoading)}</p> }
            }>
                {move || Suspend::new(async move {
                    let all = favs.await?;
                    Ok::<_, ServerFnError>(view! { <CollectionsBrowser all path=path /> })
                })}
            </Suspense>
        </div>
    }
}

#[component]
fn CollectionsBrowser(all: Vec<FavoriteWithArt>, path: RwSignal<Vec<String>>) -> impl IntoView {
    let i18n = use_i18n();
    let all = StoredValue::new(all);

    view! {
        <div class="collections-browser">
            <div class="collections-breadcrumb">
                <button class="crumb" on:click=move |_| path.set(Vec::new())>
                    {move || t(i18n.locale.get(), Key::CollectionsTitle)}
                </button>
                {move || {
                    let cur = path.get();
                    cur.iter()
                        .enumerate()
                        .map(|(i, seg)| {
                            let seg = seg.clone();
                            let upto = cur[..=i].to_vec();
                            view! {
                                <span class="crumb-sep">"/"</span>
                                <button
                                    class="crumb"
                                    on:click=move |_| path.set(upto.clone())
                                >
                                    {seg}
                                </button>
                            }
                        })
                        .collect::<Vec<_>>()
                }}
            </div>

            {move || {
                let cur = path.get();
                let depth = cur.len();
                let (folders, games) = all.with_value(|list| {
                    let mut folders: std::collections::BTreeMap<String, usize> = Default::default();
                    let mut games: Vec<FavoriteWithArt> = Vec::new();
                    for f in list.iter() {
                        let sub = &f.fav.subfolder;
                        let segs: Vec<&str> = if sub.is_empty() {
                            Vec::new()
                        } else {
                            sub.split('/').collect()
                        };
                        if segs.len() < depth {
                            continue;
                        }
                        if !cur.iter().zip(segs.iter()).all(|(a, b)| a == b) {
                            continue;
                        }
                        if segs.len() == depth {
                            games.push(f.clone());
                        } else {
                            *folders.entry(segs[depth].to_string()).or_insert(0) += 1;
                        }
                    }
                    (folders, games)
                });

                if folders.is_empty() && games.is_empty() {
                    return view! {
                        <p class="empty-state">
                            {move || t(i18n.locale.get(), Key::CollectionsEmpty)}
                        </p>
                    }
                    .into_any();
                }

                let folder_cards = folders
                    .into_iter()
                    .map(|(name, count)| {
                        let go = name.clone();
                        view! {
                            <button
                                class="collection-folder"
                                on:click=move |_| {
                                    let mut p = path.get_untracked();
                                    p.push(go.clone());
                                    path.set(p);
                                }
                            >
                                <span class="collection-folder-icon">"\u{1F4C1}"</span>
                                <span class="collection-folder-name">{name}</span>
                                <span class="collection-folder-count">{count}</span>
                            </button>
                        }
                    })
                    .collect::<Vec<_>>();

                let game_cards = games
                    .into_iter()
                    .map(|f| {
                        let href = format!(
                            "/games/{}/{}",
                            f.fav.game.system,
                            urlencoding::encode(&f.fav.game.rom_filename),
                        );
                        let name = f
                            .fav
                            .game
                            .display_name
                            .clone()
                            .unwrap_or_else(|| f.fav.game.rom_filename.clone());
                        let system = system_abbreviation(&f.fav.game.system).to_string();
                        let system_folder = f.fav.game.system.clone();
                        let box_art_url = f.box_art_url.clone();
                        view! { <GameScrollCard href name system system_folder box_art_url /> }
                    })
                    .collect::<Vec<_>>();

                let folder_section = if folder_cards.is_empty() {
                    ().into_any()
                } else {
                    view! { <div class="collections-folder-grid">{folder_cards}</div> }.into_any()
                };
                let game_section = if game_cards.is_empty() {
                    ().into_any()
                } else {
                    view! { <div class="collections-game-grid">{game_cards}</div> }.into_any()
                };

                view! {
                    <div>
                        {folder_section}
                        {game_section}
                    </div>
                }
                .into_any()
            }}
        </div>
    }
}
