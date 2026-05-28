//! Loader for community-curated metadata files under `data/community/`.
//!
//! Each file is `data/community/<system>.json`; the system is taken from the
//! file stem. See `replay_control_core::community::schema` for the contributor-
//! facing shape and `docs/contributing/community-metadata.md` for the guide.

use std::fs::{self, File};
use std::io::BufReader;
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

use replay_control_core::community::{CommunityEntry, CommunityFile};
use replay_control_core::library::resource_kind;

use crate::{normalize_console_genre, normalize_title, sha256_resource_id, title_utils};

/// Walk `data/community/*.json` and write community entries into
/// `canonical_game`, `rom_entry`, and `catalog_game_resource`. Returns the
/// number of entries inserted.
///
/// Collision policy: if `(system, filename_stem)` already has a `rom_entry`
/// row, the entry's `override` flag must be `true`; otherwise the build
/// aborts with a clear error naming the colliding source.
pub fn insert_community_entries(conn: &Connection, sources_dir: &Path) -> rusqlite::Result<usize> {
    let dir = sources_dir.join("community");
    let Ok(entries) = fs::read_dir(&dir) else {
        return Ok(0);
    };

    // Sort by path so the build is reproducible across machines — fs::read_dir
    // yields entries in filesystem order, which makes the enrichment-input
    // version and the eprintln! log differ between hosts otherwise.
    let mut json_paths: Vec<std::path::PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
        .collect();
    json_paths.sort();

    let mut total = 0usize;
    for path in &json_paths {
        let Some(system) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        total += load_community_file(conn, system, path)?;
    }

    eprintln!("Community metadata: Inserted {total} entries");
    Ok(total)
}

fn load_community_file(conn: &Connection, system: &str, path: &Path) -> rusqlite::Result<usize> {
    let file = File::open(path).map_err(|e| {
        rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(format!(
            "open {}: {e}",
            path.display()
        ))))
    })?;
    let parsed: CommunityFile = serde_json::from_reader(BufReader::new(file)).map_err(|e| {
        rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(format!(
            "parse {}: {e}",
            path.display()
        ))))
    })?;

    // Detect within-file duplicates up front so contributors get a clear error
    // pointing at the dup rather than the confusing "collides with existing
    // rom_entry from source 'community'; set override: true" that the
    // collision check would otherwise emit against the entry's own predecessor.
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for entry in &parsed.entries {
        if !seen.insert(entry.filename_stem.as_str()) {
            return Err(io_err(format!(
                "community file {} has duplicate filename_stem '{}'",
                path.display(),
                entry.filename_stem
            )));
        }
    }

    let mut inserted = 0usize;
    for entry in &parsed.entries {
        insert_entry(conn, system, entry)?;
        inserted += 1;
    }
    // Skip the log line for empty template files so the build output stays
    // readable when many systems ship with a placeholder.
    if inserted > 0 {
        eprintln!(
            "Community metadata: {} entries from {}",
            inserted,
            path.display()
        );
    }
    Ok(inserted)
}

fn insert_entry(conn: &Connection, system: &str, entry: &CommunityEntry) -> rusqlite::Result<()> {
    if entry.filename_stem.is_empty() {
        return Err(io_err(format!(
            "community entry for system {system} has empty filename_stem"
        )));
    }
    if entry.display_name.is_empty() {
        return Err(io_err(format!(
            "community entry {}/{} has empty display_name",
            system, entry.filename_stem
        )));
    }

    let existing: Option<(i64, String)> = conn
        .query_row(
            "SELECT cg.id, cg.source FROM rom_entry re \
             JOIN canonical_game cg ON cg.id = re.canonical_game_id \
             WHERE re.system = ?1 AND re.filename_stem = ?2 LIMIT 1",
            params![system, entry.filename_stem],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    if let Some((existing_cg_id, existing_source)) = existing {
        if !entry.override_existing {
            return Err(io_err(format!(
                "community entry {}/{} collides with existing rom_entry from source '{}'; \
                 set \"override\": true to replace it",
                system, entry.filename_stem, existing_source
            )));
        }
        // Delete the colliding rom_entry first, then check whether the old
        // canonical_game is orphaned (no other rom_entry referencing it). If
        // orphaned, drop it along with its rom_alternate rows so
        // system_games_by_id and alias resolution don't surface ghost data.
        conn.execute(
            "DELETE FROM rom_entry WHERE system = ?1 AND filename_stem = ?2",
            params![system, entry.filename_stem],
        )?;
        let still_referenced: i64 = conn.query_row(
            "SELECT COUNT(*) FROM rom_entry WHERE canonical_game_id = ?1",
            params![existing_cg_id],
            |row| row.get(0),
        )?;
        if still_referenced == 0 {
            conn.execute(
                "DELETE FROM rom_alternate WHERE canonical_game_id = ?1",
                params![existing_cg_id],
            )?;
            conn.execute(
                "DELETE FROM canonical_game WHERE id = ?1",
                params![existing_cg_id],
            )?;
        }
    }

    let description = entry
        .description
        .as_ref()
        .map(|d| d.en().to_string())
        .unwrap_or_default();
    if entry.description.is_some() && description.is_empty() {
        return Err(io_err(format!(
            "community entry {}/{} description is missing the required \"en\" key",
            system, entry.filename_stem
        )));
    }
    let coop_val: Option<i64> = entry.coop.map(|b| b as i64);
    let genre = entry.genre.clone().unwrap_or_default();
    let normalized_genre = normalize_console_genre(&genre);
    conn.execute(
        "INSERT INTO canonical_game \
         (system, display_name, year, genre, developer, publisher, players, coop, rating, normalized_genre, description, source) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, '', ?9, ?10, ?11)",
        params![
            system,
            entry.display_name,
            entry.year.unwrap_or(0) as i64,
            genre,
            entry.developer.clone().unwrap_or_default(),
            entry.publisher.clone().unwrap_or_default(),
            entry.players.unwrap_or(0) as i64,
            coop_val,
            normalized_genre,
            description,
            resource_kind::COMMUNITY_SOURCE,
        ],
    )?;
    let canonical_game_id = conn.last_insert_rowid();

    let crc32_val: i64 = match entry.crc32.as_deref() {
        None => 0,
        Some(s) => {
            let trimmed = s.trim().trim_start_matches("0x").trim_start_matches("0X");
            // CRC32 is exactly 8 hex chars. Accepting shorter strings like
            // "1" silently stores 0x00000001, which is almost certainly a
            // contributor typo and risks matching the wrong ROM on lookup.
            if trimmed.len() != 8 {
                return Err(io_err(format!(
                    "community entry {}/{} has invalid crc32 '{}' (expected exactly 8 hex chars, with optional 0x prefix)",
                    system, entry.filename_stem, s
                )));
            }
            match u32::from_str_radix(trimmed, 16) {
                Ok(v) => v as i64,
                Err(_) => {
                    return Err(io_err(format!(
                        "community entry {}/{} has invalid crc32 '{}' (non-hex characters)",
                        system, entry.filename_stem, s
                    )));
                }
            }
        }
    };
    // rom_entry.normalized_title must match what game_db::normalize_filename
    // produces (space-separated, alphanumerics only) since that's what the
    // runtime fuzzy-fallback queries use — see lookup_by_normalized_titles_batch
    // callers in library/game_entry_builder.rs and the search server fn.
    // catalog_game_resource.normalized_title uses the compact metadata form
    // (the shmups/manuals convention), handled separately in insert_resources.
    let rom_normalized_title = normalize_title(&entry.display_name);
    conn.execute(
        "INSERT INTO rom_entry (system, filename_stem, region, crc32, canonical_game_id, normalized_title) \
         VALUES (?1, ?2, '', ?3, ?4, ?5)",
        params![
            system,
            entry.filename_stem,
            crc32_val,
            canonical_game_id,
            rom_normalized_title,
        ],
    )?;

    let resource_normalized_title = title_utils::normalize_title_for_metadata(&entry.display_name);
    if resource_normalized_title.is_empty()
        && (entry.boxart_url.is_some()
            || entry.title_image_url.is_some()
            || !entry.screenshot_urls.is_empty()
            || !entry.manuals.is_empty()
            || !entry.videos.is_empty()
            || !entry.strategy_guides.is_empty()
            || !entry.video_indexes.is_empty())
    {
        eprintln!(
            "warning: community entry {}/{} has display_name that normalizes to empty; \
             skipping {} catalog_game_resource rows",
            system,
            entry.filename_stem,
            entry.manuals.len()
                + entry.videos.len()
                + entry.strategy_guides.len()
                + entry.video_indexes.len()
                + entry.screenshot_urls.len()
                + entry.boxart_url.iter().count()
                + entry.title_image_url.iter().count()
        );
    }
    insert_resources(conn, system, &resource_normalized_title, entry)?;
    Ok(())
}

fn insert_resources(
    conn: &Connection,
    system: &str,
    normalized_title: &str,
    entry: &CommunityEntry,
) -> rusqlite::Result<()> {
    if normalized_title.is_empty() {
        return Ok(());
    }

    let mut stmt = conn.prepare_cached(
        "INSERT OR IGNORE INTO catalog_game_resource
         (system, normalized_title, resource_type, source, resource_id, url, title, languages, mime_type)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )?;

    if let Some(url) = entry.boxart_url.as_deref().filter(|u| !u.is_empty()) {
        stmt.execute(params![
            system,
            normalized_title,
            resource_kind::BOXART,
            resource_kind::COMMUNITY_SOURCE,
            sha256_resource_id(url),
            url,
            entry.display_name,
            "",
            "image/*",
        ])?;
    }
    if let Some(url) = entry.title_image_url.as_deref().filter(|u| !u.is_empty()) {
        stmt.execute(params![
            system,
            normalized_title,
            resource_kind::TITLE_IMAGE,
            resource_kind::COMMUNITY_SOURCE,
            sha256_resource_id(url),
            url,
            entry.display_name,
            "",
            "image/*",
        ])?;
    }
    for url in entry.screenshot_urls.iter().filter(|u| !u.is_empty()) {
        stmt.execute(params![
            system,
            normalized_title,
            resource_kind::SCREENSHOT,
            resource_kind::COMMUNITY_SOURCE,
            sha256_resource_id(url),
            url,
            entry.display_name,
            "",
            "image/*",
        ])?;
    }
    for manual in &entry.manuals {
        if manual.url.is_empty() {
            continue;
        }
        stmt.execute(params![
            system,
            normalized_title,
            resource_kind::MANUAL,
            resource_kind::COMMUNITY_SOURCE,
            sha256_resource_id(&manual.url),
            manual.url,
            manual.title.clone().unwrap_or_default(),
            manual.language.clone().unwrap_or_default(),
            manual.mime_type.as_deref().unwrap_or("application/pdf"),
        ])?;
    }
    for video in &entry.videos {
        if video.url.is_empty() {
            continue;
        }
        stmt.execute(params![
            system,
            normalized_title,
            resource_kind::VIDEO,
            resource_kind::COMMUNITY_SOURCE,
            sha256_resource_id(&video.url),
            video.url,
            video.title.clone().unwrap_or_default(),
            "",
            "text/html",
        ])?;
    }
    for guide in &entry.strategy_guides {
        if guide.url.is_empty() {
            continue;
        }
        stmt.execute(params![
            system,
            normalized_title,
            resource_kind::STRATEGY_GUIDE,
            resource_kind::COMMUNITY_SOURCE,
            sha256_resource_id(&guide.url),
            guide.url,
            guide.title.clone().unwrap_or_default(),
            "",
            "text/html",
        ])?;
    }
    for index in &entry.video_indexes {
        if index.url.is_empty() {
            continue;
        }
        stmt.execute(params![
            system,
            normalized_title,
            resource_kind::VIDEO_INDEX,
            resource_kind::COMMUNITY_SOURCE,
            sha256_resource_id(&index.url),
            index.url,
            index.title.clone().unwrap_or_default(),
            "",
            "text/html",
        ])?;
    }

    Ok(())
}

fn io_err(msg: String) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(msg)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_sources_dir() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("build-catalog-community-{unique}"))
    }

    fn open_with_schema() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::create_schema(&conn).unwrap();
        conn
    }

    fn amigavision_fixture() -> &'static str {
        r#"{
            "entries": [
                {
                    "filename_stem": "AmigaVision",
                    "display_name": "AmigaVision",
                    "year": 2024,
                    "developer": "AmigaVision Project",
                    "publisher": "AmigaVision Project",
                    "genre": "Compilation",
                    "players": 1,
                    "description": "A curated collection.",
                    "boxart_url": "https://example.com/boxart.png",
                    "title_image_url": "https://example.com/title.png",
                    "screenshot_urls": [
                        "https://example.com/s1.png",
                        "https://example.com/s2.png"
                    ],
                    "manuals": [
                        {"url": "https://example.com/manual.pdf", "language": "en", "title": "Manual"}
                    ],
                    "videos": [
                        {"url": "https://youtube.com/v", "title": "Overview"}
                    ],
                    "strategy_guides": [
                        {"url": "https://example.com/guide", "title": "Setup"}
                    ]
                }
            ]
        }"#
    }

    #[test]
    fn round_trip_amigavision() {
        let dir = temp_sources_dir();
        let community_dir = dir.join("community");
        fs::create_dir_all(&community_dir).unwrap();
        fs::write(
            community_dir.join("commodore_ami.json"),
            amigavision_fixture(),
        )
        .unwrap();

        let conn = open_with_schema();
        let inserted = insert_community_entries(&conn, &dir).unwrap();
        assert_eq!(inserted, 1);

        let (display, description, developer, publisher, source): (
            String,
            String,
            String,
            String,
            String,
        ) = conn
            .query_row(
                "SELECT display_name, description, developer, publisher, source FROM canonical_game WHERE system = 'commodore_ami'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!(display, "AmigaVision");
        assert_eq!(description, "A curated collection.");
        assert_eq!(developer, "AmigaVision Project");
        assert_eq!(publisher, "AmigaVision Project");
        assert_eq!(source, "community");

        let rom_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM rom_entry WHERE system='commodore_ami' AND filename_stem='AmigaVision'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(rom_count, 1);

        let boxart: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM catalog_game_resource WHERE resource_type='boxart' AND source='community'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(boxart, 1);
        let snap: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM catalog_game_resource WHERE resource_type='snap' AND source='community'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(snap, 2);
        let manual: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM catalog_game_resource WHERE resource_type='manual' AND source='community'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(manual, 1);
        let video: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM catalog_game_resource WHERE resource_type='video' AND source='community'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(video, 1);
        let guide: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM catalog_game_resource WHERE resource_type='strategy_guide' AND source='community'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(guide, 1);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn collision_without_override_fails() {
        let dir = temp_sources_dir();
        let community_dir = dir.join("community");
        fs::create_dir_all(&community_dir).unwrap();
        fs::write(
            community_dir.join("nintendo_snes.json"),
            r#"{"entries":[{"filename_stem":"Super Mario World","display_name":"Super Mario World"}]}"#,
        )
        .unwrap();

        let conn = open_with_schema();
        conn.execute(
            "INSERT INTO canonical_game (system, display_name, source) VALUES ('nintendo_snes', 'Super Mario World', 'no-intro')",
            [],
        )
        .unwrap();
        let cg_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO rom_entry (system, filename_stem, canonical_game_id, normalized_title) VALUES ('nintendo_snes', 'Super Mario World', ?1, 'super mario world')",
            params![cg_id],
        )
        .unwrap();

        let err = insert_community_entries(&conn, &dir).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("no-intro") && msg.contains("override"),
            "error should mention colliding source and override flag, got: {msg}"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn override_replaces_existing() {
        let dir = temp_sources_dir();
        let community_dir = dir.join("community");
        fs::create_dir_all(&community_dir).unwrap();
        fs::write(
            community_dir.join("nintendo_snes.json"),
            r#"{"entries":[{"filename_stem":"Super Mario World","display_name":"Super Mario World","override":true}]}"#,
        )
        .unwrap();

        let conn = open_with_schema();
        conn.execute(
            "INSERT INTO canonical_game (system, display_name, source) VALUES ('nintendo_snes', 'Super Mario World', 'no-intro')",
            [],
        )
        .unwrap();
        let cg_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO rom_entry (system, filename_stem, canonical_game_id, normalized_title) VALUES ('nintendo_snes', 'Super Mario World', ?1, 'super mario world')",
            params![cg_id],
        )
        .unwrap();

        // Seed an alternate name pointing at the same canonical_game to verify
        // the override path cleans up rom_alternate alongside canonical_game.
        conn.execute(
            "INSERT INTO rom_alternate (canonical_game_id, system, alternate_name) VALUES (?1, 'nintendo_snes', 'Super Mario Bros 4')",
            params![cg_id],
        )
        .unwrap();

        insert_community_entries(&conn, &dir).unwrap();

        let source: String = conn
            .query_row(
                "SELECT cg.source FROM rom_entry re \
                 JOIN canonical_game cg ON cg.id = re.canonical_game_id \
                 WHERE re.system='nintendo_snes' AND re.filename_stem='Super Mario World'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(source, "community");

        // The old canonical_game and its rom_alternate must be removed —
        // not just hidden behind the new rom_entry. Orphan canonical_game rows
        // leak into system_games_by_id and alias resolution. Assert by content
        // because SQLite recycles INTEGER PRIMARY KEY rowids after DELETE.
        let no_intro_cg_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM canonical_game WHERE system='nintendo_snes' AND source='no-intro'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            no_intro_cg_count, 0,
            "old no-intro canonical_game should be deleted"
        );
        let stale_alt_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM rom_alternate WHERE alternate_name='Super Mario Bros 4'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stale_alt_count, 0, "old rom_alternate should be deleted");
        // And confirm exactly one canonical_game survives for this system —
        // the community replacement.
        let cg_total: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM canonical_game WHERE system='nintendo_snes'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            cg_total, 1,
            "exactly one canonical_game remains after override"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rom_entry_normalized_title_is_space_separated() {
        // game_db::normalize_filename produces space-separated form, so
        // rom_entry.normalized_title must match — otherwise multi-word
        // community ROMs are unreachable via the normalized-title fallback.
        let dir = temp_sources_dir();
        let community_dir = dir.join("community");
        fs::create_dir_all(&community_dir).unwrap();
        fs::write(
            community_dir.join("nintendo_snes.json"),
            r#"{"entries":[{"filename_stem":"Hyper Stunt","display_name":"Hyper Stunt 2000"}]}"#,
        )
        .unwrap();

        let conn = open_with_schema();
        insert_community_entries(&conn, &dir).unwrap();

        let rom_norm: String = conn
            .query_row(
                "SELECT normalized_title FROM rom_entry WHERE filename_stem='Hyper Stunt'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(rom_norm, "hyper stunt 2000");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn catalog_resource_normalized_title_is_compact() {
        // catalog_game_resource.normalized_title uses the shmups/manuals
        // convention (compact, no spaces). Verify community rows match.
        let dir = temp_sources_dir();
        let community_dir = dir.join("community");
        fs::create_dir_all(&community_dir).unwrap();
        fs::write(
            community_dir.join("nintendo_snes.json"),
            r#"{"entries":[{"filename_stem":"Hyper Stunt","display_name":"Hyper Stunt 2000",
                "manuals":[{"url":"https://example.com/m.pdf"}]}]}"#,
        )
        .unwrap();

        let conn = open_with_schema();
        insert_community_entries(&conn, &dir).unwrap();

        let res_norm: String = conn
            .query_row(
                "SELECT normalized_title FROM catalog_game_resource WHERE resource_type='manual'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(res_norm, "hyperstunt2000");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn genre_is_normalized() {
        let dir = temp_sources_dir();
        let community_dir = dir.join("community");
        fs::create_dir_all(&community_dir).unwrap();
        fs::write(
            community_dir.join("nintendo_snes.json"),
            r#"{"entries":[{"filename_stem":"x","display_name":"X","genre":"Action / Platformer"}]}"#,
        )
        .unwrap();

        let conn = open_with_schema();
        insert_community_entries(&conn, &dir).unwrap();

        let (genre, normalized_genre): (String, String) = conn
            .query_row(
                "SELECT genre, normalized_genre FROM canonical_game WHERE display_name='X'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(genre, "Action / Platformer");
        assert!(
            !normalized_genre.is_empty(),
            "normalized_genre should pass through normalize_console_genre, got empty"
        );
    }

    #[test]
    fn invalid_crc32_is_rejected() {
        let dir = temp_sources_dir();
        let community_dir = dir.join("community");
        fs::create_dir_all(&community_dir).unwrap();
        fs::write(
            community_dir.join("nintendo_snes.json"),
            r#"{"entries":[{"filename_stem":"x","display_name":"X","crc32":"NOTHEX!!"}]}"#,
        )
        .unwrap();

        let conn = open_with_schema();
        let err = insert_community_entries(&conn, &dir).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("invalid crc32"),
            "error should reject bad crc32, got: {msg}"
        );
    }

    #[test]
    fn short_crc32_is_rejected() {
        // "1" parses as u32 but is almost certainly a contributor typo —
        // accepting it would silently store CRC=00000001 and risk wrong-ROM
        // matches at lookup time. CRC32 must be exactly 8 hex chars.
        let dir = temp_sources_dir();
        let community_dir = dir.join("community");
        fs::create_dir_all(&community_dir).unwrap();
        fs::write(
            community_dir.join("nintendo_snes.json"),
            r#"{"entries":[{"filename_stem":"x","display_name":"X","crc32":"1"}]}"#,
        )
        .unwrap();

        let conn = open_with_schema();
        let err = insert_community_entries(&conn, &dir).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("8 hex chars"),
            "error should mention expected length, got: {msg}"
        );
    }

    #[test]
    fn valid_crc32_with_and_without_prefix_is_accepted() {
        let dir = temp_sources_dir();
        let community_dir = dir.join("community");
        fs::create_dir_all(&community_dir).unwrap();
        fs::write(
            community_dir.join("nintendo_snes.json"),
            r#"{"entries":[
                {"filename_stem":"a","display_name":"A","crc32":"DEADBEEF"},
                {"filename_stem":"b","display_name":"B","crc32":"0xCAFEBABE"}
            ]}"#,
        )
        .unwrap();

        let conn = open_with_schema();
        insert_community_entries(&conn, &dir).unwrap();

        let crcs: Vec<i64> = conn
            .prepare(
                "SELECT crc32 FROM rom_entry WHERE system='nintendo_snes' ORDER BY filename_stem",
            )
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(crcs, vec![0xDEADBEEFi64, 0xCAFEBABEi64]);
    }

    #[test]
    fn duplicate_filename_stem_in_one_file_is_rejected() {
        let dir = temp_sources_dir();
        let community_dir = dir.join("community");
        fs::create_dir_all(&community_dir).unwrap();
        fs::write(
            community_dir.join("nintendo_snes.json"),
            r#"{"entries":[
                {"filename_stem":"Foo","display_name":"Foo 1"},
                {"filename_stem":"Foo","display_name":"Foo 2"}
            ]}"#,
        )
        .unwrap();

        let conn = open_with_schema();
        let err = insert_community_entries(&conn, &dir).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("duplicate filename_stem"),
            "error should flag in-file duplicate, got: {msg}"
        );
    }

    #[test]
    fn localized_description_prefers_en() {
        let dir = temp_sources_dir();
        let community_dir = dir.join("community");
        fs::create_dir_all(&community_dir).unwrap();
        fs::write(
            community_dir.join("commodore_ami.json"),
            r#"{"entries":[{"filename_stem":"x","display_name":"X",
                "description":{"en":"hi","ja":"こんにちは"}}]}"#,
        )
        .unwrap();

        let conn = open_with_schema();
        insert_community_entries(&conn, &dir).unwrap();

        let description: String = conn
            .query_row(
                "SELECT description FROM canonical_game WHERE display_name='X'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(description, "hi");

        fs::remove_dir_all(&dir).ok();
    }
}
