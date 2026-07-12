//! `/api/core/status` — a machine-readable library-state contract for the
//! libretro core (which polls and isn't activity-aware), health checks, and the
//! E2E harness. Read-only and side-effect-free; public (see `is_public_without_auth`).

use std::collections::BTreeMap;

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

use replay_control_core::library::db::SystemStatsRefreshState;
use replay_control_core_server::library_db::LibraryDb;

use crate::api::AppState;

/// Per-system entry in [`LibraryStatus`].
#[derive(Serialize)]
struct SystemStatus {
    state: SystemStatsRefreshState,
    roms: usize,
}

/// Payload for `GET /api/core/status`.
#[derive(Serialize)]
struct LibraryStatus {
    /// Safe to browse — the boot populate has completed **and** nothing is
    /// scanning. Gated on the populate marker so it stays `false` during the boot
    /// window even when `systems` is empty; a genuinely-empty library reports
    /// `ready: true, systems: {}` only *after* the scan confirms it.
    ready: bool,
    /// Any system is being (re)scanned.
    scanning: bool,
    total_roms: usize,
    /// Coarse label of what the app is doing (see `Activity::status_label`).
    activity: &'static str,
    /// Per-system durable refresh state + ROM count, keyed by system folder.
    systems: BTreeMap<String, SystemStatus>,
}

async fn status(State(state): State<AppState>) -> Json<LibraryStatus> {
    let rows = state
        .library_reader
        .read(LibraryDb::system_refresh_status)
        .await
        .and_then(|result| result.ok())
        .unwrap_or_default();

    let scanning = rows
        .iter()
        .any(|(_, _, refresh)| *refresh == SystemStatsRefreshState::Refreshing);
    let total_roms: usize = rows.iter().map(|(_, roms, _)| roms).sum();

    Json(LibraryStatus {
        ready: state.initial_populate_done() && !scanning,
        scanning,
        total_roms,
        activity: state.activity().status_label(),
        systems: rows
            .into_iter()
            .map(|(system, roms, refresh)| {
                (
                    system,
                    SystemStatus {
                        state: refresh,
                        roms,
                    },
                )
            })
            .collect(),
    })
}

pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/status", get(status))
}
