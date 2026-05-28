#![cfg(feature = "ssr")]
//! Regression: reloading `replay.cfg` must never blank the in-memory config.
//!
//! The config-file watcher reloads `replay.cfg` into `AppState.config` (which
//! SSR and the UI read) whenever the file changes. RePlayOS rewrites
//! `replay.cfg` during the frontend restart a Wi-Fi/NFS save triggers, so a
//! reader can catch the file missing, empty, or half-written in that window.
//! The old reload adopted an empty config (`SystemConfig::parse("")`) in that
//! case, blanking the user's Wi-Fi/NFS settings until the next reload.
//! `reload_replay_config` now keeps the last-known-good config on any bad read.

use std::sync::atomic::{AtomicU32, Ordering};

use replay_control_app::api::AppState;

fn storage_with_config(cfg: &str) -> std::path::PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = std::env::temp_dir().join(format!("replay-cfgreload-{}-{id}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    for dir in &[
        "roms/_favorites",
        "roms/_recent",
        ".replay-control/media",
        "config",
    ] {
        std::fs::create_dir_all(tmp.join(dir)).unwrap();
    }
    std::fs::write(tmp.join("config/replay.cfg"), cfg).unwrap();
    tmp
}

#[tokio::test]
async fn reload_keeps_last_known_good_config_on_bad_read() {
    let tmp = storage_with_config("wifi_name = \"HomeNet\"\nsystem_storage = \"sd\"\n");
    let state = AppState::new(Some(tmp.to_string_lossy().into_owned()), None, None).unwrap();
    let cfg = tmp.join("config/replay.cfg");

    // Baseline: a valid reload adopts the wifi name.
    assert!(state.reload_replay_config());
    assert_eq!(
        state
            .replay_config
            .read()
            .unwrap()
            .as_ref()
            .and_then(|c| c.wifi_name()),
        Some("HomeNet")
    );

    // File vanishes (mid atomic-rename / storage briefly dropped).
    std::fs::remove_file(&cfg).unwrap();
    assert!(!state.reload_replay_config());
    assert_eq!(
        state
            .replay_config
            .read()
            .unwrap()
            .as_ref()
            .and_then(|c| c.wifi_name()),
        Some("HomeNet"),
        "missing config must not blank live wifi settings"
    );

    // Zero-byte file (caught mid-write).
    std::fs::write(&cfg, "").unwrap();
    assert!(!state.reload_replay_config());
    assert_eq!(
        state
            .replay_config
            .read()
            .unwrap()
            .as_ref()
            .and_then(|c| c.wifi_name()),
        Some("HomeNet"),
        "empty config must not blank live wifi settings"
    );

    // Malformed file (truncated mid-line / corrupt → parse error).
    std::fs::write(&cfg, "this_line_has_no_equals_sign").unwrap();
    assert!(!state.reload_replay_config());
    assert_eq!(
        state
            .replay_config
            .read()
            .unwrap()
            .as_ref()
            .and_then(|c| c.wifi_name()),
        Some("HomeNet"),
        "unparseable config must not blank live wifi settings"
    );

    // File restored with new content: adopt it.
    std::fs::write(&cfg, "wifi_name = \"OtherNet\"\nsystem_storage = \"sd\"\n").unwrap();
    assert!(state.reload_replay_config());
    assert_eq!(
        state
            .replay_config
            .read()
            .unwrap()
            .as_ref()
            .and_then(|c| c.wifi_name()),
        Some("OtherNet")
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

/// Off-device with `--storage-path` and no `replay.cfg`: the config is `None`
/// (never a fabricated empty one), the mode is `Standalone`, storage is a plain
/// `Folder`, and the app is serviceable.
#[tokio::test]
async fn local_without_config_is_none_folder_and_serviceable() {
    use replay_control_core::runtime_env::Mode;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = std::env::temp_dir().join(format!("replay-noconfig-{}-{id}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    for dir in &["roms/_favorites", "roms/_recent", ".replay-control/media"] {
        std::fs::create_dir_all(tmp.join(dir)).unwrap();
    }
    // Note: no config/replay.cfg is written.

    let state = AppState::new(Some(tmp.to_string_lossy().into_owned()), None, None).unwrap();

    assert!(
        state.replay_config.read().unwrap().is_none(),
        "no fabricated config"
    );
    assert!(matches!(state.mode, Mode::Standalone { .. }));
    assert_eq!(
        state.mode.standalone_root(),
        Some(tmp.as_path()),
        "Mode::Standalone carries the storage root supplied via --storage-path"
    );
    assert!(state.has_storage());
    assert!(state.is_serviceable());
    assert_eq!(
        state.storage().kind.as_str(),
        "folder",
        "no-config local storage is a plain Folder, not SD"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}
