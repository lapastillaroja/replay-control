use std::path::Path;
use std::time::Duration;

use crate::error::{Error, Result};
use crate::storage::StorageLocation;

/// Check whether the replay process has a libretro game core loaded.
///
/// Finds the replay PID via `pgrep -x replay`, then reads `/proc/{pid}/maps`
/// looking for a `libretro.so` mapping that is NOT `replay_libretro` (which is
/// the menu/frontend, not a game core).
///
/// Returns `true` if a game core is detected, `false` otherwise. Any errors
/// (process not found, permission denied, etc.) are treated as "not loaded".
fn check_game_loaded() -> bool {
    // Find the replay PID
    let output = match std::process::Command::new("pgrep")
        .args(["-x", "replay"])
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            eprintln!("[launch] health check: failed to run pgrep: {e}");
            return false;
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let pid = match stdout.lines().next().and_then(|l| l.trim().parse::<u32>().ok()) {
        Some(pid) => pid,
        None => {
            eprintln!("[launch] health check: replay process not found");
            return false;
        }
    };

    // Read the process memory maps
    let maps_path = format!("/proc/{pid}/maps");
    let maps = match std::fs::read_to_string(&maps_path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[launch] health check: failed to read {maps_path}: {e}");
            return false;
        }
    };

    // Look for a libretro core that is NOT the menu frontend (replay_libretro)
    for line in maps.lines() {
        if line.contains("libretro.so") && !line.contains("replay_libretro") {
            eprintln!("[launch] health check: game core detected in {maps_path}");
            return true;
        }
    }

    eprintln!("[launch] health check: no game core found in {maps_path}");
    false
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
/// NOTE: This is a reverse-engineered workaround — RePlayOS has no official
/// API for programmatic game launching. The autostart mechanism was designed
/// for boot-time auto-launch, not companion app integration. Check RePlayOS
/// changelogs for official remote launch support in future releases.
pub fn launch_game(storage: &StorageLocation, rom_path: &str) -> Result<()> {
    // Validate the ROM exists on disk
    let full_path = storage.root.join(rom_path.trim_start_matches('/'));
    if !full_path.exists() {
        return Err(Error::RomNotFound(full_path));
    }

    // Create the _autostart directory
    let autostart_dir = storage.roms_dir().join("_autostart");
    std::fs::create_dir_all(&autostart_dir).map_err(|e| Error::io(&autostart_dir, e))?;

    // Write the rom_path to autostart.auto
    let autostart_file = autostart_dir.join("autostart.auto");
    std::fs::write(&autostart_file, format!("{rom_path}\n"))
        .map_err(|e| Error::io(&autostart_file, e))?;

    // Restart the replay service
    let output = std::process::Command::new("systemctl")
        .args(["restart", "replay.service"])
        .output()
        .map_err(|e| Error::io(Path::new("systemctl"), e))?;

    if !output.status.success() {
        // Clean up on failure
        let _ = std::fs::remove_file(&autostart_file);
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
        eprintln!("[launch] autostart file cleaned up");

        // Wait 5 more seconds (10s total) for the game core to load
        std::thread::sleep(Duration::from_secs(5));

        if !check_game_loaded() {
            eprintln!("[launch] game core not loaded — restarting service to recover to menu");
            let result = std::process::Command::new("systemctl")
                .args(["restart", "replay.service"])
                .output();
            match result {
                Ok(o) if o.status.success() => {
                    eprintln!("[launch] recovery restart successful");
                }
                Ok(o) => {
                    let stderr = String::from_utf8_lossy(&o.stderr);
                    eprintln!("[launch] recovery restart failed: {stderr}");
                }
                Err(e) => {
                    eprintln!("[launch] recovery restart error: {e}");
                }
            }
        }
    });

    Ok(())
}
