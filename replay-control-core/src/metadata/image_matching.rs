//! Shared image matching logic for resolving ROM filenames to thumbnail image paths.
//!
//! Used by both the thumbnail import pipeline (import.rs) and the runtime image
//! cache (cache/images.rs) to avoid duplicating the multi-tier fuzzy matching logic.

use std::collections::HashMap;
use std::path::Path;

use crate::thumbnails::{base_title, is_valid_image, strip_version, thumbnail_filename};
use crate::title_utils::{normalize_aggressive, strip_n64dd_prefix, strip_tags};

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
    /// Aggressively normalized (all punctuation stripped) → relative path
    pub aggressive: HashMap<String, String>,
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
    let mut aggressive = HashMap::new();

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
                    version.entry(vs).or_insert_with(|| path.clone());
                }
                // Aggressive: strip all punctuation for last-resort matching.
                let agg = normalize_aggressive(&bt);
                aggressive.entry(agg).or_insert(path);
            }
        }
    }

    DirIndex {
        exact,
        exact_ci,
        fuzzy,
        version,
        aggressive,
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

    // Tier 7: aggressive normalization (strip all punctuation, last resort)
    let agg = normalize_aggressive(&base);
    if !agg.is_empty()
        && let Some(path) = index.aggressive.get(&agg)
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

/// Check whether two base titles match allowing for platform-identifier suffixes.
///
/// When one base title is a word-aligned prefix of the other (e.g., `"corpse killer"`
/// vs `"corpse killer 32x"`), the extra trailing words are checked against the
/// parenthesized tags of the *shorter* entry's original filename. If every extra word
/// appears inside those tags, the titles are considered equivalent.
///
/// This handles cases where libretro-thumbnails embeds a platform identifier in the
/// title stem (e.g., `Corpse Killer 32X (Europe)`) while the ROM keeps it inside a
/// tag (e.g., `Corpse Killer (USA) (Sega CD 32X)`).
///
/// Both `base_a` and `base_b` must already be lowercased.
pub fn base_titles_match_with_tags(
    base_a: &str,
    original_a: &str,
    base_b: &str,
    original_b: &str,
) -> bool {
    // Try both directions: a is prefix of b, or b is prefix of a.
    prefix_extra_in_tags(base_a, base_b, original_a)
        || prefix_extra_in_tags(base_b, base_a, original_b)
}

/// Returns true if `shorter` is a word-aligned prefix of `longer` and every extra
/// trailing word in `longer` appears inside the parenthesized tags of `shorter_original`.
fn prefix_extra_in_tags(shorter: &str, longer: &str, shorter_original: &str) -> bool {
    // `shorter` must be a strict prefix followed by a space.
    let suffix = match longer.strip_prefix(shorter) {
        Some(s) if !s.is_empty() => s,
        _ => return false,
    };
    // Must be a word boundary (space after the prefix).
    if !suffix.starts_with(' ') {
        return false;
    }
    let suffix = suffix.trim_start();
    if suffix.is_empty() {
        return false;
    }

    let tags = collect_tag_words(shorter_original);
    if tags.is_empty() {
        return false;
    }

    suffix
        .split_whitespace()
        .all(|word| tags.iter().any(|t| t == word))
}

/// Extract all whitespace-separated words from parenthesized groups, lowercased.
///
/// `"Corpse Killer (USA) (Sega CD 32X)"` → `{"usa", "sega", "cd", "32x"}`
fn collect_tag_words(filename: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut rest = filename;
    while let Some(open) = rest.find('(') {
        if let Some(close) = rest[open..].find(')') {
            let inside = &rest[open + 1..open + close];
            for w in inside.split_whitespace() {
                words.push(w.to_lowercase());
            }
            rest = &rest[open + close + 1..];
        } else {
            break;
        }
    }
    words
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
        let mut aggressive = HashMap::new();

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
            let agg = normalize_aggressive(&bt);
            aggressive.entry(agg).or_insert_with(|| path.to_string());
        }

        DirIndex {
            exact,
            exact_ci,
            fuzzy,
            version,
            aggressive,
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

    // --- base_titles_match_with_tags ---

    #[test]
    fn tag_match_platform_suffix_in_thumbnail() {
        // ROM: "Corpse Killer (USA) (Sega CD 32X)" → base "corpse killer"
        // Thumbnail: "Corpse Killer 32X (Europe)" → base "corpse killer 32x"
        // "32x" appears in ROM's tags → match.
        assert!(base_titles_match_with_tags(
            "corpse killer",
            "Corpse Killer (USA) (Sega CD 32X)",
            "corpse killer 32x",
            "Corpse Killer 32X (Europe)",
        ));
    }

    #[test]
    fn tag_match_does_not_match_sequel_number() {
        // "Corpse Killer" should NOT match "Corpse Killer 2" — "2" is not in any tag.
        assert!(!base_titles_match_with_tags(
            "corpse killer",
            "Corpse Killer (USA)",
            "corpse killer 2",
            "Corpse Killer 2 (USA)",
        ));
    }

    #[test]
    fn tag_match_exact_titles_not_needed() {
        // When base titles are identical, the caller handles it; this function
        // is only invoked when they differ, but it should still return false
        // for identical titles (no prefix remainder).
        assert!(!base_titles_match_with_tags(
            "sonic",
            "Sonic (USA)",
            "sonic",
            "Sonic (Europe)",
        ));
    }

    #[test]
    fn tag_match_multi_word_suffix() {
        // Extra suffix has multiple words, all must be in the shorter entry's tags.
        assert!(base_titles_match_with_tags(
            "game",
            "Game (Super Turbo Edition)",
            "game super turbo",
            "Game Super Turbo (USA)",
        ));
        // Only one word matches — should fail.
        assert!(!base_titles_match_with_tags(
            "game",
            "Game (Super Edition)",
            "game super turbo",
            "Game Super Turbo (USA)",
        ));
    }

    #[test]
    fn tag_match_reverse_direction() {
        // Entry base is shorter, ROM base is longer — extra words must be
        // in the entry's tags.
        assert!(base_titles_match_with_tags(
            "game turbo",
            "Game Turbo (USA)",
            "game",
            "Game (Turbo Edition)",
        ));
    }

    #[test]
    fn tag_match_no_tags_no_match() {
        // If the shorter entry has no parenthesized tags, can't verify the
        // extra words — should not match.
        assert!(!base_titles_match_with_tags(
            "corpse killer",
            "Corpse Killer",
            "corpse killer 32x",
            "Corpse Killer 32X (Europe)",
        ));
    }

    #[test]
    fn collect_tag_words_basic() {
        let words = collect_tag_words("Corpse Killer (USA) (Sega CD 32X)");
        assert_eq!(words, vec!["usa", "sega", "cd", "32x"]);
    }

    #[test]
    fn collect_tag_words_empty() {
        let words = collect_tag_words("Sonic the Hedgehog");
        assert!(words.is_empty());
    }

    // --- aggressive tier ---

    #[test]
    fn aggressive_tier_matches_punctuation_variants() {
        // Image: "Bio Hazard Battle.png" in the directory
        // ROM: "Bio-Hazard Battle.smd" — the hyphen differs
        // Aggressive normalization strips punctuation so both → "bio hazard battle"
        let index = index_from(&[("Bio Hazard Battle", "boxart/Bio Hazard Battle.png")]);
        let result = find_best_match(&index, "Bio-Hazard Battle.smd", None, None);
        assert_eq!(result.as_deref(), Some("boxart/Bio Hazard Battle.png"));
    }

    #[test]
    fn aggressive_tier_rejects_false_positives() {
        // Image: "Battletoads (Europe).png" in the directory
        // ROM: "Battletoads & Double Dragon (USA).md" — a different game
        // These should NOT match because the aggressive keys are different strings.
        let index = index_from(&[("Battletoads (Europe)", "boxart/Battletoads (Europe).png")]);
        let result = find_best_match(
            &index,
            "Battletoads & Double Dragon (USA).md",
            None,
            None,
        );
        assert!(
            result.is_none(),
            "Battletoads & Double Dragon should not match Battletoads"
        );
    }
}
