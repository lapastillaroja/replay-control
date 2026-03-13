use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::game_ref::GameRef;
use crate::rom_tags::{self, RegionPreference};
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
    /// Game rating (0.0–5.0 scale), from metadata DB or game_db.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<f32>,
    /// Maximum number of players, from game_db or arcade_db.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub players: Option<u8>,
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
///
/// The `region_pref` parameter controls the sort order of region variants:
/// ROMs from the preferred region sort before others within the same title group.
pub fn list_roms(
    storage: &StorageLocation,
    system_folder: &str,
    region_pref: RegionPreference,
) -> Result<Vec<RomEntry>> {
    let system = systems::find_system(system_folder)
        .ok_or_else(|| Error::SystemNotFound(system_folder.to_string()))?;

    let system_dir = storage.system_roms_dir(system_folder);
    if !system_dir.exists() {
        return Ok(Vec::new());
    }

    let mut roms = Vec::new();
    collect_roms_recursive(&system_dir, &storage.roms_dir(), system, &mut roms);
    apply_m3u_dedup(&mut roms, &storage.roms_dir());

    // Sort by display name, then by tier (originals before hacks), then by region
    // (using the user's region preference to determine region ordering).
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
        let (a_tier, a_region, _) = rom_tags::classify(&a.game.rom_filename);
        let (b_tier, b_region, _) = rom_tags::classify(&b.game.rom_filename);
        a_name
            .to_lowercase()
            .cmp(&b_name.to_lowercase())
            .then(a_tier.cmp(&b_tier))
            .then(
                a_region
                    .sort_key(region_pref)
                    .cmp(&b_region.sort_key(region_pref)),
            )
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
    apply_m3u_dedup(&mut all_roms, &roms_dir);

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
    count_roms_inner(dir, system, &HashSet::new())
}

fn count_roms_inner(
    dir: &Path,
    system: &System,
    parent_m3u_refs: &HashSet<String>,
) -> (usize, u64) {
    let mut count = 0usize;
    let mut size = 0u64;

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return (0, 0),
    };

    // Collect ROM files in this directory for M3U dedup.
    struct FileInfo {
        filename: String,
        size: u64,
        is_m3u: bool,
        path: PathBuf,
    }

    let mut files: Vec<FileInfo> = Vec::new();
    let mut subdirs: Vec<PathBuf> = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Skip special folders
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with('_') {
                continue;
            }
            subdirs.push(path);
        } else if is_rom_file(&path, system) {
            let filename = entry.file_name().to_string_lossy().to_string();
            let file_size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            let is_m3u = path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("m3u"));
            files.push(FileInfo {
                filename,
                size: file_size,
                is_m3u,
                path,
            });
        }
    }

    // Build exclusion set from M3U references in this directory.
    // This set is also passed down to subdirectories so that ScummVM .scummvm
    // files inside subfolders are excluded when an M3U wrapper exists.
    let mut referenced: HashSet<String> = HashSet::new();
    for f in &files {
        if f.is_m3u {
            for r in parse_m3u_references(&f.path) {
                let lower = r.to_lowercase();
                // ScummVM: also exclude the alternative extension (.svm <-> .scummvm).
                if let Some(stem) = lower.strip_suffix(".svm") {
                    referenced.insert(format!("{stem}.scummvm"));
                } else if let Some(stem) = lower.strip_suffix(".scummvm") {
                    referenced.insert(format!("{stem}.svm"));
                }
                referenced.insert(lower);
            }
        }
    }

    // Recurse into subdirectories, passing down the M3U references.
    for subdir in subdirs {
        let (sub_count, sub_size) = count_roms_inner(&subdir, system, &referenced);
        count += sub_count;
        size += sub_size;
    }

    // Merge parent M3U references with this directory's references
    // so files in this directory can also be excluded by parent M3U entries.
    let all_refs: HashSet<&String> = referenced.iter().chain(parent_m3u_refs.iter()).collect();

    // Count and sum sizes, skipping files referenced by M3U playlists.
    // Aggregate referenced file sizes into the M3U entries.
    if all_refs.is_empty() {
        // Fast path: no M3U dedup needed.
        for f in &files {
            count += 1;
            size += f.size;
        }
    } else {
        // Accumulate sizes of referenced disc files per M3U filename,
        // so we can add them to the M3U's reported size.
        let mut disc_sizes: u64 = 0;
        let mut m3u_count = 0usize;
        let mut m3u_size = 0u64;
        let mut non_m3u_count = 0usize;
        let mut non_m3u_size = 0u64;

        for f in &files {
            let lower = f.filename.to_lowercase();
            if f.is_m3u {
                m3u_count += 1;
                m3u_size += f.size;
            } else if all_refs.contains(&lower) {
                // This disc file is referenced by an M3U; skip it from count
                // but accumulate its size for the M3U aggregate.
                disc_sizes += f.size;
            } else {
                // Standalone file not referenced by any M3U.
                non_m3u_count += 1;
                non_m3u_size += f.size;
            }
        }

        count += m3u_count + non_m3u_count;
        size += m3u_size + non_m3u_size + disc_sizes;
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
                rating: None,
                players: None,
            });
        }
    }
}

/// Parse an M3U file and return the list of referenced filenames (just the
/// filename portion, no directories).
///
/// Handles both text M3U files (multi-disc playlists) and X68000 binary M3U
/// files where the first line is a filename followed by binary disc data.
/// Uses `BufReader` and reads at most `MAX_M3U_BYTES` to avoid loading large
/// binary files into memory.
fn parse_m3u_references(m3u_path: &Path) -> Vec<String> {
    /// Maximum bytes to read from an M3U file. Covers any reasonable text
    /// playlist while protecting against X68000 binary M3U files (~1.2 MB).
    const MAX_M3U_BYTES: u64 = 8192;

    let file = match std::fs::File::open(m3u_path) {
        Ok(f) => f,
        Err(_) => return vec![],
    };

    let reader = BufReader::new(file.take(MAX_M3U_BYTES));
    let mut refs = Vec::new();

    for line_result in reader.lines() {
        let line: String = match line_result {
            Ok(l) => l,
            // Non-UTF-8 line means we hit binary data; stop parsing.
            Err(_) => break,
        };
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if !looks_like_filename(trimmed) {
            // Binary/garbage line; stop parsing.
            break;
        }
        // Extract just the filename from a potentially absolute or relative path.
        // ScummVM uses absolute paths like /media/nfs/roms/scummvm/Game/Game.svm.
        // Windows-created M3U files may use backslashes (e.g., "subdir\disc1.chd")
        // which are NOT recognized as path separators on Linux by std::path::Path.
        let normalized = trimmed.replace('\\', "/");
        let filename = Path::new(&normalized)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or(trimmed);
        refs.push(filename.to_string());
    }

    refs
}

/// Check whether a string looks like a valid filename reference in an M3U.
/// Rejects binary data lines and overly long strings.
fn looks_like_filename(s: &str) -> bool {
    // Must contain a dot (for the extension), be reasonably short, and
    // contain no control characters except tab.
    s.contains('.') && s.len() < 512 && s.chars().all(|c| !c.is_control() || c == '\t')
}

/// Remove ROM entries that are referenced by M3U playlist files and aggregate
/// their sizes into the corresponding M3U entry.
///
/// This prevents double-counting of disc files alongside their M3U entry
/// (e.g., X68000 games where both `Game.m3u` and `Game.dim` exist).
fn apply_m3u_dedup(roms: &mut Vec<RomEntry>, roms_root: &Path) {
    // Collect M3U entries and parse their references.
    // Key: lowercased referenced filename -> list of M3U indices that reference it.
    let mut referenced_by_m3u: HashMap<String, Vec<usize>> = HashMap::new();

    for (idx, rom) in roms.iter().enumerate() {
        if !rom.is_m3u {
            continue;
        }
        // Resolve the M3U file's absolute path on disk from rom_path.
        // rom_path is like "/roms/sharp_x68k/Game.m3u" relative to storage root.
        let m3u_disk_path = roms_root
            .parent()
            .unwrap_or(Path::new("/"))
            .join(rom.game.rom_path.trim_start_matches('/'));

        for filename in parse_m3u_references(&m3u_disk_path) {
            let lower = filename.to_lowercase();
            referenced_by_m3u
                .entry(lower.clone())
                .or_default()
                .push(idx);
            // ScummVM M3U files reference .svm or .scummvm files in subfolders.
            // Some games have both extensions; add the alternative so both are excluded.
            if let Some(stem) = lower.strip_suffix(".svm") {
                referenced_by_m3u
                    .entry(format!("{stem}.scummvm"))
                    .or_default()
                    .push(idx);
            } else if let Some(stem) = lower.strip_suffix(".scummvm") {
                referenced_by_m3u
                    .entry(format!("{stem}.svm"))
                    .or_default()
                    .push(idx);
            }
        }
    }

    if referenced_by_m3u.is_empty() {
        return;
    }

    // Build a set of ROM indices to remove (disc files referenced by M3U).
    // Also accumulate sizes to add to each M3U entry.
    let mut m3u_extra_size: HashMap<usize, u64> = HashMap::new();
    let mut indices_to_remove: HashSet<usize> = HashSet::new();

    for (idx, rom) in roms.iter().enumerate() {
        if rom.is_m3u {
            continue;
        }
        let key = rom.game.rom_filename.to_lowercase();
        if let Some(m3u_indices) = referenced_by_m3u.get(&key) {
            indices_to_remove.insert(idx);
            // Add this disc file's size to each M3U that references it.
            for &m3u_idx in m3u_indices {
                *m3u_extra_size.entry(m3u_idx).or_default() += rom.size_bytes;
            }
        }
    }

    if indices_to_remove.is_empty() {
        return;
    }

    // Aggregate sizes into M3U entries.
    for (&m3u_idx, &extra) in &m3u_extra_size {
        roms[m3u_idx].size_bytes += extra;
    }

    // Remove referenced disc files, preserving order of remaining entries.
    let mut idx = 0;
    roms.retain(|_| {
        let keep = !indices_to_remove.contains(&idx);
        idx += 1;
        keep
    });
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

    #[test]
    fn parse_m3u_single_disc() {
        let tmp = tempdir();
        let m3u = tmp.join("game.m3u");
        fs::write(&m3u, "Game (1991)(Publisher).dim\r\n").unwrap();
        let refs = parse_m3u_references(&m3u);
        assert_eq!(refs, vec!["Game (1991)(Publisher).dim"]);
    }

    #[test]
    fn parse_m3u_multi_disc() {
        let tmp = tempdir();
        let m3u = tmp.join("game.m3u");
        fs::write(
            &m3u,
            "Game (Disk 1 of 3)(A).dim\r\nGame (Disk 2 of 3)(B).dim\r\nGame (Disk 3 of 3)(C).dim\r\n",
        )
        .unwrap();
        let refs = parse_m3u_references(&m3u);
        assert_eq!(refs.len(), 3);
        assert_eq!(refs[0], "Game (Disk 1 of 3)(A).dim");
        assert_eq!(refs[2], "Game (Disk 3 of 3)(C).dim");
    }

    #[test]
    fn parse_m3u_comments_and_blanks() {
        let tmp = tempdir();
        let m3u = tmp.join("game.m3u");
        fs::write(&m3u, "# comment\n\ndisc1.chd\n\n# another\ndisc2.chd\n").unwrap();
        let refs = parse_m3u_references(&m3u);
        assert_eq!(refs, vec!["disc1.chd", "disc2.chd"]);
    }

    #[test]
    fn parse_m3u_absolute_path() {
        let tmp = tempdir();
        let m3u = tmp.join("game.m3u");
        fs::write(
            &m3u,
            "/media/nfs/roms/scummvm/Grim Fandango (CD Spanish)/Grim Fandango (CD Spanish).svm\n",
        )
        .unwrap();
        let refs = parse_m3u_references(&m3u);
        assert_eq!(refs, vec!["Grim Fandango (CD Spanish).svm"]);
    }

    #[test]
    fn parse_m3u_windows_backslash_paths() {
        let tmp = tempdir();
        let m3u = tmp.join("game.m3u");
        // Windows-style M3U with backslash paths
        fs::write(&m3u, "subdir\\disc1.chd\r\nsubdir\\disc2.chd\r\n").unwrap();
        let refs = parse_m3u_references(&m3u);
        assert_eq!(refs, vec!["disc1.chd", "disc2.chd"]);
    }

    #[test]
    fn parse_m3u_mixed_path_separators() {
        let tmp = tempdir();
        let m3u = tmp.join("game.m3u");
        // Mix of forward slashes, backslashes, and bare filenames
        fs::write(&m3u, "disc1.chd\nsubdir/disc2.chd\nother\\disc3.chd\n").unwrap();
        let refs = parse_m3u_references(&m3u);
        assert_eq!(refs, vec!["disc1.chd", "disc2.chd", "disc3.chd"]);
    }

    #[test]
    fn parse_m3u_binary_stops_at_non_text() {
        let tmp = tempdir();
        let m3u = tmp.join("game.m3u");
        let mut content = b"Game.dim\r\n".to_vec();
        // Append binary data (like X68000 embedded disc image)
        content.extend_from_slice(&[0xe5; 1024]);
        fs::write(&m3u, &content).unwrap();
        let refs = parse_m3u_references(&m3u);
        assert_eq!(refs, vec!["Game.dim"]);
    }

    #[test]
    fn parse_m3u_nonexistent_file() {
        let refs = parse_m3u_references(Path::new("/nonexistent/path.m3u"));
        assert!(refs.is_empty());
    }

    #[test]
    fn m3u_dedup_hides_disc_files() {
        let tmp = tempdir();
        let x68k_dir = tmp.join("roms/sharp_x68k");
        fs::create_dir_all(&x68k_dir).unwrap();

        // Create M3U referencing two disc files
        fs::write(
            x68k_dir.join("Game.m3u"),
            "Game (Disk 1).dim\nGame (Disk 2).dim\n",
        )
        .unwrap();
        // Create the disc files (100 bytes each)
        fs::write(x68k_dir.join("Game (Disk 1).dim"), &[0u8; 100]).unwrap();
        fs::write(x68k_dir.join("Game (Disk 2).dim"), &[0u8; 100]).unwrap();
        // Create a standalone file not referenced by any M3U
        fs::write(x68k_dir.join("Other.dim"), &[0u8; 50]).unwrap();

        let storage = StorageLocation::from_path(tmp.clone(), crate::storage::StorageKind::Sd);
        let roms = list_roms(&storage, "sharp_x68k", RegionPreference::default()).unwrap();

        // Should have 2 entries: Game.m3u and Other.dim (disc files hidden)
        assert_eq!(roms.len(), 2);

        let m3u_entry = roms.iter().find(|r| r.is_m3u).unwrap();
        assert_eq!(m3u_entry.game.rom_filename, "Game.m3u");
        // M3U size should be its own size + both disc files (200 bytes)
        let m3u_own_size = fs::metadata(x68k_dir.join("Game.m3u")).unwrap().len();
        assert_eq!(m3u_entry.size_bytes, m3u_own_size + 200);

        let other = roms
            .iter()
            .find(|r| r.game.rom_filename == "Other.dim")
            .unwrap();
        assert_eq!(other.size_bytes, 50);
    }

    #[test]
    fn m3u_dedup_count_is_accurate() {
        let tmp = tempdir();
        let x68k_dir = tmp.join("roms/sharp_x68k");
        fs::create_dir_all(&x68k_dir).unwrap();

        // 1 M3U referencing 3 disc files + 1 standalone
        fs::write(
            x68k_dir.join("Game.m3u"),
            "Game (Disk 1).dim\nGame (Disk 2).dim\nGame (Disk 3).dim\n",
        )
        .unwrap();
        fs::write(x68k_dir.join("Game (Disk 1).dim"), &[0u8; 100]).unwrap();
        fs::write(x68k_dir.join("Game (Disk 2).dim"), &[0u8; 100]).unwrap();
        fs::write(x68k_dir.join("Game (Disk 3).dim"), &[0u8; 100]).unwrap();
        fs::write(x68k_dir.join("Standalone.hdf"), &[0u8; 200]).unwrap();

        let storage = StorageLocation::from_path(tmp.clone(), crate::storage::StorageKind::Sd);
        let summaries = scan_systems(&storage);
        let x68k = summaries
            .iter()
            .find(|s| s.folder_name == "sharp_x68k")
            .unwrap();

        // Should count 2 games (1 M3U + 1 standalone), not 5
        assert_eq!(x68k.game_count, 2);
    }

    #[test]
    fn m3u_dedup_case_insensitive() {
        let tmp = tempdir();
        let x68k_dir = tmp.join("roms/sharp_x68k");
        fs::create_dir_all(&x68k_dir).unwrap();

        // M3U references "game.DIM" but file on disk is "game.dim"
        fs::write(x68k_dir.join("Multi.m3u"), "game.DIM\n").unwrap();
        fs::write(x68k_dir.join("game.dim"), &[0u8; 100]).unwrap();

        let storage = StorageLocation::from_path(tmp.clone(), crate::storage::StorageKind::Sd);
        let roms = list_roms(&storage, "sharp_x68k", RegionPreference::default()).unwrap();

        // Only the M3U should remain; the .dim should be hidden
        assert_eq!(roms.len(), 1);
        assert!(roms[0].is_m3u);
    }

    #[test]
    fn scummvm_m3u_dedup_hides_scummvm_in_subfolder() {
        let tmp = tempdir();
        let scummvm_dir = tmp.join("roms/scummvm");
        fs::create_dir_all(&scummvm_dir).unwrap();

        // ScummVM pattern: M3U at root references .svm in subfolder,
        // but subfolder also contains a .scummvm file (which IS in the system's
        // extensions list). The .scummvm file should be hidden by M3U dedup.
        let game_dir = scummvm_dir.join("Cool Game (CD)");
        fs::create_dir_all(&game_dir).unwrap();

        // M3U references a .svm file (absolute path style)
        fs::write(
            scummvm_dir.join("Cool Game (CD).m3u"),
            "/media/roms/scummvm/Cool Game (CD)/Cool Game (CD).svm\n",
        )
        .unwrap();
        // Subfolder has both .svm (not in extensions, won't be picked up) and
        // .scummvm (in extensions, will be picked up)
        fs::write(game_dir.join("Cool Game (CD).svm"), "scummvm-id").unwrap();
        fs::write(game_dir.join("Cool Game (CD).scummvm"), "scummvm-id").unwrap();
        fs::write(game_dir.join("GAME.DAT"), &[0u8; 1000]).unwrap();

        let storage = StorageLocation::from_path(tmp.clone(), crate::storage::StorageKind::Sd);
        let roms = list_roms(&storage, "scummvm", RegionPreference::default()).unwrap();

        // Should only have the M3U entry; the .scummvm should be hidden
        assert_eq!(roms.len(), 1, "Expected 1 ROM (M3U only), got: {roms:?}");
        assert!(roms[0].is_m3u);
    }

    #[test]
    fn scummvm_m3u_dedup_count_is_accurate() {
        let tmp = tempdir();
        let scummvm_dir = tmp.join("roms/scummvm");
        fs::create_dir_all(&scummvm_dir).unwrap();

        // Two games: one with M3U wrapper + .scummvm in subfolder,
        // another with just M3U + .svm in subfolder (no .scummvm).
        let game1_dir = scummvm_dir.join("Game One (CD)");
        fs::create_dir_all(&game1_dir).unwrap();
        fs::write(
            scummvm_dir.join("Game One (CD).m3u"),
            "/roms/scummvm/Game One (CD)/Game One (CD).svm\n",
        )
        .unwrap();
        fs::write(game1_dir.join("Game One (CD).svm"), "id1").unwrap();
        fs::write(game1_dir.join("Game One (CD).scummvm"), "id1").unwrap();

        let game2_dir = scummvm_dir.join("Game Two (CD)");
        fs::create_dir_all(&game2_dir).unwrap();
        fs::write(
            scummvm_dir.join("Game Two (CD).m3u"),
            "/roms/scummvm/Game Two (CD)/Game Two (CD).scummvm\n",
        )
        .unwrap();
        fs::write(game2_dir.join("Game Two (CD).scummvm"), "id2").unwrap();

        let storage = StorageLocation::from_path(tmp.clone(), crate::storage::StorageKind::Sd);

        // Check game count via scan_systems
        let summaries = scan_systems(&storage);
        let scummvm = summaries
            .iter()
            .find(|s| s.folder_name == "scummvm")
            .unwrap();
        // Should count 2 games (2 M3Us), not 4 (2 M3Us + 2 .scummvm)
        assert_eq!(
            scummvm.game_count, 2,
            "Expected 2 games, got {}",
            scummvm.game_count
        );

        // Check ROM list
        let roms = list_roms(&storage, "scummvm", RegionPreference::default()).unwrap();
        assert_eq!(roms.len(), 2, "Expected 2 ROMs (M3Us only), got: {roms:?}");
        assert!(roms.iter().all(|r| r.is_m3u));
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
