//! Shared image matching logic for resolving ROM filenames to thumbnail image paths.
//!
//! Used by both the thumbnail import pipeline (import.rs) and the runtime image
//! cache (cache/images.rs) to avoid duplicating the multi-tier fuzzy matching logic.

use std::collections::HashMap;
use std::path::Path;

use crate::thumbnails::{base_title, strip_version, thumbnail_filename};
use crate::title_utils::strip_n64dd_prefix;

/// Directory index for matching ROM filenames to image files.
///
/// Built from a single readdir scan, provides O(1) lookups at multiple
/// fuzziness tiers: exact, case-insensitive, base_title, and version-stripped.
pub struct DirIndex {
    /// Exact thumbnail_filename stem → relative path (e.g., "boxart/Name.png")
    pub exact: HashMap<String, String>,
    /// Lowercase stem → relative path (case-insensitive exact match)
    pub exact_ci: HashMap<String, String>,
    /// base_title (tags stripped) → relative path
    pub fuzzy: HashMap<String, String>,
    /// version-stripped base_title → relative path
    pub version: HashMap<String, String>,
}

/// Build a `DirIndex` by scanning a media subdirectory (boxart or snap).
///
/// `dir` is the full path to the image directory.
/// `kind` is the subdirectory label used in relative paths (e.g., "boxart", "snap").
///
/// Only indexes `.png` files that are at least 200 bytes (to skip fake symlinks/stubs).
pub fn build_dir_index(dir: &Path, kind: &str) -> DirIndex {
    let mut exact = HashMap::new();
    let mut exact_ci = HashMap::new();
    let mut fuzzy = HashMap::new();
    let mut version = HashMap::new();

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if let Some(img_stem) = name_str.strip_suffix(".png") {
                // Skip tiny files (fake symlinks / stubs).
                let valid = entry.metadata().map(|m| m.len() >= 200).unwrap_or(false);
                if !valid {
                    continue;
                }
                let path = format!("{kind}/{name_str}");
                exact.insert(img_stem.to_string(), path.clone());
                exact_ci
                    .entry(img_stem.to_lowercase())
                    .or_insert_with(|| path.clone());
                let bt = base_title(img_stem);
                let vs = strip_version(&bt).to_string();
                fuzzy.entry(bt.clone()).or_insert_with(|| path.clone());
                if vs.len() < bt.len() {
                    version.entry(vs).or_insert(path);
                }
            }
        }
    }

    DirIndex {
        exact,
        exact_ci,
        fuzzy,
        version,
    }
}

/// Find the best matching image for a ROM in a `DirIndex`.
///
/// Matching tiers (in order):
/// 1. Exact thumbnail_filename match
/// 2. Colon variants for arcade games (": " → " - " and ": " → " ")
/// 3. Case-insensitive exact match
/// 4. Base title (strip region/version tags)
/// 5. Version-stripped base title
///
/// `rom_filename` is the ROM file name (with extension).
/// `arcade_display` is the translated display name for arcade ROMs (None for non-arcade).
pub fn find_best_match(
    index: &DirIndex,
    rom_filename: &str,
    arcade_display: Option<&str>,
) -> Option<String> {
    let stem = rom_filename
        .rfind('.')
        .map(|i| &rom_filename[..i])
        .unwrap_or(rom_filename);
    let stem = strip_n64dd_prefix(stem);
    let source = arcade_display.unwrap_or(stem);
    let thumb_name = thumbnail_filename(source);

    // Tier 1: exact
    if let Some(path) = index.exact.get(&thumb_name) {
        return Some(path.clone());
    }

    // Colon variants for arcade games.
    if source.contains(':') {
        let dash = thumbnail_filename(&source.replace(": ", " - ").replace(':', " -"));
        if let Some(path) = index.exact.get(&dash) {
            return Some(path.clone());
        }
        let drop = thumbnail_filename(&source.replace(": ", " ").replace(':', ""));
        if let Some(path) = index.exact.get(&drop) {
            return Some(path.clone());
        }
    }

    // Tier 2: case-insensitive exact (preserves region tags)
    if let Some(path) = index.exact_ci.get(&thumb_name.to_lowercase()) {
        return Some(path.clone());
    }

    // Tier 3: base title (strip tags)
    let base = base_title(&thumb_name);
    if let Some(path) = index.fuzzy.get(&base) {
        return Some(path.clone());
    }

    // Tier 4: version-stripped
    let vs = strip_version(&base);
    if vs.len() < base.len()
        && let Some(path) = index.fuzzy.get(vs).or_else(|| index.version.get(vs))
    {
        return Some(path.clone());
    }

    None
}
