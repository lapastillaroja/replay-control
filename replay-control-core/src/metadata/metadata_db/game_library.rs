//! Operations on the `game_library` and `game_library_meta` tables.

use rusqlite::{Connection, OptionalExtension, params};

use crate::error::{Error, Result};

use super::{GameEntry, MetadataDb, SystemMeta, unix_now};

/// SELECT columns for `game_library` queries that feed `row_to_game_entry()`.
///
/// The column order must match the positional indices in `row_to_game_entry()`.
const GAME_ENTRY_COLUMNS: &str = "\
    system, rom_filename, rom_path, display_name, base_title, series_key, \
    region, developer, genre, genre_group, rating, rating_count, players, \
    is_clone, is_m3u, is_translation, is_hack, is_special, \
    box_art_url, driver_status, size_bytes, crc32, hash_mtime, hash_matched_name, \
    release_year, cooperative";

/// Build the pre-computed, lowercased search index value for a game_library row.
///
/// Format: `"{display}|{rom_filename}|{base_title}[|{developer}][|{year}]"`.
/// All fields are lowercased and joined by `|`.
/// Computed at insert time so search queries only need `LIKE` on a single column.
fn build_search_text(
    display_name: Option<&str>,
    rom_filename: &str,
    base_title: &str,
    developer: &str,
    release_year: Option<u16>,
) -> String {
    let display = display_name.unwrap_or(rom_filename);
    let mut text = format!(
        "{}|{}|{}",
        display.to_lowercase(),
        rom_filename.to_lowercase(),
        base_title.to_lowercase()
    );
    if !developer.is_empty() {
        text.push('|');
        text.push_str(&developer.to_lowercase());
    }
    if let Some(year) = release_year {
        text.push('|');
        text.push_str(&year.to_string());
    }
    text
}

/// Unified filter options for all game library queries (search, ROM list, developer page).
#[derive(Debug, Default)]
pub struct SearchFilter<'a> {
    pub hide_hacks: bool,
    pub hide_translations: bool,
    pub hide_betas: bool,
    pub hide_clones: bool,
    pub genre: &'a str,
    pub multiplayer_only: bool,
    pub coop_only: bool,
    pub min_rating: Option<f64>,
    pub min_year: Option<u16>,
    pub max_year: Option<u16>,
}

impl MetadataDb {
    /// Batch lookup of game entries by primary key `(system, rom_filename)`.
    ///
    /// Groups keys by system and uses `WHERE system = ? AND rom_filename IN (...)`
    /// per group. Returns a map from `(system, rom_filename)` to `GameEntry`.
    pub fn lookup_game_entries(
        conn: &Connection,
        keys: &[(impl AsRef<str>, impl AsRef<str>)],
    ) -> Result<std::collections::HashMap<(String, String), GameEntry>> {
        use std::collections::HashMap;

        if keys.is_empty() {
            return Ok(HashMap::new());
        }

        // Group keys by system.
        let mut by_system: HashMap<&str, Vec<&str>> = HashMap::new();
        for (sys, fname) in keys {
            by_system
                .entry(sys.as_ref())
                .or_default()
                .push(fname.as_ref());
        }

        let mut result: HashMap<(String, String), GameEntry> = HashMap::new();

        for (system, filenames) in &by_system {
            let placeholders: Vec<String> = (0..filenames.len())
                .map(|i| format!("?{}", i + 2))
                .collect();
            let sql = format!(
                "SELECT {GAME_ENTRY_COLUMNS} FROM game_library \
                 WHERE system = ?1 AND rom_filename IN ({})",
                placeholders.join(", ")
            );

            let mut params: Vec<Box<dyn rusqlite::types::ToSql>> =
                Vec::with_capacity(filenames.len() + 1);
            params.push(Box::new(system.to_string()));
            for f in filenames {
                params.push(Box::new(f.to_string()));
            }
            let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                params.iter().map(|p| p.as_ref()).collect();

            let mut stmt = conn
                .prepare(&sql)
                .map_err(|e| Error::Other(format!("Prepare lookup_game_entries: {e}")))?;
            let rows = stmt
                .query_map(param_refs.as_slice(), Self::row_to_game_entry)
                .map_err(|e| Error::Other(format!("Query lookup_game_entries: {e}")))?;

            for entry in rows.flatten() {
                result.insert((entry.system.clone(), entry.rom_filename.clone()), entry);
            }
        }

        Ok(result)
    }

    /// Get all distinct `box_art_url` values from `game_library` for a given system.
    ///
    /// Returns the URL paths (e.g., `/media/sega_smd/boxart/Sonic.png`).
    pub fn active_box_art_urls(conn: &Connection, system: &str) -> Result<Vec<String>> {
        let mut stmt = conn
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
    pub fn active_systems(conn: &Connection) -> Result<Vec<String>> {
        let mut stmt = conn
            .prepare("SELECT DISTINCT system FROM game_library")
            .map_err(|e| Error::Other(format!("Query active_systems failed: {e}")))?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| Error::Other(format!("Query active_systems failed: {e}")))?;
        Ok(rows.flatten().collect())
    }

    /// Count games with thumbnails per system from `game_library.box_art_url`.
    ///
    /// This is the live source of truth -- rebuilt every enrichment pass.
    /// Returns `(system, count_with_box_art)` tuples.
    pub fn thumbnails_per_system(conn: &Connection) -> Result<Vec<(String, usize)>> {
        let mut stmt = conn
            .prepare(
                "SELECT system,
                        SUM(CASE WHEN box_art_url IS NOT NULL THEN 1 ELSE 0 END)
                 FROM game_library
                 GROUP BY system",
            )
            .map_err(|e| Error::Other(format!("Query failed: {e}")))?;

        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1).map(|v| v as usize)?,
                ))
            })
            .map_err(|e| Error::Other(format!("Query failed: {e}")))?;

        Ok(rows.flatten().collect())
    }

    // ── Game Library (L2 persistent cache) ─────────────────────────────

    /// Save a system's game list to the game_library table.
    /// Replaces all existing entries for the system in a single transaction.
    pub fn save_system_entries(
        conn: &mut Connection,
        system: &str,
        roms: &[GameEntry],
        dir_mtime_secs: Option<i64>,
    ) -> Result<()> {
        let tx = conn
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
                     base_title, series_key, region, developer, search_text,
                     genre, genre_group, rating, rating_count, players,
                     is_clone, is_m3u, is_translation, is_hack, is_special,
                     box_art_url, driver_status, size_bytes, crc32, hash_mtime, hash_matched_name,
                     release_year, cooperative)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                             ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27)",
                )
                .map_err(|e| Error::Other(format!("Prepare game_library insert: {e}")))?;

            for rom in roms {
                let search_text = build_search_text(
                    rom.display_name.as_deref(),
                    &rom.rom_filename,
                    &rom.base_title,
                    &rom.developer,
                    rom.release_year,
                );
                stmt.execute(params![
                    &rom.system,
                    &rom.rom_filename,
                    &rom.rom_path,
                    &rom.display_name,
                    &rom.base_title,
                    &rom.series_key,
                    &rom.region,
                    &rom.developer,
                    &search_text,
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
                    rom.release_year.map(|y| y as i32),
                    rom.cooperative,
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
    pub fn load_system_entries(conn: &Connection, system: &str) -> Result<Vec<GameEntry>> {
        let sql = format!("SELECT {GAME_ENTRY_COLUMNS} FROM game_library WHERE system = ?1");
        let mut stmt = conn
            .prepare(&sql)
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

    /// Count game entries for a system.
    pub fn count_system_entries(conn: &Connection, system: &str) -> Result<usize> {
        conn.query_row(
            "SELECT COUNT(*) FROM game_library WHERE system = ?1",
            params![system],
            |row| row.get::<_, i64>(0).map(|v| v as usize),
        )
        .map_err(|e| Error::Other(format!("Query count_system_entries: {e}")))
    }

    /// Load the set of rom_filenames that are marked as clones for a system.
    ///
    /// Lightweight alternative to `load_system_entries` when only `is_clone` is needed.
    pub fn load_clone_filenames(
        conn: &Connection,
        system: &str,
    ) -> Result<std::collections::HashSet<String>> {
        let mut stmt = conn
            .prepare("SELECT rom_filename FROM game_library WHERE system = ?1 AND is_clone = 1")
            .map_err(|e| Error::Other(format!("Prepare load_clone_filenames: {e}")))?;

        let rows = stmt
            .query_map(params![system], |row| row.get::<_, String>(0))
            .map_err(|e| Error::Other(format!("Query load_clone_filenames: {e}")))?;

        let mut result = std::collections::HashSet::new();
        for row in rows {
            result.insert(row.map_err(|e| Error::Other(format!("Row read failed: {e}")))?);
        }
        Ok(result)
    }

    /// Load a page of game entries for a system, sorted by display name (case-insensitive).
    ///
    /// Same columns as `load_system_entries` but with `ORDER BY` + `LIMIT`/`OFFSET`
    /// for SQL-level pagination. Used as a fast-path when the L1 cache is cold.
    pub fn load_system_entries_page(
        conn: &Connection,
        system: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<GameEntry>> {
        let sql = format!(
            "SELECT {GAME_ENTRY_COLUMNS} FROM game_library WHERE system = ?1 \
             ORDER BY COALESCE(display_name, rom_filename) COLLATE NOCASE \
             LIMIT ?2 OFFSET ?3"
        );
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| Error::Other(format!("Prepare load_system_entries_page: {e}")))?;

        let rows = stmt
            .query_map(
                params![system, limit as i64, offset as i64],
                Self::row_to_game_entry,
            )
            .map_err(|e| Error::Other(format!("Query load_system_entries_page: {e}")))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| Error::Other(format!("Row read failed: {e}")))?);
        }
        Ok(result)
    }

    /// Load a single game entry by system and filename.
    /// Uses the primary key index for O(1) lookup.
    pub fn load_single_entry(
        conn: &Connection,
        system: &str,
        rom_filename: &str,
    ) -> Result<Option<GameEntry>> {
        let sql = format!(
            "SELECT {GAME_ENTRY_COLUMNS} FROM game_library \
             WHERE system = ?1 AND rom_filename = ?2"
        );
        conn.query_row(&sql, params![system, rom_filename], Self::row_to_game_entry)
            .optional()
            .map_err(|e| Error::Other(format!("Query load_single_entry: {e}")))
    }

    /// Save just the system-level metadata (counts, mtime) without replacing game entries.
    /// Used when we know game counts from scan_systems but haven't loaded entries yet.
    pub fn save_system_meta(
        conn: &Connection,
        system: &str,
        dir_mtime_secs: Option<i64>,
        rom_count: usize,
        total_size_bytes: u64,
    ) -> Result<()> {
        let now = unix_now();
        conn.execute(
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
    pub fn load_system_meta(conn: &Connection, system: &str) -> Result<Option<SystemMeta>> {
        conn.query_row(
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
    pub fn load_all_system_meta(conn: &Connection) -> Result<Vec<SystemMeta>> {
        let mut stmt = conn
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
    pub fn load_cached_hashes(
        conn: &Connection,
        system: &str,
    ) -> Result<std::collections::HashMap<String, crate::rom_hash::CachedHash>> {
        use std::collections::HashMap;

        let mut stmt = conn
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
        conn: &mut Connection,
        system: &str,
        enrichments: &[super::RomEnrichment],
    ) -> Result<usize> {
        let tx = conn
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
        conn: &mut Connection,
        system: &str,
        enrichments: &[super::BoxArtGenreRating],
    ) -> Result<()> {
        let tx = conn
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
    pub fn clear_system_game_library(conn: &Connection, system: &str) -> Result<()> {
        conn.execute(
            "DELETE FROM game_library WHERE system = ?1",
            params![system],
        )
        .map_err(|e| Error::Other(format!("Clear system game_library: {e}")))?;
        conn.execute(
            "DELETE FROM game_library_meta WHERE system = ?1",
            params![system],
        )
        .map_err(|e| Error::Other(format!("Clear system game_library_meta: {e}")))?;
        Ok(())
    }

    /// Get filenames of visible games for a system (excludes disc files hidden by M3U dedup).
    pub fn visible_filenames(conn: &Connection, system: &str) -> Result<Vec<String>> {
        let mut stmt = conn
            .prepare("SELECT rom_filename FROM game_library WHERE system = ?1")
            .map_err(|e| Error::Other(format!("Query failed: {e}")))?;
        let rows = stmt
            .query_map(params![system], |row| row.get(0))
            .map_err(|e| Error::Other(format!("Query failed: {e}")))?;
        Ok(rows.flatten().collect())
    }

    /// Get `(rom_filename, base_title)` pairs for a system.
    ///
    /// Used by enrichment to share box art between ROMs with the same base_title
    /// (e.g., region variants, revisions).
    pub fn visible_base_titles(conn: &Connection, system: &str) -> Result<Vec<(String, String)>> {
        let mut stmt = conn
            .prepare("SELECT rom_filename, base_title FROM game_library WHERE system = ?1")
            .map_err(|e| Error::Other(format!("Query failed: {e}")))?;
        let rows = stmt
            .query_map(params![system], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| Error::Other(format!("Query failed: {e}")))?;
        Ok(rows.flatten().collect())
    }

    /// Clear all game_library and game_library_meta entries.
    pub fn clear_all_game_library(conn: &Connection) -> Result<()> {
        conn.execute("DELETE FROM game_library", [])
            .map_err(|e| Error::Other(format!("Clear game_library: {e}")))?;
        conn.execute("DELETE FROM game_library_meta", [])
            .map_err(|e| Error::Other(format!("Clear game_library_meta: {e}")))?;
        Ok(())
    }

    /// Delete the `game_library` entry for a specific ROM.
    pub fn delete_for_rom(conn: &Connection, system: &str, rom_filename: &str) {
        let _ = conn.execute(
            "DELETE FROM game_library WHERE system = ?1 AND rom_filename = ?2",
            params![system, rom_filename],
        );
    }

    /// Rename a ROM in the `game_library` table.
    /// Also rebuilds `search_text` since `rom_filename` is part of the search index.
    pub fn rename_for_rom(conn: &Connection, system: &str, old_filename: &str, new_filename: &str) {
        if let Err(e) = conn.execute(
            "UPDATE game_library
             SET rom_filename = ?3,
                 search_text = LOWER(COALESCE(display_name, ?3)) || '|' || LOWER(?3) || '|' || LOWER(base_title)
             WHERE system = ?1 AND rom_filename = ?2",
            params![system, old_filename, new_filename],
        ) {
            tracing::warn!("Failed to update game_library: {e}");
        }
    }

    /// Fetch current genres from `game_library` for a single system.
    pub fn system_rom_genres(
        conn: &Connection,
        system: &str,
    ) -> Result<std::collections::HashMap<String, String>> {
        use std::collections::HashMap;

        let mut stmt = conn
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
    pub fn system_rom_developers(
        conn: &Connection,
        system: &str,
    ) -> Result<std::collections::HashSet<String>> {
        use std::collections::HashSet;

        let mut stmt = conn
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
    pub fn update_developers(
        conn: &mut Connection,
        system: &str,
        developers: &[(String, String)],
    ) -> Result<()> {
        let tx = conn
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

    /// Fetch filenames that already have a `release_year` in `game_library` for a system.
    pub fn system_rom_release_years(
        conn: &Connection,
        system: &str,
    ) -> Result<std::collections::HashSet<String>> {
        use std::collections::HashSet;

        let mut stmt = conn
            .prepare(
                "SELECT rom_filename FROM game_library
                 WHERE system = ?1 AND release_year IS NOT NULL",
            )
            .map_err(|e| Error::Other(format!("Prepare system_rom_release_years: {e}")))?;

        let rows = stmt
            .query_map(params![system], |row| row.get::<_, String>(0))
            .map_err(|e| Error::Other(format!("System rom release_years query: {e}")))?;

        let mut set = HashSet::new();
        for row in rows.flatten() {
            set.insert(row);
        }
        Ok(set)
    }

    /// Batch-load `(rom_filename, release_year)` pairs for a system.
    ///
    /// Returns only rows where `release_year IS NOT NULL`, keyed by filename.
    pub fn system_release_years(
        conn: &Connection,
        system: &str,
    ) -> Result<std::collections::HashMap<String, u16>> {
        let mut stmt = conn
            .prepare(
                "SELECT rom_filename, release_year FROM game_library
                 WHERE system = ?1 AND release_year IS NOT NULL",
            )
            .map_err(|e| Error::Other(format!("Prepare system_release_years: {e}")))?;

        let rows = stmt
            .query_map(params![system], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, u16>(1)?))
            })
            .map_err(|e| Error::Other(format!("Query system_release_years: {e}")))?;

        let mut map = std::collections::HashMap::new();
        for row in rows.flatten() {
            map.insert(row.0, row.1);
        }
        Ok(map)
    }

    /// Batch update `release_year` for entries in game_library.
    pub fn update_release_years(
        conn: &mut Connection,
        system: &str,
        years: &[(String, u16)],
    ) -> Result<()> {
        let tx = conn
            .transaction()
            .map_err(|e| Error::Other(format!("Transaction start failed: {e}")))?;

        {
            let mut stmt = tx
                .prepare(
                    "UPDATE game_library SET release_year = ?2
                     WHERE system = ?3 AND rom_filename = ?1
                       AND release_year IS NULL",
                )
                .map_err(|e| Error::Other(format!("Prepare release_year update: {e}")))?;

            for (filename, year) in years {
                stmt.execute(params![filename, *year as i32, system])
                    .map_err(|e| Error::Other(format!("Update release_year: {e}")))?;
            }
        }

        tx.commit()
            .map_err(|e| Error::Other(format!("Transaction commit failed: {e}")))?;
        Ok(())
    }

    /// Fetch current player counts from `game_library` for a single system.
    pub fn system_rom_players(
        conn: &Connection,
        system: &str,
    ) -> Result<std::collections::HashSet<String>> {
        use std::collections::HashSet;

        let mut stmt = conn
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

    /// Fetch rom_filenames that already have cooperative=1 in `game_library` for a system.
    pub fn system_rom_cooperative(
        conn: &Connection,
        system: &str,
    ) -> Result<std::collections::HashSet<String>> {
        use std::collections::HashSet;

        let mut stmt = conn
            .prepare(
                "SELECT rom_filename FROM game_library
                 WHERE system = ?1 AND cooperative = 1",
            )
            .map_err(|e| Error::Other(format!("Prepare system_rom_cooperative: {e}")))?;

        let rows = stmt
            .query_map(params![system], |row| row.get::<_, String>(0))
            .map_err(|e| Error::Other(format!("System rom cooperative query: {e}")))?;

        let mut set = HashSet::new();
        for row in rows.flatten() {
            set.insert(row);
        }
        Ok(set)
    }

    /// Batch update `cooperative` flag for entries in game_library.
    pub fn update_cooperative(
        conn: &mut Connection,
        system: &str,
        filenames: &[String],
    ) -> Result<()> {
        let tx = conn
            .transaction()
            .map_err(|e| Error::Other(format!("Transaction start failed: {e}")))?;

        {
            let mut stmt = tx
                .prepare(
                    "UPDATE game_library SET cooperative = 1
                     WHERE system = ?2 AND rom_filename = ?1
                       AND cooperative = 0",
                )
                .map_err(|e| Error::Other(format!("Prepare cooperative update: {e}")))?;

            for filename in filenames {
                stmt.execute(params![filename, system])
                    .map_err(|e| Error::Other(format!("Update cooperative: {e}")))?;
            }
        }

        tx.commit()
            .map_err(|e| Error::Other(format!("Transaction commit failed: {e}")))?;
        Ok(())
    }

    /// Find developer names that match the given query (case-insensitive).
    pub fn find_developer_matches(conn: &Connection, query: &str) -> Result<Vec<(String, usize)>> {
        let q = query.to_lowercase();
        let mut stmt = conn
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
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1).map(|v| v as usize)?,
                ))
            })
            .map_err(|e| Error::Other(format!("Query find_developer_matches: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Get games by a specific developer, preferring those with box art.
    pub fn games_by_developer(
        conn: &Connection,
        developer: &str,
        limit: usize,
        region_pref: &str,
        region_secondary: &str,
    ) -> Result<Vec<GameEntry>> {
        let sql = format!(
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
            SELECT {GAME_ENTRY_COLUMNS}
            FROM deduped WHERE rn = 1
            ORDER BY box_art_url IS NULL, RANDOM()
            LIMIT ?4"
        );
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| Error::Other(format!("Prepare games_by_developer: {e}")))?;

        let rows = stmt
            .query_map(
                params![developer, region_pref, region_secondary, limit as i64],
                Self::row_to_game_entry,
            )
            .map_err(|e| Error::Other(format!("Query games_by_developer: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Get distinct genre groups for a developer's games, optionally filtered by system.
    pub fn developer_genre_groups(
        conn: &Connection,
        developer: &str,
        system_filter: Option<&str>,
    ) -> Result<Vec<String>> {
        let has_system = system_filter.is_some_and(|s| !s.is_empty());

        if has_system {
            let mut stmt = conn
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
            let mut stmt = conn
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
    pub fn developer_systems(conn: &Connection, developer: &str) -> Result<Vec<(String, usize)>> {
        let mut stmt = conn
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
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1).map(|v| v as usize)?,
                ))
            })
            .map_err(|e| Error::Other(format!("Query developer_systems: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Find the most common genre_group among a set of filenames in a system.
    ///
    /// Resolves filenames to `base_title` via a subquery, then groups by
    /// `genre_group` across all ROMs sharing those base titles. This uses
    /// the `idx_game_library_base_title (system, base_title)` index for
    /// the genre aggregation instead of scanning by `rom_filename`.
    ///
    /// Returns `None` if none of the filenames have a genre_group.
    pub fn top_genre_for_filenames(
        conn: &Connection,
        system: &str,
        filenames: &[&str],
    ) -> Result<Option<String>> {
        if filenames.is_empty() {
            return Ok(None);
        }

        // Build IN clause with positional parameters.
        let placeholders: Vec<String> = (0..filenames.len())
            .map(|i| format!("?{}", i + 2)) // ?1 is system
            .collect();
        let sql = format!(
            "SELECT genre_group, COUNT(*) as cnt \
             FROM game_library \
             WHERE system = ?1 AND genre_group != '' AND base_title IN (\
               SELECT base_title FROM game_library \
               WHERE system = ?1 AND rom_filename IN ({}) AND base_title != ''\
             ) \
             GROUP BY genre_group \
             ORDER BY cnt DESC \
             LIMIT 1",
            placeholders.join(", ")
        );

        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> =
            Vec::with_capacity(filenames.len() + 1);
        params.push(Box::new(system.to_string()));
        for f in filenames {
            params.push(Box::new(f.to_string()));
        }
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();

        conn.query_row(&sql, param_refs.as_slice(), |row| row.get::<_, String>(0))
            .optional()
            .map_err(|e| Error::Other(format!("top_genre_for_filenames: {e}")))
    }

    /// Build WHERE clauses and parameter values for a `SearchFilter` and optional
    /// search query words. Extracted to share logic between `search_game_library`
    /// and any future query functions.
    ///
    /// If `system` is provided, adds a `system = ?` clause.
    /// If `developer` is provided, adds a `developer = ? COLLATE NOCASE` clause.
    /// If `query_words` is non-empty, adds `search_text LIKE ?` clauses.
    fn build_filter_clauses(
        system: Option<&str>,
        developer: Option<&str>,
        query_words: &[String],
        filter: &SearchFilter<'_>,
    ) -> (Vec<String>, Vec<String>) {
        let mut where_clauses: Vec<String> = Vec::new();
        let mut param_values: Vec<String> = Vec::new();

        // System scope.
        if let Some(sys) = system {
            param_values.push(sys.to_string());
            let idx = param_values.len();
            where_clauses.push(format!("system = ?{idx}"));
        }

        // Developer scope (case-insensitive).
        if let Some(dev) = developer {
            param_values.push(dev.to_string());
            let idx = param_values.len();
            where_clauses.push(format!("developer = ?{idx} COLLATE NOCASE"));
        }

        // Each query word becomes a LIKE pattern on search_text.
        // Escape SQL wildcards in the word to prevent injection via % or _.
        for word in query_words {
            let escaped = word.replace('%', "\\%").replace('_', "\\_");
            param_values.push(format!("%{escaped}%"));
            let idx = param_values.len();
            where_clauses.push(format!("search_text LIKE ?{idx} ESCAPE '\\'"));
        }

        // Content filters (static clauses, no params needed).
        if filter.hide_hacks {
            where_clauses.push("is_hack = 0".to_string());
        }
        if filter.hide_translations {
            where_clauses.push("is_translation = 0".to_string());
        }
        if filter.hide_betas {
            where_clauses.push("is_special = 0".to_string());
        }
        if filter.hide_clones {
            where_clauses.push("is_clone = 0".to_string());
        }
        if filter.multiplayer_only {
            where_clauses.push("players >= 2".to_string());
        }
        if filter.coop_only {
            where_clauses.push("cooperative = 1".to_string());
        }

        // Genre filter (parameterized).
        if !filter.genre.is_empty() {
            param_values.push(filter.genre.to_string());
            let idx = param_values.len();
            where_clauses.push(format!("genre_group = ?{idx} COLLATE NOCASE"));
        }

        // Rating filter (parameterized).
        if let Some(mr) = filter.min_rating {
            param_values.push(mr.to_string());
            let idx = param_values.len();
            where_clauses.push(format!("rating >= ?{idx}"));
        }

        // Year range filters (parameterized). NULL release_year is excluded.
        if let Some(min_y) = filter.min_year {
            param_values.push(min_y.to_string());
            let idx = param_values.len();
            where_clauses.push(format!("release_year >= ?{idx}"));
        }
        if let Some(max_y) = filter.max_year {
            param_values.push(max_y.to_string());
            let idx = param_values.len();
            where_clauses.push(format!("release_year <= ?{idx}"));
        }

        (where_clauses, param_values)
    }

    /// Build a WHERE SQL string from clause/param vectors.
    fn build_where_sql(where_clauses: &[String]) -> String {
        if where_clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", where_clauses.join(" AND "))
        }
    }

    /// Unified game library search and pagination.
    ///
    /// Applies content filters, optional text search, optional system scope,
    /// and optional developer scope at the SQL level.
    ///
    /// - `system`: `None` = all systems, `Some(sys)` = single system.
    /// - `developer`: `None` = all developers, `Some(dev)` = single developer (case-insensitive).
    /// - `query_words`: empty = no text filter, non-empty = `search_text LIKE` matching.
    /// - `offset`/`limit`: SQL pagination. When `query_words` is non-empty, pagination
    ///   is skipped (all matches returned) so the caller can score and re-paginate.
    ///
    /// Returns `(entries, total_count)`.
    pub fn search_game_library(
        conn: &Connection,
        system: Option<&str>,
        developer: Option<&str>,
        query_words: &[String],
        filter: &SearchFilter<'_>,
        offset: usize,
        limit: usize,
    ) -> Result<(Vec<GameEntry>, usize)> {
        let (where_clauses, param_values) =
            Self::build_filter_clauses(system, developer, query_words, filter);
        let where_sql = Self::build_where_sql(&where_clauses);

        let has_text_search = !query_words.is_empty();

        // When there's a text search, skip COUNT — the caller will re-score and
        // re-paginate in Rust, so the SQL-level total is unused.
        let total: usize = if has_text_search {
            0
        } else {
            let count_sql = format!("SELECT COUNT(*) FROM game_library {where_sql}");
            let param_refs: Vec<&dyn rusqlite::types::ToSql> = param_values
                .iter()
                .map(|v| v as &dyn rusqlite::types::ToSql)
                .collect();
            let t = conn
                .query_row(&count_sql, param_refs.as_slice(), |row| {
                    row.get::<_, i64>(0).map(|v| v as usize)
                })
                .map_err(|e| Error::Other(format!("Count search_game_library: {e}")))?;
            if t == 0 {
                return Ok((Vec::new(), 0));
            }
            t
        };

        // When there's a text search, return all results for Rust-level scoring.
        // The caller will sort by relevance and paginate.
        //
        // LIMIT/OFFSET are bound as i64 (not String) because SQLite requires
        // integer types for LIMIT/OFFSET, and string values larger than i64::MAX
        // cause overflow/datatype-mismatch errors.
        let (order_and_limit, limit_offset_params) = if has_text_search {
            (String::new(), None)
        } else {
            let next_idx = param_values.len() + 1;
            (
                format!(
                    "ORDER BY COALESCE(display_name, rom_filename) COLLATE NOCASE LIMIT ?{} OFFSET ?{}",
                    next_idx,
                    next_idx + 1
                ),
                Some((
                    limit.min(i64::MAX as usize) as i64,
                    offset.min(i64::MAX as usize) as i64,
                )),
            )
        };

        let sql = format!(
            "SELECT {GAME_ENTRY_COLUMNS} \
             FROM game_library \
             {where_sql} \
             {order_and_limit}"
        );

        // Build parameter refs: string params from filters, then optional i64
        // params for LIMIT/OFFSET.
        let mut all_refs: Vec<Box<dyn rusqlite::types::ToSql>> = param_values
            .into_iter()
            .map(|v| Box::new(v) as Box<dyn rusqlite::types::ToSql>)
            .collect();
        if let Some((lim, off)) = limit_offset_params {
            all_refs.push(Box::new(lim));
            all_refs.push(Box::new(off));
        }
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            all_refs.iter().map(|v| v.as_ref()).collect();

        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| Error::Other(format!("Prepare search_game_library: {e}")))?;

        let rows = stmt
            .query_map(param_refs.as_slice(), Self::row_to_game_entry)
            .map_err(|e| Error::Other(format!("Query search_game_library: {e}")))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| Error::Other(format!("Row read failed: {e}")))?);
        }
        Ok((result, total))
    }

    /// Thin wrapper: paginated game list for a developer.
    ///
    /// Delegates to `search_game_library` with the developer parameter.
    /// Returns `(entries, total_count)`.
    pub fn developer_games(
        conn: &Connection,
        developer: &str,
        filter: &SearchFilter<'_>,
        offset: usize,
        limit: usize,
    ) -> Result<(Vec<GameEntry>, usize)> {
        Self::search_game_library(conn, None, Some(developer), &[], filter, offset, limit)
    }
}

#[cfg(test)]
mod tests {
    use super::super::MetadataDb;
    use super::super::tests::*;
    use super::SearchFilter;

    #[test]
    fn genre_enrichment_fills_empty_genre_from_launchbox() {
        let (mut conn, _dir) = open_temp_db();

        MetadataDb::bulk_upsert(
            &mut conn,
            &[(
                "sega_smd".into(),
                "Sonic.md".into(),
                make_metadata_with_genre("Platform"),
            )],
        )
        .unwrap();

        MetadataDb::save_system_entries(
            &mut conn,
            "sega_smd",
            &[make_game_entry("sega_smd", "Sonic.md", false)],
            None,
        )
        .unwrap();

        MetadataDb::update_box_art_genre_rating(
            &mut conn,
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

        let roms = MetadataDb::load_system_entries(&conn, "sega_smd").unwrap();
        assert_eq!(roms[0].genre.as_deref(), Some("Platform"));
    }

    #[test]
    fn genre_enrichment_does_not_overwrite_existing_genre() {
        let (mut conn, _dir) = open_temp_db();

        MetadataDb::save_system_entries(
            &mut conn,
            "sega_smd",
            &[make_game_entry_with_genre(
                "sega_smd", "Sonic.md", "Shooter",
            )],
            None,
        )
        .unwrap();

        MetadataDb::update_box_art_genre_rating(
            &mut conn,
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

        let roms = MetadataDb::load_system_entries(&conn, "sega_smd").unwrap();
        assert_eq!(roms[0].genre.as_deref(), Some("Shooter"));
    }

    #[test]
    fn genre_enrichment_mixed_empty_and_existing() {
        let (mut conn, _dir) = open_temp_db();

        MetadataDb::save_system_entries(
            &mut conn,
            "sega_smd",
            &[
                make_game_entry_with_genre("sega_smd", "Sonic.md", "Shooter"),
                make_game_entry("sega_smd", "Streets.md", false),
                make_game_entry("sega_smd", "Columns.md", false),
            ],
            None,
        )
        .unwrap();

        MetadataDb::update_box_art_genre_rating(
            &mut conn,
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

        let roms = MetadataDb::load_system_entries(&conn, "sega_smd").unwrap();
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
        let (mut conn, _dir) = open_temp_db();

        let mut with_art = make_game_entry("snes", "Mario.sfc", false);
        with_art.box_art_url = Some("/img/mario.png".into());
        let without_art = make_game_entry("snes", "Zelda.sfc", false);

        MetadataDb::save_system_entries(&mut conn, "snes", &[with_art, without_art], None).unwrap();

        let thumbs = MetadataDb::thumbnails_per_system(&conn).unwrap();
        assert_eq!(thumbs.len(), 1);
        assert_eq!(thumbs[0], ("snes".into(), 1));
    }

    #[test]
    fn thumbnails_per_system_empty_library_returns_empty() {
        let (mut conn, _dir) = open_temp_db();
        let thumbs = MetadataDb::thumbnails_per_system(&conn).unwrap();
        assert!(thumbs.is_empty());
    }

    #[test]
    fn thumbnails_per_system_multiple_systems() {
        let (mut conn, _dir) = open_temp_db();

        let mut snes_game = make_game_entry("snes", "Mario.sfc", false);
        snes_game.box_art_url = Some("/img/mario.png".into());
        let mut gba_game1 = make_game_entry("gba", "Metroid.gba", false);
        gba_game1.box_art_url = Some("/img/metroid.png".into());
        let mut gba_game2 = make_game_entry("gba", "Zelda.gba", false);
        gba_game2.box_art_url = Some("/img/zelda.png".into());
        let gba_game3 = make_game_entry("gba", "NoArt.gba", false);

        MetadataDb::save_system_entries(&mut conn, "snes", &[snes_game], None).unwrap();
        MetadataDb::save_system_entries(&mut conn, "gba", &[gba_game1, gba_game2, gba_game3], None)
            .unwrap();

        let thumbs = MetadataDb::thumbnails_per_system(&conn).unwrap();
        let snes = thumbs.iter().find(|(s, _)| s == "snes").unwrap();
        let gba = thumbs.iter().find(|(s, _)| s == "gba").unwrap();
        assert_eq!(snes.1, 1);
        assert_eq!(gba.1, 2);
    }

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
        let (mut conn, _dir) = open_temp_db();
        MetadataDb::save_system_entries(
            &mut conn,
            "arcade_fbneo",
            &[
                make_game_entry_with_developer("arcade_fbneo", "kof97.zip", "SNK", "KOF 97"),
                make_game_entry_with_developer("arcade_fbneo", "kof98.zip", "SNK", "KOF 98"),
                make_game_entry_with_developer(
                    "arcade_fbneo",
                    "fatfury2.zip",
                    "SNK",
                    "Fatal Fury 2",
                ),
                make_game_entry_with_developer(
                    "arcade_fbneo",
                    "samsho5.zip",
                    "SNK Playmore",
                    "Samurai Shodown V",
                ),
                make_game_entry_with_developer(
                    "arcade_fbneo",
                    "samsho6.zip",
                    "SNK Playmore",
                    "Samurai Shodown VI",
                ),
                make_game_entry_with_developer(
                    "arcade_fbneo",
                    "svc.zip",
                    "Capcom / SNK",
                    "SVC Chaos",
                ),
            ],
            None,
        )
        .unwrap();

        let matches = MetadataDb::find_developer_matches(&conn, "snk").unwrap();
        assert_eq!(matches.len(), 3);
        assert_eq!(matches[0].0, "SNK");
        assert_eq!(matches[0].1, 3);
        assert_eq!(matches[1].0, "SNK Playmore");
        assert_eq!(matches[1].1, 2);
        assert_eq!(matches[2].0, "Capcom / SNK");
        assert_eq!(matches[2].1, 1);
    }

    #[test]
    fn find_developer_matches_no_match_returns_empty() {
        let (mut conn, _dir) = open_temp_db();
        MetadataDb::save_system_entries(
            &mut conn,
            "snes",
            &[make_game_entry_with_developer(
                "snes",
                "Mario.sfc",
                "Nintendo",
                "Mario",
            )],
            None,
        )
        .unwrap();
        let matches = MetadataDb::find_developer_matches(&conn, "capcom").unwrap();
        assert!(matches.is_empty());
    }

    #[test]
    fn find_developer_matches_single_match() {
        let (mut conn, _dir) = open_temp_db();
        MetadataDb::save_system_entries(
            &mut conn,
            "snes",
            &[
                make_game_entry_with_developer("snes", "MegaManX.sfc", "Capcom", "Mega Man X"),
                make_game_entry_with_developer("snes", "BoF.sfc", "Capcom", "Breath of Fire"),
            ],
            None,
        )
        .unwrap();
        let matches = MetadataDb::find_developer_matches(&conn, "capcom").unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, "Capcom");
        assert_eq!(matches[0].1, 2);
    }

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

    #[test]
    fn developer_games_empty_genre_returns_all() {
        let (mut conn, _dir) = open_temp_db();
        MetadataDb::save_system_entries(
            &mut conn,
            "snes",
            &[
                make_dev_entry(
                    "snes",
                    "MegaManX.sfc",
                    "Capcom",
                    "Mega Man X",
                    "us",
                    Some("Action"),
                    None,
                ),
                make_dev_entry(
                    "snes",
                    "BoF.sfc",
                    "Capcom",
                    "Breath of Fire",
                    "us",
                    Some("RPG"),
                    None,
                ),
            ],
            None,
        )
        .unwrap();
        let filters = super::SearchFilter::default();
        let (entries, total) =
            MetadataDb::developer_games(&conn, "Capcom", &filters, 0, 50).unwrap();
        assert_eq!(total, 2);
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn developer_games_specific_genre() {
        let (mut conn, _dir) = open_temp_db();
        MetadataDb::save_system_entries(
            &mut conn,
            "snes",
            &[
                make_dev_entry(
                    "snes",
                    "MegaManX.sfc",
                    "Capcom",
                    "Mega Man X",
                    "us",
                    Some("Action"),
                    None,
                ),
                make_dev_entry(
                    "snes",
                    "BoF.sfc",
                    "Capcom",
                    "Breath of Fire",
                    "us",
                    Some("RPG"),
                    None,
                ),
                make_dev_entry(
                    "snes",
                    "SF2.sfc",
                    "Capcom",
                    "Street Fighter II",
                    "us",
                    Some("Fighting"),
                    None,
                ),
            ],
            None,
        )
        .unwrap();
        let filters = super::SearchFilter {
            genre: "Action",
            ..Default::default()
        };
        let (entries, total) =
            MetadataDb::developer_games(&conn, "Capcom", &filters, 0, 50).unwrap();
        assert_eq!(total, 1);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].base_title, "Mega Man X");
    }

    #[test]
    fn developer_games_system_and_genre_combined() {
        let (mut conn, _dir) = open_temp_db();
        MetadataDb::save_system_entries(
            &mut conn,
            "snes",
            &[
                make_dev_entry(
                    "snes",
                    "MegaManX.sfc",
                    "Capcom",
                    "Mega Man X",
                    "us",
                    Some("Action"),
                    None,
                ),
                make_dev_entry(
                    "snes",
                    "BoF.sfc",
                    "Capcom",
                    "Breath of Fire",
                    "us",
                    Some("RPG"),
                    None,
                ),
            ],
            None,
        )
        .unwrap();
        MetadataDb::save_system_entries(
            &mut conn,
            "gba",
            &[make_dev_entry(
                "gba",
                "MegaManZero.gba",
                "Capcom",
                "Mega Man Zero",
                "us",
                Some("Action"),
                None,
            )],
            None,
        )
        .unwrap();
        let filters = super::SearchFilter {
            genre: "Action",
            ..Default::default()
        };
        // Use search_game_library directly to combine system + developer filters.
        let (entries, total) = MetadataDb::search_game_library(
            &conn,
            Some("snes"),
            Some("Capcom"),
            &[],
            &filters,
            0,
            50,
        )
        .unwrap();
        assert_eq!(total, 1);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].base_title, "Mega Man X");
    }

    #[test]
    fn developer_games_case_insensitive() {
        let (mut conn, _dir) = open_temp_db();
        MetadataDb::save_system_entries(
            &mut conn,
            "snes",
            &[make_dev_entry(
                "snes",
                "SF2.sfc",
                "Capcom",
                "Street Fighter II",
                "us",
                None,
                None,
            )],
            None,
        )
        .unwrap();
        let filters = super::SearchFilter::default();
        // Query with different case should still match.
        let (entries, total) =
            MetadataDb::developer_games(&conn, "capcom", &filters, 0, 50).unwrap();
        assert_eq!(total, 1);
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn developer_games_offset_beyond_total() {
        let (mut conn, _dir) = open_temp_db();
        MetadataDb::save_system_entries(
            &mut conn,
            "snes",
            &[
                make_dev_entry(
                    "snes",
                    "MegaManX.sfc",
                    "Capcom",
                    "Mega Man X",
                    "us",
                    None,
                    None,
                ),
                make_dev_entry(
                    "snes",
                    "BoF.sfc",
                    "Capcom",
                    "Breath of Fire",
                    "us",
                    None,
                    None,
                ),
            ],
            None,
        )
        .unwrap();
        let filters = super::SearchFilter::default();
        let (entries, total) =
            MetadataDb::developer_games(&conn, "Capcom", &filters, 100, 50).unwrap();
        assert_eq!(total, 2);
        assert!(entries.is_empty());
    }

    #[test]
    fn developer_games_pagination() {
        let (mut conn, _dir) = open_temp_db();
        MetadataDb::save_system_entries(
            &mut conn,
            "snes",
            &[
                make_dev_entry("snes", "A.sfc", "Capcom", "Game A", "us", None, None),
                make_dev_entry("snes", "B.sfc", "Capcom", "Game B", "us", None, None),
                make_dev_entry("snes", "C.sfc", "Capcom", "Game C", "us", None, None),
            ],
            None,
        )
        .unwrap();
        let filters = super::SearchFilter::default();
        let (entries, total) =
            MetadataDb::developer_games(&conn, "Capcom", &filters, 0, 2).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(total, 3);
        let (entries, _) = MetadataDb::developer_games(&conn, "Capcom", &filters, 2, 2).unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn developer_games_hide_hacks_and_clones() {
        let (mut conn, _dir) = open_temp_db();
        MetadataDb::save_system_entries(
            &mut conn,
            "snes",
            &[
                make_dev_entry(
                    "snes",
                    "SF2.sfc",
                    "Capcom",
                    "Street Fighter II",
                    "us",
                    None,
                    None,
                ),
                make_dev_entry_hack("snes", "SF2-hack.sfc", "Capcom", "Street Fighter II Hack"),
                make_dev_entry_clone("snes", "SF2-clone.sfc", "Capcom", "Street Fighter II Clone"),
            ],
            None,
        )
        .unwrap();
        let filters = super::SearchFilter {
            hide_hacks: true,
            hide_clones: true,
            ..Default::default()
        };
        let (entries, total) =
            MetadataDb::developer_games(&conn, "Capcom", &filters, 0, 50).unwrap();
        assert_eq!(total, 1);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].base_title, "Street Fighter II");
    }

    #[test]
    fn developer_games_multiplayer_only() {
        let (mut conn, _dir) = open_temp_db();
        MetadataDb::save_system_entries(
            &mut conn,
            "snes",
            &[
                make_dev_entry_multiplayer("snes", "SF2.sfc", "Capcom", "Street Fighter II", 2),
                make_dev_entry(
                    "snes",
                    "MegaManX.sfc",
                    "Capcom",
                    "Mega Man X",
                    "us",
                    None,
                    None,
                ),
            ],
            None,
        )
        .unwrap();
        let filters = super::SearchFilter {
            multiplayer_only: true,
            ..Default::default()
        };
        let (entries, total) =
            MetadataDb::developer_games(&conn, "Capcom", &filters, 0, 50).unwrap();
        assert_eq!(total, 1);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].base_title, "Street Fighter II");
    }

    #[test]
    fn games_by_developer_deduplicates_across_systems() {
        let (mut conn, _dir) = open_temp_db();
        MetadataDb::save_system_entries(
            &mut conn,
            "snes",
            &[make_dev_entry(
                "snes",
                "SF2-snes.sfc",
                "Capcom",
                "Street Fighter II",
                "us",
                None,
                None,
            )],
            None,
        )
        .unwrap();
        MetadataDb::save_system_entries(
            &mut conn,
            "sega_smd",
            &[make_dev_entry(
                "sega_smd",
                "SF2-md.md",
                "Capcom",
                "Street Fighter II",
                "us",
                None,
                None,
            )],
            None,
        )
        .unwrap();
        let results = MetadataDb::games_by_developer(&conn, "Capcom", 50, "us", "").unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn games_by_developer_prefers_entry_with_box_art() {
        let (mut conn, _dir) = open_temp_db();
        MetadataDb::save_system_entries(
            &mut conn,
            "snes",
            &[
                make_dev_entry(
                    "snes",
                    "SF2-noart.sfc",
                    "Capcom",
                    "Street Fighter II",
                    "us",
                    None,
                    None,
                ),
                make_dev_entry(
                    "snes",
                    "SF2-art.sfc",
                    "Capcom",
                    "Street Fighter II",
                    "us",
                    None,
                    Some("/img/sf2.png"),
                ),
            ],
            None,
        )
        .unwrap();
        let results = MetadataDb::games_by_developer(&conn, "Capcom", 50, "us", "").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].box_art_url.as_deref(), Some("/img/sf2.png"));
    }

    #[test]
    fn games_by_developer_excludes_clones_and_hacks() {
        let (mut conn, _dir) = open_temp_db();
        MetadataDb::save_system_entries(
            &mut conn,
            "snes",
            &[
                make_dev_entry(
                    "snes",
                    "SF2.sfc",
                    "Capcom",
                    "Street Fighter II",
                    "us",
                    None,
                    None,
                ),
                make_dev_entry_hack("snes", "SF2-hack.sfc", "Capcom", "SF2 Hack"),
                make_dev_entry_clone("snes", "SF2-clone.sfc", "Capcom", "SF2 Clone"),
            ],
            None,
        )
        .unwrap();
        let results = MetadataDb::games_by_developer(&conn, "Capcom", 50, "us", "").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].base_title, "Street Fighter II");
    }

    #[test]
    fn games_by_developer_prefers_user_region() {
        let (mut conn, _dir) = open_temp_db();
        MetadataDb::save_system_entries(
            &mut conn,
            "snes",
            &[
                make_dev_entry(
                    "snes",
                    "SF2-jp.sfc",
                    "Capcom",
                    "Street Fighter II",
                    "japan",
                    None,
                    None,
                ),
                make_dev_entry(
                    "snes",
                    "SF2-eu.sfc",
                    "Capcom",
                    "Street Fighter II",
                    "europe",
                    None,
                    None,
                ),
            ],
            None,
        )
        .unwrap();
        let results =
            MetadataDb::games_by_developer(&conn, "Capcom", 50, "europe", "japan").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].region, "europe");
    }

    // ── count_system_entries + load_system_entries_page ───────────────

    #[test]
    fn count_system_entries_empty() {
        let (mut conn, _dir) = open_temp_db();
        let count = MetadataDb::count_system_entries(&conn, "snes").unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn count_system_entries_returns_correct_count() {
        let (mut conn, _dir) = open_temp_db();
        MetadataDb::save_system_entries(
            &mut conn,
            "snes",
            &[
                make_game_entry("snes", "Mario.sfc", false),
                make_game_entry("snes", "Zelda.sfc", false),
                make_game_entry("snes", "Metroid.sfc", false),
            ],
            None,
        )
        .unwrap();

        assert_eq!(MetadataDb::count_system_entries(&conn, "snes").unwrap(), 3);
        // Different system should return 0.
        assert_eq!(MetadataDb::count_system_entries(&conn, "gba").unwrap(), 0);
    }

    #[test]
    fn load_system_entries_page_returns_correct_page() {
        let (mut conn, _dir) = open_temp_db();

        // Insert entries with explicit display names for predictable sort order.
        let entries: Vec<super::super::GameEntry> = ["Alpha", "Bravo", "Charlie", "Delta", "Echo"]
            .iter()
            .map(|name| super::super::GameEntry {
                display_name: Some(name.to_string()),
                ..make_game_entry("snes", &format!("{name}.sfc"), false)
            })
            .collect();
        MetadataDb::save_system_entries(&mut conn, "snes", &entries, None).unwrap();

        // First page: offset=0, limit=2 → Alpha, Bravo
        let page1 = MetadataDb::load_system_entries_page(&conn, "snes", 0, 2).unwrap();
        assert_eq!(page1.len(), 2);
        assert_eq!(page1[0].display_name.as_deref(), Some("Alpha"));
        assert_eq!(page1[1].display_name.as_deref(), Some("Bravo"));

        // Second page: offset=2, limit=2 → Charlie, Delta
        let page2 = MetadataDb::load_system_entries_page(&conn, "snes", 2, 2).unwrap();
        assert_eq!(page2.len(), 2);
        assert_eq!(page2[0].display_name.as_deref(), Some("Charlie"));
        assert_eq!(page2[1].display_name.as_deref(), Some("Delta"));

        // Third page: offset=4, limit=2 → Echo (partial page)
        let page3 = MetadataDb::load_system_entries_page(&conn, "snes", 4, 2).unwrap();
        assert_eq!(page3.len(), 1);
        assert_eq!(page3[0].display_name.as_deref(), Some("Echo"));

        // Beyond range: offset=5, limit=2 → empty
        let page4 = MetadataDb::load_system_entries_page(&conn, "snes", 5, 2).unwrap();
        assert!(page4.is_empty());
    }

    #[test]
    fn load_system_entries_page_case_insensitive_sort() {
        let (mut conn, _dir) = open_temp_db();

        // Mixed case: should sort case-insensitively.
        let entries: Vec<super::super::GameEntry> = ["zebra", "Alpha", "BRAVO"]
            .iter()
            .map(|name| super::super::GameEntry {
                display_name: Some(name.to_string()),
                ..make_game_entry("snes", &format!("{name}.sfc"), false)
            })
            .collect();
        MetadataDb::save_system_entries(&mut conn, "snes", &entries, None).unwrap();

        let page = MetadataDb::load_system_entries_page(&conn, "snes", 0, 10).unwrap();
        assert_eq!(page.len(), 3);
        assert_eq!(page[0].display_name.as_deref(), Some("Alpha"));
        assert_eq!(page[1].display_name.as_deref(), Some("BRAVO"));
        assert_eq!(page[2].display_name.as_deref(), Some("zebra"));
    }

    #[test]
    fn load_system_entries_page_falls_back_to_filename_when_no_display_name() {
        let (mut conn, _dir) = open_temp_db();

        // Entry without display_name should sort by rom_filename.
        let entries = vec![
            super::super::GameEntry {
                display_name: Some("Zelda".to_string()),
                ..make_game_entry("snes", "zelda.sfc", false)
            },
            make_game_entry("snes", "alpha.sfc", false), // No display_name → uses filename
        ];
        MetadataDb::save_system_entries(&mut conn, "snes", &entries, None).unwrap();

        let page = MetadataDb::load_system_entries_page(&conn, "snes", 0, 10).unwrap();
        assert_eq!(page.len(), 2);
        // "alpha.sfc" < "Zelda" case-insensitively
        assert_eq!(page[0].rom_filename, "alpha.sfc");
        assert_eq!(page[1].display_name.as_deref(), Some("Zelda"));
    }

    // ── build_search_text tests ────────────────────────────────────────

    #[test]
    fn build_search_text_with_display_name() {
        let text = super::build_search_text(
            Some("Super Mario World"),
            "Super Mario World (USA).sfc",
            "Super Mario World",
            "",
            None,
        );
        assert_eq!(
            text,
            "super mario world|super mario world (usa).sfc|super mario world"
        );
    }

    #[test]
    fn build_search_text_without_display_name_falls_back_to_filename() {
        let text = super::build_search_text(None, "sonic.md", "Sonic", "", None);
        assert_eq!(text, "sonic.md|sonic.md|sonic");
    }

    #[test]
    fn build_search_text_lowercases_all_fields() {
        let text = super::build_search_text(
            Some("Sonic The Hedgehog"),
            "Sonic The Hedgehog (USA).md",
            "Sonic The Hedgehog",
            "",
            None,
        );
        assert!(
            text.chars()
                .all(|c| !c.is_uppercase() || !c.is_ascii_alphabetic())
        );
    }

    #[test]
    fn build_search_text_empty_base_title() {
        let text = super::build_search_text(Some("Game"), "game.rom", "", "", None);
        assert_eq!(text, "game|game.rom|");
    }

    #[test]
    fn build_search_text_with_developer() {
        let text = super::build_search_text(Some("Game"), "game.rom", "game", "Imagine", None);
        assert_eq!(text, "game|game.rom|game|imagine");
    }

    #[test]
    fn build_search_text_with_year() {
        let text = super::build_search_text(Some("Game"), "game.rom", "game", "", Some(1987));
        assert_eq!(text, "game|game.rom|game|1987");
    }

    #[test]
    fn build_search_text_with_developer_and_year() {
        let text =
            super::build_search_text(Some("Game"), "game.rom", "game", "Imagine", Some(1987));
        assert_eq!(text, "game|game.rom|game|imagine|1987");
    }

    // ── search_game_library tests ──────────────────────────────────────

    fn insert_test_library(conn: &mut rusqlite::Connection) {
        let snes_entries = vec![
            super::super::GameEntry {
                display_name: Some("Super Mario World".to_string()),
                base_title: "Super Mario World".to_string(),
                genre_group: "Platform".to_string(),
                players: Some(2),
                rating: Some(4.5),
                ..make_game_entry("snes", "Super Mario World (USA).sfc", false)
            },
            super::super::GameEntry {
                display_name: Some("Super Mario Kart".to_string()),
                base_title: "Super Mario Kart".to_string(),
                genre_group: "Racing".to_string(),
                players: Some(2),
                rating: Some(4.0),
                ..make_game_entry("snes", "Super Mario Kart (USA).sfc", false)
            },
            super::super::GameEntry {
                display_name: Some("Street Fighter II Turbo".to_string()),
                base_title: "Street Fighter II Turbo".to_string(),
                genre_group: "Fighting".to_string(),
                players: Some(2),
                rating: Some(4.3),
                is_hack: true,
                ..make_game_entry("snes", "Street Fighter II Turbo (Hack).sfc", false)
            },
            super::super::GameEntry {
                display_name: Some("Zelda - A Link to the Past".to_string()),
                base_title: "Zelda".to_string(),
                genre_group: "Adventure".to_string(),
                players: Some(1),
                rating: Some(4.8),
                is_translation: true,
                ..make_game_entry("snes", "Zelda - A Link to the Past (T-Es).sfc", false)
            },
        ];
        let smd_entries = vec![super::super::GameEntry {
            display_name: Some("Sonic the Hedgehog".to_string()),
            base_title: "Sonic the Hedgehog".to_string(),
            genre_group: "Platform".to_string(),
            players: Some(1),
            rating: Some(4.2),
            ..make_game_entry("sega_smd", "Sonic the Hedgehog (USA).md", false)
        }];
        // save_system_entries replaces all entries for a system, so batch per system.
        MetadataDb::save_system_entries(&mut *conn, "snes", &snes_entries, None).unwrap();
        MetadataDb::save_system_entries(&mut *conn, "sega_smd", &smd_entries, None).unwrap();
    }

    #[test]
    fn search_exact_match() {
        let (mut conn, _dir) = open_temp_db();
        insert_test_library(&mut conn);

        let (results, _total) = MetadataDb::search_game_library(
            &conn,
            None,
            None,
            &["sonic".to_string()],
            &SearchFilter::default(),
            0,
            usize::MAX,
        )
        .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].rom_filename, "Sonic the Hedgehog (USA).md");
    }

    #[test]
    fn search_multi_word() {
        let (mut conn, _dir) = open_temp_db();
        insert_test_library(&mut conn);

        let (results, _total) = MetadataDb::search_game_library(
            &conn,
            None,
            None,
            &["super".to_string(), "mario".to_string()],
            &SearchFilter::default(),
            0,
            usize::MAX,
        )
        .unwrap();
        // Should find both "Super Mario World" and "Super Mario Kart"
        assert_eq!(results.len(), 2);
        let filenames: Vec<&str> = results.iter().map(|r| r.rom_filename.as_str()).collect();
        assert!(filenames.contains(&"Super Mario World (USA).sfc"));
        assert!(filenames.contains(&"Super Mario Kart (USA).sfc"));
    }

    #[test]
    fn search_contains_match() {
        let (mut conn, _dir) = open_temp_db();
        insert_test_library(&mut conn);

        let (results, _total) = MetadataDb::search_game_library(
            &conn,
            None,
            None,
            &["mario".to_string()],
            &SearchFilter::default(),
            0,
            usize::MAX,
        )
        .unwrap();
        // "mario" appears in both Super Mario entries
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn search_with_hide_hacks_filter() {
        let (mut conn, _dir) = open_temp_db();
        insert_test_library(&mut conn);

        let (results, _total) = MetadataDb::search_game_library(
            &conn,
            None,
            None,
            &["street".to_string()],
            &SearchFilter {
                hide_hacks: true,
                ..SearchFilter::default()
            },
            0,
            usize::MAX,
        )
        .unwrap();
        // "Street Fighter II Turbo (Hack)" should be filtered out
        assert!(results.is_empty());
    }

    #[test]
    fn search_with_hide_translations_filter() {
        let (mut conn, _dir) = open_temp_db();
        insert_test_library(&mut conn);

        let (results, _total) = MetadataDb::search_game_library(
            &conn,
            None,
            None,
            &["zelda".to_string()],
            &SearchFilter {
                hide_translations: true,
                ..SearchFilter::default()
            },
            0,
            usize::MAX,
        )
        .unwrap();
        // Zelda translation should be filtered out
        assert!(results.is_empty());
    }

    #[test]
    fn search_with_genre_filter() {
        let (mut conn, _dir) = open_temp_db();
        insert_test_library(&mut conn);

        let (results, _total) = MetadataDb::search_game_library(
            &conn,
            None,
            None,
            &["super".to_string()],
            &SearchFilter {
                genre: "Racing",
                ..SearchFilter::default()
            },
            0,
            usize::MAX,
        )
        .unwrap();
        // Only Super Mario Kart should match (genre = Racing)
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].rom_filename, "Super Mario Kart (USA).sfc");
    }

    #[test]
    fn search_with_multiplayer_filter() {
        let (mut conn, _dir) = open_temp_db();
        insert_test_library(&mut conn);

        let (results, _total) = MetadataDb::search_game_library(
            &conn,
            None,
            None,
            &[],
            &SearchFilter {
                genre: "Platform",
                multiplayer_only: true,
                ..SearchFilter::default()
            },
            0,
            usize::MAX,
        )
        .unwrap();
        // Only Super Mario World (platform + 2 players). Sonic is platform but 1 player.
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].rom_filename, "Super Mario World (USA).sfc");
    }

    #[test]
    fn search_with_min_rating_filter() {
        let (mut conn, _dir) = open_temp_db();
        insert_test_library(&mut conn);

        let (results, _total) = MetadataDb::search_game_library(
            &conn,
            None,
            None,
            &[],
            &SearchFilter {
                min_rating: Some(4.5),
                ..SearchFilter::default()
            },
            0,
            usize::MAX,
        )
        .unwrap();
        // Only entries with rating >= 4.5
        for r in &results {
            assert!(r.rating.unwrap() >= 4.5);
        }
    }

    #[test]
    fn search_empty_words_returns_all() {
        let (mut conn, _dir) = open_temp_db();
        insert_test_library(&mut conn);

        let (results, _total) = MetadataDb::search_game_library(
            &conn,
            None,
            None,
            &[],
            &SearchFilter::default(),
            0,
            usize::MAX,
        )
        .unwrap();
        // Should return all entries (no text filter)
        assert!(results.len() >= 4);
    }

    #[test]
    fn search_cross_system() {
        let (mut conn, _dir) = open_temp_db();
        insert_test_library(&mut conn);

        // "hedgehog" should find the sega_smd entry
        let (results, _total) = MetadataDb::search_game_library(
            &conn,
            None,
            None,
            &["hedgehog".to_string()],
            &SearchFilter::default(),
            0,
            usize::MAX,
        )
        .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].system, "sega_smd");
    }

    #[test]
    fn search_escapes_sql_wildcards() {
        let (mut conn, _dir) = open_temp_db();
        insert_test_library(&mut conn);

        // Searching for "%" or "_" should not match everything
        let (results, _total) = MetadataDb::search_game_library(
            &conn,
            None,
            None,
            &["%".to_string()],
            &SearchFilter::default(),
            0,
            usize::MAX,
        )
        .unwrap();
        assert!(results.is_empty());

        let (results, _total) = MetadataDb::search_game_library(
            &conn,
            None,
            None,
            &["_".to_string()],
            &SearchFilter::default(),
            0,
            usize::MAX,
        )
        .unwrap();
        assert!(results.is_empty());
    }

    // ── top_genre_for_filenames ──────────────────────────────────────

    #[test]
    fn top_genre_for_filenames_returns_most_common() {
        let (mut conn, _dir) = open_temp_db();
        // Entries need base_title set — the query resolves filenames to base_titles
        // via a subquery and then aggregates genre_group by base_title.
        let mut mario = make_game_entry_with_genre("snes", "mario.sfc", "Platform");
        mario.base_title = "Super Mario World".into();
        let mut zelda = make_game_entry_with_genre("snes", "zelda.sfc", "Action / RPG");
        zelda.base_title = "The Legend of Zelda".into();
        let mut metroid = make_game_entry_with_genre("snes", "metroid.sfc", "Platform");
        metroid.base_title = "Super Metroid".into();
        MetadataDb::save_system_entries(&mut conn, "snes", &[mario, zelda, metroid], None).unwrap();

        let result = MetadataDb::top_genre_for_filenames(
            &conn,
            "snes",
            &["mario.sfc", "zelda.sfc", "metroid.sfc"],
        )
        .unwrap();
        // Platform appears twice, Action / RPG once.
        assert_eq!(result.as_deref(), Some("Platform"));
    }

    #[test]
    fn top_genre_for_filenames_empty_input() {
        let (conn, _dir) = open_temp_db();
        let result = MetadataDb::top_genre_for_filenames(&conn, "snes", &[]).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn top_genre_for_filenames_no_matches() {
        let (mut conn, _dir) = open_temp_db();
        MetadataDb::save_system_entries(
            &mut conn,
            "snes",
            &[make_game_entry("snes", "mario.sfc", false)],
            None,
        )
        .unwrap();
        // "mario.sfc" has no genre_group set and no base_title.
        let result = MetadataDb::top_genre_for_filenames(&conn, "snes", &["mario.sfc"]).unwrap();
        assert_eq!(result, None);
    }

    // ── lookup_game_entries ─────────────────────────────────────────

    #[test]
    fn lookup_game_entries_returns_matching() {
        let (mut conn, _dir) = open_temp_db();
        MetadataDb::save_system_entries(
            &mut conn,
            "snes",
            &[
                make_game_entry("snes", "mario.sfc", false),
                make_game_entry("snes", "zelda.sfc", false),
            ],
            None,
        )
        .unwrap();

        let keys = vec![
            ("snes".to_string(), "mario.sfc".to_string()),
            ("snes".to_string(), "zelda.sfc".to_string()),
        ];
        let result = MetadataDb::lookup_game_entries(&conn, &keys).unwrap();
        assert_eq!(result.len(), 2);
        assert!(result.contains_key(&("snes".into(), "mario.sfc".into())));
        assert!(result.contains_key(&("snes".into(), "zelda.sfc".into())));
    }

    #[test]
    fn lookup_game_entries_empty_keys() {
        let (conn, _dir) = open_temp_db();
        let keys: Vec<(String, String)> = vec![];
        let result = MetadataDb::lookup_game_entries(&conn, &keys).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn lookup_game_entries_missing_entries() {
        let (mut conn, _dir) = open_temp_db();
        MetadataDb::save_system_entries(
            &mut conn,
            "snes",
            &[make_game_entry("snes", "mario.sfc", false)],
            None,
        )
        .unwrap();

        let keys = vec![
            ("snes".to_string(), "mario.sfc".to_string()),
            ("snes".to_string(), "nonexistent.sfc".to_string()),
        ];
        let result = MetadataDb::lookup_game_entries(&conn, &keys).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result.contains_key(&("snes".into(), "mario.sfc".into())));
    }

    #[test]
    fn lookup_game_entries_multi_system() {
        let (mut conn, _dir) = open_temp_db();
        MetadataDb::save_system_entries(
            &mut conn,
            "snes",
            &[make_game_entry("snes", "mario.sfc", false)],
            None,
        )
        .unwrap();
        MetadataDb::save_system_entries(
            &mut conn,
            "nes",
            &[make_game_entry("nes", "contra.nes", false)],
            None,
        )
        .unwrap();

        let keys = vec![
            ("snes".to_string(), "mario.sfc".to_string()),
            ("nes".to_string(), "contra.nes".to_string()),
        ];
        let result = MetadataDb::lookup_game_entries(&conn, &keys).unwrap();
        assert_eq!(result.len(), 2);
        assert!(result.contains_key(&("snes".into(), "mario.sfc".into())));
        assert!(result.contains_key(&("nes".into(), "contra.nes".into())));
    }

    #[test]
    fn search_filter_coop_only() {
        let (mut conn, _dir) = open_temp_db();

        let mut coop_game = make_game_entry("snes", "Contra.sfc", false);
        coop_game.cooperative = true;
        let solo_game1 = make_game_entry("snes", "Mario.sfc", false);
        let solo_game2 = make_game_entry("snes", "Zelda.sfc", false);

        MetadataDb::save_system_entries(
            &mut conn,
            "snes",
            &[coop_game, solo_game1, solo_game2],
            None,
        )
        .unwrap();

        let filter = SearchFilter {
            coop_only: true,
            ..SearchFilter::default()
        };
        let (entries, total) =
            MetadataDb::search_game_library(&conn, Some("snes"), None, &[], &filter, 0, 50)
                .unwrap();

        assert_eq!(total, 1);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].rom_filename, "Contra.sfc");
        assert!(entries[0].cooperative);
    }
}
