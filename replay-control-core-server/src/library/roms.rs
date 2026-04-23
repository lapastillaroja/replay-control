use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::game_ref::GameRef;
use crate::storage::StorageLocation;
use replay_control_core::error::{Error, Result};
use replay_control_core::rom_tags::{self, RegionPreference};
use replay_control_core::systems::{self, System};

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
///
/// A cold USB/NFS scan issues hundreds of `read_dir` + `metadata` syscalls
/// across every system folder; runs on the blocking pool so it doesn't pin
/// a tokio worker.
pub async fn scan_systems(storage: &StorageLocation) -> Vec<SystemSummary> {
    let roms_dir = storage.roms_dir();
    let walk = move || {
        let mut summaries = Vec::new();
        for system in systems::visible_systems() {
            let system_dir = roms_dir.join(system.folder_name);
            let (count, size) = if system_dir.exists() {
                count_roms_recursive(&system_dir, system, &roms_dir)
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

        summaries.sort_by(|a, b| {
            let a_has = a.game_count > 0;
            let b_has = b.game_count > 0;
            b_has.cmp(&a_has).then(a.display_name.cmp(&b.display_name))
        });

        summaries
    };

    {
        tokio::task::spawn_blocking(walk).await.unwrap_or_else(|e| {
            tracing::warn!("scan_systems panicked: {e}");
            Vec::new()
        })
    }
}

/// List ROM files for a specific system.
///
/// The `region_pref` parameter controls the sort order of region variants:
/// ROMs from the preferred region sort before others within the same title group.
/// The optional `region_secondary` provides a fallback region preference.
pub async fn list_roms(
    storage: &StorageLocation,
    system_folder: &str,
    region_pref: RegionPreference,
    region_secondary: Option<RegionPreference>,
) -> Result<Vec<RomEntry>> {
    let system = systems::find_system(system_folder)
        .ok_or_else(|| Error::SystemNotFound(system_folder.to_string()))?;

    let system_dir = storage.system_roms_dir(system_folder);
    let roms_root = storage.roms_dir();

    // Filesystem walk runs on the blocking pool — a cold NFS/USB `roms/<system>`
    // scan can issue hundreds of `read_dir` + `metadata` syscalls and must not
    // pin a tokio worker.
    let raw = walk_raw_roms_blocking(system_dir, roms_root.clone(), system).await;
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    let mut roms = materialize_rom_entries(system, raw).await;

    // m3u dedup opens referenced playlists from disk — also blocking IO.
    roms = apply_m3u_dedup_blocking(roms, roms_root).await;

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
                    .sort_key(region_pref, region_secondary)
                    .cmp(&b_region.sort_key(region_pref, region_secondary)),
            )
    });

    Ok(roms)
}

/// Walk the system's ROM directory on the blocking pool (native) or inline
/// (wasm, which has no thread pool). Returns `None` if the directory does not
/// exist.
async fn walk_raw_roms_blocking(
    system_dir: PathBuf,
    roms_root: PathBuf,
    system: &'static System,
) -> Option<Vec<RawRom>> {
    let walk = move || -> Option<Vec<RawRom>> {
        if !system_dir.exists() {
            return None;
        }
        let mut raw = Vec::new();
        collect_raw_roms_recursive(&system_dir, &roms_root, system, &mut raw);
        Some(raw)
    };
    {
        tokio::task::spawn_blocking(walk).await.unwrap_or_else(|e| {
            tracing::warn!("rom walk panicked: {e}");
            None
        })
    }
}

async fn apply_m3u_dedup_blocking(roms: Vec<RomEntry>, roms_root: PathBuf) -> Vec<RomEntry> {
    let dedup = move || {
        let mut roms = roms;
        // catch_unwind so a panic inside apply_m3u_dedup doesn't erase the
        // whole input list. Silently returning `Vec::new()` on panic would
        // make every system look empty on any dedup bug — far worse than
        // the original symptom (at worst, some disc files not deduped).
        // AssertUnwindSafe: we don't care about `roms`' post-panic state
        // beyond "it's a valid Vec we can return".
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            apply_m3u_dedup(&mut roms, &roms_root);
        }));
        roms
    };
    {
        tokio::task::spawn_blocking(dedup)
            .await
            .unwrap_or_else(|e| {
                // JoinError here is task-cancel during runtime shutdown —
                // no useful recovery, and we've already lost the input.
                tracing::warn!("m3u dedup task join failed: {e}");
                Vec::new()
            })
    }
}

/// Mark each ROM entry's `is_favorite` flag using the favorites on disk.
/// Efficient: collects favorite filenames once, then checks via HashSet lookup.
pub async fn mark_favorites(storage: &StorageLocation, system: &str, roms: &mut [RomEntry]) {
    let fav_set: std::collections::HashSet<String> =
        crate::favorites::list_favorites_for_system(storage, system)
            .await
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

// ---------------------------------------------------------------------------
// ROM grouping, multi-file delete, rename restrictions, and disc detection
// ---------------------------------------------------------------------------

/// Classification of a file within a ROM group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileKind {
    /// The main ROM file the user interacts with (M3U, CUE, CHD, ZIP, etc.)
    Primary,
    /// A disc image referenced by an M3U or CUE (CHD, BIN, DIM, GDI dir, etc.)
    Disc,
    /// A companion sidecar file (SBI, etc.)
    Companion,
    /// An entire data directory (ScummVM game folder contents)
    DataDir,
}

/// A single file (or directory) within a grouped ROM set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupedFile {
    /// Absolute path on disk.
    pub path: PathBuf,
    /// Size in bytes (for directories, the recursive total).
    pub size_bytes: u64,
    /// Classification of this file in the group.
    pub kind: FileKind,
}

/// Report returned by `delete_rom_group`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteReport {
    pub deleted: Vec<PathBuf>,
    pub bytes_freed: u64,
    pub errors: Vec<String>,
}

/// Information about multi-disc sets detected by filename pattern.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscInfo {
    pub disc_number: u32,
    pub total_discs: u32,
    /// Filenames of all sibling discs (including this one).
    pub siblings: Vec<String>,
}

/// Enumerate all files that belong to a ROM group.
///
/// Given the `relative_path` of the primary ROM file (as returned by
/// `rom_path` in `GameRef`), returns every file and directory that should
/// be treated as a unit for delete/size purposes.
///
/// `system` is the system folder name (e.g. "sony_psx", "scummvm").
pub fn list_rom_group(
    storage: &StorageLocation,
    system: &str,
    relative_path: &str,
) -> Result<Vec<GroupedFile>> {
    let full_path = storage.root.join(relative_path.trim_start_matches('/'));
    if !full_path.exists() {
        return Err(Error::RomNotFound(full_path.clone()));
    }

    // Validate path stays within storage root.
    let canonical = full_path
        .canonicalize()
        .map_err(|e| Error::io(&full_path, e))?;
    let root_canonical = storage
        .root
        .canonicalize()
        .map_err(|e| Error::io(&storage.root, e))?;
    if !canonical.starts_with(&root_canonical) {
        return Err(Error::Other("Path traversal detected".to_string()));
    }

    let ext = full_path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    let parent_dir = full_path.parent().unwrap_or(Path::new("/"));

    let mut group = Vec::new();

    // Always include the primary file.
    let primary_size = std::fs::metadata(&full_path).map(|m| m.len()).unwrap_or(0);
    group.push(GroupedFile {
        path: full_path.clone(),
        size_bytes: primary_size,
        kind: FileKind::Primary,
    });

    if ext == "m3u" {
        // Determine if this is a ScummVM M3U by checking the system folder.
        let is_scummvm = system == "scummvm";

        // Parse the M3U to get referenced filenames.
        let refs = parse_m3u_references(&full_path);

        if is_scummvm {
            // ScummVM M3U: the M3U references a .svm/.scummvm file inside a
            // game subdirectory. We want to include the entire subdirectory.
            //
            // Parse the raw lines to find the subdirectory path.
            let raw_lines = parse_m3u_raw_lines(&full_path);
            for raw_line in &raw_lines {
                let normalized = raw_line.replace('\\', "/");
                // Try resolving the referenced file to find its parent dir.
                let resolved = resolve_m3u_reference(&normalized, parent_dir, &storage.roms_dir());
                if let Some(ref_path) = resolved
                    && let Some(game_dir) = ref_path.parent()
                    && game_dir.is_dir()
                    && game_dir != parent_dir
                {
                    // The game data directory is the parent of the .svm file.
                    let dir_size = dir_total_size(game_dir);
                    group.push(GroupedFile {
                        path: game_dir.to_path_buf(),
                        size_bytes: dir_size,
                        kind: FileKind::DataDir,
                    });
                }
            }
        } else {
            // Non-ScummVM M3U: add each referenced disc file.
            // Parse raw lines once (needed for resolving subdirectory references).
            let raw_lines = parse_m3u_raw_lines(&full_path);
            for ref_name in &refs {
                // Try to find the file in the same directory or resolve
                // from the raw M3U lines.
                let disc_path = parent_dir.join(ref_name);
                if disc_path.exists() && disc_path.is_file() {
                    let size = std::fs::metadata(&disc_path).map(|m| m.len()).unwrap_or(0);
                    group.push(GroupedFile {
                        path: disc_path,
                        size_bytes: size,
                        kind: FileKind::Disc,
                    });
                } else {
                    // Try resolving from raw lines (subdirectory references).
                    for raw_line in &raw_lines {
                        let normalized = raw_line.replace('\\', "/");
                        let filename = Path::new(&normalized)
                            .file_name()
                            .and_then(|f| f.to_str())
                            .unwrap_or("");
                        if filename.eq_ignore_ascii_case(ref_name)
                            && let Some(resolved) =
                                resolve_m3u_reference(&normalized, parent_dir, &storage.roms_dir())
                        {
                            if resolved.is_file() {
                                let size =
                                    std::fs::metadata(&resolved).map(|m| m.len()).unwrap_or(0);
                                group.push(GroupedFile {
                                    path: resolved,
                                    size_bytes: size,
                                    kind: FileKind::Disc,
                                });
                            } else if resolved.is_dir() {
                                // GDI subdirectory reference.
                                let size = dir_total_size(&resolved);
                                group.push(GroupedFile {
                                    path: resolved,
                                    size_bytes: size,
                                    kind: FileKind::Disc,
                                });
                            }
                        }
                    }
                }
            }

            // For M3U + CHD/DIM: also check for SBI companions for each disc.
            for ref_name in &refs {
                let disc_path = parent_dir.join(ref_name);
                add_sbi_companion(&disc_path, &mut group);
            }
        }
    } else if ext == "cue" {
        // CUE+BIN: parse FILE directives to find referenced BIN files.
        let bin_files = parse_cue_file_references(&full_path);
        for bin_name in &bin_files {
            let bin_path = parent_dir.join(bin_name);
            if bin_path.exists() && bin_path.is_file() {
                let size = std::fs::metadata(&bin_path).map(|m| m.len()).unwrap_or(0);
                group.push(GroupedFile {
                    path: bin_path,
                    size_bytes: size,
                    kind: FileKind::Disc,
                });
            }
        }
    } else if ext == "chd" {
        // Single CHD: check for SBI companions.
        add_sbi_companion(&full_path, &mut group);

        // For arcade_dc: check for companion GD-ROM CHD files.
        if system == "arcade_dc" {
            add_arcade_dc_companion_chds(&full_path, parent_dir, &mut group);
        }
    } else if ext == "zip" && system == "arcade_dc" {
        // Arcade ZIP: check for companion GD-ROM CHD files.
        add_arcade_dc_companion_chds(&full_path, parent_dir, &mut group);
    }

    // Add SBI companion for the primary file itself (non-M3U case).
    if ext != "m3u" && ext != "cue" {
        add_sbi_companion(&full_path, &mut group);
        // Deduplicate: remove any SBI that was added twice (by the primary check
        // and by the CHD-specific check).
        dedup_group(&mut group);
    }

    Ok(group)
}

/// Delete all files in a ROM group and clean up empty parent directories.
pub fn delete_rom_group(
    storage: &StorageLocation,
    system: &str,
    relative_path: &str,
) -> Result<DeleteReport> {
    let group = list_rom_group(storage, system, relative_path)?;

    let mut report = DeleteReport {
        deleted: Vec::new(),
        bytes_freed: 0,
        errors: Vec::new(),
    };

    // Delete files first, then directories (in reverse order so children
    // are deleted before parents).
    let (dirs, files): (Vec<_>, Vec<_>) = group
        .into_iter()
        .partition(|g| g.kind == FileKind::DataDir || g.path.is_dir());

    for file in &files {
        match std::fs::remove_file(&file.path) {
            Ok(()) => {
                report.deleted.push(file.path.clone());
                report.bytes_freed += file.size_bytes;
            }
            Err(e) => {
                report.errors.push(format!("{}: {e}", file.path.display()));
            }
        }
    }

    // Delete data directories (recursively).
    for dir in &dirs {
        match std::fs::remove_dir_all(&dir.path) {
            Ok(()) => {
                report.deleted.push(dir.path.clone());
                report.bytes_freed += dir.size_bytes;
            }
            Err(e) => {
                report.errors.push(format!("{}: {e}", dir.path.display()));
            }
        }
    }

    // Clean up empty parent directories (walk up from the primary file's
    // parent, stopping at the system roms directory).
    let system_dir = storage.system_roms_dir(system);
    if let Some(primary) = files.first().or(dirs.first())
        && let Some(parent) = primary.path.parent()
    {
        cleanup_empty_parents(parent, &system_dir);
    }

    Ok(report)
}

/// Detect multi-disc sets based on `(Disc N)` / `(Disk N)` filename patterns.
///
/// Returns `Some(DiscInfo)` if this filename is part of a multi-disc set
/// (i.e., there are sibling files with the same base name but different
/// disc numbers). Returns `None` for single-disc games or non-matching names.
pub fn detect_disc_set(
    storage: &StorageLocation,
    system: &str,
    rom_filename: &str,
) -> Option<DiscInfo> {
    let (base, disc_num) = parse_disc_pattern(rom_filename)?;

    let system_dir = storage.system_roms_dir(system);
    if !system_dir.exists() {
        return None;
    }

    // Scan the directory for siblings with the same base pattern.
    let entries = std::fs::read_dir(&system_dir).ok()?;
    let mut siblings: Vec<(u32, String)> = Vec::new();

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if let Some((other_base, other_num)) = parse_disc_pattern(&name)
            && other_base == base
        {
            siblings.push((other_num, name));
        }
    }

    // Only report as multi-disc if there's more than one disc.
    if siblings.len() <= 1 {
        return None;
    }

    siblings.sort_by_key(|(num, _)| *num);
    let total_discs = siblings.len() as u32;
    let sibling_names: Vec<String> = siblings.into_iter().map(|(_, name)| name).collect();

    Some(DiscInfo {
        disc_number: disc_num,
        total_discs,
        siblings: sibling_names,
    })
}

/// Determine whether a ROM file can be safely renamed.
///
/// Returns `(allowed, reason)` where `reason` explains why rename is blocked.
pub fn check_rename_allowed(
    storage: &StorageLocation,
    system: &str,
    relative_path: &str,
) -> (bool, Option<String>) {
    let full_path = storage.root.join(relative_path.trim_start_matches('/'));
    let ext = full_path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    // CUE+BIN: renaming would break internal FILE directives.
    if ext == "cue" {
        return (
            false,
            Some("Rename is not available for CUE files — they reference BIN files by name internally, and renaming would break the game.".to_string()),
        );
    }

    // ScummVM M3U: contains absolute paths to game data.
    if ext == "m3u" && system == "scummvm" {
        return (
            false,
            Some("Rename is not available for ScummVM games — playlists contain absolute paths to game data.".to_string()),
        );
    }

    // Binary M3U (X68000): check if the M3U contains binary data.
    if ext == "m3u" && is_binary_m3u(&full_path) {
        return (
            false,
            Some(
                "Rename is not available — this playlist contains embedded disc data.".to_string(),
            ),
        );
    }

    (true, None)
}

// ---------------------------------------------------------------------------
// Helper functions for ROM grouping
// ---------------------------------------------------------------------------

/// Parse an M3U file and return the raw line content (before extracting filenames).
/// This preserves full paths for ScummVM absolute path resolution.
fn parse_m3u_raw_lines(m3u_path: &Path) -> Vec<String> {
    const MAX_M3U_BYTES: u64 = 8192;

    let file = match std::fs::File::open(m3u_path) {
        Ok(f) => f,
        Err(_) => return vec![],
    };

    let reader = BufReader::new(file.take(MAX_M3U_BYTES));
    let mut lines = Vec::new();

    for line_result in reader.lines() {
        let line = match line_result {
            Ok(l) => l,
            Err(_) => break,
        };
        let trimmed = line.trim().to_string();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if !looks_like_filename(&trimmed) {
            break;
        }
        lines.push(trimmed);
    }

    lines
}

/// Resolve an M3U reference line to an absolute path on disk.
///
/// Handles:
/// - Absolute paths with `/roms/` prefix (Pi-side paths)
/// - Relative paths (resolved from the M3U's parent directory)
fn resolve_m3u_reference(line: &str, m3u_parent: &Path, roms_root: &Path) -> Option<PathBuf> {
    let normalized = line.replace('\\', "/");

    // Try resolving via /roms/ prefix (absolute Pi-side paths).
    if let Some(after_roms) = normalized.split("/roms/").nth(1) {
        let candidate = roms_root.join(after_roms);
        if candidate.exists() {
            return Some(candidate);
        }
    }

    // Try resolving relative to the M3U file's parent directory.
    let candidate = m3u_parent.join(&normalized);
    if candidate.exists() {
        return Some(candidate);
    }

    // Try just the filename in the parent directory.
    let filename = Path::new(&normalized)
        .file_name()
        .and_then(|f| f.to_str())?;
    let candidate = m3u_parent.join(filename);
    if candidate.exists() {
        return Some(candidate);
    }

    None
}

/// Parse CUE file FILE directives to extract referenced BIN filenames.
///
/// CUE files contain lines like:
///   FILE "023 RADIANT SILVERGUN (J).BIN" BINARY
fn parse_cue_file_references(cue_path: &Path) -> Vec<String> {
    let content = match std::fs::read_to_string(cue_path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    let mut files = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("FILE ") {
            // Extract the filename between quotes, or the first token.
            if let Some(start) = rest.find('"') {
                if let Some(end) = rest[start + 1..].find('"') {
                    files.push(rest[start + 1..start + 1 + end].to_string());
                }
            } else {
                // No quotes: take first whitespace-delimited token.
                if let Some(name) = rest.split_whitespace().next() {
                    files.push(name.to_string());
                }
            }
        }
    }

    files
}

/// Check if an M3U file is a binary M3U (X68000 style: first line is a
/// filename, followed by raw binary disc data).
fn is_binary_m3u(m3u_path: &Path) -> bool {
    let file = match std::fs::File::open(m3u_path) {
        Ok(f) => f,
        Err(_) => return false,
    };

    // Read the first 256 bytes. If we find non-UTF-8 data after the first
    // line, it's a binary M3U.
    let mut buf = [0u8; 256];
    let n = match std::io::Read::read(&mut &file, &mut buf) {
        Ok(n) => n,
        Err(_) => return false,
    };

    // Find the end of the first line.
    let first_newline = buf[..n].iter().position(|&b| b == b'\n' || b == b'\r');
    if let Some(pos) = first_newline {
        // Check if data after the first line contains control characters
        // (indicating binary content). Skip CR/LF.
        let after = &buf[pos..n];
        for &b in after {
            if b == b'\n' || b == b'\r' || b == b'\t' {
                continue;
            }
            if !(0x20..=0x7E).contains(&b) {
                return true; // Binary data found.
            }
        }
    }

    false
}

/// Add SBI companion file if it exists alongside the given ROM path.
fn add_sbi_companion(rom_path: &Path, group: &mut Vec<GroupedFile>) {
    if let Some(stem) = rom_path.file_stem().and_then(|s| s.to_str())
        && let Some(parent) = rom_path.parent()
    {
        let sbi_path = parent.join(format!("{stem}.sbi"));
        if sbi_path.exists() && sbi_path.is_file() {
            let size = std::fs::metadata(&sbi_path).map(|m| m.len()).unwrap_or(0);
            group.push(GroupedFile {
                path: sbi_path,
                size_bytes: size,
                kind: FileKind::Companion,
            });
        }
    }
}

/// Add companion GD-ROM CHD files for arcade_dc ZIP/CHD ROMs.
///
/// In arcade_dc, games like `ikaruga.zip` have companion CHD files
/// named like `gdl-0010.chd` or `gds-0009a.chd`.
/// We look up known companion CHDs using the arcade_db.
fn add_arcade_dc_companion_chds(_rom_path: &Path, parent_dir: &Path, group: &mut Vec<GroupedFile>) {
    // Scan for any gdl-*.chd or gds-*.chd files in the same directory.
    // These are always companion files, never standalone games.
    if let Ok(entries) = std::fs::read_dir(parent_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if name.ends_with(".chd") && (name.starts_with("gdl-") || name.starts_with("gds-")) {
                let path = entry.path();
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                group.push(GroupedFile {
                    path,
                    size_bytes: size,
                    kind: FileKind::Companion,
                });
            }
        }
    }
}

/// Recursively compute the total size of all files in a directory.
fn dir_total_size(dir: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                total += dir_total_size(&path);
            } else {
                total += entry.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    total
}

/// Remove duplicate entries from a GroupedFile list (by path).
fn dedup_group(group: &mut Vec<GroupedFile>) {
    let mut seen = HashSet::new();
    group.retain(|g| seen.insert(g.path.clone()));
}

/// Walk up from `dir` towards `stop_at`, removing empty directories.
fn cleanup_empty_parents(dir: &Path, stop_at: &Path) {
    let mut current = dir.to_path_buf();
    while current != *stop_at && current.starts_with(stop_at) {
        // Only remove if the directory is empty.
        match std::fs::read_dir(&current) {
            Ok(mut entries) => {
                if entries.next().is_some() {
                    break; // Directory not empty.
                }
                let _ = std::fs::remove_dir(&current);
            }
            Err(_) => break,
        }
        current = match current.parent() {
            Some(p) => p.to_path_buf(),
            None => break,
        };
    }
}

/// Parse `(Disc N)`, `(Disk N)`, or `(Side X)` pattern from a filename.
///
/// Returns `(base_with_ext, disc_number)` where `base_with_ext` is the
/// filename with the disc/side indicator removed but extension preserved,
/// used for matching siblings.
///
/// Side letters are mapped to numbers: A=1, B=2, C=3, etc.
fn parse_disc_pattern(filename: &str) -> Option<(String, u32)> {
    // Match patterns like "(Disc 1)", "(Disk 2)", "(Disc 1 of 4)",
    // "(Side A)", "(Side B)" etc. Case-insensitive.
    let lower = filename.to_lowercase();

    // Find the disc/disk/tape/part pattern.
    let patterns = ["(disc ", "(disk ", "(tape ", "(part "];
    for pattern in &patterns {
        if let Some(start) = lower.find(pattern) {
            // Find the closing parenthesis.
            if let Some(end) = lower[start..].find(')') {
                let inner = &lower[start + pattern.len()..start + end];
                // Extract the disc number (might be "1", "1 of 4", etc.)
                let num_str = inner.split_whitespace().next()?;
                let disc_num: u32 = num_str.parse().ok()?;

                // Build the base string: everything except the disc pattern.
                // Preserve the original case for the base.
                let base = format!(
                    "{}{}",
                    &filename[..start].trim_end(),
                    &filename[start + end + 1..]
                );

                return Some((base.trim().to_string(), disc_num));
            }
        }
    }

    // Match "(Side A)", "(Side B)", etc. Map letter to number (A=1, B=2, ...).
    if let Some(start) = lower.find("(side ")
        && let Some(end) = lower[start..].find(')')
    {
        let inner = &lower[start + 6..start + end];
        let letter = inner.trim().chars().next()?;
        if letter.is_ascii_alphabetic() {
            let side_num = (letter as u32) - ('a' as u32) + 1;

            let base = format!(
                "{}{}",
                &filename[..start].trim_end(),
                &filename[start + end + 1..]
            );

            return Some((base.trim().to_string(), side_num));
        }
    }

    None
}

/// Detect duplicate ROMs across all systems by file size + name similarity.
pub async fn find_duplicates(storage: &StorageLocation) -> Vec<(RomEntry, RomEntry)> {
    let roms_dir = storage.roms_dir();
    let mut all_roms: Vec<RomEntry> = Vec::new();

    for system in systems::visible_systems() {
        let system_dir = roms_dir.join(system.folder_name);
        if system_dir.exists() {
            let mut raw = Vec::new();
            collect_raw_roms_recursive(&system_dir, &roms_dir, system, &mut raw);
            let mut entries = materialize_rom_entries(system, raw).await;
            all_roms.append(&mut entries);
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

fn count_roms_recursive(dir: &Path, system: &System, roms_root: &Path) -> (usize, u64) {
    count_roms_inner(dir, system, &HashSet::new(), roms_root)
}

fn count_roms_inner(
    dir: &Path,
    system: &System,
    parent_m3u_refs: &HashSet<String>,
    roms_root: &Path,
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
        let (sub_count, sub_size) = count_roms_inner(&subdir, system, &referenced, roms_root);
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
                // Skip orphan M3Us whose targets don't exist on disk.
                if !m3u_has_target_on_disk(&f.path, roms_root) {
                    continue;
                }
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

/// Raw scan result — filename and path only, no catalog-resolved display name.
struct RawRom {
    rom_filename: String,
    rom_path: String,
    size_bytes: u64,
    is_m3u: bool,
}

fn collect_raw_roms_recursive(
    dir: &Path,
    roms_root: &Path,
    system: &System,
    out: &mut Vec<RawRom>,
) {
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
            collect_raw_roms_recursive(&path, roms_root, system, out);
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

            out.push(RawRom {
                rom_filename,
                rom_path,
                size_bytes,
                is_m3u,
            });
        }
    }
}

/// Resolve display names for a raw scan in one batch per system, then build
/// `RomEntry` rows.
async fn materialize_rom_entries(system: &System, raw: Vec<RawRom>) -> Vec<RomEntry> {
    use crate::arcade_db;
    use crate::game_db;

    let is_arcade = systems::is_arcade_system(system.folder_name);
    let mut out: Vec<RomEntry> = Vec::with_capacity(raw.len());

    if is_arcade {
        let stems: Vec<&str> = raw
            .iter()
            .map(|r| replay_control_core::title_utils::filename_stem(&r.rom_filename))
            .collect();
        let mut batch = arcade_db::lookup_arcade_games_batch(&stems).await;
        for r in raw {
            let stem = replay_control_core::title_utils::filename_stem(&r.rom_filename);
            let resolved = batch.remove(stem).map(|info| info.display_name);
            let game =
                GameRef::from_parts(system.folder_name, r.rom_filename, r.rom_path, resolved);
            out.push(RomEntry {
                game,
                size_bytes: r.size_bytes,
                is_m3u: r.is_m3u,
                is_favorite: false,
                box_art_url: None,
                driver_status: None,
                rating: None,
                players: None,
            });
        }
    } else {
        let filenames: Vec<&str> = raw.iter().map(|r| r.rom_filename.as_str()).collect();
        let mut names = game_db::display_names_batch(system.folder_name, &filenames).await;
        for r in raw {
            let resolved = names.remove(&r.rom_filename);
            let game =
                GameRef::from_parts(system.folder_name, r.rom_filename, r.rom_path, resolved);
            out.push(RomEntry {
                game,
                size_bytes: r.size_bytes,
                is_m3u: r.is_m3u,
                is_favorite: false,
                box_art_url: None,
                driver_status: None,
                rating: None,
                players: None,
            });
        }
    }

    out
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

/// Check whether an M3U file's target(s) exist on disk.
///
/// Reads the M3U content, extracts each referenced path, and checks if at
/// least one target file exists. Handles absolute Pi-side paths like
/// `/media/nfs/roms/scummvm/Game/Game.svm` by extracting the portion after
/// `/roms/` and resolving it relative to `roms_root`.
fn m3u_has_target_on_disk(m3u_path: &Path, roms_root: &Path) -> bool {
    let file = match std::fs::File::open(m3u_path) {
        Ok(f) => f,
        Err(_) => return false,
    };

    let reader = BufReader::new(file.take(8192));
    for line_result in reader.lines() {
        let line = match line_result {
            Ok(l) => l,
            Err(_) => break,
        };
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if !looks_like_filename(trimmed) {
            break;
        }
        let normalized = trimmed.replace('\\', "/");

        // Try resolving via /roms/ prefix (absolute Pi-side paths).
        if let Some(after_roms) = normalized.split("/roms/").nth(1)
            && roms_root.join(after_roms).exists()
        {
            return true;
        }

        // Try resolving relative to the M3U file's parent directory.
        if let Some(parent) = m3u_path.parent()
            && parent.join(&normalized).exists()
        {
            return true;
        }
    }

    false
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

    // Collect the set of M3U indices that have at least one reference.
    let m3u_indices_with_refs: HashSet<usize> = referenced_by_m3u
        .values()
        .flat_map(|v| v.iter().copied())
        .collect();

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

    // Orphan M3U detection: an M3U whose references matched zero collected
    // ROMs is a stub (e.g., ScummVM wrapper M3U without actual game files).
    // Remove these so they don't appear as phantom entries.
    for &m3u_idx in &m3u_indices_with_refs {
        if !m3u_extra_size.contains_key(&m3u_idx) {
            indices_to_remove.insert(m3u_idx);
        }
    }

    if indices_to_remove.is_empty() {
        return;
    }

    // Aggregate sizes into M3U entries.
    for (&m3u_idx, &extra) in &m3u_extra_size {
        roms[m3u_idx].size_bytes += extra;
    }

    // Remove referenced disc files and orphan M3Us, preserving order.
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

    if !system.extensions.iter().any(|e| *e == ext_lower) {
        return false;
    }

    // Filter supplementary GD-ROM disc images in arcade_dc.
    // Files like "gdl-0010.chd" and "gds-0009a.chd" are MAME GD-ROM data
    // required alongside the parent ZIP ROM — they are not standalone games.
    if system.folder_name == "arcade_dc"
        && ext_lower == "chd"
        && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
    {
        let lower = stem.to_ascii_lowercase();
        if lower.starts_with("gdl-") || lower.starts_with("gds-") {
            return false;
        }
    }

    true
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
    fn arcade_dc_filters_gdrom_chd_files() {
        let sys = systems::find_system("arcade_dc").unwrap();
        // ZIP ROMs are valid
        assert!(is_rom_file(Path::new("ikaruga.zip"), sys));
        // Standalone CHD (non-GD-ROM name) should be valid
        assert!(is_rom_file(Path::new("somegame.chd"), sys));
        // GD-ROM disc images should be filtered out
        assert!(!is_rom_file(Path::new("gdl-0010.chd"), sys));
        assert!(!is_rom_file(Path::new("gds-0009a.chd"), sys));
        assert!(!is_rom_file(Path::new("GDL-0010.chd"), sys));
        assert!(!is_rom_file(Path::new("GDS-0009A.CHD"), sys));
    }

    #[test]
    fn non_arcade_dc_chd_not_filtered() {
        // CHD files in other systems should NOT be filtered
        let sega_dc = systems::find_system("sega_dc").unwrap();
        assert!(is_rom_file(Path::new("game.chd"), sega_dc));
        assert!(is_rom_file(Path::new("gdl-0010.chd"), sega_dc));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn scan_empty_storage() {
        let tmp = tempdir();
        fs::create_dir_all(tmp.join("roms")).unwrap();
        let storage = StorageLocation::from_path(tmp.clone(), crate::storage::StorageKind::Sd);
        let summaries = scan_systems(&storage).await;
        assert!(!summaries.is_empty());
        assert!(summaries.iter().all(|s| s.game_count == 0));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn scan_with_roms() {
        let tmp = tempdir();
        let nes_dir = tmp.join("roms/nintendo_nes");
        fs::create_dir_all(&nes_dir).unwrap();
        fs::write(nes_dir.join("game1.nes"), "data").unwrap();
        fs::write(nes_dir.join("game2.nes"), "data").unwrap();
        fs::write(nes_dir.join("readme.txt"), "not a rom").unwrap();

        let storage = StorageLocation::from_path(tmp.clone(), crate::storage::StorageKind::Sd);
        let summaries = scan_systems(&storage).await;

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

    #[tokio::test(flavor = "current_thread")]
    async fn m3u_dedup_hides_disc_files() {
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
        fs::write(x68k_dir.join("Game (Disk 1).dim"), [0u8; 100]).unwrap();
        fs::write(x68k_dir.join("Game (Disk 2).dim"), [0u8; 100]).unwrap();
        // Create a standalone file not referenced by any M3U
        fs::write(x68k_dir.join("Other.dim"), [0u8; 50]).unwrap();

        let storage = StorageLocation::from_path(tmp.clone(), crate::storage::StorageKind::Sd);
        let roms = list_roms(&storage, "sharp_x68k", RegionPreference::default(), None)
            .await
            .unwrap();

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

    #[tokio::test(flavor = "current_thread")]
    async fn m3u_dedup_count_is_accurate() {
        let tmp = tempdir();
        let x68k_dir = tmp.join("roms/sharp_x68k");
        fs::create_dir_all(&x68k_dir).unwrap();

        // 1 M3U referencing 3 disc files + 1 standalone
        fs::write(
            x68k_dir.join("Game.m3u"),
            "Game (Disk 1).dim\nGame (Disk 2).dim\nGame (Disk 3).dim\n",
        )
        .unwrap();
        fs::write(x68k_dir.join("Game (Disk 1).dim"), [0u8; 100]).unwrap();
        fs::write(x68k_dir.join("Game (Disk 2).dim"), [0u8; 100]).unwrap();
        fs::write(x68k_dir.join("Game (Disk 3).dim"), [0u8; 100]).unwrap();
        fs::write(x68k_dir.join("Standalone.hdf"), [0u8; 200]).unwrap();

        let storage = StorageLocation::from_path(tmp.clone(), crate::storage::StorageKind::Sd);
        let summaries = scan_systems(&storage).await;
        let x68k = summaries
            .iter()
            .find(|s| s.folder_name == "sharp_x68k")
            .unwrap();

        // Should count 2 games (1 M3U + 1 standalone), not 5
        assert_eq!(x68k.game_count, 2);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn m3u_dedup_case_insensitive() {
        let tmp = tempdir();
        let x68k_dir = tmp.join("roms/sharp_x68k");
        fs::create_dir_all(&x68k_dir).unwrap();

        // M3U references "game.DIM" but file on disk is "game.dim"
        fs::write(x68k_dir.join("Multi.m3u"), "game.DIM\n").unwrap();
        fs::write(x68k_dir.join("game.dim"), [0u8; 100]).unwrap();

        let storage = StorageLocation::from_path(tmp.clone(), crate::storage::StorageKind::Sd);
        let roms = list_roms(&storage, "sharp_x68k", RegionPreference::default(), None)
            .await
            .unwrap();

        // Only the M3U should remain; the .dim should be hidden
        assert_eq!(roms.len(), 1);
        assert!(roms[0].is_m3u);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn scummvm_m3u_dedup_hides_scummvm_in_subfolder() {
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
        fs::write(game_dir.join("GAME.DAT"), [0u8; 1000]).unwrap();

        let storage = StorageLocation::from_path(tmp.clone(), crate::storage::StorageKind::Sd);
        let roms = list_roms(&storage, "scummvm", RegionPreference::default(), None)
            .await
            .unwrap();

        // Should only have the M3U entry; the .scummvm should be hidden
        assert_eq!(roms.len(), 1, "Expected 1 ROM (M3U only), got: {roms:?}");
        assert!(roms[0].is_m3u);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn scummvm_m3u_dedup_count_is_accurate() {
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
        let summaries = scan_systems(&storage).await;
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
        let roms = list_roms(&storage, "scummvm", RegionPreference::default(), None)
            .await
            .unwrap();
        assert_eq!(roms.len(), 2, "Expected 2 ROMs (M3Us only), got: {roms:?}");
        assert!(roms.iter().all(|r| r.is_m3u));
    }

    #[test]
    fn scummvm_svm_extension_recognized() {
        let sys = systems::find_system("scummvm").unwrap();
        assert!(
            sys.extensions.contains(&"svm"),
            "ScummVM system should include 'svm' extension"
        );
        assert!(is_rom_file(Path::new("game.svm"), sys));
        assert!(is_rom_file(Path::new("game.SVM"), sys));
        assert!(is_rom_file(Path::new("game.scummvm"), sys));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn scummvm_svm_with_m3u_hides_svm() {
        let tmp = tempdir();
        let scummvm_dir = tmp.join("roms/scummvm");
        let game_dir = scummvm_dir.join("My Game (CD)");
        fs::create_dir_all(&game_dir).unwrap();

        // M3U references .svm in subfolder (absolute Pi-side path)
        fs::write(
            scummvm_dir.join("My Game (CD).m3u"),
            "/media/nfs/roms/scummvm/My Game (CD)/My Game (CD).svm\n",
        )
        .unwrap();
        fs::write(game_dir.join("My Game (CD).svm"), "scummvm-id").unwrap();
        fs::write(game_dir.join("DATA.PAK"), [0u8; 5000]).unwrap();

        let storage = StorageLocation::from_path(tmp.clone(), crate::storage::StorageKind::Sd);

        // list_roms: only M3U should appear, .svm hidden
        let roms = list_roms(&storage, "scummvm", RegionPreference::default(), None)
            .await
            .unwrap();
        assert_eq!(roms.len(), 1, "Expected 1 ROM (M3U only), got: {roms:?}");
        assert!(roms[0].is_m3u);
        assert_eq!(roms[0].game.rom_filename, "My Game (CD).m3u");

        // scan_systems: count should be 1
        let summaries = scan_systems(&storage).await;
        let scummvm = summaries
            .iter()
            .find(|s| s.folder_name == "scummvm")
            .unwrap();
        assert_eq!(scummvm.game_count, 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn scummvm_orphan_m3u_hidden() {
        let tmp = tempdir();
        let scummvm_dir = tmp.join("roms/scummvm");
        fs::create_dir_all(&scummvm_dir).unwrap();

        // M3U references a .svm that does NOT exist on disk — this is a stub
        fs::write(
            scummvm_dir.join("Missing Game.m3u"),
            "/media/nfs/roms/scummvm/Missing Game/Missing Game.svm\n",
        )
        .unwrap();

        let storage = StorageLocation::from_path(tmp.clone(), crate::storage::StorageKind::Sd);

        // list_roms: orphan M3U should be hidden
        let roms = list_roms(&storage, "scummvm", RegionPreference::default(), None)
            .await
            .unwrap();
        assert_eq!(roms.len(), 0, "Orphan M3U should be hidden, got: {roms:?}");

        // scan_systems: count should be 0
        let summaries = scan_systems(&storage).await;
        let scummvm = summaries
            .iter()
            .find(|s| s.folder_name == "scummvm")
            .unwrap();
        assert_eq!(scummvm.game_count, 0, "Orphan M3U should not be counted");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn scummvm_svm_without_m3u_shown() {
        let tmp = tempdir();
        let scummvm_dir = tmp.join("roms/scummvm");
        let game_dir = scummvm_dir.join("Standalone Game");
        fs::create_dir_all(&game_dir).unwrap();

        // .svm file with no M3U wrapper — should be shown as-is
        fs::write(game_dir.join("Standalone Game.svm"), "scummvm-id").unwrap();
        fs::write(game_dir.join("DATA.PAK"), [0u8; 2000]).unwrap();

        let storage = StorageLocation::from_path(tmp.clone(), crate::storage::StorageKind::Sd);

        let roms = list_roms(&storage, "scummvm", RegionPreference::default(), None)
            .await
            .unwrap();
        assert_eq!(
            roms.len(),
            1,
            "Standalone .svm should appear, got: {roms:?}"
        );
        assert_eq!(roms[0].game.rom_filename, "Standalone Game.svm");
        assert!(!roms[0].is_m3u);

        // scan_systems: should count 1
        let summaries = scan_systems(&storage).await;
        let scummvm = summaries
            .iter()
            .find(|s| s.folder_name == "scummvm")
            .unwrap();
        assert_eq!(scummvm.game_count, 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn psx_m3u_multi_disc_not_affected() {
        // Ensure PSX multi-disc M3U behavior is preserved:
        // M3U shown, disc .chd files hidden.
        let tmp = tempdir();
        let psx_dir = tmp.join("roms/sony_psx");
        fs::create_dir_all(&psx_dir).unwrap();

        // Multi-disc PSX game with M3U
        fs::write(
            psx_dir.join("Final Fantasy VII.m3u"),
            "Final Fantasy VII (Disc 1).chd\nFinal Fantasy VII (Disc 2).chd\nFinal Fantasy VII (Disc 3).chd\n",
        )
        .unwrap();
        fs::write(psx_dir.join("Final Fantasy VII (Disc 1).chd"), [0u8; 500]).unwrap();
        fs::write(psx_dir.join("Final Fantasy VII (Disc 2).chd"), [0u8; 500]).unwrap();
        fs::write(psx_dir.join("Final Fantasy VII (Disc 3).chd"), [0u8; 500]).unwrap();

        // Single-disc PSX game (no M3U)
        fs::write(psx_dir.join("Crash Bandicoot.chd"), [0u8; 300]).unwrap();

        let storage = StorageLocation::from_path(tmp.clone(), crate::storage::StorageKind::Sd);

        // list_roms: M3U + standalone game
        let roms = list_roms(&storage, "sony_psx", RegionPreference::default(), None)
            .await
            .unwrap();
        assert_eq!(roms.len(), 2, "Expected 2 ROMs, got: {roms:?}");

        let m3u = roms.iter().find(|r| r.is_m3u).unwrap();
        assert_eq!(m3u.game.rom_filename, "Final Fantasy VII.m3u");
        // M3U size should include disc file sizes
        let m3u_own_size = fs::metadata(psx_dir.join("Final Fantasy VII.m3u"))
            .unwrap()
            .len();
        assert_eq!(m3u.size_bytes, m3u_own_size + 1500);

        let standalone = roms.iter().find(|r| !r.is_m3u).unwrap();
        assert_eq!(standalone.game.rom_filename, "Crash Bandicoot.chd");

        // scan_systems: should count 2
        let summaries = scan_systems(&storage).await;
        let psx = summaries
            .iter()
            .find(|s| s.folder_name == "sony_psx")
            .unwrap();
        assert_eq!(psx.game_count, 2);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn x68k_m3u_multi_disc_not_affected() {
        // Ensure X68000 multi-disc M3U behavior is preserved.
        let tmp = tempdir();
        let x68k_dir = tmp.join("roms/sharp_x68k");
        fs::create_dir_all(&x68k_dir).unwrap();

        fs::write(
            x68k_dir.join("Game.m3u"),
            "Game (Disk 1).dim\nGame (Disk 2).dim\n",
        )
        .unwrap();
        fs::write(x68k_dir.join("Game (Disk 1).dim"), [0u8; 100]).unwrap();
        fs::write(x68k_dir.join("Game (Disk 2).dim"), [0u8; 100]).unwrap();
        fs::write(x68k_dir.join("Other.dim"), [0u8; 50]).unwrap();

        let storage = StorageLocation::from_path(tmp.clone(), crate::storage::StorageKind::Sd);

        let roms = list_roms(&storage, "sharp_x68k", RegionPreference::default(), None)
            .await
            .unwrap();
        assert_eq!(
            roms.len(),
            2,
            "Expected 2 ROMs (M3U + standalone), got: {roms:?}"
        );
        assert!(
            roms.iter()
                .any(|r| r.is_m3u && r.game.rom_filename == "Game.m3u")
        );
        assert!(roms.iter().any(|r| r.game.rom_filename == "Other.dim"));

        let summaries = scan_systems(&storage).await;
        let x68k = summaries
            .iter()
            .find(|s| s.folder_name == "sharp_x68k")
            .unwrap();
        assert_eq!(x68k.game_count, 2);
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

    // --- parse_cue_file_references ---

    #[test]
    fn parse_cue_simple_bin() {
        let tmp = tempdir();
        let cue = tmp.join("game.cue");
        fs::write(
            &cue,
            "FILE \"GAME.BIN\" BINARY\n  TRACK 01 MODE1/2352\n    INDEX 01 00:00:00\n",
        )
        .unwrap();
        let refs = parse_cue_file_references(&cue);
        assert_eq!(refs, vec!["GAME.BIN"]);
    }

    #[test]
    fn parse_cue_multiple_bins() {
        let tmp = tempdir();
        let cue = tmp.join("game.cue");
        fs::write(
            &cue,
            "FILE \"Track 01.bin\" BINARY\n  TRACK 01 MODE1/2352\n    INDEX 01 00:00:00\n\
             FILE \"Track 02.bin\" BINARY\n  TRACK 02 AUDIO\n    INDEX 01 00:00:00\n",
        )
        .unwrap();
        let refs = parse_cue_file_references(&cue);
        assert_eq!(refs, vec!["Track 01.bin", "Track 02.bin"]);
    }

    #[test]
    fn parse_cue_nonexistent() {
        let refs = parse_cue_file_references(Path::new("/nonexistent/game.cue"));
        assert!(refs.is_empty());
    }

    // --- is_binary_m3u ---

    #[test]
    fn detect_binary_m3u() {
        let tmp = tempdir();
        let m3u = tmp.join("game.m3u");
        let mut content = b"Game.dim\r\n".to_vec();
        content.extend_from_slice(&[0xe5; 256]);
        fs::write(&m3u, &content).unwrap();
        assert!(is_binary_m3u(&m3u));
    }

    #[test]
    fn detect_text_m3u() {
        let tmp = tempdir();
        let m3u = tmp.join("game.m3u");
        fs::write(&m3u, "Disc1.chd\nDisc2.chd\n").unwrap();
        assert!(!is_binary_m3u(&m3u));
    }

    // --- parse_disc_pattern ---

    #[test]
    fn parse_disc_pattern_standard() {
        let (base, num) = parse_disc_pattern("Panzer Dragoon Saga (USA) (Disc 1).chd").unwrap();
        assert_eq!(num, 1);
        assert_eq!(base, "Panzer Dragoon Saga (USA).chd");
    }

    #[test]
    fn parse_disc_pattern_disk_variant() {
        let (base, num) = parse_disc_pattern("Game (1989)(System Sacom)(Disk 2 of 4).dim").unwrap();
        assert_eq!(num, 2);
        assert_eq!(base, "Game (1989)(System Sacom).dim");
    }

    #[test]
    fn parse_disc_pattern_no_match() {
        assert!(parse_disc_pattern("Sonic The Hedgehog (USA).md").is_none());
    }

    #[test]
    fn parse_disc_pattern_side_a() {
        let (base, num) = parse_disc_pattern("Arkanoid (1987)(Imagine)(GB)(Side A).dsk").unwrap();
        assert_eq!(num, 1);
        assert_eq!(base, "Arkanoid (1987)(Imagine)(GB).dsk");
    }

    #[test]
    fn parse_disc_pattern_side_b() {
        let (base, num) = parse_disc_pattern("Arkanoid (1987)(Imagine)(GB)(Side B).dsk").unwrap();
        assert_eq!(num, 2);
        assert_eq!(base, "Arkanoid (1987)(Imagine)(GB).dsk");
    }

    #[test]
    fn parse_disc_pattern_side_c() {
        let (base, num) = parse_disc_pattern("Game (1990)(Publisher)(Side C).dsk").unwrap();
        assert_eq!(num, 3);
        assert_eq!(base, "Game (1990)(Publisher).dsk");
    }

    #[test]
    fn parse_disc_pattern_tape() {
        let (base, num) =
            parse_disc_pattern("UROK (2019)(RetroWorks)(es)(Tape 1 of 3).cdt").unwrap();
        assert_eq!(num, 1);
        assert_eq!(base, "UROK (2019)(RetroWorks)(es).cdt");
    }

    #[test]
    fn parse_disc_pattern_tape_2() {
        let (base, num) =
            parse_disc_pattern("UROK (2019)(RetroWorks)(es)(Tape 2 of 3).cdt").unwrap();
        assert_eq!(num, 2);
        assert_eq!(base, "UROK (2019)(RetroWorks)(es).cdt");
    }

    #[test]
    fn parse_disc_pattern_part() {
        let (base, num) =
            parse_disc_pattern("Cero Absoluto (2016)(ESP Soft)(es)(Part 1 of 2).cdt").unwrap();
        assert_eq!(num, 1);
        assert_eq!(base, "Cero Absoluto (2016)(ESP Soft)(es).cdt");
    }

    #[test]
    fn parse_disc_pattern_part_2() {
        let (base, num) =
            parse_disc_pattern("Cero Absoluto (2016)(ESP Soft)(es)(Part 2 of 2).cdt").unwrap();
        assert_eq!(num, 2);
        assert_eq!(base, "Cero Absoluto (2016)(ESP Soft)(es).cdt");
    }

    // --- detect_disc_set ---

    #[test]
    fn detect_multi_disc_set() {
        let tmp = tempdir();
        let saturn_dir = tmp.join("roms/sega_st");
        fs::create_dir_all(&saturn_dir).unwrap();

        fs::write(saturn_dir.join("PDS (USA) (Disc 1).chd"), [0u8; 100]).unwrap();
        fs::write(saturn_dir.join("PDS (USA) (Disc 2).chd"), [0u8; 100]).unwrap();
        fs::write(saturn_dir.join("PDS (USA) (Disc 3).chd"), [0u8; 100]).unwrap();
        fs::write(saturn_dir.join("PDS (USA) (Disc 4).chd"), [0u8; 100]).unwrap();

        let storage = StorageLocation::from_path(tmp.clone(), crate::storage::StorageKind::Sd);
        let info = detect_disc_set(&storage, "sega_st", "PDS (USA) (Disc 1).chd").unwrap();

        assert_eq!(info.disc_number, 1);
        assert_eq!(info.total_discs, 4);
        assert_eq!(info.siblings.len(), 4);
    }

    #[test]
    fn detect_single_disc_returns_none() {
        let tmp = tempdir();
        let saturn_dir = tmp.join("roms/sega_st");
        fs::create_dir_all(&saturn_dir).unwrap();

        fs::write(saturn_dir.join("Game (Disc 1).chd"), [0u8; 100]).unwrap();

        let storage = StorageLocation::from_path(tmp.clone(), crate::storage::StorageKind::Sd);
        assert!(detect_disc_set(&storage, "sega_st", "Game (Disc 1).chd").is_none());
    }

    // --- list_rom_group ---

    #[test]
    fn group_single_file() {
        let tmp = tempdir();
        let nes_dir = tmp.join("roms/nintendo_nes");
        fs::create_dir_all(&nes_dir).unwrap();
        fs::write(nes_dir.join("Sonic.nes"), [0u8; 256]).unwrap();

        let storage = StorageLocation::from_path(tmp.clone(), crate::storage::StorageKind::Sd);
        let group =
            list_rom_group(&storage, "nintendo_nes", "roms/nintendo_nes/Sonic.nes").unwrap();

        assert_eq!(group.len(), 1);
        assert_eq!(group[0].kind, FileKind::Primary);
        assert_eq!(group[0].size_bytes, 256);
    }

    #[test]
    fn group_m3u_with_disc_files() {
        let tmp = tempdir();
        let psx_dir = tmp.join("roms/sony_psx");
        fs::create_dir_all(&psx_dir).unwrap();

        fs::write(
            psx_dir.join("FF7.m3u"),
            "FF7 (Disc 1).chd\nFF7 (Disc 2).chd\nFF7 (Disc 3).chd\n",
        )
        .unwrap();
        fs::write(psx_dir.join("FF7 (Disc 1).chd"), [0u8; 500]).unwrap();
        fs::write(psx_dir.join("FF7 (Disc 2).chd"), [0u8; 600]).unwrap();
        fs::write(psx_dir.join("FF7 (Disc 3).chd"), [0u8; 700]).unwrap();

        let storage = StorageLocation::from_path(tmp.clone(), crate::storage::StorageKind::Sd);
        let group = list_rom_group(&storage, "sony_psx", "roms/sony_psx/FF7.m3u").unwrap();

        // Should have: 1 primary (M3U) + 3 disc files
        assert_eq!(group.len(), 4);
        assert_eq!(
            group.iter().filter(|g| g.kind == FileKind::Primary).count(),
            1
        );
        assert_eq!(group.iter().filter(|g| g.kind == FileKind::Disc).count(), 3);
        let total_size: u64 = group.iter().map(|g| g.size_bytes).sum();
        assert!(total_size > 1800); // At least the disc file sizes
    }

    #[test]
    fn group_m3u_with_sbi_companions() {
        let tmp = tempdir();
        let psx_dir = tmp.join("roms/sony_psx");
        fs::create_dir_all(&psx_dir).unwrap();

        fs::write(psx_dir.join("FF8.m3u"), "FF8 (Disc 1).chd\n").unwrap();
        fs::write(psx_dir.join("FF8 (Disc 1).chd"), [0u8; 500]).unwrap();
        fs::write(psx_dir.join("FF8 (Disc 1).sbi"), [0u8; 50]).unwrap();

        let storage = StorageLocation::from_path(tmp.clone(), crate::storage::StorageKind::Sd);
        let group = list_rom_group(&storage, "sony_psx", "roms/sony_psx/FF8.m3u").unwrap();

        // M3U + disc CHD + SBI companion
        assert_eq!(group.len(), 3);
        assert!(group.iter().any(|g| g.kind == FileKind::Companion));
    }

    #[test]
    fn group_cue_with_bin() {
        let tmp = tempdir();
        let saturn_dir = tmp.join("roms/sega_st/Game");
        fs::create_dir_all(&saturn_dir).unwrap();

        fs::write(
            saturn_dir.join("Game.cue"),
            "FILE \"GAME.BIN\" BINARY\n  TRACK 01 MODE1/2352\n",
        )
        .unwrap();
        fs::write(saturn_dir.join("GAME.BIN"), [0u8; 1000]).unwrap();

        let storage = StorageLocation::from_path(tmp.clone(), crate::storage::StorageKind::Sd);
        let group = list_rom_group(&storage, "sega_st", "roms/sega_st/Game/Game.cue").unwrap();

        assert_eq!(group.len(), 2);
        assert!(group.iter().any(|g| g.kind == FileKind::Primary));
        assert!(group.iter().any(|g| g.kind == FileKind::Disc));
    }

    #[test]
    fn group_chd_with_sbi() {
        let tmp = tempdir();
        let psx_dir = tmp.join("roms/sony_psx");
        fs::create_dir_all(&psx_dir).unwrap();

        fs::write(psx_dir.join("Game.chd"), [0u8; 500]).unwrap();
        fs::write(psx_dir.join("Game.sbi"), [0u8; 30]).unwrap();

        let storage = StorageLocation::from_path(tmp.clone(), crate::storage::StorageKind::Sd);
        let group = list_rom_group(&storage, "sony_psx", "roms/sony_psx/Game.chd").unwrap();

        assert_eq!(group.len(), 2);
        assert!(group.iter().any(|g| g.kind == FileKind::Primary));
        assert!(group.iter().any(|g| g.kind == FileKind::Companion));
    }

    #[test]
    fn group_scummvm_m3u_includes_game_dir() {
        let tmp = tempdir();
        let scummvm_dir = tmp.join("roms/scummvm");
        let game_dir = scummvm_dir.join("Cool Game (CD)");
        fs::create_dir_all(&game_dir).unwrap();

        fs::write(
            scummvm_dir.join("Cool Game (CD).m3u"),
            format!(
                "{}/Cool Game (CD)/Cool Game (CD).svm\n",
                scummvm_dir.display()
            ),
        )
        .unwrap();
        fs::write(game_dir.join("Cool Game (CD).svm"), "scummvm-id").unwrap();
        fs::write(game_dir.join("DATA.PAK"), [0u8; 5000]).unwrap();

        let storage = StorageLocation::from_path(tmp.clone(), crate::storage::StorageKind::Sd);
        let group = list_rom_group(&storage, "scummvm", "roms/scummvm/Cool Game (CD).m3u").unwrap();

        // Primary M3U + DataDir
        assert_eq!(group.len(), 2);
        assert!(group.iter().any(|g| g.kind == FileKind::Primary));
        assert!(group.iter().any(|g| g.kind == FileKind::DataDir));
        // DataDir should include size of both files in the game directory
        let dir_entry = group.iter().find(|g| g.kind == FileKind::DataDir).unwrap();
        assert!(dir_entry.size_bytes >= 5000);
    }

    // --- delete_rom_group ---

    #[test]
    fn delete_single_file_group() {
        let tmp = tempdir();
        let nes_dir = tmp.join("roms/nintendo_nes");
        fs::create_dir_all(&nes_dir).unwrap();
        fs::write(nes_dir.join("Game.nes"), [0u8; 256]).unwrap();

        let storage = StorageLocation::from_path(tmp.clone(), crate::storage::StorageKind::Sd);
        let report =
            delete_rom_group(&storage, "nintendo_nes", "roms/nintendo_nes/Game.nes").unwrap();

        assert_eq!(report.deleted.len(), 1);
        assert_eq!(report.bytes_freed, 256);
        assert!(report.errors.is_empty());
        assert!(!nes_dir.join("Game.nes").exists());
    }

    #[test]
    fn delete_m3u_with_discs() {
        let tmp = tempdir();
        let psx_dir = tmp.join("roms/sony_psx");
        fs::create_dir_all(&psx_dir).unwrap();

        fs::write(
            psx_dir.join("Game.m3u"),
            "Game (Disc 1).chd\nGame (Disc 2).chd\n",
        )
        .unwrap();
        fs::write(psx_dir.join("Game (Disc 1).chd"), [0u8; 500]).unwrap();
        fs::write(psx_dir.join("Game (Disc 2).chd"), [0u8; 600]).unwrap();

        let storage = StorageLocation::from_path(tmp.clone(), crate::storage::StorageKind::Sd);
        let report = delete_rom_group(&storage, "sony_psx", "roms/sony_psx/Game.m3u").unwrap();

        assert!(report.errors.is_empty());
        assert!(!psx_dir.join("Game.m3u").exists());
        assert!(!psx_dir.join("Game (Disc 1).chd").exists());
        assert!(!psx_dir.join("Game (Disc 2).chd").exists());
    }

    #[test]
    fn delete_cue_bin_in_subdir() {
        let tmp = tempdir();
        let saturn_dir = tmp.join("roms/sega_st/Game");
        fs::create_dir_all(&saturn_dir).unwrap();

        fs::write(saturn_dir.join("Game.cue"), "FILE \"GAME.BIN\" BINARY\n").unwrap();
        fs::write(saturn_dir.join("GAME.BIN"), [0u8; 1000]).unwrap();

        let storage = StorageLocation::from_path(tmp.clone(), crate::storage::StorageKind::Sd);
        let report = delete_rom_group(&storage, "sega_st", "roms/sega_st/Game/Game.cue").unwrap();

        assert!(report.errors.is_empty());
        assert!(!saturn_dir.join("Game.cue").exists());
        assert!(!saturn_dir.join("GAME.BIN").exists());
        // Empty subdirectory should be cleaned up.
        assert!(!saturn_dir.exists());
    }

    #[test]
    fn delete_scummvm_with_game_dir() {
        let tmp = tempdir();
        let scummvm_dir = tmp.join("roms/scummvm");
        let game_dir = scummvm_dir.join("Cool Game");
        fs::create_dir_all(&game_dir).unwrap();

        fs::write(
            scummvm_dir.join("Cool Game.m3u"),
            format!("{}/Cool Game/Cool Game.svm\n", scummvm_dir.display()),
        )
        .unwrap();
        fs::write(game_dir.join("Cool Game.svm"), "scummvm-id").unwrap();
        fs::write(game_dir.join("DATA.PAK"), [0u8; 5000]).unwrap();

        let storage = StorageLocation::from_path(tmp.clone(), crate::storage::StorageKind::Sd);
        let report = delete_rom_group(&storage, "scummvm", "roms/scummvm/Cool Game.m3u").unwrap();

        assert!(report.errors.is_empty());
        assert!(!scummvm_dir.join("Cool Game.m3u").exists());
        assert!(!game_dir.exists());
    }

    // --- check_rename_allowed ---

    #[test]
    fn rename_allowed_for_single_file() {
        let tmp = tempdir();
        let nes_dir = tmp.join("roms/nintendo_nes");
        fs::create_dir_all(&nes_dir).unwrap();
        fs::write(nes_dir.join("Game.nes"), [0u8; 10]).unwrap();

        let storage = StorageLocation::from_path(tmp.clone(), crate::storage::StorageKind::Sd);
        let (allowed, reason) =
            check_rename_allowed(&storage, "nintendo_nes", "roms/nintendo_nes/Game.nes");
        assert!(allowed);
        assert!(reason.is_none());
    }

    #[test]
    fn rename_blocked_for_cue() {
        let tmp = tempdir();
        let dir = tmp.join("roms/sega_st");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("Game.cue"), "FILE \"G.BIN\" BINARY\n").unwrap();

        let storage = StorageLocation::from_path(tmp.clone(), crate::storage::StorageKind::Sd);
        let (allowed, reason) = check_rename_allowed(&storage, "sega_st", "roms/sega_st/Game.cue");
        assert!(!allowed);
        assert!(reason.is_some());
    }

    #[test]
    fn rename_blocked_for_scummvm_m3u() {
        let tmp = tempdir();
        let dir = tmp.join("roms/scummvm");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("Game.m3u"), "/path/to/Game.svm\n").unwrap();

        let storage = StorageLocation::from_path(tmp.clone(), crate::storage::StorageKind::Sd);
        let (allowed, reason) = check_rename_allowed(&storage, "scummvm", "roms/scummvm/Game.m3u");
        assert!(!allowed);
        assert!(reason.is_some());
    }

    #[test]
    fn rename_blocked_for_binary_m3u() {
        let tmp = tempdir();
        let dir = tmp.join("roms/sharp_x68k");
        fs::create_dir_all(&dir).unwrap();

        let mut content = b"Game.dim\r\n".to_vec();
        content.extend_from_slice(&[0xe5; 256]);
        fs::write(dir.join("Game.m3u"), &content).unwrap();

        let storage = StorageLocation::from_path(tmp.clone(), crate::storage::StorageKind::Sd);
        let (allowed, reason) =
            check_rename_allowed(&storage, "sharp_x68k", "roms/sharp_x68k/Game.m3u");
        assert!(!allowed);
        assert!(reason.is_some());
    }

    #[test]
    fn rename_allowed_for_text_m3u() {
        let tmp = tempdir();
        let dir = tmp.join("roms/sony_psx");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("Game.m3u"), "Game (Disc 1).chd\n").unwrap();

        let storage = StorageLocation::from_path(tmp.clone(), crate::storage::StorageKind::Sd);
        let (allowed, _) = check_rename_allowed(&storage, "sony_psx", "roms/sony_psx/Game.m3u");
        assert!(allowed);
    }
}
