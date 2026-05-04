//! Image resolution: build per-system image indexes and resolve box art paths.
//!
//! Extracted from `enrichment.rs` — contains `ImageIndex`, `BoxArtResult`,
//! `build_image_index`, `resolve_box_art_with_hash`, and URL formatting helpers.

use std::collections::HashMap;
use std::path::Path;

use crate::external_metadata::ThumbnailManifestEntry;
use crate::image_matching::{self, DirIndex};
use crate::thumbnail_manifest::{self, ManifestFuzzyIndex, ManifestMatch};
use crate::thumbnails::{self, ThumbnailKind};

/// Per-system image directory index for batch box art resolution.
///
/// Wraps a core `DirIndex` for filesystem-based matching, plus DB path
/// lookups and manifest-backed fallback for images not yet downloaded.
///
/// Built as a temporary value during enrichment — NOT cached across requests.
pub struct ImageIndex {
    /// Core directory index: exact, case-insensitive, fuzzy, version-stripped.
    pub dir_index: DirIndex,
    /// DB paths: rom_filename -> "boxart/{path}"
    pub db_paths: HashMap<String, String>,
    /// Manifest-backed fallback for images not yet downloaded.
    /// None if the thumbnail_index has no entries for this system.
    pub manifest: Option<ManifestFuzzyIndex>,
}

/// Result of a box art resolution attempt.
pub enum BoxArtResult<'a> {
    /// Found a local image — contains the relative path (e.g., `"boxart/Name.png"`).
    Found(String),
    /// No local image, but the manifest has a match that can be downloaded.
    ManifestHit(&'a ManifestMatch),
    /// No match at all.
    NotFound,
}

/// Build an image index for a system from the filesystem.
///
/// Scans the boxart media directory, resolves fake symlinks, applies the
/// user's per-ROM box-art overrides, and wraps the libretro fuzzy index.
///
/// # Arguments
/// * `system` - System folder name.
/// * `storage_root` - Root of the storage device (e.g., `/media/usb`).
/// * `user_overrides` - User box art overrides from user_data.db
///   (rom_filename → relative path under `media/<system>/`).
/// * `libretro_repo_data` - Pre-loaded libretro manifest data for this system,
///   loaded by the caller from `external_metadata.db` via
///   `thumbnail_manifest::load_repo_manifest_data`. Empty when no repos
///   apply or the host-global pool was unavailable.
pub fn build_image_index(
    system: &str,
    storage_root: &Path,
    user_overrides: HashMap<String, String>,
    libretro_repo_data: Vec<(String, String, Vec<ThumbnailManifestEntry>)>,
) -> ImageIndex {
    let boxart_media = ThumbnailKind::Boxart.media_dir();
    let rc_dir = storage_root.join(crate::storage::RC_DIR);
    let media_base = rc_dir.join("media").join(system);
    let boxart_dir = media_base.join(boxart_media);

    // Build the base index using the shared image matching module.
    let mut dir_index = image_matching::build_dir_index(&boxart_dir, boxart_media);

    // Second pass: resolve fake symlinks (small text files pointing to real images).
    let base_title = thumbnails::base_title;
    if let Ok(entries) = std::fs::read_dir(&boxart_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if let Some(img_stem) = thumbnails::strip_image_ext(&name_str) {
                if dir_index.exact.contains_key(img_stem) {
                    continue; // Already indexed by build_dir_index.
                }
                let full = entry.path();
                if let Some(resolved) =
                    thumbnails::try_resolve_fake_symlink_sync(&full, &boxart_dir)
                {
                    let resolved_path = format!("boxart/{resolved}");
                    dir_index
                        .exact
                        .insert(img_stem.to_string(), resolved_path.clone());
                    dir_index
                        .exact_ci
                        .entry(img_stem.to_lowercase())
                        .and_modify(|existing| {
                            if resolved_path < *existing {
                                *existing = resolved_path.clone();
                            }
                        })
                        .or_insert_with(|| resolved_path.clone());
                    let bt = base_title(img_stem);
                    let vs = thumbnails::strip_version(&bt).to_string();
                    dir_index
                        .fuzzy
                        .entry(bt.clone())
                        .and_modify(|existing| {
                            if resolved_path < *existing {
                                *existing = resolved_path.clone();
                            }
                        })
                        .or_insert_with(|| resolved_path.clone());
                    if vs.len() < bt.len() {
                        dir_index
                            .version
                            .entry(vs)
                            .and_modify(|existing| {
                                if resolved_path < *existing {
                                    *existing = resolved_path.clone();
                                }
                            })
                            .or_insert_with(|| resolved_path.clone());
                    }
                    let agg = replay_control_core::title_utils::normalize_aggressive(&bt);
                    dir_index
                        .aggressive
                        .entry(agg)
                        .and_modify(|existing| {
                            if resolved_path < *existing {
                                *existing = resolved_path.clone();
                            }
                        })
                        .or_insert(resolved_path);
                }
            }
        }
    }

    // db_paths used to come from `game_metadata.box_art_path` (legacy
    // thumbnail-download path). With v2, `game_library.box_art_url` is
    // written directly during enrichment, so the only thing left here is
    // the user's per-ROM override layer.
    let db_paths: HashMap<String, String> = user_overrides;

    // Build the manifest fuzzy index from the pre-loaded libretro repo data.
    // Returns None when no libretro thumbnail data exists.
    let manifest: Option<ManifestFuzzyIndex> = if libretro_repo_data.is_empty() {
        None
    } else {
        let idx = thumbnail_manifest::build_manifest_fuzzy_index_from_raw(&libretro_repo_data);
        if idx.exact.is_empty() {
            None
        } else {
            Some(idx)
        }
    };

    ImageIndex {
        dir_index,
        db_paths,
        manifest,
    }
}

/// Pre-fetched map of `ArcadeGameInfo` keyed by filename stem.
#[derive(Debug, Default, Clone)]
pub struct ArcadeInfoLookup {
    pub map: std::collections::HashMap<String, crate::arcade_db::ArcadeGameInfo>,
}

impl ArcadeInfoLookup {
    /// Build from a list of filename stems by issuing one batch query.
    pub async fn build(system: &str, rom_filenames: &[String]) -> Self {
        if !replay_control_core::systems::is_arcade_system(system) || rom_filenames.is_empty() {
            return Self::default();
        }
        let mut stems: Vec<&str> = rom_filenames
            .iter()
            .map(|f| replay_control_core::title_utils::filename_stem(f.as_str()))
            .collect();
        // Include parent stems via a second pass (follow-up batch below).
        stems.sort();
        stems.dedup();
        let mut map = crate::arcade_db::lookup_arcade_games_batch(system, &stems).await;
        let parent_stems: Vec<String> = map
            .values()
            .filter(|info| !info.parent.is_empty() && !map.contains_key(info.parent.as_str()))
            .map(|info| info.parent.clone())
            .collect();
        if !parent_stems.is_empty() {
            let parent_refs: Vec<&str> = parent_stems.iter().map(String::as_str).collect();
            let parents = crate::arcade_db::lookup_arcade_games_batch(system, &parent_refs).await;
            map.extend(parents);
        }
        Self { map }
    }

    pub fn get(&self, stem: &str) -> Option<&crate::arcade_db::ArcadeGameInfo> {
        self.map.get(stem)
    }
}

/// Resolve box art with an optional `hash_matched_name` fallback.
///
/// When the filename-based lookup fails but a No-Intro `hash_matched_name` is available,
/// retries the lookup using that name. This works well because libretro-thumbnails repos
/// use No-Intro naming, so the hash_matched_name will often match directly.
pub fn resolve_box_art_with_hash<'a>(
    index: &'a ImageIndex,
    arcade_lookup: &ArcadeInfoLookup,
    system: &str,
    rom_filename: &str,
    hash_matched_name: Option<&str>,
) -> BoxArtResult<'a> {
    let stem = replay_control_core::title_utils::filename_stem(rom_filename);
    let stem = replay_control_core::title_utils::strip_n64dd_prefix(stem);
    let is_arcade = replay_control_core::systems::is_arcade_system(system);
    let arcade_info = if is_arcade {
        arcade_lookup.get(stem)
    } else {
        None
    };
    // `info.display_name` is already the per-system merged value (the lookup
    // batch was built with `system`), so each arcade system's thumbnail
    // matching uses its upstream's curated name.
    let arcade_display: Option<&str> = arcade_info.map(|i| i.display_name.as_str());

    // Delegate all filesystem-based matching tiers to core image_matching.
    let db_paths = if index.db_paths.is_empty() {
        None
    } else {
        Some(&index.db_paths)
    };
    if let Some(path) =
        image_matching::find_best_match(&index.dir_index, rom_filename, arcade_display, db_paths)
    {
        return BoxArtResult::Found(path);
    }

    // Check manifest for a remote thumbnail to download.
    if let Some(ref manifest) = index.manifest
        && let Some(m) =
            thumbnail_manifest::find_in_manifest(manifest, rom_filename, arcade_display)
    {
        return BoxArtResult::ManifestHit(m);
    }

    // Arcade clone fallback: if this ROM is a clone, try the parent's display name.
    if is_arcade
        && let Some(info) = arcade_info
        && !info.parent.is_empty()
        && let Some(parent_info) = arcade_lookup.get(&info.parent)
    {
        // Build a synthetic rom_filename from the parent codename so
        // find_best_match uses the parent's display name for matching.
        let parent_filename = format!("{}.zip", info.parent);
        if let Some(path) = image_matching::find_best_match(
            &index.dir_index,
            &parent_filename,
            Some(parent_info.display_name.as_str()),
            db_paths,
        ) {
            return BoxArtResult::Found(path);
        }

        // Also try manifest with parent.
        if let Some(ref manifest) = index.manifest
            && let Some(m) = thumbnail_manifest::find_in_manifest(
                manifest,
                &parent_filename,
                Some(parent_info.display_name.as_str()),
            )
        {
            return BoxArtResult::ManifestHit(m);
        }
    }

    // Hash-matched name fallback: if we have a No-Intro canonical name from CRC matching,
    // try it as an alternative ROM filename. This works for all ROMs (including translations/
    // hacks/specials — showing the original game's box art is correct).
    if let Some(matched_name) = hash_matched_name {
        let synthetic_filename = format!("{matched_name}.rom");
        if let Some(path) =
            image_matching::find_best_match(&index.dir_index, &synthetic_filename, None, db_paths)
        {
            return BoxArtResult::Found(path);
        }
        // Also try manifest with hash_matched_name.
        if let Some(ref manifest) = index.manifest
            && let Some(m) =
                thumbnail_manifest::find_in_manifest(manifest, &synthetic_filename, None)
        {
            return BoxArtResult::ManifestHit(m);
        }
    }

    BoxArtResult::NotFound
}

/// Format a relative box art path as a URL path for the media endpoint.
///
/// URL-encodes each path segment and prepends `/media/{system}/`.
pub fn format_box_art_url(system: &str, relative_path: &str) -> String {
    let encoded_path: String = relative_path
        .split('/')
        .map(encode_uri_path_segment)
        .collect::<Vec<_>>()
        .join("/");
    format!("/media/{system}/{encoded_path}")
}

/// Percent-encode a single URI path segment (RFC 3986 unreserved chars only).
///
/// Matches the behavior of `urlencoding::encode`: preserves only
/// ALPHA / DIGIT / `-` / `.` / `_` / `~`, encodes everything else.
fn encode_uri_path_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push(char::from(b"0123456789ABCDEF"[(b >> 4) as usize]));
                out.push(char::from(b"0123456789ABCDEF"[(b & 0x0F) as usize]));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image_matching;

    /// Build a minimal ImageIndex with given (stem, path) entries and no manifest.
    fn image_index_from(entries: &[(&str, &str)]) -> ImageIndex {
        let mut exact: HashMap<String, String> = HashMap::new();
        let mut exact_ci: HashMap<String, String> = HashMap::new();
        let mut fuzzy: HashMap<String, String> = HashMap::new();
        let mut version: HashMap<String, String> = HashMap::new();
        let mut aggressive: HashMap<String, String> = HashMap::new();
        let mut aggressive_compact: HashMap<String, String> = HashMap::new();

        for &(stem, path) in entries {
            use crate::thumbnails::{base_title, strip_version};
            use replay_control_core::title_utils::{
                normalize_aggressive, normalize_aggressive_compact,
            };

            exact.insert(stem.to_string(), path.to_string());
            exact_ci
                .entry(stem.to_lowercase())
                .and_modify(|existing| {
                    if path < existing.as_str() {
                        *existing = path.to_string();
                    }
                })
                .or_insert_with(|| path.to_string());
            let bt = base_title(stem);
            let vs = strip_version(&bt).to_string();
            fuzzy
                .entry(bt.clone())
                .and_modify(|existing| {
                    if path < existing.as_str() {
                        *existing = path.to_string();
                    }
                })
                .or_insert_with(|| path.to_string());
            if vs.len() < bt.len() {
                version
                    .entry(vs)
                    .and_modify(|existing| {
                        if path < existing.as_str() {
                            *existing = path.to_string();
                        }
                    })
                    .or_insert_with(|| path.to_string());
            }
            let agg = normalize_aggressive(&bt);
            aggressive
                .entry(agg)
                .and_modify(|existing| {
                    if path < existing.as_str() {
                        *existing = path.to_string();
                    }
                })
                .or_insert_with(|| path.to_string());
            let agg_compact = normalize_aggressive_compact(&bt);
            if !agg_compact.is_empty() {
                aggressive_compact
                    .entry(agg_compact)
                    .and_modify(|existing| {
                        if path < existing.as_str() {
                            *existing = path.to_string();
                        }
                    })
                    .or_insert_with(|| path.to_string());
            }
        }

        ImageIndex {
            dir_index: image_matching::DirIndex {
                exact,
                exact_ci,
                fuzzy,
                version,
                aggressive,
                aggressive_compact,
            },
            db_paths: HashMap::new(),
            manifest: None,
        }
    }

    #[tokio::test]
    async fn arcade_clone_falls_back_to_parent_art() {
        crate::catalog_pool::init_test_catalog().await;
        if crate::catalog_pool::using_stub_data() {
            return;
        }
        // "pacman" is a clone of "puckman". If there's no art for
        // pacman's display name but there IS art for puckman's display name,
        // the clone fallback should find the parent's art.
        let batch = ArcadeInfoLookup::build("arcade_mame", &["pacman.zip".to_string()]).await;
        let clone_info = batch
            .get("pacman")
            .cloned()
            .expect("pacman should be in arcade DB");
        assert!(clone_info.is_clone, "pacman should be a clone");
        assert_eq!(clone_info.parent, "puckman");

        let parent_info = batch
            .get("puckman")
            .cloned()
            .expect("puckman should be in arcade DB");

        // Put the parent's display name as a thumbnail stem in the index.
        // thumbnail_filename converts special chars, so use that to build
        // the stem as it would appear on disk.
        let parent_thumb = crate::thumbnails::thumbnail_filename(&parent_info.display_name);
        let path = format!("boxart/{parent_thumb}.png");
        let index = image_index_from(&[(&parent_thumb, &path)]);

        let result = resolve_box_art_with_hash(&index, &batch, "arcade_mame", "pacman.zip", None);
        match result {
            BoxArtResult::Found(found_path) => {
                assert_eq!(found_path, path);
            }
            _ => panic!("Expected clone fallback to find parent art for pacman → puckman"),
        }
    }

    #[tokio::test]
    async fn hash_matched_name_finds_art_when_filename_misses() {
        // ROM "Dong Gu Ri Te Chi Jak Jeon (Korea).md" doesn't match any thumbnail,
        // but hash_matched_name "Dongguri Techi Jakjeon (Korea)" does.
        let thumb_stem = "Dongguri Techi Jakjeon (Korea)";
        let path = format!("boxart/{thumb_stem}.png");
        let index = image_index_from(&[(thumb_stem, &path)]);

        // Without hash: no match.
        let batch = ArcadeInfoLookup::default();
        let result = resolve_box_art_with_hash(
            &index,
            &batch,
            "sega_smd",
            "Dong Gu Ri Te Chi Jak Jeon (Korea).md",
            None,
        );
        assert!(
            matches!(result, BoxArtResult::NotFound),
            "Filename alone should not match"
        );

        // With hash: finds art via fallback.
        let result = resolve_box_art_with_hash(
            &index,
            &batch,
            "sega_smd",
            "Dong Gu Ri Te Chi Jak Jeon (Korea).md",
            Some("Dongguri Techi Jakjeon (Korea)"),
        );
        match result {
            BoxArtResult::Found(found_path) => assert_eq!(found_path, path),
            _ => panic!("Hash fallback should find art for mismatched filename"),
        }
    }

    #[tokio::test]
    async fn hash_matched_name_not_used_when_filename_matches() {
        // When the filename already matches, the hash fallback shouldn't change the result.
        let thumb_stem = "Sonic the Hedgehog (USA)";
        let path = format!("boxart/{thumb_stem}.png");
        let index = image_index_from(&[(thumb_stem, &path)]);
        let batch = ArcadeInfoLookup::default();

        let result = resolve_box_art_with_hash(
            &index,
            &batch,
            "sega_smd",
            "Sonic the Hedgehog (USA).md",
            Some("Sonic the Hedgehog (USA)"),
        );
        match result {
            BoxArtResult::Found(found_path) => assert_eq!(found_path, path),
            _ => panic!("Direct filename match should still work"),
        }
    }

    #[tokio::test]
    async fn hash_matched_name_none_returns_not_found() {
        // No art on disk, no hash — should be NotFound.
        let index = image_index_from(&[("Unrelated Game", "boxart/Unrelated Game.png")]);
        let batch = ArcadeInfoLookup::default();

        let result =
            resolve_box_art_with_hash(&index, &batch, "sega_smd", "Unknown ROM (Korea).md", None);
        assert!(matches!(result, BoxArtResult::NotFound));
    }

    #[tokio::test]
    async fn non_clone_does_not_use_parent_fallback() {
        crate::catalog_pool::init_test_catalog().await;
        if crate::catalog_pool::using_stub_data() {
            return;
        }
        // "puckman" is NOT a clone — it's a parent ROM. If there's no art
        // matching "Puck Man", it should return NotFound (no parent to fall back to).
        let batch = ArcadeInfoLookup::build("arcade_mame", &["puckman.zip".to_string()]).await;
        let info = batch
            .get("puckman")
            .cloned()
            .expect("puckman should be in arcade DB");
        assert!(!info.is_clone, "puckman should not be a clone");

        // Index has art for an unrelated game only.
        let index = image_index_from(&[("Unrelated Game", "boxart/Unrelated Game.png")]);

        let result = resolve_box_art_with_hash(&index, &batch, "arcade_mame", "puckman.zip", None);
        assert!(
            matches!(result, BoxArtResult::NotFound),
            "Non-clone should not find art via parent fallback"
        );
    }
}
