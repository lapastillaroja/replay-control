//! Operations on the `game_library` and `game_library_meta` tables.

use std::collections::{HashMap, HashSet};

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use replay_control_core::error::{Error, Result};
use replay_control_core::rom_tags::RegionPreference;
use replay_control_core::search_scoring::{search_score, split_into_words};

use super::{
    DpSql, GameEntry, LibraryDb, PhaseState, SystemMeta, ThumbnailDownloadJob, ThumbnailJobState,
    ThumbnailPhaseState, library_meta, unix_now,
};

/// SELECT columns for `game_library` queries that feed `row_to_game_entry()`.
///
/// The column order must match the positional indices in `row_to_game_entry()`.
const GAME_ENTRY_COLUMNS: &str = "\
    system, rom_filename, rom_path, display_name, base_title, series_key, \
    region, developer, genre, genre_group, rating, rating_count, players, \
    is_clone, is_m3u, is_translation, is_hack, is_special, \
    box_art_url, driver_status, size_bytes, crc32, hash_mtime, hash_size_bytes, hash_matched_name, \
    identity_state, release_date, release_precision, release_region_used, cooperative, \
    normalized_title, normalized_title_alt, board";

pub const DISCOVERY_SAVE_CHUNK_ROWS: usize = 200;

#[derive(Debug, Clone, Copy)]
pub struct DiscoveryFinalizeStats<'a> {
    pub dir_mtime_secs: Option<i64>,
    pub rom_count: usize,
    pub total_size: u64,
    pub scanned_at: i64,
    pub scan_fingerprint: Option<&'a str>,
}

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
    release_date: Option<&str>,
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
    if let Some(year) = release_date.and_then(super::year_from_release_date) {
        text.push('|');
        text.push_str(&year.to_string());
    }
    text
}

fn warn_slow_discovery_write(
    system: &str,
    scan_token: i64,
    rows: usize,
    elapsed: std::time::Duration,
) {
    if elapsed > std::time::Duration::from_secs(2) {
        tracing::warn!(
            "discovery.save slow chunk system={} scan_token={} rows={} writer_ms={}",
            system,
            scan_token,
            rows,
            elapsed.as_millis()
        );
    }
}

fn system_meta_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SystemMeta> {
    Ok(SystemMeta {
        system: row.get(0)?,
        dir_mtime_secs: row.get(1)?,
        scanned_at: row.get(2)?,
        rom_count: row.get::<_, i64>(3)? as usize,
        total_size_bytes: row.get::<_, i64>(4)? as u64,
        discovery_state: PhaseState::from_i64(row.get::<_, i64>(5)?),
        enrichment_state: PhaseState::from_i64(row.get::<_, i64>(6)?),
        thumbnail_state: ThumbnailPhaseState::from_i64(row.get::<_, i64>(7)?),
    })
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
    /// Restrict results to a single arcade board. Hits the partial index
    /// `idx_gl_board` for an indexed lookup; ignored when `None`.
    pub board: Option<replay_control_core::arcade_board::ArcadeBoard>,
}

impl LibraryDb {
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

    /// Resolve a library row whose `rom_path` is a prefix of the given
    /// relative rom path — the now-playing fallback when the API-reported
    /// game doesn't resolve by `(system, game_name)` exactly. Longest prefix
    /// wins so nested-subdir rows beat their parents.
    pub fn lookup_game_by_path_prefix(
        conn: &Connection,
        system: &str,
        rom_path: &str,
    ) -> Result<Option<GameEntry>> {
        let sql = format!(
            "SELECT {GAME_ENTRY_COLUMNS} FROM game_library \
             WHERE system = ?1 AND ?2 LIKE rom_path || '%' \
             ORDER BY LENGTH(rom_path) DESC LIMIT 1"
        );
        conn.prepare(&sql)
            .map_err(|e| Error::Other(format!("Prepare lookup_game_by_path_prefix: {e}")))?
            .query_row(params![system, rom_path], Self::row_to_game_entry)
            .optional()
            .map_err(|e| Error::Other(format!("Query lookup_game_by_path_prefix: {e}")))
    }

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

    /// Look up `genre` for a single ROM from `game_library`. Returns the
    /// stored value (which enrichment populates from catalog at scan-time
    /// and from LaunchBox at fill-empty time) or empty when nothing is set.
    pub fn rom_genre(conn: &Connection, system: &str, rom_filename: &str) -> Result<String> {
        use rusqlite::OptionalExtension;
        let genre: Option<Option<String>> = conn
            .query_row(
                "SELECT genre FROM game_library WHERE system = ?1 AND rom_filename = ?2",
                rusqlite::params![system, rom_filename],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| Error::Other(format!("rom_genre: {e}")))?;
        Ok(genre.flatten().unwrap_or_default())
    }

    /// All `(system, rom_filename) → rating` pairs across the library.
    /// Used by the favorites organizer to rank entries; reads from
    /// `game_library.rating` (which enrichment populates from LaunchBox).
    pub fn all_ratings(
        conn: &Connection,
    ) -> Result<std::collections::HashMap<(String, String), f64>> {
        let mut stmt = conn
            .prepare(
                "SELECT system, rom_filename, rating
                 FROM game_library
                 WHERE rating IS NOT NULL",
            )
            .map_err(|e| Error::Other(format!("all_ratings prepare: {e}")))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, f64>(2)?,
                ))
            })
            .map_err(|e| Error::Other(format!("all_ratings query: {e}")))?;
        let mut map = std::collections::HashMap::new();
        for r in rows.flatten() {
            map.insert((r.0, r.1), r.2);
        }
        Ok(map)
    }

    // ── Game Library (L2 persistent cache) ─────────────────────────────

    /// Save a system's game list to the game_library table.
    /// Reconciles the system with a durable scan token so unchanged ROM rows
    /// keep their derived child rows while stale ROMs are deleted only after
    /// every current row has been written.
    pub fn save_system_entries(
        conn: &mut Connection,
        system: &str,
        roms: &[GameEntry],
        dir_mtime_secs: Option<i64>,
    ) -> Result<()> {
        let mut seen = HashSet::new();
        let unique_roms: Vec<&GameEntry> = roms
            .iter()
            .filter(|rom| seen.insert(rom.rom_filename.as_str()))
            .collect();
        let total_size: u64 = unique_roms.iter().map(|r| r.size_bytes).sum();
        let now = unix_now();

        let first_chunk_len = unique_roms.len().min(DISCOVERY_SAVE_CHUNK_ROWS);
        let token = {
            let chunk_started = std::time::Instant::now();
            let tx = conn
                .transaction()
                .map_err(|e| Error::Other(format!("Transaction start failed: {e}")))?;
            let token = Self::allocate_scan_token(&tx)?;
            Self::mark_discovery_running(&tx, system, now)?;
            if first_chunk_len > 0 {
                Self::upsert_discovery_chunk(&tx, system, token, &unique_roms[..first_chunk_len])?;
            }
            tx.commit()
                .map_err(|e| Error::Other(format!("Transaction commit failed: {e}")))?;
            warn_slow_discovery_write(system, token, first_chunk_len, chunk_started.elapsed());
            token
        };

        for chunk in unique_roms[first_chunk_len..].chunks(DISCOVERY_SAVE_CHUNK_ROWS) {
            let chunk_started = std::time::Instant::now();
            let tx = conn
                .transaction()
                .map_err(|e| Error::Other(format!("Transaction start failed: {e}")))?;
            Self::upsert_discovery_chunk(&tx, system, token, chunk)?;
            tx.commit()
                .map_err(|e| Error::Other(format!("Transaction commit failed: {e}")))?;
            warn_slow_discovery_write(system, token, chunk.len(), chunk_started.elapsed());
        }

        let finalize_started = std::time::Instant::now();
        let tx = conn
            .transaction()
            .map_err(|e| Error::Other(format!("Transaction start failed: {e}")))?;
        Self::finalize_discovery(
            &tx,
            system,
            token,
            DiscoveryFinalizeStats {
                dir_mtime_secs,
                rom_count: unique_roms.len(),
                total_size,
                scanned_at: now,
                scan_fingerprint: None,
            },
        )?;
        tx.commit()
            .map_err(|e| Error::Other(format!("Transaction commit failed: {e}")))?;
        let finalize_elapsed = finalize_started.elapsed();
        if finalize_elapsed > std::time::Duration::from_secs(2) {
            tracing::warn!(
                "discovery.save slow finalize system={} scan_token={} rows={} finalize_ms={}",
                system,
                token,
                unique_roms.len(),
                finalize_elapsed.as_millis()
            );
        }

        Ok(())
    }

    pub fn begin_system_discovery(conn: &mut Connection, system: &str) -> Result<(i64, i64)> {
        let now = unix_now();
        let tx = conn
            .transaction()
            .map_err(|e| Error::Other(format!("Transaction start failed: {e}")))?;
        let token = Self::allocate_scan_token(&tx)?;
        Self::mark_discovery_running(&tx, system, now)?;
        tx.commit()
            .map_err(|e| Error::Other(format!("Transaction commit failed: {e}")))?;
        Ok((token, now))
    }

    pub fn save_system_entries_chunk(
        conn: &mut Connection,
        system: &str,
        scan_token: i64,
        roms: &[GameEntry],
    ) -> Result<usize> {
        if roms.is_empty() {
            return Ok(0);
        }

        let chunk_started = std::time::Instant::now();
        let tx = conn
            .transaction()
            .map_err(|e| Error::Other(format!("Transaction start failed: {e}")))?;
        let refs: Vec<&GameEntry> = roms.iter().collect();
        Self::upsert_discovery_chunk(&tx, system, scan_token, &refs)?;
        tx.commit()
            .map_err(|e| Error::Other(format!("Transaction commit failed: {e}")))?;
        warn_slow_discovery_write(system, scan_token, roms.len(), chunk_started.elapsed());
        Ok(roms.len())
    }

    pub fn finalize_system_discovery(
        conn: &mut Connection,
        system: &str,
        scan_token: i64,
        stats: DiscoveryFinalizeStats<'_>,
    ) -> Result<()> {
        let finalize_started = std::time::Instant::now();
        let tx = conn
            .transaction()
            .map_err(|e| Error::Other(format!("Transaction start failed: {e}")))?;
        Self::finalize_discovery(&tx, system, scan_token, stats)?;
        tx.commit()
            .map_err(|e| Error::Other(format!("Transaction commit failed: {e}")))?;
        let finalize_elapsed = finalize_started.elapsed();
        if finalize_elapsed > std::time::Duration::from_secs(2) {
            tracing::warn!(
                "discovery.save slow finalize system={} scan_token={} rows={} finalize_ms={}",
                system,
                scan_token,
                stats.rom_count,
                finalize_elapsed.as_millis()
            );
        }
        Ok(())
    }

    fn allocate_scan_token(tx: &Transaction<'_>) -> Result<i64> {
        tx.execute(
            "INSERT OR IGNORE INTO library_build_sequence (name, next_value)
             VALUES ('scan_token', 1)",
            [],
        )
        .map_err(|e| Error::Other(format!("Initialize scan token sequence: {e}")))?;
        let token = tx
            .query_row(
                "SELECT next_value
                 FROM library_build_sequence
                 WHERE name = 'scan_token'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|e| Error::Other(format!("Read scan token sequence: {e}")))?;
        tx.execute(
            "UPDATE library_build_sequence
             SET next_value = next_value + 1
             WHERE name = 'scan_token'",
            [],
        )
        .map_err(|e| Error::Other(format!("Advance scan token sequence: {e}")))?;
        Ok(token)
    }

    fn mark_discovery_running(tx: &Transaction<'_>, system: &str, now: i64) -> Result<()> {
        tx.execute(
            "INSERT INTO game_library_meta (
                system, dir_mtime_secs, scanned_at, rom_count, total_size_bytes,
                discovery_state, enrichment_state, thumbnail_state
             )
             VALUES (?1, NULL, ?2, 0, 0, ?3, ?4, ?5)
             ON CONFLICT(system) DO UPDATE SET
                discovery_state = excluded.discovery_state,
                scanned_at = excluded.scanned_at",
            params![
                system,
                now,
                PhaseState::Running.as_i64(),
                PhaseState::Pending.as_i64(),
                ThumbnailPhaseState::Pending.as_i64()
            ],
        )
        .map_err(|e| Error::Other(format!("Mark discovery running for {system}: {e}")))?;
        Ok(())
    }

    fn upsert_discovery_chunk(
        tx: &Transaction<'_>,
        system: &str,
        scan_token: i64,
        roms: &[&GameEntry],
    ) -> Result<()> {
        let mut stmt = tx
            .prepare(
                "INSERT INTO game_library (system, rom_filename, rom_path, display_name,
                 base_title, series_key, region, developer, search_text,
                 genre, genre_group, rating, rating_count, players,
                 is_clone, is_m3u, is_translation, is_hack, is_special,
                 box_art_url, driver_status, size_bytes, crc32, hash_mtime, hash_size_bytes, hash_matched_name,
                 scan_token, identity_state,
                 release_date, release_precision, release_region_used, cooperative,
                 normalized_title, normalized_title_alt, board)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                         ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27,
                         ?28, ?29, ?30, ?31, ?32, ?33, ?34, ?35)
                 ON CONFLICT(system, rom_filename) DO UPDATE SET
                    rom_path = excluded.rom_path,
                    display_name = excluded.display_name,
                    base_title = excluded.base_title,
                    series_key = excluded.series_key,
                    region = excluded.region,
                    developer = excluded.developer,
                    search_text = excluded.search_text,
                    genre = excluded.genre,
                    genre_group = excluded.genre_group,
                    rating = excluded.rating,
                    rating_count = excluded.rating_count,
                    players = excluded.players,
                    is_clone = excluded.is_clone,
                    is_m3u = excluded.is_m3u,
                    is_translation = excluded.is_translation,
                    is_hack = excluded.is_hack,
                    is_special = excluded.is_special,
                    box_art_url = excluded.box_art_url,
                    driver_status = excluded.driver_status,
                    size_bytes = excluded.size_bytes,
                    crc32 = excluded.crc32,
                    hash_mtime = excluded.hash_mtime,
                    hash_size_bytes = excluded.hash_size_bytes,
                    hash_matched_name = excluded.hash_matched_name,
                    scan_token = excluded.scan_token,
                    identity_state = excluded.identity_state,
                    release_date = excluded.release_date,
                    release_precision = excluded.release_precision,
                    release_region_used = excluded.release_region_used,
                    cooperative = excluded.cooperative,
                    normalized_title = excluded.normalized_title,
                    normalized_title_alt = excluded.normalized_title_alt,
                    board = excluded.board",
            )
            .map_err(|e| Error::Other(format!("Prepare game_library upsert: {e}")))?;

        for rom in roms {
            let search_text = build_search_text(
                rom.display_name.as_deref(),
                &rom.rom_filename,
                &rom.base_title,
                &rom.developer,
                rom.release_date.as_deref(),
            );
            stmt.execute(params![
                system,
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
                rom.hash_size_bytes.map(|s| s as i64),
                &rom.hash_matched_name,
                scan_token,
                rom.identity_state.as_i64(),
                &rom.release_date,
                rom.release_precision.map(DpSql),
                &rom.release_region_used,
                rom.cooperative,
                &rom.normalized_title,
                &rom.normalized_title_alt,
                rom.board.map(|b| b.as_tag()).unwrap_or_default(),
            ])
            .map_err(|e| Error::Other(format!("Upsert game_library failed: {e}")))?;
        }
        Ok(())
    }

    fn finalize_discovery(
        tx: &Transaction<'_>,
        system: &str,
        scan_token: i64,
        stats: DiscoveryFinalizeStats<'_>,
    ) -> Result<()> {
        tx.execute(
            "DELETE FROM game_detail_metadata
             WHERE system = ?1
               AND NOT EXISTS (
                   SELECT 1
                   FROM game_library current
                   WHERE current.system = game_detail_metadata.system
                     AND current.rom_filename = game_detail_metadata.rom_filename
                     AND current.scan_token = ?2
               )",
            params![system, scan_token],
        )
        .map_err(|e| Error::Other(format!("Delete stale game_detail_metadata failed: {e}")))?;

        tx.execute(
            "DELETE FROM game_library
             WHERE system = ?1
               AND (scan_token IS NULL OR scan_token != ?2)",
            params![system, scan_token],
        )
        .map_err(|e| Error::Other(format!("Delete stale game_library failed: {e}")))?;

        tx.execute(
            "INSERT INTO game_library_meta (
                system, dir_mtime_secs, scanned_at, rom_count, total_size_bytes,
                discovery_state, enrichment_state, thumbnail_state
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(system) DO UPDATE SET
                dir_mtime_secs = excluded.dir_mtime_secs,
                scanned_at = excluded.scanned_at,
                rom_count = excluded.rom_count,
                total_size_bytes = excluded.total_size_bytes,
                discovery_state = excluded.discovery_state,
                enrichment_state = excluded.enrichment_state,
                thumbnail_state = excluded.thumbnail_state",
            params![
                system,
                stats.dir_mtime_secs,
                stats.scanned_at,
                stats.rom_count as i64,
                stats.total_size as i64,
                PhaseState::Complete.as_i64(),
                PhaseState::Pending.as_i64(),
                ThumbnailPhaseState::Pending.as_i64()
            ],
        )
        .map_err(|e| Error::Other(format!("Upsert game_library_meta failed: {e}")))?;
        let fingerprint_key = library_meta::keys::discovery_fingerprint(system);
        library_meta::write_meta(tx, &fingerprint_key, stats.scan_fingerprint)?;
        Self::refresh_game_library_system_stats_state(tx, system, super::StatsRefreshState::Stale)?;
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
    /// Used by callers that have a known rom count + mtime independent of
    /// the per-system scan path.
    ///
    /// **Zero-overwrite protection.** A racy NFS / autofs / USB hot-plug scan
    /// can return rom_count=0 for a system that actually has games on disk.
    /// On UPDATE conflicts the SQL refuses to lower a non-zero rom_count to
    /// zero — the existing count is preserved instead. INSERTs into a fresh
    /// row are not affected. Explicit clears go through `clear_*` (DELETE).
    /// See `2026-04-29-nfs-startup-race-and-thumbnail-silent-failure.md`.
    ///
    /// Returns the rom_count that ended up in the row after the operation.
    /// Callers can compare against the input to detect when the protection
    /// fired and log a warning.
    pub fn save_system_meta(
        conn: &Connection,
        system: &str,
        dir_mtime_secs: Option<i64>,
        rom_count: usize,
        total_size_bytes: u64,
    ) -> Result<usize> {
        let now = unix_now();
        let final_count: i64 = conn
            .query_row(
                "INSERT INTO game_library_meta (
                    system, dir_mtime_secs, scanned_at, rom_count, total_size_bytes,
                    discovery_state, enrichment_state, thumbnail_state
                 )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(system) DO UPDATE SET
                    dir_mtime_secs = excluded.dir_mtime_secs,
                    scanned_at = excluded.scanned_at,
                    rom_count = CASE
                        WHEN excluded.rom_count = 0 AND game_library_meta.rom_count > 0
                            THEN game_library_meta.rom_count
                        ELSE excluded.rom_count
                    END,
                    total_size_bytes = CASE
                        WHEN excluded.rom_count = 0 AND game_library_meta.rom_count > 0
                            THEN game_library_meta.total_size_bytes
                        ELSE excluded.total_size_bytes
                    END,
                    discovery_state = CASE
                        WHEN excluded.rom_count = 0 AND game_library_meta.rom_count > 0
                            THEN game_library_meta.discovery_state
                        ELSE excluded.discovery_state
                    END,
                    enrichment_state = CASE
                        WHEN excluded.rom_count = 0 AND game_library_meta.rom_count > 0
                            THEN game_library_meta.enrichment_state
                        ELSE excluded.enrichment_state
                    END,
                    thumbnail_state = CASE
                        WHEN excluded.rom_count = 0 AND game_library_meta.rom_count > 0
                            THEN game_library_meta.thumbnail_state
                        ELSE excluded.thumbnail_state
                    END
                 RETURNING rom_count",
                rusqlite::params![
                    system,
                    dir_mtime_secs,
                    now,
                    rom_count as i64,
                    total_size_bytes as i64,
                    PhaseState::Complete.as_i64(),
                    PhaseState::Pending.as_i64(),
                    ThumbnailPhaseState::Pending.as_i64()
                ],
                |row| row.get(0),
            )
            .map_err(|e| Error::Other(format!("Upsert game_library_meta: {e}")))?;
        Ok(final_count as usize)
    }

    /// Load library metadata for a single system.
    pub fn load_system_meta(conn: &Connection, system: &str) -> Result<Option<SystemMeta>> {
        conn.query_row(
            "SELECT system, dir_mtime_secs, scanned_at, rom_count, total_size_bytes,
                    discovery_state, enrichment_state, thumbnail_state
                 FROM game_library_meta WHERE system = ?1",
            params![system],
            system_meta_from_row,
        )
        .optional()
        .map_err(|e| Error::Other(format!("Query load_system_meta: {e}")))
    }

    /// Return the last complete startup-skip fingerprint only when the system
    /// is fully reconciled and has no unresolved identity work.
    pub fn clean_startup_discovery_fingerprint(
        conn: &Connection,
        system: &str,
    ) -> Result<Option<String>> {
        let Some(meta) = Self::load_system_meta(conn, system)? else {
            return Ok(None);
        };
        if meta.discovery_state != PhaseState::Complete
            || meta.enrichment_state != PhaseState::Complete
        {
            return Ok(None);
        }

        let unresolved_identity_count: i64 = conn
            .query_row(
                "SELECT COUNT(*)
                 FROM game_library
                 WHERE system = ?1
                   AND identity_state NOT IN (?2, ?3, ?4)",
                params![
                    system,
                    super::IdentityState::NotApplicable.as_i64(),
                    super::IdentityState::CompleteMatched.as_i64(),
                    super::IdentityState::CompleteUnmatched.as_i64(),
                ],
                |row| row.get(0),
            )
            .map_err(|e| Error::Other(format!("Query unresolved identity count: {e}")))?;
        if unresolved_identity_count > 0 {
            return Ok(None);
        }

        let fingerprint_key = library_meta::keys::discovery_fingerprint(system);
        library_meta::read_meta_result(conn, &fingerprint_key)
    }

    /// Load library metadata for all systems.
    pub fn load_all_system_meta(conn: &Connection) -> Result<Vec<SystemMeta>> {
        let mut stmt = conn
            .prepare(
                "SELECT system, dir_mtime_secs, scanned_at, rom_count, total_size_bytes,
                        discovery_state, enrichment_state, thumbnail_state
                 FROM game_library_meta",
            )
            .map_err(|e| Error::Other(format!("Prepare load_all_system_meta: {e}")))?;

        let rows = stmt
            .query_map([], system_meta_from_row)
            .map_err(|e| Error::Other(format!("Query load_all_system_meta: {e}")))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| Error::Other(format!("Row read failed: {e}")))?);
        }
        Ok(result)
    }

    /// Pick one random non-special ROM from actual `game_library` rows.
    pub fn random_library_rom(conn: &Connection) -> Result<Option<(String, String)>> {
        conn.query_row(
            "SELECT system, rom_filename
             FROM game_library
             WHERE is_special = 0
             ORDER BY RANDOM()
             LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|e| Error::Other(format!("Query random_library_rom: {e}")))
    }

    /// Pick one random non-special ROM from a specific system.
    pub fn random_library_rom_for_system(
        conn: &Connection,
        system: &str,
    ) -> Result<Option<(String, String)>> {
        conn.query_row(
            "SELECT system, rom_filename
             FROM game_library
             WHERE system = ?1 AND is_special = 0
             ORDER BY RANDOM()
             LIMIT 1",
            params![system],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|e| Error::Other(format!("Query random_library_rom_for_system: {e}")))
    }

    /// Load cached hash data for all ROMs of a system from the game_library table.
    pub fn load_cached_hashes(
        conn: &Connection,
        system: &str,
    ) -> Result<std::collections::HashMap<String, crate::rom_hash::CachedHash>> {
        use std::collections::HashMap;

        let mut stmt = conn
            .prepare(
                "SELECT rom_filename, crc32, hash_mtime, hash_size_bytes, hash_matched_name
                 FROM game_library
                 WHERE system = ?1
                   AND crc32 IS NOT NULL
                   AND identity_state IN (?2, ?3)",
            )
            .map_err(|e| Error::Other(format!("Prepare load_cached_hashes: {e}")))?;

        let rows = stmt
            .query_map(
                params![
                    system,
                    super::IdentityState::CompleteMatched.as_i64(),
                    super::IdentityState::CompleteUnmatched.as_i64()
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        crate::rom_hash::CachedHash {
                            crc32: row.get::<_, i64>(1)? as u32,
                            hash_mtime: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
                            hash_size_bytes: row.get::<_, Option<i64>>(3)?.map(|s| s as u64),
                            matched_name: row.get(4)?,
                        },
                    ))
                },
            )
            .map_err(|e| Error::Other(format!("Query load_cached_hashes: {e}")))?;

        let mut map = HashMap::new();
        for row in rows.flatten() {
            map.insert(row.0, row.1);
        }
        Ok(map)
    }

    /// Systems with durable identity work that should resume in the background.
    pub fn systems_with_pending_identity(conn: &Connection) -> Result<Vec<String>> {
        let mut stmt = conn
            .prepare(
                "SELECT DISTINCT system
                 FROM game_library
                 WHERE identity_state IN (?1, ?2)
                 ORDER BY system",
            )
            .map_err(|e| Error::Other(format!("Prepare systems_with_pending_identity: {e}")))?;
        let rows = stmt
            .query_map(
                params![
                    super::IdentityState::Pending.as_i64(),
                    super::IdentityState::Failed.as_i64()
                ],
                |row| row.get::<_, String>(0),
            )
            .map_err(|e| Error::Other(format!("Query systems_with_pending_identity: {e}")))?;
        let mut systems = Vec::new();
        for row in rows {
            systems
                .push(row.map_err(|e| Error::Other(format!("Read pending identity system: {e}")))?);
        }
        Ok(systems)
    }

    /// Count identity rows that candidate jobs would claim.
    ///
    /// The app already knows which systems are hash-eligible candidates from
    /// the current scan. This helper keeps SQL ownership in core-server while
    /// letting logs distinguish candidate systems from systems with claimable
    /// durable work to do.
    pub fn identity_work_counts(
        conn: &Connection,
        systems: &[(String, bool)],
    ) -> Result<(usize, usize)> {
        if systems.is_empty() {
            return Ok((0, 0));
        }

        let mut pending_stmt = conn
            .prepare(
                "SELECT COUNT(*)
                 FROM game_library
                 WHERE system = ?1
                   AND identity_state IN (?2, ?3)",
            )
            .map_err(|e| Error::Other(format!("Prepare identity_work_counts pending: {e}")))?;
        let mut force_stmt = conn
            .prepare(
                "SELECT COUNT(*)
                 FROM game_library
                 WHERE system = ?1
                   AND identity_state != ?2",
            )
            .map_err(|e| Error::Other(format!("Prepare identity_work_counts force: {e}")))?;
        let mut work_systems = 0usize;
        let mut work_rows = 0usize;
        for (system, force_rehash) in systems {
            let count = if *force_rehash {
                force_stmt.query_row(
                    params![system, super::IdentityState::NotApplicable.as_i64()],
                    |row| row.get::<_, i64>(0),
                )
            } else {
                pending_stmt.query_row(
                    params![
                        system,
                        super::IdentityState::Pending.as_i64(),
                        super::IdentityState::Failed.as_i64()
                    ],
                    |row| row.get::<_, i64>(0),
                )
            }
            .map_err(|e| Error::Other(format!("Read identity_work_counts: {e}")))?
                as usize;
            if count > 0 {
                work_systems += 1;
                work_rows += count;
            }
        }
        Ok((work_systems, work_rows))
    }

    /// Claim a bounded set of rows for one identity worker batch.
    pub fn claim_identity_batch(
        conn: &mut Connection,
        system: &str,
        force_rehash: bool,
        limit: usize,
    ) -> Result<Vec<String>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let tx = conn
            .transaction()
            .map_err(|e| Error::Other(format!("Transaction start failed: {e}")))?;
        let sql = if force_rehash {
            "SELECT rom_filename
             FROM game_library
             WHERE system = ?1
               AND identity_state != ?2
             ORDER BY rom_filename
             LIMIT ?3"
        } else {
            "SELECT rom_filename
             FROM game_library
             WHERE system = ?1
               AND identity_state IN (?2, ?3)
             ORDER BY rom_filename
             LIMIT ?4"
        };
        let filenames = if force_rehash {
            let mut stmt = tx
                .prepare(sql)
                .map_err(|e| Error::Other(format!("Prepare force identity claim: {e}")))?;
            let rows = stmt
                .query_map(
                    params![
                        system,
                        super::IdentityState::NotApplicable.as_i64(),
                        limit as i64
                    ],
                    |row| row.get::<_, String>(0),
                )
                .map_err(|e| Error::Other(format!("Query force identity claim: {e}")))?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| Error::Other(format!("Read force identity claim: {e}")))?
        } else {
            let mut stmt = tx
                .prepare(sql)
                .map_err(|e| Error::Other(format!("Prepare identity claim: {e}")))?;
            let rows = stmt
                .query_map(
                    params![
                        system,
                        super::IdentityState::Pending.as_i64(),
                        super::IdentityState::Failed.as_i64(),
                        limit as i64
                    ],
                    |row| row.get::<_, String>(0),
                )
                .map_err(|e| Error::Other(format!("Query identity claim: {e}")))?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| Error::Other(format!("Read identity claim: {e}")))?
        };
        if !filenames.is_empty() {
            let mut stmt = tx
                .prepare(
                    "UPDATE game_library
                     SET identity_state = ?3
                     WHERE system = ?1
                       AND rom_filename = ?2",
                )
                .map_err(|e| Error::Other(format!("Prepare mark identity batch running: {e}")))?;
            for filename in &filenames {
                stmt.execute(params![
                    system,
                    filename,
                    super::IdentityState::Running.as_i64()
                ])
                .map_err(|e| Error::Other(format!("Mark identity batch running: {e}")))?;
            }
        }
        tx.commit()
            .map_err(|e| Error::Other(format!("Transaction commit failed: {e}")))?;
        Ok(filenames)
    }

    /// Claim a specific filename slice for force-verify identity work.
    pub fn claim_identity_filenames(
        conn: &mut Connection,
        system: &str,
        filenames: &[String],
    ) -> Result<Vec<String>> {
        if filenames.is_empty() {
            return Ok(Vec::new());
        }
        let tx = conn
            .transaction()
            .map_err(|e| Error::Other(format!("Transaction start failed: {e}")))?;
        let mut claimed = Vec::new();
        {
            let mut stmt = tx
                .prepare(
                    "UPDATE game_library
                     SET identity_state = ?3
                     WHERE system = ?1
                       AND rom_filename = ?2
                       AND identity_state NOT IN (?4, ?5)",
                )
                .map_err(|e| Error::Other(format!("Prepare force identity filename claim: {e}")))?;
            for filename in filenames {
                let affected = stmt
                    .execute(params![
                        system,
                        filename,
                        super::IdentityState::Running.as_i64(),
                        super::IdentityState::NotApplicable.as_i64(),
                        super::IdentityState::Running.as_i64()
                    ])
                    .map_err(|e| Error::Other(format!("Claim force identity filename: {e}")))?;
                if affected > 0 {
                    claimed.push(filename.clone());
                }
            }
        }
        tx.commit()
            .map_err(|e| Error::Other(format!("Transaction commit failed: {e}")))?;
        Ok(claimed)
    }

    /// Mark rows that the background identity phase is about to hash.
    ///
    /// Kept for tests and compatibility; new production workers use
    /// `claim_identity_batch` so cancellation only loses one mini-batch.
    pub fn mark_identity_running_for_system(
        conn: &Connection,
        system: &str,
        force_rehash: bool,
    ) -> Result<usize> {
        let affected = if force_rehash {
            conn.execute(
                "UPDATE game_library
                 SET identity_state = ?1
                 WHERE system = ?2
                   AND identity_state != ?3",
                params![
                    super::IdentityState::Running.as_i64(),
                    system,
                    super::IdentityState::NotApplicable.as_i64()
                ],
            )
        } else {
            conn.execute(
                "UPDATE game_library
                 SET identity_state = ?1
                 WHERE system = ?2
                   AND identity_state IN (?3, ?4)",
                params![
                    super::IdentityState::Running.as_i64(),
                    system,
                    super::IdentityState::Pending.as_i64(),
                    super::IdentityState::Failed.as_i64()
                ],
            )
        }
        .map_err(|e| Error::Other(format!("mark identity running for {system}: {e}")))?;
        Ok(affected)
    }

    /// Apply hash identity results to rows claimed by the identity worker.
    ///
    /// This intentionally updates rows in place instead of re-saving the full
    /// system. Discovery owns row membership; identity work must not delete
    /// rows if a later watcher/rescan already reconciled the system.
    pub fn update_running_identity_entries(
        conn: &mut Connection,
        system: &str,
        entries: &[GameEntry],
    ) -> Result<usize> {
        if entries.is_empty() {
            return Ok(0);
        }

        let tx = conn
            .transaction()
            .map_err(|e| Error::Other(format!("Transaction start failed: {e}")))?;
        let mut updated = 0usize;
        {
            let mut stmt = tx
                .prepare(
                    "UPDATE game_library
                     SET display_name = ?3,
                         base_title = ?4,
                         series_key = ?5,
                         search_text = ?6,
                         crc32 = ?7,
                         hash_mtime = ?8,
                         hash_size_bytes = ?9,
                         hash_matched_name = ?10,
                         identity_state = ?11,
                         normalized_title = ?12,
                         normalized_title_alt = ?13
                     WHERE system = ?1
                       AND rom_filename = ?2
                       AND identity_state = ?14
                       AND size_bytes = ?15",
                )
                .map_err(|e| Error::Other(format!("Prepare update identity entries: {e}")))?;
            for entry in entries {
                let search_text = build_search_text(
                    entry.display_name.as_deref(),
                    &entry.rom_filename,
                    &entry.base_title,
                    &entry.developer,
                    entry.release_date.as_deref(),
                );
                updated += stmt
                    .execute(params![
                        system,
                        &entry.rom_filename,
                        &entry.display_name,
                        &entry.base_title,
                        &entry.series_key,
                        &search_text,
                        entry.crc32.map(|c| c as i64),
                        entry.hash_mtime,
                        entry.hash_size_bytes.map(|s| s as i64),
                        &entry.hash_matched_name,
                        entry.identity_state.as_i64(),
                        &entry.normalized_title,
                        &entry.normalized_title_alt,
                        super::IdentityState::Running.as_i64(),
                        entry.size_bytes as i64,
                    ])
                    .map_err(|e| Error::Other(format!("Update identity entry: {e}")))?;
            }
        }
        tx.commit()
            .map_err(|e| Error::Other(format!("Transaction commit failed: {e}")))?;
        Ok(updated)
    }

    /// Mark rows claimed by an identity worker but not successfully updated.
    pub fn finish_unresolved_identity_running(
        conn: &Connection,
        system: &str,
        state: super::IdentityState,
    ) -> Result<usize> {
        conn.execute(
            "UPDATE game_library
             SET identity_state = ?1
             WHERE system = ?2
               AND identity_state = ?3",
            params![
                state.as_i64(),
                system,
                super::IdentityState::Running.as_i64()
            ],
        )
        .map_err(|e| Error::Other(format!("finish unresolved identity for {system}: {e}")))
    }

    /// Mark a specific claimed identity batch as pending/failed.
    pub fn finish_identity_batch(
        conn: &mut Connection,
        system: &str,
        filenames: &[String],
        state: super::IdentityState,
    ) -> Result<usize> {
        if filenames.is_empty() {
            return Ok(0);
        }
        let tx = conn
            .transaction()
            .map_err(|e| Error::Other(format!("Transaction start failed: {e}")))?;
        let mut updated = 0usize;
        {
            let mut stmt = tx
                .prepare(
                    "UPDATE game_library
                     SET identity_state = ?3
                     WHERE system = ?1
                       AND rom_filename = ?2
                       AND identity_state = ?4",
                )
                .map_err(|e| Error::Other(format!("Prepare finish identity batch: {e}")))?;
            for filename in filenames {
                updated += stmt
                    .execute(params![
                        system,
                        filename,
                        state.as_i64(),
                        super::IdentityState::Running.as_i64()
                    ])
                    .map_err(|e| Error::Other(format!("Finish identity batch row: {e}")))?;
            }
        }
        tx.commit()
            .map_err(|e| Error::Other(format!("Transaction commit failed: {e}")))?;
        Ok(updated)
    }

    pub fn set_enrichment_state(
        conn: &Connection,
        system: &str,
        state: PhaseState,
    ) -> Result<usize> {
        let updated = conn
            .execute(
                "UPDATE game_library_meta
             SET enrichment_state = ?1
             WHERE system = ?2",
                params![state.as_i64(), system],
            )
            .map_err(|e| Error::Other(format!("set enrichment state for {system}: {e}")))?;
        match state {
            PhaseState::Complete => Self::refresh_game_library_system_stats(conn, system)?,
            PhaseState::Running => {
                Self::set_game_library_system_stats_state(
                    conn,
                    system,
                    super::StatsRefreshState::Refreshing,
                )?;
            }
            PhaseState::Failed => {
                Self::set_game_library_system_stats_state(
                    conn,
                    system,
                    super::StatsRefreshState::Failed,
                )?;
            }
            PhaseState::Pending => {
                Self::set_game_library_system_stats_state(
                    conn,
                    system,
                    super::StatsRefreshState::Stale,
                )?;
            }
            PhaseState::Unknown => {}
        }
        Ok(updated)
    }

    pub fn set_thumbnail_state(
        conn: &Connection,
        system: &str,
        state: ThumbnailPhaseState,
    ) -> Result<usize> {
        conn.execute(
            "UPDATE game_library_meta
             SET thumbnail_state = ?1
             WHERE system = ?2",
            params![state.as_i64(), system],
        )
        .map_err(|e| Error::Other(format!("set thumbnail state for {system}: {e}")))
    }

    pub fn upsert_thumbnail_job(
        conn: &Connection,
        system: &str,
        rom_filename: &str,
        kind: crate::thumbnails::ThumbnailKind,
        manifest: &crate::thumbnail_manifest::ManifestMatch,
    ) -> Result<usize> {
        conn.execute(
            "INSERT INTO library_thumbnail_job (
                system, rom_filename, kind, filename, repo_url_name, branch,
                is_symlink, state, attempts, priority, updated_at
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?9, ?10)
             ON CONFLICT(system, rom_filename, kind, filename) DO UPDATE SET
                repo_url_name = excluded.repo_url_name,
                branch = excluded.branch,
                is_symlink = excluded.is_symlink,
                state = excluded.state,
                priority = excluded.priority,
                updated_at = excluded.updated_at",
            params![
                system,
                rom_filename,
                kind.repo_dir(),
                &manifest.filename,
                &manifest.repo_url_name,
                &manifest.branch,
                manifest.is_symlink,
                ThumbnailJobState::Queued.as_i64(),
                super::thumbnail_priority(kind),
                unix_now(),
            ],
        )
        .map_err(|e| Error::Other(format!("upsert thumbnail job for {system}: {e}")))
    }

    pub fn upsert_thumbnail_jobs(
        conn: &mut Connection,
        jobs: &[ThumbnailDownloadJob],
    ) -> Result<usize> {
        if jobs.is_empty() {
            return Ok(0);
        }
        let tx = conn
            .transaction()
            .map_err(|e| Error::Other(format!("Transaction start failed: {e}")))?;
        let mut stmt = tx
            .prepare(
                "INSERT INTO library_thumbnail_job (
                    system, rom_filename, kind, filename, repo_url_name, branch,
                    is_symlink, state, attempts, priority, updated_at
                 )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?9, ?10)
                 ON CONFLICT(system, rom_filename, kind, filename) DO UPDATE SET
                    repo_url_name = excluded.repo_url_name,
                    branch = excluded.branch,
                    is_symlink = excluded.is_symlink,
                    state = excluded.state,
                    priority = excluded.priority,
                    updated_at = excluded.updated_at",
            )
            .map_err(|e| Error::Other(format!("Prepare upsert thumbnail jobs: {e}")))?;
        let now = unix_now();
        let mut updated = 0usize;
        for job in jobs {
            updated += stmt
                .execute(params![
                    &job.system,
                    &job.rom_filename,
                    job.kind.repo_dir(),
                    &job.manifest.filename,
                    &job.manifest.repo_url_name,
                    &job.manifest.branch,
                    job.manifest.is_symlink,
                    ThumbnailJobState::Queued.as_i64(),
                    job.priority(),
                    now,
                ])
                .map_err(|e| {
                    Error::Other(format!("upsert thumbnail job for {}: {e}", job.system))
                })?;
        }
        drop(stmt);
        tx.commit()
            .map_err(|e| Error::Other(format!("Transaction commit failed: {e}")))?;
        Ok(updated)
    }

    pub fn load_pending_thumbnail_jobs(
        conn: &Connection,
        limit: usize,
    ) -> Result<Vec<ThumbnailDownloadJob>> {
        let mut jobs = Vec::new();
        for priority in 0..=2 {
            for system in replay_control_core::systems::visible_systems() {
                if jobs.len() >= limit {
                    return Ok(jobs);
                }
                Self::load_pending_thumbnail_jobs_for_system_priority(
                    conn,
                    system.folder_name,
                    priority,
                    limit - jobs.len(),
                    &mut jobs,
                )?;
            }
        }
        Ok(jobs)
    }

    fn load_pending_thumbnail_jobs_for_system_priority(
        conn: &Connection,
        system: &str,
        priority: i64,
        limit: usize,
        jobs: &mut Vec<ThumbnailDownloadJob>,
    ) -> Result<()> {
        if limit == 0 {
            return Ok(());
        }
        let mut stmt = conn
            .prepare(
                "SELECT system, rom_filename, kind, filename, repo_url_name, branch, is_symlink
                 FROM library_thumbnail_job
                 WHERE system = ?1
                   AND priority = ?2
                   AND state IN (?3, ?4)
                 ORDER BY state, rom_filename, kind
                 LIMIT ?5",
            )
            .map_err(|e| Error::Other(format!("Prepare load_pending_thumbnail_jobs: {e}")))?;
        let rows = stmt
            .query_map(
                params![
                    system,
                    priority,
                    ThumbnailJobState::Queued.as_i64(),
                    ThumbnailJobState::Failed.as_i64(),
                    limit as i64
                ],
                |row| {
                    let kind_str: String = row.get(2)?;
                    let kind = crate::thumbnails::ThumbnailKind::from_repo_dir(&kind_str)
                        .ok_or_else(|| {
                            rusqlite::Error::InvalidColumnType(
                                2,
                                "kind".to_string(),
                                rusqlite::types::Type::Text,
                            )
                        })?;
                    Ok(ThumbnailDownloadJob {
                        system: row.get(0)?,
                        rom_filename: row.get(1)?,
                        kind,
                        manifest: crate::thumbnail_manifest::ManifestMatch {
                            filename: row.get(3)?,
                            repo_url_name: row.get(4)?,
                            branch: row.get(5)?,
                            is_symlink: row.get(6)?,
                        },
                    })
                },
            )
            .map_err(|e| Error::Other(format!("Query load_pending_thumbnail_jobs: {e}")))?;
        for row in rows {
            jobs.push(row.map_err(|e| Error::Other(format!("Read thumbnail job: {e}")))?);
        }
        Ok(())
    }

    pub fn complete_thumbnail_jobs_for_key(
        conn: &mut Connection,
        system: &str,
        kind: crate::thumbnails::ThumbnailKind,
        filename: &str,
        box_art_url: &str,
    ) -> Result<usize> {
        let tx = conn
            .transaction()
            .map_err(|e| Error::Other(format!("Transaction start failed: {e}")))?;
        let updated = if kind == crate::thumbnails::ThumbnailKind::Boxart {
            tx.execute(
                "UPDATE game_library
                     SET box_art_url = ?1
                     WHERE system = ?2
                       AND rom_filename IN (
                         SELECT rom_filename
                         FROM library_thumbnail_job
                         WHERE system = ?2 AND kind = ?3 AND filename = ?4
                       )",
                params![box_art_url, system, kind.repo_dir(), filename],
            )
            .map_err(|e| Error::Other(format!("Update thumbnail job box_art_url: {e}")))?
        } else {
            0
        };
        tx.execute(
            "DELETE FROM library_thumbnail_job
             WHERE system = ?1 AND kind = ?2 AND filename = ?3",
            params![system, kind.repo_dir(), filename],
        )
        .map_err(|e| Error::Other(format!("Delete completed thumbnail jobs: {e}")))?;
        Self::refresh_thumbnail_phase_after_job_change(&tx, system)?;
        if kind == crate::thumbnails::ThumbnailKind::Boxart {
            Self::refresh_game_library_system_boxart_count(&tx, system)?;
        }
        tx.commit()
            .map_err(|e| Error::Other(format!("Transaction commit failed: {e}")))?;
        Ok(updated)
    }

    pub fn fail_thumbnail_jobs_for_key(
        conn: &mut Connection,
        system: &str,
        kind: crate::thumbnails::ThumbnailKind,
        filename: &str,
    ) -> Result<usize> {
        let tx = conn
            .transaction()
            .map_err(|e| Error::Other(format!("Transaction start failed: {e}")))?;
        let updated = tx
            .execute(
                "UPDATE library_thumbnail_job
                 SET state = ?1,
                     attempts = attempts + 1,
                     updated_at = ?2
                 WHERE system = ?3 AND kind = ?4 AND filename = ?5",
                params![
                    ThumbnailJobState::Failed.as_i64(),
                    unix_now(),
                    system,
                    kind.repo_dir(),
                    filename
                ],
            )
            .map_err(|e| Error::Other(format!("Mark thumbnail jobs failed: {e}")))?;
        tx.execute(
            "UPDATE game_library_meta
             SET thumbnail_state = ?1
             WHERE system = ?2",
            params![ThumbnailPhaseState::Failed.as_i64(), system],
        )
        .map_err(|e| Error::Other(format!("Mark thumbnail phase failed: {e}")))?;
        tx.commit()
            .map_err(|e| Error::Other(format!("Transaction commit failed: {e}")))?;
        Ok(updated)
    }

    fn refresh_thumbnail_phase_after_job_change(conn: &Connection, system: &str) -> Result<()> {
        let remaining: i64 = conn
            .query_row(
                "SELECT COUNT(*)
                 FROM library_thumbnail_job
                 WHERE system = ?1",
                params![system],
                |row| row.get(0),
            )
            .map_err(|e| Error::Other(format!("Count thumbnail jobs for {system}: {e}")))?;
        let state = if remaining == 0 {
            ThumbnailPhaseState::Complete
        } else {
            ThumbnailPhaseState::Queued
        };
        conn.execute(
            "UPDATE game_library_meta
             SET thumbnail_state = ?1
             WHERE system = ?2",
            params![state.as_i64(), system],
        )
        .map_err(|e| Error::Other(format!("Refresh thumbnail phase for {system}: {e}")))?;
        Ok(())
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
                    .map(replay_control_core::genre::normalize_genre)
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
                    let gg = replay_control_core::genre::normalize_genre(g);
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
        conn.execute(
            "DELETE FROM game_library_system_stats WHERE system = ?1",
            params![system],
        )
        .map_err(|e| Error::Other(format!("Clear system game_library_system_stats: {e}")))?;
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

    /// Bulk-update the stored normalized-title columns for a system.
    ///
    /// Each tuple is `(rom_filename, normalized_title, normalized_title_alt)`.
    /// Single transaction; rows missing from `updates` are left untouched.
    /// Used by the boot-time `TITLE_NORM_VERSION` reconcile after the
    /// caller pre-computes the new values from arcade_db / filename stems.
    pub fn update_normalized_titles(
        conn: &mut Connection,
        system: &str,
        updates: &[(String, String, String)],
    ) -> Result<usize> {
        if updates.is_empty() {
            return Ok(0);
        }
        let tx = conn
            .transaction()
            .map_err(|e| Error::Other(format!("Tx start update_normalized_titles: {e}")))?;
        let mut count = 0usize;
        {
            let mut stmt = tx
                .prepare(
                    "UPDATE game_library
                     SET normalized_title = ?2,
                         normalized_title_alt = ?3
                     WHERE system = ?4 AND rom_filename = ?1",
                )
                .map_err(|e| Error::Other(format!("Prepare update_normalized_titles: {e}")))?;
            for (filename, norm, norm_alt) in updates {
                count += stmt
                    .execute(params![filename, norm, norm_alt, system])
                    .map_err(|e| Error::Other(format!("update_normalized_titles: {e}")))?;
            }
        }
        tx.commit()
            .map_err(|e| Error::Other(format!("Commit update_normalized_titles: {e}")))?;
        Ok(count)
    }

    /// Get `rom_filename → (normalized_title, normalized_title_alt)` for a system.
    ///
    /// Drives the enrichment matcher: each ROM is looked up against the
    /// LaunchBox row map by its stored normalized title (and the secondary
    /// arcade-clone parent title, when present). Populated at scan time so
    /// matching is a hashmap probe, not a per-ROM normalize() call.
    pub fn visible_normalized_titles(
        conn: &Connection,
        system: &str,
    ) -> Result<HashMap<String, (String, String)>> {
        let mut stmt = conn
            .prepare(
                "SELECT rom_filename, normalized_title, normalized_title_alt
                 FROM game_library
                 WHERE system = ?1",
            )
            .map_err(|e| Error::Other(format!("Prepare visible_normalized_titles: {e}")))?;
        let rows = stmt
            .query_map(params![system], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|e| Error::Other(format!("Query visible_normalized_titles: {e}")))?;
        let mut out = HashMap::new();
        for r in rows.flatten() {
            out.insert(r.0, (r.1, r.2));
        }
        Ok(out)
    }

    /// Get `rom_filename → hash_matched_name` for ROMs that have a CRC match.
    ///
    /// Used by enrichment to try No-Intro canonical names as thumbnail lookup keys.
    pub fn visible_hash_matched_names(
        conn: &Connection,
        system: &str,
    ) -> Result<HashMap<String, String>> {
        let mut stmt = conn
            .prepare(
                "SELECT rom_filename, hash_matched_name FROM game_library
                 WHERE system = ?1 AND hash_matched_name IS NOT NULL",
            )
            .map_err(|e| Error::Other(format!("Query failed: {e}")))?;
        let rows = stmt
            .query_map(params![system], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| Error::Other(format!("Query failed: {e}")))?;
        Ok(rows.flatten().collect())
    }

    /// Clear all game_library and game_library_meta entries.
    pub fn clear_all_game_library(conn: &Connection) -> Result<()> {
        conn.execute("DELETE FROM game_library", [])
            .map_err(|e| Error::Other(format!("Clear game_library: {e}")))?;
        conn.execute("DELETE FROM game_library_meta", [])
            .map_err(|e| Error::Other(format!("Clear game_library_meta: {e}")))?;
        conn.execute("DELETE FROM game_library_system_stats", [])
            .map_err(|e| Error::Other(format!("Clear game_library_system_stats: {e}")))?;
        Ok(())
    }

    /// Total row count in `game_library`.
    pub fn game_library_count(conn: &Connection) -> Result<usize> {
        conn.query_row("SELECT COUNT(*) FROM game_library", [], |r| {
            r.get::<_, i64>(0)
        })
        .map(|n| n as usize)
        .map_err(|e| Error::Other(format!("game_library_count: {e}")))
    }

    /// Per-system row counts from `game_library`. Systems with zero rows
    /// are absent from the returned map.
    pub fn row_counts_per_system(conn: &Connection) -> Result<HashMap<String, usize>> {
        let mut stmt = conn
            .prepare("SELECT system, COUNT(*) FROM game_library GROUP BY system")
            .map_err(|e| Error::Other(format!("row_counts_per_system prepare: {e}")))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
            })
            .map_err(|e| Error::Other(format!("row_counts_per_system query: {e}")))?;
        Ok(rows.flatten().collect())
    }

    /// Clear all `box_art_url` values from `game_library`.
    /// Used after clearing images from disk so the UI doesn't show 404 placeholders.
    pub fn clear_all_box_art_urls(conn: &Connection) -> Result<()> {
        conn.execute(
            "UPDATE game_library SET box_art_url = NULL WHERE box_art_url IS NOT NULL",
            [],
        )
        .map_err(|e| Error::Other(format!("Clear box_art_url: {e}")))?;
        Self::clear_game_library_system_boxart_counts(conn)?;
        Ok(())
    }

    /// Set or clear a single ROM's `box_art_url`. Pass `None` to revert to
    /// the enrichment-resolved default on the next enrichment pass.
    pub fn update_box_art_url(
        conn: &Connection,
        system: &str,
        rom_filename: &str,
        url: Option<&str>,
    ) -> Result<()> {
        match url {
            Some(url) => conn.execute(
                "UPDATE game_library SET box_art_url = ?1 WHERE system = ?2 AND rom_filename = ?3",
                params![url, system, rom_filename],
            ),
            None => conn.execute(
                "UPDATE game_library SET box_art_url = NULL WHERE system = ?1 AND rom_filename = ?2",
                params![system, rom_filename],
            ),
        }
        .map_err(|e| Error::Other(format!("Update box_art_url: {e}")))?;
        Self::refresh_game_library_system_boxart_count(conn, system)?;
        Ok(())
    }

    /// Delete the `game_library` entry for a specific ROM.
    pub fn delete_for_rom(conn: &Connection, system: &str, rom_filename: &str) {
        let _ = conn.execute(
            "DELETE FROM game_library WHERE system = ?1 AND rom_filename = ?2",
            params![system, rom_filename],
        );
        let _ = Self::refresh_game_library_system_stats(conn, system);
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
        let _ = Self::refresh_game_library_system_stats(conn, system);
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

    /// Batch-load `(rom_filename, release_year)` pairs for a system (derived from release_date).
    ///
    /// Returns only rows where `release_date IS NOT NULL`, keyed by filename.
    pub fn system_release_years(
        conn: &Connection,
        system: &str,
    ) -> Result<std::collections::HashMap<String, u16>> {
        let mut stmt = conn
            .prepare(
                "SELECT rom_filename, CAST(substr(release_date, 1, 4) AS INTEGER) \
                 FROM game_library \
                 WHERE system = ?1 AND release_date IS NOT NULL",
            )
            .map_err(|e| Error::Other(format!("Prepare system_release_years: {e}")))?;

        let rows = stmt
            .query_map(params![system], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<i64>>(1)?.unwrap_or(0) as u16,
                ))
            })
            .map_err(|e| Error::Other(format!("Query system_release_years: {e}")))?;

        let mut map = std::collections::HashMap::new();
        for row in rows.flatten() {
            if row.1 > 0 {
                map.insert(row.0, row.1);
            }
        }
        Ok(map)
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

    /// Distinct base-title counts for a set of arcade boards, in one grouped
    /// query. Used by `/search` to size the "Matched board" cards and decide
    /// which to show (boards with no hits are simply absent from the map).
    ///
    /// `board_tags` are `ArcadeBoard::as_tag()` slugs. Hits the partial index
    /// `idx_gl_board(system, board) WHERE board != ''` and skips hacks /
    /// translations / specials / clones to match the developer-count
    /// semantics on the same page.
    pub fn board_game_counts(
        conn: &Connection,
        board_tags: &[&str],
    ) -> Result<HashMap<String, usize>> {
        if board_tags.is_empty() {
            return Ok(HashMap::new());
        }
        let placeholders = vec!["?"; board_tags.len()].join(", ");
        let sql = format!(
            "SELECT board, COUNT(DISTINCT base_title) FROM game_library \
             WHERE board IN ({placeholders}) \
               AND is_clone = 0 \
               AND is_translation = 0 \
               AND is_hack = 0 \
               AND is_special = 0 \
             GROUP BY board"
        );
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| Error::Other(format!("Prepare board_game_counts: {e}")))?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(board_tags), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
            })
            .map_err(|e| Error::Other(format!("Query board_game_counts: {e}")))?;
        Ok(rows.flatten().collect())
    }

    /// Get systems where the user has games on a given arcade board, with
    /// game counts. Mirrors `developer_systems` shape so the board page can
    /// render the same system-filter chips.
    pub fn board_systems(conn: &Connection, board_tag: &str) -> Result<Vec<(String, usize)>> {
        if board_tag.is_empty() {
            return Ok(Vec::new());
        }
        let mut stmt = conn
            .prepare(
                "SELECT system, COUNT(DISTINCT base_title) as cnt
                 FROM game_library
                 WHERE board = ?1
                   AND is_clone = 0
                   AND is_translation = 0
                   AND is_hack = 0
                   AND is_special = 0
                 GROUP BY system
                 ORDER BY cnt DESC",
            )
            .map_err(|e| Error::Other(format!("Prepare board_systems: {e}")))?;

        let rows = stmt
            .query_map(params![board_tag], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1).map(|v| v as usize)?,
                ))
            })
            .map_err(|e| Error::Other(format!("Query board_systems: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Get the top N games on a given arcade board, preferring entries with
    /// cover art. Mirrors `games_by_developer` shape so the `/search` board
    /// discovery block can render an identical 3-thumbnail preview.
    pub fn games_by_board(
        conn: &Connection,
        board_tag: &str,
        limit: usize,
        region_pref: &str,
        region_secondary: &str,
    ) -> Result<Vec<GameEntry>> {
        if board_tag.is_empty() {
            return Ok(Vec::new());
        }
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
                WHERE board = ?1
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
            .map_err(|e| Error::Other(format!("Prepare games_by_board: {e}")))?;

        let rows = stmt
            .query_map(
                params![board_tag, region_pref, region_secondary, limit as i64],
                Self::row_to_game_entry,
            )
            .map_err(|e| Error::Other(format!("Query games_by_board: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Get distinct genre groups for games on a given arcade board, optionally
    /// filtered by system. Mirrors `developer_genre_groups`.
    pub fn board_genre_groups(
        conn: &Connection,
        board_tag: &str,
        system_filter: Option<&str>,
    ) -> Result<Vec<String>> {
        if board_tag.is_empty() {
            return Ok(Vec::new());
        }
        let has_system = system_filter.is_some_and(|s| !s.is_empty());

        if has_system {
            let mut stmt = conn
                .prepare(
                    "SELECT DISTINCT genre_group FROM game_library
                     WHERE board = ?1 AND genre_group != '' AND system = ?2
                     ORDER BY genre_group",
                )
                .map_err(|e| Error::Other(format!("Prepare board_genre_groups: {e}")))?;
            let rows = stmt
                .query_map(params![board_tag, system_filter.unwrap()], |row| {
                    row.get::<_, String>(0)
                })
                .map_err(|e| Error::Other(format!("Query board_genre_groups: {e}")))?;
            Ok(rows.flatten().collect())
        } else {
            let mut stmt = conn
                .prepare(
                    "SELECT DISTINCT genre_group FROM game_library
                     WHERE board = ?1 AND genre_group != ''
                     ORDER BY genre_group",
                )
                .map_err(|e| Error::Other(format!("Prepare board_genre_groups: {e}")))?;
            let rows = stmt
                .query_map(params![board_tag], |row| row.get::<_, String>(0))
                .map_err(|e| Error::Other(format!("Query board_genre_groups: {e}")))?;
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

        // Board filter (parameterized). Matches the `idx_gl_board` partial
        // index on (system, board) WHERE board != ''.
        if let Some(board) = filter.board {
            param_values.push(board.as_tag().to_string());
            let idx = param_values.len();
            where_clauses.push(format!("board = ?{idx}"));
        }

        // Rating filter (parameterized).
        if let Some(mr) = filter.min_rating {
            param_values.push(mr.to_string());
            let idx = param_values.len();
            where_clauses.push(format!("rating >= ?{idx}"));
        }

        // Year range filters (parameterized). Lexicographic compare on ISO date
        // strings — hits the `idx_release_date_chrono` index directly. NULL
        // release_date is excluded.
        //
        // Half-open upper bound (`< '(max+1)'`) includes day-precision entries
        // of the max year: `"1999-12-31" < "2000"` but `"1999-12-31" > "1999"`,
        // so `<= '1999'` would miss them.
        //
        // PERF NOTE: ISO string compare is fast and indexable, but if year-keyed
        // queries become hot (timeline, per-year histograms, aggregates), revisit
        // adding a computed `release_year_cached` column (`GENERATED ALWAYS AS
        // (CAST(substr(release_date,1,4) AS INTEGER))`) with its own index.
        if let Some(min_y) = filter.min_year {
            param_values.push(format!("{min_y:04}"));
            let idx = param_values.len();
            where_clauses.push(format!("release_date >= ?{idx}"));
        }
        if let Some(max_y) = filter.max_year {
            param_values.push(format!("{:04}", max_y.saturating_add(1)));
            let idx = param_values.len();
            where_clauses.push(format!("release_date < ?{idx}"));
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

    /// Ranked game search shared by global search and per-system ROM lists.
    ///
    /// Empty text queries preserve the existing SQL-ordered pagination path.
    /// Non-empty text queries gather normal `search_text` matches plus alias
    /// candidates, dedupe them, score with the shared search scorer, then
    /// paginate the ranked results.
    #[allow(clippy::too_many_arguments)]
    pub fn search_game_library_ranked(
        conn: &Connection,
        system: Option<&str>,
        query: &str,
        filter: &SearchFilter<'_>,
        offset: usize,
        limit: usize,
        region_pref: RegionPreference,
        region_secondary: Option<RegionPreference>,
    ) -> Result<(Vec<GameEntry>, usize)> {
        let normalized_query = query.trim().to_lowercase();
        if normalized_query.is_empty() {
            return Self::search_game_library(conn, system, None, &[], filter, offset, limit);
        }

        let query_words: Vec<String> = split_into_words(&normalized_query)
            .into_iter()
            .map(str::to_string)
            .collect();

        let (normal_candidates, _) =
            Self::search_game_library(conn, system, None, &query_words, filter, 0, usize::MAX)?;
        let alias_base_titles =
            Self::search_alias_base_titles_for_query(conn, system, &normalized_query)?;
        let alias_candidates =
            Self::search_alias_candidate_entries(conn, system, &alias_base_titles, filter)?;

        let mut candidates: HashMap<(String, String), GameEntry> = HashMap::new();
        for entry in normal_candidates.into_iter().chain(alias_candidates) {
            candidates.insert((entry.system.clone(), entry.rom_filename.clone()), entry);
        }

        let mut scored: Vec<(u32, GameEntry)> = candidates
            .into_values()
            .filter_map(|entry| {
                let display = entry.display_name.as_deref().unwrap_or(&entry.rom_filename);
                let mut score = search_score(
                    &normalized_query,
                    display,
                    &entry.rom_filename,
                    region_pref,
                    region_secondary,
                );

                if score == 0
                    && !entry.base_title.is_empty()
                    && alias_base_titles
                        .get(&entry.system)
                        .is_some_and(|titles| titles.contains(&entry.base_title))
                {
                    score = 350;
                }

                (score > 0).then_some((score, entry))
            })
            .collect();

        scored.sort_by(|(score_a, entry_a), (score_b, entry_b)| {
            score_b
                .cmp(score_a)
                .then_with(|| {
                    let name_a = entry_a
                        .display_name
                        .as_deref()
                        .unwrap_or(&entry_a.rom_filename)
                        .to_lowercase();
                    let name_b = entry_b
                        .display_name
                        .as_deref()
                        .unwrap_or(&entry_b.rom_filename)
                        .to_lowercase();
                    name_a.cmp(&name_b)
                })
                .then_with(|| entry_a.system.cmp(&entry_b.system))
                .then_with(|| entry_a.rom_filename.cmp(&entry_b.rom_filename))
        });

        let total = scored.len();
        let entries = scored
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(|(_, entry)| entry)
            .collect();

        Ok((entries, total))
    }

    fn search_alias_base_titles_for_query(
        conn: &Connection,
        system: Option<&str>,
        query: &str,
    ) -> Result<HashMap<String, HashSet<String>>> {
        if query.is_empty() {
            return Ok(HashMap::new());
        }

        let escaped = query
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let like_pattern = format!("%{escaped}%");
        let sql = if system.is_some() {
            "SELECT DISTINCT system, base_title
             FROM game_alias
             WHERE system = ?1 AND alias_name LIKE ?2 ESCAPE '\\' COLLATE NOCASE"
        } else {
            "SELECT DISTINCT system, base_title
             FROM game_alias
             WHERE alias_name LIKE ?1 ESCAPE '\\' COLLATE NOCASE"
        };
        let mut stmt = conn.prepare(sql).map_err(|e| {
            Error::Other(format!("Prepare search_alias_base_titles_for_query: {e}"))
        })?;

        let mut out: HashMap<String, HashSet<String>> = HashMap::new();
        let mut read_rows = |params: &[&dyn rusqlite::types::ToSql]| -> Result<()> {
            let rows = stmt
                .query_map(params, |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|e| {
                    Error::Other(format!("Query search_alias_base_titles_for_query: {e}"))
                })?;

            for row in rows {
                let (system, base_title) =
                    row.map_err(|e| Error::Other(format!("Read alias search row: {e}")))?;
                out.entry(system).or_default().insert(base_title);
            }
            Ok(())
        };

        if let Some(system) = system {
            read_rows(&[&system, &like_pattern])?;
        } else {
            read_rows(&[&like_pattern])?;
        }

        Ok(out)
    }

    fn search_alias_candidate_entries(
        conn: &Connection,
        system: Option<&str>,
        alias_base_titles: &HashMap<String, HashSet<String>>,
        filter: &SearchFilter<'_>,
    ) -> Result<Vec<GameEntry>> {
        let mut entries = Vec::new();
        for (alias_system, titles) in alias_base_titles {
            if titles.is_empty() || system.is_some_and(|requested| requested != alias_system) {
                continue;
            }

            let (mut where_clauses, mut param_values) =
                Self::build_filter_clauses(Some(alias_system), None, &[], filter);

            let title_placeholders: Vec<String> = titles
                .iter()
                .map(|title| {
                    param_values.push(title.clone());
                    format!("?{}", param_values.len())
                })
                .collect();
            where_clauses.push(format!(
                "base_title COLLATE NOCASE IN ({})",
                title_placeholders.join(",")
            ));

            let sql = format!(
                "SELECT {GAME_ENTRY_COLUMNS} \
                 FROM game_library \
                 {}",
                Self::build_where_sql(&where_clauses)
            );
            let params: Vec<Box<dyn rusqlite::types::ToSql>> = param_values
                .into_iter()
                .map(|v| Box::new(v) as Box<dyn rusqlite::types::ToSql>)
                .collect();
            let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                params.iter().map(|v| v.as_ref()).collect();

            let mut stmt = conn.prepare(&sql).map_err(|e| {
                Error::Other(format!("Prepare search_alias_candidate_entries: {e}"))
            })?;
            let rows = stmt
                .query_map(param_refs.as_slice(), Self::row_to_game_entry)
                .map_err(|e| Error::Other(format!("Query search_alias_candidate_entries: {e}")))?;
            for row in rows {
                entries
                    .push(row.map_err(|e| Error::Other(format!("Read alias candidate row: {e}")))?);
            }
        }

        Ok(entries)
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
    use super::super::tests::*;
    use super::super::{IdentityState, LibraryDb, LibraryGameResource, PhaseState, library_meta};
    use super::SearchFilter;
    use replay_control_core::resource_kind;
    use replay_control_core::rom_tags::RegionPreference;

    // Removed: `genre_enrichment_fills_empty_genre_from_launchbox` — exercised
    // LibraryDb::bulk_upsert + the legacy game_metadata table. The
    // genre-update behavior is still covered by
    // `genre_enrichment_does_not_overwrite_existing_genre` and
    // `genre_enrichment_mixed_empty_and_existing` below, which feed
    // BoxArtGenreRating directly.

    #[test]
    fn genre_enrichment_does_not_overwrite_existing_genre() {
        let (mut conn, _dir) = open_temp_db();

        LibraryDb::save_system_entries(
            &mut conn,
            "sega_smd",
            &[make_game_entry_with_genre(
                "sega_smd", "Sonic.md", "Shooter",
            )],
            None,
        )
        .unwrap();

        LibraryDb::update_box_art_genre_rating(
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

        let roms = LibraryDb::load_system_entries(&conn, "sega_smd").unwrap();
        assert_eq!(roms[0].genre.as_deref(), Some("Shooter"));
    }

    #[test]
    fn genre_enrichment_mixed_empty_and_existing() {
        let (mut conn, _dir) = open_temp_db();

        LibraryDb::save_system_entries(
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

        LibraryDb::update_box_art_genre_rating(
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

        let roms = LibraryDb::load_system_entries(&conn, "sega_smd").unwrap();
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
    fn thumbnail_jobs_complete_all_roms_for_same_manifest_key() {
        let (mut conn, _dir) = open_temp_db();
        LibraryDb::save_system_entries(
            &mut conn,
            "nintendo_snes",
            &[
                make_game_entry("nintendo_snes", "Mario (USA).sfc", false),
                make_game_entry("nintendo_snes", "Mario (Europe).sfc", false),
            ],
            None,
        )
        .unwrap();
        let manifest = crate::thumbnail_manifest::ManifestMatch {
            filename: "Super Mario World".into(),
            repo_url_name: "Nintendo_-_Super_Nintendo_Entertainment_System".into(),
            branch: "master".into(),
            is_symlink: false,
        };
        for rom in ["Mario (USA).sfc", "Mario (Europe).sfc"] {
            LibraryDb::upsert_thumbnail_job(
                &conn,
                "nintendo_snes",
                rom,
                crate::thumbnails::ThumbnailKind::Boxart,
                &manifest,
            )
            .unwrap();
        }

        let jobs = LibraryDb::load_pending_thumbnail_jobs(&conn, 10).unwrap();
        assert_eq!(jobs.len(), 2);

        let updated = LibraryDb::complete_thumbnail_jobs_for_key(
            &mut conn,
            "nintendo_snes",
            crate::thumbnails::ThumbnailKind::Boxart,
            "Super Mario World",
            "/media/nintendo_snes/boxart/Super Mario World.png",
        )
        .unwrap();
        assert_eq!(updated, 2);

        let jobs = LibraryDb::load_pending_thumbnail_jobs(&conn, 10).unwrap();
        assert!(jobs.is_empty());
        let roms = LibraryDb::load_system_entries(&conn, "nintendo_snes").unwrap();
        assert!(roms.iter().all(|rom| rom.box_art_url.as_deref()
            == Some("/media/nintendo_snes/boxart/Super Mario World.png")));
        let meta = LibraryDb::load_system_meta(&conn, "nintendo_snes")
            .unwrap()
            .unwrap();
        assert_eq!(
            meta.thumbnail_state,
            super::super::ThumbnailPhaseState::Complete
        );
    }

    #[test]
    fn pending_thumbnail_jobs_order_by_priority_before_system() {
        let (mut conn, _dir) = open_temp_db();
        LibraryDb::save_system_entries(
            &mut conn,
            "nintendo_snes",
            &[make_game_entry("nintendo_snes", "Mario.sfc", false)],
            None,
        )
        .unwrap();
        LibraryDb::save_system_entries(
            &mut conn,
            "sega_smd",
            &[make_game_entry("sega_smd", "Sonic.md", false)],
            None,
        )
        .unwrap();
        let manifest = crate::thumbnail_manifest::ManifestMatch {
            filename: "Image".into(),
            repo_url_name: "Repo".into(),
            branch: "master".into(),
            is_symlink: false,
        };
        LibraryDb::upsert_thumbnail_job(
            &conn,
            "nintendo_snes",
            "Mario.sfc",
            crate::thumbnails::ThumbnailKind::Snap,
            &manifest,
        )
        .unwrap();
        LibraryDb::upsert_thumbnail_job(
            &conn,
            "sega_smd",
            "Sonic.md",
            crate::thumbnails::ThumbnailKind::Title,
            &manifest,
        )
        .unwrap();
        LibraryDb::upsert_thumbnail_job(
            &conn,
            "nintendo_snes",
            "Mario.sfc",
            crate::thumbnails::ThumbnailKind::Boxart,
            &manifest,
        )
        .unwrap();

        let jobs = LibraryDb::load_pending_thumbnail_jobs(&conn, 10).unwrap();
        let kinds: Vec<_> = jobs.iter().map(|job| job.kind).collect();
        assert_eq!(
            kinds,
            vec![
                crate::thumbnails::ThumbnailKind::Boxart,
                crate::thumbnails::ThumbnailKind::Title,
                crate::thumbnails::ThumbnailKind::Snap
            ]
        );
    }

    #[test]
    fn save_system_entries_preserves_resources_for_unchanged_roms() {
        let (mut conn, _dir) = open_temp_db();

        LibraryDb::save_system_entries(
            &mut conn,
            "snes",
            &[make_game_entry("snes", "Mario.sfc", false)],
            None,
        )
        .unwrap();
        LibraryDb::replace_detail_metadata_and_resources_for_system(
            &mut conn,
            "snes",
            &[],
            &[manual_resource("Mario.sfc")],
        )
        .unwrap();

        let mut rescanned = make_game_entry("snes", "Mario.sfc", false);
        rescanned.display_name = Some("Super Mario World".into());
        LibraryDb::save_system_entries(&mut conn, "snes", &[rescanned], Some(123)).unwrap();

        let resources =
            LibraryDb::game_resources(&conn, "snes", "Mario.sfc", resource_kind::MANUAL).unwrap();
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].url, "https://example.test/Mario.sfc.pdf");
    }

    #[test]
    fn save_system_entries_cascades_resources_for_removed_roms() {
        let (mut conn, _dir) = open_temp_db();

        LibraryDb::save_system_entries(
            &mut conn,
            "snes",
            &[
                make_game_entry("snes", "Mario.sfc", false),
                make_game_entry("snes", "Zelda.sfc", false),
            ],
            None,
        )
        .unwrap();
        LibraryDb::replace_detail_metadata_and_resources_for_system(
            &mut conn,
            "snes",
            &[],
            &[manual_resource("Mario.sfc"), manual_resource("Zelda.sfc")],
        )
        .unwrap();

        LibraryDb::save_system_entries(
            &mut conn,
            "snes",
            &[make_game_entry("snes", "Mario.sfc", false)],
            Some(456),
        )
        .unwrap();

        let mario_resources =
            LibraryDb::game_resources(&conn, "snes", "Mario.sfc", resource_kind::MANUAL).unwrap();
        let zelda_resources =
            LibraryDb::game_resources(&conn, "snes", "Zelda.sfc", resource_kind::MANUAL).unwrap();
        assert_eq!(mario_resources.len(), 1);
        assert!(zelda_resources.is_empty());
    }

    #[test]
    fn save_system_entries_assigns_new_scan_tokens_and_finalizes_stale_rows() {
        let (mut conn, _dir) = open_temp_db();

        LibraryDb::save_system_entries(
            &mut conn,
            "snes",
            &[
                make_game_entry("snes", "Mario.sfc", false),
                make_game_entry("snes", "Zelda.sfc", false),
            ],
            None,
        )
        .unwrap();
        let first_token: i64 = conn
            .query_row(
                "SELECT scan_token FROM game_library WHERE system = 'snes' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();

        LibraryDb::save_system_entries(
            &mut conn,
            "snes",
            &[make_game_entry("snes", "Mario.sfc", false)],
            None,
        )
        .unwrap();
        let rows: Vec<(String, i64)> = {
            let mut stmt = conn
                .prepare("SELECT rom_filename, scan_token FROM game_library WHERE system = 'snes'")
                .unwrap();
            stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .collect::<std::result::Result<_, _>>()
                .unwrap()
        };

        assert_eq!(rows, vec![("Mario.sfc".to_string(), first_token + 1)]);
        let meta = LibraryDb::load_system_meta(&conn, "snes").unwrap().unwrap();
        assert_eq!(meta.rom_count, 1);
        assert_eq!(meta.discovery_state, super::super::PhaseState::Complete);
    }

    #[test]
    fn save_system_entries_keeps_first_duplicate_rom_filename() {
        let (mut conn, _dir) = open_temp_db();
        let mut first = make_game_entry("snes", "Mario.sfc", false);
        first.display_name = Some("First".into());
        let mut second = make_game_entry("snes", "Mario.sfc", false);
        second.display_name = Some("Second".into());

        LibraryDb::save_system_entries(&mut conn, "snes", &[first, second], None).unwrap();

        let roms = LibraryDb::load_system_entries(&conn, "snes").unwrap();
        assert_eq!(roms.len(), 1);
        assert_eq!(roms[0].display_name.as_deref(), Some("First"));
    }

    #[test]
    fn save_system_entries_persists_identity_state() {
        let (mut conn, _dir) = open_temp_db();
        let mut entry = make_game_entry("nintendo_snes", "Mario.sfc", false);
        entry.crc32 = Some(0x1234_ABCD);
        entry.hash_mtime = Some(42);
        entry.hash_size_bytes = Some(1000);
        entry.hash_matched_name = Some("Super Mario World (USA)".into());
        entry.identity_state = super::super::IdentityState::CompleteMatched;

        LibraryDb::save_system_entries(&mut conn, "nintendo_snes", &[entry], None).unwrap();

        let roms = LibraryDb::load_system_entries(&conn, "nintendo_snes").unwrap();
        assert_eq!(
            roms[0].identity_state,
            super::super::IdentityState::CompleteMatched
        );
        let meta = LibraryDb::load_system_meta(&conn, "nintendo_snes")
            .unwrap()
            .unwrap();
        assert_eq!(meta.discovery_state, super::super::PhaseState::Complete);
        assert_eq!(meta.enrichment_state, super::super::PhaseState::Pending);
        assert_eq!(
            meta.thumbnail_state,
            super::super::ThumbnailPhaseState::Pending
        );
    }

    #[test]
    fn clean_startup_discovery_fingerprint_requires_complete_system_state() {
        let (mut conn, _dir) = open_temp_db();
        let mut complete = make_game_entry("nintendo_snes", "Complete.sfc", false);
        complete.identity_state = IdentityState::CompleteMatched;
        LibraryDb::save_system_entries(&mut conn, "nintendo_snes", &[complete], None).unwrap();
        let key = library_meta::keys::discovery_fingerprint("nintendo_snes");
        library_meta::write_meta(&conn, &key, Some("fingerprint-1")).unwrap();

        assert_eq!(
            LibraryDb::clean_startup_discovery_fingerprint(&conn, "nintendo_snes").unwrap(),
            None,
            "pending enrichment must not be treated as clean"
        );

        LibraryDb::set_enrichment_state(&conn, "nintendo_snes", PhaseState::Complete).unwrap();
        assert_eq!(
            LibraryDb::clean_startup_discovery_fingerprint(&conn, "nintendo_snes").unwrap(),
            Some("fingerprint-1".to_string())
        );

        let mut pending = make_game_entry("nintendo_snes", "Pending.sfc", false);
        pending.identity_state = IdentityState::Pending;
        LibraryDb::save_system_entries(&mut conn, "nintendo_snes", &[pending], None).unwrap();
        library_meta::write_meta(&conn, &key, Some("fingerprint-2")).unwrap();
        LibraryDb::set_enrichment_state(&conn, "nintendo_snes", PhaseState::Complete).unwrap();
        assert_eq!(
            LibraryDb::clean_startup_discovery_fingerprint(&conn, "nintendo_snes").unwrap(),
            None,
            "pending identity must not be treated as clean"
        );
    }

    #[test]
    fn mark_identity_running_targets_pending_rows() {
        let (mut conn, _dir) = open_temp_db();
        let mut pending = make_game_entry("nintendo_snes", "Pending.sfc", false);
        pending.identity_state = super::super::IdentityState::Pending;
        let mut complete = make_game_entry("nintendo_snes", "Complete.sfc", false);
        complete.identity_state = super::super::IdentityState::CompleteMatched;

        LibraryDb::save_system_entries(&mut conn, "nintendo_snes", &[pending, complete], None)
            .unwrap();

        let changed =
            LibraryDb::mark_identity_running_for_system(&conn, "nintendo_snes", false).unwrap();
        assert_eq!(changed, 1);

        let roms = LibraryDb::load_system_entries(&conn, "nintendo_snes").unwrap();
        let pending = roms
            .iter()
            .find(|rom| rom.rom_filename == "Pending.sfc")
            .unwrap();
        let complete = roms
            .iter()
            .find(|rom| rom.rom_filename == "Complete.sfc")
            .unwrap();
        assert_eq!(pending.identity_state, super::super::IdentityState::Running);
        assert_eq!(
            complete.identity_state,
            super::super::IdentityState::CompleteMatched
        );
    }

    #[test]
    fn load_cached_hashes_excludes_in_flight_identity_rows() {
        let (mut conn, _dir) = open_temp_db();
        let mut complete = make_game_entry("nintendo_snes", "Complete.sfc", false);
        complete.crc32 = Some(0x1234_ABCD);
        complete.hash_mtime = Some(42);
        complete.hash_size_bytes = Some(1000);
        complete.hash_matched_name = Some("Complete (USA)".into());
        complete.identity_state = super::super::IdentityState::CompleteMatched;

        let mut running = make_game_entry("nintendo_snes", "Running.sfc", false);
        running.crc32 = Some(0xCAFE_BABE);
        running.hash_mtime = Some(99);
        running.hash_size_bytes = Some(2000);
        running.hash_matched_name = Some("Running (USA)".into());
        running.identity_state = super::super::IdentityState::Running;

        LibraryDb::save_system_entries(&mut conn, "nintendo_snes", &[complete, running], None)
            .unwrap();

        let hashes = LibraryDb::load_cached_hashes(&conn, "nintendo_snes").unwrap();
        assert!(hashes.contains_key("Complete.sfc"));
        assert!(!hashes.contains_key("Running.sfc"));
    }

    #[test]
    fn systems_with_pending_identity_returns_pending_and_failed_systems() {
        let (mut conn, _dir) = open_temp_db();
        let mut pending = make_game_entry("nintendo_snes", "Pending.sfc", false);
        pending.identity_state = super::super::IdentityState::Pending;
        let mut failed = make_game_entry("nintendo_gba", "Failed.gba", false);
        failed.identity_state = super::super::IdentityState::Failed;
        let mut complete = make_game_entry("sega_smd", "Complete.md", false);
        complete.identity_state = super::super::IdentityState::CompleteMatched;

        LibraryDb::save_system_entries(&mut conn, "nintendo_snes", &[pending], None).unwrap();
        LibraryDb::save_system_entries(&mut conn, "nintendo_gba", &[failed], None).unwrap();
        LibraryDb::save_system_entries(&mut conn, "sega_smd", &[complete], None).unwrap();

        let systems = LibraryDb::systems_with_pending_identity(&conn).unwrap();
        assert_eq!(
            systems,
            vec!["nintendo_gba".to_string(), "nintendo_snes".to_string()]
        );
    }

    #[test]
    fn identity_work_counts_counts_candidate_systems_with_work() {
        let (mut conn, _dir) = open_temp_db();
        let mut pending = make_game_entry("nintendo_snes", "Pending.sfc", false);
        pending.identity_state = super::super::IdentityState::Pending;
        let mut failed = make_game_entry("nintendo_snes", "Failed.sfc", false);
        failed.identity_state = super::super::IdentityState::Failed;
        let mut complete = make_game_entry("sega_smd", "Complete.md", false);
        complete.identity_state = super::super::IdentityState::CompleteMatched;

        LibraryDb::save_system_entries(&mut conn, "nintendo_snes", &[pending, failed], None)
            .unwrap();
        LibraryDb::save_system_entries(&mut conn, "sega_smd", &[complete], None).unwrap();

        let normal_systems = vec![
            ("nintendo_snes".to_string(), false),
            ("sega_smd".to_string(), false),
            ("nintendo_gba".to_string(), false),
        ];
        let counts = LibraryDb::identity_work_counts(&conn, &normal_systems).unwrap();
        assert_eq!(counts, (1, 2));

        let force_systems = vec![
            ("nintendo_snes".to_string(), true),
            ("sega_smd".to_string(), true),
            ("nintendo_gba".to_string(), true),
        ];
        let counts = LibraryDb::identity_work_counts(&conn, &force_systems).unwrap();
        assert_eq!(counts, (2, 3));
    }

    #[test]
    fn update_running_identity_entries_does_not_replace_system_rows() {
        let (mut conn, _dir) = open_temp_db();
        let mut target = make_game_entry("nintendo_snes", "Pending.sfc", false);
        target.identity_state = super::super::IdentityState::Pending;
        let sibling = make_game_entry("nintendo_snes", "Sibling.sfc", false);

        LibraryDb::save_system_entries(&mut conn, "nintendo_snes", &[target, sibling], None)
            .unwrap();
        LibraryDb::replace_detail_metadata_and_resources_for_system(
            &mut conn,
            "nintendo_snes",
            &[],
            &[manual_resource("Sibling.sfc")],
        )
        .unwrap();
        LibraryDb::mark_identity_running_for_system(&conn, "nintendo_snes", false).unwrap();

        let mut identified = make_game_entry("nintendo_snes", "Pending.sfc", false);
        identified.display_name = Some("Canonical Pending".into());
        identified.base_title = "canonical pending".into();
        identified.series_key = "canonical".into();
        identified.crc32 = Some(0xCAFE_BABE);
        identified.hash_mtime = Some(123);
        identified.hash_size_bytes = Some(identified.size_bytes);
        identified.hash_matched_name = Some("Canonical Pending (USA)".into());
        identified.identity_state = super::super::IdentityState::CompleteMatched;
        identified.normalized_title = "canonical pending".into();

        let updated =
            LibraryDb::update_running_identity_entries(&mut conn, "nintendo_snes", &[identified])
                .unwrap();
        assert_eq!(updated, 1);

        let roms = LibraryDb::load_system_entries(&conn, "nintendo_snes").unwrap();
        assert_eq!(roms.len(), 2);
        let pending = roms
            .iter()
            .find(|rom| rom.rom_filename == "Pending.sfc")
            .unwrap();
        assert_eq!(
            pending.identity_state,
            super::super::IdentityState::CompleteMatched
        );
        assert_eq!(pending.display_name.as_deref(), Some("Canonical Pending"));

        let sibling_resources =
            LibraryDb::game_resources(&conn, "nintendo_snes", "Sibling.sfc", resource_kind::MANUAL)
                .unwrap();
        assert_eq!(sibling_resources.len(), 1);
    }

    #[test]
    fn update_running_identity_entries_ignores_unclaimed_rows() {
        let (mut conn, _dir) = open_temp_db();
        let mut target = make_game_entry("nintendo_snes", "Pending.sfc", false);
        target.identity_state = super::super::IdentityState::Pending;
        LibraryDb::save_system_entries(&mut conn, "nintendo_snes", &[target], None).unwrap();

        let mut identified = make_game_entry("nintendo_snes", "Pending.sfc", false);
        identified.display_name = Some("Canonical Pending".into());
        identified.crc32 = Some(0xCAFE_BABE);
        identified.hash_mtime = Some(123);
        identified.hash_size_bytes = Some(identified.size_bytes);
        identified.identity_state = super::super::IdentityState::CompleteMatched;

        let updated =
            LibraryDb::update_running_identity_entries(&mut conn, "nintendo_snes", &[identified])
                .unwrap();
        assert_eq!(updated, 0);

        let roms = LibraryDb::load_system_entries(&conn, "nintendo_snes").unwrap();
        assert_eq!(roms[0].identity_state, super::super::IdentityState::Pending);
        assert_eq!(roms[0].crc32, None);
    }

    #[test]
    fn claim_identity_batch_limits_and_returns_only_pending_work() {
        let (mut conn, _dir) = open_temp_db();
        let mut a = make_game_entry("nintendo_snes", "A.sfc", false);
        a.identity_state = super::super::IdentityState::Pending;
        let mut b = make_game_entry("nintendo_snes", "B.sfc", false);
        b.identity_state = super::super::IdentityState::Failed;
        let mut c = make_game_entry("nintendo_snes", "C.sfc", false);
        c.identity_state = super::super::IdentityState::CompleteMatched;
        LibraryDb::save_system_entries(&mut conn, "nintendo_snes", &[a, b, c], None).unwrap();

        let claimed =
            LibraryDb::claim_identity_batch(&mut conn, "nintendo_snes", false, 1).unwrap();
        assert_eq!(claimed, vec!["A.sfc".to_string()]);

        let roms = LibraryDb::load_system_entries(&conn, "nintendo_snes").unwrap();
        let a = roms.iter().find(|rom| rom.rom_filename == "A.sfc").unwrap();
        let b = roms.iter().find(|rom| rom.rom_filename == "B.sfc").unwrap();
        assert_eq!(a.identity_state, super::super::IdentityState::Running);
        assert_eq!(b.identity_state, super::super::IdentityState::Failed);
    }

    #[test]
    fn finish_identity_batch_changes_only_running_claimed_rows() {
        let (mut conn, _dir) = open_temp_db();
        let mut running = make_game_entry("nintendo_snes", "Running.sfc", false);
        running.identity_state = super::super::IdentityState::Running;
        let mut pending = make_game_entry("nintendo_snes", "Pending.sfc", false);
        pending.identity_state = super::super::IdentityState::Pending;
        LibraryDb::save_system_entries(&mut conn, "nintendo_snes", &[running, pending], None)
            .unwrap();

        let changed = LibraryDb::finish_identity_batch(
            &mut conn,
            "nintendo_snes",
            &["Running.sfc".into(), "Pending.sfc".into()],
            super::super::IdentityState::Failed,
        )
        .unwrap();
        assert_eq!(changed, 1);

        let roms = LibraryDb::load_system_entries(&conn, "nintendo_snes").unwrap();
        let running = roms
            .iter()
            .find(|rom| rom.rom_filename == "Running.sfc")
            .unwrap();
        let pending = roms
            .iter()
            .find(|rom| rom.rom_filename == "Pending.sfc")
            .unwrap();
        assert_eq!(running.identity_state, super::super::IdentityState::Failed);
        assert_eq!(pending.identity_state, super::super::IdentityState::Pending);
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

    fn manual_resource(rom_filename: &str) -> LibraryGameResource {
        LibraryGameResource {
            rom_filename: rom_filename.into(),
            source: "test".into(),
            resource_type: resource_kind::MANUAL.into(),
            resource_id: format!("manual:{rom_filename}"),
            url: format!("https://example.test/{rom_filename}.pdf"),
            title: Some(format!("{rom_filename} Manual")),
            languages: Some("en".into()),
            platform: Some("snes".into()),
            mime_type: Some("application/pdf".into()),
        }
    }

    #[test]
    fn find_developer_matches_exact_match_first() {
        let (mut conn, _dir) = open_temp_db();
        LibraryDb::save_system_entries(
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

        let matches = LibraryDb::find_developer_matches(&conn, "snk").unwrap();
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
        LibraryDb::save_system_entries(
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
        let matches = LibraryDb::find_developer_matches(&conn, "capcom").unwrap();
        assert!(matches.is_empty());
    }

    #[test]
    fn find_developer_matches_single_match() {
        let (mut conn, _dir) = open_temp_db();
        LibraryDb::save_system_entries(
            &mut conn,
            "snes",
            &[
                make_game_entry_with_developer("snes", "MegaManX.sfc", "Capcom", "Mega Man X"),
                make_game_entry_with_developer("snes", "BoF.sfc", "Capcom", "Breath of Fire"),
            ],
            None,
        )
        .unwrap();
        let matches = LibraryDb::find_developer_matches(&conn, "capcom").unwrap();
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
            .map(|g| replay_control_core::genre::normalize_genre(g).to_string())
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
        LibraryDb::save_system_entries(
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
            LibraryDb::developer_games(&conn, "Capcom", &filters, 0, 50).unwrap();
        assert_eq!(total, 2);
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn developer_games_specific_genre() {
        let (mut conn, _dir) = open_temp_db();
        LibraryDb::save_system_entries(
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
            LibraryDb::developer_games(&conn, "Capcom", &filters, 0, 50).unwrap();
        assert_eq!(total, 1);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].base_title, "Mega Man X");
    }

    #[test]
    fn developer_games_system_and_genre_combined() {
        let (mut conn, _dir) = open_temp_db();
        LibraryDb::save_system_entries(
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
        LibraryDb::save_system_entries(
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
        let (entries, total) = LibraryDb::search_game_library(
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
        LibraryDb::save_system_entries(
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
            LibraryDb::developer_games(&conn, "capcom", &filters, 0, 50).unwrap();
        assert_eq!(total, 1);
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn developer_games_offset_beyond_total() {
        let (mut conn, _dir) = open_temp_db();
        LibraryDb::save_system_entries(
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
            LibraryDb::developer_games(&conn, "Capcom", &filters, 100, 50).unwrap();
        assert_eq!(total, 2);
        assert!(entries.is_empty());
    }

    #[test]
    fn developer_games_pagination() {
        let (mut conn, _dir) = open_temp_db();
        LibraryDb::save_system_entries(
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
        let (entries, total) = LibraryDb::developer_games(&conn, "Capcom", &filters, 0, 2).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(total, 3);
        let (entries, _) = LibraryDb::developer_games(&conn, "Capcom", &filters, 2, 2).unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn developer_games_hide_hacks_and_clones() {
        let (mut conn, _dir) = open_temp_db();
        LibraryDb::save_system_entries(
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
            LibraryDb::developer_games(&conn, "Capcom", &filters, 0, 50).unwrap();
        assert_eq!(total, 1);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].base_title, "Street Fighter II");
    }

    #[test]
    fn developer_games_multiplayer_only() {
        let (mut conn, _dir) = open_temp_db();
        LibraryDb::save_system_entries(
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
            LibraryDb::developer_games(&conn, "Capcom", &filters, 0, 50).unwrap();
        assert_eq!(total, 1);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].base_title, "Street Fighter II");
    }

    #[test]
    fn games_by_developer_deduplicates_across_systems() {
        let (mut conn, _dir) = open_temp_db();
        LibraryDb::save_system_entries(
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
        LibraryDb::save_system_entries(
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
        let results = LibraryDb::games_by_developer(&conn, "Capcom", 50, "us", "").unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn games_by_developer_prefers_entry_with_box_art() {
        let (mut conn, _dir) = open_temp_db();
        LibraryDb::save_system_entries(
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
        let results = LibraryDb::games_by_developer(&conn, "Capcom", 50, "us", "").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].box_art_url.as_deref(), Some("/img/sf2.png"));
    }

    #[test]
    fn games_by_developer_excludes_clones_and_hacks() {
        let (mut conn, _dir) = open_temp_db();
        LibraryDb::save_system_entries(
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
        let results = LibraryDb::games_by_developer(&conn, "Capcom", 50, "us", "").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].base_title, "Street Fighter II");
    }

    #[test]
    fn games_by_developer_prefers_user_region() {
        let (mut conn, _dir) = open_temp_db();
        LibraryDb::save_system_entries(
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
            LibraryDb::games_by_developer(&conn, "Capcom", 50, "europe", "japan").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].region, "europe");
    }

    // ── count_system_entries + load_system_entries_page ───────────────

    #[test]
    fn count_system_entries_empty() {
        let (conn, _dir) = open_temp_db();
        let count = LibraryDb::count_system_entries(&conn, "snes").unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn count_system_entries_returns_correct_count() {
        let (mut conn, _dir) = open_temp_db();
        LibraryDb::save_system_entries(
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

        assert_eq!(LibraryDb::count_system_entries(&conn, "snes").unwrap(), 3);
        // Different system should return 0.
        assert_eq!(LibraryDb::count_system_entries(&conn, "gba").unwrap(), 0);
    }

    #[test]
    fn load_cached_hashes_round_trips_hash_size() {
        let (mut conn, _dir) = open_temp_db();
        let mut entry = make_game_entry("snes", "Mario.sfc", false);
        entry.crc32 = Some(0x1234_ABCD);
        entry.hash_mtime = Some(42);
        entry.hash_size_bytes = Some(3_145_728);
        entry.hash_matched_name = Some("Super Mario World (USA)".to_string());
        entry.identity_state = super::super::IdentityState::CompleteMatched;

        LibraryDb::save_system_entries(&mut conn, "snes", &[entry], None).unwrap();

        let hashes = LibraryDb::load_cached_hashes(&conn, "snes").unwrap();
        let cached = hashes.get("Mario.sfc").expect("hash row should load");
        assert_eq!(cached.crc32, 0x1234_ABCD);
        assert_eq!(cached.hash_mtime, 42);
        assert_eq!(cached.hash_size_bytes, Some(3_145_728));
        assert_eq!(
            cached.matched_name.as_deref(),
            Some("Super Mario World (USA)")
        );
    }

    #[test]
    fn random_library_rom_returns_real_non_special_row() {
        let (mut conn, _dir) = open_temp_db();
        let mut bios = make_game_entry("snes", "Bios.sfc", false);
        bios.is_special = true;
        LibraryDb::save_system_entries(
            &mut conn,
            "snes",
            &[make_game_entry("snes", "Mario.sfc", false), bios],
            None,
        )
        .unwrap();

        let random = LibraryDb::random_library_rom(&conn).unwrap();
        assert_eq!(random, Some(("snes".to_string(), "Mario.sfc".to_string())));
    }

    #[test]
    fn random_library_rom_empty_or_special_only_returns_none() {
        let (mut conn, _dir) = open_temp_db();
        assert_eq!(LibraryDb::random_library_rom(&conn).unwrap(), None);

        let mut bios = make_game_entry("snes", "Bios.sfc", false);
        bios.is_special = true;
        LibraryDb::save_system_entries(&mut conn, "snes", &[bios], None).unwrap();

        assert_eq!(LibraryDb::random_library_rom(&conn).unwrap(), None);
    }

    #[test]
    fn random_library_rom_for_system_is_scoped_and_skips_specials() {
        let (mut conn, _dir) = open_temp_db();

        let mut snes_special = make_game_entry("snes", "Bios.sfc", false);
        snes_special.is_special = true;
        let snes_game = make_game_entry("snes", "Mario.sfc", false);
        let genesis_game = make_game_entry("sega_smd", "Sonic.md", false);
        LibraryDb::save_system_entries(&mut conn, "snes", &[snes_special, snes_game], None)
            .unwrap();
        LibraryDb::save_system_entries(&mut conn, "sega_smd", &[genesis_game], None).unwrap();

        assert_eq!(
            LibraryDb::random_library_rom_for_system(&conn, "snes").unwrap(),
            Some(("snes".to_string(), "Mario.sfc".to_string()))
        );
        assert_eq!(
            LibraryDb::random_library_rom_for_system(&conn, "pcengine").unwrap(),
            None
        );
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
        LibraryDb::save_system_entries(&mut conn, "snes", &entries, None).unwrap();

        // First page: offset=0, limit=2 → Alpha, Bravo
        let page1 = LibraryDb::load_system_entries_page(&conn, "snes", 0, 2).unwrap();
        assert_eq!(page1.len(), 2);
        assert_eq!(page1[0].display_name.as_deref(), Some("Alpha"));
        assert_eq!(page1[1].display_name.as_deref(), Some("Bravo"));

        // Second page: offset=2, limit=2 → Charlie, Delta
        let page2 = LibraryDb::load_system_entries_page(&conn, "snes", 2, 2).unwrap();
        assert_eq!(page2.len(), 2);
        assert_eq!(page2[0].display_name.as_deref(), Some("Charlie"));
        assert_eq!(page2[1].display_name.as_deref(), Some("Delta"));

        // Third page: offset=4, limit=2 → Echo (partial page)
        let page3 = LibraryDb::load_system_entries_page(&conn, "snes", 4, 2).unwrap();
        assert_eq!(page3.len(), 1);
        assert_eq!(page3[0].display_name.as_deref(), Some("Echo"));

        // Beyond range: offset=5, limit=2 → empty
        let page4 = LibraryDb::load_system_entries_page(&conn, "snes", 5, 2).unwrap();
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
        LibraryDb::save_system_entries(&mut conn, "snes", &entries, None).unwrap();

        let page = LibraryDb::load_system_entries_page(&conn, "snes", 0, 10).unwrap();
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
        LibraryDb::save_system_entries(&mut conn, "snes", &entries, None).unwrap();

        let page = LibraryDb::load_system_entries_page(&conn, "snes", 0, 10).unwrap();
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
        let text = super::build_search_text(Some("Game"), "game.rom", "game", "", Some("1987"));
        assert_eq!(text, "game|game.rom|game|1987");
    }

    #[test]
    fn build_search_text_with_developer_and_year() {
        let text = super::build_search_text(
            Some("Game"),
            "game.rom",
            "game",
            "Imagine",
            Some("1987-06-15"),
        );
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
        LibraryDb::save_system_entries(&mut *conn, "snes", &snes_entries, None).unwrap();
        LibraryDb::save_system_entries(&mut *conn, "sega_smd", &smd_entries, None).unwrap();
    }

    #[test]
    fn search_exact_match() {
        let (mut conn, _dir) = open_temp_db();
        insert_test_library(&mut conn);

        let (results, _total) = LibraryDb::search_game_library(
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

        let (results, _total) = LibraryDb::search_game_library(
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

        let (results, _total) = LibraryDb::search_game_library(
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

        let (results, _total) = LibraryDb::search_game_library(
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

        let (results, _total) = LibraryDb::search_game_library(
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

        let (results, _total) = LibraryDb::search_game_library(
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

        let (results, _total) = LibraryDb::search_game_library(
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

        let (results, _total) = LibraryDb::search_game_library(
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

        let (results, _total) = LibraryDb::search_game_library(
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
        let (results, _total) = LibraryDb::search_game_library(
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
        let (results, _total) = LibraryDb::search_game_library(
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

        let (results, _total) = LibraryDb::search_game_library(
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

    #[test]
    fn ranked_search_includes_alias_only_candidates() {
        let (mut conn, _dir) = open_temp_db();
        let entry = super::super::GameEntry {
            display_name: Some("Streets of Rage".to_string()),
            base_title: "Streets of Rage".to_string(),
            ..make_game_entry("sega_smd", "Streets of Rage (USA).md", false)
        };
        LibraryDb::save_system_entries(&mut conn, "sega_smd", &[entry], None).unwrap();
        LibraryDb::upsert_alias(
            &conn,
            "sega_smd",
            "Streets of Rage",
            "Bare Knuckle",
            "jp",
            "test",
        )
        .unwrap();

        let (results, total) = LibraryDb::search_game_library_ranked(
            &conn,
            None,
            "bare knuckle",
            &SearchFilter::default(),
            0,
            10,
            RegionPreference::World,
            None,
        )
        .unwrap();

        assert_eq!(total, 1);
        assert_eq!(results[0].rom_filename, "Streets of Rage (USA).md");
    }

    #[test]
    fn ranked_search_alias_candidates_respect_system_scope() {
        let (mut conn, _dir) = open_temp_db();
        let smd_entry = super::super::GameEntry {
            display_name: Some("Streets of Rage".to_string()),
            base_title: "Streets of Rage".to_string(),
            ..make_game_entry("sega_smd", "Streets of Rage (USA).md", false)
        };
        let snes_entry = super::super::GameEntry {
            display_name: Some("River City Ransom".to_string()),
            base_title: "River City Ransom".to_string(),
            ..make_game_entry("snes", "River City Ransom.sfc", false)
        };
        LibraryDb::save_system_entries(&mut conn, "sega_smd", &[smd_entry], None).unwrap();
        LibraryDb::save_system_entries(&mut conn, "snes", &[snes_entry], None).unwrap();
        LibraryDb::upsert_alias(
            &conn,
            "sega_smd",
            "Streets of Rage",
            "Bare Knuckle",
            "jp",
            "test",
        )
        .unwrap();

        let (results, total) = LibraryDb::search_game_library_ranked(
            &conn,
            Some("snes"),
            "bare knuckle",
            &SearchFilter::default(),
            0,
            10,
            RegionPreference::World,
            None,
        )
        .unwrap();

        assert_eq!(total, 0);
        assert!(results.is_empty());
    }

    #[test]
    fn ranked_search_paginates_scored_results() {
        let (mut conn, _dir) = open_temp_db();
        insert_test_library(&mut conn);

        let (results, total) = LibraryDb::search_game_library_ranked(
            &conn,
            Some("snes"),
            "super mario",
            &SearchFilter::default(),
            1,
            1,
            RegionPreference::World,
            None,
        )
        .unwrap();

        assert_eq!(total, 2);
        assert_eq!(results.len(), 1);
        assert!(results[0].rom_filename.contains("Super Mario"));
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
        LibraryDb::save_system_entries(&mut conn, "snes", &[mario, zelda, metroid], None).unwrap();

        let result = LibraryDb::top_genre_for_filenames(
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
        let result = LibraryDb::top_genre_for_filenames(&conn, "snes", &[]).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn top_genre_for_filenames_no_matches() {
        let (mut conn, _dir) = open_temp_db();
        LibraryDb::save_system_entries(
            &mut conn,
            "snes",
            &[make_game_entry("snes", "mario.sfc", false)],
            None,
        )
        .unwrap();
        // "mario.sfc" has no genre_group set and no base_title.
        let result = LibraryDb::top_genre_for_filenames(&conn, "snes", &["mario.sfc"]).unwrap();
        assert_eq!(result, None);
    }

    // ── lookup_game_entries ─────────────────────────────────────────

    #[test]
    fn lookup_game_entries_returns_matching() {
        let (mut conn, _dir) = open_temp_db();
        LibraryDb::save_system_entries(
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
        let result = LibraryDb::lookup_game_entries(&conn, &keys).unwrap();
        assert_eq!(result.len(), 2);
        assert!(result.contains_key(&("snes".into(), "mario.sfc".into())));
        assert!(result.contains_key(&("snes".into(), "zelda.sfc".into())));
    }

    #[test]
    fn lookup_game_entries_empty_keys() {
        let (conn, _dir) = open_temp_db();
        let keys: Vec<(String, String)> = vec![];
        let result = LibraryDb::lookup_game_entries(&conn, &keys).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn lookup_game_entries_missing_entries() {
        let (mut conn, _dir) = open_temp_db();
        LibraryDb::save_system_entries(
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
        let result = LibraryDb::lookup_game_entries(&conn, &keys).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result.contains_key(&("snes".into(), "mario.sfc".into())));
    }

    #[test]
    fn lookup_game_entries_multi_system() {
        let (mut conn, _dir) = open_temp_db();
        LibraryDb::save_system_entries(
            &mut conn,
            "snes",
            &[make_game_entry("snes", "mario.sfc", false)],
            None,
        )
        .unwrap();
        LibraryDb::save_system_entries(
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
        let result = LibraryDb::lookup_game_entries(&conn, &keys).unwrap();
        assert_eq!(result.len(), 2);
        assert!(result.contains_key(&("snes".into(), "mario.sfc".into())));
        assert!(result.contains_key(&("nes".into(), "contra.nes".into())));
    }

    // ── lookup_game_by_path_prefix ──────────────────────────────────

    #[test]
    fn lookup_game_by_path_prefix_finds_longest_match() {
        let (mut conn, _dir) = open_temp_db();
        let mut sub = make_game_entry("sega_smd", "Game.md", false);
        sub.rom_path = "/roms/sega_smd/sub/Game.md".to_string();
        let top = make_game_entry("sega_smd", "Other.md", false);
        LibraryDb::save_system_entries(&mut conn, "sega_smd", &[sub, top], None).unwrap();

        let got =
            LibraryDb::lookup_game_by_path_prefix(&conn, "sega_smd", "/roms/sega_smd/sub/Game.md")
                .unwrap();
        assert_eq!(got.map(|e| e.rom_filename), Some("Game.md".to_string()));
    }

    #[test]
    fn lookup_game_by_path_prefix_no_match_returns_none() {
        let (mut conn, _dir) = open_temp_db();
        let entry = make_game_entry("sega_smd", "Game.md", false);
        LibraryDb::save_system_entries(&mut conn, "sega_smd", &[entry], None).unwrap();

        let got =
            LibraryDb::lookup_game_by_path_prefix(&conn, "sega_smd", "/roms/sega_smd/Missing.md")
                .unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn search_filter_coop_only() {
        let (mut conn, _dir) = open_temp_db();

        let mut coop_game = make_game_entry("snes", "Contra.sfc", false);
        coop_game.cooperative = true;
        let solo_game1 = make_game_entry("snes", "Mario.sfc", false);
        let solo_game2 = make_game_entry("snes", "Zelda.sfc", false);

        LibraryDb::save_system_entries(
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
            LibraryDb::search_game_library(&conn, Some("snes"), None, &[], &filter, 0, 50).unwrap();

        assert_eq!(total, 1);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].rom_filename, "Contra.sfc");
        assert!(entries[0].cooperative);
    }

    // ── save_system_meta zero-overwrite protection ─────────────────────
    //
    // Defends against the NFS startup race documented in
    // `2026-04-29-nfs-startup-race-and-thumbnail-silent-failure.md`: a
    // racy scan that returns rom_count=0 for a system that actually has
    // games (because the storage subdirectory hadn't materialised yet)
    // must not lower a previously valid non-zero count to zero.

    #[test]
    fn save_system_meta_inserts_zero_on_fresh_row() {
        let (conn, _dir) = open_temp_db();
        let stored = LibraryDb::save_system_meta(&conn, "snes", None, 0, 0).unwrap();
        assert_eq!(stored, 0, "fresh insert with 0 is allowed");
    }

    #[test]
    fn save_system_meta_inserts_nonzero_on_fresh_row() {
        let (conn, _dir) = open_temp_db();
        let stored = LibraryDb::save_system_meta(&conn, "snes", Some(123), 42, 1024).unwrap();
        assert_eq!(stored, 42);
    }

    #[test]
    fn save_system_meta_updates_nonzero_to_nonzero() {
        let (conn, _dir) = open_temp_db();
        LibraryDb::save_system_meta(&conn, "snes", None, 10, 1024).unwrap();
        let stored = LibraryDb::save_system_meta(&conn, "snes", None, 20, 2048).unwrap();
        assert_eq!(stored, 20, "non-zero updates pass through");
        let meta = LibraryDb::load_system_meta(&conn, "snes").unwrap().unwrap();
        assert_eq!(meta.total_size_bytes, 2048);
    }

    #[test]
    fn save_system_meta_preserves_nonzero_on_zero_overwrite_attempt() {
        let (conn, _dir) = open_temp_db();
        LibraryDb::save_system_meta(&conn, "snes", Some(100), 8421, 1_000_000).unwrap();

        // Simulate a racy scan returning 0 for a system that has games.
        let stored = LibraryDb::save_system_meta(&conn, "snes", Some(200), 0, 0).unwrap();
        assert_eq!(
            stored, 8421,
            "existing non-zero rom_count must not be clobbered by a zero scan"
        );

        // total_size_bytes is preserved alongside.
        let meta = LibraryDb::load_system_meta(&conn, "snes").unwrap().unwrap();
        assert_eq!(meta.rom_count, 8421);
        assert_eq!(meta.total_size_bytes, 1_000_000);
    }

    #[test]
    fn save_system_meta_zero_to_zero_is_idempotent() {
        let (conn, _dir) = open_temp_db();
        LibraryDb::save_system_meta(&conn, "snes", None, 0, 0).unwrap();
        let stored = LibraryDb::save_system_meta(&conn, "snes", Some(1), 0, 0).unwrap();
        assert_eq!(stored, 0, "zero-to-zero update is a no-op");
    }
}
