//! Operations on the `game_library` and `game_library_meta` tables.

use rusqlite::{OptionalExtension, params};

use crate::error::{Error, Result};

use super::{GameEntry, MetadataDb, SystemMeta, unix_now};

/// Helper for building dynamic SQL WHERE clauses with parameterized values.
///
/// Collects clause fragments and their associated parameter values together,
/// so you never have to manually track parameter indices across multiple queries.
struct WhereBuilder {
    clauses: Vec<String>,
    params: Vec<Box<dyn rusqlite::types::ToSql>>,
}

impl WhereBuilder {
    fn new() -> Self {
        Self {
            clauses: Vec::new(),
            params: Vec::new(),
        }
    }

    /// Add a WHERE clause fragment with a parameterized value.
    /// Use `{}` as the placeholder -- it will be replaced with the correct `?N` index
    /// at build time.
    fn add(&mut self, clause_template: &str, param: impl rusqlite::types::ToSql + 'static) {
        self.clauses.push(clause_template.to_string());
        self.params.push(Box::new(param));
    }

    /// Add a clause without a parameter (e.g., `is_clone = 0`).
    fn add_static(&mut self, clause: &str) {
        self.clauses.push(clause.to_string());
    }

    /// Build the WHERE string (joined with AND) and a reference slice for binding.
    ///
    /// Parameter indices start at `base_index` (e.g., if base_index=3, the first
    /// parameterized clause gets `?3`, the next `?4`, etc.).
    ///
    /// Each parameterized clause template must contain exactly one `{}` placeholder,
    /// which is replaced with `?N`. Static clauses (added via `add_static`) are
    /// included as-is.
    fn build(&self, base_index: usize) -> (String, Vec<&dyn rusqlite::types::ToSql>) {
        let mut idx = base_index;
        let mut parts = Vec::with_capacity(self.clauses.len());
        let mut param_refs: Vec<&dyn rusqlite::types::ToSql> = Vec::new();
        let mut param_pos = 0;

        for clause in &self.clauses {
            if clause.contains("{}") {
                parts.push(clause.replacen("{}", &format!("?{idx}"), 1));
                param_refs.push(self.params[param_pos].as_ref());
                param_pos += 1;
                idx += 1;
            } else {
                parts.push(clause.clone());
            }
        }

        (parts.join(" AND "), param_refs)
    }
}

/// Filter options for the developer games paginated query.
#[derive(Debug, Default)]
pub struct DeveloperGamesFilter<'a> {
    pub hide_hacks: bool,
    pub hide_translations: bool,
    pub hide_clones: bool,
    pub multiplayer_only: bool,
    pub genre: &'a str,
    pub min_rating: Option<f64>,
}

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
        tx.execute(
            "DELETE FROM game_library WHERE system = ?1",
            params![system],
        )
        .map_err(|e| Error::Other(format!("Delete game_library failed: {e}")))?;

        {
            let mut stmt = tx
                .prepare(
                    "INSERT OR IGNORE INTO game_library (system, rom_filename, rom_path, display_name,
                     base_title, series_key, region, developer, genre, genre_group, rating, rating_count, players,
                     is_clone, is_m3u, is_translation, is_hack, is_special,
                     box_art_url, driver_status, size_bytes, crc32, hash_mtime, hash_matched_name)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18,
                             ?19, ?20, ?21, ?22, ?23, ?24)",
                )
                .map_err(|e| Error::Other(format!("Prepare game_library insert: {e}")))?;

            for rom in roms {
                stmt.execute(params![
                    &rom.system,
                    &rom.rom_filename,
                    &rom.rom_path,
                    &rom.display_name,
                    &rom.base_title,
                    &rom.series_key,
                    &rom.region,
                    &rom.developer,
                    &rom.genre,
                    &rom.genre_group,
                    rom.rating,
                    rom.rating_count.map(|c| c as i64),
                    rom.players.map(|p| p as i32),
                    rom.is_clone,
                    rom.is_m3u,
                    rom.is_translation,
                    rom.is_hack,
                    rom.is_special,
                    &rom.box_art_url,
                    &rom.driver_status,
                    rom.size_bytes as i64,
                    rom.crc32.map(|c| c as i64),
                    rom.hash_mtime,
                    &rom.hash_matched_name,
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
                "SELECT system, rom_filename, rom_path, display_name, base_title, series_key,
                        region, developer, genre, genre_group, rating, rating_count, players,
                        is_clone, is_m3u, is_translation, is_hack, is_special,
                        box_art_url, driver_status, size_bytes, crc32, hash_mtime, hash_matched_name
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
        enrichments: &[super::RomEnrichment],
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

            for e in enrichments {
                let genre_group = e
                    .genre
                    .as_deref()
                    .map(crate::genre::normalize_genre)
                    .unwrap_or("");
                let updated = stmt
                    .execute(params![
                        e.rom_filename,
                        e.box_art_url,
                        e.genre,
                        genre_group,
                        e.players.map(|p| p as i32),
                        e.rating,
                        e.driver_status,
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
        enrichments: &[super::BoxArtGenreRating],
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

            let mut rating_count_stmt = tx
                .prepare(
                    "UPDATE game_library SET rating_count = ?2
                     WHERE system = ?3 AND rom_filename = ?1",
                )
                .map_err(|e| Error::Other(format!("Prepare rating_count update: {e}")))?;

            for e in enrichments {
                if let Some(ref url) = e.box_art_url {
                    art_stmt
                        .execute(params![e.rom_filename, url, system])
                        .map_err(|e| Error::Other(format!("Update box_art_url: {e}")))?;
                }
                if let Some(ref g) = e.genre {
                    let gg = crate::genre::normalize_genre(g);
                    genre_stmt
                        .execute(params![e.rom_filename, g, gg, system])
                        .map_err(|e| Error::Other(format!("Update genre: {e}")))?;
                }
                if let Some(p) = e.players {
                    players_stmt
                        .execute(params![e.rom_filename, p as i32, system])
                        .map_err(|e| Error::Other(format!("Update players: {e}")))?;
                }
                if let Some(r) = e.rating {
                    rating_stmt
                        .execute(params![e.rom_filename, r, system])
                        .map_err(|e| Error::Other(format!("Update rating: {e}")))?;
                }
                if let Some(c) = e.rating_count {
                    rating_count_stmt
                        .execute(params![e.rom_filename, c as i64, system])
                        .map_err(|e| Error::Other(format!("Update rating_count: {e}")))?;
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
            .execute(
                "DELETE FROM game_library WHERE system = ?1",
                params![system],
            )
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

    /// Fetch current developers from `game_library` for a single system.
    /// Returns a set of `rom_filename` values that already have a non-empty developer.
    /// Used during enrichment to know which ROMs already have developer data.
    pub fn system_rom_developers(&self, system: &str) -> Result<std::collections::HashSet<String>> {
        use std::collections::HashSet;

        let mut stmt = self
            .conn
            .prepare(
                "SELECT rom_filename FROM game_library
                 WHERE system = ?1 AND developer != ''",
            )
            .map_err(|e| Error::Other(format!("Prepare system_rom_developers: {e}")))?;

        let rows = stmt
            .query_map(params![system], |row| row.get::<_, String>(0))
            .map_err(|e| Error::Other(format!("System rom developers query: {e}")))?;

        let mut set = HashSet::new();
        for row in rows.flatten() {
            set.insert(row);
        }
        Ok(set)
    }

    /// Batch update `developer` for entries in game_library.
    /// Only updates entries where the existing developer is empty.
    pub fn update_developers(
        &mut self,
        system: &str,
        developers: &[(String, String)],
    ) -> Result<()> {
        let tx = self
            .conn
            .transaction()
            .map_err(|e| Error::Other(format!("Transaction start failed: {e}")))?;

        {
            let mut stmt = tx
                .prepare(
                    "UPDATE game_library SET developer = ?2
                     WHERE system = ?3 AND rom_filename = ?1
                       AND developer = ''",
                )
                .map_err(|e| Error::Other(format!("Prepare developer update: {e}")))?;

            for (filename, developer) in developers {
                stmt.execute(params![filename, developer, system])
                    .map_err(|e| Error::Other(format!("Update developer: {e}")))?;
            }
        }

        tx.commit()
            .map_err(|e| Error::Other(format!("Transaction commit failed: {e}")))?;
        Ok(())
    }

    /// Fetch current player counts from `game_library` for a single system.
    /// Returns the set of `rom_filename` values that already have a players value.
    /// Used during enrichment to know which ROMs already have player data.
    pub fn system_rom_players(&self, system: &str) -> Result<std::collections::HashSet<String>> {
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

    /// Find developer names that match the given query (case-insensitive).
    /// Returns up to 3 matches as `(developer_name, game_count)` tuples,
    /// ranked by match quality (exact > word-boundary > substring) then by
    /// game count descending.
    pub fn find_developer_matches(&self, query: &str) -> Result<Vec<(String, usize)>> {
        let q = query.to_lowercase();
        let mut stmt = self
            .conn
            .prepare(
                "SELECT developer, COUNT(DISTINCT base_title) as game_count
                 FROM game_library
                 WHERE developer != '' AND LOWER(developer) LIKE '%' || LOWER(?1) || '%'
                 GROUP BY developer
                 ORDER BY
                     CASE WHEN LOWER(developer) = LOWER(?1) THEN 0
                          WHEN LOWER(developer) LIKE LOWER(?1) || ' %'
                            OR LOWER(developer) LIKE '% ' || LOWER(?1) THEN 1
                          ELSE 2
                     END,
                     COUNT(DISTINCT base_title) DESC
                 LIMIT 3",
            )
            .map_err(|e| Error::Other(format!("Prepare find_developer_matches: {e}")))?;

        let rows = stmt
            .query_map(params![q], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, usize>(1)?))
            })
            .map_err(|e| Error::Other(format!("Query find_developer_matches: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Get games by a specific developer, preferring those with box art.
    /// Deduplicates by base_title across all systems (one ROM per game title)
    /// and filters out clones, translations, hacks, and specials.
    /// `region_pref` / `region_secondary` control which regional variant is kept.
    /// Within each base_title, prefers entries with box art and the user's region.
    pub fn games_by_developer(
        &self,
        developer: &str,
        limit: usize,
        region_pref: &str,
        region_secondary: &str,
    ) -> Result<Vec<GameEntry>> {
        let mut stmt = self
            .conn
            .prepare(
                "WITH deduped AS (
                    SELECT *, ROW_NUMBER() OVER (
                        PARTITION BY base_title
                        ORDER BY
                            box_art_url IS NULL,
                            CASE
                                WHEN region = ?2 THEN 0
                                WHEN region = ?3 THEN 1
                                WHEN region = 'world' THEN 2
                                ELSE 3
                            END
                    ) AS rn
                    FROM game_library
                    WHERE developer = ?1
                      AND is_clone = 0
                      AND is_translation = 0
                      AND is_hack = 0
                      AND is_special = 0
                      AND base_title != ''
                )
                SELECT system, rom_filename, rom_path, display_name, base_title, series_key,
                        region, developer, genre, genre_group, rating, rating_count, players,
                        is_clone, is_m3u, is_translation, is_hack, is_special,
                        box_art_url, driver_status, size_bytes, crc32, hash_mtime, hash_matched_name
                FROM deduped WHERE rn = 1
                ORDER BY box_art_url IS NULL, RANDOM()
                LIMIT ?4",
            )
            .map_err(|e| Error::Other(format!("Prepare games_by_developer: {e}")))?;

        let rows = stmt
            .query_map(
                params![developer, region_pref, region_secondary, limit as i64],
                Self::row_to_game_entry,
            )
            .map_err(|e| Error::Other(format!("Query games_by_developer: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Paginated game list for a developer, optionally filtered by system and content filters.
    /// Deduplicates by base_title per system, with configurable filtering.
    /// Region preference controls which variant is kept.
    /// Returns `(entries, has_more, total_count)`.
    #[allow(clippy::too_many_arguments)]
    pub fn developer_games_paginated(
        &self,
        developer: &str,
        system_filter: Option<&str>,
        offset: usize,
        limit: usize,
        region_pref: &str,
        region_secondary: &str,
        filters: &DeveloperGamesFilter,
    ) -> Result<(Vec<GameEntry>, bool, usize)> {
        // Build shared WHERE clause from filters using WhereBuilder.
        let mut wb = WhereBuilder::new();
        wb.add("developer = {}", developer.to_string());
        wb.add_static("is_special = 0");
        if filters.hide_hacks {
            wb.add_static("is_hack = 0");
        }
        if filters.hide_translations {
            wb.add_static("is_translation = 0");
        }
        if filters.hide_clones {
            wb.add_static("is_clone = 0");
        }
        if filters.multiplayer_only {
            wb.add_static("players >= 2");
        }
        if !filters.genre.is_empty() {
            wb.add("genre_group = {}", filters.genre.to_string());
        }
        if let Some(mr) = filters.min_rating {
            wb.add("rating >= {}", mr);
        }
        if let Some(sys) = system_filter.filter(|s| !s.is_empty()) {
            wb.add("system = {}", sys.to_string());
        }

        // ── Count query (lightweight, for "X / Y games" UI display) ──
        let (count_where, count_refs) = wb.build(1);
        let count_sql = format!(
            "SELECT COUNT(*) FROM (
                SELECT DISTINCT system || '/' || base_title
                FROM game_library
                WHERE {count_where}
            )"
        );
        let total: usize = self
            .conn
            .query_row(&count_sql, count_refs.as_slice(), |row| row.get(0))
            .map_err(|e| Error::Other(format!("Count developer_games_paginated: {e}")))?;

        // ── Fetch query ──
        // Fetch limit+1 rows to determine has_more without a separate query.
        // Layout: ?1=region_pref, ?2=region_secondary, ?3=fetch_limit, ?4=offset,
        //         ?5..?N = WhereBuilder params (developer + optional filters).
        let fetch_limit = limit + 1;
        let (fetch_where, filter_refs) = wb.build(5);
        let fetch_sql = format!(
            "WITH deduped AS (
                SELECT *, ROW_NUMBER() OVER (
                    PARTITION BY system, base_title
                    ORDER BY CASE
                        WHEN region = ?1 THEN 0
                        WHEN region = ?2 THEN 1
                        WHEN region = 'world' THEN 2
                        ELSE 3
                    END
                ) AS rn
                FROM game_library
                WHERE {fetch_where}
            )
            SELECT system, rom_filename, rom_path, display_name, base_title, series_key,
                    region, developer, genre, genre_group, rating, rating_count, players,
                    is_clone, is_m3u, is_translation, is_hack, is_special,
                    box_art_url, driver_status, size_bytes, crc32, hash_mtime, hash_matched_name
            FROM deduped WHERE rn = 1
            ORDER BY display_name COLLATE NOCASE
            LIMIT ?3 OFFSET ?4"
        );

        // Assemble fetch params: fixed params first, then WhereBuilder params.
        let region_pref_box: Box<dyn rusqlite::types::ToSql> = Box::new(region_pref.to_string());
        let region_secondary_box: Box<dyn rusqlite::types::ToSql> = Box::new(region_secondary.to_string());
        let limit_box: Box<dyn rusqlite::types::ToSql> = Box::new(fetch_limit as i64);
        let offset_box: Box<dyn rusqlite::types::ToSql> = Box::new(offset as i64);

        let mut fetch_refs: Vec<&dyn rusqlite::types::ToSql> = vec![
            region_pref_box.as_ref(),
            region_secondary_box.as_ref(),
            limit_box.as_ref(),
            offset_box.as_ref(),
        ];
        fetch_refs.extend(filter_refs);

        let mut stmt = self
            .conn
            .prepare(&fetch_sql)
            .map_err(|e| Error::Other(format!("Prepare developer_games_paginated: {e}")))?;
        let rows = stmt
            .query_map(fetch_refs.as_slice(), Self::row_to_game_entry)
            .map_err(|e| Error::Other(format!("Query developer_games_paginated: {e}")))?;
        let mut entries: Vec<GameEntry> = rows.flatten().collect();

        // If we got the extra row, there are more results.
        let has_more = entries.len() > limit;
        entries.truncate(limit);

        Ok((entries, has_more, total))
    }

    /// Get distinct genre groups for a developer's games, optionally filtered by system.
    pub fn developer_genre_groups(
        &self,
        developer: &str,
        system_filter: Option<&str>,
    ) -> Result<Vec<String>> {
        let has_system = system_filter.is_some_and(|s| !s.is_empty());

        if has_system {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT DISTINCT genre_group FROM game_library
                     WHERE developer = ?1 AND genre_group != '' AND system = ?2
                     ORDER BY genre_group",
                )
                .map_err(|e| Error::Other(format!("Prepare developer_genre_groups: {e}")))?;
            let rows = stmt
                .query_map(params![developer, system_filter.unwrap()], |row| {
                    row.get::<_, String>(0)
                })
                .map_err(|e| Error::Other(format!("Query developer_genre_groups: {e}")))?;
            Ok(rows.flatten().collect())
        } else {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT DISTINCT genre_group FROM game_library
                     WHERE developer = ?1 AND genre_group != ''
                     ORDER BY genre_group",
                )
                .map_err(|e| Error::Other(format!("Prepare developer_genre_groups: {e}")))?;
            let rows = stmt
                .query_map(params![developer], |row| row.get::<_, String>(0))
                .map_err(|e| Error::Other(format!("Query developer_genre_groups: {e}")))?;
            Ok(rows.flatten().collect())
        }
    }

    /// Get systems where a developer has games, with display names and game counts.
    /// Returns `(system_folder, game_count)` sorted by count descending.
    pub fn developer_systems(&self, developer: &str) -> Result<Vec<(String, usize)>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT system, COUNT(DISTINCT base_title) as cnt
                 FROM game_library
                 WHERE developer = ?1
                   AND is_clone = 0
                   AND is_translation = 0
                   AND is_hack = 0
                   AND is_special = 0
                 GROUP BY system
                 ORDER BY cnt DESC",
            )
            .map_err(|e| Error::Other(format!("Prepare developer_systems: {e}")))?;

        let rows = stmt
            .query_map(params![developer], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, usize>(1)?))
            })
            .map_err(|e| Error::Other(format!("Query developer_systems: {e}")))?;

        Ok(rows.flatten().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::*;

    #[test]
    fn genre_enrichment_fills_empty_genre_from_launchbox() {
        let (mut db, _dir) = open_temp_db();

        db.bulk_upsert(&[(
            "sega_smd".into(),
            "Sonic.md".into(),
            make_metadata_with_genre("Platform"),
        )])
        .unwrap();

        db.save_system_entries(
            "sega_smd",
            &[make_game_entry("sega_smd", "Sonic.md", false)],
            None,
        )
        .unwrap();

        db.update_box_art_genre_rating(
            "sega_smd",
            &[super::super::BoxArtGenreRating {
                rom_filename: "Sonic.md".into(),
                box_art_url: None,
                genre: Some("Platform".into()),
                players: None,
                rating: None,
                rating_count: None,
            }],
        )
        .unwrap();

        let roms = db.load_system_entries("sega_smd").unwrap();
        assert_eq!(roms[0].genre.as_deref(), Some("Platform"));
    }

    #[test]
    fn genre_enrichment_does_not_overwrite_existing_genre() {
        let (mut db, _dir) = open_temp_db();

        db.save_system_entries(
            "sega_smd",
            &[make_game_entry_with_genre(
                "sega_smd", "Sonic.md", "Shooter",
            )],
            None,
        )
        .unwrap();

        db.update_box_art_genre_rating(
            "sega_smd",
            &[super::super::BoxArtGenreRating {
                rom_filename: "Sonic.md".into(),
                box_art_url: None,
                genre: Some("Platform".into()),
                players: None,
                rating: None,
                rating_count: None,
            }],
        )
        .unwrap();

        let roms = db.load_system_entries("sega_smd").unwrap();
        assert_eq!(roms[0].genre.as_deref(), Some("Shooter"));
    }

    #[test]
    fn genre_enrichment_mixed_empty_and_existing() {
        let (mut db, _dir) = open_temp_db();

        db.save_system_entries(
            "sega_smd",
            &[
                make_game_entry_with_genre("sega_smd", "Sonic.md", "Shooter"),
                make_game_entry("sega_smd", "Streets.md", false),
                make_game_entry("sega_smd", "Columns.md", false),
            ],
            None,
        )
        .unwrap();

        db.update_box_art_genre_rating(
            "sega_smd",
            &[
                super::super::BoxArtGenreRating {
                    rom_filename: "Sonic.md".into(),
                    box_art_url: None,
                    genre: Some("Platform".into()),
                    players: None,
                    rating: None,
                    rating_count: None,
                },
                super::super::BoxArtGenreRating {
                    rom_filename: "Streets.md".into(),
                    box_art_url: None,
                    genre: Some("Beat'em Up".into()),
                    players: None,
                    rating: None,
                    rating_count: None,
                },
            ],
        )
        .unwrap();

        let roms = db.load_system_entries("sega_smd").unwrap();
        let sonic = roms.iter().find(|r| r.rom_filename == "Sonic.md").unwrap();
        let streets = roms
            .iter()
            .find(|r| r.rom_filename == "Streets.md")
            .unwrap();
        let columns = roms
            .iter()
            .find(|r| r.rom_filename == "Columns.md")
            .unwrap();

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

    /// Helper to create a game entry with a specific developer and base_title.
    fn make_game_entry_with_developer(
        system: &str,
        filename: &str,
        developer: &str,
        base_title: &str,
    ) -> super::super::GameEntry {
        super::super::GameEntry {
            developer: developer.into(),
            base_title: base_title.into(),
            ..make_game_entry(system, filename, false)
        }
    }

    #[test]
    fn find_developer_matches_exact_match_first() {
        let (mut db, _dir) = open_temp_db();

        // Insert games: "SNK" (3 games), "SNK Playmore" (2 games), "Capcom / SNK" (1 game)
        db.save_system_entries(
            "arcade_fbneo",
            &[
                make_game_entry_with_developer("arcade_fbneo", "kof97.zip", "SNK", "KOF 97"),
                make_game_entry_with_developer("arcade_fbneo", "kof98.zip", "SNK", "KOF 98"),
                make_game_entry_with_developer("arcade_fbneo", "fatfury2.zip", "SNK", "Fatal Fury 2"),
                make_game_entry_with_developer("arcade_fbneo", "samsho5.zip", "SNK Playmore", "Samurai Shodown V"),
                make_game_entry_with_developer("arcade_fbneo", "samsho6.zip", "SNK Playmore", "Samurai Shodown VI"),
                make_game_entry_with_developer("arcade_fbneo", "svc.zip", "Capcom / SNK", "SVC Chaos"),
            ],
            None,
        )
        .unwrap();

        let matches = db.find_developer_matches("snk").unwrap();
        assert_eq!(matches.len(), 3, "Should find 3 matching developers");

        // Exact match "SNK" should be first.
        assert_eq!(matches[0].0, "SNK");
        assert_eq!(matches[0].1, 3);

        // "SNK Playmore" is a word-boundary match, should be second.
        assert_eq!(matches[1].0, "SNK Playmore");
        assert_eq!(matches[1].1, 2);

        // "Capcom / SNK" is a substring match, should be last.
        assert_eq!(matches[2].0, "Capcom / SNK");
        assert_eq!(matches[2].1, 1);
    }

    #[test]
    fn find_developer_matches_no_match_returns_empty() {
        let (mut db, _dir) = open_temp_db();

        db.save_system_entries(
            "snes",
            &[make_game_entry_with_developer("snes", "Mario.sfc", "Nintendo", "Mario")],
            None,
        )
        .unwrap();

        let matches = db.find_developer_matches("capcom").unwrap();
        assert!(matches.is_empty());
    }

    #[test]
    fn find_developer_matches_single_match() {
        let (mut db, _dir) = open_temp_db();

        db.save_system_entries(
            "snes",
            &[
                make_game_entry_with_developer("snes", "MegaManX.sfc", "Capcom", "Mega Man X"),
                make_game_entry_with_developer("snes", "BoF.sfc", "Capcom", "Breath of Fire"),
            ],
            None,
        )
        .unwrap();

        let matches = db.find_developer_matches("capcom").unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, "Capcom");
        assert_eq!(matches[0].1, 2);
    }

    // ── Helper to create game entries with full control over fields ──

    fn make_dev_entry(
        system: &str,
        filename: &str,
        developer: &str,
        base_title: &str,
        region: &str,
        genre: Option<&str>,
        box_art: Option<&str>,
    ) -> super::super::GameEntry {
        let genre_group = genre
            .map(|g| crate::genre::normalize_genre(g).to_string())
            .unwrap_or_default();
        super::super::GameEntry {
            developer: developer.into(),
            base_title: base_title.into(),
            region: region.into(),
            genre: genre.map(String::from),
            genre_group,
            box_art_url: box_art.map(String::from),
            ..make_game_entry(system, filename, false)
        }
    }

    fn make_dev_entry_clone(
        system: &str,
        filename: &str,
        developer: &str,
        base_title: &str,
    ) -> super::super::GameEntry {
        super::super::GameEntry {
            is_clone: true,
            ..make_dev_entry(system, filename, developer, base_title, "", None, None)
        }
    }

    fn make_dev_entry_hack(
        system: &str,
        filename: &str,
        developer: &str,
        base_title: &str,
    ) -> super::super::GameEntry {
        super::super::GameEntry {
            is_hack: true,
            ..make_dev_entry(system, filename, developer, base_title, "", None, None)
        }
    }

    fn make_dev_entry_multiplayer(
        system: &str,
        filename: &str,
        developer: &str,
        base_title: &str,
        players: u8,
    ) -> super::super::GameEntry {
        super::super::GameEntry {
            players: Some(players),
            ..make_dev_entry(system, filename, developer, base_title, "", None, None)
        }
    }

    // ── Tests for developer_games_paginated ──

    #[test]
    fn developer_games_paginated_empty_genre_returns_all() {
        let (mut db, _dir) = open_temp_db();

        db.save_system_entries(
            "snes",
            &[
                make_dev_entry("snes", "MegaManX.sfc", "Capcom", "Mega Man X", "us", Some("Action"), None),
                make_dev_entry("snes", "BoF.sfc", "Capcom", "Breath of Fire", "us", Some("RPG"), None),
            ],
            None,
        )
        .unwrap();

        let filters = super::DeveloperGamesFilter::default();
        let (entries, has_more, total) = db
            .developer_games_paginated("Capcom", None, 0, 50, "us", "", &filters)
            .unwrap();

        assert_eq!(total, 2);
        assert_eq!(entries.len(), 2);
        assert!(!has_more);
    }

    #[test]
    fn developer_games_paginated_specific_genre() {
        let (mut db, _dir) = open_temp_db();

        db.save_system_entries(
            "snes",
            &[
                make_dev_entry("snes", "MegaManX.sfc", "Capcom", "Mega Man X", "us", Some("Action"), None),
                make_dev_entry("snes", "BoF.sfc", "Capcom", "Breath of Fire", "us", Some("RPG"), None),
                make_dev_entry("snes", "SF2.sfc", "Capcom", "Street Fighter II", "us", Some("Fighting"), None),
            ],
            None,
        )
        .unwrap();

        let filters = super::DeveloperGamesFilter {
            genre: "Action",
            ..Default::default()
        };
        let (entries, _, total) = db
            .developer_games_paginated("Capcom", None, 0, 50, "us", "", &filters)
            .unwrap();

        assert_eq!(total, 1);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].base_title, "Mega Man X");
    }

    #[test]
    fn developer_games_paginated_system_and_genre_combined() {
        let (mut db, _dir) = open_temp_db();

        db.save_system_entries(
            "snes",
            &[
                make_dev_entry("snes", "MegaManX.sfc", "Capcom", "Mega Man X", "us", Some("Action"), None),
                make_dev_entry("snes", "BoF.sfc", "Capcom", "Breath of Fire", "us", Some("RPG"), None),
            ],
            None,
        )
        .unwrap();

        db.save_system_entries(
            "gba",
            &[
                make_dev_entry("gba", "MegaManZero.gba", "Capcom", "Mega Man Zero", "us", Some("Action"), None),
            ],
            None,
        )
        .unwrap();

        // Filter: system=snes + genre=Action -> only Mega Man X.
        let filters = super::DeveloperGamesFilter {
            genre: "Action",
            ..Default::default()
        };
        let (entries, _, total) = db
            .developer_games_paginated("Capcom", Some("snes"), 0, 50, "us", "", &filters)
            .unwrap();

        assert_eq!(total, 1);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].base_title, "Mega Man X");
    }

    #[test]
    fn developer_games_paginated_region_dedup_prefers_user_region() {
        let (mut db, _dir) = open_temp_db();

        // Same base_title, same system, different regions.
        db.save_system_entries(
            "snes",
            &[
                make_dev_entry("snes", "SF2-us.sfc", "Capcom", "Street Fighter II", "us", None, None),
                make_dev_entry("snes", "SF2-jp.sfc", "Capcom", "Street Fighter II", "japan", None, None),
                make_dev_entry("snes", "SF2-eu.sfc", "Capcom", "Street Fighter II", "europe", None, None),
            ],
            None,
        )
        .unwrap();

        let filters = super::DeveloperGamesFilter::default();
        let (entries, _, total) = db
            .developer_games_paginated("Capcom", None, 0, 50, "us", "europe", &filters)
            .unwrap();

        // Should be deduped to 1 entry.
        assert_eq!(total, 1);
        assert_eq!(entries.len(), 1);
        // Preferred region should win.
        assert_eq!(entries[0].region, "us");
    }

    #[test]
    fn developer_games_paginated_offset_beyond_total() {
        let (mut db, _dir) = open_temp_db();

        db.save_system_entries(
            "snes",
            &[
                make_dev_entry("snes", "MegaManX.sfc", "Capcom", "Mega Man X", "us", None, None),
                make_dev_entry("snes", "BoF.sfc", "Capcom", "Breath of Fire", "us", None, None),
            ],
            None,
        )
        .unwrap();

        let filters = super::DeveloperGamesFilter::default();
        let (entries, has_more, total) = db
            .developer_games_paginated("Capcom", None, 100, 50, "us", "", &filters)
            .unwrap();

        // Total still reflects the real count, but no entries returned.
        assert_eq!(total, 2);
        assert!(entries.is_empty());
        assert!(!has_more);
    }

    #[test]
    fn developer_games_paginated_has_more_with_limit_plus_one() {
        let (mut db, _dir) = open_temp_db();

        db.save_system_entries(
            "snes",
            &[
                make_dev_entry("snes", "A.sfc", "Capcom", "Game A", "us", None, None),
                make_dev_entry("snes", "B.sfc", "Capcom", "Game B", "us", None, None),
                make_dev_entry("snes", "C.sfc", "Capcom", "Game C", "us", None, None),
            ],
            None,
        )
        .unwrap();

        let filters = super::DeveloperGamesFilter::default();

        // Limit 2 -> should get 2 entries, has_more=true.
        let (entries, has_more, total) = db
            .developer_games_paginated("Capcom", None, 0, 2, "us", "", &filters)
            .unwrap();
        assert_eq!(entries.len(), 2);
        assert!(has_more);
        assert_eq!(total, 3);

        // Offset 2, limit 2 -> should get 1 entry, has_more=false.
        let (entries, has_more, _) = db
            .developer_games_paginated("Capcom", None, 2, 2, "us", "", &filters)
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert!(!has_more);
    }

    #[test]
    fn developer_games_paginated_hide_hacks_and_clones() {
        let (mut db, _dir) = open_temp_db();

        db.save_system_entries(
            "snes",
            &[
                make_dev_entry("snes", "SF2.sfc", "Capcom", "Street Fighter II", "us", None, None),
                make_dev_entry_hack("snes", "SF2-hack.sfc", "Capcom", "Street Fighter II Hack"),
                make_dev_entry_clone("snes", "SF2-clone.sfc", "Capcom", "Street Fighter II Clone"),
            ],
            None,
        )
        .unwrap();

        let filters = super::DeveloperGamesFilter {
            hide_hacks: true,
            hide_clones: true,
            ..Default::default()
        };
        let (entries, _, total) = db
            .developer_games_paginated("Capcom", None, 0, 50, "us", "", &filters)
            .unwrap();

        assert_eq!(total, 1);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].base_title, "Street Fighter II");
    }

    #[test]
    fn developer_games_paginated_multiplayer_only() {
        let (mut db, _dir) = open_temp_db();

        db.save_system_entries(
            "snes",
            &[
                make_dev_entry_multiplayer("snes", "SF2.sfc", "Capcom", "Street Fighter II", 2),
                make_dev_entry("snes", "MegaManX.sfc", "Capcom", "Mega Man X", "us", None, None),
            ],
            None,
        )
        .unwrap();

        let filters = super::DeveloperGamesFilter {
            multiplayer_only: true,
            ..Default::default()
        };
        let (entries, _, total) = db
            .developer_games_paginated("Capcom", None, 0, 50, "us", "", &filters)
            .unwrap();

        assert_eq!(total, 1);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].base_title, "Street Fighter II");
    }

    // ── Tests for games_by_developer dedup ──

    #[test]
    fn games_by_developer_deduplicates_across_systems() {
        let (mut db, _dir) = open_temp_db();

        // Same game on two systems.
        db.save_system_entries(
            "snes",
            &[make_dev_entry("snes", "SF2-snes.sfc", "Capcom", "Street Fighter II", "us", None, None)],
            None,
        )
        .unwrap();

        db.save_system_entries(
            "sega_smd",
            &[make_dev_entry("sega_smd", "SF2-md.md", "Capcom", "Street Fighter II", "us", None, None)],
            None,
        )
        .unwrap();

        let results = db.games_by_developer("Capcom", 50, "us", "").unwrap();

        // Should be deduped to 1 entry (same base_title).
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn games_by_developer_prefers_entry_with_box_art() {
        let (mut db, _dir) = open_temp_db();

        // Same game, one with box art and one without.
        db.save_system_entries(
            "snes",
            &[
                make_dev_entry("snes", "SF2-noart.sfc", "Capcom", "Street Fighter II", "us", None, None),
                make_dev_entry("snes", "SF2-art.sfc", "Capcom", "Street Fighter II", "us", None, Some("/img/sf2.png")),
            ],
            None,
        )
        .unwrap();

        let results = db.games_by_developer("Capcom", 50, "us", "").unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].box_art_url.as_deref(), Some("/img/sf2.png"));
    }

    #[test]
    fn games_by_developer_excludes_clones_and_hacks() {
        let (mut db, _dir) = open_temp_db();

        db.save_system_entries(
            "snes",
            &[
                make_dev_entry("snes", "SF2.sfc", "Capcom", "Street Fighter II", "us", None, None),
                make_dev_entry_hack("snes", "SF2-hack.sfc", "Capcom", "SF2 Hack"),
                make_dev_entry_clone("snes", "SF2-clone.sfc", "Capcom", "SF2 Clone"),
            ],
            None,
        )
        .unwrap();

        let results = db.games_by_developer("Capcom", 50, "us", "").unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].base_title, "Street Fighter II");
    }

    #[test]
    fn games_by_developer_prefers_user_region() {
        let (mut db, _dir) = open_temp_db();

        db.save_system_entries(
            "snes",
            &[
                make_dev_entry("snes", "SF2-jp.sfc", "Capcom", "Street Fighter II", "japan", None, None),
                make_dev_entry("snes", "SF2-eu.sfc", "Capcom", "Street Fighter II", "europe", None, None),
            ],
            None,
        )
        .unwrap();

        // Prefer europe, secondary japan.
        let results = db.games_by_developer("Capcom", 50, "europe", "japan").unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].region, "europe");
    }
}
