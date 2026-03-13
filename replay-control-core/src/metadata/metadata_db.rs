//! Local SQLite cache for external game metadata (descriptions, ratings, etc.).
//!
//! Stored at `<rom_storage>/.replay-control/metadata.db`.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};

use crate::error::{Error, Result};

// Re-export RC_DIR from storage (the canonical definition).
pub use crate::storage::RC_DIR;

/// Enrichment tuple: (filename, box_art_url, genre, players, rating, driver_status).
pub type RomEnrichment = (
    String,
    Option<String>,
    Option<String>,
    Option<u8>,
    Option<f32>,
    Option<String>,
);

/// Filename for the SQLite metadata database.
pub const METADATA_DB_FILE: &str = "metadata.db";
/// Filename for the LaunchBox XML dump.
pub const LAUNCHBOX_XML: &str = "launchbox-metadata.xml";

/// A row from the `data_sources` table.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DataSourceInfo {
    pub source_name: String,
    pub source_type: String,
    pub version_hash: Option<String>,
    pub imported_at: i64,
    pub entry_count: usize,
    pub branch: Option<String>,
}

/// Aggregate stats for a source type.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DataSourceStats {
    pub repo_count: usize,
    pub total_entries: usize,
    pub oldest_imported_at: Option<i64>,
}

/// A single entry from the `thumbnail_index` table.
#[derive(Debug, Clone)]
pub struct ThumbnailIndexEntry {
    pub filename: String,
    pub symlink_target: Option<String>,
}

/// State of a metadata import operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ImportState {
    Downloading,
    BuildingIndex,
    Parsing,
    Complete,
    Failed,
}

/// Progress of an ongoing metadata import.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ImportProgress {
    pub state: ImportState,
    pub processed: usize,
    pub matched: usize,
    pub inserted: usize,
    pub elapsed_secs: u64,
    pub error: Option<String>,
}

/// Per-system metadata coverage.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SystemCoverage {
    pub system: String,
    pub display_name: String,
    pub total_games: usize,
    pub with_metadata: usize,
    pub with_thumbnail: usize,
}

/// Cached metadata for a single game.
#[derive(Debug, Clone)]
pub struct GameMetadata {
    pub description: Option<String>,
    pub rating: Option<f64>,
    pub publisher: Option<String>,
    pub genre: Option<String>,
    pub source: String,
    pub fetched_at: i64,
    pub box_art_path: Option<String>,
    pub screenshot_path: Option<String>,
}

/// Import statistics.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ImportStats {
    pub total_source: usize,
    pub matched: usize,
    pub inserted: usize,
    pub skipped: usize,
}

/// Coverage statistics.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct MetadataStats {
    pub total_entries: usize,
    pub with_description: usize,
    pub with_rating: usize,
    pub db_size_bytes: u64,
    pub last_updated_text: String,
}

/// A cached ROM entry from the `rom_cache` table.
#[derive(Debug, Clone)]
pub struct CachedRom {
    pub system: String,
    pub rom_filename: String,
    pub rom_path: String,
    pub display_name: Option<String>,
    pub size_bytes: u64,
    pub is_m3u: bool,
    pub box_art_url: Option<String>,
    pub driver_status: Option<String>,
    pub genre: Option<String>,
    pub players: Option<u8>,
    pub rating: Option<f32>,
    pub is_clone: bool,
    pub base_title: String,
    pub region: String,
    pub is_translation: bool,
    pub is_hack: bool,
    pub is_special: bool,
}

/// Per-system cache metadata from the `rom_cache_meta` table.
#[derive(Debug, Clone)]
pub struct CachedSystemMeta {
    pub system: String,
    pub dir_mtime_secs: Option<i64>,
    pub scanned_at: i64,
    pub rom_count: usize,
    pub total_size_bytes: u64,
}

/// Handle to the metadata SQLite database.
pub struct MetadataDb {
    conn: Connection,
    db_path: PathBuf,
}

impl MetadataDb {
    /// Tables to probe for corruption detection.
    const TABLES: &[&str] = &[
        "game_metadata",
        "rom_cache",
        "data_sources",
        "thumbnail_index",
    ];

    /// Open (or create) the metadata database at `<storage_root>/.replay-control/metadata.db`.
    ///
    /// Uses the shared nolock→WAL open strategy (see `db_common`), runs table
    /// init, then probes all tables for corruption — auto-recreates if corrupt.
    pub fn open(storage_root: &Path) -> Result<Self> {
        let dir = storage_root.join(RC_DIR);
        std::fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
        let db_path = dir.join(METADATA_DB_FILE);

        let conn = crate::db_common::open_connection(&db_path, "metadata.db")?;
        let db = Self {
            conn,
            db_path: db_path.clone(),
        };
        db.init()?;

        if let Err(detail) = crate::db_common::probe_tables(&db.conn, Self::TABLES) {
            tracing::warn!("Metadata DB corrupt ({detail}), deleting and recreating");
            drop(db);
            crate::db_common::delete_db_files(&db_path);
            let conn = crate::db_common::open_connection(&db_path, "metadata.db")?;
            let db = Self { conn, db_path };
            db.init()?;
            return Ok(db);
        }

        Ok(db)
    }

    /// Create all tables if they don't exist.
    fn init(&self) -> Result<()> {
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS game_metadata (
                    system TEXT NOT NULL,
                    rom_filename TEXT NOT NULL,
                    description TEXT,
                    rating REAL,
                    publisher TEXT,
                    genre TEXT,
                    source TEXT NOT NULL,
                    fetched_at INTEGER NOT NULL,
                    box_art_path TEXT,
                    screenshot_path TEXT,
                    PRIMARY KEY (system, rom_filename)
                );

                CREATE TABLE IF NOT EXISTS data_sources (
                    source_name TEXT PRIMARY KEY,
                    source_type TEXT NOT NULL,
                    version_hash TEXT,
                    imported_at INTEGER NOT NULL,
                    entry_count INTEGER NOT NULL DEFAULT 0,
                    branch TEXT
                );

                CREATE TABLE IF NOT EXISTS thumbnail_index (
                    repo_name TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    filename TEXT NOT NULL,
                    symlink_target TEXT,
                    PRIMARY KEY (repo_name, kind, filename),
                    FOREIGN KEY (repo_name) REFERENCES data_sources(source_name)
                );
                CREATE INDEX IF NOT EXISTS idx_thumbidx_repo ON thumbnail_index(repo_name);

                CREATE TABLE IF NOT EXISTS rom_cache (
                    system TEXT NOT NULL,
                    rom_filename TEXT NOT NULL,
                    rom_path TEXT NOT NULL,
                    display_name TEXT,
                    size_bytes INTEGER NOT NULL DEFAULT 0,
                    is_m3u INTEGER NOT NULL DEFAULT 0,
                    box_art_url TEXT,
                    driver_status TEXT,
                    genre TEXT,
                    players INTEGER,
                    rating REAL,
                    is_clone INTEGER NOT NULL DEFAULT 0,
                    base_title TEXT NOT NULL DEFAULT '',
                    region TEXT NOT NULL DEFAULT '',
                    is_translation INTEGER NOT NULL DEFAULT 0,
                    is_hack INTEGER NOT NULL DEFAULT 0,
                    is_special INTEGER NOT NULL DEFAULT 0,
                    PRIMARY KEY (system, rom_filename)
                );

                CREATE TABLE IF NOT EXISTS rom_cache_meta (
                    system TEXT PRIMARY KEY,
                    dir_mtime_secs INTEGER,
                    scanned_at INTEGER NOT NULL,
                    rom_count INTEGER NOT NULL DEFAULT 0,
                    total_size_bytes INTEGER NOT NULL DEFAULT 0
                );

                CREATE INDEX IF NOT EXISTS idx_rom_cache_genre
                  ON rom_cache (system, genre)
                  WHERE genre IS NOT NULL AND genre != '';",
            )
            .map_err(|e| Error::Other(format!("Failed to create tables: {e}")))?;

        // Migration: add is_special column if upgrading from an older schema.
        // ALTER TABLE ... ADD COLUMN is idempotent-safe: if it already exists,
        // SQLite returns an error we simply ignore.
        let _ = self.conn.execute_batch(
            "ALTER TABLE rom_cache ADD COLUMN is_special INTEGER NOT NULL DEFAULT 0;",
        );

        Ok(())
    }

    /// Look up cached metadata for a specific game.
    pub fn lookup(&self, system: &str, rom_filename: &str) -> Result<Option<GameMetadata>> {
        let result = self
            .conn
            .query_row(
                "SELECT description, rating, publisher, genre, source, fetched_at, box_art_path, screenshot_path
                 FROM game_metadata WHERE system = ?1 AND rom_filename = ?2",
                params![system, rom_filename],
                |row| {
                    Ok(GameMetadata {
                        description: row.get(0)?,
                        rating: row.get(1)?,
                        publisher: row.get(2)?,
                        genre: row.get(3)?,
                        source: row.get(4)?,
                        fetched_at: row.get(5)?,
                        box_art_path: row.get(6)?,
                        screenshot_path: row.get(7)?,
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
        // Use a prepared statement and iterate — avoids building dynamic SQL
        // while still being efficient (single prepared statement, many binds).
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
    /// Used to fill empty `rom_cache.genre` entries during enrichment.
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

    /// Fetch current genres from `rom_cache` for a single system.
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
                "SELECT rom_filename, genre FROM rom_cache
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

    /// Fetch all metadata entries for a system.
    /// Returns a vec of `(rom_filename, GameMetadata)`.
    /// Used for normalized-title matching when enriching new ROMs.
    pub fn system_metadata_all(&self, system: &str) -> Result<Vec<(String, GameMetadata)>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT rom_filename, description, rating, publisher, genre, source, fetched_at,
                        box_art_path, screenshot_path
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
                        genre: row.get(4)?,
                        source: row.get(5)?,
                        fetched_at: row.get(6)?,
                        box_art_path: row.get(7)?,
                        screenshot_path: row.get(8)?,
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
                "INSERT INTO game_metadata (system, rom_filename, description, rating, publisher, source, fetched_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(system, rom_filename) DO UPDATE SET
                    description = excluded.description,
                    rating = excluded.rating,
                    publisher = excluded.publisher,
                    source = excluded.source,
                    fetched_at = excluded.fetched_at",
                params![
                    system,
                    rom_filename,
                    meta.description,
                    meta.rating,
                    meta.publisher,
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
                    "INSERT INTO game_metadata (system, rom_filename, description, rating, publisher, genre, source, fetched_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                     ON CONFLICT(system, rom_filename) DO UPDATE SET
                        description = excluded.description,
                        rating = excluded.rating,
                        publisher = excluded.publisher,
                        genre = excluded.genre,
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
                    meta.genre,
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

    /// Count metadata entries per system, ordered by count descending.
    ///
    /// Uses a LEFT JOIN with rom_cache for M3U dedup: when rom_cache is populated
    /// for a system, only entries matching rom_cache are counted (disc files
    /// referenced by .m3u playlists are excluded). When rom_cache is empty for a
    /// system (e.g. cache not yet warmed after import), all game_metadata entries
    /// are counted as a fallback to avoid showing 0.
    pub fn entries_per_system(&self) -> Result<Vec<(String, usize)>> {
        let mut stmt = self
            .conn
            .prepare(
                // LEFT JOIN + NOT EXISTS fallback: use rom_cache for M3U dedup
                // when available, fall back to raw game_metadata count when
                // rom_cache is empty for a system.
                "SELECT gm.system, COUNT(*) as cnt
                 FROM game_metadata gm
                 LEFT JOIN rom_cache rc ON gm.system = rc.system AND gm.rom_filename = rc.rom_filename
                 WHERE rc.rom_filename IS NOT NULL
                    OR NOT EXISTS (SELECT 1 FROM rom_cache rc2 WHERE rc2.system = gm.system)
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

            // Also prepare an INSERT for games that might not have a metadata row yet.
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
                    // No existing row — insert a minimal one with image paths.
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
    pub fn image_stats(&self) -> Result<(usize, usize)> {
        let with_boxart: usize = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM game_metadata WHERE box_art_path IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .map_err(|e| Error::Other(format!("Image stats query failed: {e}")))?;
        let with_screenshot: usize = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM game_metadata WHERE screenshot_path IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .map_err(|e| Error::Other(format!("Image stats query failed: {e}")))?;
        Ok((with_boxart, with_screenshot))
    }

    /// Count image entries per system.
    ///
    /// Same LEFT JOIN + NOT EXISTS fallback as [`entries_per_system`] — see its
    /// doc comment for rationale on M3U dedup vs empty-cache fallback.
    pub fn images_per_system(&self) -> Result<Vec<(String, usize, usize)>> {
        let mut stmt = self
            .conn
            .prepare(
                // See entries_per_system() for why we use LEFT JOIN + NOT EXISTS.
                "SELECT gm.system,
                        SUM(CASE WHEN gm.box_art_path IS NOT NULL THEN 1 ELSE 0 END),
                        SUM(CASE WHEN gm.screenshot_path IS NOT NULL THEN 1 ELSE 0 END)
                 FROM game_metadata gm
                 LEFT JOIN rom_cache rc ON gm.system = rc.system AND gm.rom_filename = rc.rom_filename
                 WHERE rc.rom_filename IS NOT NULL
                    OR NOT EXISTS (SELECT 1 FROM rom_cache rc2 WHERE rc2.system = gm.system)
                 GROUP BY gm.system",
            )
            .map_err(|e| Error::Other(format!("Query failed: {e}")))?;

        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, usize>(1)?,
                    row.get::<_, usize>(2)?,
                ))
            })
            .map_err(|e| Error::Other(format!("Query failed: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Get path to the database file.
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    // ── ROM Cache (L2 persistent cache) ──────────────────────────────

    /// Save a system's ROM list to the rom_cache table.
    /// Replaces all existing entries for the system in a single transaction.
    pub fn save_system_roms(
        &mut self,
        system: &str,
        roms: &[CachedRom],
        dir_mtime_secs: Option<i64>,
    ) -> Result<()> {
        let tx = self
            .conn
            .transaction()
            .map_err(|e| Error::Other(format!("Transaction start failed: {e}")))?;

        // Delete existing entries for this system.
        tx.execute("DELETE FROM rom_cache WHERE system = ?1", params![system])
            .map_err(|e| Error::Other(format!("Delete rom_cache failed: {e}")))?;

        {
            let mut stmt = tx
                .prepare(
                    "INSERT OR IGNORE INTO rom_cache (system, rom_filename, rom_path, display_name,
                     size_bytes, is_m3u, box_art_url, driver_status, genre, players, rating,
                     is_clone, base_title, region, is_translation, is_hack, is_special)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
                )
                .map_err(|e| Error::Other(format!("Prepare rom_cache insert: {e}")))?;

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
                    rom.players.map(|p| p as i32),
                    rom.rating,
                    rom.is_clone,
                    &rom.base_title,
                    &rom.region,
                    rom.is_translation,
                    rom.is_hack,
                    rom.is_special,
                ])
                .map_err(|e| Error::Other(format!("Insert rom_cache failed: {e}")))?;
            }
        }

        // Update system metadata.
        let total_size: u64 = roms.iter().map(|r| r.size_bytes).sum();
        let now = unix_now();
        tx.execute(
            "INSERT INTO rom_cache_meta (system, dir_mtime_secs, scanned_at, rom_count, total_size_bytes)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(system) DO UPDATE SET
                dir_mtime_secs = excluded.dir_mtime_secs,
                scanned_at = excluded.scanned_at,
                rom_count = excluded.rom_count,
                total_size_bytes = excluded.total_size_bytes",
            params![system, dir_mtime_secs, now, roms.len() as i64, total_size as i64],
        )
        .map_err(|e| Error::Other(format!("Upsert rom_cache_meta failed: {e}")))?;

        tx.commit()
            .map_err(|e| Error::Other(format!("Transaction commit failed: {e}")))?;

        Ok(())
    }

    /// Load all cached ROMs for a system.
    pub fn load_system_roms(&self, system: &str) -> Result<Vec<CachedRom>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT system, rom_filename, rom_path, display_name, size_bytes,
                        is_m3u, box_art_url, driver_status, genre, players, rating,
                        is_clone, base_title, region, is_translation, is_hack, is_special
                 FROM rom_cache WHERE system = ?1",
            )
            .map_err(|e| Error::Other(format!("Prepare load_system_roms: {e}")))?;

        let rows = stmt
            .query_map(params![system], Self::row_to_cached_rom)
            .map_err(|e| Error::Other(format!("Query load_system_roms: {e}")))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| Error::Other(format!("Row read failed: {e}")))?);
        }
        Ok(result)
    }

    /// Save just the system-level metadata (counts, mtime) without replacing ROM entries.
    /// Used when we know game counts from scan_systems but haven't loaded ROMs yet.
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
                "INSERT INTO rom_cache_meta (system, dir_mtime_secs, scanned_at, rom_count, total_size_bytes)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(system) DO UPDATE SET
                    dir_mtime_secs = excluded.dir_mtime_secs,
                    scanned_at = excluded.scanned_at,
                    rom_count = excluded.rom_count,
                    total_size_bytes = excluded.total_size_bytes",
                rusqlite::params![system, dir_mtime_secs, now, rom_count as i64, total_size_bytes as i64],
            )
            .map_err(|e| Error::Other(format!("Upsert rom_cache_meta: {e}")))?;
        Ok(())
    }

    /// Load cache metadata for a single system.
    pub fn load_system_meta(&self, system: &str) -> Result<Option<CachedSystemMeta>> {
        self.conn
            .query_row(
                "SELECT system, dir_mtime_secs, scanned_at, rom_count, total_size_bytes
                 FROM rom_cache_meta WHERE system = ?1",
                params![system],
                |row| {
                    Ok(CachedSystemMeta {
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

    /// Load cache metadata for all systems.
    pub fn load_all_system_meta(&self) -> Result<Vec<CachedSystemMeta>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT system, dir_mtime_secs, scanned_at, rom_count, total_size_bytes
                 FROM rom_cache_meta",
            )
            .map_err(|e| Error::Other(format!("Prepare load_all_system_meta: {e}")))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(CachedSystemMeta {
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

    /// Batch update enrichment fields (box_art_url, genre, players, rating, driver_status)
    /// for ROMs already in the cache.
    pub fn update_rom_enrichment(
        &mut self,
        system: &str,
        enrichments: &[RomEnrichment],
    ) -> Result<usize> {
        let tx = self
            .conn
            .transaction()
            .map_err(|e| Error::Other(format!("Transaction start failed: {e}")))?;

        let mut count = 0usize;
        {
            let mut stmt = tx
                .prepare(
                    "UPDATE rom_cache SET box_art_url = ?2, genre = ?3, players = ?4,
                            rating = ?5, driver_status = ?6
                     WHERE system = ?7 AND rom_filename = ?1",
                )
                .map_err(|e| Error::Other(format!("Prepare update_rom_enrichment: {e}")))?;

            for (filename, box_art_url, genre, players, rating, driver_status) in enrichments {
                let updated = stmt
                    .execute(params![
                        filename,
                        box_art_url,
                        genre,
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

    /// Batch update box_art_url, genre, and rating for ROMs in rom_cache.
    /// Only updates non-None fields. Genre is only set when the existing
    /// value is NULL or empty (baked-in genre is never overwritten).
    pub fn update_box_art_genre_rating(
        &mut self,
        system: &str,
        enrichments: &[(String, Option<String>, Option<String>, Option<f32>)],
    ) -> Result<()> {
        let tx = self
            .conn
            .transaction()
            .map_err(|e| Error::Other(format!("Transaction start failed: {e}")))?;

        {
            let mut art_stmt = tx
                .prepare(
                    "UPDATE rom_cache SET box_art_url = ?2
                     WHERE system = ?3 AND rom_filename = ?1",
                )
                .map_err(|e| Error::Other(format!("Prepare box_art update: {e}")))?;

            let mut genre_stmt = tx
                .prepare(
                    "UPDATE rom_cache SET genre = ?2
                     WHERE system = ?3 AND rom_filename = ?1
                       AND (genre IS NULL OR genre = '')",
                )
                .map_err(|e| Error::Other(format!("Prepare genre update: {e}")))?;

            let mut rating_stmt = tx
                .prepare(
                    "UPDATE rom_cache SET rating = ?2
                     WHERE system = ?3 AND rom_filename = ?1",
                )
                .map_err(|e| Error::Other(format!("Prepare rating update: {e}")))?;

            for (filename, box_art_url, genre, rating) in enrichments {
                if let Some(url) = box_art_url {
                    art_stmt
                        .execute(params![filename, url, system])
                        .map_err(|e| Error::Other(format!("Update box_art_url: {e}")))?;
                }
                if let Some(g) = genre {
                    genre_stmt
                        .execute(params![filename, g, system])
                        .map_err(|e| Error::Other(format!("Update genre: {e}")))?;
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

    /// Clear the rom_cache and rom_cache_meta for a specific system.
    pub fn clear_system_rom_cache(&self, system: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM rom_cache WHERE system = ?1", params![system])
            .map_err(|e| Error::Other(format!("Clear system rom_cache: {e}")))?;
        self.conn
            .execute(
                "DELETE FROM rom_cache_meta WHERE system = ?1",
                params![system],
            )
            .map_err(|e| Error::Other(format!("Clear system rom_cache_meta: {e}")))?;
        Ok(())
    }

    /// Get filenames of visible games for a system (excludes disc files hidden by M3U dedup).
    pub fn visible_filenames(&self, system: &str) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT rom_filename FROM rom_cache WHERE system = ?1")
            .map_err(|e| Error::Other(format!("Query failed: {e}")))?;
        let rows = stmt
            .query_map(params![system], |row| row.get(0))
            .map_err(|e| Error::Other(format!("Query failed: {e}")))?;
        Ok(rows.flatten().collect())
    }

    /// Clear all rom_cache and rom_cache_meta entries.
    pub fn clear_all_rom_cache(&self) -> Result<()> {
        self.conn
            .execute("DELETE FROM rom_cache", [])
            .map_err(|e| Error::Other(format!("Clear rom_cache: {e}")))?;
        self.conn
            .execute("DELETE FROM rom_cache_meta", [])
            .map_err(|e| Error::Other(format!("Clear rom_cache_meta: {e}")))?;
        Ok(())
    }

    // ── Data Sources ─────────────────────────────────────────────────

    /// Insert or update a data source entry.
    pub fn upsert_data_source(
        &self,
        source_name: &str,
        source_type: &str,
        version_hash: &str,
        branch: &str,
        entry_count: usize,
    ) -> Result<()> {
        let now = unix_now();
        self.conn
            .execute(
                "INSERT INTO data_sources (source_name, source_type, version_hash, imported_at, entry_count, branch)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(source_name) DO UPDATE SET
                    version_hash = excluded.version_hash,
                    imported_at = excluded.imported_at,
                    entry_count = excluded.entry_count,
                    branch = excluded.branch",
                params![source_name, source_type, version_hash, now, entry_count as i64, branch],
            )
            .map_err(|e| Error::Other(format!("Upsert data_source failed: {e}")))?;
        Ok(())
    }

    /// Look up a single data source.
    pub fn get_data_source(&self, source_name: &str) -> Result<Option<DataSourceInfo>> {
        self.conn
            .query_row(
                "SELECT source_name, source_type, version_hash, imported_at, entry_count, branch
                 FROM data_sources WHERE source_name = ?1",
                params![source_name],
                |row| {
                    Ok(DataSourceInfo {
                        source_name: row.get(0)?,
                        source_type: row.get(1)?,
                        version_hash: row.get(2)?,
                        imported_at: row.get(3)?,
                        entry_count: row.get::<_, i64>(4)? as usize,
                        branch: row.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(|e| Error::Other(format!("get_data_source failed: {e}")))
    }

    /// Get aggregate stats for a source type (e.g., "libretro-thumbnails").
    pub fn get_data_source_stats(&self, source_type: &str) -> Result<DataSourceStats> {
        self.conn
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(entry_count), 0), MIN(imported_at)
                 FROM data_sources WHERE source_type = ?1",
                params![source_type],
                |row| {
                    Ok(DataSourceStats {
                        repo_count: row.get::<_, i64>(0)? as usize,
                        total_entries: row.get::<_, i64>(1)? as usize,
                        oldest_imported_at: row.get(2)?,
                    })
                },
            )
            .map_err(|e| Error::Other(format!("get_data_source_stats failed: {e}")))
    }

    // ── Thumbnail Index ─────────────────────────────────────────────

    /// Query thumbnail_index entries for a given repo and kind.
    pub fn query_thumbnail_index(
        &self,
        repo_name: &str,
        kind: &str,
    ) -> Result<Vec<ThumbnailIndexEntry>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT filename, symlink_target
                 FROM thumbnail_index
                 WHERE repo_name = ?1 AND kind = ?2",
            )
            .map_err(|e| Error::Other(format!("Prepare query_thumbnail_index: {e}")))?;

        let rows = stmt
            .query_map(params![repo_name, kind], |row| {
                Ok(ThumbnailIndexEntry {
                    filename: row.get(0)?,
                    symlink_target: row.get(1)?,
                })
            })
            .map_err(|e| Error::Other(format!("Query thumbnail_index: {e}")))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| Error::Other(format!("Row read failed: {e}")))?);
        }
        Ok(result)
    }

    /// Delete all thumbnail_index entries for a given repo.
    pub fn delete_thumbnail_index(&self, repo_name: &str) -> Result<usize> {
        let count = self
            .conn
            .execute(
                "DELETE FROM thumbnail_index WHERE repo_name = ?1",
                params![repo_name],
            )
            .map_err(|e| Error::Other(format!("delete_thumbnail_index failed: {e}")))?;
        Ok(count)
    }

    /// Bulk insert thumbnail_index entries within a single transaction.
    /// Deletes existing entries for the repo first.
    pub fn bulk_insert_thumbnail_index(
        &mut self,
        repo_name: &str,
        entries: &[(String, String, Option<String>)], // (kind, filename, symlink_target)
    ) -> Result<usize> {
        let tx = self
            .conn
            .transaction()
            .map_err(|e| Error::Other(format!("Transaction start failed: {e}")))?;

        // Delete existing entries for this repo.
        tx.execute(
            "DELETE FROM thumbnail_index WHERE repo_name = ?1",
            params![repo_name],
        )
        .map_err(|e| Error::Other(format!("Delete thumbnail_index failed: {e}")))?;

        let mut count = 0usize;
        {
            let mut stmt = tx
                .prepare(
                    "INSERT OR REPLACE INTO thumbnail_index
                     (repo_name, kind, filename, symlink_target)
                     VALUES (?1, ?2, ?3, ?4)",
                )
                .map_err(|e| Error::Other(format!("Prepare failed: {e}")))?;

            for (kind, filename, symlink_target) in entries {
                stmt.execute(params![repo_name, kind, filename, symlink_target])
                    .map_err(|e| Error::Other(format!("Insert thumbnail_index failed: {e}")))?;
                count += 1;
            }
        }

        tx.commit()
            .map_err(|e| Error::Other(format!("Transaction commit failed: {e}")))?;
        Ok(count)
    }

    /// Clear all thumbnail index entries and their data_sources rows.
    pub fn clear_thumbnail_index(&self) -> Result<()> {
        self.conn
            .execute("DELETE FROM thumbnail_index", [])
            .map_err(|e| Error::Other(format!("Clear thumbnail_index failed: {e}")))?;
        self.conn
            .execute(
                "DELETE FROM data_sources WHERE source_type = 'libretro-thumbnails'",
                [],
            )
            .map_err(|e| Error::Other(format!("Clear libretro data_sources failed: {e}")))?;
        Ok(())
    }

    /// Provide a reference to the raw connection (for use by thumbnail_manifest).
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    // ── SQL-Based Recommendation Queries ─────────────────────────────

    /// Get random cached ROMs with box art from all systems.
    /// Returns a diverse selection across different systems.
    /// Filters out arcade clones and deduplicates regional variants,
    /// preferring the user's region preference.
    pub fn random_cached_roms_diverse(
        &self,
        count: usize,
        region_pref: &str,
    ) -> Result<Vec<CachedRom>> {
        let mut stmt = self
            .conn
            .prepare(
                "WITH deduped AS (
                    SELECT *, ROW_NUMBER() OVER (
                        PARTITION BY system, base_title
                        ORDER BY CASE
                            WHEN region = ?2 THEN 0
                            WHEN region = 'world' THEN 1
                            ELSE 2
                        END
                    ) AS rn
                    FROM rom_cache
                    WHERE is_clone = 0 AND is_translation = 0 AND is_hack = 0 AND is_special = 0
                )
                SELECT system, rom_filename, rom_path, display_name, size_bytes,
                        is_m3u, box_art_url, driver_status, genre, players, rating,
                        is_clone, base_title, region, is_translation, is_hack, is_special
                FROM deduped WHERE rn = 1
                ORDER BY RANDOM() LIMIT ?1",
            )
            .map_err(|e| Error::Other(format!("Prepare random_cached_roms_diverse: {e}")))?;

        let rows = stmt
            .query_map(params![(count * 5) as i64, region_pref], Self::row_to_cached_rom)
            .map_err(|e| Error::Other(format!("Query random_cached_roms_diverse: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Get random cached ROMs with box art from a specific system.
    pub fn random_cached_roms(&self, system: &str, count: usize) -> Result<Vec<CachedRom>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT system, rom_filename, rom_path, display_name, size_bytes,
                        is_m3u, box_art_url, driver_status, genre, players, rating,
                        is_clone, base_title, region, is_translation, is_hack, is_special
                 FROM rom_cache
                 WHERE system = ?1 AND box_art_url IS NOT NULL AND is_special = 0
                 ORDER BY RANDOM() LIMIT ?2",
            )
            .map_err(|e| Error::Other(format!("Prepare random_cached_roms: {e}")))?;

        let rows = stmt
            .query_map(params![system, count as i64], Self::row_to_cached_rom)
            .map_err(|e| Error::Other(format!("Query random_cached_roms: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Get top-rated cached ROMs across all systems.
    /// Filters out arcade clones and deduplicates regional variants,
    /// then selects from the top-rated pool and randomizes within it
    /// so each page load shows a different selection of highly-rated games.
    pub fn top_rated_cached_roms(
        &self,
        count: usize,
        region_pref: &str,
    ) -> Result<Vec<CachedRom>> {
        let pool_size = (count * 4).max(40) as i64;
        let mut stmt = self
            .conn
            .prepare(
                "WITH deduped AS (
                    SELECT *, ROW_NUMBER() OVER (
                        PARTITION BY system, base_title
                        ORDER BY CASE
                            WHEN region = ?2 THEN 0
                            WHEN region = 'world' THEN 1
                            ELSE 2
                        END
                    ) AS rn
                    FROM rom_cache
                    WHERE is_clone = 0 AND is_translation = 0 AND is_hack = 0 AND is_special = 0 AND rating IS NOT NULL AND rating > 0
                )
                SELECT system, rom_filename, rom_path, display_name, size_bytes,
                        is_m3u, box_art_url, driver_status, genre, players, rating,
                        is_clone, base_title, region, is_translation, is_hack, is_special
                FROM (
                    SELECT * FROM deduped WHERE rn = 1
                    ORDER BY rating DESC
                    LIMIT ?1
                )
                ORDER BY RANDOM()",
            )
            .map_err(|e| Error::Other(format!("Prepare top_rated_cached_roms: {e}")))?;

        let rows = stmt
            .query_map(params![pool_size, region_pref], Self::row_to_cached_rom)
            .map_err(|e| Error::Other(format!("Query top_rated_cached_roms: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Get genre counts across the entire library.
    pub fn genre_counts(&self) -> Result<Vec<(String, usize)>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT genre, COUNT(*) as cnt FROM rom_cache
                 WHERE genre IS NOT NULL AND genre != ''
                 GROUP BY genre ORDER BY cnt DESC",
            )
            .map_err(|e| Error::Other(format!("Prepare genre_counts: {e}")))?;

        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, usize>(1)?))
            })
            .map_err(|e| Error::Other(format!("Query genre_counts: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Count multiplayer games (players >= 2) across the entire library.
    pub fn multiplayer_count(&self) -> Result<usize> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM rom_cache WHERE players IS NOT NULL AND players >= 2",
                [],
                |row| row.get(0),
            )
            .map_err(|e| Error::Other(format!("Query multiplayer_count: {e}")))
    }

    /// Get non-favorited ROMs from a system, optionally filtered by genre.
    /// Filters out arcade clones and deduplicates regional variants.
    /// Selects from top-rated games and randomizes via SQL so each load
    /// shows different recommendations. Used for "Because You Love" section.
    pub fn system_roms_excluding(
        &self,
        system: &str,
        exclude_filenames: &[&str],
        genre_filter: Option<&str>,
        count: usize,
        region_pref: &str,
    ) -> Result<Vec<CachedRom>> {
        let exclude_set: std::collections::HashSet<&str> =
            exclude_filenames.iter().copied().collect();

        // Fetch a larger pool to allow for exclusion filtering.
        let limit = ((count + exclude_filenames.len()) * 4).max(40) as i64;

        let roms = if let Some(genre) = genre_filter {
            let mut stmt = self
                .conn
                .prepare(
                    "WITH deduped AS (
                        SELECT *, ROW_NUMBER() OVER (
                            PARTITION BY system, base_title
                            ORDER BY CASE
                                WHEN region = ?4 THEN 0
                                WHEN region = 'world' THEN 1
                                ELSE 2
                            END
                        ) AS rn
                        FROM rom_cache
                        WHERE system = ?1 AND genre = ?2 AND is_clone = 0 AND is_translation = 0 AND is_hack = 0 AND is_special = 0
                    )
                    SELECT system, rom_filename, rom_path, display_name, size_bytes,
                            is_m3u, box_art_url, driver_status, genre, players, rating,
                            is_clone, base_title, region, is_translation, is_hack, is_special
                    FROM (
                        SELECT * FROM deduped WHERE rn = 1
                        ORDER BY rating DESC NULLS LAST
                        LIMIT ?3
                    )
                    ORDER BY RANDOM()",
                )
                .map_err(|e| Error::Other(format!("Prepare system_roms_excluding: {e}")))?;

            let rows = stmt
                .query_map(
                    params![system, genre, limit, region_pref],
                    Self::row_to_cached_rom,
                )
                .map_err(|e| Error::Other(format!("Query system_roms_excluding: {e}")))?;
            rows.flatten().collect::<Vec<_>>()
        } else {
            let mut stmt = self
                .conn
                .prepare(
                    "WITH deduped AS (
                        SELECT *, ROW_NUMBER() OVER (
                            PARTITION BY system, base_title
                            ORDER BY CASE
                                WHEN region = ?3 THEN 0
                                WHEN region = 'world' THEN 1
                                ELSE 2
                            END
                        ) AS rn
                        FROM rom_cache
                        WHERE system = ?1 AND is_clone = 0 AND is_translation = 0 AND is_hack = 0 AND is_special = 0
                    )
                    SELECT system, rom_filename, rom_path, display_name, size_bytes,
                            is_m3u, box_art_url, driver_status, genre, players, rating,
                            is_clone, base_title, region, is_translation, is_hack, is_special
                    FROM (
                        SELECT * FROM deduped WHERE rn = 1
                        ORDER BY rating DESC NULLS LAST
                         LIMIT ?2
                     )
                     ORDER BY RANDOM()",
                )
                .map_err(|e| Error::Other(format!("Prepare system_roms_excluding: {e}")))?;

            let rows = stmt
                .query_map(params![system, limit, region_pref], Self::row_to_cached_rom)
                .map_err(|e| Error::Other(format!("Query system_roms_excluding: {e}")))?;
            rows.flatten().collect::<Vec<_>>()
        };

        Ok(roms
            .into_iter()
            .filter(|r| !exclude_set.contains(r.rom_filename.as_str()))
            .take(count)
            .collect())
    }

    /// Find regional variants of a game: other ROMs sharing the same base_title.
    /// Returns (rom_filename, region) pairs sorted by region priority (USA, Europe, Japan first).
    /// Returns an empty Vec if the game has no base_title or only one region exists.
    /// Find regional variants of a game: other ROMs sharing the same base_title
    /// that are not translations, hacks, specials, or arcade clones.
    /// Returns `(rom_filename, region, display_name)` tuples.
    pub fn regional_variants(
        &self,
        system: &str,
        rom_filename: &str,
    ) -> Result<Vec<(String, String, Option<String>)>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT rom_filename, region, display_name FROM rom_cache
                 WHERE system = ?1
                   AND base_title != ''
                   AND is_translation = 0
                   AND is_hack = 0
                   AND is_special = 0
                   AND is_clone = 0
                   AND base_title = (
                       SELECT base_title FROM rom_cache
                       WHERE system = ?1 AND rom_filename = ?2
                   )
                 ORDER BY
                   CASE region
                       WHEN 'USA' THEN 1
                       WHEN 'Europe' THEN 2
                       WHEN 'Japan' THEN 3
                       ELSE 4
                   END,
                   region",
            )
            .map_err(|e| Error::Other(format!("Prepare regional_variants: {e}")))?;

        let rows = stmt
            .query_map(params![system, rom_filename], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })
            .map_err(|e| Error::Other(format!("Query regional_variants: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Find translations of a game: other ROMs sharing the same base_title that are translations.
    /// Returns (rom_filename, display_name) pairs sorted by display_name.
    /// Returns an empty Vec if the game has no base_title or no translations exist.
    pub fn translations(
        &self,
        system: &str,
        rom_filename: &str,
    ) -> Result<Vec<(String, Option<String>)>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT rom_filename, display_name FROM rom_cache
                 WHERE system = ?1
                   AND base_title != ''
                   AND is_translation = 1
                   AND base_title = (
                       SELECT base_title FROM rom_cache
                       WHERE system = ?1 AND rom_filename = ?2
                   )
                 ORDER BY display_name",
            )
            .map_err(|e| Error::Other(format!("Prepare translations: {e}")))?;

        let rows = stmt
            .query_map(params![system, rom_filename], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })
            .map_err(|e| Error::Other(format!("Query translations: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Find hacks of a game: other ROMs sharing the same base_title that are hacks.
    /// Returns (rom_filename, display_name) pairs sorted by display_name.
    /// Returns an empty Vec if the game has no base_title or no hacks exist.
    pub fn hacks(
        &self,
        system: &str,
        rom_filename: &str,
    ) -> Result<Vec<(String, Option<String>)>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT rom_filename, display_name FROM rom_cache
                 WHERE system = ?1
                   AND base_title != ''
                   AND is_hack = 1
                   AND base_title = (
                       SELECT base_title FROM rom_cache
                       WHERE system = ?1 AND rom_filename = ?2
                   )
                 ORDER BY display_name",
            )
            .map_err(|e| Error::Other(format!("Prepare hacks: {e}")))?;

        let rows = stmt
            .query_map(params![system, rom_filename], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })
            .map_err(|e| Error::Other(format!("Query hacks: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Find special versions of a game: other ROMs sharing the same base_title that are special.
    /// Returns (rom_filename, display_name) pairs sorted by display_name.
    /// Returns an empty Vec if the game has no base_title or no specials exist.
    pub fn specials(
        &self,
        system: &str,
        rom_filename: &str,
    ) -> Result<Vec<(String, Option<String>)>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT rom_filename, display_name FROM rom_cache
                 WHERE system = ?1
                   AND base_title != ''
                   AND is_special = 1
                   AND base_title = (
                       SELECT base_title FROM rom_cache
                       WHERE system = ?1 AND rom_filename = ?2
                   )
                 ORDER BY display_name",
            )
            .map_err(|e| Error::Other(format!("Prepare specials: {e}")))?;

        let rows = stmt
            .query_map(params![system, rom_filename], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })
            .map_err(|e| Error::Other(format!("Query specials: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Find similar games by genre within the same system.
    /// Excludes the given ROM, clones, and games without a genre.
    /// Returns randomized results up to `limit`.
    pub fn similar_by_genre(
        &self,
        system: &str,
        genre: &str,
        exclude_filename: &str,
        limit: usize,
    ) -> Result<Vec<CachedRom>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT system, rom_filename, rom_path, display_name, size_bytes,
                        is_m3u, box_art_url, driver_status, genre, players, rating,
                        is_clone, base_title, region, is_translation, is_hack, is_special
                 FROM rom_cache
                 WHERE system = ?1
                   AND genre = ?2
                   AND genre != ''
                   AND rom_filename != ?3
                   AND is_clone = 0
                   AND is_translation = 0
                   AND is_hack = 0
                   AND is_special = 0
                 ORDER BY RANDOM()
                 LIMIT ?4",
            )
            .map_err(|e| Error::Other(format!("Prepare similar_by_genre: {e}")))?;

        let rows = stmt
            .query_map(
                params![system, genre, exclude_filename, limit as i64],
                Self::row_to_cached_rom,
            )
            .map_err(|e| Error::Other(format!("Query similar_by_genre: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Helper: convert a row to CachedRom (used by multiple queries).
    fn row_to_cached_rom(row: &rusqlite::Row<'_>) -> rusqlite::Result<CachedRom> {
        Ok(CachedRom {
            system: row.get(0)?,
            rom_filename: row.get(1)?,
            rom_path: row.get(2)?,
            display_name: row.get(3)?,
            size_bytes: row.get::<_, i64>(4)? as u64,
            is_m3u: row.get(5)?,
            box_art_url: row.get(6)?,
            driver_status: row.get(7)?,
            genre: row.get(8)?,
            players: row.get::<_, Option<i32>>(9)?.map(|p| p as u8),
            rating: row.get(10)?,
            is_clone: row.get(11)?,
            base_title: row.get(12)?,
            region: row.get(13)?,
            is_translation: row.get(14)?,
            is_hack: row.get(15)?,
            is_special: row.get(16)?,
        })
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Open a MetadataDb backed by a temp directory.
    fn open_temp_db() -> (MetadataDb, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db = MetadataDb::open(dir.path()).unwrap();
        (db, dir)
    }

    fn make_metadata(box_art: Option<&str>) -> GameMetadata {
        GameMetadata {
            description: None,
            rating: Some(4.0),
            publisher: None,
            genre: None,
            source: "test".into(),
            fetched_at: 0,
            box_art_path: box_art.map(String::from),
            screenshot_path: None,
        }
    }

    fn make_metadata_with_genre(genre: &str) -> GameMetadata {
        GameMetadata {
            genre: Some(genre.into()),
            ..make_metadata(None)
        }
    }

    fn make_cached_rom(system: &str, filename: &str, is_m3u: bool) -> CachedRom {
        CachedRom {
            system: system.into(),
            rom_filename: filename.into(),
            rom_path: format!("/roms/{system}/{filename}"),
            display_name: None,
            size_bytes: 1000,
            is_m3u,
            box_art_url: None,
            driver_status: None,
            genre: None,
            players: None,
            rating: None,
            is_clone: false,
            base_title: String::new(),
            region: String::new(),
            is_translation: false,
            is_hack: false,
            is_special: false,
        }
    }

    fn make_cached_rom_with_genre(system: &str, filename: &str, genre: &str) -> CachedRom {
        CachedRom {
            genre: Some(genre.into()),
            ..make_cached_rom(system, filename, false)
        }
    }

    #[test]
    fn genre_enrichment_fills_empty_genre_from_launchbox() {
        // When rom_cache has no genre but game_metadata does,
        // update_box_art_genre_rating should fill it.
        let (mut db, _dir) = open_temp_db();

        // game_metadata has genre "Platform" for Sonic
        db.bulk_upsert(&[
            ("sega_smd".into(), "Sonic.md".into(), make_metadata_with_genre("Platform")),
        ]).unwrap();

        // rom_cache has Sonic with no genre
        db.save_system_roms("sega_smd", &[
            make_cached_rom("sega_smd", "Sonic.md", false),
        ], None).unwrap();

        // Enrich with genre from LaunchBox
        db.update_box_art_genre_rating("sega_smd", &[
            ("Sonic.md".into(), None, Some("Platform".into()), None),
        ]).unwrap();

        let roms = db.load_system_roms("sega_smd").unwrap();
        assert_eq!(roms[0].genre.as_deref(), Some("Platform"));
    }

    #[test]
    fn genre_enrichment_does_not_overwrite_existing_genre() {
        // When rom_cache already has a baked-in genre, the SQL guard
        // (genre IS NULL OR genre = '') should prevent overwriting it.
        let (mut db, _dir) = open_temp_db();

        // rom_cache has Sonic with baked-in "Shooter" (wrong but exists)
        db.save_system_roms("sega_smd", &[
            make_cached_rom_with_genre("sega_smd", "Sonic.md", "Shooter"),
        ], None).unwrap();

        // Try to enrich with "Platform" from LaunchBox
        db.update_box_art_genre_rating("sega_smd", &[
            ("Sonic.md".into(), None, Some("Platform".into()), None),
        ]).unwrap();

        let roms = db.load_system_roms("sega_smd").unwrap();
        // Should still be "Shooter" — not overwritten
        assert_eq!(roms[0].genre.as_deref(), Some("Shooter"));
    }

    #[test]
    fn genre_enrichment_mixed_empty_and_existing() {
        // Multiple ROMs: some with genre, some without. Only empty ones get filled.
        let (mut db, _dir) = open_temp_db();

        db.save_system_roms("sega_smd", &[
            make_cached_rom_with_genre("sega_smd", "Sonic.md", "Shooter"),
            make_cached_rom("sega_smd", "Streets.md", false),
            make_cached_rom("sega_smd", "Columns.md", false),
        ], None).unwrap();

        db.update_box_art_genre_rating("sega_smd", &[
            ("Sonic.md".into(), None, Some("Platform".into()), None),
            ("Streets.md".into(), None, Some("Beat'em Up".into()), None),
            // Columns has no LaunchBox genre either — no enrichment tuple
        ]).unwrap();

        let roms = db.load_system_roms("sega_smd").unwrap();
        let sonic = roms.iter().find(|r| r.rom_filename == "Sonic.md").unwrap();
        let streets = roms.iter().find(|r| r.rom_filename == "Streets.md").unwrap();
        let columns = roms.iter().find(|r| r.rom_filename == "Columns.md").unwrap();

        // Sonic: baked-in "Shooter" preserved
        assert_eq!(sonic.genre.as_deref(), Some("Shooter"));
        // Streets: empty → filled with "Beat'em Up"
        assert_eq!(streets.genre.as_deref(), Some("Beat'em Up"));
        // Columns: still empty (no enrichment data)
        assert_eq!(columns.genre, None);
    }

    #[test]
    fn entries_per_system_no_rom_cache_returns_all() {
        // When rom_cache is empty, entries_per_system should count all
        // game_metadata entries (fallback behavior).
        let (mut db, _dir) = open_temp_db();
        db.bulk_upsert(&[
            ("sega_smd".into(), "Sonic.md".into(), make_metadata(None)),
            ("sega_smd".into(), "Streets.md".into(), make_metadata(None)),
            ("snes".into(), "Mario.sfc".into(), make_metadata(None)),
        ])
        .unwrap();

        let counts = db.entries_per_system().unwrap();
        assert_eq!(counts.len(), 2);
        // Ordered by count DESC
        assert_eq!(counts[0], ("sega_smd".into(), 2));
        assert_eq!(counts[1], ("snes".into(), 1));
    }

    #[test]
    fn entries_per_system_with_rom_cache_deduplicates_m3u() {
        // When rom_cache has data, entries_per_system should only count
        // game_metadata entries that match rom_cache — disc files excluded
        // by M3U dedup in rom_cache should not be counted.
        let (mut db, _dir) = open_temp_db();

        // game_metadata has 3 entries for sega_cd: the .m3u + 2 disc files
        db.bulk_upsert(&[
            ("sega_cd".into(), "Game.m3u".into(), make_metadata(None)),
            ("sega_cd".into(), "Game (Disc 1).cue".into(), make_metadata(None)),
            ("sega_cd".into(), "Game (Disc 2).cue".into(), make_metadata(None)),
            ("snes".into(), "Mario.sfc".into(), make_metadata(None)),
        ])
        .unwrap();

        // rom_cache only has the .m3u (disc files were deduped out)
        db.save_system_roms(
            "sega_cd",
            &[make_cached_rom("sega_cd", "Game.m3u", true)],
            None,
        )
        .unwrap();
        db.save_system_roms(
            "snes",
            &[make_cached_rom("snes", "Mario.sfc", false)],
            None,
        )
        .unwrap();

        let counts = db.entries_per_system().unwrap();
        let sega_cd = counts.iter().find(|(s, _)| s == "sega_cd").unwrap();
        let snes = counts.iter().find(|(s, _)| s == "snes").unwrap();

        // sega_cd: only 1 (the .m3u), not 3
        assert_eq!(sega_cd.1, 1);
        assert_eq!(snes.1, 1);
    }

    #[test]
    fn entries_per_system_mixed_cached_and_uncached_systems() {
        // One system has rom_cache data (should dedup), another doesn't
        // (should fall back to full count).
        let (mut db, _dir) = open_temp_db();

        db.bulk_upsert(&[
            ("sega_cd".into(), "Game.m3u".into(), make_metadata(None)),
            ("sega_cd".into(), "Game (Disc 1).cue".into(), make_metadata(None)),
            ("snes".into(), "Mario.sfc".into(), make_metadata(None)),
            ("snes".into(), "Zelda.sfc".into(), make_metadata(None)),
        ])
        .unwrap();

        // Only sega_cd has rom_cache data
        db.save_system_roms(
            "sega_cd",
            &[make_cached_rom("sega_cd", "Game.m3u", true)],
            None,
        )
        .unwrap();

        let counts = db.entries_per_system().unwrap();
        let sega_cd = counts.iter().find(|(s, _)| s == "sega_cd").unwrap();
        let snes = counts.iter().find(|(s, _)| s == "snes").unwrap();

        // sega_cd: deduped via rom_cache → 1
        assert_eq!(sega_cd.1, 1);
        // snes: no rom_cache, fallback to raw count → 2
        assert_eq!(snes.1, 2);
    }

    #[test]
    fn images_per_system_no_rom_cache_returns_all() {
        let (mut db, _dir) = open_temp_db();
        db.bulk_upsert(&[
            ("snes".into(), "Mario.sfc".into(), make_metadata(None)),
            ("snes".into(), "Zelda.sfc".into(), make_metadata(None)),
        ])
        .unwrap();
        // bulk_upsert doesn't write image paths; use bulk_update_image_paths.
        db.bulk_update_image_paths(&[
            ("snes".into(), "Mario.sfc".into(), Some("/img/mario.png".into()), None),
        ])
        .unwrap();

        let imgs = db.images_per_system().unwrap();
        assert_eq!(imgs.len(), 1);
        // (system, boxart_count, screenshot_count)
        assert_eq!(imgs[0], ("snes".into(), 1, 0));
    }

    #[test]
    fn images_per_system_with_rom_cache_deduplicates_m3u() {
        let (mut db, _dir) = open_temp_db();

        db.bulk_upsert(&[
            ("sega_cd".into(), "Game.m3u".into(), make_metadata(None)),
            ("sega_cd".into(), "Game (Disc 1).cue".into(), make_metadata(None)),
            ("sega_cd".into(), "Game (Disc 2).cue".into(), make_metadata(None)),
        ])
        .unwrap();
        db.bulk_update_image_paths(&[
            ("sega_cd".into(), "Game.m3u".into(), Some("/img/game.png".into()), None),
            ("sega_cd".into(), "Game (Disc 1).cue".into(), Some("/img/game.png".into()), None),
            ("sega_cd".into(), "Game (Disc 2).cue".into(), Some("/img/game.png".into()), None),
        ])
        .unwrap();

        // rom_cache only has the .m3u
        db.save_system_roms(
            "sega_cd",
            &[make_cached_rom("sega_cd", "Game.m3u", true)],
            None,
        )
        .unwrap();

        let imgs = db.images_per_system().unwrap();
        let sega_cd = imgs.iter().find(|(s, _, _)| s == "sega_cd").unwrap();

        // Only 1 boxart counted (the .m3u match), not 3
        assert_eq!(sega_cd.1, 1);
    }

    #[test]
    fn specials_returns_special_roms_sharing_base_title() {
        let (db, _dir) = open_temp_db();

        let mut original = make_cached_rom("snes", "Game (USA).sfc", false);
        original.base_title = "Game".into();
        original.region = "usa".into();

        let mut fastrom = make_cached_rom("snes", "Game (USA) (FastRom).sfc", false);
        fastrom.base_title = "Game".into();
        fastrom.region = "usa".into();
        fastrom.is_special = true;

        let mut hz60 = make_cached_rom("snes", "Game (Europe) (60hz).sfc", false);
        hz60.base_title = "Game".into();
        hz60.region = "europe".into();
        hz60.is_special = true;

        db.save_system_roms("snes", &[original, fastrom, hz60], None)
            .unwrap();

        let specials = db.specials("snes", "Game (USA).sfc").unwrap();
        assert_eq!(specials.len(), 2);
        let filenames: Vec<&str> = specials.iter().map(|(f, _)| f.as_str()).collect();
        assert!(filenames.contains(&"Game (USA) (FastRom).sfc"));
        assert!(filenames.contains(&"Game (Europe) (60hz).sfc"));
    }

    #[test]
    fn recommendation_queries_exclude_special_roms() {
        let (db, _dir) = open_temp_db();

        let mut normal = make_cached_rom("snes", "Mario (USA).sfc", false);
        normal.base_title = "Mario".into();
        normal.region = "usa".into();
        normal.box_art_url = Some("/img/mario.png".into());
        normal.rating = Some(4.5);
        normal.genre = Some("Platform".into());

        let mut special = make_cached_rom("snes", "Mario (USA) (FastRom).sfc", false);
        special.base_title = "Mario FastRom".into();
        special.region = "usa".into();
        special.box_art_url = Some("/img/mario.png".into());
        special.rating = Some(4.5);
        special.genre = Some("Platform".into());
        special.is_special = true;

        db.save_system_roms("snes", &[normal, special], None)
            .unwrap();

        // random_cached_roms should exclude is_special
        let random = db.random_cached_roms("snes", 10).unwrap();
        assert_eq!(random.len(), 1);
        assert_eq!(random[0].rom_filename, "Mario (USA).sfc");

        // similar_by_genre should exclude is_special
        let similar = db
            .similar_by_genre("snes", "Platform", "Other.sfc", 10)
            .unwrap();
        assert_eq!(similar.len(), 1);
        assert_eq!(similar[0].rom_filename, "Mario (USA).sfc");
    }
}
