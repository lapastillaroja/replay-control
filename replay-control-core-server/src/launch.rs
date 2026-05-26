use std::path::PathBuf;
use std::time::Duration;

use crate::storage::StorageLocation;
use replay_control_core::error::{Error, Result};

/// How often the post-launch watcher polls `/proc/<replay-pid>/maps`.
const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// How long the watcher will wait for the binary to reach a terminal state
/// (game core mapped, or back at the menu) before giving up. Chosen to cover
/// slow autostart reads on large libraries — observed up to ~7s on 100k-ROM
/// Pi 5 setups, with headroom for slower configurations.
const POLL_TIMEOUT: Duration = Duration::from_secs(30);

/// Launch a game on RePlayOS via the autostart + systemctl restart mechanism.
///
/// Writes the `rom_path` to `_autostart/autostart.auto`, restarts
/// `replay.service`, then spawns a background watcher that polls
/// `/proc/<replay-pid>/maps` until one of:
///
/// - A libretro game core is mapped → success; delete the autostart file.
/// - The binary is alive but only the menu/frontend is mapped, for the full
///   timeout window → autostart was not picked up; delete the file. No
///   recovery restart is needed because the binary has already recovered
///   itself to the menu.
/// - No replay process is alive at the timeout → genuinely hung (e.g. a core
///   that fails silently and leaves the screen black); delete the file and
///   restart `replay.service` so the user gets back to the menu.
///
/// `autostart.auto` must not be deleted on a fixed short timer: on large
/// libraries the binary's read of the file can take several seconds, and
/// removing it before that read causes the launch to silently fall back to
/// the menu.
///
/// NOTE: This uses the autostart mechanism documented in RePlayOS — there
/// is no official API for programmatic game launching. The autostart
/// mechanism was designed for boot-time auto-launch, not companion app
/// integration. Check RePlayOS changelogs for official remote launch support
/// in future releases.
pub async fn launch_game(storage: &StorageLocation, rom_path: &str) -> Result<()> {
    tracing::info!(rom = %rom_path, "launching game via autostart");

    // Validate the ROM exists on disk
    let full_path = storage.root.join(rom_path.trim_start_matches('/'));
    if !tokio::fs::try_exists(&full_path).await.unwrap_or(false) {
        return Err(Error::RomNotFound(full_path));
    }

    // Create the _autostart directory
    let autostart_file = autostart_path(storage);
    let autostart_dir = autostart_file
        .parent()
        .expect("autostart path always has a parent");
    tokio::fs::create_dir_all(autostart_dir)
        .await
        .map_err(|e| Error::io(autostart_dir, e))?;

    // Write the rom_path to autostart.auto
    tokio::fs::write(&autostart_file, format!("{rom_path}\n"))
        .await
        .map_err(|e| Error::io(&autostart_file, e))?;

    if let Err(error) = crate::replay_service::restart_async().await {
        let _ = clear_autostart(storage);
        return Err(error);
    }

    std::thread::spawn({
        let storage = storage.clone();
        move || watch_launch(storage)
    });

    Ok(())
}

/// Path to the autostart file RePlayOS reads on `replay.service` start.
fn autostart_path(storage: &StorageLocation) -> PathBuf {
    storage.roms_dir().join("_autostart").join("autostart.auto")
}

/// Remove `_autostart/autostart.auto` so a subsequent `replay.service` restart
/// returns to the menu instead of relaunching the last game.
///
/// Idempotent: a missing file is success, since that already means nothing will
/// be auto-launched. The stop-game path must call this *before* restarting,
/// because between a launch and its watcher cleanup the file is still present
/// and a bare restart would re-read it and relaunch the same game.
pub fn clear_autostart(storage: &StorageLocation) -> Result<()> {
    let autostart_file = autostart_path(storage);
    match std::fs::remove_file(&autostart_file) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::io(&autostart_file, e)),
    }
}

/// Background watcher: polls the replay binary's state until it lands in a
/// terminal one, then cleans up `autostart.auto` and (only if the binary is
/// hung) triggers a recovery restart.
#[cfg(target_os = "linux")]
fn watch_launch(storage: StorageLocation) {
    use crate::replay_proc::{ReplayState, current_replay_state};

    let start = std::time::Instant::now();
    let mut cached_pid: Option<u32> = None;

    let timed_out_state = loop {
        let state = current_replay_state(cached_pid);
        match &state {
            ReplayState::Playing { pid, .. } => {
                tracing::info!(
                    pid = *pid,
                    "launch: game core mapped, cleaning up autostart"
                );
                let _ = clear_autostart(&storage);
                return;
            }
            ReplayState::Menu { pid } => cached_pid = Some(*pid),
            ReplayState::NotRunning => cached_pid = None,
        }
        if start.elapsed() >= POLL_TIMEOUT {
            break state;
        }
        std::thread::sleep(POLL_INTERVAL);
    };

    // Timed out without seeing a game core. Always clean up so we don't
    // re-launch a stale ROM on the next boot.
    let _ = clear_autostart(&storage);

    match timed_out_state {
        ReplayState::Menu { pid } => {
            tracing::info!(
                pid,
                "launch: binary stayed on menu, autostart not picked up -- no recovery needed"
            );
        }
        ReplayState::NotRunning => {
            tracing::warn!(
                "launch: no replay process after {}s -- restarting to recover",
                POLL_TIMEOUT.as_secs()
            );
            match crate::replay_service::restart() {
                Ok(()) => tracing::info!("recovery restart successful"),
                Err(e) => tracing::error!("recovery restart failed: {e}"),
            }
        }
        ReplayState::Playing { .. } => unreachable!("Playing returns early from the poll loop"),
    }
}

#[cfg(not(target_os = "linux"))]
fn watch_launch(_storage: StorageLocation) {
    // Non-Linux dev hosts: no /proc to poll. The autostart file is left in
    // place since there's no real replay binary to drive cleanup; tests
    // that need a clean tree handle this themselves.
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::StorageKind;

    #[test]
    fn clear_autostart_removes_file_and_is_idempotent() {
        let root =
            std::env::temp_dir().join(format!("replay-clear-autostart-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let storage = StorageLocation::from_path(root.clone(), StorageKind::Sd);

        let autostart = autostart_path(&storage);
        std::fs::create_dir_all(autostart.parent().unwrap()).unwrap();
        std::fs::write(&autostart, "roms/nes/game.nes\n").unwrap();

        // First clear removes the file.
        clear_autostart(&storage).unwrap();
        assert!(!autostart.exists());

        // Second clear on a now-missing file still succeeds (idempotent), so a
        // stop racing the launch watcher never fails on an already-gone file.
        clear_autostart(&storage).unwrap();

        let _ = std::fs::remove_dir_all(&root);
    }
}
