//! Local SQLite cache for external game metadata (descriptions, ratings, etc.).
//!
//! Stored at `<rom_storage>/.replay-control/metadata.db`.

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OpenFlags, OptionalExtension};

use crate::error::{Error, Result};

/// Directory name for Replay Control data on ROM storage.
pub const RC_DIR: &str = ".replay-control";
const DB_FILE: &str = "metadata.db";

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
}

/// Cached metadata for a single game.
#[derive(Debug, Clone)]
pub struct GameMetadata {
    pub description: Option<String>,
    pub rating: Option<f64>,
    pub publisher: Option<String>,
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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MetadataStats {
    pub total_entries: usize,
    pub with_description: usize,
    pub with_rating: usize,
    pub db_size_bytes: u64,
}

/// Handle to the metadata SQLite database.
pub struct MetadataDb {
    conn: Connection,
    db_path: PathBuf,
}

impl MetadataDb {
    /// Open (or create) the metadata database at `<storage_root>/.replay-control/metadata.db`.
    pub fn open(storage_root: &Path) -> Result<Self> {
        let dir = storage_root.join(RC_DIR);
        std::fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
        let db_path = dir.join(DB_FILE);

        // Try normal open first, then fall back to nolock mode for NFS.
        let conn = match Self::try_open(&db_path) {
            Ok(conn) => conn,
            Err(_) => {
                tracing::info!(
                    "Standard SQLite open failed (NFS?), retrying with nolock VFS"
                );
                Self::try_open_nolock(&db_path)?
            }
        };

        let db = Self { conn, db_path };
        db.init()?;
        Ok(db)
    }

    /// Try to open SQLite with normal locking + WAL.
    fn try_open(db_path: &Path) -> Result<Connection> {
        let conn = Connection::open(db_path)
            .map_err(|e| Error::Other(format!("Failed to open metadata DB: {e}")))?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA cache_size = -8000;",
        )
        .map_err(|e| Error::Other(format!("Failed to set pragmas: {e}")))?;
        Ok(conn)
    }

    /// Open SQLite with file locking disabled (for NFS/network filesystems).
    /// Safe because we hold the connection behind a Mutex (single-writer).
    fn try_open_nolock(db_path: &Path) -> Result<Connection> {
        // Use file: URI with nolock=1 to bypass filesystem locking.
        let uri = format!("file:{}?nolock=1", db_path.display());
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_URI;
        let conn = Connection::open_with_flags(uri, flags)
            .map_err(|e| Error::Other(format!("Failed to open metadata DB (nolock): {e}")))?;
        conn.execute_batch(
            "PRAGMA journal_mode = DELETE;
             PRAGMA synchronous = NORMAL;
             PRAGMA cache_size = -8000;",
        )
        .map_err(|e| Error::Other(format!("Failed to set pragmas: {e}")))?;
        Ok(conn)
    }

    /// Create tables if they don't exist, and run migrations.
    fn init(&self) -> Result<()> {
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS game_metadata (
                    system TEXT NOT NULL,
                    rom_filename TEXT NOT NULL,
                    description TEXT,
                    rating REAL,
                    publisher TEXT,
                    source TEXT NOT NULL,
                    fetched_at INTEGER NOT NULL,
                    PRIMARY KEY (system, rom_filename)
                );",
            )
            .map_err(|e| Error::Other(format!("Failed to create metadata table: {e}")))?;

        // Migration: add image path columns (idempotent — ignore "duplicate column" errors).
        let _ = self
            .conn
            .execute_batch("ALTER TABLE game_metadata ADD COLUMN box_art_path TEXT;");
        let _ = self
            .conn
            .execute_batch("ALTER TABLE game_metadata ADD COLUMN screenshot_path TEXT;");

        Ok(())
    }

    /// Look up cached metadata for a specific game.
    pub fn lookup(&self, system: &str, rom_filename: &str) -> Result<Option<GameMetadata>> {
        let result = self
            .conn
            .query_row(
                "SELECT description, rating, publisher, source, fetched_at, box_art_path, screenshot_path
                 FROM game_metadata WHERE system = ?1 AND rom_filename = ?2",
                params![system, rom_filename],
                |row| {
                    Ok(GameMetadata {
                        description: row.get(0)?,
                        rating: row.get(1)?,
                        publisher: row.get(2)?,
                        source: row.get(3)?,
                        fetched_at: row.get(4)?,
                        box_art_path: row.get(5)?,
                        screenshot_path: row.get(6)?,
                    })
                },
            )
            .optional()
            .map_err(|e| Error::Other(format!("Metadata lookup failed: {e}")))?;
        Ok(result)
    }

    /// Insert or update metadata for a game.
    pub fn upsert(
        &self,
        system: &str,
        rom_filename: &str,
        meta: &GameMetadata,
    ) -> Result<()> {
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
    pub fn bulk_upsert(
        &mut self,
        entries: &[(String, String, GameMetadata)],
    ) -> Result<usize> {
        let tx = self
            .conn
            .transaction()
            .map_err(|e| Error::Other(format!("Transaction start failed: {e}")))?;

        let mut count = 0usize;
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO game_metadata (system, rom_filename, description, rating, publisher, source, fetched_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                     ON CONFLICT(system, rom_filename) DO UPDATE SET
                        description = excluded.description,
                        rating = excluded.rating,
                        publisher = excluded.publisher,
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

        Ok(MetadataStats {
            total_entries,
            with_description,
            with_rating,
            db_size_bytes,
        })
    }

    /// Get all ratings as a map of `(system, rom_filename) -> rating`.
    pub fn all_ratings(&self) -> Result<std::collections::HashMap<(String, String), f64>> {
        let mut stmt = self
            .conn
            .prepare("SELECT system, rom_filename, rating FROM game_metadata WHERE rating IS NOT NULL")
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
    pub fn entries_per_system(&self) -> Result<Vec<(String, usize)>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT system, COUNT(*) as cnt FROM game_metadata GROUP BY system ORDER BY cnt DESC",
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
    pub fn images_per_system(&self) -> Result<Vec<(String, usize, usize)>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT system,
                        SUM(CASE WHEN box_art_path IS NOT NULL THEN 1 ELSE 0 END),
                        SUM(CASE WHEN screenshot_path IS NOT NULL THEN 1 ELSE 0 END)
                 FROM game_metadata GROUP BY system",
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
}
