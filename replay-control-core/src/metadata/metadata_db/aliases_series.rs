//! Operations on the `game_alias` and `game_series` tables.

use rusqlite::params;

use crate::error::{Error, Result};

use super::{AliasInsert, GameEntry, MetadataDb, SeriesInsert};

impl MetadataDb {
    /// Insert or update a game alias (cross-name variant).
    pub fn upsert_alias(
        &self,
        system: &str,
        base_title: &str,
        alias_name: &str,
        alias_region: &str,
        source: &str,
    ) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO game_alias (system, base_title, alias_name, alias_region, source)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![system, base_title, alias_name, alias_region, source],
            )
            .map_err(|e| Error::Other(format!("Upsert alias failed: {e}")))?;
        Ok(())
    }

    /// Bulk insert game aliases within a single transaction.
    pub fn bulk_insert_aliases(&mut self, aliases: &[AliasInsert]) -> Result<usize> {
        if aliases.is_empty() {
            return Ok(0);
        }

        let tx = self
            .conn
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

    /// Clear all game aliases.
    pub fn clear_aliases(&self) -> Result<()> {
        self.conn
            .execute("DELETE FROM game_alias", [])
            .map_err(|e| Error::Other(format!("Clear game_alias failed: {e}")))?;
        Ok(())
    }

    // ── Game Series (Wikidata-sourced franchise data) ────────────────────

    /// Bulk insert game series entries within a single transaction.
    pub fn bulk_insert_series(&mut self, entries: &[SeriesInsert]) -> Result<usize> {
        if entries.is_empty() {
            return Ok(0);
        }

        let tx = self
            .conn
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
        &self,
        system: &str,
        base_title: &str,
        region_pref: &str,
        limit: usize,
    ) -> Result<Vec<(GameEntry, Option<i32>)>> {
        let mut stmt = self
            .conn
            .prepare(
                "WITH current_series AS (
                    SELECT series_name FROM game_series
                    WHERE system = ?1 AND base_title = ?2
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
                SELECT system, rom_filename, rom_path, display_name, size_bytes,
                        is_m3u, box_art_url, driver_status, genre, genre_group, players, rating,
                        is_clone, base_title, region, is_translation, is_hack, is_special,
                        crc32, hash_mtime, hash_matched_name, series_key, series_order
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
                    let order: Option<i32> = row.get(22)?;
                    Ok((entry, order))
                },
            )
            .map_err(|e| Error::Other(format!("Query wikidata_series_siblings: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Check if a game has Wikidata series data in the `game_series` table.
    pub fn has_wikidata_series(&self, system: &str, base_title: &str) -> bool {
        self.conn
            .query_row(
                "SELECT 1 FROM game_series WHERE system = ?1 AND base_title = ?2 LIMIT 1",
                params![system, base_title],
                |_| Ok(()),
            )
            .is_ok()
    }

    /// Get the series name for a game from the `game_series` table.
    pub fn lookup_series_name(&self, system: &str, base_title: &str) -> Option<String> {
        self.conn
            .query_row(
                "SELECT series_name FROM game_series WHERE system = ?1 AND base_title = ?2 LIMIT 1",
                params![system, base_title],
                |row| row.get(0),
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
        &self,
        system: &str,
        base_title: &str,
        region_pref: &str,
    ) -> Result<SequelChainInfo> {
        // Step 1: Get series data for this game.
        let row = self.conn.query_row(
            "SELECT follows_base_title, followed_by_base_title, series_order, series_name
             FROM game_series
             WHERE system = ?1 AND base_title = ?2
               AND series_name <> ''
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
            self.conn
                .query_row(
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
                .and_then(|title| self.find_best_rom(title, system, region_pref));
            let followed_by_entry = followed_by_title
                .as_ref()
                .and_then(|title| self.find_best_rom(title, system, region_pref));

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
            let prev = self.find_series_neighbor(&series_name, order - 1, system, region_pref);
            let next = self.find_series_neighbor(&series_name, order + 1, system, region_pref);

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
        &self,
        series_name: &str,
        target_order: i32,
        preferred_system: &str,
        region_pref: &str,
    ) -> Option<(String, Option<GameEntry>)> {
        if target_order < 1 {
            return None;
        }
        // Find a game_series entry with this ordinal, preferring the same system.
        let base_title: String = self
            .conn
            .query_row(
                "SELECT base_title FROM game_series
                 WHERE series_name = ?1 AND series_order = ?2
                 ORDER BY CASE WHEN system = ?3 THEN 0 ELSE 1 END
                 LIMIT 1",
                params![series_name, target_order, preferred_system],
                |row| row.get(0),
            )
            .ok()?;

        let entry = self.find_best_rom(&base_title, preferred_system, region_pref);
        // Use display_name from the entry if available, otherwise the base_title.
        let display = entry
            .as_ref()
            .and_then(|e| e.display_name.clone())
            .unwrap_or_else(|| base_title.clone());
        Some((display, entry))
    }

    /// Find the best ROM for a given base_title, preferring a specific system and region.
    fn find_best_rom(
        &self,
        base_title: &str,
        preferred_system: &str,
        region_pref: &str,
    ) -> Option<GameEntry> {
        self.conn
            .query_row(
                "SELECT system, rom_filename, rom_path, display_name, size_bytes,
                        is_m3u, box_art_url, driver_status, genre, genre_group, players, rating,
                        is_clone, base_title, region, is_translation, is_hack, is_special,
                        crc32, hash_mtime, hash_matched_name, series_key
                 FROM game_library
                 WHERE base_title = ?1 COLLATE NOCASE
                   AND is_clone = 0 AND is_translation = 0 AND is_hack = 0 AND is_special = 0
                 ORDER BY
                     CASE WHEN system = ?2 THEN 0 ELSE 1 END,
                     CASE WHEN region = ?3 THEN 0 WHEN region = 'world' THEN 1 ELSE 2 END
                 LIMIT 1",
                params![base_title, preferred_system, region_pref],
                Self::row_to_game_entry,
            )
            .ok()
    }

    /// Clear all game series data.
    pub fn clear_series(&self) -> Result<()> {
        self.conn
            .execute("DELETE FROM game_series", [])
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
    use super::super::tests::*;
    use super::super::SeriesInsert;

    #[test]
    fn series_siblings_excludes_current_game() {
        let (mut db, _dir) = open_temp_db();

        // Create game_library entries for Final Fight on two systems.
        let mut ff_arcade = make_game_entry("arcade_fbneo", "ffight.zip", false);
        ff_arcade.base_title = "final fight".into();
        let mut ff_snes = make_game_entry("nintendo_snes", "Final Fight (USA).sfc", false);
        ff_snes.base_title = "final fight".into();
        let mut ff2 = make_game_entry("nintendo_snes", "Final Fight 2 (USA).sfc", false);
        ff2.base_title = "final fight 2".into();

        db.save_system_entries("arcade_fbneo", &[ff_arcade], None)
            .unwrap();
        db.save_system_entries("nintendo_snes", &[ff_snes, ff2], None)
            .unwrap();

        // Populate game_series for all three games.
        db.bulk_insert_series(&[
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
        ])
        .unwrap();

        // Query siblings for arcade Final Fight.
        let siblings = db
            .wikidata_series_siblings("arcade_fbneo", "final fight", "usa", 20)
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
