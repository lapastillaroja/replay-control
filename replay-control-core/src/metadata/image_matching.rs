//! Shared image matching logic for resolving ROM filenames to thumbnail image paths.
//!
//! Used by both the thumbnail import pipeline (import.rs) and the runtime image
//! cache (cache/images.rs) to avoid duplicating the multi-tier fuzzy matching logic.

use std::collections::HashMap;
use std::path::Path;

use crate::thumbnails::{base_title, is_valid_image, strip_version, thumbnail_filename};
use crate::title_utils::{strip_n64dd_prefix, strip_tags};

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
                if !is_valid_image(&entry.path()) {
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
/// 1. DB path lookup (if `db_paths` provided and entry exists on disk)
/// 2. Exact thumbnail_filename match
/// 3. Colon variants for arcade games (": " → " - " and ": " → " ")
/// 4. Case-insensitive exact match
/// 5. Base title (strip region/version tags)
/// 6. Tilde dual-title split — for names with ` ~ `, try each half through exact + fuzzy
/// 7. Version-stripped base title
///
/// `rom_filename` is the ROM file name (with extension).
/// `arcade_display` is the translated display name for arcade ROMs (None for non-arcade).
/// `db_paths` is an optional map from rom_filename → relative image path (e.g., "boxart/Name.png").
///   When provided, a DB path is checked first but only returned if the referenced file
///   exists in `index.exact` (i.e., the image is actually on disk).
pub fn find_best_match(
    index: &DirIndex,
    rom_filename: &str,
    arcade_display: Option<&str>,
    db_paths: Option<&HashMap<String, String>>,
) -> Option<String> {
    // Tier 1: DB path lookup.
    if let Some(db_paths) = db_paths
        && let Some(db_path) = db_paths.get(rom_filename)
    {
        let stem = db_path
            .strip_prefix("boxart/")
            .unwrap_or(db_path)
            .strip_suffix(".png")
            .unwrap_or(db_path);
        if index.exact.contains_key(stem) {
            return Some(db_path.clone());
        }
    }

    let stem = rom_filename
        .rfind('.')
        .map(|i| &rom_filename[..i])
        .unwrap_or(rom_filename);
    let stem = strip_n64dd_prefix(stem);
    let source = arcade_display.unwrap_or(stem);
    let thumb_name = thumbnail_filename(source);

    // Tier 2: exact
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

    // Tier 3: case-insensitive exact (preserves region tags)
    if let Some(path) = index.exact_ci.get(&thumb_name.to_lowercase()) {
        return Some(path.clone());
    }

    // Tier 4: base title (strip tags)
    let base = base_title(&thumb_name);
    if let Some(path) = index.fuzzy.get(&base) {
        return Some(path.clone());
    }

    // Tier 5: tilde dual-title split — try each half through exact + fuzzy.
    // For ROMs like "Bare Knuckle ~ Streets of Rage", try "Bare Knuckle" and
    // "Streets of Rage" independently. Note: base_title() already takes the
    // right half, so this tier adds coverage for the left half and explicit
    // exact matching of each half.
    if source.contains(" ~ ") {
        for half in source.split(" ~ ") {
            let half = half.trim();
            let half_thumb = thumbnail_filename(half);
            if let Some(path) = index.exact.get(&half_thumb) {
                return Some(path.clone());
            }
            let half_base = base_title(&half_thumb);
            if let Some(path) = index.fuzzy.get(&half_base) {
                return Some(path.clone());
            }
        }
    }

    // Tier 6: version-stripped
    let vs = strip_version(&base);
    if vs.len() < base.len()
        && let Some(path) = index.fuzzy.get(vs).or_else(|| index.version.get(vs))
    {
        return Some(path.clone());
    }

    None
}

/// Compute tilde half-titles for matching.
///
/// For source names containing ` ~ ` (e.g., "Bare Knuckle ~ Streets of Rage"),
/// returns a Vec of lowercased, tag-stripped half-titles. Used by
/// `find_boxart_variants` and `count_boxart_variants` in `thumbnail_manifest.rs`
/// to match manifest entries against either half of a dual-title ROM.
///
/// Returns an empty Vec if the source contains no ` ~ `.
pub fn tilde_halves(source: &str) -> Vec<String> {
    if source.contains(" ~ ") {
        source
            .split(" ~ ")
            .map(|half| strip_tags(&thumbnail_filename(half.trim())).to_lowercase())
            .collect()
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to build a DirIndex from a list of (stem, relative_path) pairs.
    fn index_from(entries: &[(&str, &str)]) -> DirIndex {
        let mut exact = HashMap::new();
        let mut exact_ci = HashMap::new();
        let mut fuzzy = HashMap::new();
        let mut version = HashMap::new();

        for &(stem, path) in entries {
            exact.insert(stem.to_string(), path.to_string());
            exact_ci
                .entry(stem.to_lowercase())
                .or_insert_with(|| path.to_string());
            let bt = base_title(stem);
            let vs = strip_version(&bt).to_string();
            fuzzy.entry(bt.clone()).or_insert_with(|| path.to_string());
            if vs.len() < bt.len() {
                version.entry(vs).or_insert_with(|| path.to_string());
            }
        }

        DirIndex {
            exact,
            exact_ci,
            fuzzy,
            version,
        }
    }

    #[test]
    fn exact_match() {
        let index = index_from(&[("Sonic the Hedgehog", "boxart/Sonic the Hedgehog.png")]);
        let result = find_best_match(&index, "Sonic the Hedgehog.md", None, None);
        assert_eq!(result.as_deref(), Some("boxart/Sonic the Hedgehog.png"));
    }

    #[test]
    fn case_insensitive_match() {
        let index = index_from(&[("Sonic The Hedgehog", "boxart/Sonic The Hedgehog.png")]);
        let result = find_best_match(&index, "sonic the hedgehog.md", None, None);
        assert_eq!(result.as_deref(), Some("boxart/Sonic The Hedgehog.png"));
    }

    #[test]
    fn fuzzy_base_title_match() {
        let index = index_from(&[("Sonic the Hedgehog", "boxart/Sonic the Hedgehog.png")]);
        let result = find_best_match(&index, "Sonic the Hedgehog (USA).md", None, None);
        assert_eq!(result.as_deref(), Some("boxart/Sonic the Hedgehog.png"));
    }

    #[test]
    fn tilde_left_half_exact() {
        // Image named after left half of tilde title.
        let index = index_from(&[("Bare Knuckle", "boxart/Bare Knuckle.png")]);
        let result = find_best_match(&index, "Bare Knuckle ~ Streets of Rage.md", None, None);
        assert_eq!(result.as_deref(), Some("boxart/Bare Knuckle.png"));
    }

    #[test]
    fn tilde_right_half_exact() {
        // Image named after right half of tilde title.
        let index = index_from(&[("Streets of Rage", "boxart/Streets of Rage.png")]);
        let result = find_best_match(&index, "Bare Knuckle ~ Streets of Rage.md", None, None);
        assert_eq!(result.as_deref(), Some("boxart/Streets of Rage.png"));
    }

    #[test]
    fn tilde_right_half_fuzzy() {
        // Image has tags, matching via fuzzy against right half.
        let index = index_from(&[("Streets of Rage (USA)", "boxart/Streets of Rage (USA).png")]);
        let result = find_best_match(&index, "Bare Knuckle ~ Streets of Rage.md", None, None);
        assert_eq!(result.as_deref(), Some("boxart/Streets of Rage (USA).png"));
    }

    #[test]
    fn tilde_no_match_returns_none() {
        let index = index_from(&[("Unrelated Game", "boxart/Unrelated Game.png")]);
        let result = find_best_match(&index, "Bare Knuckle ~ Streets of Rage.md", None, None);
        assert!(result.is_none());
    }

    #[test]
    fn db_path_used_when_file_exists() {
        let index = index_from(&[("Custom Art", "boxart/Custom Art.png")]);
        let mut db_paths = HashMap::new();
        db_paths.insert("game.rom".to_string(), "boxart/Custom Art.png".to_string());
        let result = find_best_match(&index, "game.rom", None, Some(&db_paths));
        assert_eq!(result.as_deref(), Some("boxart/Custom Art.png"));
    }

    #[test]
    fn db_path_skipped_when_file_missing() {
        // DB says to use "boxart/Missing.png" but it's not on disk.
        let index = index_from(&[("game", "boxart/game.png")]);
        let mut db_paths = HashMap::new();
        db_paths.insert("game.rom".to_string(), "boxart/Missing.png".to_string());
        let result = find_best_match(&index, "game.rom", None, Some(&db_paths));
        // Falls through to exact match on stem.
        assert_eq!(result.as_deref(), Some("boxart/game.png"));
    }

    #[test]
    fn version_stripped_match() {
        let index = index_from(&[("Sonic Adventure 2", "boxart/Sonic Adventure 2.png")]);
        let result = find_best_match(&index, "Sonic Adventure 2 v1.008.md", None, None);
        assert_eq!(result.as_deref(), Some("boxart/Sonic Adventure 2.png"));
    }

    #[test]
    fn tilde_halves_splits_correctly() {
        let halves = tilde_halves("Bare Knuckle ~ Streets of Rage");
        assert_eq!(halves.len(), 2);
        assert_eq!(halves[0], "bare knuckle");
        assert_eq!(halves[1], "streets of rage");
    }

    #[test]
    fn tilde_halves_empty_for_no_tilde() {
        let halves = tilde_halves("Sonic the Hedgehog");
        assert!(halves.is_empty());
    }
}
