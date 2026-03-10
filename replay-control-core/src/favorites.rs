use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::game_db;
use crate::game_ref::GameRef;
use crate::storage::StorageLocation;
use crate::systems;

/// A parsed favorite entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Favorite {
    #[serde(flatten)]
    pub game: GameRef,
    /// The .fav marker filename (e.g., "sega_smd@Sonic.md.fav")
    pub marker_filename: String,
    /// Subfolder within _favorites (empty string if at root)
    pub subfolder: String,
    /// Unix timestamp when the favorite was added (from file mtime)
    pub date_added: u64,
}

/// List all favorites, searching root and subfolders.
pub fn list_favorites(storage: &StorageLocation) -> Result<Vec<Favorite>> {
    let favs_dir = storage.favorites_dir();
    if !favs_dir.exists() {
        return Ok(Vec::new());
    }

    let mut favorites = Vec::new();
    collect_favorites(&favs_dir, &favs_dir, &mut favorites)?;

    // Deduplicate: when the same .fav exists at root AND in a subfolder
    // (from "keep originals" organize mode), prefer the subfolder version.
    let mut seen = std::collections::HashMap::new();
    for (i, fav) in favorites.iter().enumerate() {
        let entry = seen.entry(fav.marker_filename.clone()).or_insert(i);
        // Prefer the one with a non-empty subfolder (organized copy).
        if favorites[*entry].subfolder.is_empty() && !fav.subfolder.is_empty() {
            *entry = i;
        }
    }
    let keep: std::collections::HashSet<usize> = seen.into_values().collect();
    let mut favorites: Vec<_> = favorites
        .into_iter()
        .enumerate()
        .filter(|(i, _)| keep.contains(i))
        .map(|(_, f)| f)
        .collect();

    favorites.sort_by(|a, b| {
        a.game.system.cmp(&b.game.system).then(
            a.game
                .rom_filename
                .to_lowercase()
                .cmp(&b.game.rom_filename.to_lowercase()),
        )
    });

    Ok(favorites)
}

/// List favorites for a specific system.
pub fn list_favorites_for_system(
    storage: &StorageLocation,
    system_folder: &str,
) -> Result<Vec<Favorite>> {
    let all = list_favorites(storage)?;
    Ok(all
        .into_iter()
        .filter(|f| f.game.system == system_folder)
        .collect())
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
        let dir = storage.favorites_dir();
        std::fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
        dir
    };

    let fav_path = target_dir.join(&fav_filename);
    if fav_path.exists() {
        return Err(Error::FavoriteExists(fav_path));
    }

    std::fs::write(&fav_path, rom_relative_path).map_err(|e| Error::io(&fav_path, e))?;

    let subfolder = if grouped_by_system {
        system_folder.to_string()
    } else {
        String::new()
    };

    let date_added = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    Ok(Favorite {
        game: GameRef::new(system_folder, rom_filename, rom_relative_path.to_string()),
        marker_filename: fav_filename,
        subfolder,
        date_added,
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
/// Handles arbitrarily deep nesting and deduplicates by skipping files
/// that already exist at root.
pub fn flatten_favorites(storage: &StorageLocation) -> Result<usize> {
    flatten_favorites_deep(storage)
}

/// Criteria for organizing favorites into subfolders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrganizeCriteria {
    System,
    Genre,
    Players,
    Rating,
    Alphabetical,
}

/// Result of an organize or flatten operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrganizeResult {
    pub organized: usize,
    pub skipped: usize,
}

/// Organize favorites into subfolders based on the given criteria.
///
/// - `primary`: First level of subfolder nesting.
/// - `secondary`: Optional second level of nesting within the first.
/// - `keep_originals`: If true, copies files to subfolders while keeping root copies.
///   If false, moves files from root to subfolders.
pub fn organize_favorites(
    storage: &StorageLocation,
    primary: OrganizeCriteria,
    secondary: Option<OrganizeCriteria>,
    keep_originals: bool,
    ratings: Option<&std::collections::HashMap<(String, String), f64>>,
) -> Result<OrganizeResult> {
    let favs_dir = storage.favorites_dir();
    if !favs_dir.exists() {
        return Ok(OrganizeResult {
            organized: 0,
            skipped: 0,
        });
    }

    // First, flatten everything to root to start clean.
    flatten_favorites(storage)?;

    // Collect all .fav files from root.
    let entries: Vec<_> = std::fs::read_dir(&favs_dir)
        .map_err(|e| Error::io(&favs_dir, e))?
        .flatten()
        .filter(|e| e.path().is_file() && e.file_name().to_string_lossy().ends_with(".fav"))
        .collect();

    let mut organized = 0;
    let mut skipped = 0;

    for entry in &entries {
        let filename = entry.file_name().to_string_lossy().to_string();
        let src = entry.path();

        // Parse system and rom_filename from the .fav filename.
        let Some(system) = systems::system_from_fav_filename(&filename) else {
            skipped += 1;
            continue;
        };
        let rom_filename = filename
            .split_once('@')
            .map(|(_, rest)| rest.trim_end_matches(".fav"))
            .unwrap_or("");

        // Determine the subfolder path.
        let primary_folder = criteria_folder(primary, system, rom_filename, ratings);
        let subfolder = match secondary {
            Some(sec) => {
                let secondary_folder = criteria_folder(sec, system, rom_filename, ratings);
                PathBuf::from(&primary_folder).join(&secondary_folder)
            }
            None => PathBuf::from(&primary_folder),
        };

        let target_dir = favs_dir.join(&subfolder);
        std::fs::create_dir_all(&target_dir).map_err(|e| Error::io(&target_dir, e))?;

        let dest = target_dir.join(&filename);
        if dest.exists() {
            skipped += 1;
            continue;
        }

        if keep_originals {
            std::fs::copy(&src, &dest).map_err(|e| Error::io(&src, e))?;
        } else {
            std::fs::rename(&src, &dest).map_err(|e| Error::io(&src, e))?;
        }
        organized += 1;
    }

    Ok(OrganizeResult { organized, skipped })
}

/// Sanitize a string for use as a directory name.
/// Replaces `/` and other filesystem-unsafe characters with `-`.
fn sanitize_folder_name(name: &str) -> String {
    name.replace('/', "-").replace('\\', "-").replace(':', "-")
}

/// Determine the subfolder name for a favorite based on the given criteria.
fn criteria_folder(
    criteria: OrganizeCriteria,
    system: &str,
    rom_filename: &str,
    ratings: Option<&std::collections::HashMap<(String, String), f64>>,
) -> String {
    let raw = criteria_folder_raw(criteria, system, rom_filename, ratings);
    sanitize_folder_name(&raw)
}

fn criteria_folder_raw(
    criteria: OrganizeCriteria,
    system: &str,
    rom_filename: &str,
    ratings: Option<&std::collections::HashMap<(String, String), f64>>,
) -> String {
    let is_arcade =
        systems::find_system(system).is_some_and(|s| s.category == systems::SystemCategory::Arcade);

    match criteria {
        OrganizeCriteria::System => systems::find_system(system)
            .map(|s| s.display_name.to_string())
            .unwrap_or_else(|| system.to_string()),
        OrganizeCriteria::Genre => {
            if is_arcade {
                // Use arcade_db for genre resolution.
                let stem = rom_filename.strip_suffix(".zip").unwrap_or(rom_filename);
                let genre = crate::arcade_db::lookup_arcade_game(stem)
                    .map(|info| info.normalized_genre)
                    .unwrap_or("");
                if genre.is_empty() {
                    "Other".to_string()
                } else {
                    genre.to_string()
                }
            } else {
                // Use game_db for genre resolution.
                let stem = rom_filename
                    .rfind('.')
                    .map(|i| &rom_filename[..i])
                    .unwrap_or(rom_filename);
                let genre = game_db::lookup_game(system, stem)
                    .map(|e| e.game.normalized_genre)
                    .or_else(|| {
                        let normalized = game_db::normalize_filename(stem);
                        game_db::lookup_by_normalized_title(system, &normalized)
                            .map(|g| g.normalized_genre)
                    })
                    .unwrap_or("");
                if genre.is_empty() {
                    "Other".to_string()
                } else {
                    genre.to_string()
                }
            }
        }
        OrganizeCriteria::Players => {
            if is_arcade {
                // Use arcade_db for players resolution.
                let stem = rom_filename.strip_suffix(".zip").unwrap_or(rom_filename);
                let players = crate::arcade_db::lookup_arcade_game(stem)
                    .map(|info| info.players)
                    .unwrap_or(0);
                match players {
                    0 => "Unknown".to_string(),
                    1 => "1 Player".to_string(),
                    2 => "2 Players".to_string(),
                    n => format!("{n} Players"),
                }
            } else {
                let stem = rom_filename
                    .rfind('.')
                    .map(|i| &rom_filename[..i])
                    .unwrap_or(rom_filename);
                let players = game_db::lookup_game(system, stem)
                    .map(|e| e.game.players)
                    .or_else(|| {
                        let normalized = game_db::normalize_filename(stem);
                        game_db::lookup_by_normalized_title(system, &normalized).map(|g| g.players)
                    })
                    .unwrap_or(0);
                match players {
                    0 => "Unknown".to_string(),
                    1 => "1 Player".to_string(),
                    2 => "2 Players".to_string(),
                    n => format!("{n} Players"),
                }
            }
        }
        OrganizeCriteria::Rating => {
            let rating = ratings
                .and_then(|m| m.get(&(system.to_string(), rom_filename.to_string())))
                .copied();
            match rating {
                Some(r) if r >= 4.5 => "\u{2605}\u{2605}\u{2605}\u{2605}\u{2605}".to_string(),
                Some(r) if r >= 3.5 => "\u{2605}\u{2605}\u{2605}\u{2605}".to_string(),
                Some(r) if r >= 2.5 => "\u{2605}\u{2605}\u{2605}".to_string(),
                Some(r) if r >= 1.5 => "\u{2605}\u{2605}".to_string(),
                Some(r) if r >= 0.5 => "\u{2605}".to_string(),
                _ => "Not Rated".to_string(),
            }
        }
        OrganizeCriteria::Alphabetical => {
            let display = if is_arcade {
                let stem = rom_filename.strip_suffix(".zip").unwrap_or(rom_filename);
                crate::arcade_db::lookup_arcade_game(stem).map(|info| info.display_name)
            } else {
                game_db::game_display_name(system, rom_filename)
            };
            let display = display.unwrap_or(rom_filename);
            let first = display.chars().next().unwrap_or('#').to_ascii_uppercase();
            if first.is_ascii_alphabetic() {
                first.to_string()
            } else {
                "#".to_string()
            }
        }
    }
}

/// Flatten favorites: recursively move all .fav files from any subfolder back to root.
/// Skips duplicates (if file already exists at root). Removes empty subfolders.
pub fn flatten_favorites_deep(storage: &StorageLocation) -> Result<usize> {
    let favs_dir = storage.favorites_dir();
    if !favs_dir.exists() {
        return Ok(0);
    }

    let mut moved = 0;
    flatten_recursive(&favs_dir, &favs_dir, &mut moved)?;
    // Clean up empty directories.
    remove_empty_dirs(&favs_dir);
    Ok(moved)
}

fn flatten_recursive(dir: &Path, root: &Path, moved: &mut usize) -> Result<()> {
    let entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| Error::io(dir, e))?
        .flatten()
        .collect();

    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with('_') && !name.starts_with('.') {
                flatten_recursive(&path, root, moved)?;
            }
        } else if path.is_file()
            && path.extension().is_some_and(|e| e == "fav")
            && path.parent() != Some(root)
        {
            let dest = root.join(entry.file_name());
            if dest.exists() {
                // Duplicate — remove the subfolder copy.
                let _ = std::fs::remove_file(&path);
            } else {
                std::fs::rename(&path, &dest).map_err(|e| Error::io(&path, e))?;
                *moved += 1;
            }
        }
    }
    Ok(())
}

fn remove_empty_dirs(dir: &Path) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                if !name.starts_with('_') && !name.starts_with('.') {
                    remove_empty_dirs(&path);
                    let _ = std::fs::remove_dir(&path); // only succeeds if empty
                }
            }
        }
    }
}

/// Check if a ROM is favorited (searches root and all subfolders).
pub fn is_favorite(storage: &StorageLocation, system_folder: &str, rom_filename: &str) -> bool {
    let fav_filename = format!("{system_folder}@{rom_filename}.fav");
    let favs_dir = storage.favorites_dir();

    // Check root first (fast path).
    if favs_dir.join(&fav_filename).exists() {
        return true;
    }

    // Check system subfolder (common case).
    if favs_dir.join(system_folder).join(&fav_filename).exists() {
        return true;
    }

    // Search all subfolders recursively (for genre/players/alpha organization).
    find_fav_recursive(&favs_dir, &fav_filename)
}

fn find_fav_recursive(dir: &Path, fav_filename: &str) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with('_') && !name.starts_with('.') {
                if path.join(fav_filename).exists() || find_fav_recursive(&path, fav_filename) {
                    return true;
                }
            }
        }
    }
    false
}

fn collect_favorites(dir: &Path, favs_root: &Path, out: &mut Vec<Favorite>) -> Result<()> {
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

        let date_added = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        out.push(Favorite {
            game: GameRef::new(&system, rom_filename, rom_path),
            marker_filename: filename,
            subfolder,
            date_added,
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
        let tmp = std::env::temp_dir().join(format!("replay-fav-test-{}-{id}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let favs = tmp.join("roms/_favorites");
        std::fs::create_dir_all(&favs).unwrap();
        let storage = StorageLocation::from_path(tmp.clone(), StorageKind::Sd);
        (tmp, storage)
    }

    #[test]
    fn add_and_list_favorite() {
        let (_tmp, storage) = setup_test_storage();

        add_favorite(&storage, "sega_smd", "/roms/sega_smd/Sonic.md", false).unwrap();

        let favs = list_favorites(&storage).unwrap();
        assert_eq!(favs.len(), 1);
        assert_eq!(favs[0].game.system, "sega_smd");
        assert_eq!(favs[0].game.rom_filename, "Sonic.md");
        assert_eq!(favs[0].game.rom_path, "/roms/sega_smd/Sonic.md");
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
        add_favorite(
            &storage,
            "nintendo_nes",
            "/roms/nintendo_nes/Mario.nes",
            false,
        )
        .unwrap();

        let moved = group_by_system(&storage).unwrap();
        assert_eq!(moved, 2);

        // Verify files moved to subfolders
        assert!(
            storage
                .favorites_dir()
                .join("sega_smd/sega_smd@Sonic.md.fav")
                .exists()
        );
        assert!(
            storage
                .favorites_dir()
                .join("nintendo_nes/nintendo_nes@Mario.nes.fav")
                .exists()
        );

        let moved_back = flatten_favorites(&storage).unwrap();
        assert_eq!(moved_back, 2);

        // Verify files back at root
        assert!(
            storage
                .favorites_dir()
                .join("sega_smd@Sonic.md.fav")
                .exists()
        );
    }

    #[test]
    fn organize_by_system_moves_files() {
        let (_tmp, storage) = setup_test_storage();

        add_favorite(&storage, "sega_smd", "/roms/sega_smd/Sonic.md", false).unwrap();
        add_favorite(
            &storage,
            "nintendo_nes",
            "/roms/nintendo_nes/Mario.nes",
            false,
        )
        .unwrap();

        let result =
            organize_favorites(&storage, OrganizeCriteria::System, None, false, None).unwrap();
        assert_eq!(result.organized, 2);

        // Files should be in system-named subfolders.
        let favs = list_favorites(&storage).unwrap();
        assert_eq!(favs.len(), 2);
        assert!(favs.iter().all(|f| !f.subfolder.is_empty()));

        // Root should be empty of .fav files.
        let root_favs: Vec<_> = std::fs::read_dir(storage.favorites_dir())
            .unwrap()
            .flatten()
            .filter(|e| e.path().is_file() && e.file_name().to_string_lossy().ends_with(".fav"))
            .collect();
        assert_eq!(root_favs.len(), 0);
    }

    #[test]
    fn organize_keep_originals_duplicates() {
        let (_tmp, storage) = setup_test_storage();

        add_favorite(&storage, "sega_smd", "/roms/sega_smd/Sonic.md", false).unwrap();

        let result =
            organize_favorites(&storage, OrganizeCriteria::System, None, true, None).unwrap();
        assert_eq!(result.organized, 1);

        // File should exist both at root and in subfolder.
        let favs_dir = storage.favorites_dir();
        assert!(favs_dir.join("sega_smd@Sonic.md.fav").exists());

        // list_favorites should deduplicate: only 1 entry.
        let favs = list_favorites(&storage).unwrap();
        assert_eq!(favs.len(), 1);
        // Should prefer subfolder version.
        assert!(!favs[0].subfolder.is_empty());
    }

    #[test]
    fn flatten_after_organize() {
        let (_tmp, storage) = setup_test_storage();

        add_favorite(&storage, "sega_smd", "/roms/sega_smd/Sonic.md", false).unwrap();
        add_favorite(
            &storage,
            "nintendo_nes",
            "/roms/nintendo_nes/Mario.nes",
            false,
        )
        .unwrap();

        organize_favorites(&storage, OrganizeCriteria::System, None, false, None).unwrap();
        let moved = flatten_favorites(&storage).unwrap();
        assert_eq!(moved, 2);

        let favs = list_favorites(&storage).unwrap();
        assert_eq!(favs.len(), 2);
        assert!(favs.iter().all(|f| f.subfolder.is_empty()));
    }

    #[test]
    fn organize_alphabetical() {
        let (_tmp, storage) = setup_test_storage();

        add_favorite(&storage, "sega_smd", "/roms/sega_smd/Sonic.md", false).unwrap();
        add_favorite(
            &storage,
            "nintendo_nes",
            "/roms/nintendo_nes/Mario.nes",
            false,
        )
        .unwrap();

        let result =
            organize_favorites(&storage, OrganizeCriteria::Alphabetical, None, false, None)
                .unwrap();
        assert_eq!(result.organized, 2);

        let favs = list_favorites(&storage).unwrap();
        // One should be in "S/" and the other in "M/"
        let subfolders: Vec<_> = favs.iter().map(|f| f.subfolder.as_str()).collect();
        assert!(subfolders.contains(&"S"));
        assert!(subfolders.contains(&"M"));
    }

    #[test]
    fn organize_two_levels() {
        let (_tmp, storage) = setup_test_storage();

        add_favorite(&storage, "sega_smd", "/roms/sega_smd/Sonic.md", false).unwrap();

        let result = organize_favorites(
            &storage,
            OrganizeCriteria::Alphabetical,
            Some(OrganizeCriteria::System),
            false,
            None,
        )
        .unwrap();
        assert_eq!(result.organized, 1);

        let favs = list_favorites(&storage).unwrap();
        assert_eq!(favs.len(), 1);
        // Subfolder should be "S/{system_display_name}"
        assert!(favs[0].subfolder.starts_with("S"));
        assert!(favs[0].subfolder.contains(std::path::MAIN_SEPARATOR));
    }

    #[test]
    fn organize_system_with_slash_in_name() {
        let (_tmp, storage) = setup_test_storage();

        // Sega Mega Drive / Genesis has a `/` in the display name.
        add_favorite(&storage, "sega_smd", "/roms/sega_smd/Sonic.md", false).unwrap();
        // Arcade (Atomiswave/Naomi) also has `/`.
        add_favorite(
            &storage,
            "arcade_flycast",
            "/roms/arcade_flycast/game.zip",
            false,
        )
        .unwrap();

        let result =
            organize_favorites(&storage, OrganizeCriteria::System, None, false, None).unwrap();
        assert_eq!(result.organized, 2);

        // The subfolder should NOT create nested dirs from the `/`.
        let favs = list_favorites(&storage).unwrap();
        assert_eq!(favs.len(), 2);
        for fav in &favs {
            // Subfolder must be a single-level name (no path separator).
            assert!(
                !fav.subfolder.contains('/'),
                "Subfolder '{}' should not contain '/'",
                fav.subfolder
            );
        }

        // Flatten should bring them all back.
        let moved = flatten_favorites(&storage).unwrap();
        assert_eq!(moved, 2);
        let favs = list_favorites(&storage).unwrap();
        assert!(favs.iter().all(|f| f.subfolder.is_empty()));
    }

    #[test]
    fn flatten_deep_nesting() {
        let (_tmp, storage) = setup_test_storage();

        add_favorite(&storage, "sega_smd", "/roms/sega_smd/Sonic.md", false).unwrap();
        add_favorite(
            &storage,
            "nintendo_nes",
            "/roms/nintendo_nes/Mario.nes",
            false,
        )
        .unwrap();

        // Organize with 2 levels.
        organize_favorites(
            &storage,
            OrganizeCriteria::Alphabetical,
            Some(OrganizeCriteria::System),
            false,
            None,
        )
        .unwrap();

        // Verify they're nested.
        let favs = list_favorites(&storage).unwrap();
        assert!(
            favs.iter()
                .all(|f| f.subfolder.contains(std::path::MAIN_SEPARATOR))
        );

        // Flatten should handle deep nesting.
        let moved = flatten_favorites(&storage).unwrap();
        assert_eq!(moved, 2);
        let favs = list_favorites(&storage).unwrap();
        assert_eq!(favs.len(), 2);
        assert!(favs.iter().all(|f| f.subfolder.is_empty()));
    }

    #[test]
    fn duplicate_favorite_errors() {
        let (_tmp, storage) = setup_test_storage();

        add_favorite(&storage, "sega_smd", "/roms/sega_smd/Sonic.md", false).unwrap();
        let result = add_favorite(&storage, "sega_smd", "/roms/sega_smd/Sonic.md", false);
        assert!(result.is_err());
    }
}
