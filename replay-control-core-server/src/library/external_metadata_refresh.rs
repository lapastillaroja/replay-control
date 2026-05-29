//! Writers for the host-global `external_metadata.db`.
//!
//! Refreshes happen in-place via a single SQLite transaction:
//! `BEGIN; DELETE FROM <source-tables>; INSERT...; UPDATE external_meta; COMMIT`.
//! The reader pool keeps its open connection — SQLite's MVCC gives in-flight
//! readers either the old-complete or new-complete state.

use std::collections::HashMap;
use std::path::Path;

use rusqlite::{Connection, params};

use crate::external_metadata::{self, LAUNCHBOX_PROVIDER, meta_keys};
use crate::library::imports::launchbox::{LbGame, normalize_title, parse_xml, platform_map};
use crate::library_db::{DatePrecision, DpSql};
use replay_control_core::error::{Error, Result};

/// Outcome of one LaunchBox refresh.
#[derive(Debug, Default, Clone)]
pub struct LaunchboxRefreshStats {
    pub source_entries: usize,
    pub games_written: usize,
    pub alternates_written: usize,
    pub resources_written: usize,
}

/// Parsed LaunchBox data ready to apply to `external_metadata.db`.
pub struct PreparedLaunchboxRefresh {
    xml_crc32: String,
    games: HashMap<(String, String), LaunchboxGameRow>,
    alternates: Vec<(String, String, String, String)>,
    resources: HashMap<(String, String, String), LaunchboxResourceRow>,
    source_entries: usize,
}

/// One row destined for `provider_game`. Mirrors the schema declared in
/// `external_metadata.rs`.
struct LaunchboxGameRow {
    description: Option<String>,
    genre: Option<String>,
    developer: Option<String>,
    publisher: Option<String>,
    release_date: Option<String>,
    release_precision: Option<DatePrecision>,
    rating: Option<f64>,
    rating_count: Option<u32>,
    cooperative: bool,
    players: Option<u8>,
}

#[derive(Debug, Clone)]
struct LaunchboxResourceRow {
    resource_id: String,
    url: String,
    title: Option<String>,
    platform: Option<String>,
}

fn skip_empty(s: &str) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

/// Fill empty fields on `dst` from `src`. Non-empty fields on `dst` are kept.
///
/// LaunchBox dual-lists many games across platform tags (e.g. a title appears
/// under both `Sega ST-V` and `Arcade`, where the latter typically carries
/// VideoURL/Overview/Wikipedia and the former is name-only). When a system's
/// `launchbox_platforms` field lists both tags, the importer sees two rows for
/// the same `(system, normalized_title)` key; this merge ensures the rich row
/// fills in the sparse row's gaps regardless of XML iteration order.
fn merge_in_place(dst: &mut LaunchboxGameRow, src: LaunchboxGameRow) {
    if dst.description.is_none() {
        dst.description = src.description;
    }
    if dst.genre.is_none() {
        dst.genre = src.genre;
    }
    if dst.developer.is_none() {
        dst.developer = src.developer;
    }
    if dst.publisher.is_none() {
        dst.publisher = src.publisher;
    }
    if dst.release_date.is_none() {
        dst.release_date = src.release_date;
        dst.release_precision = src.release_precision;
    }
    // rating and rating_count move as a unit, gated on the rating: a vote
    // count belongs to its own rating value, so we never staple src's count
    // onto a different dst rating. When dst already has a rating we keep both
    // of dst's fields (count may legitimately be None — a rating with no
    // recorded vote count). For the dual-listed same-game case both rows carry
    // identical rating fields, so the pairing is a no-op there.
    if dst.rating.is_none() {
        dst.rating = src.rating;
        dst.rating_count = src.rating_count;
    }
    if dst.players.is_none() {
        dst.players = src.players;
    }
    // `cooperative` is bool, not Option. Treat `true` as the signal — only
    // promote, never demote.
    if !dst.cooperative {
        dst.cooperative = src.cooperative;
    }
}

fn row_from_lb(g: &LbGame) -> Option<LaunchboxGameRow> {
    // Skip entries with no useful data — same gate as the legacy importer
    // so the host-global DB doesn't grow with thousands of empty rows.
    if g.overview.is_empty()
        && g.rating.is_none()
        && g.genre.is_empty()
        && g.developer.is_empty()
        && g.publisher.is_empty()
        && g.max_players.is_none()
        && g.release_date.is_none()
        && !g.cooperative
    {
        return None;
    }
    Some(LaunchboxGameRow {
        description: skip_empty(&g.overview),
        genre: skip_empty(&g.genre),
        developer: skip_empty(&g.developer),
        publisher: skip_empty(&g.publisher),
        release_date: g.release_date.clone(),
        release_precision: g.release_precision,
        rating: g.rating,
        rating_count: g.rating_count,
        cooperative: g.cooperative,
        players: g.max_players,
    })
}

fn video_resource_from_lb(g: &LbGame) -> Option<LaunchboxResourceRow> {
    let url = g.video_url.trim();
    if url.is_empty() {
        return None;
    }
    match replay_control_core::video_url::parse_video_url(url) {
        Ok(parsed) => Some(LaunchboxResourceRow {
            resource_id: format!("{}:{}", parsed.platform, parsed.video_id),
            url: parsed.canonical_url,
            title: if g.name.is_empty() {
                None
            } else {
                Some(g.name.clone())
            },
            platform: Some(parsed.platform.as_str().to_string()),
        }),
        Err(_) => {
            let digest = ring::digest::digest(&ring::digest::SHA256, url.as_bytes());
            let hex = digest
                .as_ref()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>();
            Some(LaunchboxResourceRow {
                resource_id: format!("urlhash:{hex}"),
                url: url.to_string(),
                title: if g.name.is_empty() {
                    None
                } else {
                    Some(g.name.clone())
                },
                platform: None,
            })
        }
    }
}

/// Parse a LaunchBox XML file and build the rows needed for refresh.
///
/// This function does not touch SQLite. Call it before acquiring the
/// `external_metadata.db` writer so the writer slot is held only while rows
/// are applied.
pub fn prepare_launchbox_refresh(
    xml_path: &Path,
    on_progress: impl Fn(usize) + Send + Sync,
) -> Result<PreparedLaunchboxRefresh> {
    // Hash before any DB work — if parsing or writing fails, the stamp is
    // never persisted and the next boot retries automatically.
    let xml_crc32 = external_metadata::hash_file_crc32(xml_path)?;

    let file = std::fs::File::open(xml_path).map_err(|e| Error::io(xml_path, e))?;
    let reader = std::io::BufReader::with_capacity(256 * 1024, file);
    let platforms = platform_map();

    // Per-(system, normalized_title) dedup with field-level richer-wins
    // merging. Two source entries collapse to the same key when:
    //   1. A title appears with parenthetical noise — "Game" and "Game ()"
    //      normalize identically. Rare.
    //   2. A title is dual-listed under multiple platform tags both mapped to
    //      the same system — e.g. arcade_stv includes both `Sega ST-V`
    //      (sparse, often name-only) and `Arcade` (richer, with VideoURL,
    //      Overview, Wikipedia). Common.
    // `merge_in_place` keeps existing non-empty fields and fills empties from
    // the incoming row, so the final row is the union of both sources
    // regardless of XML iteration order.
    let mut games: HashMap<(String, String), LaunchboxGameRow> = HashMap::new();
    // database_id → all (system, normalized_title) rows the game ended up in.
    // Used to attach LaunchBox alternate names to the right provider_game keys.
    let mut db_id_to_keys: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut resources: HashMap<(String, String, String), LaunchboxResourceRow> = HashMap::new();
    let mut source_entries = 0usize;

    let parse_result = parse_xml(reader, &platforms, |game, system_folder| {
        source_entries += 1;
        let norm = normalize_title(&game.name);
        if norm.is_empty() {
            return;
        }
        let key = (system_folder.to_string(), norm);
        if !game.database_id.is_empty() {
            db_id_to_keys
                .entry(game.database_id.clone())
                .or_default()
                .push(key.clone());
        }
        if let Some(row) = row_from_lb(game) {
            match games.entry(key.clone()) {
                std::collections::hash_map::Entry::Vacant(v) => {
                    v.insert(row);
                }
                std::collections::hash_map::Entry::Occupied(mut o) => {
                    merge_in_place(o.get_mut(), row);
                }
            }
        }
        if let Some(resource) = video_resource_from_lb(game) {
            resources.insert(
                (key.0.clone(), key.1.clone(), resource.resource_id.clone()),
                resource,
            );
        }

        if source_entries.is_multiple_of(5000) {
            on_progress(source_entries);
        }
    })?;

    // Resolve alternate names against the (system, normalized_title) keys
    // built during the game pass. Compute `normalized_alternate` once here so
    // the enrichment matcher can do hashmap lookups against ROM filenames'
    // stored normalized titles instead of normalizing per ROM at match time.
    let mut alternates: Vec<(String, String, String, String)> = Vec::new();
    for alt in &parse_result.alternate_names {
        if alt.alternate_name.is_empty() || alt.database_id.is_empty() {
            continue;
        }
        let norm_alt = normalize_title(&alt.alternate_name);
        if let Some(keys) = db_id_to_keys.get(&alt.database_id) {
            for (system, normalized_title) in keys {
                alternates.push((
                    system.clone(),
                    normalized_title.clone(),
                    alt.alternate_name.clone(),
                    norm_alt.clone(),
                ));
            }
        }
    }

    on_progress(source_entries);
    Ok(PreparedLaunchboxRefresh {
        xml_crc32,
        games,
        alternates,
        resources,
        source_entries,
    })
}

/// Apply parsed LaunchBox rows to `external_metadata.db`.
///
/// Single in-place transaction. On success, stamps
/// `external_meta.launchbox_xml_crc32` so the next boot's freshness check
/// is a no-op until the XML changes.
pub fn apply_launchbox_refresh(
    conn: &mut Connection,
    prepared: PreparedLaunchboxRefresh,
) -> Result<LaunchboxRefreshStats> {
    let PreparedLaunchboxRefresh {
        xml_crc32,
        games,
        alternates,
        resources,
        source_entries,
    } = prepared;

    // External_metadata is set to synchronous=FULL at pool open so each
    // commit fsyncs.
    let games_written;
    let alternates_written;
    let resources_written;
    {
        let tx = conn
            .transaction()
            .map_err(|e| Error::Other(format!("begin: {e}")))?;
        tx.execute(
            "DELETE FROM provider_game WHERE provider = ?1",
            [LAUNCHBOX_PROVIDER],
        )
        .map_err(|e| Error::Other(format!("clear provider_game: {e}")))?;
        tx.execute(
            "DELETE FROM provider_alternate WHERE provider = ?1",
            [LAUNCHBOX_PROVIDER],
        )
        .map_err(|e| Error::Other(format!("clear provider_alternate: {e}")))?;
        tx.execute(
            "DELETE FROM provider_resource WHERE provider = ?1",
            [LAUNCHBOX_PROVIDER],
        )
        .map_err(|e| Error::Other(format!("clear provider_resource: {e}")))?;

        {
            let mut game_stmt = tx
                .prepare(
                    "INSERT INTO provider_game
                       (provider, system, normalized_title, description, genre, developer, publisher,
                        release_date, release_precision, rating, rating_count,
                        cooperative, players)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                )
                .map_err(|e| Error::Other(format!("prepare insert game: {e}")))?;
            let mut count = 0usize;
            for ((system, normalized_title), row) in &games {
                game_stmt
                    .execute(params![
                        LAUNCHBOX_PROVIDER,
                        system,
                        normalized_title,
                        row.description,
                        row.genre,
                        row.developer,
                        row.publisher,
                        row.release_date,
                        row.release_precision.map(DpSql),
                        row.rating,
                        row.rating_count.map(|c| c as i64),
                        row.cooperative as i32,
                        row.players.map(|p| p as i32),
                    ])
                    .map_err(|e| Error::Other(format!("insert provider_game: {e}")))?;
                count += 1;
            }
            games_written = count;
        }
        {
            let mut alt_stmt = tx
                .prepare(
                    "INSERT OR IGNORE INTO provider_alternate
                       (provider, system, normalized_title, alternate_name, normalized_alternate)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                )
                .map_err(|e| Error::Other(format!("prepare insert alt: {e}")))?;
            let mut count = 0usize;
            for (system, normalized_title, alternate_name, normalized_alternate) in &alternates {
                alt_stmt
                    .execute(params![
                        LAUNCHBOX_PROVIDER,
                        system,
                        normalized_title,
                        alternate_name,
                        normalized_alternate,
                    ])
                    .map_err(|e| Error::Other(format!("insert provider_alternate: {e}")))?;
                count += 1;
            }
            alternates_written = count;
        }
        {
            let mut resource_stmt = tx
                .prepare(
                    "INSERT OR IGNORE INTO provider_resource
                       (provider, system, normalized_title, resource_type, resource_id,
                        url, title, languages, platform, mime_type)
                     VALUES (?1, ?2, ?3, 'video', ?4, ?5, ?6, '', ?7, '')",
                )
                .map_err(|e| Error::Other(format!("prepare insert provider_resource: {e}")))?;
            let mut count = 0usize;
            for ((system, normalized_title, _resource_id), row) in &resources {
                resource_stmt
                    .execute(params![
                        LAUNCHBOX_PROVIDER,
                        system,
                        normalized_title,
                        row.resource_id,
                        row.url,
                        row.title,
                        row.platform,
                    ])
                    .map_err(|e| Error::Other(format!("insert provider_resource: {e}")))?;
                count += 1;
            }
            resources_written = count;
        }

        external_metadata::write_meta(&tx, meta_keys::LAUNCHBOX_XML_CRC32, Some(&xml_crc32))?;
        // Stamp the normalizer version so a future boot can detect
        // version skew between the deployed binary and the cached
        // normalized values in `provider_alternate`.
        external_metadata::write_meta(
            &tx,
            meta_keys::TITLE_NORM_VERSION,
            Some(&replay_control_core::title_utils::TITLE_NORM_VERSION.to_string()),
        )?;
        // Stamp the LaunchBox platform map fingerprint so adding a new
        // system (e.g. `arcade_stv`) on a future deploy re-triggers the
        // reparse even when the XML hash hasn't changed.
        external_metadata::write_meta(
            &tx,
            meta_keys::LAUNCHBOX_PLATFORM_MAP_HASH,
            Some(&replay_control_core::systems::launchbox_platform_map_fingerprint()),
        )?;
        tx.commit()
            .map_err(|e| Error::Other(format!("commit: {e}")))?;
    }

    tracing::info!(
        "external_metadata launchbox refresh: {source_entries} source entries, \
         {games_written} games, {alternates_written} alternates, {resources_written} resources"
    );

    Ok(LaunchboxRefreshStats {
        source_entries,
        games_written,
        alternates_written,
        resources_written,
    })
}

/// Refresh the LaunchBox tables in `external_metadata.db` from the given XML.
///
/// Convenience wrapper for synchronous callers. Async pool callers should use
/// `prepare_launchbox_refresh` before acquiring the writer, then
/// `apply_launchbox_refresh` inside the writer closure.
pub fn refresh_launchbox(
    xml_path: &Path,
    conn: &mut Connection,
    on_progress: impl Fn(usize) + Send + Sync,
) -> Result<LaunchboxRefreshStats> {
    let prepared = prepare_launchbox_refresh(xml_path, on_progress)?;
    apply_launchbox_refresh(conn, prepared)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::external_metadata::EXTERNAL_METADATA_DB_FILE;

    /// Minimal fixture — two games on Nintendo Entertainment System with one
    /// alternate name. Deliberately one row per `(Name, Platform)` so the
    /// keys collide deterministically with `normalize_title`.
    const FIXTURE_XML: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<LaunchBox>
  <Game>
    <Name>Super Mario Bros.</Name>
    <DatabaseID>1</DatabaseID>
    <Platform>Nintendo Entertainment System</Platform>
    <Overview>The first Mario.</Overview>
    <CommunityRating>4.5</CommunityRating>
    <CommunityRatingCount>1234</CommunityRatingCount>
    <Developer>Nintendo</Developer>
    <Publisher>Nintendo</Publisher>
    <Genres>Platform</Genres>
    <MaxPlayers>2</MaxPlayers>
    <ReleaseDate>1985-09-13T00:00:00-05:00</ReleaseDate>
    <Cooperative>false</Cooperative>
  </Game>
  <Game>
    <Name>Empty Game</Name>
    <DatabaseID>2</DatabaseID>
    <Platform>Nintendo Entertainment System</Platform>
  </Game>
  <Game>
    <Name>The Legend of Zelda</Name>
    <DatabaseID>3</DatabaseID>
    <Platform>Nintendo Entertainment System</Platform>
    <Overview>Action adventure.</Overview>
    <Developer>Nintendo</Developer>
    <Genres>Action / Adventure</Genres>
    <MaxPlayers>1</MaxPlayers>
  </Game>
  <GameAlternateName>
    <DatabaseID>3</DatabaseID>
    <AlternateName>Zelda no Densetsu</AlternateName>
    <Region>Japan</Region>
  </GameAlternateName>
</LaunchBox>
"#;

    fn open_em_for_test(db_path: &Path) -> Connection {
        let conn = external_metadata::open_at(db_path).unwrap();
        conn.execute_batch("PRAGMA synchronous=FULL;").unwrap();
        conn
    }

    #[test]
    fn refresh_writes_rows_and_alternates_and_stamps_crc32() {
        let tmp = tempfile::tempdir().unwrap();
        let xml_path = tmp.path().join("launchbox-metadata.xml");
        std::fs::write(&xml_path, FIXTURE_XML).unwrap();
        let db_path = tmp.path().join(EXTERNAL_METADATA_DB_FILE);
        let mut conn = open_em_for_test(&db_path);

        let stats = refresh_launchbox(&xml_path, &mut conn, |_| {}).unwrap();
        assert_eq!(stats.source_entries, 3, "all 3 game elements seen");
        assert_eq!(
            stats.games_written, 2,
            "Empty Game (no useful fields) is dropped"
        );
        assert_eq!(
            stats.alternates_written, 1,
            "alternate attached to Zelda's row"
        );

        let stamped: Option<String> =
            external_metadata::read_meta(&conn, meta_keys::LAUNCHBOX_XML_CRC32);
        let expected = external_metadata::hash_file_crc32(&xml_path).unwrap();
        assert_eq!(stamped.as_deref(), Some(expected.as_str()));

        let mario_genre: Option<String> = conn
            .query_row(
                "SELECT genre FROM provider_game
                 WHERE provider = ?1 AND system = 'nintendo_nes' AND normalized_title = ?2",
                params![LAUNCHBOX_PROVIDER, normalize_title("Super Mario Bros.")],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(mario_genre.as_deref(), Some("Platform"));

        let alt_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM provider_alternate
                 WHERE provider = ?1 AND system = 'nintendo_nes' AND alternate_name = 'Zelda no Densetsu'",
                params![LAUNCHBOX_PROVIDER],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(alt_count, 1);
    }

    #[test]
    fn readers_return_normalized_title_keyed_rows() {
        let tmp = tempfile::tempdir().unwrap();
        let xml_path = tmp.path().join("launchbox-metadata.xml");
        std::fs::write(&xml_path, FIXTURE_XML).unwrap();
        let db_path = tmp.path().join(EXTERNAL_METADATA_DB_FILE);
        let mut conn = open_em_for_test(&db_path);
        refresh_launchbox(&xml_path, &mut conn, |_| {}).unwrap();

        let rows = external_metadata::system_launchbox_rows(&conn, "nintendo_nes").unwrap();
        assert_eq!(rows.len(), 2, "Mario + Zelda; Empty Game dropped");

        let mario = rows
            .get(&normalize_title("Super Mario Bros."))
            .expect("Mario row");
        assert_eq!(mario.developer.as_deref(), Some("Nintendo"));
        assert_eq!(mario.players, Some(2));
        assert_eq!(mario.rating, Some(4.5));
        assert_eq!(mario.rating_count, Some(1234));
        assert_eq!(mario.release_date.as_deref(), Some("1985-09-13"));
        assert_eq!(
            mario.release_precision,
            Some(replay_control_core::DatePrecision::Day),
        );
        assert!(!mario.cooperative);

        let alts = external_metadata::system_launchbox_alternates(&conn, "nintendo_nes").unwrap();
        assert_eq!(alts.len(), 1);
        assert_eq!(alts[0].0, normalize_title("The Legend of Zelda"));
        assert_eq!(alts[0].1, "Zelda no Densetsu");
        assert_eq!(alts[0].2, normalize_title("Zelda no Densetsu"));
    }

    #[test]
    fn refresh_is_idempotent_and_replaces_prior_rows() {
        let tmp = tempfile::tempdir().unwrap();
        let xml_path = tmp.path().join("launchbox-metadata.xml");
        std::fs::write(&xml_path, FIXTURE_XML).unwrap();
        let db_path = tmp.path().join(EXTERNAL_METADATA_DB_FILE);
        let mut conn = open_em_for_test(&db_path);

        let _first = refresh_launchbox(&xml_path, &mut conn, |_| {}).unwrap();
        let second = refresh_launchbox(&xml_path, &mut conn, |_| {}).unwrap();
        assert_eq!(second.games_written, 2);

        // Replace the XML with a single-game one. Refresh should drop
        // previously-imported rows.
        std::fs::write(
            &xml_path,
            r#"<?xml version="1.0" encoding="utf-8"?>
<LaunchBox>
  <Game>
    <Name>New Game</Name>
    <DatabaseID>99</DatabaseID>
    <Platform>Nintendo Entertainment System</Platform>
    <Overview>Brand new.</Overview>
  </Game>
</LaunchBox>
"#,
        )
        .unwrap();
        let third = refresh_launchbox(&xml_path, &mut conn, |_| {}).unwrap();
        assert_eq!(third.games_written, 1);

        let total: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM provider_game WHERE provider = ?1",
                params![LAUNCHBOX_PROVIDER],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(total, 1, "old rows wiped on refresh");
    }
}
