use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::arcade_db::{self, ArcadeGameInfo};
use crate::game_db::{self, CanonicalGame};
use crate::game_ref::GameRef;
use crate::storage::StorageLocation;
use replay_control_core::error::{Error, Result};
use replay_control_core::systems;

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
pub async fn list_favorites(storage: &StorageLocation) -> Result<Vec<Favorite>> {
    let favs_dir = storage.favorites_dir();
    if !favs_dir.exists() {
        return Ok(Vec::new());
    }

    let mut favorites = Vec::new();
    collect_favorites(&favs_dir, &favs_dir, &mut favorites).await?;

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

    // Sort by date added (newest first) so the favorites page shows
    // the most recently favorited games at the top, regardless of subfolder.
    favorites.sort_by_key(|f| std::cmp::Reverse(f.date_added));

    Ok(favorites)
}

/// List favorites for a specific system.
pub async fn list_favorites_for_system(
    storage: &StorageLocation,
    system_folder: &str,
) -> Result<Vec<Favorite>> {
    let all = list_favorites(storage).await?;
    Ok(all
        .into_iter()
        .filter(|f| f.game.system == system_folder)
        .collect())
}

/// Add a ROM to favorites.
pub async fn add_favorite(
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
        storage.favorites_dir().join(system_folder)
    } else {
        storage.favorites_dir()
    };

    let fav_path = target_dir.join(&fav_filename);
    write_favorite_marker(target_dir, fav_path.clone(), rom_relative_path.to_string()).await?;

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
        game: GameRef::new(system_folder, rom_filename, rom_relative_path.to_string()).await,
        marker_filename: fav_filename,
        subfolder,
        date_added,
    })
}

/// mkdir + write-unless-exists on the blocking pool. Factored out because the
/// sync variant of `add_favorite` (below) uses the same steps inline.
async fn write_favorite_marker(
    target_dir: PathBuf,
    fav_path: PathBuf,
    contents: String,
) -> Result<()> {
    let work = move || -> Result<()> {
        std::fs::create_dir_all(&target_dir).map_err(|e| Error::io(&target_dir, e))?;
        if fav_path.exists() {
            return Err(Error::FavoriteExists(fav_path));
        }
        std::fs::write(&fav_path, contents).map_err(|e| Error::io(&fav_path, e))
    };
    {
        tokio::task::spawn_blocking(work)
            .await
            .map_err(|e| Error::Other(format!("add_favorite task panicked: {e}")))?
    }
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

/// Remove a .fav file from all locations (root + all subfolders, recursively).
/// Used when the caller doesn't know which subfolder the file is in.
/// Returns the number of files removed.
pub fn remove_favorite_everywhere(storage: &StorageLocation, fav_filename: &str) -> Result<usize> {
    let favs_dir = storage.favorites_dir();
    let mut removed = 0;

    fn walk(dir: &Path, fav_filename: &str, removed: &mut usize) -> Result<()> {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return Ok(()),
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, fav_filename, removed)?;
            } else if entry.file_name().to_string_lossy() == fav_filename {
                std::fs::remove_file(&path).map_err(|e| Error::io(&path, e))?;
                *removed += 1;
            }
        }
        Ok(())
    }

    walk(&favs_dir, fav_filename, &mut removed)?;

    if removed == 0 {
        return Err(Error::RomNotFound(favs_dir.join(fav_filename)));
    }
    Ok(removed)
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
    Developer,
}

/// Result of an organize or flatten operation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct OrganizeResult {
    pub organized: usize,
    pub skipped: usize,
}

/// (src_path, fav_filename, system_folder, rom_filename) for each parsed
/// favorite marker found during the organize scan.
type ParsedFavEntry = (std::path::PathBuf, String, String, String);

/// Organize favorites into subfolders based on the given criteria.
///
/// - `primary`: First level of subfolder nesting.
/// - `secondary`: Optional second level of nesting within the first.
/// - `keep_originals`: If true, copies files to subfolders while keeping root copies.
///   If false, moves files from root to subfolders.
pub async fn organize_favorites(
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

    let (parsed_entries, skipped_from_scan): (Vec<ParsedFavEntry>, usize) = {
        let storage = storage.clone();
        let favs_dir = favs_dir.clone();
        let scan = move || -> Result<(Vec<ParsedFavEntry>, usize)> {
            flatten_favorites(&storage)?;
            let mut out = Vec::new();
            let mut skipped = 0usize;
            for entry in std::fs::read_dir(&favs_dir)
                .map_err(|e| Error::io(&favs_dir, e))?
                .flatten()
            {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let filename = entry.file_name().to_string_lossy().to_string();
                if !filename.ends_with(".fav") {
                    continue;
                }
                let Some(system) = systems::system_from_fav_filename(&filename).map(str::to_string)
                else {
                    skipped += 1;
                    continue;
                };
                let rom_filename = filename
                    .split_once('@')
                    .map(|(_, rest)| rest.trim_end_matches(".fav"))
                    .unwrap_or("")
                    .to_string();
                out.push((path, filename, system, rom_filename));
            }
            Ok((out, skipped))
        };
        {
            tokio::task::spawn_blocking(scan)
                .await
                .map_err(|e| Error::Other(format!("organize scan panicked: {e}")))??
        }
    };

    let batch_input: Vec<(String, String, String)> = parsed_entries
        .iter()
        .map(|(_, f, s, r)| (f.clone(), s.clone(), r.clone()))
        .collect();
    let batch = CatalogLookup::prefetch(&batch_input).await;

    // Phase 3: organize (copy/rename) on the blocking pool.
    let ratings_owned = ratings.cloned();
    let favs_dir_owned = favs_dir.clone();
    let organize = move || -> Result<(usize, usize)> {
        let mut organized = 0usize;
        let mut skipped = skipped_from_scan;
        for (src, filename, system, rom_filename) in &parsed_entries {
            let primary_folder = criteria_folder(
                primary,
                system,
                rom_filename,
                ratings_owned.as_ref(),
                &batch,
            );
            let subfolder = match secondary {
                Some(sec) => {
                    let secondary_folder =
                        criteria_folder(sec, system, rom_filename, ratings_owned.as_ref(), &batch);
                    PathBuf::from(&primary_folder).join(&secondary_folder)
                }
                None => PathBuf::from(&primary_folder),
            };

            let target_dir = favs_dir_owned.join(&subfolder);
            std::fs::create_dir_all(&target_dir).map_err(|e| Error::io(&target_dir, e))?;

            let dest = target_dir.join(filename);
            if dest.exists() {
                skipped += 1;
                continue;
            }

            if keep_originals {
                let mtime = std::fs::metadata(src).and_then(|m| m.modified()).ok();
                std::fs::copy(src, &dest).map_err(|e| Error::io(src, e))?;
                if let Some(mtime) = mtime {
                    let _ = std::fs::File::options()
                        .write(true)
                        .open(&dest)
                        .and_then(|f| f.set_modified(mtime));
                }
            } else {
                std::fs::rename(src, &dest).map_err(|e| Error::io(src, e))?;
            }
            organized += 1;
        }
        Ok((organized, skipped))
    };

    let (organized, skipped) = tokio::task::spawn_blocking(organize)
        .await
        .map_err(|e| Error::Other(format!("organize task panicked: {e}")))??;

    Ok(OrganizeResult { organized, skipped })
}

/// Prefetched catalog data indexed by `(system, stem)`.
#[derive(Default)]
struct CatalogLookup {
    arcade: HashMap<(String, String), ArcadeGameInfo>,
    game: HashMap<(String, String), CanonicalGame>,
}

impl CatalogLookup {
    async fn prefetch(parsed: &[(String, String, String)]) -> Self {
        let mut by_system: HashMap<&str, Vec<&str>> = HashMap::new();
        for (_fav, system, rom_filename) in parsed {
            by_system
                .entry(system.as_str())
                .or_default()
                .push(rom_filename.as_str());
        }

        let mut batch = CatalogLookup::default();
        for (system, rom_filenames) in &by_system {
            let is_arcade = systems::is_arcade_system(system);
            if is_arcade {
                let stems: Vec<&str> = rom_filenames
                    .iter()
                    .map(|f| replay_control_core::title_utils::filename_stem(f))
                    .collect();
                let hits = arcade_db::lookup_arcade_games_batch(&stems).await;
                for (stem, info) in hits {
                    batch.arcade.insert(((*system).to_string(), stem), info);
                }
            } else {
                let stems: Vec<&str> = rom_filenames
                    .iter()
                    .map(|f| replay_control_core::title_utils::filename_stem(f))
                    .collect();
                let mut exact = game_db::lookup_games_batch(system, &stems).await;

                let mut need_norm: Vec<(String, String)> = Vec::new();
                for stem in &stems {
                    if let Some(entry) = exact.remove(*stem) {
                        batch
                            .game
                            .insert(((*system).to_string(), (*stem).to_string()), entry.game);
                    } else {
                        let norm = game_db::normalize_filename(stem);
                        if !norm.is_empty() {
                            need_norm.push(((*stem).to_string(), norm));
                        }
                    }
                }

                if !need_norm.is_empty() {
                    let norms: Vec<&str> = need_norm.iter().map(|(_, n)| n.as_str()).collect();
                    let fuzzy = game_db::lookup_by_normalized_titles_batch(system, &norms).await;
                    for (stem, norm) in need_norm {
                        if let Some(cg) = fuzzy.get(&norm) {
                            batch.game.insert(((*system).to_string(), stem), cg.clone());
                        }
                    }
                }
            }
        }
        batch
    }
}

/// Sanitize a string for use as a directory name.
/// Replaces `/` and other filesystem-unsafe characters with `-`.
fn sanitize_folder_name(name: &str) -> String {
    name.replace(['/', '\\', ':'], "-")
}

/// Determine the subfolder name for a favorite based on the given criteria.
fn criteria_folder(
    criteria: OrganizeCriteria,
    system: &str,
    rom_filename: &str,
    ratings: Option<&std::collections::HashMap<(String, String), f64>>,
    batch: &CatalogLookup,
) -> String {
    let raw = criteria_folder_raw(criteria, system, rom_filename, ratings, batch);
    sanitize_folder_name(&raw)
}

fn criteria_folder_raw(
    criteria: OrganizeCriteria,
    system: &str,
    rom_filename: &str,
    ratings: Option<&std::collections::HashMap<(String, String), f64>>,
    batch: &CatalogLookup,
) -> String {
    let is_arcade = systems::is_arcade_system(system);

    let arcade_info = if is_arcade {
        let stem = replay_control_core::title_utils::filename_stem(rom_filename);
        batch.arcade.get(&(system.to_string(), stem.to_string()))
    } else {
        None
    };
    let game_info = if is_arcade {
        None
    } else {
        let stem = replay_control_core::title_utils::filename_stem(rom_filename);
        batch.game.get(&(system.to_string(), stem.to_string()))
    };

    match criteria {
        OrganizeCriteria::System => systems::find_system(system)
            .map(|s| s.display_name.to_string())
            .unwrap_or_else(|| system.to_string()),
        OrganizeCriteria::Genre => {
            let raw_genre = if is_arcade {
                arcade_info.map(|i| i.category.as_str()).unwrap_or_default()
            } else {
                game_info.map(|g| g.genre.as_str()).unwrap_or_default()
            };
            let genre_group = replay_control_core::genre::normalize_genre(raw_genre);
            if genre_group.is_empty() {
                "Other".to_string()
            } else {
                genre_group.to_string()
            }
        }
        OrganizeCriteria::Players => {
            let players = if is_arcade {
                arcade_info.map(|i| i.players).unwrap_or(0)
            } else {
                game_info.map(|g| g.players).unwrap_or(0)
            };
            match players {
                0 => "Unknown".to_string(),
                1 => "1 Player".to_string(),
                2 => "2 Players".to_string(),
                n => format!("{n} Players"),
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
        OrganizeCriteria::Developer => {
            let raw_developer = if is_arcade {
                arcade_info
                    .map(|i| i.manufacturer.as_str())
                    .unwrap_or_default()
            } else {
                game_info.map(|g| g.developer.as_str()).unwrap_or_default()
            };
            let developer = replay_control_core::developer::normalize_developer(raw_developer);
            if developer.is_empty() {
                "Unknown".to_string()
            } else {
                developer
            }
        }
        OrganizeCriteria::Alphabetical => {
            let display = if is_arcade {
                arcade_info.map(|i| i.display_name.clone())
            } else {
                game_info.map(|g| g.display_name.clone())
            };
            let display = display.unwrap_or_else(|| rom_filename.to_string());
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

pub(crate) fn is_favorite_sync(
    storage: &StorageLocation,
    system_folder: &str,
    rom_filename: &str,
) -> bool {
    let fav_filename = format!("{system_folder}@{rom_filename}.fav");
    let favs_dir = storage.favorites_dir();

    if favs_dir.join(&fav_filename).exists() {
        return true;
    }

    if favs_dir.join(system_folder).join(&fav_filename).exists() {
        return true;
    }

    find_fav_recursive(&favs_dir, &fav_filename)
}

/// Check if a ROM is favorited. Runs the `exists()` syscalls plus a
/// recursive `read_dir` walk on the blocking pool.
pub async fn is_favorite(
    storage: &StorageLocation,
    system_folder: &str,
    rom_filename: &str,
) -> bool {
    let storage = storage.clone();
    let system_folder = system_folder.to_string();
    let rom_filename = rom_filename.to_string();
    tokio::task::spawn_blocking(move || is_favorite_sync(&storage, &system_folder, &rom_filename))
        .await
        .unwrap_or(false)
}

fn find_fav_recursive(dir: &Path, fav_filename: &str) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with('_')
                && !name.starts_with('.')
                && (path.join(fav_filename).exists() || find_fav_recursive(&path, fav_filename))
            {
                return true;
            }
        }
    }
    false
}

struct RawFavorite {
    system: String,
    rom_filename: String,
    rom_path: String,
    marker_filename: String,
    subfolder: String,
    date_added: u64,
}

async fn walk_favorites_blocking(dir: PathBuf, favs_root: PathBuf) -> Result<Vec<RawFavorite>> {
    let walk = move || -> Result<Vec<RawFavorite>> {
        let mut raw = Vec::new();
        collect_raw_favorites(&dir, &favs_root, &mut raw)?;
        Ok(raw)
    };
    {
        tokio::task::spawn_blocking(walk)
            .await
            .map_err(|e| Error::Other(format!("favorites walk panicked: {e}")))?
    }
}

fn collect_raw_favorites(dir: &Path, favs_root: &Path, out: &mut Vec<RawFavorite>) -> Result<()> {
    let entries = std::fs::read_dir(dir).map_err(|e| Error::io(dir, e))?;

    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with('_') && !name.starts_with('.') {
                collect_raw_favorites(&path, favs_root, out)?;
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

        out.push(RawFavorite {
            system,
            rom_filename,
            rom_path,
            marker_filename: filename,
            subfolder,
            date_added,
        });
    }

    Ok(())
}

async fn collect_favorites(dir: &Path, favs_root: &Path, out: &mut Vec<Favorite>) -> Result<()> {
    let raw = walk_favorites_blocking(dir.to_path_buf(), favs_root.to_path_buf()).await?;

    let parsed: Vec<(String, String, String)> = raw
        .iter()
        .map(|r| {
            (
                r.marker_filename.clone(),
                r.system.clone(),
                r.rom_filename.clone(),
            )
        })
        .collect();
    let batch = CatalogLookup::prefetch(&parsed).await;

    for r in raw {
        let game = build_game_ref(&r.system, r.rom_filename, r.rom_path, &batch);
        out.push(Favorite {
            game,
            marker_filename: r.marker_filename,
            subfolder: r.subfolder,
            date_added: r.date_added,
        });
    }

    Ok(())
}

/// Build a `GameRef` from a prefetched `CatalogLookup`.
/// Falls back to letting `GameRef` compute a display name from the filename
/// stem (tags, disc labels, article inversion) when the catalog has no match.
fn build_game_ref(
    system: &str,
    rom_filename: String,
    rom_path: String,
    batch: &CatalogLookup,
) -> GameRef {
    let is_arcade = systems::is_arcade_system(system);

    let resolved_name: Option<String> = if is_arcade {
        let stem = replay_control_core::title_utils::filename_stem(&rom_filename);
        batch
            .arcade
            .get(&(system.to_string(), stem.to_string()))
            .map(|info| info.display_name.clone())
    } else {
        let stem = replay_control_core::title_utils::filename_stem(&rom_filename);
        batch
            .game
            .get(&(system.to_string(), stem.to_string()))
            .map(|g| g.display_name.clone())
    };

    GameRef::from_parts(system, rom_filename, rom_path, resolved_name)
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

    #[tokio::test(flavor = "current_thread")]
    async fn add_and_list_favorite() {
        let (_tmp, storage) = setup_test_storage();

        add_favorite(&storage, "sega_smd", "/roms/sega_smd/Sonic.md", false)
            .await
            .unwrap();

        let favs = list_favorites(&storage).await.unwrap();
        assert_eq!(favs.len(), 1);
        assert_eq!(favs[0].game.system, "sega_smd");
        assert_eq!(favs[0].game.rom_filename, "Sonic.md");
        assert_eq!(favs[0].game.rom_path, "/roms/sega_smd/Sonic.md");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_remove_favorite() {
        let (_tmp, storage) = setup_test_storage();

        add_favorite(&storage, "sega_smd", "/roms/sega_smd/Sonic.md", false)
            .await
            .unwrap();
        assert!(is_favorite_sync(&storage, "sega_smd", "Sonic.md"));

        super::remove_favorite(&storage, "sega_smd@Sonic.md.fav", None).unwrap();
        assert!(!is_favorite_sync(&storage, "sega_smd", "Sonic.md"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn group_and_flatten() {
        let (_tmp, storage) = setup_test_storage();

        add_favorite(&storage, "sega_smd", "/roms/sega_smd/Sonic.md", false)
            .await
            .unwrap();
        add_favorite(
            &storage,
            "nintendo_nes",
            "/roms/nintendo_nes/Mario.nes",
            false,
        )
        .await
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

    #[tokio::test(flavor = "current_thread")]
    async fn organize_by_system_moves_files() {
        let (_tmp, storage) = setup_test_storage();

        add_favorite(&storage, "sega_smd", "/roms/sega_smd/Sonic.md", false)
            .await
            .unwrap();
        add_favorite(
            &storage,
            "nintendo_nes",
            "/roms/nintendo_nes/Mario.nes",
            false,
        )
        .await
        .unwrap();

        let result = organize_favorites(&storage, OrganizeCriteria::System, None, false, None)
            .await
            .unwrap();
        assert_eq!(result.organized, 2);

        // Files should be in system-named subfolders.
        let favs = list_favorites(&storage).await.unwrap();
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

    #[tokio::test(flavor = "current_thread")]
    async fn organize_keep_originals_duplicates() {
        let (_tmp, storage) = setup_test_storage();

        add_favorite(&storage, "sega_smd", "/roms/sega_smd/Sonic.md", false)
            .await
            .unwrap();

        let result = organize_favorites(&storage, OrganizeCriteria::System, None, true, None)
            .await
            .unwrap();
        assert_eq!(result.organized, 1);

        // File should exist both at root and in subfolder.
        let favs_dir = storage.favorites_dir();
        assert!(favs_dir.join("sega_smd@Sonic.md.fav").exists());

        // list_favorites should deduplicate: only 1 entry.
        let favs = list_favorites(&storage).await.unwrap();
        assert_eq!(favs.len(), 1);
        // Should prefer subfolder version.
        assert!(!favs[0].subfolder.is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn flatten_after_organize() {
        let (_tmp, storage) = setup_test_storage();

        add_favorite(&storage, "sega_smd", "/roms/sega_smd/Sonic.md", false)
            .await
            .unwrap();
        add_favorite(
            &storage,
            "nintendo_nes",
            "/roms/nintendo_nes/Mario.nes",
            false,
        )
        .await
        .unwrap();

        organize_favorites(&storage, OrganizeCriteria::System, None, false, None)
            .await
            .unwrap();
        let moved = flatten_favorites(&storage).unwrap();
        assert_eq!(moved, 2);

        let favs = list_favorites(&storage).await.unwrap();
        assert_eq!(favs.len(), 2);
        assert!(favs.iter().all(|f| f.subfolder.is_empty()));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn organize_alphabetical() {
        let (_tmp, storage) = setup_test_storage();

        add_favorite(&storage, "sega_smd", "/roms/sega_smd/Sonic.md", false)
            .await
            .unwrap();
        add_favorite(
            &storage,
            "nintendo_nes",
            "/roms/nintendo_nes/Mario.nes",
            false,
        )
        .await
        .unwrap();

        let result =
            organize_favorites(&storage, OrganizeCriteria::Alphabetical, None, false, None)
                .await
                .unwrap();
        assert_eq!(result.organized, 2);

        let favs = list_favorites(&storage).await.unwrap();
        // One should be in "S/" and the other in "M/"
        let subfolders: Vec<_> = favs.iter().map(|f| f.subfolder.as_str()).collect();
        assert!(subfolders.contains(&"S"));
        assert!(subfolders.contains(&"M"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn organize_two_levels() {
        let (_tmp, storage) = setup_test_storage();

        add_favorite(&storage, "sega_smd", "/roms/sega_smd/Sonic.md", false)
            .await
            .unwrap();

        let result = organize_favorites(
            &storage,
            OrganizeCriteria::Alphabetical,
            Some(OrganizeCriteria::System),
            false,
            None,
        )
        .await
        .unwrap();
        assert_eq!(result.organized, 1);

        let favs = list_favorites(&storage).await.unwrap();
        assert_eq!(favs.len(), 1);
        // Subfolder should be "S/{system_display_name}"
        assert!(favs[0].subfolder.starts_with("S"));
        assert!(favs[0].subfolder.contains(std::path::MAIN_SEPARATOR));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn organize_system_with_slash_in_name() {
        let (_tmp, storage) = setup_test_storage();

        // Sega Mega Drive / Genesis has a `/` in the display name.
        add_favorite(&storage, "sega_smd", "/roms/sega_smd/Sonic.md", false)
            .await
            .unwrap();
        // Arcade (Atomiswave/Naomi) also has `/`.
        add_favorite(
            &storage,
            "arcade_flycast",
            "/roms/arcade_flycast/game.zip",
            false,
        )
        .await
        .unwrap();

        let result = organize_favorites(&storage, OrganizeCriteria::System, None, false, None)
            .await
            .unwrap();
        assert_eq!(result.organized, 2);

        // The subfolder should NOT create nested dirs from the `/`.
        let favs = list_favorites(&storage).await.unwrap();
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
        let favs = list_favorites(&storage).await.unwrap();
        assert!(favs.iter().all(|f| f.subfolder.is_empty()));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn flatten_deep_nesting() {
        let (_tmp, storage) = setup_test_storage();

        add_favorite(&storage, "sega_smd", "/roms/sega_smd/Sonic.md", false)
            .await
            .unwrap();
        add_favorite(
            &storage,
            "nintendo_nes",
            "/roms/nintendo_nes/Mario.nes",
            false,
        )
        .await
        .unwrap();

        // Organize with 2 levels.
        organize_favorites(
            &storage,
            OrganizeCriteria::Alphabetical,
            Some(OrganizeCriteria::System),
            false,
            None,
        )
        .await
        .unwrap();

        // Verify they're nested.
        let favs = list_favorites(&storage).await.unwrap();
        assert!(
            favs.iter()
                .all(|f| f.subfolder.contains(std::path::MAIN_SEPARATOR))
        );

        // Flatten should handle deep nesting.
        let moved = flatten_favorites(&storage).unwrap();
        assert_eq!(moved, 2);
        let favs = list_favorites(&storage).await.unwrap();
        assert_eq!(favs.len(), 2);
        assert!(favs.iter().all(|f| f.subfolder.is_empty()));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn duplicate_favorite_errors() {
        let (_tmp, storage) = setup_test_storage();

        add_favorite(&storage, "sega_smd", "/roms/sega_smd/Sonic.md", false)
            .await
            .unwrap();
        let result = add_favorite(&storage, "sega_smd", "/roms/sega_smd/Sonic.md", false).await;
        assert!(result.is_err());
    }
}
