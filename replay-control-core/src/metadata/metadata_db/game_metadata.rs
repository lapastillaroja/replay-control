//! Operations on the `game_metadata` table.

use rusqlite::{OptionalExtension, params};

use crate::error::{Error, Result};

use super::{GameMetadata, MetadataDb, MetadataStats};

impl MetadataDb {
    /// Look up cached metadata for a specific game.
    pub fn lookup(&self, system: &str, rom_filename: &str) -> Result<Option<GameMetadata>> {
        let result = self
            .conn
            .query_row(
                "SELECT description, rating, publisher, developer, genre, players, release_year,
                        cooperative, source, fetched_at, box_art_path, screenshot_path
                 FROM game_metadata WHERE system = ?1 AND rom_filename = ?2",
                params![system, rom_filename],
                |row| {
                    Ok(GameMetadata {
                        description: row.get(0)?,
                        rating: row.get(1)?,
                        publisher: row.get(2)?,
                        developer: row.get(3)?,
                        genre: row.get(4)?,
                        players: row.get::<_, Option<i32>>(5)?.map(|p| p as u8),
                        release_year: row.get::<_, Option<i32>>(6)?.map(|y| y as u16),
                        cooperative: row.get::<_, i32>(7)? != 0,
                        source: row.get(8)?,
                        fetched_at: row.get(9)?,
                        box_art_path: row.get(10)?,
                        screenshot_path: row.get(11)?,
                    })
                },
            )
            .optional()
            .map_err(|e| Error::Other(format!("Metadata lookup failed: {e}")))?;
        Ok(result)
    }

    /// Fetch all box art paths for a system in one query.
    /// Returns a map of rom_filename → box_art_path for entries that have one.
    pub fn system_box_art_paths(
        &self,
        system: &str,
    ) -> Result<std::collections::HashMap<String, String>> {
        use std::collections::HashMap;

        let mut stmt = self
            .conn
            .prepare(
                "SELECT rom_filename, box_art_path FROM game_metadata
                 WHERE system = ?1 AND box_art_path IS NOT NULL",
            )
            .map_err(|e| Error::Other(format!("Prepare system box art lookup: {e}")))?;

        let rows = stmt
            .query_map(params![system], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| Error::Other(format!("System box art lookup: {e}")))?;

        let mut map = HashMap::new();
        for row in rows.flatten() {
            map.insert(row.0, row.1);
        }

        Ok(map)
    }

    /// Batch look up ratings for a list of ROMs on a single system.
    /// Returns a map of rom_filename -> rating for those that have a rating.
    pub fn lookup_ratings(
        &self,
        system: &str,
        rom_filenames: &[&str],
    ) -> Result<std::collections::HashMap<String, f64>> {
        use std::collections::HashMap;

        if rom_filenames.is_empty() {
            return Ok(HashMap::new());
        }

        let mut map = HashMap::new();
        let mut stmt = self
            .conn
            .prepare(
                "SELECT rom_filename, rating FROM game_metadata
                 WHERE system = ?1 AND rom_filename = ?2 AND rating IS NOT NULL",
            )
            .map_err(|e| Error::Other(format!("Prepare batch rating lookup: {e}")))?;

        for filename in rom_filenames {
            if let Some((name, rating)) = stmt
                .query_row(params![system, filename], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
                })
                .optional()
                .map_err(|e| Error::Other(format!("Batch rating lookup: {e}")))?
            {
                map.insert(name, rating);
            }
        }

        Ok(map)
    }

    /// Fetch all ratings for a single system in one query.
    /// Returns a map of rom_filename -> rating for entries with a non-null rating.
    /// More efficient than `lookup_ratings()` when filtering all ROMs in a system.
    pub fn system_ratings(&self, system: &str) -> Result<std::collections::HashMap<String, f64>> {
        use std::collections::HashMap;

        let mut stmt = self
            .conn
            .prepare(
                "SELECT rom_filename, rating FROM game_metadata
                 WHERE system = ?1 AND rating IS NOT NULL",
            )
            .map_err(|e| Error::Other(format!("Prepare system ratings query: {e}")))?;

        let rows = stmt
            .query_map(params![system], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })
            .map_err(|e| Error::Other(format!("System ratings query: {e}")))?;

        let mut map = HashMap::new();
        for row in rows.flatten() {
            map.insert(row.0, row.1);
        }
        Ok(map)
    }

    /// Fetch all non-empty genres from `game_metadata` for a single system.
    /// Returns a map of `rom_filename -> genre`.
    /// Used to fill empty `game_library.genre` entries during enrichment.
    pub fn system_metadata_genres(
        &self,
        system: &str,
    ) -> Result<std::collections::HashMap<String, String>> {
        use std::collections::HashMap;

        let mut stmt = self
            .conn
            .prepare(
                "SELECT rom_filename, genre FROM game_metadata
                 WHERE system = ?1 AND genre IS NOT NULL AND genre != ''",
            )
            .map_err(|e| Error::Other(format!("Prepare system_metadata_genres: {e}")))?;

        let rows = stmt
            .query_map(params![system], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| Error::Other(format!("System metadata genres query: {e}")))?;

        let mut map = HashMap::new();
        for row in rows.flatten() {
            map.insert(row.0, row.1);
        }
        Ok(map)
    }

    /// Fetch all non-null player counts from `game_metadata` for a single system.
    /// Returns a map of `rom_filename -> players`.
    /// Used to fill empty `game_library.players` entries during enrichment.
    pub fn system_metadata_players(
        &self,
        system: &str,
    ) -> Result<std::collections::HashMap<String, u8>> {
        use std::collections::HashMap;

        let mut stmt = self
            .conn
            .prepare(
                "SELECT rom_filename, players FROM game_metadata
                 WHERE system = ?1 AND players IS NOT NULL",
            )
            .map_err(|e| Error::Other(format!("Prepare system_metadata_players: {e}")))?;

        let rows = stmt
            .query_map(params![system], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i32>(1)? as u8))
            })
            .map_err(|e| Error::Other(format!("System metadata players query: {e}")))?;

        let mut map = HashMap::new();
        for row in rows.flatten() {
            map.insert(row.0, row.1);
        }
        Ok(map)
    }

    /// Fetch all non-null release years from `game_metadata` for a single system.
    /// Returns a map of `rom_filename -> release_year`.
    /// Used to fill empty release year entries during enrichment.
    pub fn system_metadata_release_years(
        &self,
        system: &str,
    ) -> Result<std::collections::HashMap<String, u16>> {
        use std::collections::HashMap;

        let mut stmt = self
            .conn
            .prepare(
                "SELECT rom_filename, release_year FROM game_metadata
                 WHERE system = ?1 AND release_year IS NOT NULL",
            )
            .map_err(|e| Error::Other(format!("Prepare system_metadata_release_years: {e}")))?;

        let rows = stmt
            .query_map(params![system], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i32>(1)? as u16))
            })
            .map_err(|e| Error::Other(format!("System metadata release_years query: {e}")))?;

        let mut map = HashMap::new();
        for row in rows.flatten() {
            map.insert(row.0, row.1);
        }
        Ok(map)
    }

    /// Fetch all non-empty developers from `game_metadata` for a single system.
    /// Returns a map of `rom_filename -> developer`.
    /// Used to fill empty developer entries during enrichment.
    pub fn system_metadata_developers(
        &self,
        system: &str,
    ) -> Result<std::collections::HashMap<String, String>> {
        use std::collections::HashMap;

        let mut stmt = self
            .conn
            .prepare(
                "SELECT rom_filename, developer FROM game_metadata
                 WHERE system = ?1 AND developer IS NOT NULL AND developer != ''",
            )
            .map_err(|e| Error::Other(format!("Prepare system_metadata_developers: {e}")))?;

        let rows = stmt
            .query_map(params![system], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| Error::Other(format!("System metadata developers query: {e}")))?;

        let mut map = HashMap::new();
        for row in rows.flatten() {
            map.insert(row.0, row.1);
        }
        Ok(map)
    }

    /// Fetch all metadata entries for a system.
    /// Returns a vec of `(rom_filename, GameMetadata)`.
    /// Used for normalized-title matching when enriching new ROMs.
    pub fn system_metadata_all(&self, system: &str) -> Result<Vec<(String, GameMetadata)>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT rom_filename, description, rating, publisher, developer, genre, players,
                        release_year, cooperative, source, fetched_at, box_art_path, screenshot_path
                 FROM game_metadata WHERE system = ?1",
            )
            .map_err(|e| Error::Other(format!("Prepare system_metadata_all: {e}")))?;

        let rows = stmt
            .query_map(params![system], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    GameMetadata {
                        description: row.get(1)?,
                        rating: row.get(2)?,
                        publisher: row.get(3)?,
                        developer: row.get(4)?,
                        genre: row.get(5)?,
                        players: row.get::<_, Option<i32>>(6)?.map(|p| p as u8),
                        release_year: row.get::<_, Option<i32>>(7)?.map(|y| y as u16),
                        cooperative: row.get::<_, i32>(8)? != 0,
                        source: row.get(9)?,
                        fetched_at: row.get(10)?,
                        box_art_path: row.get(11)?,
                        screenshot_path: row.get(12)?,
                    },
                ))
            })
            .map_err(|e| Error::Other(format!("Query system_metadata_all: {e}")))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| Error::Other(format!("Row read failed: {e}")))?);
        }
        Ok(result)
    }

    /// Insert or update metadata for a game.
    pub fn upsert(&self, system: &str, rom_filename: &str, meta: &GameMetadata) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO game_metadata (system, rom_filename, description, rating, publisher, developer,
                    genre, players, release_year, cooperative, source, fetched_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                 ON CONFLICT(system, rom_filename) DO UPDATE SET
                    description = excluded.description,
                    rating = excluded.rating,
                    publisher = excluded.publisher,
                    developer = excluded.developer,
                    genre = excluded.genre,
                    players = excluded.players,
                    release_year = excluded.release_year,
                    cooperative = excluded.cooperative,
                    source = excluded.source,
                    fetched_at = excluded.fetched_at",
                params![
                    system,
                    rom_filename,
                    meta.description,
                    meta.rating,
                    meta.publisher,
                    meta.developer,
                    meta.genre,
                    meta.players.map(|p| p as i32),
                    meta.release_year.map(|y| y as i32),
                    meta.cooperative as i32,
                    meta.source,
                    meta.fetched_at,
                ],
            )
            .map_err(|e| Error::Other(format!("Metadata upsert failed: {e}")))?;
        Ok(())
    }

    /// Bulk insert/update metadata within a single transaction.
    pub fn bulk_upsert(&mut self, entries: &[(String, String, GameMetadata)]) -> Result<usize> {
        let tx = self
            .conn
            .transaction()
            .map_err(|e| Error::Other(format!("Transaction start failed: {e}")))?;

        let mut count = 0usize;
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO game_metadata (system, rom_filename, description, rating, publisher, developer,
                        genre, players, release_year, cooperative, source, fetched_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                     ON CONFLICT(system, rom_filename) DO UPDATE SET
                        description = excluded.description,
                        rating = excluded.rating,
                        publisher = excluded.publisher,
                        developer = excluded.developer,
                        genre = excluded.genre,
                        players = excluded.players,
                        release_year = excluded.release_year,
                        cooperative = excluded.cooperative,
                        source = excluded.source,
                        fetched_at = excluded.fetched_at",
                )
                .map_err(|e| Error::Other(format!("Prepare failed: {e}")))?;

            for (system, rom_filename, meta) in entries {
                stmt.execute(params![
                    system,
                    rom_filename,
                    meta.description,
                    meta.rating,
                    meta.publisher,
                    meta.developer,
                    meta.genre,
                    meta.players.map(|p| p as i32),
                    meta.release_year.map(|y| y as i32),
                    meta.cooperative as i32,
                    meta.source,
                    meta.fetched_at,
                ])
                .map_err(|e| Error::Other(format!("Bulk upsert failed: {e}")))?;
                count += 1;
            }
        }

        tx.commit()
            .map_err(|e| Error::Other(format!("Transaction commit failed: {e}")))?;
        Ok(count)
    }

    /// Get coverage statistics.
    pub fn stats(&self) -> Result<MetadataStats> {
        let total_entries: usize = self
            .conn
            .query_row("SELECT COUNT(*) FROM game_metadata", [], |row| row.get(0))
            .map_err(|e| Error::Other(format!("Stats query failed: {e}")))?;

        let with_description: usize = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM game_metadata WHERE description IS NOT NULL AND description != ''",
                [],
                |row| row.get(0),
            )
            .map_err(|e| Error::Other(format!("Stats query failed: {e}")))?;

        let with_rating: usize = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM game_metadata WHERE rating IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .map_err(|e| Error::Other(format!("Stats query failed: {e}")))?;

        let db_size_bytes = std::fs::metadata(&self.db_path)
            .map(|m| m.len())
            .unwrap_or(0);

        let last_updated_text = self
            .conn
            .query_row(
                "SELECT imported_at FROM data_sources WHERE source_name = 'launchbox'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .ok()
            .map(|ts| {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;
                let diff = now - ts;
                if diff < 60 {
                    "just now".to_string()
                } else if diff < 3600 {
                    format!("{}m ago", diff / 60)
                } else if diff < 86400 {
                    format!("{}h ago", diff / 3600)
                } else {
                    format!("{}d ago", diff / 86400)
                }
            })
            .unwrap_or_default();

        Ok(MetadataStats {
            total_entries,
            with_description,
            with_rating,
            db_size_bytes,
            last_updated_text,
        })
    }

    /// Get all ratings as a map of `(system, rom_filename) -> rating`.
    pub fn all_ratings(&self) -> Result<std::collections::HashMap<(String, String), f64>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT system, rom_filename, rating FROM game_metadata WHERE rating IS NOT NULL",
            )
            .map_err(|e| Error::Other(format!("Query failed: {e}")))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, f64>(2)?,
                ))
            })
            .map_err(|e| Error::Other(format!("Query failed: {e}")))?;
        let mut map = std::collections::HashMap::new();
        for row in rows.flatten() {
            map.insert((row.0, row.1), row.2);
        }
        Ok(map)
    }

    /// Delete all cached metadata.
    pub fn clear(&self) -> Result<()> {
        self.conn
            .execute("DELETE FROM game_metadata", [])
            .map_err(|e| Error::Other(format!("Clear failed: {e}")))?;
        self.conn
            .execute("VACUUM", [])
            .map_err(|e| Error::Other(format!("Vacuum failed: {e}")))?;
        Ok(())
    }

    /// Check if the database has any entries.
    pub fn is_empty(&self) -> Result<bool> {
        let count: usize = self
            .conn
            .query_row("SELECT COUNT(*) FROM game_metadata", [], |row| row.get(0))
            .map_err(|e| Error::Other(format!("Count query failed: {e}")))?;
        Ok(count == 0)
    }

    /// Count metadata entries *with descriptions* per system, ordered by count descending.
    ///
    /// Only counts rows where `description IS NOT NULL AND description != ''` so that
    /// thumbnail-only metadata rows (created by `bulk_update_image_paths`) are excluded.
    ///
    /// Uses a LEFT JOIN with game_library for M3U dedup: when game_library is populated
    /// for a system, only entries matching game_library are counted (disc files
    /// referenced by .m3u playlists are excluded). When game_library is empty for a
    /// system (e.g. library not yet warmed after import), all game_metadata entries
    /// are counted as a fallback to avoid showing 0.
    pub fn entries_per_system(&self) -> Result<Vec<(String, usize)>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT gm.system, COUNT(*) as cnt
                 FROM game_metadata gm
                 LEFT JOIN game_library gl ON gm.system = gl.system AND gm.rom_filename = gl.rom_filename
                 WHERE gm.description IS NOT NULL AND gm.description != ''
                   AND (gl.rom_filename IS NOT NULL
                        OR NOT EXISTS (SELECT 1 FROM game_library gl2 WHERE gl2.system = gm.system))
                 GROUP BY gm.system ORDER BY cnt DESC",
            )
            .map_err(|e| Error::Other(format!("Query failed: {e}")))?;

        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, usize>(1)?))
            })
            .map_err(|e| Error::Other(format!("Query failed: {e}")))?;

        let mut result = Vec::new();
        for row in rows.flatten() {
            result.push(row);
        }
        Ok(result)
    }

    /// Bulk update image paths for games within a single transaction.
    /// Each entry is `(system, rom_filename, box_art_path, screenshot_path)`.
    pub fn bulk_update_image_paths(
        &mut self,
        entries: &[(String, String, Option<String>, Option<String>)],
    ) -> Result<usize> {
        let tx = self
            .conn
            .transaction()
            .map_err(|e| Error::Other(format!("Transaction start failed: {e}")))?;

        let mut count = 0usize;
        {
            let mut stmt = tx
                .prepare(
                    "UPDATE game_metadata SET box_art_path = ?3, screenshot_path = ?4
                     WHERE system = ?1 AND rom_filename = ?2",
                )
                .map_err(|e| Error::Other(format!("Prepare failed: {e}")))?;

            let mut insert_stmt = tx
                .prepare(
                    "INSERT OR IGNORE INTO game_metadata (system, rom_filename, source, fetched_at, box_art_path, screenshot_path)
                     VALUES (?1, ?2, 'thumbnails', ?3, ?4, ?5)",
                )
                .map_err(|e| Error::Other(format!("Prepare insert failed: {e}")))?;

            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;

            for (system, rom_filename, box_art, screenshot) in entries {
                let updated = stmt
                    .execute(params![system, rom_filename, box_art, screenshot])
                    .map_err(|e| Error::Other(format!("Image path update failed: {e}")))?;
                if updated == 0 {
                    insert_stmt
                        .execute(params![system, rom_filename, now, box_art, screenshot])
                        .map_err(|e| Error::Other(format!("Image path insert failed: {e}")))?;
                }
                count += 1;
            }
        }

        tx.commit()
            .map_err(|e| Error::Other(format!("Transaction commit failed: {e}")))?;
        Ok(count)
    }

    /// Clear image paths for a specific system in the DB.
    pub fn clear_system_image_paths(&self, system: &str) -> Result<usize> {
        let count = self
            .conn
            .execute(
                "UPDATE game_metadata SET box_art_path = NULL, screenshot_path = NULL WHERE system = ?1",
                params![system],
            )
            .map_err(|e| Error::Other(format!("Clear image paths failed: {e}")))?;
        Ok(count)
    }

    /// Count entries that have image paths.
    ///
    /// Only counts `game_metadata` rows that have a matching entry in `game_library`,
    /// so orphaned metadata rows (for ROMs that have been deleted) are excluded.
    /// Falls back to counting all `game_metadata` rows when `game_library` is empty.
    pub fn image_stats(&self) -> Result<(usize, usize)> {
        let with_boxart: usize = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM game_metadata gm
                 LEFT JOIN game_library gl ON gm.system = gl.system AND gm.rom_filename = gl.rom_filename
                 WHERE gm.box_art_path IS NOT NULL
                   AND (gl.rom_filename IS NOT NULL
                        OR NOT EXISTS (SELECT 1 FROM game_library gl2 WHERE gl2.system = gm.system))",
                [],
                |row| row.get(0),
            )
            .map_err(|e| Error::Other(format!("Image stats query failed: {e}")))?;
        let with_screenshot: usize = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM game_metadata gm
                 LEFT JOIN game_library gl ON gm.system = gl.system AND gm.rom_filename = gl.rom_filename
                 WHERE gm.screenshot_path IS NOT NULL
                   AND (gl.rom_filename IS NOT NULL
                        OR NOT EXISTS (SELECT 1 FROM game_library gl2 WHERE gl2.system = gm.system))",
                [],
                |row| row.get(0),
            )
            .map_err(|e| Error::Other(format!("Image stats query failed: {e}")))?;
        Ok((with_boxart, with_screenshot))
    }

    /// Delete `game_metadata` rows where the ROM no longer exists in `game_library`.
    ///
    /// Only deletes for systems that have entries in `game_library` (i.e., the library
    /// has been populated). Returns the number of deleted rows.
    pub fn delete_orphaned_metadata(&self) -> Result<usize> {
        let count = self
            .conn
            .execute(
                "DELETE FROM game_metadata
                 WHERE EXISTS (SELECT 1 FROM game_library gl2 WHERE gl2.system = game_metadata.system)
                   AND (system, rom_filename) NOT IN (
                       SELECT system, rom_filename FROM game_library
                   )",
                [],
            )
            .map_err(|e| Error::Other(format!("Delete orphaned metadata failed: {e}")))?;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::*;

    #[test]
    fn entries_per_system_no_game_library_returns_all() {
        let (mut db, _dir) = open_temp_db();
        db.bulk_upsert(&[
            (
                "sega_smd".into(),
                "Sonic.md".into(),
                make_metadata_with_desc("Sonic game", None),
            ),
            (
                "sega_smd".into(),
                "Streets.md".into(),
                make_metadata_with_desc("Streets game", None),
            ),
            (
                "snes".into(),
                "Mario.sfc".into(),
                make_metadata_with_desc("Mario game", None),
            ),
        ])
        .unwrap();

        let counts = db.entries_per_system().unwrap();
        assert_eq!(counts.len(), 2);
        assert_eq!(counts[0], ("sega_smd".into(), 2));
        assert_eq!(counts[1], ("snes".into(), 1));
    }

    #[test]
    fn entries_per_system_with_game_library_deduplicates_m3u() {
        let (mut db, _dir) = open_temp_db();

        db.bulk_upsert(&[
            (
                "sega_cd".into(),
                "Game.m3u".into(),
                make_metadata_with_desc("Game desc", None),
            ),
            (
                "sega_cd".into(),
                "Game (Disc 1).cue".into(),
                make_metadata_with_desc("Game disc 1", None),
            ),
            (
                "sega_cd".into(),
                "Game (Disc 2).cue".into(),
                make_metadata_with_desc("Game disc 2", None),
            ),
            (
                "snes".into(),
                "Mario.sfc".into(),
                make_metadata_with_desc("Mario desc", None),
            ),
        ])
        .unwrap();

        db.save_system_entries(
            "sega_cd",
            &[make_game_entry("sega_cd", "Game.m3u", true)],
            None,
        )
        .unwrap();
        db.save_system_entries("snes", &[make_game_entry("snes", "Mario.sfc", false)], None)
            .unwrap();

        let counts = db.entries_per_system().unwrap();
        let sega_cd = counts.iter().find(|(s, _)| s == "sega_cd").unwrap();
        let snes = counts.iter().find(|(s, _)| s == "snes").unwrap();

        assert_eq!(sega_cd.1, 1);
        assert_eq!(snes.1, 1);
    }

    #[test]
    fn entries_per_system_mixed_cached_and_uncached_systems() {
        let (mut db, _dir) = open_temp_db();

        db.bulk_upsert(&[
            (
                "sega_cd".into(),
                "Game.m3u".into(),
                make_metadata_with_desc("Game desc", None),
            ),
            (
                "sega_cd".into(),
                "Game (Disc 1).cue".into(),
                make_metadata_with_desc("Disc 1 desc", None),
            ),
            (
                "snes".into(),
                "Mario.sfc".into(),
                make_metadata_with_desc("Mario desc", None),
            ),
            (
                "snes".into(),
                "Zelda.sfc".into(),
                make_metadata_with_desc("Zelda desc", None),
            ),
        ])
        .unwrap();

        db.save_system_entries(
            "sega_cd",
            &[make_game_entry("sega_cd", "Game.m3u", true)],
            None,
        )
        .unwrap();

        let counts = db.entries_per_system().unwrap();
        let sega_cd = counts.iter().find(|(s, _)| s == "sega_cd").unwrap();
        let snes = counts.iter().find(|(s, _)| s == "snes").unwrap();

        assert_eq!(sega_cd.1, 1);
        assert_eq!(snes.1, 2);
    }

    /// Thumbnail-only metadata rows (no description) should not be counted.
    /// This was the root cause of description coverage >100%.
    #[test]
    fn entries_per_system_excludes_thumbnail_only_rows() {
        let (mut db, _dir) = open_temp_db();

        // Insert metadata with descriptions for 2 games.
        db.bulk_upsert(&[
            (
                "snes".into(),
                "Mario.sfc".into(),
                make_metadata_with_desc("Mario game", None),
            ),
            (
                "snes".into(),
                "Zelda.sfc".into(),
                make_metadata_with_desc("Zelda game", None),
            ),
        ])
        .unwrap();

        // Insert a thumbnail-only row (no description) via bulk_update_image_paths.
        // This simulates what happens when thumbnails are downloaded for a game
        // that has no LaunchBox metadata entry.
        db.bulk_update_image_paths(&[(
            "snes".into(),
            "Metroid.sfc".into(),
            Some("boxart/Metroid.png".into()),
            None,
        )])
        .unwrap();

        // Populate game_library with 3 games.
        db.save_system_entries(
            "snes",
            &[
                make_game_entry("snes", "Mario.sfc", false),
                make_game_entry("snes", "Zelda.sfc", false),
                make_game_entry("snes", "Metroid.sfc", false),
            ],
            None,
        )
        .unwrap();

        // entries_per_system should only count the 2 games with descriptions,
        // not the thumbnail-only Metroid row.
        let counts = db.entries_per_system().unwrap();
        assert_eq!(counts.len(), 1);
        assert_eq!(counts[0], ("snes".into(), 2));
    }

    /// Entries with no description should not be counted even without game_library.
    #[test]
    fn entries_per_system_no_description_not_counted() {
        let (mut db, _dir) = open_temp_db();

        db.bulk_upsert(&[
            (
                "snes".into(),
                "Mario.sfc".into(),
                make_metadata_with_desc("Mario game", None),
            ),
            // This entry has rating but no description.
            ("snes".into(), "Zelda.sfc".into(), make_metadata(None)),
        ])
        .unwrap();

        let counts = db.entries_per_system().unwrap();
        assert_eq!(counts.len(), 1);
        assert_eq!(counts[0], ("snes".into(), 1));
    }

    /// Empty string description should not be counted.
    #[test]
    fn entries_per_system_empty_description_not_counted() {
        let (mut db, _dir) = open_temp_db();

        db.bulk_upsert(&[
            (
                "snes".into(),
                "Mario.sfc".into(),
                make_metadata_with_desc("Mario game", None),
            ),
            (
                "snes".into(),
                "Zelda.sfc".into(),
                make_metadata_with_desc("", None),
            ),
        ])
        .unwrap();

        let counts = db.entries_per_system().unwrap();
        assert_eq!(counts.len(), 1);
        assert_eq!(counts[0], ("snes".into(), 1));
    }
}
