//! Operations on the `game_library` and `game_library_meta` tables.

use rusqlite::{OptionalExtension, params};

use crate::error::{Error, Result};

use super::{unix_now, GameEntry, MetadataDb, SystemMeta};

impl MetadataDb {
    /// Get all distinct `box_art_url` values from `game_library` for a given system.
    ///
    /// Returns the URL paths (e.g., `/media/sega_smd/boxart/Sonic.png`).
    pub fn active_box_art_urls(&self, system: &str) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT DISTINCT box_art_url FROM game_library
                 WHERE system = ?1 AND box_art_url IS NOT NULL",
            )
            .map_err(|e| Error::Other(format!("Query active_box_art_urls failed: {e}")))?;
        let rows = stmt
            .query_map(params![system], |row| row.get::<_, String>(0))
            .map_err(|e| Error::Other(format!("Query active_box_art_urls failed: {e}")))?;
        Ok(rows.flatten().collect())
    }

    /// Get all systems that have entries in `game_library`.
    pub fn active_systems(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT system FROM game_library")
            .map_err(|e| Error::Other(format!("Query active_systems failed: {e}")))?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| Error::Other(format!("Query active_systems failed: {e}")))?;
        Ok(rows.flatten().collect())
    }

    /// Count games with thumbnails per system from `game_library.box_art_url`.
    ///
    /// This is the live source of truth — rebuilt every enrichment pass.
    /// Returns `(system, count_with_box_art)` tuples.
    pub fn thumbnails_per_system(&self) -> Result<Vec<(String, usize)>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT system,
                        SUM(CASE WHEN box_art_url IS NOT NULL THEN 1 ELSE 0 END)
                 FROM game_library
                 GROUP BY system",
            )
            .map_err(|e| Error::Other(format!("Query failed: {e}")))?;

        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, usize>(1)?))
            })
            .map_err(|e| Error::Other(format!("Query failed: {e}")))?;

        Ok(rows.flatten().collect())
    }

    // ── Game Library (L2 persistent cache) ─────────────────────────────

    /// Save a system's game list to the game_library table.
    /// Replaces all existing entries for the system in a single transaction.
    pub fn save_system_entries(
        &mut self,
        system: &str,
        roms: &[GameEntry],
        dir_mtime_secs: Option<i64>,
    ) -> Result<()> {
        let tx = self
            .conn
            .transaction()
            .map_err(|e| Error::Other(format!("Transaction start failed: {e}")))?;

        // Delete existing entries for this system.
        tx.execute("DELETE FROM game_library WHERE system = ?1", params![system])
            .map_err(|e| Error::Other(format!("Delete game_library failed: {e}")))?;

        {
            let mut stmt = tx
                .prepare(
                    "INSERT OR IGNORE INTO game_library (system, rom_filename, rom_path, display_name,
                     size_bytes, is_m3u, box_art_url, driver_status, genre, genre_group, players, rating,
                     is_clone, base_title, region, is_translation, is_hack, is_special,
                     crc32, hash_mtime, hash_matched_name, series_key)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18,
                             ?19, ?20, ?21, ?22)",
                )
                .map_err(|e| Error::Other(format!("Prepare game_library insert: {e}")))?;

            for rom in roms {
                stmt.execute(params![
                    &rom.system,
                    &rom.rom_filename,
                    &rom.rom_path,
                    &rom.display_name,
                    rom.size_bytes as i64,
                    rom.is_m3u,
                    &rom.box_art_url,
                    &rom.driver_status,
                    &rom.genre,
                    &rom.genre_group,
                    rom.players.map(|p| p as i32),
                    rom.rating,
                    rom.is_clone,
                    &rom.base_title,
                    &rom.region,
                    rom.is_translation,
                    rom.is_hack,
                    rom.is_special,
                    rom.crc32.map(|c| c as i64),
                    rom.hash_mtime,
                    &rom.hash_matched_name,
                    &rom.series_key,
                ])
                .map_err(|e| Error::Other(format!("Insert game_library failed: {e}")))?;
            }
        }

        // Update system metadata.
        let total_size: u64 = roms.iter().map(|r| r.size_bytes).sum();
        let now = unix_now();
        tx.execute(
            "INSERT INTO game_library_meta (system, dir_mtime_secs, scanned_at, rom_count, total_size_bytes)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(system) DO UPDATE SET
                dir_mtime_secs = excluded.dir_mtime_secs,
                scanned_at = excluded.scanned_at,
                rom_count = excluded.rom_count,
                total_size_bytes = excluded.total_size_bytes",
            params![system, dir_mtime_secs, now, roms.len() as i64, total_size as i64],
        )
        .map_err(|e| Error::Other(format!("Upsert game_library_meta failed: {e}")))?;

        tx.commit()
            .map_err(|e| Error::Other(format!("Transaction commit failed: {e}")))?;

        Ok(())
    }

    /// Load all game entries for a system.
    pub fn load_system_entries(&self, system: &str) -> Result<Vec<GameEntry>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT system, rom_filename, rom_path, display_name, size_bytes,
                        is_m3u, box_art_url, driver_status, genre, genre_group, players, rating,
                        is_clone, base_title, region, is_translation, is_hack, is_special,
                        crc32, hash_mtime, hash_matched_name, series_key
                 FROM game_library WHERE system = ?1",
            )
            .map_err(|e| Error::Other(format!("Prepare load_system_entries: {e}")))?;

        let rows = stmt
            .query_map(params![system], Self::row_to_game_entry)
            .map_err(|e| Error::Other(format!("Query load_system_entries: {e}")))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| Error::Other(format!("Row read failed: {e}")))?);
        }
        Ok(result)
    }

    /// Save just the system-level metadata (counts, mtime) without replacing game entries.
    /// Used when we know game counts from scan_systems but haven't loaded entries yet.
    pub fn save_system_meta(
        &self,
        system: &str,
        dir_mtime_secs: Option<i64>,
        rom_count: usize,
        total_size_bytes: u64,
    ) -> Result<()> {
        let now = unix_now();
        self.conn
            .execute(
                "INSERT INTO game_library_meta (system, dir_mtime_secs, scanned_at, rom_count, total_size_bytes)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(system) DO UPDATE SET
                    dir_mtime_secs = excluded.dir_mtime_secs,
                    scanned_at = excluded.scanned_at,
                    rom_count = excluded.rom_count,
                    total_size_bytes = excluded.total_size_bytes",
                rusqlite::params![system, dir_mtime_secs, now, rom_count as i64, total_size_bytes as i64],
            )
            .map_err(|e| Error::Other(format!("Upsert game_library_meta: {e}")))?;
        Ok(())
    }

    /// Load library metadata for a single system.
    pub fn load_system_meta(&self, system: &str) -> Result<Option<SystemMeta>> {
        self.conn
            .query_row(
                "SELECT system, dir_mtime_secs, scanned_at, rom_count, total_size_bytes
                 FROM game_library_meta WHERE system = ?1",
                params![system],
                |row| {
                    Ok(SystemMeta {
                        system: row.get(0)?,
                        dir_mtime_secs: row.get(1)?,
                        scanned_at: row.get(2)?,
                        rom_count: row.get::<_, i64>(3)? as usize,
                        total_size_bytes: row.get::<_, i64>(4)? as u64,
                    })
                },
            )
            .optional()
            .map_err(|e| Error::Other(format!("Query load_system_meta: {e}")))
    }

    /// Load library metadata for all systems.
    pub fn load_all_system_meta(&self) -> Result<Vec<SystemMeta>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT system, dir_mtime_secs, scanned_at, rom_count, total_size_bytes
                 FROM game_library_meta",
            )
            .map_err(|e| Error::Other(format!("Prepare load_all_system_meta: {e}")))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(SystemMeta {
                    system: row.get(0)?,
                    dir_mtime_secs: row.get(1)?,
                    scanned_at: row.get(2)?,
                    rom_count: row.get::<_, i64>(3)? as usize,
                    total_size_bytes: row.get::<_, i64>(4)? as u64,
                })
            })
            .map_err(|e| Error::Other(format!("Query load_all_system_meta: {e}")))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| Error::Other(format!("Row read failed: {e}")))?);
        }
        Ok(result)
    }

    /// Load cached hash data for all ROMs of a system from the game_library table.
    ///
    /// Returns a map of rom_filename -> CachedHash for entries that have a
    /// non-NULL crc32 value. Used by `hash_and_identify()` to skip re-hashing
    /// files whose mtime hasn't changed.
    pub fn load_cached_hashes(
        &self,
        system: &str,
    ) -> Result<std::collections::HashMap<String, crate::rom_hash::CachedHash>> {
        use std::collections::HashMap;

        let mut stmt = self
            .conn
            .prepare(
                "SELECT rom_filename, crc32, hash_mtime, hash_matched_name
                 FROM game_library
                 WHERE system = ?1 AND crc32 IS NOT NULL",
            )
            .map_err(|e| Error::Other(format!("Prepare load_cached_hashes: {e}")))?;

        let rows = stmt
            .query_map(params![system], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    crate::rom_hash::CachedHash {
                        crc32: row.get::<_, i64>(1)? as u32,
                        hash_mtime: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
                        matched_name: row.get(3)?,
                    },
                ))
            })
            .map_err(|e| Error::Other(format!("Query load_cached_hashes: {e}")))?;

        let mut map = HashMap::new();
        for row in rows.flatten() {
            map.insert(row.0, row.1);
        }
        Ok(map)
    }

    /// Batch update enrichment fields (box_art_url, genre, players, rating, driver_status)
    /// for entries already in the game library.
    pub fn update_rom_enrichment(
        &mut self,
        system: &str,
        enrichments: &[(String, Option<String>, Option<String>, Option<u8>, Option<f32>, Option<String>)],
    ) -> Result<usize> {
        let tx = self
            .conn
            .transaction()
            .map_err(|e| Error::Other(format!("Transaction start failed: {e}")))?;

        let mut count = 0usize;
        {
            let mut stmt = tx
                .prepare(
                    "UPDATE game_library SET box_art_url = ?2, genre = ?3, genre_group = ?4,
                            players = ?5, rating = ?6, driver_status = ?7
                     WHERE system = ?8 AND rom_filename = ?1",
                )
                .map_err(|e| Error::Other(format!("Prepare update_rom_enrichment: {e}")))?;

            for (filename, box_art_url, genre, players, rating, driver_status) in enrichments {
                let genre_group = genre
                    .as_deref()
                    .map(crate::genre::normalize_genre)
                    .unwrap_or("");
                let updated = stmt
                    .execute(params![
                        filename,
                        box_art_url,
                        genre,
                        genre_group,
                        players.map(|p| p as i32),
                        rating,
                        driver_status,
                        system,
                    ])
                    .map_err(|e| Error::Other(format!("Update rom enrichment: {e}")))?;
                count += updated;
            }
        }

        tx.commit()
            .map_err(|e| Error::Other(format!("Transaction commit failed: {e}")))?;
        Ok(count)
    }

    /// Batch update box_art_url, genre, players, and rating for entries in game_library.
    /// Only updates non-None fields. Genre and players are only set when the existing
    /// value is NULL or empty (baked-in data is never overwritten).
    pub fn update_box_art_genre_rating(
        &mut self,
        system: &str,
        enrichments: &[(String, Option<String>, Option<String>, Option<u8>, Option<f32>)],
    ) -> Result<()> {
        let tx = self
            .conn
            .transaction()
            .map_err(|e| Error::Other(format!("Transaction start failed: {e}")))?;

        {
            let mut art_stmt = tx
                .prepare(
                    "UPDATE game_library SET box_art_url = ?2
                     WHERE system = ?3 AND rom_filename = ?1",
                )
                .map_err(|e| Error::Other(format!("Prepare box_art update: {e}")))?;

            let mut genre_stmt = tx
                .prepare(
                    "UPDATE game_library SET genre = ?2, genre_group = ?3
                     WHERE system = ?4 AND rom_filename = ?1
                       AND (genre IS NULL OR genre = '')",
                )
                .map_err(|e| Error::Other(format!("Prepare genre update: {e}")))?;

            let mut players_stmt = tx
                .prepare(
                    "UPDATE game_library SET players = ?2
                     WHERE system = ?3 AND rom_filename = ?1
                       AND players IS NULL",
                )
                .map_err(|e| Error::Other(format!("Prepare players update: {e}")))?;

            let mut rating_stmt = tx
                .prepare(
                    "UPDATE game_library SET rating = ?2
                     WHERE system = ?3 AND rom_filename = ?1",
                )
                .map_err(|e| Error::Other(format!("Prepare rating update: {e}")))?;

            for (filename, box_art_url, genre, players, rating) in enrichments {
                if let Some(url) = box_art_url {
                    art_stmt
                        .execute(params![filename, url, system])
                        .map_err(|e| Error::Other(format!("Update box_art_url: {e}")))?;
                }
                if let Some(g) = genre {
                    let gg = crate::genre::normalize_genre(g);
                    genre_stmt
                        .execute(params![filename, g, gg, system])
                        .map_err(|e| Error::Other(format!("Update genre: {e}")))?;
                }
                if let Some(p) = players {
                    players_stmt
                        .execute(params![filename, *p as i32, system])
                        .map_err(|e| Error::Other(format!("Update players: {e}")))?;
                }
                if let Some(r) = rating {
                    rating_stmt
                        .execute(params![filename, r, system])
                        .map_err(|e| Error::Other(format!("Update rating: {e}")))?;
                }
            }
        }

        tx.commit()
            .map_err(|e| Error::Other(format!("Transaction commit failed: {e}")))?;
        Ok(())
    }

    /// Clear the game_library and game_library_meta for a specific system.
    pub fn clear_system_game_library(&self, system: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM game_library WHERE system = ?1", params![system])
            .map_err(|e| Error::Other(format!("Clear system game_library: {e}")))?;
        self.conn
            .execute(
                "DELETE FROM game_library_meta WHERE system = ?1",
                params![system],
            )
            .map_err(|e| Error::Other(format!("Clear system game_library_meta: {e}")))?;
        Ok(())
    }

    /// Get filenames of visible games for a system (excludes disc files hidden by M3U dedup).
    pub fn visible_filenames(&self, system: &str) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT rom_filename FROM game_library WHERE system = ?1")
            .map_err(|e| Error::Other(format!("Query failed: {e}")))?;
        let rows = stmt
            .query_map(params![system], |row| row.get(0))
            .map_err(|e| Error::Other(format!("Query failed: {e}")))?;
        Ok(rows.flatten().collect())
    }

    /// Clear all game_library and game_library_meta entries.
    pub fn clear_all_game_library(&self) -> Result<()> {
        self.conn
            .execute("DELETE FROM game_library", [])
            .map_err(|e| Error::Other(format!("Clear game_library: {e}")))?;
        self.conn
            .execute("DELETE FROM game_library_meta", [])
            .map_err(|e| Error::Other(format!("Clear game_library_meta: {e}")))?;
        Ok(())
    }

    /// Fetch current genres from `game_library` for a single system.
    /// Returns a map of `rom_filename -> genre` (only entries with non-empty genre).
    /// Used during enrichment to know which ROMs already have a genre.
    pub fn system_rom_genres(
        &self,
        system: &str,
    ) -> Result<std::collections::HashMap<String, String>> {
        use std::collections::HashMap;

        let mut stmt = self
            .conn
            .prepare(
                "SELECT rom_filename, genre FROM game_library
                 WHERE system = ?1 AND genre IS NOT NULL AND genre != ''",
            )
            .map_err(|e| Error::Other(format!("Prepare system_rom_genres: {e}")))?;

        let rows = stmt
            .query_map(params![system], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| Error::Other(format!("System rom genres query: {e}")))?;

        let mut map = HashMap::new();
        for row in rows.flatten() {
            map.insert(row.0, row.1);
        }
        Ok(map)
    }

    /// Fetch current player counts from `game_library` for a single system.
    /// Returns the set of `rom_filename` values that already have a players value.
    /// Used during enrichment to know which ROMs already have player data.
    pub fn system_rom_players(
        &self,
        system: &str,
    ) -> Result<std::collections::HashSet<String>> {
        use std::collections::HashSet;

        let mut stmt = self
            .conn
            .prepare(
                "SELECT rom_filename FROM game_library
                 WHERE system = ?1 AND players IS NOT NULL",
            )
            .map_err(|e| Error::Other(format!("Prepare system_rom_players: {e}")))?;

        let rows = stmt
            .query_map(params![system], |row| row.get::<_, String>(0))
            .map_err(|e| Error::Other(format!("System rom players query: {e}")))?;

        let mut set = HashSet::new();
        for row in rows.flatten() {
            set.insert(row);
        }
        Ok(set)
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::*;

    #[test]
    fn genre_enrichment_fills_empty_genre_from_launchbox() {
        let (mut db, _dir) = open_temp_db();

        db.bulk_upsert(&[
            ("sega_smd".into(), "Sonic.md".into(), make_metadata_with_genre("Platform")),
        ]).unwrap();

        db.save_system_entries("sega_smd", &[
            make_game_entry("sega_smd", "Sonic.md", false),
        ], None).unwrap();

        db.update_box_art_genre_rating("sega_smd", &[
            ("Sonic.md".into(), None, Some("Platform".into()), None, None),
        ]).unwrap();

        let roms = db.load_system_entries("sega_smd").unwrap();
        assert_eq!(roms[0].genre.as_deref(), Some("Platform"));
    }

    #[test]
    fn genre_enrichment_does_not_overwrite_existing_genre() {
        let (mut db, _dir) = open_temp_db();

        db.save_system_entries("sega_smd", &[
            make_game_entry_with_genre("sega_smd", "Sonic.md", "Shooter"),
        ], None).unwrap();

        db.update_box_art_genre_rating("sega_smd", &[
            ("Sonic.md".into(), None, Some("Platform".into()), None, None),
        ]).unwrap();

        let roms = db.load_system_entries("sega_smd").unwrap();
        assert_eq!(roms[0].genre.as_deref(), Some("Shooter"));
    }

    #[test]
    fn genre_enrichment_mixed_empty_and_existing() {
        let (mut db, _dir) = open_temp_db();

        db.save_system_entries("sega_smd", &[
            make_game_entry_with_genre("sega_smd", "Sonic.md", "Shooter"),
            make_game_entry("sega_smd", "Streets.md", false),
            make_game_entry("sega_smd", "Columns.md", false),
        ], None).unwrap();

        db.update_box_art_genre_rating("sega_smd", &[
            ("Sonic.md".into(), None, Some("Platform".into()), None, None),
            ("Streets.md".into(), None, Some("Beat'em Up".into()), None, None),
        ]).unwrap();

        let roms = db.load_system_entries("sega_smd").unwrap();
        let sonic = roms.iter().find(|r| r.rom_filename == "Sonic.md").unwrap();
        let streets = roms.iter().find(|r| r.rom_filename == "Streets.md").unwrap();
        let columns = roms.iter().find(|r| r.rom_filename == "Columns.md").unwrap();

        assert_eq!(sonic.genre.as_deref(), Some("Shooter"));
        assert_eq!(streets.genre.as_deref(), Some("Beat'em Up"));
        assert_eq!(columns.genre, None);
    }

    #[test]
    fn thumbnails_per_system_counts_box_art_url() {
        let (mut db, _dir) = open_temp_db();

        let mut with_art = make_game_entry("snes", "Mario.sfc", false);
        with_art.box_art_url = Some("/img/mario.png".into());

        let without_art = make_game_entry("snes", "Zelda.sfc", false);

        db.save_system_entries("snes", &[with_art, without_art], None)
            .unwrap();

        let thumbs = db.thumbnails_per_system().unwrap();
        assert_eq!(thumbs.len(), 1);
        assert_eq!(thumbs[0], ("snes".into(), 1));
    }

    #[test]
    fn thumbnails_per_system_empty_library_returns_empty() {
        let (db, _dir) = open_temp_db();

        let thumbs = db.thumbnails_per_system().unwrap();
        assert!(thumbs.is_empty());
    }

    #[test]
    fn thumbnails_per_system_multiple_systems() {
        let (mut db, _dir) = open_temp_db();

        let mut snes_game = make_game_entry("snes", "Mario.sfc", false);
        snes_game.box_art_url = Some("/img/mario.png".into());

        let mut gba_game1 = make_game_entry("gba", "Metroid.gba", false);
        gba_game1.box_art_url = Some("/img/metroid.png".into());

        let mut gba_game2 = make_game_entry("gba", "Zelda.gba", false);
        gba_game2.box_art_url = Some("/img/zelda.png".into());

        let gba_game3 = make_game_entry("gba", "NoArt.gba", false);

        db.save_system_entries("snes", &[snes_game], None).unwrap();
        db.save_system_entries("gba", &[gba_game1, gba_game2, gba_game3], None)
            .unwrap();

        let thumbs = db.thumbnails_per_system().unwrap();
        let snes = thumbs.iter().find(|(s, _)| s == "snes").unwrap();
        let gba = thumbs.iter().find(|(s, _)| s == "gba").unwrap();

        assert_eq!(snes.1, 1);
        assert_eq!(gba.1, 2);
    }
}
