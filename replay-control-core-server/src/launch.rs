use std::path::Path;
use std::time::Duration;

use crate::storage::StorageLocation;
use replay_control_core::error::{Error, Result};

/// Check whether the replay process has a libretro game core loaded.
/// Returns `false` for any failure (process not found, unreadable maps, etc.).
#[cfg(target_os = "linux")]
fn check_game_loaded() -> bool {
    use crate::replay_proc::{find_replay_pid, maps_have_active_game_core};

    let Some(pid) = find_replay_pid() else {
        tracing::debug!("health check: replay process not found");
        return false;
    };
    let maps_path = format!("/proc/{pid}/maps");
    let maps = match std::fs::read_to_string(&maps_path) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!("health check: failed to read {maps_path}: {e}");
            return false;
        }
    };
    let loaded = maps_have_active_game_core(&maps);
    if loaded {
        tracing::info!("health check: game core detected in {maps_path}");
    } else {
        tracing::warn!("health check: no game core found in {maps_path}");
    }
    loaded
}

#[cfg(not(target_os = "linux"))]
fn check_game_loaded() -> bool {
    // Non-Linux dev hosts: no /proc, treat as "loaded" so the recovery path
    // doesn't fire.
    true
}

/// Launch a game on RePlayOS via the autostart + systemctl restart mechanism.
///
/// Writes the `rom_path` to `_autostart/autostart.auto`, restarts the
/// `replay.service`, then spawns a background thread that:
///
/// 1. Waits 5 seconds for the replay binary to boot and read the autostart file
/// 2. Deletes the autostart file (cleanup)
/// 3. Waits 5 more seconds (10s total from restart) for the game to load
/// 4. Checks if a libretro game core is loaded via `/proc/PID/maps`
/// 5. If no game core is found, restarts the service cleanly (boots to menu)
///
/// The health check recovery exists because some cores (notably Flycast for
/// arcade_dc systems like Atomiswave/Naomi) can fail to load silently, leaving
/// the user stuck on a blank screen. By detecting the failure and restarting
/// to the menu, we ensure the user can always recover without manual
/// intervention. This also future-proofs against any core crash at launch time.
///
/// NOTE: This uses the autostart mechanism documented in RePlayOS — there
/// is no official API for programmatic game launching. The autostart mechanism
/// was designed for boot-time auto-launch, not companion app integration.
/// Check RePlayOS changelogs for official remote launch support in future
/// releases.
pub async fn launch_game(storage: &StorageLocation, rom_path: &str) -> Result<()> {
    // Validate the ROM exists on disk
    let full_path = storage.root.join(rom_path.trim_start_matches('/'));
    if !tokio::fs::try_exists(&full_path).await.unwrap_or(false) {
        return Err(Error::RomNotFound(full_path));
    }

    // Create the _autostart directory
    let autostart_dir = storage.roms_dir().join("_autostart");
    tokio::fs::create_dir_all(&autostart_dir)
        .await
        .map_err(|e| Error::io(&autostart_dir, e))?;

    // Write the rom_path to autostart.auto
    let autostart_file = autostart_dir.join("autostart.auto");
    tokio::fs::write(&autostart_file, format!("{rom_path}\n"))
        .await
        .map_err(|e| Error::io(&autostart_file, e))?;

    // Restart the replay service
    let output = tokio::process::Command::new("systemctl")
        .args(["restart", "replay.service"])
        .output()
        .await
        .map_err(|e| Error::io(Path::new("systemctl"), e))?;

    if !output.status.success() {
        // Clean up on failure
        let _ = tokio::fs::remove_file(&autostart_file).await;
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Other(format!(
            "Failed to restart replay service: {stderr}"
        )));
    }

    // Spawn background thread: cleanup autostart file, then health check.
    let cleanup_path = autostart_file.clone();
    std::thread::spawn(move || {
        // Wait 5s for the replay binary to boot and read the autostart file
        std::thread::sleep(Duration::from_secs(5));
        let _ = std::fs::remove_file(&cleanup_path);
        tracing::debug!("autostart file cleaned up");

        // Wait 5 more seconds (10s total) for the game core to load
        std::thread::sleep(Duration::from_secs(5));

        if !check_game_loaded() {
            tracing::warn!("game core not loaded -- restarting service to recover to menu");
            let result = std::process::Command::new("systemctl")
                .args(["restart", "replay.service"])
                .output();
            match result {
                Ok(o) if o.status.success() => {
                    tracing::info!("recovery restart successful");
                }
                Ok(o) => {
                    let stderr = String::from_utf8_lossy(&o.stderr);
                    tracing::error!("recovery restart failed: {stderr}");
                }
                Err(e) => {
                    tracing::error!("recovery restart error: {e}");
                }
            }
        }
    });

    Ok(())
}
