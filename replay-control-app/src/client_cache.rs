//! Session-scoped client caches that "freeze" regenerating recommendation data.
//!
//! Recommendations regenerate server-side on a short interval (and on fav/game
//! events), independent of the user's current navigation. Without a client cache,
//! pressing browser Back re-mounts the page, re-fetches, and can return a *new*
//! set — the row the user was mid-scroll through silently shuffles and the scroll
//! position is lost. These caches hold the resolved data for the page session so
//! Back resumes the exact set the user saw. They're provided once at the app root
//! and cleared on a full reload (the "deliberate fresh visit" that refreshes recs).
//!
//! Last Played and Favorites are *not* cached here: they only change on an
//! explicit user action (playing a game, toggling a favorite), so a re-fetch on
//! Back returns the same content already — the scroll-memory hook handles those.
//!
//! Design decision and known tradeoff: this cache deliberately has **no
//! client-side invalidation path**. It lives for the whole page session and only
//! clears on a full reload (reload / reopen / pull-to-refresh — the "deliberate
//! fresh visit"). The consequence is that recommendation rows can show stale data
//! after a mutation that would change them — notably toggling a favorite (the
//! favorites-derived "Because You Love" row) or a storage switch that swaps the
//! whole library — until the next reload. This is intentional: refreshing on
//! every such event would re-introduce the bug this fixes — the row regenerates
//! and loses its horizontal scroll position on Back. If a particular mutation's
//! staleness becomes worth addressing, the right move is a *targeted*
//! `<cache>.0.set(None)` at that mutation site (a storage switch and
//! add/remove-favorite are the strongest candidates), which lets the next render
//! refetch fresh data and correctly reset the row's scroll — **not** a blanket
//! TTL, which would bring the shuffle-on-Back back.

use std::future::Future;

use leptos::prelude::*;
use serde::Serialize;
use serde::de::DeserializeOwned;
use server_fn::ServerFnError;

use crate::server_fns::{GameSection, RecommendationData};

/// Frozen home-page recommendations snapshot (covers the random picks, favorites
/// picks, and curated spotlight rows, which all live in `RecommendationData`).
#[derive(Clone, Copy)]
pub struct RecsCache(pub RwSignal<Option<RecommendationData>>);

/// Frozen favorites-page recommendation rows.
#[derive(Clone, Copy)]
pub struct FavRecsCache(pub RwSignal<Option<Vec<GameSection>>>);

/// Provide the client caches at the app root so they outlive route components.
pub fn provide_client_caches() {
    provide_context(RecsCache(RwSignal::new(None)));
    provide_context(FavRecsCache(RwSignal::new(None)));
}

/// Build a `Resource` that "freezes" its data in `cache` for the page session.
///
/// On the first load the resource fetches and an effect stashes the result in
/// `cache`; on later mounts (e.g. a browser-Back re-mount) the fetcher returns
/// the cached value instead of re-fetching, so the rows resume the exact set the
/// user saw. The cache only clears on a full reload (see the module docs for the
/// deliberate no-invalidation tradeoff).
pub fn use_frozen_resource<T, Fut>(
    cache: RwSignal<Option<T>>,
    fetch: impl Fn() -> Fut + Clone + Send + Sync + 'static,
) -> Resource<Result<T, ServerFnError>>
where
    T: Clone + Send + Sync + Serialize + DeserializeOwned + 'static,
    Fut: Future<Output = Result<T, ServerFnError>> + Send + 'static,
{
    let resource = Resource::new(
        || (),
        move |_| {
            let fetch = fetch.clone();
            async move {
                if let Some(cached) = cache.get_untracked() {
                    return Ok(cached);
                }
                fetch().await
            }
        },
    );
    Effect::new(move || {
        if cache.get_untracked().is_none()
            && let Some(Ok(data)) = resource.get()
        {
            cache.set(Some(data));
        }
    });
    resource
}
