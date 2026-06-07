//! Game launching on RePlayOS, via the official local REST API.
//!
//! RePlayOS 1.7.3 remounts storage on every `replay.service` start. Launching
//! through the API avoids restarting `replay.service`, so the launch path
//! works the same way across SD, USB, NVMe, and NFS without provoking a
//! storage remount. The HTTP call itself goes through
//! [`crate::replay_api::ReplayApiClient`] — the app-layer launch server fn
//! owns the client (and its status machine); this module keeps only the
//! launch-specific pieces: ROM existence validation and the rom-path →
//! `(system, game_file)` split the `load_game` endpoint expects.

use crate::storage::StorageLocation;
use replay_control_core::error::{Error, Result};

/// Verify the ROM exists on the active storage before asking RePlayOS to
/// launch it — a clearer error than the API's generic "Game file not found".
pub async fn validate_rom_exists(storage: &StorageLocation, rom_path: &str) -> Result<()> {
    let full_path = storage.root.join(rom_path.trim_start_matches('/'));
    if tokio::fs::try_exists(&full_path).await.unwrap_or(false) {
        Ok(())
    } else {
        Err(Error::RomNotFound(full_path))
    }
}

/// Split a library rom path (`/roms/<system>/<subdirs...>/<file>`) into the
/// `(system, game_file)` pair `load_game` expects — `game_file` is relative to
/// the system folder and keeps any subdirectories.
pub fn launch_parts(rom_path: &str) -> Result<(&str, &str)> {
    let path = rom_path.trim_start_matches('/');
    let path = path.strip_prefix("roms/").unwrap_or(path);
    let Some((system, game_file)) = path.split_once('/') else {
        return Err(Error::Other(format!(
            "Cannot launch game: invalid ROM path {rom_path}"
        )));
    };
    if system.is_empty() || game_file.is_empty() {
        return Err(Error::Other(format!(
            "Cannot launch game: invalid ROM path {rom_path}"
        )));
    }
    Ok((system, game_file))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_parts_splits_system_and_relative_file() {
        let (system, game_file) =
            launch_parts("/roms/sega_smd/00 Clean Romset/Sonic 2.md").unwrap();
        assert_eq!(system, "sega_smd");
        assert_eq!(game_file, "00 Clean Romset/Sonic 2.md");

        // Subdir-less and prefix-less forms work too.
        let (system, game_file) = launch_parts("nintendo_nes/game.nes").unwrap();
        assert_eq!(system, "nintendo_nes");
        assert_eq!(game_file, "game.nes");
    }

    #[test]
    fn launch_parts_rejects_invalid_paths() {
        assert!(launch_parts("/roms/").is_err());
        assert!(launch_parts("just_a_file.md").is_err());
        assert!(launch_parts("/roms/system_only/").is_err());
    }
}
