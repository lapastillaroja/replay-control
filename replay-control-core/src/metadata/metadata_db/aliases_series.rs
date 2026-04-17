//! Operations on the `game_alias` and `game_series` tables.

use rusqlite::{Connection, params};

use crate::error::{Error, Result};

use super::{AliasInsert, GameEntry, MetadataDb, SeriesInsert};

impl MetadataDb {
    /// Insert or update a game alias (cross-name variant).
    pub fn upsert_alias(
        conn: &Connection,
        system: &str,
        base_title: &str,
        alias_name: &str,
        alias_region: &str,
        source: &str,
    ) -> Result<()> {
        conn.execute(
                "INSERT OR REPLACE INTO game_alias (system, base_title, alias_name, alias_region, source)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![system, base_title, alias_name, alias_region, source],
            )
            .map_err(|e| Error::Other(format!("Upsert alias failed: {e}")))?;
        Ok(())
    }

    /// Bulk insert game aliases within a single transaction.
    pub fn bulk_insert_aliases(conn: &mut Connection, aliases: &[AliasInsert]) -> Result<usize> {
        if aliases.is_empty() {
            return Ok(0);
        }

        let tx = conn
            .transaction()
            .map_err(|e| Error::Other(format!("Transaction start failed: {e}")))?;

        let mut count = 0usize;
        {
            let mut stmt = tx
                .prepare(
                    "INSERT OR REPLACE INTO game_alias (system, base_title, alias_name, alias_region, source)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                )
                .map_err(|e| Error::Other(format!("Prepare bulk_insert_aliases: {e}")))?;

            for a in aliases {
                stmt.execute(params![
                    a.system,
                    a.base_title,
                    a.alias_name,
                    a.alias_region,
                    a.source
                ])
                .map_err(|e| Error::Other(format!("Insert alias failed: {e}")))?;
                count += 1;
            }
        }

        tx.commit()
            .map_err(|e| Error::Other(format!("Transaction commit failed: {e}")))?;
        Ok(count)
    }

    /// Get all alias base_titles for a game, for cross-name video sharing.
    ///
    /// Given `(system, base_title)`, returns all equivalent base_titles from
    /// `game_alias`: both aliases OF this game and canonical titles this game
    /// is an alias OF. Does not include the input `base_title` itself.
    pub fn alias_base_titles(conn: &Connection, system: &str, base_title: &str) -> Vec<String> {
        let mut titles = Vec::new();

        // 1. Aliases of this game: game_alias.alias_name WHERE base_title = ?
        if let Ok(mut stmt) = conn.prepare(
            "SELECT DISTINCT alias_name FROM game_alias
             WHERE system = ?1 AND base_title = ?2 COLLATE NOCASE",
        ) && let Ok(rows) = stmt.query_map(params![system, base_title], |row| row.get(0))
        {
            for name in rows.flatten() {
                let name: String = name;
                if name.to_lowercase() != base_title.to_lowercase() {
                    titles.push(name);
                }
            }
        }

        // 2. Canonical base_title that this game is an alias of
        if let Ok(mut stmt) = conn.prepare(
            "SELECT DISTINCT base_title FROM game_alias
             WHERE system = ?1 AND alias_name = ?2 COLLATE NOCASE",
        ) && let Ok(rows) = stmt.query_map(params![system, base_title], |row| row.get(0))
        {
            for bt in rows.flatten() {
                let bt: String = bt;
                if bt.to_lowercase() != base_title.to_lowercase()
                    && !titles.iter().any(|t| t.to_lowercase() == bt.to_lowercase())
                {
                    titles.push(bt);
                }
            }
        }

        titles
    }

    /// Clear all game aliases.
    pub fn clear_aliases(conn: &Connection) -> Result<()> {
        conn.execute("DELETE FROM game_alias", [])
            .map_err(|e| Error::Other(format!("Clear game_alias failed: {e}")))?;
        Ok(())
    }

    // ── Game Series (Wikidata-sourced franchise data) ────────────────────

    /// Bulk insert game series entries within a single transaction.
    pub fn bulk_insert_series(conn: &mut Connection, entries: &[SeriesInsert]) -> Result<usize> {
        if entries.is_empty() {
            return Ok(0);
        }

        let tx = conn
            .transaction()
            .map_err(|e| Error::Other(format!("Transaction start failed: {e}")))?;

        let mut count = 0usize;
        {
            let mut stmt = tx
                .prepare(
                    "INSERT OR REPLACE INTO game_series
                     (system, base_title, series_name, series_order, source, follows_base_title, followed_by_base_title)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                )
                .map_err(|e| Error::Other(format!("Prepare bulk_insert_series: {e}")))?;

            for s in entries {
                stmt.execute(params![
                    s.system,
                    s.base_title,
                    s.series_name,
                    s.series_order,
                    s.source,
                    s.follows_base_title,
                    s.followed_by_base_title
                ])
                .map_err(|e| Error::Other(format!("Insert series failed: {e}")))?;
                count += 1;
            }
        }

        tx.commit()
            .map_err(|e| Error::Other(format!("Transaction commit failed: {e}")))?;
        Ok(count)
    }

    /// Find series siblings using the `game_series` table (Wikidata data).
    ///
    /// Given a game's system and base_title, find other games in the SAME series
    /// across ALL systems. Uses the `game_series` table populated from Wikidata.
    /// Returns results ordered by series_order when available.
    pub fn wikidata_series_siblings(
        conn: &Connection,
        system: &str,
        base_title: &str,
        region_pref: &str,
        limit: usize,
    ) -> Result<Vec<(GameEntry, Option<i32>)>> {
        let mut stmt = conn
            .prepare(
                "WITH candidates AS (
                    SELECT ?2 AS bt
                    UNION SELECT base_title FROM game_alias
                        WHERE system = ?1 AND alias_name = ?2
                    UNION SELECT alias_name FROM game_alias
                        WHERE system = ?1 AND base_title = ?2
                    UNION SELECT ga2.alias_name FROM game_alias ga
                        JOIN game_alias ga2 ON ga2.system = ga.system
                            AND ga2.base_title = ga.base_title
                        WHERE ga.system = ?1 AND ga.alias_name = ?2
                ),
                current_series AS (
                    SELECT series_name FROM game_series
                    WHERE system = ?1 AND base_title IN (SELECT bt FROM candidates)
                    LIMIT 1
                ),
                series_games AS (
                    SELECT gs.system, gs.base_title, gs.series_order, gs.series_name
                    FROM game_series gs
                    JOIN current_series cs ON gs.series_name = cs.series_name
                ),
                deduped AS (
                    SELECT gl.*, sg.series_order,
                        ROW_NUMBER() OVER (
                            PARTITION BY gl.system, gl.base_title
                            ORDER BY CASE
                                WHEN gl.region = ?3 THEN 0
                                WHEN gl.region = 'world' THEN 1
                                ELSE 2
                            END
                        ) AS rn
                    FROM series_games sg
                    JOIN game_library gl ON gl.base_title = sg.base_title COLLATE NOCASE
                    WHERE gl.is_clone = 0
                      AND gl.is_translation = 0
                      AND gl.is_hack = 0
                      AND gl.is_special = 0
                      AND NOT (gl.system = ?1 AND gl.base_title = ?2 COLLATE NOCASE)
                )
                SELECT system, rom_filename, rom_path, display_name, base_title, series_key,
                        region, developer, genre, genre_group, rating, rating_count, players,
                        is_clone, is_m3u, is_translation, is_hack, is_special,
                        box_art_url, driver_status, size_bytes, crc32, hash_mtime, hash_matched_name,
                        release_year, series_order
                FROM deduped WHERE rn = 1
                ORDER BY series_order NULLS LAST, display_name
                LIMIT ?4",
            )
            .map_err(|e| Error::Other(format!("Prepare wikidata_series_siblings: {e}")))?;

        let rows = stmt
            .query_map(
                params![system, base_title, region_pref, limit as i64],
                |row| {
                    let entry = Self::row_to_game_entry(row)?;
                    let order: Option<i32> = row.get(25)?;
                    Ok((entry, order))
                },
            )
            .map_err(|e| Error::Other(format!("Query wikidata_series_siblings: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Check if a game (or any of its aliases) has Wikidata series data.
    pub fn has_wikidata_series(conn: &Connection, system: &str, base_title: &str) -> bool {
        conn.query_row(
            "WITH candidates AS (
                 SELECT ?2 AS bt
                 UNION SELECT base_title FROM game_alias
                     WHERE system = ?1 AND alias_name = ?2
                 UNION SELECT alias_name FROM game_alias
                     WHERE system = ?1 AND base_title = ?2
                 UNION SELECT ga2.alias_name FROM game_alias ga
                     JOIN game_alias ga2 ON ga2.system = ga.system
                         AND ga2.base_title = ga.base_title
                     WHERE ga.system = ?1 AND ga.alias_name = ?2
             )
             SELECT 1 FROM game_series
             WHERE system = ?1 AND base_title IN (SELECT bt FROM candidates)
             LIMIT 1",
            params![system, base_title],
            |_| Ok(()),
        )
        .is_ok()
    }

    /// Get the series name for a game (or any of its aliases).
    ///
    /// Direct matches on `base_title` are preferred over alias-resolved ones so
    /// callers get the same answer for an aliased input as they would for the
    /// canonical.
    pub fn lookup_series_name(conn: &Connection, system: &str, base_title: &str) -> Option<String> {
        conn.query_row(
            "WITH candidates AS (
                 SELECT ?2 AS bt
                 UNION SELECT base_title FROM game_alias
                     WHERE system = ?1 AND alias_name = ?2
                 UNION SELECT alias_name FROM game_alias
                     WHERE system = ?1 AND base_title = ?2
                 UNION SELECT ga2.alias_name FROM game_alias ga
                     JOIN game_alias ga2 ON ga2.system = ga.system
                         AND ga2.base_title = ga.base_title
                     WHERE ga.system = ?1 AND ga.alias_name = ?2
             )
             SELECT series_name FROM game_series
             WHERE system = ?1 AND base_title IN (SELECT bt FROM candidates)
             ORDER BY CASE WHEN base_title = ?2 THEN 0 ELSE 1 END
             LIMIT 1",
            params![system, base_title],
            |row| row.get::<_, String>(0),
        )
        .ok()
    }

    /// Get sequel/prequel chain info for a game.
    ///
    /// Returns the predecessor and successor titles, optional `GameEntry` for each
    /// (if the linked game is in the library), the current game's series position,
    /// and the max position in the series (for "N of M" display).
    ///
    /// Strategy: first try explicit P155/P156 sequel links (`follows_base_title`,
    /// `followed_by_base_title`). If none exist, fall back to ordinal-based prev/next
    /// using `series_order` (find games with order N-1 and N+1 in the same series).
    pub fn sequel_info(
        conn: &Connection,
        system: &str,
        base_title: &str,
        region_pref: &str,
    ) -> Result<SequelChainInfo> {
        // Step 1: Get series data for this game (alias-aware).
        let row = conn.query_row(
            "WITH candidates AS (
                 SELECT ?2 AS bt
                 UNION SELECT base_title FROM game_alias
                     WHERE system = ?1 AND alias_name = ?2
                 UNION SELECT alias_name FROM game_alias
                     WHERE system = ?1 AND base_title = ?2
                 UNION SELECT ga2.alias_name FROM game_alias ga
                     JOIN game_alias ga2 ON ga2.system = ga.system
                         AND ga2.base_title = ga.base_title
                     WHERE ga.system = ?1 AND ga.alias_name = ?2
             )
             SELECT follows_base_title, followed_by_base_title, series_order, series_name
             FROM game_series
             WHERE system = ?1 AND base_title IN (SELECT bt FROM candidates)
               AND series_name <> ''
             ORDER BY CASE WHEN base_title = ?2 THEN 0 ELSE 1 END
             LIMIT 1",
            params![system, base_title],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<i32>>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        );

        let (follows_title, followed_by_title, series_order, series_name) = match row {
            Ok(data) => data,
            Err(_) => return Ok(SequelChainInfo::default()),
        };

        // Treat empty strings as None (SQLite stores '' not NULL for unpopulated columns).
        let follows_title = follows_title.filter(|s| !s.is_empty());
        let followed_by_title = followed_by_title.filter(|s| !s.is_empty());
        let has_explicit_links = follows_title.is_some() || followed_by_title.is_some();

        // Step 2: Get max series_order for "N of M" position display.
        let series_max_order = if series_order.is_some() {
            conn.query_row(
                "SELECT MAX(series_order) FROM game_series
                     WHERE series_name = ?1 AND series_order IS NOT NULL",
                params![series_name],
                |row| row.get::<_, Option<i32>>(0),
            )
            .ok()
            .flatten()
        } else {
            None
        };

        if has_explicit_links {
            // Use explicit P155/P156 sequel links.
            let follows_entry = follows_title
                .as_ref()
                .and_then(|title| Self::find_best_rom(conn, title, system, region_pref));
            let followed_by_entry = followed_by_title
                .as_ref()
                .and_then(|title| Self::find_best_rom(conn, title, system, region_pref));

            Ok(SequelChainInfo {
                follows_title,
                follows_entry,
                followed_by_title,
                followed_by_entry,
                series_order,
                series_max_order,
            })
        } else if let Some(order) = series_order {
            // Fallback: synthesize prev/next from series_order (N-1, N+1).
            // Search across all systems for the adjacent ordinals.
            let prev =
                Self::find_series_neighbor(conn, &series_name, order - 1, system, region_pref);
            let next =
                Self::find_series_neighbor(conn, &series_name, order + 1, system, region_pref);

            if prev.is_none() && next.is_none() {
                return Ok(SequelChainInfo::default());
            }

            let (follows_title, follows_entry) = match prev {
                Some((title, entry)) => (Some(title), entry),
                None => (None, None),
            };
            let (followed_by_title, followed_by_entry) = match next {
                Some((title, entry)) => (Some(title), entry),
                None => (None, None),
            };

            Ok(SequelChainInfo {
                follows_title,
                follows_entry,
                followed_by_title,
                followed_by_entry,
                series_order: Some(order),
                series_max_order,
            })
        } else {
            Ok(SequelChainInfo::default())
        }
    }

    /// Find a game in the same series with a specific ordinal, for ordinal-based prev/next.
    ///
    /// Returns `Some((display_title, Option<GameEntry>))` — the title is always present,
    /// the GameEntry only if the game is in the user's library.
    fn find_series_neighbor(
        conn: &Connection,
        series_name: &str,
        target_order: i32,
        preferred_system: &str,
        region_pref: &str,
    ) -> Option<(String, Option<GameEntry>)> {
        if target_order < 1 {
            return None;
        }
        // Find a game_series entry with this ordinal, preferring the same system.
        let base_title: String = conn
            .query_row(
                "SELECT base_title FROM game_series
                 WHERE series_name = ?1 AND series_order = ?2
                 ORDER BY CASE WHEN system = ?3 THEN 0 ELSE 1 END
                 LIMIT 1",
                params![series_name, target_order, preferred_system],
                |row| row.get(0),
            )
            .ok()?;

        let entry = Self::find_best_rom(conn, &base_title, preferred_system, region_pref);
        // Use display_name from the entry if available, otherwise the base_title.
        let display = entry
            .as_ref()
            .and_then(|e| e.display_name.clone())
            .unwrap_or_else(|| base_title.clone());
        Some((display, entry))
    }

    /// Find the best ROM for a given base_title, preferring a specific system and region.
    fn find_best_rom(
        conn: &Connection,
        base_title: &str,
        preferred_system: &str,
        region_pref: &str,
    ) -> Option<GameEntry> {
        // Try exact base_title match, preferring non-clones.
        // Falls back to clones if no original exists (common for arcade games
        // where the Wikidata title matches a regional clone, not the parent ROM).
        conn.query_row(
            "SELECT system, rom_filename, rom_path, display_name, base_title, series_key,
                        region, developer, genre, genre_group, rating, rating_count, players,
                        is_clone, is_m3u, is_translation, is_hack, is_special,
                        box_art_url, driver_status, size_bytes, crc32, hash_mtime, hash_matched_name,
                        release_year
                 FROM game_library
                 WHERE base_title = ?1 COLLATE NOCASE
                   AND is_translation = 0 AND is_hack = 0 AND is_special = 0
                 ORDER BY
                     is_clone ASC,
                     CASE WHEN system = ?2 THEN 0 ELSE 1 END,
                     CASE WHEN region = ?3 THEN 0 WHEN region = 'world' THEN 1 ELSE 2 END
                 LIMIT 1",
            params![base_title, preferred_system, region_pref],
            Self::row_to_game_entry,
        )
        .ok()
    }

    /// Clear all game series data.
    pub fn clear_series(conn: &Connection) -> Result<()> {
        conn.execute("DELETE FROM game_series", [])
            .map_err(|e| Error::Other(format!("Clear game_series failed: {e}")))?;
        Ok(())
    }
}

/// Sequel/prequel chain info for a game.
#[derive(Debug, Clone, Default)]
pub struct SequelChainInfo {
    /// Title of the preceding game (for display). `None` if no predecessor.
    pub follows_title: Option<String>,
    /// The predecessor's library entry, if it exists in the user's library.
    pub follows_entry: Option<GameEntry>,
    /// Title of the following game (for display). `None` if no successor.
    pub followed_by_title: Option<String>,
    /// The successor's library entry, if it exists in the user's library.
    pub followed_by_entry: Option<GameEntry>,
    /// Current game's position in the series (P1545 ordinal).
    pub series_order: Option<i32>,
    /// Max position in the series (for "N of M" display).
    pub series_max_order: Option<i32>,
}

#[cfg(test)]
mod tests {
    use super::super::SeriesInsert;
    use super::super::tests::*;

    #[test]
    fn series_via_alias_finds_siblings() {
        let (mut conn, _dir) = open_temp_db();

        // "hoshi no kirby super deluxe" is an alias for "kirby super star".
        // Only "kirby super star" has a game_series entry.
        let mut kirby_jp = make_game_entry("nintendo_snes", "Hoshi no Kirby (J).sfc", false);
        kirby_jp.base_title = "hoshi no kirby super deluxe".into();
        let mut kirby_us = make_game_entry("nintendo_snes", "Kirby Super Star (USA).sfc", false);
        kirby_us.base_title = "kirby super star".into();
        let mut kirby3 = make_game_entry("nintendo_snes", "Kirby's Dream Land 3 (USA).sfc", false);
        kirby3.base_title = "kirby's dream land 3".into();

        super::super::MetadataDb::save_system_entries(
            &mut conn,
            "nintendo_snes",
            &[kirby_jp, kirby_us, kirby3],
            None,
        )
        .unwrap();

        // Insert alias: hoshi no kirby super deluxe -> kirby super star
        super::super::MetadataDb::upsert_alias(
            &conn,
            "nintendo_snes",
            "kirby super star",
            "hoshi no kirby super deluxe",
            "japan",
            "test",
        )
        .unwrap();

        // Insert series entries for the canonical titles only.
        super::super::MetadataDb::bulk_insert_series(
            &mut conn,
            &[
                SeriesInsert {
                    system: "nintendo_snes".into(),
                    base_title: "kirby super star".into(),
                    series_name: "Kirby".into(),
                    series_order: Some(1),
                    source: "wikidata".into(),
                    follows_base_title: None,
                    followed_by_base_title: None,
                },
                SeriesInsert {
                    system: "nintendo_snes".into(),
                    base_title: "kirby's dream land 3".into(),
                    series_name: "Kirby".into(),
                    series_order: Some(2),
                    source: "wikidata".into(),
                    follows_base_title: None,
                    followed_by_base_title: None,
                },
            ],
        )
        .unwrap();

        // Query siblings for the aliased title — should find Kirby series.
        let siblings = super::super::MetadataDb::wikidata_series_siblings(
            &conn,
            "nintendo_snes",
            "hoshi no kirby super deluxe",
            "usa",
            20,
        )
        .unwrap();

        let titles: Vec<&str> = siblings
            .iter()
            .map(|(e, _)| e.base_title.as_str())
            .collect();
        assert!(
            titles.contains(&"kirby's dream land 3"),
            "Should find series siblings via alias. Got: {titles:?}",
        );

        // has_wikidata_series should also work via alias.
        assert!(
            super::super::MetadataDb::has_wikidata_series(
                &conn,
                "nintendo_snes",
                "hoshi no kirby super deluxe"
            ),
            "has_wikidata_series should return true for aliased game"
        );

        // lookup_series_name should also work via alias.
        let series = super::super::MetadataDb::lookup_series_name(
            &conn,
            "nintendo_snes",
            "hoshi no kirby super deluxe",
        );
        assert_eq!(series.as_deref(), Some("Kirby"));
    }

    #[test]
    fn series_siblings_excludes_current_game() {
        let (mut conn, _dir) = open_temp_db();

        // Create game_library entries for Final Fight on two systems.
        let mut ff_arcade = make_game_entry("arcade_fbneo", "ffight.zip", false);
        ff_arcade.base_title = "final fight".into();
        let mut ff_snes = make_game_entry("nintendo_snes", "Final Fight (USA).sfc", false);
        ff_snes.base_title = "final fight".into();
        let mut ff2 = make_game_entry("nintendo_snes", "Final Fight 2 (USA).sfc", false);
        ff2.base_title = "final fight 2".into();

        super::super::MetadataDb::save_system_entries(
            &mut conn,
            "arcade_fbneo",
            &[ff_arcade],
            None,
        )
        .unwrap();
        super::super::MetadataDb::save_system_entries(
            &mut conn,
            "nintendo_snes",
            &[ff_snes, ff2],
            None,
        )
        .unwrap();

        // Populate game_series for all three games.
        super::super::MetadataDb::bulk_insert_series(
            &mut conn,
            &[
                SeriesInsert {
                    system: "arcade_fbneo".into(),
                    base_title: "final fight".into(),
                    series_name: "Final Fight".into(),
                    series_order: Some(1),
                    source: "wikidata".into(),
                    follows_base_title: None,
                    followed_by_base_title: None,
                },
                SeriesInsert {
                    system: "nintendo_snes".into(),
                    base_title: "final fight".into(),
                    series_name: "Final Fight".into(),
                    series_order: Some(1),
                    source: "wikidata".into(),
                    follows_base_title: None,
                    followed_by_base_title: None,
                },
                SeriesInsert {
                    system: "nintendo_snes".into(),
                    base_title: "final fight 2".into(),
                    series_name: "Final Fight".into(),
                    series_order: Some(2),
                    source: "wikidata".into(),
                    follows_base_title: None,
                    followed_by_base_title: None,
                },
            ],
        )
        .unwrap();

        // Query siblings for arcade Final Fight.
        let siblings = super::super::MetadataDb::wikidata_series_siblings(
            &conn,
            "arcade_fbneo",
            "final fight",
            "usa",
            20,
        )
        .unwrap();

        let sibling_titles: Vec<&str> = siblings
            .iter()
            .map(|(e, _)| e.base_title.as_str())
            .collect();

        // "final fight 2" should appear as a sibling.
        assert!(
            sibling_titles.contains(&"final fight 2"),
            "Final Fight 2 should be a series sibling. Got: {:?}",
            sibling_titles
        );

        // "final fight" on SNES should appear (same game, different system).
        let snes_ff = siblings
            .iter()
            .find(|(e, _)| e.base_title == "final fight" && e.system == "nintendo_snes");
        assert!(
            snes_ff.is_some(),
            "Final Fight on SNES should appear in arcade FF's series. Got: {:?}",
            sibling_titles
        );

        // "final fight" on arcade (the current game itself) must NOT appear.
        let arcade_ff = siblings
            .iter()
            .find(|(e, _)| e.base_title == "final fight" && e.system == "arcade_fbneo");
        assert!(
            arcade_ff.is_none(),
            "Current game (arcade final fight) must not appear in its own series. Got: {:?}",
            sibling_titles
        );
    }
}
