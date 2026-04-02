//! Pure enrichment pipeline: scan → match → resolve → produce updates.
//!
//! This module contains the data-oriented enrichment logic that takes a DB
//! connection, filesystem paths, and pre-loaded data, then produces enrichment
//! updates. No web server state, connection pools, or caches — the app layer
//! handles orchestration.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::Connection;

use crate::developer::normalize_developer;
use crate::image_matching::{self, DirIndex};
use crate::metadata_db::{BoxArtGenreRating, MetadataDb};
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

/// Batched metadata from LaunchBox import, keyed by ROM filename.
struct LaunchBoxMetadata {
    ratings: HashMap<String, f64>,
    genres: HashMap<String, String>,
    players: HashMap<String, u8>,
    rating_counts: HashMap<String, u32>,
    developers: HashMap<String, String>,
    release_years: HashMap<String, u16>,
}

/// All enrichment updates produced for a single system.
///
/// The app layer writes these to the DB and handles cache invalidation.
pub struct EnrichmentResult {
    /// Box art, genre, players, rating, rating_count updates.
    pub enrichments: Vec<BoxArtGenreRating>,
    /// Developer updates: (rom_filename, normalized_developer).
    pub developer_updates: Vec<(String, String)>,
    /// Release year updates: (rom_filename, year).
    pub year_updates: Vec<(String, u16)>,
    /// On-demand manifest matches that need background downloads.
    /// Each entry is (rom_filename, ManifestMatch).
    pub manifest_downloads: Vec<(String, ManifestMatch)>,
}

/// Build an image index for a system from the filesystem and DB.
///
/// Scans the boxart media directory, resolves fake symlinks, loads DB paths
/// (with user overrides merged in), and builds the manifest fuzzy index.
///
/// # Arguments
/// * `conn` - Metadata DB connection (for box_art_paths, manifest data)
/// * `system` - System folder name
/// * `storage_root` - Root of the storage device (e.g., `/media/usb`)
/// * `user_overrides` - User box art overrides from user_data.db (rom_filename -> path)
pub fn build_image_index(
    conn: &Connection,
    system: &str,
    storage_root: &Path,
    user_overrides: HashMap<String, String>,
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
            if let Some(img_stem) = name_str.strip_suffix(".png") {
                if dir_index.exact.contains_key(img_stem) {
                    continue; // Already indexed by build_dir_index.
                }
                let full = entry.path();
                if let Some(resolved) = thumbnails::try_resolve_fake_symlink(&full, &boxart_dir) {
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
                    let agg = crate::title_utils::normalize_aggressive(&bt);
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

    // Load DB paths from metadata DB.
    let mut db_paths = MetadataDb::system_box_art_paths(conn, system).unwrap_or_default();

    // Inject user box art overrides (highest priority — overwrites auto-matched paths).
    for (rom_filename, override_path) in user_overrides {
        db_paths.insert(rom_filename, override_path);
    }

    // Load raw manifest data and build the manifest fuzzy index.
    let manifest = if let Some(repo_names) = thumbnails::thumbnail_repo_names(system) {
        let mut repo_data = Vec::new();
        for display_name in repo_names {
            let url_name = thumbnails::repo_url_name(display_name);
            let source_name = thumbnails::libretro_source_name(display_name);
            let branch = MetadataDb::get_data_source(conn, &source_name)
                .ok()
                .flatten()
                .and_then(|s| s.branch)
                .unwrap_or_else(|| "master".to_string());
            let entries = MetadataDb::query_thumbnail_index(
                conn,
                &source_name,
                ThumbnailKind::Boxart.repo_dir(),
            )
            .unwrap_or_default();
            repo_data.push((url_name, branch, entries));
        }
        let idx = thumbnail_manifest::build_manifest_fuzzy_index_from_raw(&repo_data);
        if idx.exact.is_empty() {
            None
        } else {
            Some(idx)
        }
    } else {
        None
    };

    ImageIndex {
        dir_index,
        db_paths,
        manifest,
    }
}

/// Resolve a box art path for a single ROM using the image index.
///
/// Returns the relative media path (e.g., `"boxart/Name.png"`) or None.
/// Does NOT produce a URL — the app layer adds the `/media/{system}/` prefix
/// and URL-encodes path segments.
///
/// If no local image is found but the manifest has a match, the match is
/// returned via `manifest_hit` so the caller can queue a background download.
pub fn resolve_box_art<'a>(
    index: &'a ImageIndex,
    system: &str,
    rom_filename: &str,
) -> BoxArtResult<'a> {
    let stem = rom_filename
        .rfind('.')
        .map(|i| &rom_filename[..i])
        .unwrap_or(rom_filename);
    let stem = crate::title_utils::strip_n64dd_prefix(stem);
    let is_arcade = crate::systems::is_arcade_system(system);
    let arcade_display = if is_arcade {
        crate::arcade_db::lookup_arcade_game(stem).map(|info| info.display_name)
    } else {
        None
    };

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
        && let Some(m) = thumbnail_manifest::find_in_manifest(manifest, rom_filename, system)
    {
        return BoxArtResult::ManifestHit(m);
    }

    // Arcade clone fallback: if this ROM is a clone, try the parent's display name.
    if is_arcade {
        if let Some(info) = crate::arcade_db::lookup_arcade_game(stem)
            && !info.parent.is_empty()
            && let Some(parent_info) = crate::arcade_db::lookup_arcade_game(info.parent)
        {
            // Build a synthetic rom_filename from the parent codename so
            // find_best_match uses the parent's display name for matching.
            let parent_filename = format!("{}.zip", info.parent);
            if let Some(path) = image_matching::find_best_match(
                &index.dir_index,
                &parent_filename,
                Some(parent_info.display_name),
                db_paths,
            ) {
                return BoxArtResult::Found(path);
            }

            // Also try manifest with parent.
            if let Some(ref manifest) = index.manifest
                && let Some(m) =
                    thumbnail_manifest::find_in_manifest(manifest, &parent_filename, system)
            {
                return BoxArtResult::ManifestHit(m);
            }
        }
    }

    BoxArtResult::NotFound
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

/// Run the full enrichment pipeline for a system.
///
/// Pure data function: reads from DB + filesystem, returns all updates.
/// The app layer is responsible for writing updates and cache invalidation.
///
/// # Arguments
/// * `conn` - Metadata DB connection (same DB has both game_library and game_metadata)
/// * `system` - System folder name
/// * `index` - Pre-built image index for this system
/// * `auto_matched_ratings` - Ratings from auto-matching (pre-computed by app)
pub fn enrich_system(
    conn: &Connection,
    system: &str,
    index: &ImageIndex,
    auto_matched_ratings: &HashMap<String, f64>,
) -> EnrichmentResult {
    // Load LaunchBox metadata from game_metadata table.
    let lb = LaunchBoxMetadata {
        ratings: MetadataDb::system_ratings(conn, system)
            .ok()
            .unwrap_or_default(),
        genres: MetadataDb::system_metadata_genres(conn, system)
            .ok()
            .unwrap_or_default(),
        players: MetadataDb::system_metadata_players(conn, system)
            .ok()
            .unwrap_or_default(),
        rating_counts: MetadataDb::system_metadata_rating_counts(conn, system)
            .ok()
            .unwrap_or_default(),
        developers: MetadataDb::system_metadata_developers(conn, system)
            .ok()
            .unwrap_or_default(),
        release_years: MetadataDb::system_metadata_release_years(conn, system)
            .ok()
            .unwrap_or_default(),
    };

    // Load existing game_library values to know which are already set.
    let existing_genres: HashSet<String> = MetadataDb::system_rom_genres(conn, system)
        .map(|map| map.into_keys().collect())
        .unwrap_or_default();
    let existing_players: HashSet<String> =
        MetadataDb::system_rom_players(conn, system).unwrap_or_default();
    let existing_developers: HashSet<String> =
        MetadataDb::system_rom_developers(conn, system).unwrap_or_default();
    let existing_years: HashSet<String> =
        MetadataDb::system_rom_release_years(conn, system).unwrap_or_default();

    // Merge auto-matched ratings into the main ratings map.
    let mut all_ratings = lb.ratings;
    for (filename, rating) in auto_matched_ratings {
        all_ratings.entry(filename.clone()).or_insert(*rating);
    }

    // Read visible filenames from game_library.
    let rom_filenames: Vec<String> =
        MetadataDb::visible_filenames(conn, system).unwrap_or_default();

    if rom_filenames.is_empty() {
        return EnrichmentResult {
            enrichments: Vec::new(),
            developer_updates: Vec::new(),
            year_updates: Vec::new(),
            manifest_downloads: Vec::new(),
        };
    }

    // Build enrichment entries + collect manifest download requests.
    let mut manifest_downloads: Vec<(String, ManifestMatch)> = Vec::new();

    let enrichments: Vec<BoxArtGenreRating> = rom_filenames
        .iter()
        .filter_map(|filename| {
            let art = match resolve_box_art(index, system, filename) {
                BoxArtResult::Found(path) => Some(format_box_art_url(system, &path)),
                BoxArtResult::ManifestHit(m) => {
                    manifest_downloads.push((filename.clone(), m.clone()));
                    None
                }
                BoxArtResult::NotFound => None,
            };
            let rating = all_ratings.get(filename).map(|&r| r as f32);
            let rating_count = lb.rating_counts.get(filename).copied();
            let genre = if !existing_genres.contains(filename) {
                lb.genres.get(filename).cloned()
            } else {
                None
            };
            let players = if !existing_players.contains(filename) {
                lb.players.get(filename).copied()
            } else {
                None
            };
            if art.is_none()
                && rating.is_none()
                && rating_count.is_none()
                && genre.is_none()
                && players.is_none()
            {
                return None;
            }
            Some(BoxArtGenreRating {
                rom_filename: filename.clone(),
                box_art_url: art,
                genre,
                players,
                rating,
                rating_count,
            })
        })
        .collect();

    // ── Second pass: base_title fallback ──────────────────────────────
    // If a ROM has no art but a sibling (same system + base_title) does, use it.
    // This covers region variants (USA has art, Europe doesn't), revisions, etc.
    let enrichments = apply_base_title_fallback(conn, system, enrichments, &rom_filenames);

    // Developer enrichment: fill from LaunchBox for ROMs that don't already have one.
    let developer_updates: Vec<(String, String)> = rom_filenames
        .iter()
        .filter(|f| !existing_developers.contains(*f))
        .filter_map(|f| {
            lb.developers
                .get(f)
                .map(|dev| (f.clone(), normalize_developer(dev)))
        })
        .filter(|(_, dev)| !dev.is_empty())
        .collect();

    // Release year enrichment: fill from LaunchBox for ROMs that don't already have one.
    let year_updates: Vec<(String, u16)> = rom_filenames
        .iter()
        .filter(|f| !existing_years.contains(*f))
        .filter_map(|f| lb.release_years.get(f).map(|&year| (f.clone(), year)))
        .collect();

    EnrichmentResult {
        enrichments,
        developer_updates,
        year_updates,
        manifest_downloads,
    }
}

/// Apply base_title fallback: share box art between ROMs with the same base_title.
///
/// After per-ROM resolution, some ROMs have art and some don't. For those without
/// art, if another ROM in the same system shares the same `base_title` and HAS art,
/// use that art. This handles region variants, revisions, etc.
///
/// Returns a new enrichments vec with fallback art injected.
fn apply_base_title_fallback(
    conn: &Connection,
    system: &str,
    mut enrichments: Vec<BoxArtGenreRating>,
    rom_filenames: &[String],
) -> Vec<BoxArtGenreRating> {
    // Load base_title for every ROM in this system.
    let base_titles: HashMap<String, String> = MetadataDb::visible_base_titles(conn, system)
        .unwrap_or_default()
        .into_iter()
        .collect();

    // Build map: base_title → box_art_url from enrichments that resolved art.
    let mut art_by_base_title: HashMap<&str, &str> = HashMap::new();
    for e in &enrichments {
        if let Some(ref url) = e.box_art_url {
            if let Some(bt) = base_titles.get(&e.rom_filename) {
                if !bt.is_empty() {
                    art_by_base_title.entry(bt.as_str()).or_insert(url.as_str());
                }
            }
        }
    }

    if art_by_base_title.is_empty() {
        return enrichments;
    }

    // Collect owned art URLs by base_title (avoids borrow issues with mutable pass below).
    let art_by_base_title: HashMap<String, String> = art_by_base_title
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

    // Track which ROMs already have an enrichment entry.
    let enriched: HashSet<String> = enrichments.iter().map(|e| e.rom_filename.clone()).collect();

    // Pass 1: fill in existing enrichment entries that have no art.
    for e in &mut enrichments {
        if e.box_art_url.is_none() {
            if let Some(bt) = base_titles.get(&e.rom_filename) {
                if !bt.is_empty() {
                    if let Some(url) = art_by_base_title.get(bt.as_str()) {
                        e.box_art_url = Some(url.clone());
                    }
                }
            }
        }
    }

    // Pass 2: ROMs with no enrichment entry at all (no art, no rating, no genre, etc.)
    // that can get art via base_title fallback.
    for filename in rom_filenames {
        if enriched.contains(filename) {
            continue;
        }
        if let Some(bt) = base_titles.get(filename) {
            if !bt.is_empty() {
                if let Some(url) = art_by_base_title.get(bt.as_str()) {
                    enrichments.push(BoxArtGenreRating {
                        rom_filename: filename.clone(),
                        box_art_url: Some(url.clone()),
                        genre: None,
                        players: None,
                        rating: None,
                        rating_count: None,
                    });
                }
            }
        }
    }

    enrichments
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
        let mut exact = HashMap::new();
        let mut exact_ci = HashMap::new();
        let mut fuzzy = HashMap::new();
        let mut version = HashMap::new();
        let mut aggressive = HashMap::new();

        for &(stem, path) in entries {
            use crate::thumbnails::{base_title, strip_version};
            use crate::title_utils::normalize_aggressive;

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
        }

        ImageIndex {
            dir_index: image_matching::DirIndex {
                exact,
                exact_ci,
                fuzzy,
                version,
                aggressive,
            },
            db_paths: HashMap::new(),
            manifest: None,
        }
    }

    #[test]
    fn arcade_clone_falls_back_to_parent_art() {
        // "pacman" is a clone of "puckman". If there's no art for
        // pacman's display name but there IS art for puckman's display name,
        // the clone fallback should find the parent's art.
        let clone_info =
            crate::arcade_db::lookup_arcade_game("pacman").expect("pacman should be in arcade DB");
        assert!(clone_info.is_clone, "pacman should be a clone");
        assert_eq!(clone_info.parent, "puckman");

        let parent_info = crate::arcade_db::lookup_arcade_game("puckman")
            .expect("puckman should be in arcade DB");

        // Put the parent's display name as a thumbnail stem in the index.
        // thumbnail_filename converts special chars, so use that to build
        // the stem as it would appear on disk.
        let parent_thumb = crate::thumbnails::thumbnail_filename(parent_info.display_name);
        let path = format!("boxart/{parent_thumb}.png");
        let index = image_index_from(&[(&parent_thumb, &path)]);

        let result = resolve_box_art(&index, "arcade_mame", "pacman.zip");
        match result {
            BoxArtResult::Found(found_path) => {
                assert_eq!(found_path, path);
            }
            _ => panic!("Expected clone fallback to find parent art for pacman → puckman"),
        }
    }

    // ── base_title fallback tests ────────────────────────────────────

    /// Open a temp metadata DB for enrichment tests.
    fn open_temp_db() -> (rusqlite::Connection, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let (conn, _path) = MetadataDb::open(dir.path()).unwrap();
        (conn, dir)
    }

    /// Helper: create a game entry with a specific base_title.
    fn make_entry_with_base_title(
        system: &str,
        filename: &str,
        base_title: &str,
    ) -> crate::metadata_db::GameEntry {
        crate::metadata_db::GameEntry {
            system: system.into(),
            rom_filename: filename.into(),
            rom_path: format!("/roms/{system}/{filename}"),
            display_name: None,
            size_bytes: 1000,
            is_m3u: false,
            box_art_url: None,
            driver_status: None,
            genre: None,
            genre_group: String::new(),
            players: None,
            rating: None,
            rating_count: None,
            is_clone: false,
            base_title: base_title.into(),
            region: String::new(),
            is_translation: false,
            is_hack: false,
            is_special: false,
            crc32: None,
            hash_mtime: None,
            hash_matched_name: None,
            series_key: String::new(),
            developer: String::new(),
            release_year: None,
        }
    }

    #[test]
    fn base_title_fallback_shares_art_between_variants() {
        let (mut conn, _dir) = open_temp_db();

        // Two ROMs with the same base_title "sonic".
        MetadataDb::save_system_entries(
            &mut conn,
            "sega_smd",
            &[
                make_entry_with_base_title("sega_smd", "Sonic (USA).md", "sonic"),
                make_entry_with_base_title("sega_smd", "Sonic (Europe).md", "sonic"),
            ],
            None,
        )
        .unwrap();

        // Only USA has art resolved. Europe has no art but has a rating.
        let enrichments = vec![
            BoxArtGenreRating {
                rom_filename: "Sonic (USA).md".into(),
                box_art_url: Some("/media/sega_smd/boxart/Sonic.png".into()),
                genre: None,
                players: None,
                rating: Some(4.5),
                rating_count: None,
            },
            BoxArtGenreRating {
                rom_filename: "Sonic (Europe).md".into(),
                box_art_url: None,
                genre: None,
                players: None,
                rating: Some(4.5),
                rating_count: None,
            },
        ];

        let rom_filenames = vec![
            "Sonic (USA).md".to_string(),
            "Sonic (Europe).md".to_string(),
        ];

        let result = apply_base_title_fallback(&conn, "sega_smd", enrichments, &rom_filenames);

        let europe = result
            .iter()
            .find(|e| e.rom_filename == "Sonic (Europe).md")
            .expect("Europe entry should exist");
        assert_eq!(
            europe.box_art_url.as_deref(),
            Some("/media/sega_smd/boxart/Sonic.png"),
            "Europe should get USA's art via base_title fallback"
        );
    }

    #[test]
    fn base_title_fallback_does_not_cross_systems() {
        let (mut conn, _dir) = open_temp_db();

        // "Sonic" on sega_smd with art.
        MetadataDb::save_system_entries(
            &mut conn,
            "sega_smd",
            &[make_entry_with_base_title(
                "sega_smd",
                "Sonic (USA).md",
                "sonic",
            )],
            None,
        )
        .unwrap();

        // "Sonic" on sega_gg with no art.
        MetadataDb::save_system_entries(
            &mut conn,
            "sega_gg",
            &[make_entry_with_base_title(
                "sega_gg",
                "Sonic (USA).gg",
                "sonic",
            )],
            None,
        )
        .unwrap();

        // sega_smd has art; sega_gg has no enrichments (no art at all in that system).
        let enrichments: Vec<BoxArtGenreRating> = vec![];
        let rom_filenames = vec!["Sonic (USA).gg".to_string()];

        let result = apply_base_title_fallback(&conn, "sega_gg", enrichments, &rom_filenames);

        // GG should NOT get MD's art — fallback is per-system only.
        let gg = result.iter().find(|e| e.rom_filename == "Sonic (USA).gg");
        assert!(
            gg.is_none(),
            "GG should not get art from a different system"
        );
    }

    #[test]
    fn base_title_fallback_skips_empty_base_title() {
        let (mut conn, _dir) = open_temp_db();

        // Two ROMs with empty base_title.
        MetadataDb::save_system_entries(
            &mut conn,
            "sega_smd",
            &[
                make_entry_with_base_title("sega_smd", "ROM_A.md", ""),
                make_entry_with_base_title("sega_smd", "ROM_B.md", ""),
            ],
            None,
        )
        .unwrap();

        // ROM_A has art, ROM_B does not.
        let enrichments = vec![BoxArtGenreRating {
            rom_filename: "ROM_A.md".into(),
            box_art_url: Some("/media/sega_smd/boxart/ROM_A.png".into()),
            genre: None,
            players: None,
            rating: None,
            rating_count: None,
        }];

        let rom_filenames = vec!["ROM_A.md".to_string(), "ROM_B.md".to_string()];

        let result = apply_base_title_fallback(&conn, "sega_smd", enrichments, &rom_filenames);

        // ROM_B should NOT get art — empty base_title is excluded.
        let rom_b = result.iter().find(|e| e.rom_filename == "ROM_B.md");
        assert!(
            rom_b.is_none(),
            "Empty base_title should not participate in fallback"
        );
    }

    #[test]
    fn base_title_fallback_adds_entry_for_rom_without_any_enrichment() {
        let (mut conn, _dir) = open_temp_db();

        // ROM with art and ROM without any enrichment data (no art, no rating, nothing).
        MetadataDb::save_system_entries(
            &mut conn,
            "snes",
            &[
                make_entry_with_base_title("snes", "Zelda (USA).sfc", "zelda"),
                make_entry_with_base_title("snes", "Zelda (Japan).sfc", "zelda"),
            ],
            None,
        )
        .unwrap();

        // Only USA is in enrichments. Japan was completely filtered out (no data).
        let enrichments = vec![BoxArtGenreRating {
            rom_filename: "Zelda (USA).sfc".into(),
            box_art_url: Some("/media/snes/boxart/Zelda.png".into()),
            genre: None,
            players: None,
            rating: None,
            rating_count: None,
        }];

        let rom_filenames = vec![
            "Zelda (USA).sfc".to_string(),
            "Zelda (Japan).sfc".to_string(),
        ];

        let result = apply_base_title_fallback(&conn, "snes", enrichments, &rom_filenames);

        let japan = result
            .iter()
            .find(|e| e.rom_filename == "Zelda (Japan).sfc")
            .expect("Japan entry should be created by fallback");
        assert_eq!(
            japan.box_art_url.as_deref(),
            Some("/media/snes/boxart/Zelda.png"),
            "Japan should get USA's art via base_title fallback (new entry)"
        );
    }

    #[test]
    fn non_clone_does_not_use_parent_fallback() {
        // "puckman" is NOT a clone — it's a parent ROM. If there's no art
        // matching "Puck Man", it should return NotFound (no parent to fall back to).
        let info = crate::arcade_db::lookup_arcade_game("puckman")
            .expect("puckman should be in arcade DB");
        assert!(!info.is_clone, "puckman should not be a clone");

        // Index has art for an unrelated game only.
        let index = image_index_from(&[("Unrelated Game", "boxart/Unrelated Game.png")]);

        let result = resolve_box_art(&index, "arcade_mame", "puckman.zip");
        assert!(
            matches!(result, BoxArtResult::NotFound),
            "Non-clone should not find art via parent fallback"
        );
    }
}
