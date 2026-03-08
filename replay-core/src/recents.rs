use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::storage::StorageLocation;
use crate::systems;

/// A recently played game entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentEntry {
    /// The .rec filename
    pub filename: String,
    /// System folder name
    pub system: String,
    /// Display name of the system
    pub system_display: String,
    /// ROM filename
    pub rom_filename: String,
    /// Full ROM path stored inside the .rec file
    pub rom_path: String,
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
            .map(|(_, rest)| rest.trim_end_matches(".rec").to_string())
            .unwrap_or_default();

        let last_played = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let system_display = systems::find_system(&system)
            .map(|s| s.display_name.to_string())
            .unwrap_or_else(|| system.clone());

        recents.push(RecentEntry {
            filename,
            system,
            system_display,
            rom_filename,
            rom_path,
            last_played,
        });
    }

    // Sort by most recently played
    recents.sort_by(|a, b| b.last_played.cmp(&a.last_played));

    Ok(recents)
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
        assert_eq!(recents[0].system, "sega_smd");
        assert_eq!(recents[0].rom_filename, "Sonic.md");
    }
}
