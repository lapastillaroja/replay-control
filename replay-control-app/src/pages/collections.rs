use leptos::prelude::*;
use leptos_router::components::A;
use replay_control_core::systems::system_abbreviation;
use server_fn::ServerFnError;

use crate::components::boxart_placeholder::BoxArtPlaceholder;
use crate::i18n::{Key, t, use_i18n};
use crate::server_fns::{self, FavoriteWithArt};

/// Sentinel path segment for the generic system favorites list — the loose
/// `.fav` markers sitting directly in `_favorites/` (RePlayOS's own ⭐ list).
/// Not a real folder name, so it can never collide with a subfolder.
const ROOT_LIST: &str = "\u{0}general";

/// Browse the favorites `_favorites/` folder tree as navigable collections.
///
/// Same idiom as the "My Games" page: a horizontal chip/tab row to pick a
/// collection (folder) at the top, and a card grid of ROMs below. Nested
/// folders drill in via the chips; the breadcrumb walks back.
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
        <div class="page my-games-page collections-page">
            <h1 class="page-title">{move || t(i18n.locale.get(), Key::CollectionsTitle)}</h1>
            <Suspense fallback=move || {
                view! { <div class="my-games-loading">{move || t(i18n.locale.get(), Key::CollectionsLoading)}</div> }
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
        <div class="collections-breadcrumb">
            <button
                class="crumb"
                class:active=move || path.get().is_empty()
                on:click=move |_| path.set(Vec::new())
            >
                {move || t(i18n.locale.get(), Key::CollectionsTitle)}
            </button>
            {move || {
                let cur = path.get();
                let locale = i18n.locale.get();
                cur.iter()
                    .enumerate()
                    .map(|(i, seg)| {
                        let label = if seg == ROOT_LIST {
                            t(locale, Key::CollectionsGeneral).to_string()
                        } else {
                            seg.clone()
                        };
                        let upto = cur[..=i].to_vec();
                        view! {
                            <span class="crumb-sep">"/"</span>
                            <button class="crumb" on:click=move |_| path.set(upto.clone())>
                                {label}
                            </button>
                        }
                    })
                    .collect::<Vec<_>>()
            }}
        </div>

        {move || {
            let cur = path.get();
            let depth = cur.len();
            // Are we inside the generic system list (loose root favorites)?
            let in_root_list = cur.first().map(|s| s == ROOT_LIST).unwrap_or(false);

            let (folders, games, loose_count) = all.with_value(|list| {
                let mut folders: std::collections::BTreeMap<String, usize> = Default::default();
                let mut games: Vec<FavoriteWithArt> = Vec::new();
                let mut loose_count = 0usize;
                for f in list.iter() {
                    let sub = &f.fav.subfolder;
                    let segs: Vec<&str> = if sub.is_empty() {
                        Vec::new()
                    } else {
                        sub.split('/').collect()
                    };
                    if in_root_list {
                        // The generic list = favorites with no subfolder.
                        if segs.is_empty() {
                            games.push(f.clone());
                        }
                        continue;
                    }
                    if segs.is_empty() {
                        // Loose favorite — surfaced via the synthetic "General" chip
                        // at the root, not dumped inline among the folders.
                        loose_count += 1;
                        continue;
                    }
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
                (folders, games, loose_count)
            });

            let show_general = depth == 0 && loose_count > 0;

            if folders.is_empty() && games.is_empty() && !show_general {
                return view! {
                    <p class="my-games-empty">
                        {move || t(i18n.locale.get(), Key::CollectionsEmpty)}
                    </p>
                }
                .into_any();
            }

            // Collection chips — same look as My Games tabs. Clicking drills in.
            // The generic system list leads as its own chip at the root.
            let general_chip = if show_general {
                let label = t(i18n.locale.get(), Key::CollectionsGeneral).to_string();
                Some(view! {
                    <button
                        class="my-games-tab"
                        on:click=move |_| path.set(vec![ROOT_LIST.to_string()])
                    >
                        <span class="my-games-tab-icon">"\u{2B50}"</span>
                        <span>{label}</span>
                        <span class="collection-chip-count">{loose_count}</span>
                    </button>
                })
            } else {
                None
            };

            let mut chips: Vec<_> = general_chip.into_iter().map(|v| v.into_any()).collect();
            chips.extend(folders.into_iter().map(|(name, count)| {
                let go = name.clone();
                view! {
                    <button
                        class="my-games-tab"
                        on:click=move |_| {
                            let mut p = path.get_untracked();
                            p.push(go.clone());
                            path.set(p);
                        }
                    >
                        <span class="my-games-tab-icon">"\u{1F4C1}"</span>
                        <span>{name}</span>
                        <span class="collection-chip-count">{count}</span>
                    </button>
                }
                .into_any()
            }));

            let cards = games
                .into_iter()
                .map(|f| view! { <CollectionGameCard f=f /> })
                .collect::<Vec<_>>();

            let chip_row = if chips.is_empty() {
                ().into_any()
            } else {
                view! { <div class="my-games-tabs collections-chips">{chips}</div> }.into_any()
            };
            let grid = if cards.is_empty() {
                ().into_any()
            } else {
                view! { <div class="my-games-grid">{cards}</div> }.into_any()
            };

            view! { <div>{chip_row}{grid}</div> }.into_any()
        }}
    }
}

/// A single ROM card, matching the My Games card layout.
#[component]
fn CollectionGameCard(f: FavoriteWithArt) -> impl IntoView {
    let href = format!(
        "/games/{}/{}",
        f.fav.game.system,
        urlencoding::encode(&f.fav.game.rom_filename),
    );
    let system_label = system_abbreviation(&f.fav.game.system).to_string();
    let name = f
        .fav
        .game
        .display_name
        .clone()
        .unwrap_or_else(|| f.fav.game.rom_filename.clone());

    let sys_sv = StoredValue::new(f.fav.game.system.clone());
    let name_sv = StoredValue::new(name);
    let box_sv = StoredValue::new(f.box_art_url.clone());
    let genre_sv = StoredValue::new(f.genre.clone());

    view! {
        <A href=href attr:class="my-games-card">
            <div class="my-games-card-cover">
                <Show
                    when=move || box_sv.get_value().is_some()
                    fallback=move || view! {
                        <BoxArtPlaceholder
                            system=sys_sv.get_value()
                            name=name_sv.get_value()
                            size="card".to_string()
                        />
                    }
                >
                    <img
                        src=box_sv.get_value().unwrap_or_default()
                        alt=name_sv.get_value()
                        loading="lazy"
                    />
                </Show>
            </div>
            <div class="my-games-card-info">
                <div class="my-games-card-title">{name_sv.get_value()}</div>
                <div class="my-games-card-meta">
                    <span class="my-games-card-system">{system_label}</span>
                    <Show when=move || genre_sv.get_value().is_some()>
                        <span class="my-games-card-sep">{"\u{00B7}"}</span>
                        <span class="my-games-card-genre">{genre_sv.get_value().unwrap_or_default()}</span>
                    </Show>
                </div>
            </div>
        </A>
    }
}
