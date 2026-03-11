use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::game_ref::GameRef;
use crate::rom_tags;
use crate::storage::StorageLocation;
use crate::systems::{self, System};

/// A ROM file on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RomEntry {
    #[serde(flatten)]
    pub game: GameRef,
    /// File size in bytes
    pub size_bytes: u64,
    /// Whether this is an M3U playlist file
    pub is_m3u: bool,
    /// Whether this ROM is in the user's favorites
    #[serde(default)]
    pub is_favorite: bool,
    /// Box art image URL (relative path under /media/), populated by the app layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub box_art_url: Option<String>,
    /// Arcade driver emulation status (Working/Imperfect/Preliminary/Unknown).
    /// Only populated for arcade systems.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub driver_status: Option<String>,
}

/// Summary of a system's ROM collection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemSummary {
    pub folder_name: String,
    pub display_name: String,
    pub manufacturer: String,
    pub category: String,
    pub game_count: usize,
    pub total_size_bytes: u64,
}

/// Scan all systems and return a summary of each.
pub fn scan_systems(storage: &StorageLocation) -> Vec<SystemSummary> {
    let roms_dir = storage.roms_dir();
    let mut summaries = Vec::new();

    for system in systems::visible_systems() {
        let system_dir = roms_dir.join(system.folder_name);
        let (count, size) = if system_dir.exists() {
            count_roms_recursive(&system_dir, system)
        } else {
            (0, 0)
        };

        summaries.push(SystemSummary {
            folder_name: system.folder_name.to_string(),
            display_name: system.display_name.to_string(),
            manufacturer: system.manufacturer.to_string(),
            category: format!("{:?}", system.category).to_lowercase(),
            game_count: count,
            total_size_bytes: size,
        });
    }

    // Sort: systems with games first, then alphabetically
    summaries.sort_by(|a, b| {
        let a_has = a.game_count > 0;
        let b_has = b.game_count > 0;
        b_has.cmp(&a_has).then(a.display_name.cmp(&b.display_name))
    });

    summaries
}

/// List ROM files for a specific system.
pub fn list_roms(storage: &StorageLocation, system_folder: &str) -> Result<Vec<RomEntry>> {
    let system = systems::find_system(system_folder)
        .ok_or_else(|| Error::SystemNotFound(system_folder.to_string()))?;

    let system_dir = storage.system_roms_dir(system_folder);
    if !system_dir.exists() {
        return Ok(Vec::new());
    }

    let mut roms = Vec::new();
    collect_roms_recursive(&system_dir, &storage.roms_dir(), system, &mut roms);

    // Sort by display name, then by tier (originals before hacks), then by region.
    roms.sort_by(|a, b| {
        let a_name = a
            .game
            .display_name
            .as_deref()
            .unwrap_or(&a.game.rom_filename);
        let b_name = b
            .game
            .display_name
            .as_deref()
            .unwrap_or(&b.game.rom_filename);
        let (a_tier, a_region) = rom_tags::classify(&a.game.rom_filename);
        let (b_tier, b_region) = rom_tags::classify(&b.game.rom_filename);
        a_name
            .to_lowercase()
            .cmp(&b_name.to_lowercase())
            .then(a_tier.cmp(&b_tier))
            .then(a_region.cmp(&b_region))
    });

    Ok(roms)
}

/// Mark each ROM entry's `is_favorite` flag using the favorites on disk.
/// Efficient: collects favorite filenames once, then checks via HashSet lookup.
pub fn mark_favorites(storage: &StorageLocation, system: &str, roms: &mut [RomEntry]) {
    let fav_set: std::collections::HashSet<String> =
        crate::favorites::list_favorites_for_system(storage, system)
            .unwrap_or_default()
            .into_iter()
            .map(|f| f.game.rom_filename)
            .collect();

    for rom in roms.iter_mut() {
        rom.is_favorite = fav_set.contains(&rom.game.rom_filename);
    }
}

/// Delete a ROM file.
pub fn delete_rom(storage: &StorageLocation, relative_path: &str) -> Result<()> {
    let full_path = storage.root.join(relative_path.trim_start_matches('/'));
    if !full_path.exists() {
        return Err(Error::RomNotFound(full_path));
    }
    std::fs::remove_file(&full_path).map_err(|e| Error::io(&full_path, e))
}

/// Rename a ROM file.
pub fn rename_rom(
    storage: &StorageLocation,
    relative_path: &str,
    new_filename: &str,
) -> Result<PathBuf> {
    let full_path = storage.root.join(relative_path.trim_start_matches('/'));
    if !full_path.exists() {
        return Err(Error::RomNotFound(full_path));
    }

    let new_path = full_path
        .parent()
        .unwrap_or(Path::new("/"))
        .join(new_filename);

    std::fs::rename(&full_path, &new_path).map_err(|e| Error::io(&full_path, e))?;
    Ok(new_path)
}

/// Detect duplicate ROMs across all systems by file size + name similarity.
pub fn find_duplicates(storage: &StorageLocation) -> Vec<(RomEntry, RomEntry)> {
    let roms_dir = storage.roms_dir();
    let mut all_roms: Vec<RomEntry> = Vec::new();

    for system in systems::visible_systems() {
        let system_dir = roms_dir.join(system.folder_name);
        if system_dir.exists() {
            collect_roms_recursive(&system_dir, &roms_dir, system, &mut all_roms);
        }
    }

    // Group by (filename, size) — exact duplicates
    let mut seen: std::collections::HashMap<(String, u64), RomEntry> =
        std::collections::HashMap::new();
    let mut duplicates = Vec::new();

    for rom in all_roms {
        let key = (rom.game.rom_filename.to_lowercase(), rom.size_bytes);
        if let Some(original) = seen.get(&key) {
            duplicates.push((original.clone(), rom));
        } else {
            seen.insert(key, rom);
        }
    }

    duplicates
}

fn count_roms_recursive(dir: &Path, system: &System) -> (usize, u64) {
    let mut count = 0usize;
    let mut size = 0u64;

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return (0, 0),
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Skip special folders
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with('_') {
                continue;
            }
            let (sub_count, sub_size) = count_roms_recursive(&path, system);
            count += sub_count;
            size += sub_size;
        } else if is_rom_file(&path, system) {
            count += 1;
            size += entry.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }

    (count, size)
}

fn collect_roms_recursive(dir: &Path, roms_root: &Path, system: &System, out: &mut Vec<RomEntry>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with('_') {
                continue;
            }
            collect_roms_recursive(&path, roms_root, system, out);
        } else if is_rom_file(&path, system) {
            let rom_filename = entry.file_name().to_string_lossy().to_string();
            let relative = path
                .strip_prefix(roms_root.parent().unwrap_or(Path::new("/")))
                .unwrap_or(&path);
            let rom_path = format!("/{}", relative.display());
            let size_bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
            let is_m3u = path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("m3u"));

            out.push(RomEntry {
                game: GameRef::new(system.folder_name, rom_filename, rom_path),
                size_bytes,
                is_m3u,
                is_favorite: false,
                box_art_url: None,
                driver_status: None,
            });
        }
    }
}

fn is_rom_file(path: &Path, system: &System) -> bool {
    let Some(ext) = path.extension() else {
        return false;
    };
    let ext_lower = ext.to_string_lossy().to_lowercase();

    // M3U files are always valid (multi-disc playlists)
    if ext_lower == "m3u" {
        return true;
    }

    system.extensions.iter().any(|e| *e == ext_lower)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn is_rom_file_matches_extensions() {
        let sys = systems::find_system("nintendo_nes").unwrap();
        assert!(is_rom_file(Path::new("game.nes"), sys));
        assert!(is_rom_file(Path::new("game.NES"), sys));
        assert!(!is_rom_file(Path::new("game.txt"), sys));
        assert!(is_rom_file(Path::new("multi.m3u"), sys));
    }

    #[test]
    fn scan_empty_storage() {
        let tmp = tempdir();
        fs::create_dir_all(tmp.join("roms")).unwrap();
        let storage = StorageLocation::from_path(tmp.clone(), crate::storage::StorageKind::Sd);
        let summaries = scan_systems(&storage);
        assert!(!summaries.is_empty());
        assert!(summaries.iter().all(|s| s.game_count == 0));
    }

    #[test]
    fn scan_with_roms() {
        let tmp = tempdir();
        let nes_dir = tmp.join("roms/nintendo_nes");
        fs::create_dir_all(&nes_dir).unwrap();
        fs::write(nes_dir.join("game1.nes"), "data").unwrap();
        fs::write(nes_dir.join("game2.nes"), "data").unwrap();
        fs::write(nes_dir.join("readme.txt"), "not a rom").unwrap();

        let storage = StorageLocation::from_path(tmp.clone(), crate::storage::StorageKind::Sd);
        let summaries = scan_systems(&storage);

        let nes = summaries
            .iter()
            .find(|s| s.folder_name == "nintendo_nes")
            .unwrap();
        assert_eq!(nes.game_count, 2);

        // NES should be sorted first (has games)
        assert!(summaries[0].game_count > 0 || summaries.iter().all(|s| s.game_count == 0));
    }

    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn tempdir() -> PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("replay-test-{}-{id}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
