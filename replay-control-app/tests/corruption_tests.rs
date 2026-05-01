#![cfg(feature = "ssr")]
//! Integration tests for the database-corruption broadcast + recovery flow.
//!
//! Covers the contract the live SSE banner depends on:
//!   1. Pool flag transitions broadcast `ConfigEvent::CorruptionChanged` on
//!      `config_tx` (the wire that backs `/sse/config`).
//!   2. The three recovery server fns (`repair_corrupt_user_data`,
//!      `restore_user_data_backup`, `rebuild_corrupt_library`) clear the
//!      corrupt flag *and* broadcast the inverse event.
//!   3. Startup with a clobbered `user_data.db` magic header does not crash
//!      the service — the pool comes up flagged corrupt, ready for recovery.

mod common;

use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use server_fn::ServerFn;
use tokio::time::timeout;
use tower::ServiceExt;

use common::{TestEnv, init_executor, register_server_fns, test_router};
use replay_control_app::api::{AppState, ConfigEvent, DbPool};
use replay_control_app::server_fns;

fn setup() {
    init_executor();
    register_server_fns();
}

/// Wait up to 1s for the next `CorruptionChanged` event on the channel.
/// Other event variants (Skin/Storage/UpdateAvailable) are skipped so the
/// helper is robust if those happen to fire concurrently.
async fn next_corruption_event(
    rx: &mut tokio::sync::broadcast::Receiver<ConfigEvent>,
) -> ConfigEvent {
    let deadline = Duration::from_secs(1);
    let result = timeout(deadline, async {
        loop {
            match rx.recv().await {
                Ok(ev @ ConfigEvent::CorruptionChanged { .. }) => return ev,
                Ok(_) => continue,
                Err(e) => panic!("broadcast channel error: {e}"),
            }
        }
    })
    .await;
    result.expect("expected CorruptionChanged within 1s")
}

/// Overwrite the first 4 KiB of a file with random-looking bytes. Used to
/// clobber the SQLite magic header (`SQLite format 3\0`) without truncating
/// — what a torn write on power loss looks like, or `dd` in our manual tests.
fn corrupt_file_header(path: &std::path::Path) {
    use std::io::Write;

    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("open file for corruption");
    let garbage = [0xDEu8; 4096];
    f.write_all(&garbage).expect("write garbage");
}

/// Storage dir whose lifetime is independent of any `AppState` — the regular
/// `TestEnv::drop` wipes the dir, which is the wrong shape for tests that
/// need to outlive (or reconstruct) the state to corrupt files on disk.
struct StandaloneStorage {
    path: std::path::PathBuf,
}

impl StandaloneStorage {
    fn new(tag: &str) -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("replay-corrupt-{tag}-{}-{id}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        for dir in &[
            "roms/_favorites",
            "roms/_recent",
            "roms/nintendo_nes",
            ".replay-control/media",
            "config",
        ] {
            std::fs::create_dir_all(path.join(dir)).unwrap();
        }
        std::fs::write(path.join("config/replay.cfg"), "storage_mode=sd\n").unwrap();
        Self { path }
    }

    fn build_state(&self) -> AppState {
        AppState::new(
            Some(self.path.to_string_lossy().into_owned()),
            None,
            None,
            None,
        )
        .unwrap()
    }
}

impl Drop for StandaloneStorage {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn user_data_mark_corrupt_broadcasts_event() {
    setup();
    let env = TestEnv::new();
    let mut rx = env.state.config_tx.subscribe();

    env.state.user_data_pool.mark_corrupt();

    let ev = next_corruption_event(&mut rx).await;
    match ev {
        ConfigEvent::CorruptionChanged {
            library_corrupt,
            user_data_corrupt,
            ..
        } => {
            assert!(user_data_corrupt, "user_data flag should be true");
            assert!(!library_corrupt, "library flag should still be false");
        }
        _ => unreachable!(),
    }
    assert!(env.state.user_data_pool.is_corrupt());
}

#[tokio::test(flavor = "multi_thread")]
async fn library_mark_corrupt_broadcasts_event() {
    setup();
    let env = TestEnv::new();
    let mut rx = env.state.config_tx.subscribe();

    env.state.library_pool.mark_corrupt();

    let ev = next_corruption_event(&mut rx).await;
    match ev {
        ConfigEvent::CorruptionChanged {
            library_corrupt,
            user_data_corrupt,
            ..
        } => {
            assert!(library_corrupt, "library flag should be true");
            assert!(!user_data_corrupt, "user_data flag should still be false");
        }
        _ => unreachable!(),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn idempotent_mark_corrupt_does_not_re_broadcast() {
    setup();
    let env = TestEnv::new();
    let mut rx = env.state.config_tx.subscribe();

    env.state.user_data_pool.mark_corrupt();
    let _first = next_corruption_event(&mut rx).await;

    env.state.user_data_pool.mark_corrupt(); // already corrupt
    let second = timeout(Duration::from_millis(150), rx.recv()).await;
    assert!(
        second.is_err(),
        "no second event expected on idempotent mark_corrupt"
    );
}

async fn invoke_server_fn<F: ServerFn>(state: AppState, body: &str) -> StatusCode {
    let app = test_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(F::PATH)
                .header("content-type", "application/x-www-form-urlencoded")
                .header("accept", "application/x-www-form-urlencoded")
                .body(Body::from(body.to_owned()))
                .unwrap(),
        )
        .await
        .unwrap();
    resp.status()
}

/// Shared shape of every recovery-broadcast test: pick a pool, mark it
/// corrupt, drain the "set" event, invoke the recovery server fn, return the
/// "cleared" event for the caller to assert on. Keeps each test focused on
/// the one assertion that actually differs (which flag flips back to false).
async fn run_recovery_test<F: ServerFn>(pick_pool: fn(&AppState) -> &DbPool) -> ConfigEvent {
    setup();
    let env = TestEnv::new();
    let mut rx = env.state.config_tx.subscribe();

    let pool = pick_pool(&env.state);
    pool.mark_corrupt();
    let _set = next_corruption_event(&mut rx).await;

    let status = invoke_server_fn::<F>(env.state.clone(), "").await;
    assert_eq!(status, StatusCode::OK, "recovery server fn should succeed");

    let cleared = next_corruption_event(&mut rx).await;
    assert!(
        !pick_pool(&env.state).is_corrupt(),
        "pool must clear after recovery"
    );
    cleared
}

#[tokio::test(flavor = "multi_thread")]
async fn repair_corrupt_user_data_clears_flag_and_broadcasts_inverse() {
    let cleared =
        run_recovery_test::<server_fns::RepairCorruptUserData>(|s| &s.user_data_pool).await;
    match cleared {
        ConfigEvent::CorruptionChanged {
            user_data_corrupt, ..
        } => assert!(!user_data_corrupt, "flag must clear after repair"),
        _ => unreachable!(),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn restore_user_data_backup_clears_flag_and_broadcasts_inverse() {
    setup();
    let env = TestEnv::new();

    // TestEnv brings up AppState which auto-saves a healthy backup at startup;
    // that's exactly what restore expects to find. Verify the precondition so
    // failure here is a setup issue, not a logic issue.
    let backup_path = env.state.user_data_pool.db_path().with_extension("db.bak");
    assert!(
        backup_path.exists(),
        "TestEnv setup should have created .bak"
    );

    let mut rx = env.state.config_tx.subscribe();
    env.state.user_data_pool.mark_corrupt();
    let _set = next_corruption_event(&mut rx).await;

    let status = invoke_server_fn::<server_fns::RestoreUserDataBackup>(env.state.clone(), "").await;
    assert_eq!(status, StatusCode::OK, "restore should succeed");

    let cleared = next_corruption_event(&mut rx).await;
    match cleared {
        ConfigEvent::CorruptionChanged {
            user_data_corrupt, ..
        } => assert!(!user_data_corrupt),
        _ => unreachable!(),
    }
    assert!(!env.state.user_data_pool.is_corrupt());
}

#[tokio::test(flavor = "multi_thread")]
async fn rebuild_corrupt_library_clears_flag_and_broadcasts_inverse() {
    let cleared = run_recovery_test::<server_fns::RebuildCorruptLibrary>(|s| &s.library_pool).await;
    match cleared {
        ConfigEvent::CorruptionChanged {
            library_corrupt, ..
        } => assert!(!library_corrupt),
        _ => unreachable!(),
    }
}

/// Content-survival assertion: rebuild must actually wipe the library file,
/// not just flip the corrupt flag. A future refactor that no-ops the
/// drain/unlink while still setting the flag would pass the broadcast tests
/// above; this one fails it.
#[tokio::test(flavor = "multi_thread")]
async fn rebuild_corrupt_library_wipes_table_content() {
    setup();
    let env = TestEnv::new();

    // Pre-populate a sentinel row that survives only if reset_to_empty is a
    // no-op. game_library_meta is the simplest table with a known schema.
    let inserted = env
        .state
        .library_pool
        .write(|conn| {
            conn.execute(
                "INSERT INTO game_library_meta \
                 (system, dir_mtime_secs, scanned_at, rom_count, total_size_bytes) \
                 VALUES ('rebuild_sentinel', NULL, 0, 1, 0)",
                [],
            )
        })
        .await;
    assert!(matches!(inserted, Some(Ok(1))), "sentinel insert failed");

    env.state.library_pool.mark_corrupt();

    let status = invoke_server_fn::<server_fns::RebuildCorruptLibrary>(env.state.clone(), "").await;
    assert_eq!(status, StatusCode::OK);

    let count: i64 = env
        .state
        .library_pool
        .read(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM game_library_meta WHERE system = 'rebuild_sentinel'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(-1)
        })
        .await
        .unwrap_or(-1);
    assert_eq!(
        count, 0,
        "rebuild must wipe library content; sentinel row should not survive"
    );
}

/// `repair_corrupt_user_data` actually drains and unlinks the user_data
/// file. A no-op refactor that just flipped the corrupt flag would pass
/// `repair_corrupt_user_data_clears_flag_and_broadcasts_inverse` above —
/// the content assertion here closes that gap symmetrically with the
/// library-side test.
#[tokio::test(flavor = "multi_thread")]
async fn repair_corrupt_user_data_wipes_table_content() {
    setup();
    let env = TestEnv::new();

    let inserted = env
        .state
        .user_data_pool
        .write(|conn| {
            conn.execute(
                "INSERT INTO box_art_overrides \
                 (system, rom_filename, override_path, set_at) \
                 VALUES ('test_sys', 'sentinel.rom', '/tmp/x.png', 0)",
                [],
            )
        })
        .await;
    assert!(matches!(inserted, Some(Ok(1))), "sentinel insert failed");

    env.state.user_data_pool.mark_corrupt();

    let status =
        invoke_server_fn::<server_fns::RepairCorruptUserData>(env.state.clone(), "").await;
    assert_eq!(status, StatusCode::OK);

    let count: i64 = env
        .state
        .user_data_pool
        .read(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM box_art_overrides WHERE rom_filename = 'sentinel.rom'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(-1)
        })
        .await
        .unwrap_or(-1);
    assert_eq!(
        count, 0,
        "repair must wipe user_data content; sentinel row should not survive"
    );
}

/// `restore_user_data_backup` actually copies the .bak content over the
/// live DB. The flag-flip-only `restore_user_data_backup_clears_flag_and_
/// broadcasts_inverse` test would pass even if `replace_with_file` was a
/// no-op — this asserts the *contract*: a sentinel row in the backup
/// shows up in the live DB after restore.
#[tokio::test(flavor = "multi_thread")]
async fn restore_user_data_backup_actually_restores_content() {
    setup();
    let env = TestEnv::new();

    // Insert a sentinel BEFORE creating the backup. (TestEnv's auto-backup
    // ran at startup against an empty DB, so it doesn't have our row.)
    let inserted = env
        .state
        .user_data_pool
        .write(|conn| {
            conn.execute(
                "INSERT INTO box_art_overrides \
                 (system, rom_filename, override_path, set_at) \
                 VALUES ('test_sys', 'restore_sentinel.rom', '/tmp/y.png', 42)",
                [],
            )
        })
        .await;
    assert!(matches!(inserted, Some(Ok(1))), "sentinel insert failed");

    // Checkpoint so the WAL is rolled into the main file before we copy —
    // otherwise the .bak we make is missing the row.
    env.state.user_data_pool.checkpoint().await;

    // Manually refresh the .bak so it carries the sentinel row.
    let ud_path = env.state.user_data_pool.db_path();
    let bak_path = ud_path.with_extension("db.bak");
    std::fs::copy(&ud_path, &bak_path).expect("manual backup snapshot");

    env.state.user_data_pool.mark_corrupt();

    let status =
        invoke_server_fn::<server_fns::RestoreUserDataBackup>(env.state.clone(), "").await;
    assert_eq!(status, StatusCode::OK);

    let restored: Option<String> = env
        .state
        .user_data_pool
        .read(|conn| {
            conn.query_row(
                "SELECT override_path FROM box_art_overrides \
                 WHERE rom_filename = 'restore_sentinel.rom'",
                [],
                |r| r.get(0),
            )
            .ok()
        })
        .await
        .flatten();
    assert_eq!(
        restored.as_deref(),
        Some("/tmp/y.png"),
        "restore must round-trip the backup's content into the live DB"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn startup_with_clobbered_user_data_header_does_not_crash() {
    setup();
    let storage = StandaloneStorage::new("nocrash");

    // First boot — creates user_data.db + .bak.
    let state1 = storage.build_state();
    let ud_path = state1.user_data_pool.db_path();
    let bak_path = ud_path.with_extension("db.bak");
    assert!(bak_path.exists());
    drop(state1);

    // Clobber the magic header on disk — what a torn write or `dd` does.
    corrupt_file_header(&ud_path);

    // Second boot. Pre-fix this returned an error and the service
    // crash-looped under systemd. Post-fix this succeeds with the user_data
    // pool flagged corrupt.
    let state2 = storage.build_state();
    assert!(
        state2.user_data_pool.is_corrupt(),
        "user_data pool must come up flagged corrupt"
    );
    assert!(
        !state2.library_pool.is_corrupt(),
        "library pool must be unaffected"
    );

    // The .bak we preserved is still there so the recovery banner shows
    // Restore (not just Reset).
    let (lib, ud, backup) = state2.corruption_status();
    assert!(!lib && ud && backup);
}

#[tokio::test(flavor = "multi_thread")]
async fn restore_after_startup_corruption_recovers_pool() {
    setup();
    let storage = StandaloneStorage::new("recover");

    let state1 = storage.build_state();
    let ud_path = state1.user_data_pool.db_path();
    drop(state1);

    corrupt_file_header(&ud_path);
    let state = storage.build_state();
    assert!(state.user_data_pool.is_corrupt());

    let mut rx = state.config_tx.subscribe();
    let status = invoke_server_fn::<server_fns::RestoreUserDataBackup>(state.clone(), "").await;
    assert_eq!(status, StatusCode::OK);

    let cleared = next_corruption_event(&mut rx).await;
    match cleared {
        ConfigEvent::CorruptionChanged {
            user_data_corrupt, ..
        } => assert!(!user_data_corrupt),
        _ => unreachable!(),
    }
    assert!(!state.user_data_pool.is_corrupt());
}
