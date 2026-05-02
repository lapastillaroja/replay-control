use leptos::prelude::*;
use leptos_router::components::A;
use server_fn::ServerFnError;

use crate::components::boxart_placeholder::BoxArtPlaceholder;
use crate::i18n::{Key, t, use_i18n};
use crate::server_fns::{self, BacklogEntry, HltbData};

#[component]
pub fn BacklogPage() -> impl IntoView {
    let i18n = use_i18n();
    let entries = Resource::new(|| (), |_| server_fns::get_backlog());

    view! {
        <div class="page backlog-page">
            <div class="rom-header">
                <h1 class="page-title">{move || t(i18n.locale.get(), Key::BacklogTitle)}</h1>
            </div>
            <Suspense fallback=move || view! { <BacklogSkeleton /> }>
                {move || Suspend::new(async move {
                    let games = entries.await?;
                    Ok::<_, ServerFnError>(view! { <BacklogContent games entries /> })
                })}
            </Suspense>
        </div>
    }
}

#[component]
fn BacklogContent(
    games: Vec<BacklogEntry>,
    entries: Resource<Result<Vec<BacklogEntry>, ServerFnError>>,
) -> impl IntoView {
    let i18n = use_i18n();

    if games.is_empty() {
        return view! {
            <div class="backlog-empty">
                <p>{move || t(i18n.locale.get(), Key::BacklogEmpty)}</p>
                <A href="/">{move || t(i18n.locale.get(), Key::BacklogBrowse)}</A>
            </div>
        }
        .into_any();
    }

    let total = games.len();

    view! {
        <div class="backlog-list">
            <p class="backlog-count">
                {move || {
                    let locale = i18n.locale.get();
                    format!("{} {}", total, t(locale, Key::BacklogGames))
                }}
            </p>
            {games
                .into_iter()
                .map(|entry| {
                    view! { <BacklogRow entry entries /> }
                })
                .collect::<Vec<_>>()}
        </div>
    }
    .into_any()
}

#[component]
fn BacklogRow(
    entry: BacklogEntry,
    entries: Resource<Result<Vec<BacklogEntry>, ServerFnError>>,
) -> impl IntoView {
    let i18n = use_i18n();

    let system = StoredValue::new(entry.entry.system.clone());
    let rom_filename = StoredValue::new(entry.entry.rom_filename.clone());
    let display_name = StoredValue::new(
        entry
            .display_name
            .clone()
            .unwrap_or_else(|| entry.entry.rom_filename.clone()),
    );
    let box_art_url = StoredValue::new(entry.box_art_url.clone());
    let hltb = StoredValue::new(entry.hltb.clone());
    let href = StoredValue::new(format!(
        "/games/{}/{}",
        urlencoding::encode(&entry.entry.system),
        urlencoding::encode(&entry.entry.rom_filename)
    ));

    let removing = RwSignal::new(false);

    let on_remove = move |_| {
        let sys = system.get_value();
        let fname = rom_filename.get_value();
        removing.set(true);
        leptos::task::spawn_local(async move {
            let _ = server_fns::remove_from_backlog(sys, fname).await;
            entries.refetch();
        });
    };

    view! {
        <div class="backlog-row">
            <A href=move || href.get_value() attr:class="backlog-row-link">
                // Box art
                <div class="backlog-art">
                    {move || {
                        if let Some(url) = box_art_url.get_value() {
                            view! { <img class="backlog-art-img" src=url alt="" /> }.into_any()
                        } else {
                            view! {
                                <BoxArtPlaceholder
                                    system=system.get_value()
                                    name=display_name.get_value()
                                    size="list".to_string()
                                />
                            }.into_any()
                        }
                    }}
                </div>

                // Game info
                <div class="backlog-info">
                    <span class="backlog-name">{move || display_name.get_value()}</span>
                    {move || hltb.get_value().map(|data| {
                        view! { <HltbBadge data /> }
                    })}
                </div>
            </A>

            // Remove button
            <button
                class="backlog-remove-btn"
                on:click=on_remove
                disabled=move || removing.get()
                aria-label=move || t(i18n.locale.get(), Key::BacklogRemove)
            >
                "✕"
            </button>
        </div>
    }
}

/// Compact inline HLTB times badge shown in the backlog row.
#[component]
fn HltbBadge(data: HltbData) -> impl IntoView {
    let parts: Vec<String> = [
        data.main_secs.map(|s| format!("⏱ {}", HltbData::format_hours(s))),
        data.main_extra_secs
            .map(|s| format!("+ {}", HltbData::format_hours(s))),
        data.completionist_secs
            .map(|s| format!("100% {}", HltbData::format_hours(s))),
    ]
    .into_iter()
    .flatten()
    .collect();

    if parts.is_empty() {
        return view! { <span></span> }.into_any();
    }

    view! {
        <span class="backlog-hltb-badge">{parts.join(" · ")}</span>
    }
    .into_any()
}

#[component]
fn BacklogSkeleton() -> impl IntoView {
    view! {
        <div class="backlog-list">
            {(0..6)
                .map(|_| {
                    view! {
                        <div class="backlog-row skeleton-shimmer backlog-row-skeleton"></div>
                    }
                })
                .collect::<Vec<_>>()}
        </div>
    }
}
