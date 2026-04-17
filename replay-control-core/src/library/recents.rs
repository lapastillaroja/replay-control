use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::game_ref::GameRef;
use crate::storage::StorageLocation;
use crate::systems;

/// A recently played game entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentEntry {
    #[serde(flatten)]
    pub game: GameRef,
    /// The .rec marker filename
    pub marker_filename: String,
    /// Timestamp of last play (file modification time as unix epoch seconds)
    pub last_played: u64,
}

/// List recently played games, sorted by most recent first.
pub fn list_recents(storage: &StorageLocation) -> Result<Vec<RecentEntry>> {
    let recents_dir = storage.recents_dir();
    if !recents_dir.exists() {
        return Ok(Vec::new());
    }

    let mut recents = Vec::new();
    let entries = std::fs::read_dir(&recents_dir).map_err(|e| Error::io(&recents_dir, e))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let filename = entry.file_name().to_string_lossy().to_string();
        if !filename.ends_with(".rec") {
            continue;
        }

        let rom_path = match std::fs::read_to_string(&path) {
            Ok(content) => content.trim().to_string(),
            Err(_) => continue,
        };

        let Some(system) = systems::system_from_fav_filename(&filename) else {
            continue;
        };
        let system = system.to_string();

        let rom_filename = filename
            .split_once('@')
            .map(|(_, rest)| {
                rest.trim_end_matches(".rec")
                    .trim_end_matches(".fav")
                    .to_string()
            })
            .unwrap_or_default();

        let last_played = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        recents.push(RecentEntry {
            game: GameRef::new(&system, rom_filename, rom_path),
            marker_filename: filename,
            last_played,
        });
    }

    // Sort by most recently played
    recents.sort_by_key(|r| std::cmp::Reverse(r.last_played));

    // Deduplicate: a game launched via .fav symlink and directly produces
    // two .rec markers with the same underlying ROM. Keep only the most
    // recent entry per (system, rom_filename).
    let mut seen = std::collections::HashSet::new();
    recents.retain(|e| seen.insert((e.game.system.clone(), e.game.rom_filename.clone())));

    Ok(recents)
}

/// Create or update a recent entry for a game.
///
/// Creates `<system>@<rom_filename>.rec` in `_recent/` with the ROM path as content.
/// If the file already exists, its mtime is updated to the current time (overwrite).
pub fn add_recent(
    storage: &StorageLocation,
    system_folder: &str,
    rom_filename: &str,
    rom_path: &str,
) -> Result<()> {
    let recents_dir = storage.recents_dir();
    std::fs::create_dir_all(&recents_dir).map_err(|e| Error::io(&recents_dir, e))?;

    let rec_filename = format!("{system_folder}@{rom_filename}.rec");
    let rec_path = recents_dir.join(&rec_filename);

    // Write (or overwrite) the marker file.
    // Overwriting an existing file also updates its mtime.
    std::fs::write(&rec_path, format!("{rom_path}\n")).map_err(|e| Error::io(&rec_path, e))?;

    Ok(())
}

/// Get the most recently played game.
pub fn last_played(storage: &StorageLocation) -> Result<Option<RecentEntry>> {
    let recents = list_recents(storage)?;
    Ok(recents.into_iter().next())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::StorageKind;

    #[test]
    fn list_empty_recents() {
        let tmp = std::env::temp_dir().join(format!("replay-rec-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("roms/_recent")).unwrap();

        let storage = StorageLocation::from_path(tmp.clone(), StorageKind::Sd);
        let recents = list_recents(&storage).unwrap();
        assert!(recents.is_empty());
    }

    #[test]
    fn list_recents_parses_files() {
        let tmp = std::env::temp_dir().join(format!("replay-rec-test2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let recent_dir = tmp.join("roms/_recent");
        std::fs::create_dir_all(&recent_dir).unwrap();

        std::fs::write(
            recent_dir.join("sega_smd@Sonic.md.rec"),
            "/roms/sega_smd/Sonic.md",
        )
        .unwrap();

        let storage = StorageLocation::from_path(tmp.clone(), StorageKind::Sd);
        let recents = list_recents(&storage).unwrap();
        assert_eq!(recents.len(), 1);
        assert_eq!(recents[0].game.system, "sega_smd");
        assert_eq!(recents[0].game.rom_filename, "Sonic.md");
    }

    #[test]
    fn fav_suffix_stripped_from_rom_filename() {
        let tmp = std::env::temp_dir().join(format!("replay-rec-fav-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let recent_dir = tmp.join("roms/_recent");
        std::fs::create_dir_all(&recent_dir).unwrap();

        // Marker created when game is launched via .fav symlink
        std::fs::write(
            recent_dir.join("arcade_fbneo@chelnov.zip.fav.rec"),
            "/roms/arcade_fbneo/chelnov.zip",
        )
        .unwrap();

        let storage = StorageLocation::from_path(tmp.clone(), StorageKind::Sd);
        let recents = list_recents(&storage).unwrap();
        assert_eq!(recents.len(), 1);
        assert_eq!(recents[0].game.rom_filename, "chelnov.zip");
    }

    #[test]
    fn add_recent_creates_marker() {
        let tmp = std::env::temp_dir().join(format!("replay-rec-add-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("roms")).unwrap();

        let storage = StorageLocation::from_path(tmp.clone(), StorageKind::Sd);
        add_recent(&storage, "sega_smd", "Sonic.md", "/roms/sega_smd/Sonic.md").unwrap();

        // Verify the file was created with correct content
        let rec_path = tmp.join("roms/_recent/sega_smd@Sonic.md.rec");
        assert!(rec_path.exists());
        let content = std::fs::read_to_string(&rec_path).unwrap();
        assert_eq!(content, "/roms/sega_smd/Sonic.md\n");

        // Verify it shows up in list_recents
        let recents = list_recents(&storage).unwrap();
        assert_eq!(recents.len(), 1);
        assert_eq!(recents[0].game.system, "sega_smd");
        assert_eq!(recents[0].game.rom_filename, "Sonic.md");
    }

    #[test]
    fn add_recent_overwrites_existing() {
        let tmp = std::env::temp_dir().join(format!("replay-rec-overwrite-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let recent_dir = tmp.join("roms/_recent");
        std::fs::create_dir_all(&recent_dir).unwrap();

        // Write an old marker with stale content
        std::fs::write(
            recent_dir.join("sega_smd@Sonic.md.rec"),
            "/roms/sega_smd/old_path/Sonic.md\n",
        )
        .unwrap();

        let storage = StorageLocation::from_path(tmp.clone(), StorageKind::Sd);
        add_recent(&storage, "sega_smd", "Sonic.md", "/roms/sega_smd/Sonic.md").unwrap();

        let content = std::fs::read_to_string(recent_dir.join("sega_smd@Sonic.md.rec")).unwrap();
        assert_eq!(content, "/roms/sega_smd/Sonic.md\n");
    }

    #[test]
    fn add_recent_creates_directory() {
        let tmp = std::env::temp_dir().join(format!("replay-rec-mkdir-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        // Only create the base roms dir, not _recent
        std::fs::create_dir_all(tmp.join("roms")).unwrap();

        let storage = StorageLocation::from_path(tmp.clone(), StorageKind::Sd);
        add_recent(
            &storage,
            "arcade_dc",
            "ggx15.zip",
            "/roms/arcade_dc/Atomiswave/Horizontal Games/00 Clean Romset/ggx15.zip",
        )
        .unwrap();

        let rec_path = tmp.join("roms/_recent/arcade_dc@ggx15.zip.rec");
        assert!(rec_path.exists());
        let content = std::fs::read_to_string(&rec_path).unwrap();
        assert_eq!(
            content,
            "/roms/arcade_dc/Atomiswave/Horizontal Games/00 Clean Romset/ggx15.zip\n"
        );
    }

    #[test]
    fn fav_and_non_fav_deduplicated() {
        let tmp = std::env::temp_dir().join(format!("replay-rec-dedup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let recent_dir = tmp.join("roms/_recent");
        std::fs::create_dir_all(&recent_dir).unwrap();

        // Both markers for the same game
        std::fs::write(
            recent_dir.join("arcade_fbneo@chelnov.zip.rec"),
            "/roms/arcade_fbneo/chelnov.zip",
        )
        .unwrap();
        std::fs::write(
            recent_dir.join("arcade_fbneo@chelnov.zip.fav.rec"),
            "/roms/arcade_fbneo/chelnov.zip",
        )
        .unwrap();

        let storage = StorageLocation::from_path(tmp.clone(), StorageKind::Sd);
        let recents = list_recents(&storage).unwrap();
        // Should deduplicate to one entry
        assert_eq!(recents.len(), 1);
        assert_eq!(recents[0].game.rom_filename, "chelnov.zip");
    }
}
