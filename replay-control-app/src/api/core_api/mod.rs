//! Plain REST endpoints for the libretro core.
//!
//! Lightweight Axum handlers — not Leptos server functions — so they have stable,
//! hash-free URLs the core can call reliably. Split per API group: [`games`]
//! (recents/favorites/detail) and [`status`] (library state).

mod games;
mod status;

use axum::Router;

use crate::api::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().merge(games::routes()).merge(status::routes())
}
