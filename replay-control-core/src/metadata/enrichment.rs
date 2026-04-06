//! Pure enrichment pipeline: scan → match → resolve → produce updates.
//!
//! This module contains the data-oriented enrichment logic that takes a DB
//! connection, filesystem paths, and pre-loaded data, then produces enrichment
//! updates. No web server state, connection pools, or caches — the app layer
//! handles orchestration.

use std::collections::{HashMap, HashSet};

use rusqlite::Connection;

use crate::developer::normalize_developer;
use crate::metadata_db::{BoxArtGenreRating, MetadataDb};
use crate::thumbnail_manifest::ManifestMatch;

// Re-export image resolution types so existing `use enrichment::*` paths keep working.
pub use crate::image_resolution::{
    build_image_index, format_box_art_url, resolve_box_art, BoxArtResult, ImageIndex,
};

/// Batched metadata from LaunchBox import, keyed by ROM filename.
struct LaunchBoxMetadata {
    ratings: HashMap<String, f64>,
    genres: HashMap<String, String>,
    players: HashMap<String, u8>,
    rating_counts: HashMap<String, u32>,
    developers: HashMap<String, String>,
    release_years: HashMap<String, u16>,
    cooperative: HashSet<String>,
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
    /// Cooperative flag updates: rom_filenames that should be set to cooperative=1.
    pub cooperative_updates: Vec<String>,
    /// On-demand manifest matches that need background downloads.
    /// Each entry is (rom_filename, ManifestMatch).
    pub manifest_downloads: Vec<(String, ManifestMatch)>,
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
        cooperative: MetadataDb::system_metadata_cooperative(conn, system)
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
            cooperative_updates: Vec::new(),
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

    // Cooperative enrichment: set cooperative=1 for ROMs flagged by LaunchBox.
    // Only update ROMs that are not already cooperative (existing_cooperative tracks those).
    let existing_cooperative: HashSet<String> =
        MetadataDb::system_rom_cooperative(conn, system).unwrap_or_default();
    let cooperative_updates: Vec<String> = rom_filenames
        .iter()
        .filter(|f| !existing_cooperative.contains(*f))
        .filter(|f| lb.cooperative.contains(*f))
        .cloned()
        .collect();

    EnrichmentResult {
        enrichments,
        developer_updates,
        year_updates,
        cooperative_updates,
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
        if let Some(ref url) = e.box_art_url
            && let Some(bt) = base_titles.get(&e.rom_filename)
            && !bt.is_empty()
        {
            art_by_base_title.entry(bt.as_str()).or_insert(url.as_str());
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
        if e.box_art_url.is_none()
            && let Some(bt) = base_titles.get(&e.rom_filename)
            && !bt.is_empty()
            && let Some(url) = art_by_base_title.get(bt.as_str())
        {
            e.box_art_url = Some(url.clone());
        }
    }

    // Pass 2: ROMs with no enrichment entry at all (no art, no rating, no genre, etc.)
    // that can get art via base_title fallback.
    for filename in rom_filenames {
        if enriched.contains(filename) {
            continue;
        }
        if let Some(bt) = base_titles.get(filename)
            && !bt.is_empty()
            && let Some(url) = art_by_base_title.get(bt.as_str())
        {
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

    enrichments
}

#[cfg(test)]
mod tests {
    use super::*;

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
            cooperative: false,
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
    fn cooperative_or_merge_from_launchbox() {
        let (mut conn, _dir) = open_temp_db();

        // Insert a game into game_library with cooperative = false.
        let mut entry = make_entry_with_base_title("sega_smd", "Streets (USA).md", "streets");
        entry.cooperative = false;
        MetadataDb::save_system_entries(&mut conn, "sega_smd", &[entry], None).unwrap();

        // Insert same game into game_metadata with cooperative = true.
        let meta = crate::metadata_db::GameMetadata {
            cooperative: true,
            ..crate::metadata_db::tests::make_metadata(None)
        };
        MetadataDb::bulk_upsert(
            &mut conn,
            &[("sega_smd".into(), "Streets (USA).md".into(), meta)],
        )
        .unwrap();

        // Verify game_library starts with cooperative = false.
        let before = MetadataDb::load_system_entries(&conn, "sega_smd").unwrap();
        assert!(!before[0].cooperative, "should start non-cooperative");

        // Simulate the enrichment cooperative update (the enrich_system pipeline
        // reads game_metadata cooperative and produces cooperative_updates).
        let coop_set = MetadataDb::system_metadata_cooperative(&conn, "sega_smd").unwrap();
        assert!(coop_set.contains("Streets (USA).md"));

        let existing = MetadataDb::system_rom_cooperative(&conn, "sega_smd").unwrap();
        let updates: Vec<String> = coop_set
            .into_iter()
            .filter(|f| !existing.contains(f))
            .collect();
        MetadataDb::update_cooperative(&mut conn, "sega_smd", &updates).unwrap();

        let after = MetadataDb::load_system_entries(&conn, "sega_smd").unwrap();
        assert!(after[0].cooperative, "should be cooperative after enrichment (OR merge)");
    }

    #[test]
    fn enrichment_fills_genre_gap_but_does_not_overwrite() {
        let (mut conn, _dir) = open_temp_db();

        // Game with existing genre "Action".
        let mut entry_with_genre =
            make_entry_with_base_title("sega_smd", "Sonic (USA).md", "sonic");
        entry_with_genre.genre = Some("Action".into());
        entry_with_genre.genre_group = crate::genre::normalize_genre("Action").to_string();

        // Game with no genre.
        let entry_no_genre = make_entry_with_base_title("sega_smd", "Streets (USA).md", "streets");

        MetadataDb::save_system_entries(
            &mut conn,
            "sega_smd",
            &[entry_with_genre, entry_no_genre],
            None,
        )
        .unwrap();

        // Enrichment tries to set genre for both.
        MetadataDb::update_box_art_genre_rating(
            &mut conn,
            "sega_smd",
            &[
                crate::metadata_db::BoxArtGenreRating {
                    rom_filename: "Sonic (USA).md".into(),
                    box_art_url: None,
                    genre: Some("Adventure".into()),
                    players: None,
                    rating: None,
                    rating_count: None,
                },
                crate::metadata_db::BoxArtGenreRating {
                    rom_filename: "Streets (USA).md".into(),
                    box_art_url: None,
                    genre: Some("Adventure".into()),
                    players: None,
                    rating: None,
                    rating_count: None,
                },
            ],
        )
        .unwrap();

        let roms = MetadataDb::load_system_entries(&conn, "sega_smd").unwrap();
        let sonic = roms
            .iter()
            .find(|r| r.rom_filename == "Sonic (USA).md")
            .unwrap();
        let streets = roms
            .iter()
            .find(|r| r.rom_filename == "Streets (USA).md")
            .unwrap();

        assert_eq!(
            sonic.genre.as_deref(),
            Some("Action"),
            "existing genre should NOT be overwritten"
        );
        assert_eq!(
            streets.genre.as_deref(),
            Some("Adventure"),
            "NULL genre should be filled"
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

}
