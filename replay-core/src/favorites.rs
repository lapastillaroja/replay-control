use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::storage::StorageLocation;
use crate::systems;

/// A parsed favorite entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Favorite {
    /// The .fav filename (e.g., "sega_smd@Sonic.md.fav")
    pub filename: String,
    /// System folder name extracted from the filename
    pub system: String,
    /// Display name of the system
    pub system_display: String,
    /// ROM filename extracted from the fav filename
    pub rom_filename: String,
    /// Full ROM path stored inside the .fav file
    pub rom_path: String,
    /// Subfolder within _favorites (empty string if at root)
    pub subfolder: String,
}

/// List all favorites, searching root and subfolders.
pub fn list_favorites(storage: &StorageLocation) -> Result<Vec<Favorite>> {
    let favs_dir = storage.favorites_dir();
    if !favs_dir.exists() {
        return Ok(Vec::new());
    }

    let mut favorites = Vec::new();
    collect_favorites(&favs_dir, &favs_dir, &mut favorites)?;

    favorites.sort_by(|a, b| {
        a.system
            .cmp(&b.system)
            .then(a.rom_filename.to_lowercase().cmp(&b.rom_filename.to_lowercase()))
    });

    Ok(favorites)
}

/// List favorites for a specific system.
pub fn list_favorites_for_system(
    storage: &StorageLocation,
    system_folder: &str,
) -> Result<Vec<Favorite>> {
    let all = list_favorites(storage)?;
    Ok(all.into_iter().filter(|f| f.system == system_folder).collect())
}

/// Add a ROM to favorites.
pub fn add_favorite(
    storage: &StorageLocation,
    system_folder: &str,
    rom_relative_path: &str,
    grouped_by_system: bool,
) -> Result<Favorite> {
    let rom_filename = Path::new(rom_relative_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let fav_filename = format!("{system_folder}@{rom_filename}.fav");

    let target_dir = if grouped_by_system {
        let dir = storage.favorites_dir().join(system_folder);
        std::fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
        dir
    } else {
        storage.favorites_dir()
    };

    let fav_path = target_dir.join(&fav_filename);
    if fav_path.exists() {
        return Err(Error::FavoriteExists(fav_path));
    }

    std::fs::write(&fav_path, rom_relative_path).map_err(|e| Error::io(&fav_path, e))?;

    let system_display = systems::find_system(system_folder)
        .map(|s| s.display_name.to_string())
        .unwrap_or_else(|| system_folder.to_string());

    let subfolder = if grouped_by_system {
        system_folder.to_string()
    } else {
        String::new()
    };

    Ok(Favorite {
        filename: fav_filename,
        system: system_folder.to_string(),
        system_display,
        rom_filename,
        rom_path: rom_relative_path.to_string(),
        subfolder,
    })
}

/// Remove a favorite by its .fav filename and optional subfolder.
pub fn remove_favorite(
    storage: &StorageLocation,
    fav_filename: &str,
    subfolder: Option<&str>,
) -> Result<()> {
    let fav_path = match subfolder {
        Some(sub) if !sub.is_empty() => storage.favorites_dir().join(sub).join(fav_filename),
        _ => storage.favorites_dir().join(fav_filename),
    };

    if !fav_path.exists() {
        return Err(Error::RomNotFound(fav_path));
    }

    std::fs::remove_file(&fav_path).map_err(|e| Error::io(&fav_path, e))
}

/// Organize favorites by system: move all .fav files from root into
/// system-named subfolders.
pub fn group_by_system(storage: &StorageLocation) -> Result<usize> {
    let favs_dir = storage.favorites_dir();
    if !favs_dir.exists() {
        return Ok(0);
    }

    let mut moved = 0;
    let entries: Vec<_> = std::fs::read_dir(&favs_dir)
        .map_err(|e| Error::io(&favs_dir, e))?
        .flatten()
        .collect();

    for entry in entries {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let filename = entry.file_name().to_string_lossy().to_string();
        if !filename.ends_with(".fav") {
            continue;
        }

        let Some(system_name) = systems::system_from_fav_filename(&filename) else {
            continue;
        };

        let system_dir = favs_dir.join(system_name);
        std::fs::create_dir_all(&system_dir).map_err(|e| Error::io(&system_dir, e))?;

        let dest = system_dir.join(&filename);
        std::fs::rename(&path, &dest).map_err(|e| Error::io(&path, e))?;
        moved += 1;
    }

    Ok(moved)
}

/// Flatten favorites: move all .fav files from subfolders back to root.
pub fn flatten_favorites(storage: &StorageLocation) -> Result<usize> {
    let favs_dir = storage.favorites_dir();
    if !favs_dir.exists() {
        return Ok(0);
    }

    let mut moved = 0;
    let entries: Vec<_> = std::fs::read_dir(&favs_dir)
        .map_err(|e| Error::io(&favs_dir, e))?
        .flatten()
        .collect();

    for entry in entries {
        let sub_dir = entry.path();
        if !sub_dir.is_dir() {
            continue;
        }

        // Skip non-system directories
        let dir_name = entry.file_name().to_string_lossy().to_string();
        if dir_name.starts_with('_') || dir_name.starts_with('.') {
            continue;
        }

        let sub_entries: Vec<_> = std::fs::read_dir(&sub_dir)
            .map_err(|e| Error::io(&sub_dir, e))?
            .flatten()
            .collect();

        for sub_entry in sub_entries {
            let path = sub_entry.path();
            if path.is_file() && path.extension().is_some_and(|e| e == "fav") {
                let dest = favs_dir.join(sub_entry.file_name());
                std::fs::rename(&path, &dest).map_err(|e| Error::io(&path, e))?;
                moved += 1;
            }
        }

        // Remove empty subfolder
        let _ = std::fs::remove_dir(&sub_dir);
    }

    Ok(moved)
}

/// Check if a ROM is favorited.
pub fn is_favorite(storage: &StorageLocation, system_folder: &str, rom_filename: &str) -> bool {
    let fav_filename = format!("{system_folder}@{rom_filename}.fav");
    let favs_dir = storage.favorites_dir();

    // Check root
    if favs_dir.join(&fav_filename).exists() {
        return true;
    }

    // Check system subfolder
    favs_dir.join(system_folder).join(&fav_filename).exists()
}

fn collect_favorites(
    dir: &Path,
    favs_root: &Path,
    out: &mut Vec<Favorite>,
) -> Result<()> {
    let entries = std::fs::read_dir(dir).map_err(|e| Error::io(dir, e))?;

    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with('_') && !name.starts_with('.') {
                collect_favorites(&path, favs_root, out)?;
            }
            continue;
        }

        let filename = entry.file_name().to_string_lossy().to_string();
        if !filename.ends_with(".fav") {
            continue;
        }

        let rom_path = std::fs::read_to_string(&path)
            .map_err(|e| Error::io(&path, e))?
            .trim()
            .to_string();

        let Some(system) = systems::system_from_fav_filename(&filename) else {
            continue;
        };
        let system = system.to_string();

        // Extract ROM filename: everything between '@' and '.fav'
        let rom_filename = filename
            .split_once('@')
            .map(|(_, rest)| rest.trim_end_matches(".fav").to_string())
            .unwrap_or_default();

        let subfolder = path
            .parent()
            .and_then(|p| p.strip_prefix(favs_root).ok())
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let system_display = systems::find_system(&system)
            .map(|s| s.display_name.to_string())
            .unwrap_or_else(|| system.clone());

        out.push(Favorite {
            filename,
            system,
            system_display,
            rom_filename,
            rom_path,
            subfolder,
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::StorageKind;
    use std::path::PathBuf;

    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn setup_test_storage() -> (PathBuf, StorageLocation) {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp = std::env::temp_dir().join(format!(
            "replay-fav-test-{}-{id}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        let favs = tmp.join("roms/_favorites");
        std::fs::create_dir_all(&favs).unwrap();
        let storage = StorageLocation::from_path(tmp.clone(), StorageKind::Sd);
        (tmp, storage)
    }

    #[test]
    fn add_and_list_favorite() {
        let (_tmp, storage) = setup_test_storage();

        add_favorite(
            &storage,
            "sega_smd",
            "/roms/sega_smd/Sonic.md",
            false,
        )
        .unwrap();

        let favs = list_favorites(&storage).unwrap();
        assert_eq!(favs.len(), 1);
        assert_eq!(favs[0].system, "sega_smd");
        assert_eq!(favs[0].rom_filename, "Sonic.md");
        assert_eq!(favs[0].rom_path, "/roms/sega_smd/Sonic.md");
    }

    #[test]
    fn test_remove_favorite() {
        let (_tmp, storage) = setup_test_storage();

        add_favorite(&storage, "sega_smd", "/roms/sega_smd/Sonic.md", false).unwrap();
        assert!(is_favorite(&storage, "sega_smd", "Sonic.md"));

        super::remove_favorite(&storage, "sega_smd@Sonic.md.fav", None).unwrap();
        assert!(!is_favorite(&storage, "sega_smd", "Sonic.md"));
    }

    #[test]
    fn group_and_flatten() {
        let (_tmp, storage) = setup_test_storage();

        add_favorite(&storage, "sega_smd", "/roms/sega_smd/Sonic.md", false).unwrap();
        add_favorite(&storage, "nintendo_nes", "/roms/nintendo_nes/Mario.nes", false).unwrap();

        let moved = group_by_system(&storage).unwrap();
        assert_eq!(moved, 2);

        // Verify files moved to subfolders
        assert!(storage
            .favorites_dir()
            .join("sega_smd/sega_smd@Sonic.md.fav")
            .exists());
        assert!(storage
            .favorites_dir()
            .join("nintendo_nes/nintendo_nes@Mario.nes.fav")
            .exists());

        let moved_back = flatten_favorites(&storage).unwrap();
        assert_eq!(moved_back, 2);

        // Verify files back at root
        assert!(storage
            .favorites_dir()
            .join("sega_smd@Sonic.md.fav")
            .exists());
    }

    #[test]
    fn duplicate_favorite_errors() {
        let (_tmp, storage) = setup_test_storage();

        add_favorite(&storage, "sega_smd", "/roms/sega_smd/Sonic.md", false).unwrap();
        let result = add_favorite(&storage, "sega_smd", "/roms/sega_smd/Sonic.md", false);
        assert!(result.is_err());
    }
}
