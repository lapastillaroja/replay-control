//! Shared image matching logic for resolving ROM filenames to thumbnail image paths.
//!
//! Used by both the thumbnail import pipeline (import.rs) and the runtime image
//! cache (cache/images.rs) to avoid duplicating the multi-tier fuzzy matching logic.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::thumbnails::{
    IMAGE_EXTENSIONS, base_title, is_real_image_file, is_valid_image_sync, strip_image_ext,
    strip_version, thumbnail_filename, try_resolve_fake_symlink_sync,
};
use replay_control_core::title_utils::{
    filename_stem, normalize_aggressive, normalize_aggressive_compact, strip_n64dd_prefix,
    strip_tags,
};

/// Directory index for matching ROM filenames to image files.
///
/// Built from a single readdir scan, provides O(1) lookups at multiple
/// fuzziness tiers: exact, case-insensitive, base_title, and version-stripped.
///
/// Deliberately kept separate from [`manifest::ManifestFuzzyIndex`](super::manifest::ManifestFuzzyIndex),
/// the upstream-repo matcher: the two index different value types (a path
/// string here vs a `ManifestMatch` there) with tier strategies tuned to their
/// own data. The reusable pieces — the key-extraction primitives (`base_title`,
/// `strip_tags`, `strip_version`, `normalize_aggressive*`) — already live in
/// the shared `thumbnails` module and are used by both, so merging the index
/// structs would trade real per-source tuning for little gain.
#[derive(Default)]
pub struct DirIndex {
    /// Exact thumbnail_filename stem → relative path (e.g., "boxart/Name.png")
    pub exact: HashMap<String, String>,
    /// Lowercase stem → relative path (case-insensitive exact match)
    pub exact_ci: HashMap<String, String>,
    /// base_title (tags stripped) → relative path
    pub fuzzy: HashMap<String, String>,
    /// version-stripped base_title → relative path
    pub version: HashMap<String, String>,
    /// Aggressively normalized (all punctuation stripped, spaces preserved)
    pub aggressive: HashMap<String, String>,
    /// Compact-aggressive normalization (punctuation AND spaces stripped).
    /// Last-resort tier; mirrors `ManifestFuzzyIndex::by_aggressive_compact`.
    /// Catches "Galaga '88.png" ↔ catalog "Galaga88" both collapsing to
    /// "galaga88".
    pub aggressive_compact: HashMap<String, String>,
}

/// Result of one thumbnail directory scan.
///
/// `index` is used for resolver-accurate matching. `valid_files` is the
/// deletion-candidate set from the same directory snapshot, containing only
/// real image files; libretro fake-symlink stubs can resolve entries into the
/// index, but are not themselves image bytes to delete.
#[derive(Default)]
pub struct DirScan {
    pub index: DirIndex,
    pub valid_files: Vec<(String, PathBuf)>,
}

impl DirIndex {
    /// Insert one image — identified by its filename `stem` and relative
    /// `path` — into every matching tier.
    ///
    /// Both the directory scan (`build_dir_index`) and the fake-symlink
    /// resolution pass (`build_image_index`) funnel through here so the set of
    /// populated tiers can never drift between the two. For the fuzzy tiers,
    /// ties resolve to the alphabetically-first path so results are independent
    /// of directory iteration order.
    pub fn insert(&mut self, stem: &str, path: String) {
        // Keep the alphabetically-first path for a fuzzy key (deterministic).
        fn keep_first(map: &mut HashMap<String, String>, key: String, path: &str) {
            map.entry(key)
                .and_modify(|existing| {
                    if path < existing.as_str() {
                        *existing = path.to_string();
                    }
                })
                .or_insert_with(|| path.to_string());
        }

        // Exact tier: keep the most-preferred candidate deterministically (see
        // exact_pref) — a real self-named {stem}.<ext> file over a same-stem
        // fake-symlink redirect, then .png over .jpg. Plain last-writer-wins here
        // would make the exact match depend on readdir order, so two independent
        // scans (runtime serve vs orphan cleanup) could resolve the same stem to
        // different files — and the fast path, which serves the real {stem}.png,
        // would disagree with a scan that landed on the .jpg or a stub's target.
        keep_preferred_exact(&mut self.exact, stem.to_string(), &path);
        keep_first(&mut self.exact_ci, stem.to_lowercase(), &path);

        let bt = base_title(stem);
        keep_first(&mut self.fuzzy, bt.clone(), &path);

        let vs = strip_version(&bt).to_string();
        if vs.len() < bt.len() {
            keep_first(&mut self.version, vs, &path);
        }

        // Aggressive: strip all punctuation, keep spaces.
        keep_first(&mut self.aggressive, normalize_aggressive(&bt), &path);

        // Aggressive-compact: also strips spaces.
        let agg_compact = normalize_aggressive_compact(&bt);
        if !agg_compact.is_empty() {
            keep_first(&mut self.aggressive_compact, agg_compact, &path);
        }
    }
}

/// Basename (after the last `/`) of a relative image path.
fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Priority of `path`'s extension per [`IMAGE_EXTENSIONS`] (lower = preferred);
/// unrecognized extensions sort last.
fn ext_rank(path: &str) -> usize {
    IMAGE_EXTENSIONS
        .iter()
        .position(|ext| path.ends_with(ext))
        .unwrap_or(IMAGE_EXTENSIONS.len())
}

/// Exact-tier preference for a candidate that resolves stem `key` to `path`
/// (lower wins). A real self-named file — basename stem equals `key`, i.e.
/// `{key}.<ext>` — ranks ahead of a fake-symlink redirect, whose resolved target
/// is named something else; then by extension priority (`.png` over `.jpg`).
///
/// The self-named rule is what keeps the real `{key}.png` the exact match even
/// when a same-stem `.jpg` stub points at an alphabetically-earlier `.png`
/// target: without it the tie-break on the resolved path could pick the stub's
/// target, and the on-disk fast path (which serves the real `{key}.png`) would
/// disagree with the full scan — cleanup could then delete the served file.
fn exact_pref(key: &str, path: &str) -> (bool, usize) {
    let self_named = strip_image_ext(basename(path)) == Some(key);
    (!self_named, ext_rank(path))
}

/// Insert into an exact-match map, keeping the most-preferred candidate per
/// [`exact_pref`] (ties broken alphabetically by path), so a stem present under
/// multiple files/stubs always resolves the same way regardless of scan order.
fn keep_preferred_exact(map: &mut HashMap<String, String>, key: String, path: &str) {
    let replace = match map.get(&key) {
        Some(existing) => {
            (exact_pref(&key, path), path) < (exact_pref(&key, existing), existing.as_str())
        }
        None => true,
    };
    if replace {
        map.insert(key, path.to_string());
    }
}

/// Build a `DirIndex` by scanning a media subdirectory (boxart or snap).
///
/// `dir` is the full path to the image directory.
/// `kind` is the subdirectory label used in relative paths (e.g., "boxart", "snap").
///
/// Only indexes `.png` and `.jpg` files that are at least 200 bytes (to skip fake symlinks/stubs).
pub fn build_dir_index(dir: &Path, kind: &str) -> DirIndex {
    let mut index = DirIndex::default();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if let Some(img_stem) = strip_image_ext(&name_str) {
                // Skip tiny files (fake symlinks / stubs).
                if !is_valid_image_sync(&entry.path()) {
                    continue;
                }
                index.insert(img_stem, format!("{kind}/{name_str}"));
            }
        }
    }
    index
}

/// Scan `dir` once, always building the resolver index; when `collect_files`,
/// also gather the real image files from the same snapshot as the deletion-
/// candidate set. Libretro fake-symlink stubs resolve into the index so matching
/// can find their targets, but only real image files are candidates — never a
/// directory whose metadata length happens to reach the validity threshold.
fn scan_dir(dir: &Path, kind: &str, collect_files: bool) -> DirScan {
    let mut scan = DirScan::default();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let Some(stem) = strip_image_ext(&name) else {
                continue;
            };
            let path = entry.path();
            // Real image files only (is_real_image_file: non-symlink regular file
            // >= 200 bytes) — never a directory, an OS symlink, or a fake-symlink
            // stub (a tiny text file, handled by the else branch). The on-disk
            // fast path uses the SAME predicate, so serve and cleanup classify
            // every entry identically.
            if is_real_image_file(&path) {
                let relative = format!("{kind}/{name}");
                scan.index.insert(stem, relative.clone());
                if collect_files {
                    scan.valid_files.push((relative, path));
                }
            } else if let Some(target) = try_resolve_fake_symlink_sync(&path, dir) {
                scan.index.insert(stem, format!("{kind}/{target}"));
            }
        }
    }
    if collect_files {
        scan.valid_files.sort_by(|a, b| a.0.cmp(&b.0));
    }
    scan
}

/// Scan a thumbnail directory, building the resolver index and collecting real
/// image files from the same snapshot (the orphan-cleanup deletion candidates).
///
/// Libretro fake-symlink stubs are resolved into the index so matching can find
/// their targets, but only real image files are included in `valid_files`.
pub fn scan_dir_with_symlinks(dir: &Path, kind: &str) -> DirScan {
    scan_dir(dir, kind, true)
}

/// Build a `DirIndex` and additionally resolve libretro fake-symlink stubs
/// (tiny text files pointing at a real image) to their targets, inserting each
/// through `DirIndex::insert` so resolved stubs populate exactly the same tiers
/// as the directory scan. This is the single index builder used by the runtime
/// resolver and enrichment; it skips collecting the deletion-candidate set that
/// only orphan cleanup needs (via [`scan_dir_with_symlinks`]).
pub fn build_dir_index_with_symlinks(dir: &Path, kind: &str) -> DirIndex {
    scan_dir(dir, kind, false).index
}

/// Resolve one ROM to its image in `index` — the single resolution entry point
/// shared by runtime serving and orphan cleanup, so a file cleanup would delete
/// is exactly a file serving could never show. For arcade ROMs the translated
/// display name is tried first (arcade art is filed under the display name),
/// then the ROM filename stem as a fallback (art filed under the MAME short
/// name), mirroring the two lookups the runtime has always made.
pub fn resolve_thumbnail(
    index: &DirIndex,
    rom_filename: &str,
    arcade_display: Option<&str>,
) -> Option<String> {
    if arcade_display.is_some()
        && let Some(path) = find_best_match(index, rom_filename, arcade_display, None)
    {
        return Some(path);
    }
    find_best_match(index, rom_filename, None, None)
}

/// The ROM filename stem used for thumbnail matching: extension stripped, then
/// the N64DD disk-side prefix removed. The single definition of "which part of a
/// ROM filename the thumbnail tiers match on", shared by [`find_best_match`] and
/// the on-disk exact fast path so they can never disagree on the stem.
pub fn match_stem(rom_filename: &str) -> &str {
    strip_n64dd_prefix(filename_stem(rom_filename))
}

/// The single exact `thumbnail_filename` key the on-disk fast path may safely
/// probe: the globally highest-priority tier the resolver would consult.
///
/// For arcade (a `arcade_display` is present) that is the display-exact key —
/// [`resolve_thumbnail`] runs *every* display-name tier before it ever falls
/// back to the ROM stem, so if the display-exact file exists on disk the
/// resolver is guaranteed to return it. For non-arcade it is the stem-exact key,
/// the resolver's top tier. This is byte-for-byte the key [`find_best_match`]
/// computes for its tier-2 exact match on the same source, so a fast-path hit
/// can never outrank a real resolver pick.
///
/// Any *lower* tier (colon/case-insensitive/fuzzy variants, or the arcade stem
/// fallback) is deliberately NOT probed here: a competing higher-tier file would
/// make the full resolver choose differently, so those cases must miss and fall
/// through to `build_dir_index_with_symlinks` + [`resolve_thumbnail`].
pub fn exact_thumbnail_key(rom_filename: &str, arcade_display: Option<&str>) -> String {
    thumbnail_filename(arcade_display.unwrap_or_else(|| match_stem(rom_filename)))
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
/// 8. Slash/underscore dual-title split — for ` / ` or ` _ ` (raw) names, try each half
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
        let without_prefix = db_path.strip_prefix("boxart/").unwrap_or(db_path);
        let stem = strip_image_ext(without_prefix).unwrap_or(without_prefix);
        if index.exact.contains_key(stem) {
            return Some(db_path.clone());
        }
    }

    let stem = match_stem(rom_filename);
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

    // Tier 7: slash/underscore dual-title split — for "A / B" or "A _ B" names,
    // try each half through exact + fuzzy: arcade "/" names ("Demon Front / Moyu
    // Zhanxian ..."), lock-on combos ("Sonic & Knuckles _ Sonic 3"). Split the
    // RAW source, not the '&'-mapped thumb, so "A & B" compounds ("Battletoads &
    // Double Dragon") are NOT split into halves and mis-matched to a single-title
    // cover. Halves under 5 chars are too generic to match on.
    let dual_sep = if source.contains(" / ") {
        Some(" / ")
    } else if source.contains(" _ ") {
        Some(" _ ")
    } else {
        None
    };
    if let Some(sep) = dual_sep {
        for half in source.split(sep) {
            let half = half.trim();
            if half.len() < 5 {
                continue;
            }
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

    // Tier 8: aggressive normalization (strip all punctuation, keep spaces)
    let agg = normalize_aggressive(&base);
    if !agg.is_empty()
        && let Some(path) = index.aggressive.get(&agg)
    {
        return Some(path.clone());
    }

    // Tier 9: compact-aggressive (also strips spaces). Mirror of
    // find_in_manifest tier 9. Same guard: only fire when the source's
    // aggressive form has no internal whitespace, to avoid over-matching
    // transliterated names that have spaces on one side.
    if !agg.contains(' ') {
        let agg_compact = normalize_aggressive_compact(&base);
        if !agg_compact.is_empty()
            && let Some(path) = index.aggressive_compact.get(&agg_compact)
        {
            return Some(path.clone());
        }
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
        let mut index = DirIndex::default();
        for &(stem, path) in entries {
            index.insert(stem, path.to_string());
        }
        index
    }

    #[test]
    fn arcade_display_fuzzy_match_beats_stem_exact_match() {
        // Resolution behavior: for an arcade ROM, resolve_thumbnail exhausts the
        // display-name tiers before the ROM stem. So art matched by a display
        // colon-variant outranks a stem-exact file — the property the on-disk fast
        // path must not violate (covered end-to-end in the serve==cleanup tests).
        let index = index_from(&[
            (
                "Street Fighter II - Champion Edition",
                "boxart/Street Fighter II - Champion Edition.png",
            ),
            ("sf2ce", "boxart/sf2ce.png"),
        ]);
        assert_eq!(
            resolve_thumbnail(
                &index,
                "sf2ce.zip",
                Some("Street Fighter II: Champion Edition")
            )
            .as_deref(),
            Some("boxart/Street Fighter II - Champion Edition.png"),
        );
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
        let result = find_best_match(&index, "Battletoads & Double Dragon (USA).md", None, None);
        assert!(
            result.is_none(),
            "Battletoads & Double Dragon should not match Battletoads"
        );
    }

    // --- deterministic tier tests ---

    #[test]
    fn build_dir_index_deterministic_fuzzy_key() {
        // Two images that normalize to the same fuzzy key "game".
        // "Game (Europe).png" sorts before "Game (USA).png" alphabetically,
        // so the Europe path should win regardless of insertion order.
        let index_ab = index_from(&[
            ("Game (USA)", "boxart/Game (USA).png"),
            ("Game (Europe)", "boxart/Game (Europe).png"),
        ]);
        let index_ba = index_from(&[
            ("Game (Europe)", "boxart/Game (Europe).png"),
            ("Game (USA)", "boxart/Game (USA).png"),
        ]);
        // Both should resolve to the alphabetically-first path.
        assert_eq!(
            index_ab.fuzzy.get("game"),
            Some(&"boxart/Game (Europe).png".to_string()),
            "fuzzy tier should pick alphabetically-first path"
        );
        assert_eq!(
            index_ab.fuzzy.get("game"),
            index_ba.fuzzy.get("game"),
            "fuzzy tier must be deterministic regardless of insertion order"
        );
        // Also check exact_ci tier.
        assert_eq!(
            index_ab.exact_ci.get("game (europe)"),
            index_ba.exact_ci.get("game (europe)"),
        );
        // Aggressive tier should also be deterministic.
        assert_eq!(
            index_ab.aggressive.get("game"),
            Some(&"boxart/Game (Europe).png".to_string()),
        );
        assert_eq!(
            index_ab.aggressive.get("game"),
            index_ba.aggressive.get("game"),
            "aggressive tier must be deterministic"
        );
    }

    #[test]
    fn resolution_prefers_png_over_jpg_regardless_of_scan_order() {
        // A stem present as both Name.png and Name.jpg resolves to the .png,
        // deterministically, whichever order the directory scan inserted them.
        let png_first = index_from(&[("Zelda", "boxart/Zelda.png"), ("Zelda", "boxart/Zelda.jpg")]);
        let jpg_first = index_from(&[("Zelda", "boxart/Zelda.jpg"), ("Zelda", "boxart/Zelda.png")]);
        assert_eq!(
            find_best_match(&png_first, "Zelda.sfc", None, None).as_deref(),
            Some("boxart/Zelda.png"),
        );
        assert_eq!(
            find_best_match(&jpg_first, "Zelda.sfc", None, None).as_deref(),
            Some("boxart/Zelda.png"),
            "resolution must be independent of scan order",
        );
    }

    #[test]
    fn resolution_prefers_real_self_named_file_over_a_same_stem_redirect() {
        // When a stem resolves both to its own real "{stem}.png" and to a
        // fake-symlink redirect pointing at differently-named art, the real
        // self-named file wins — regardless of insertion order.
        let real_first = index_from(&[
            ("Sonic", "boxart/Sonic.png"),
            ("Sonic", "boxart/Aardvark.png"),
        ]);
        let redirect_first = index_from(&[
            ("Sonic", "boxart/Aardvark.png"),
            ("Sonic", "boxart/Sonic.png"),
        ]);
        assert_eq!(
            find_best_match(&real_first, "Sonic.sfc", None, None).as_deref(),
            Some("boxart/Sonic.png"),
        );
        assert_eq!(
            find_best_match(&redirect_first, "Sonic.sfc", None, None).as_deref(),
            Some("boxart/Sonic.png"),
            "the real self-named file wins independent of insertion order",
        );
    }

    #[test]
    fn aggressive_tier_matches_digit_rom_against_roman_thumbnail() {
        // Issue #66: libretro-thumbnails/DOS ships "Doom II.png"; the user's
        // ROM is "Doom 2.zip". The aggressive tier folds "II" -> "2" so both
        // sides normalize to "doom 2".
        let index = index_from(&[
            ("Doom", "boxart/Doom.png"),
            ("Doom II", "boxart/Doom II.png"),
        ]);
        assert_eq!(
            find_best_match(&index, "Doom 2.zip", None, None).as_deref(),
            Some("boxart/Doom II.png"),
        );
        // The plain "Doom" still matches its own art, not the sequel.
        assert_eq!(
            find_best_match(&index, "Doom.zip", None, None).as_deref(),
            Some("boxart/Doom.png"),
        );
    }

    #[test]
    fn aggressive_tier_does_not_fold_single_letter_numerals() {
        // "Mega Man X" must NOT match a "Mega Man 10" thumbnail (X is a name).
        let index = index_from(&[("Mega Man 10", "boxart/Mega Man 10.png")]);
        assert_eq!(find_best_match(&index, "Mega Man X.zip", None, None), None);
    }
}
