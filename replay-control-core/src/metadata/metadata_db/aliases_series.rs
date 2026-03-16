//! Operations on the `game_alias` and `game_series` tables.

use rusqlite::params;

use crate::error::{Error, Result};

use super::{GameEntry, MetadataDb};

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
    pub fn bulk_insert_aliases(
        &mut self,
        aliases: &[(String, String, String, String, String)], // (system, base_title, alias_name, alias_region, source)
    ) -> Result<usize> {
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

            for (system, base_title, alias_name, alias_region, source) in aliases {
                stmt.execute(params![system, base_title, alias_name, alias_region, source])
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
    pub fn bulk_insert_series(
        &mut self,
        entries: &[(String, String, String, Option<i32>, String)], // (system, base_title, series_name, series_order, source)
    ) -> Result<usize> {
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
                    "INSERT OR REPLACE INTO game_series (system, base_title, series_name, series_order, source)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                )
                .map_err(|e| Error::Other(format!("Prepare bulk_insert_series: {e}")))?;

            for (system, base_title, series_name, series_order, source) in entries {
                stmt.execute(params![system, base_title, series_name, series_order, source])
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
                    WHERE NOT (gs.system = ?1 AND gs.base_title = ?2)
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
                    JOIN game_library gl ON gl.system = sg.system AND gl.base_title = sg.base_title
                    WHERE gl.is_clone = 0
                      AND gl.is_translation = 0
                      AND gl.is_hack = 0
                      AND gl.is_special = 0
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

    /// Clear all game series data.
    pub fn clear_series(&self) -> Result<()> {
        self.conn
            .execute("DELETE FROM game_series", [])
            .map_err(|e| Error::Other(format!("Clear game_series failed: {e}")))?;
        Ok(())
    }
}
