//! Now-playing detection via the official RePlayOS REST API.
//!
//! Polls `get_status` and maps the response onto [`NowPlayingState`]. This
//! replaced the `/proc/<pid>/mem` heap scanner (and all its selection
//! heuristics) once RePlayOS 1.7.3 shipped the API — `get_status` reports the
//! launched game authoritatively, which structurally eliminates the
//! wrong-game bug classes (arcade clones, multi-disc `.m3u`, ScummVM, MAME).
//!
//! Measured semantics this mapping is built on (see the integration plan's
//! measured-answers table, 2026-06-07):
//!
//! - RePlayOS has no "exit a game" concept: once loaded, `game_file` stays set
//!   until shutdown/restart or another launch. Menus are overlays over the
//!   loaded game, and their `view_id` is inconsistent (0/1/3 depending on
//!   navigation path) — so the mapping keys off `game_file`, never off
//!   menu-view distinctions.
//! - `game_name` always carries the launched entry's filename (even for
//!   ScummVM, where `game_file` points at the inner `.svm`), so
//!   `(system, game_name)` is the primary library-resolution key.
//! - `paused` is live core-pause truth (follows `system_ui_pauses_core`).
//! - Transient `200` payloads with missing fields occur during UI
//!   transitions (~one 0.5 s sample) — hold the previous state on those.
//!
//! The loop is subscriber-gated: it makes zero API calls unless the
//! integration is `Active` *and* at least one client is on the now-playing
//! SSE stream. An unwatched Pi stays silent.

use std::time::Duration;

use replay_control_core::replay_api::{Classification, StatusResponse, classify};
use replay_control_core::systems::system_display_name;
use replay_control_core_server::arcade_db;
use replay_control_core_server::library_db::{GameEntry, LibraryDb};

use super::AppState;
use super::replay_api::ReplayApi;
use crate::types::NowPlayingState;

const POLL_INTERVAL: Duration = Duration::from_secs(4);
/// While nobody is watching (or the integration isn't `Active`), only this
/// cheap in-process check runs — no API traffic.
const IDLE_CHECK_INTERVAL: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(60);

pub async fn run_now_playing_loop(state: AppState) {
    // Standalone: no RePlayOS, no detection. Structural absence.
    let Some(api) = state.replay_api.clone() else {
        return;
    };

    let mut backoff = POLL_INTERVAL;
    let mut resolved_cache: Option<ResolvedSession> = None;
    loop {
        let watched = state.now_playing_tx.receiver_count() > 0;
        if !watched || !api.status().is_active() {
            tokio::time::sleep(IDLE_CHECK_INTERVAL).await;
            continue;
        }

        match api.client().get_status().await {
            Ok(status) => {
                backoff = POLL_INTERVAL;
                apply_status(&state, &api, status, &mut resolved_cache).await;
                tokio::time::sleep(POLL_INTERVAL).await;
            }
            Err(error) => {
                // Feeds the status machine: 401 ⇒ Unauthorized (stops the
                // loop via the Active gate), unreachable ⇒ Error unless a
                // self-initiated restart window is open.
                api.report_error(&error);
                backoff = (backoff * 2).min(MAX_BACKOFF);
                tokio::time::sleep(backoff).await;
            }
        }
    }
}

/// Library resolution for the current game session, computed once on a game
/// transition and reused on every steady-state tick (sessions last minutes to
/// hours; re-resolving an unchanged game every 4 s would be a pointless DB
/// pool round-trip per tick).
struct ResolvedSession {
    /// Raw identity from `get_status` — the transition detector.
    system: String,
    game_name: String,
    /// Resolved presentation fields.
    filename: String,
    display_name: String,
    box_art_url: Option<String>,
    started_at_unix_secs: u64,
}

async fn apply_status(
    state: &AppState,
    api: &ReplayApi,
    status: StatusResponse,
    resolved_cache: &mut Option<ResolvedSession>,
) {
    let next = match classify(&status) {
        Classification::Hold => return,
        Classification::Menu => {
            *resolved_cache = None;
            NowPlayingState::Menu
        }
        Classification::Loaded {
            system,
            game_name,
            game_file,
            play_state,
        } => {
            let same_session = resolved_cache
                .as_ref()
                .is_some_and(|s| s.system == system && s.game_name == game_name);
            if !same_session {
                let resolved = resolve_game(state, &system, &game_name, &game_file).await;
                let (filename, display_name, box_art_url) = match resolved {
                    Some(entry) => {
                        let box_art_url = resolve_box_art_url(state, &entry).await;
                        let display = entry
                            .display_name
                            .clone()
                            .unwrap_or_else(|| entry.rom_filename.clone());
                        (entry.rom_filename, display, box_art_url)
                    }
                    // Unresolved (e.g. a ROM outside the library): show the
                    // raw name rather than nothing.
                    None => (game_name.clone(), game_name.clone(), None),
                };
                *resolved_cache = Some(ResolvedSession {
                    system: system.clone(),
                    game_name,
                    filename,
                    display_name,
                    box_art_url,
                    started_at_unix_secs: now_unix_secs(),
                });

                // RePlayOS wrote a fresh `_recent/` marker for this session.
                // TV-side launches bypass the launch server fn, and on NFS
                // storage no filesystem watcher sees the marker (disabled by
                // design) — the observed transition is the invalidation
                // signal. Mirrors the launch server fn.
                state.library.invalidate_after_launch().await;
            }
            let session = resolved_cache.as_ref().expect("session resolved above");

            // Disc position for multi-disc games — decoration on top of
            // detection; a failure here must never disturb the state. Polled
            // every tick on purpose: it keeps TV-side disc swaps live, and a
            // one-shot "is this game disc-capable?" check could misclassify a
            // CD game probed mid-boot. Localhost, sub-millisecond.
            let disc = api
                .client()
                .get_media_status()
                .await
                .ok()
                .and_then(|media| media.disc_info());

            NowPlayingState::Playing {
                system_display: system_display_name(&session.system),
                system: session.system.clone(),
                filename: session.filename.clone(),
                display_name: session.display_name.clone(),
                box_art_url: session.box_art_url.clone(),
                started_at_unix_secs: session.started_at_unix_secs,
                play_state,
                disc,
            }
        }
    };

    // Dedupes + broadcasts on change.
    state.set_now_playing(next);
}

/// Resolve the library row for the running game: exact `(system, game_name)`
/// first (the launched entry's filename — covers everything measured,
/// ScummVM included), then longest-prefix on `rom_path` from the absolute
/// `game_file` as the fallback for anything exotic.
async fn resolve_game(
    state: &AppState,
    system: &str,
    game_name: &str,
    game_file: &str,
) -> Option<GameEntry> {
    let sys = system.to_string();
    let name = game_name.to_string();
    let exact = state
        .library_reader
        .read(move |conn| LibraryDb::lookup_game_entries(conn, &[(&sys, &name)]))
        .await
        .and_then(|r| r.ok())
        .and_then(|rows| rows.into_values().next());
    if exact.is_some() {
        return exact;
    }

    let rom_path = extract_rom_path(game_file)?;
    let sys = system.to_string();
    state
        .library_reader
        .read(move |conn| LibraryDb::lookup_game_by_path_prefix(conn, &sys, &rom_path))
        .await
        .and_then(|r| r.ok())
        .flatten()
}

/// Resolve now-playing cover art with the same precedence as detail pages:
/// explicit user override, library-enriched URL, then filesystem fallback.
async fn resolve_box_art_url(state: &AppState, entry: &GameEntry) -> Option<String> {
    let arcade_display =
        arcade_db::display_name_if_arcade(&entry.system, &entry.rom_filename).await;
    crate::server_fns::resolve_box_art_url(
        state,
        &entry.system,
        &entry.rom_filename,
        entry.box_art_url.as_deref(),
        arcade_display.as_deref(),
    )
    .await
}

/// `/media/nfs/roms/sega_smd/sub/Game.md` → `/roms/sega_smd/sub/Game.md`
/// (the library's storage-relative `rom_path` shape).
fn extract_rom_path(path: &str) -> Option<String> {
    let idx = path.find("/roms/")?;
    Some(path[idx..].to_string())
}

fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_rom_path_keeps_storage_relative_shape() {
        assert_eq!(
            extract_rom_path("/media/nfs/roms/sega_smd/sub/Game.md").as_deref(),
            Some("/roms/sega_smd/sub/Game.md")
        );
        assert_eq!(extract_rom_path("/media/sd/other/file"), None);
    }
}
