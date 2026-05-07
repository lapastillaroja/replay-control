#![cfg(feature = "ssr")]
//! Regression tests for the three layers that block read-time writes
//! to `library.db` during the NFS partial-mount window:
//!
//!   1. `scan_systems` returns `AllSystemsMissing` when the walk
//!      finds zero ROMs across every visible system.
//!   2. `cached_systems` is strictly read-only — an empty L2 returns
//!      `[]` rather than falling through to a filesystem scan.
//!   3. `GET /api/systems/<sys>/roms` is L2-only — no L3 fallback.

mod common;

use axum::body::Body;
use axum::http::Request;
use tower::ServiceExt;

use common::{assert_json_ok, test_api_router};

/// Build a TestEnv-like setup *without* populating the library, so we
/// can simulate the cold-cache state where the boot pipeline hasn't
/// run yet (or returned empty due to a partial mount).
fn unpopulated_state() -> (replay_control_app::api::AppState, std::path::PathBuf) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);

    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = std::env::temp_dir().join(format!("replay-cold-nfs-{}-{id}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);

    // Top-level system folders exist but contain no ROM files —
    // mirrors the partial-mount observation: `read_dir(roms_dir)`
    // surfaces the system folder names, but each subdir's dirent
    // cache is still cold so a recursive walk finds nothing.
    for dir in &[
        "roms/_favorites",
        "roms/_recent",
        "roms/nintendo_nes",
        "roms/sega_smd",
        ".replay-control/media",
        "config",
    ] {
        std::fs::create_dir_all(tmp.join(dir)).unwrap();
    }
    std::fs::write(tmp.join("config/replay.cfg"), "storage_mode=sd\n").unwrap();

    let state = replay_control_app::api::AppState::new(
        Some(tmp.to_string_lossy().into_owned()),
        None,
        None,
        None,
    )
    .unwrap();
    (state, tmp)
}

fn library_meta_row_count(state: &replay_control_app::api::AppState) -> usize {
    use replay_control_core_server::library_db::LibraryDb;
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async {
            state
                .library_reader
                .read(LibraryDb::load_all_system_meta)
                .await
                .unwrap()
                .unwrap()
                .len()
        })
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn get_systems_on_partial_mount_does_not_poison_library_db() {
    let (state, tmp) = unpopulated_state();
    let app = test_api_router(state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/systems")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let json = assert_json_ok(resp).await;
    let systems = json.as_array().expect("array");
    assert!(
        systems.is_empty(),
        "GET /systems on cold cache must return [] (got {} entries) — \
         the L3 fallback used to fire here and persist all-zero rows",
        systems.len()
    );

    assert_eq!(
        library_meta_row_count(&state),
        0,
        "GET /systems must not write to game_library_meta — that was the \
         cold-NFS poisoning vector"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[tokio::test(flavor = "multi_thread")]
async fn get_info_on_partial_mount_does_not_poison_library_db() {
    let (state, tmp) = unpopulated_state();
    let app = test_api_router(state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/info")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let json = assert_json_ok(resp).await;
    assert_eq!(json["total_games"].as_u64().unwrap(), 0);
    assert_eq!(json["systems_with_games"].as_u64().unwrap(), 0);

    assert_eq!(
        library_meta_row_count(&state),
        0,
        "GET /info must not write to game_library_meta on a cold cache"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[tokio::test(flavor = "multi_thread")]
async fn get_system_roms_on_partial_mount_returns_empty_without_writing() {
    let (state, tmp) = unpopulated_state();
    let app = test_api_router(state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/systems/nintendo_nes/roms")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let json = assert_json_ok(resp).await;
    let roms = json.as_array().expect("array");
    assert!(
        roms.is_empty(),
        "GET /systems/<sys>/roms must return [] without falling through \
         to scan_and_cache_system from the request path"
    );

    assert_eq!(
        library_meta_row_count(&state),
        0,
        "GET /systems/<sys>/roms must not write to game_library_meta"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}
