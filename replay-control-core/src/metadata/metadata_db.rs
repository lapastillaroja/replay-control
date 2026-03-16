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
    pub developer: Option<String>,
    pub genre: Option<String>,
    pub players: Option<u8>,
    pub release_year: Option<u16>,
    pub cooperative: bool,
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

/// A cached ROM entry from the `game_library` table.
#[derive(Debug, Clone)]
pub struct GameEntry {
    pub system: String,
    pub rom_filename: String,
    pub rom_path: String,
    pub display_name: Option<String>,
    pub size_bytes: u64,
    pub is_m3u: bool,
    pub box_art_url: Option<String>,
    pub driver_status: Option<String>,
    /// Detail/original genre (e.g., "Maze / Shooter", "Shoot'em Up").
    pub genre: Option<String>,
    /// Normalized genre group (e.g., "Shooter", "Maze"). Used for filtering/grouping.
    pub genre_group: String,
    pub players: Option<u8>,
    pub rating: Option<f32>,
    pub is_clone: bool,
    pub base_title: String,
    pub region: String,
    pub is_translation: bool,
    pub is_hack: bool,
    pub is_special: bool,
    /// CRC32 hash of the ROM file. NULL for CD/computer/arcade systems.
    pub crc32: Option<u32>,
    /// File mtime (seconds since UNIX epoch) when the CRC32 was computed.
    /// Used as a cache key: if the file's mtime changes, the hash is stale.
    pub hash_mtime: Option<i64>,
    /// No-Intro canonical name if CRC32 matched the DAT data.
    /// NULL means either not hashed, or hashed but no match.
    pub hash_matched_name: Option<String>,
    /// Algorithmic series key for franchise grouping.
    /// Computed by stripping trailing numbers/roman numerals from `base_title`.
    /// Empty string means no series could be extracted.
    pub series_key: String,
}

/// Per-system metadata from the `game_library_meta` table.
#[derive(Debug, Clone)]
pub struct SystemMeta {
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
        "game_library",
        "data_sources",
        "thumbnail_index",
        "game_alias",
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
                    developer TEXT,
                    genre TEXT,
                    players INTEGER,
                    release_year INTEGER,
                    cooperative INTEGER NOT NULL DEFAULT 0,
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

                CREATE TABLE IF NOT EXISTS game_library (
                    system TEXT NOT NULL,
                    rom_filename TEXT NOT NULL,
                    rom_path TEXT NOT NULL,
                    display_name TEXT,
                    size_bytes INTEGER NOT NULL DEFAULT 0,
                    is_m3u INTEGER NOT NULL DEFAULT 0,
                    box_art_url TEXT,
                    driver_status TEXT,
                    genre TEXT,
                    genre_group TEXT NOT NULL DEFAULT '',
                    players INTEGER,
                    rating REAL,
                    is_clone INTEGER NOT NULL DEFAULT 0,
                    base_title TEXT NOT NULL DEFAULT '',
                    region TEXT NOT NULL DEFAULT '',
                    is_translation INTEGER NOT NULL DEFAULT 0,
                    is_hack INTEGER NOT NULL DEFAULT 0,
                    is_special INTEGER NOT NULL DEFAULT 0,
                    crc32 INTEGER,
                    hash_mtime INTEGER,
                    hash_matched_name TEXT,
                    series_key TEXT NOT NULL DEFAULT '',
                    PRIMARY KEY (system, rom_filename)
                );

                CREATE TABLE IF NOT EXISTS game_library_meta (
                    system TEXT PRIMARY KEY,
                    dir_mtime_secs INTEGER,
                    scanned_at INTEGER NOT NULL,
                    rom_count INTEGER NOT NULL DEFAULT 0,
                    total_size_bytes INTEGER NOT NULL DEFAULT 0
                );

                CREATE INDEX IF NOT EXISTS idx_game_library_genre
                  ON game_library (system, genre)
                  WHERE genre IS NOT NULL AND genre != '';

                CREATE INDEX IF NOT EXISTS idx_game_library_genre_group
                  ON game_library (system, genre_group)
                  WHERE genre_group != '';

                CREATE INDEX IF NOT EXISTS idx_game_library_series_key
                  ON game_library (series_key)
                  WHERE series_key != '';

                CREATE TABLE IF NOT EXISTS game_alias (
                    system TEXT NOT NULL,
                    base_title TEXT NOT NULL,
                    alias_name TEXT NOT NULL,
                    alias_region TEXT NOT NULL DEFAULT '',
                    source TEXT NOT NULL,
                    PRIMARY KEY (system, base_title, alias_name)
                );
                CREATE INDEX IF NOT EXISTS idx_game_alias_name
                    ON game_alias(alias_name COLLATE NOCASE);

",
            )
            .map_err(|e| Error::Other(format!("Failed to create tables: {e}")))?;

        Ok(())
    }

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

    /// Count metadata entries per system, ordered by count descending.
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
                // LEFT JOIN + NOT EXISTS fallback: use game_library for M3U dedup
                // when available, fall back to raw game_metadata count when
                // game_library is empty for a system.
                "SELECT gm.system, COUNT(*) as cnt
                 FROM game_metadata gm
                 LEFT JOIN game_library gl ON gm.system = gl.system AND gm.rom_filename = gl.rom_filename
                 WHERE gl.rom_filename IS NOT NULL
                    OR NOT EXISTS (SELECT 1 FROM game_library gl2 WHERE gl2.system = gm.system)
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
    ///
    /// Only counts `game_metadata` rows that have a matching entry in `game_library`,
    /// so orphaned metadata rows (for ROMs that have been deleted) are excluded.
    /// Falls back to counting all `game_metadata` rows when `game_library` is empty
    /// (e.g., library not yet warmed).
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

    /// Get path to the database file.
    pub fn db_path(&self) -> &Path {
        &self.db_path
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
    /// `region_secondary` is the fallback region (empty string = no secondary).
    pub fn random_cached_roms_diverse(
        &self,
        count: usize,
        region_pref: &str,
        region_secondary: &str,
    ) -> Result<Vec<GameEntry>> {
        let mut stmt = self
            .conn
            .prepare(
                "WITH deduped AS (
                    SELECT *, ROW_NUMBER() OVER (
                        PARTITION BY system, base_title
                        ORDER BY CASE
                            WHEN region = ?2 THEN 0
                            WHEN region = ?3 THEN 1
                            WHEN region = 'world' THEN 2
                            ELSE 3
                        END
                    ) AS rn
                    FROM game_library
                    WHERE is_clone = 0 AND is_translation = 0 AND is_hack = 0 AND is_special = 0
                )
                SELECT system, rom_filename, rom_path, display_name, size_bytes,
                        is_m3u, box_art_url, driver_status, genre, genre_group, players, rating,
                        is_clone, base_title, region, is_translation, is_hack, is_special,
                        crc32, hash_mtime, hash_matched_name
                FROM deduped WHERE rn = 1
                ORDER BY RANDOM() LIMIT ?1",
            )
            .map_err(|e| Error::Other(format!("Prepare random_cached_roms_diverse: {e}")))?;

        let rows = stmt
            .query_map(
                params![(count * 5) as i64, region_pref, region_secondary],
                Self::row_to_game_entry,
            )
            .map_err(|e| Error::Other(format!("Query random_cached_roms_diverse: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Get random cached ROMs with box art from a specific system.
    pub fn random_cached_roms(&self, system: &str, count: usize) -> Result<Vec<GameEntry>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT system, rom_filename, rom_path, display_name, size_bytes,
                        is_m3u, box_art_url, driver_status, genre, genre_group, players, rating,
                        is_clone, base_title, region, is_translation, is_hack, is_special,
                        crc32, hash_mtime, hash_matched_name, series_key
                 FROM game_library
                 WHERE system = ?1 AND box_art_url IS NOT NULL AND is_special = 0
                 ORDER BY RANDOM() LIMIT ?2",
            )
            .map_err(|e| Error::Other(format!("Prepare random_cached_roms: {e}")))?;

        let rows = stmt
            .query_map(params![system, count as i64], Self::row_to_game_entry)
            .map_err(|e| Error::Other(format!("Query random_cached_roms: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Get top-rated cached ROMs across all systems.
    /// Filters out arcade clones and deduplicates regional variants,
    /// then selects from the top-rated pool and randomizes within it
    /// so each page load shows a different selection of highly-rated games.
    /// `region_secondary` is the fallback region (empty string = no secondary).
    pub fn top_rated_cached_roms(
        &self,
        count: usize,
        region_pref: &str,
        region_secondary: &str,
    ) -> Result<Vec<GameEntry>> {
        let pool_size = (count * 4).max(40) as i64;
        let mut stmt = self
            .conn
            .prepare(
                "WITH deduped AS (
                    SELECT *, ROW_NUMBER() OVER (
                        PARTITION BY system, base_title
                        ORDER BY CASE
                            WHEN region = ?2 THEN 0
                            WHEN region = ?3 THEN 1
                            WHEN region = 'world' THEN 2
                            ELSE 3
                        END
                    ) AS rn
                    FROM game_library
                    WHERE is_clone = 0 AND is_translation = 0 AND is_hack = 0 AND is_special = 0 AND rating IS NOT NULL AND rating > 0
                )
                SELECT system, rom_filename, rom_path, display_name, size_bytes,
                        is_m3u, box_art_url, driver_status, genre, genre_group, players, rating,
                        is_clone, base_title, region, is_translation, is_hack, is_special,
                        crc32, hash_mtime, hash_matched_name
                FROM (
                    SELECT * FROM deduped WHERE rn = 1
                    ORDER BY rating DESC
                    LIMIT ?1
                )
                ORDER BY RANDOM()",
            )
            .map_err(|e| Error::Other(format!("Prepare top_rated_cached_roms: {e}")))?;

        let rows = stmt
            .query_map(
                params![pool_size, region_pref, region_secondary],
                Self::row_to_game_entry,
            )
            .map_err(|e| Error::Other(format!("Query top_rated_cached_roms: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Get genre counts across the entire library.
    pub fn genre_counts(&self) -> Result<Vec<(String, usize)>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT genre_group, COUNT(*) as cnt FROM game_library
                 WHERE genre_group != ''
                 GROUP BY genre_group ORDER BY cnt DESC",
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
                "SELECT COUNT(*) FROM game_library WHERE players IS NOT NULL AND players >= 2",
                [],
                |row| row.get(0),
            )
            .map_err(|e| Error::Other(format!("Query multiplayer_count: {e}")))
    }

    /// Get all distinct genre groups across the entire game library.
    /// Returns sorted genre group names (excludes empty strings).
    pub fn all_genre_groups(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT DISTINCT genre_group FROM game_library
                 WHERE genre_group != ''
                 ORDER BY genre_group",
            )
            .map_err(|e| Error::Other(format!("Prepare all_genre_groups: {e}")))?;

        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| Error::Other(format!("Query all_genre_groups: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Get distinct genre groups for a specific system.
    /// Returns sorted genre group names (excludes empty strings).
    pub fn system_genre_groups(&self, system: &str) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT DISTINCT genre_group FROM game_library
                 WHERE system = ?1 AND genre_group != ''
                 ORDER BY genre_group",
            )
            .map_err(|e| Error::Other(format!("Prepare system_genre_groups: {e}")))?;

        let rows = stmt
            .query_map(params![system], |row| row.get::<_, String>(0))
            .map_err(|e| Error::Other(format!("Query system_genre_groups: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Get non-favorited ROMs from a system, optionally filtered by genre.
    /// Filters out arcade clones and deduplicates regional variants.
    /// Selects from top-rated games and randomizes via SQL so each load
    /// shows different recommendations. Used for "Because You Love" section.
    /// `region_secondary` is the fallback region (empty string = no secondary).
    pub fn system_roms_excluding(
        &self,
        system: &str,
        exclude_filenames: &[&str],
        genre_filter: Option<&str>,
        count: usize,
        region_pref: &str,
        region_secondary: &str,
    ) -> Result<Vec<GameEntry>> {
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
                                WHEN region = ?5 THEN 1
                                WHEN region = 'world' THEN 2
                                ELSE 3
                            END
                        ) AS rn
                        FROM game_library
                        WHERE system = ?1 AND genre_group = ?2 AND is_clone = 0 AND is_translation = 0 AND is_hack = 0 AND is_special = 0
                    )
                    SELECT system, rom_filename, rom_path, display_name, size_bytes,
                            is_m3u, box_art_url, driver_status, genre, genre_group, players, rating,
                            is_clone, base_title, region, is_translation, is_hack, is_special,
                            crc32, hash_mtime, hash_matched_name
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
                    params![system, genre, limit, region_pref, region_secondary],
                    Self::row_to_game_entry,
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
                                WHEN region = ?4 THEN 1
                                WHEN region = 'world' THEN 2
                                ELSE 3
                            END
                        ) AS rn
                        FROM game_library
                        WHERE system = ?1 AND is_clone = 0 AND is_translation = 0 AND is_hack = 0 AND is_special = 0
                    )
                    SELECT system, rom_filename, rom_path, display_name, size_bytes,
                            is_m3u, box_art_url, driver_status, genre, genre_group, players, rating,
                            is_clone, base_title, region, is_translation, is_hack, is_special,
                            crc32, hash_mtime, hash_matched_name
                    FROM (
                        SELECT * FROM deduped WHERE rn = 1
                        ORDER BY rating DESC NULLS LAST
                         LIMIT ?2
                     )
                     ORDER BY RANDOM()",
                )
                .map_err(|e| Error::Other(format!("Prepare system_roms_excluding: {e}")))?;

            let rows = stmt
                .query_map(
                    params![system, limit, region_pref, region_secondary],
                    Self::row_to_game_entry,
                )
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
                "SELECT rom_filename, region, display_name FROM game_library
                 WHERE system = ?1
                   AND base_title != ''
                   AND is_translation = 0
                   AND is_hack = 0
                   AND is_special = 0
                   AND is_clone = 0
                   AND base_title = (
                       SELECT base_title FROM game_library
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
                "SELECT rom_filename, display_name FROM game_library
                 WHERE system = ?1
                   AND base_title != ''
                   AND is_translation = 1
                   AND base_title = (
                       SELECT base_title FROM game_library
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
                "SELECT rom_filename, display_name FROM game_library
                 WHERE system = ?1
                   AND base_title != ''
                   AND is_hack = 1
                   AND base_title = (
                       SELECT base_title FROM game_library
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
                "SELECT rom_filename, display_name FROM game_library
                 WHERE system = ?1
                   AND base_title != ''
                   AND is_special = 1
                   AND base_title = (
                       SELECT base_title FROM game_library
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
    ///
    /// Uses a two-tier weighted query:
    /// - Exact `genre` match gets relevance=2 (same detailed genre)
    /// - `genre_group` match gets relevance=1 (same genre family)
    ///
    /// Results are ordered by relevance (exact first), then randomized
    /// within each tier. Excludes the given ROM, clones, translations,
    /// hacks, specials, and games without a genre.
    pub fn similar_by_genre(
        &self,
        system: &str,
        genre: &str,
        exclude_filename: &str,
        limit: usize,
    ) -> Result<Vec<GameEntry>> {
        // Compute the genre_group for the input genre to find family matches.
        let genre_group = crate::genre::normalize_genre(genre);

        let mut stmt = self
            .conn
            .prepare(
                "SELECT system, rom_filename, rom_path, display_name, size_bytes,
                        is_m3u, box_art_url, driver_status, genre, genre_group, players, rating,
                        is_clone, base_title, region, is_translation, is_hack, is_special,
                        crc32, hash_mtime, hash_matched_name, series_key
                 FROM game_library
                 WHERE system = ?1
                   AND (genre = ?2 OR genre_group = ?3)
                   AND genre_group != ''
                   AND rom_filename != ?4
                   AND is_clone = 0
                   AND is_translation = 0
                   AND is_hack = 0
                   AND is_special = 0
                 ORDER BY
                   CASE WHEN genre = ?2 THEN 0 ELSE 1 END,
                   RANDOM()
                 LIMIT ?5",
            )
            .map_err(|e| Error::Other(format!("Prepare similar_by_genre: {e}")))?;

        let rows = stmt
            .query_map(
                params![system, genre, genre_group, exclude_filename, limit as i64],
                Self::row_to_game_entry,
            )
            .map_err(|e| Error::Other(format!("Query similar_by_genre: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Find series siblings: games with the same `series_key` but different `base_title`,
    /// across ALL systems (cross-system series).
    ///
    /// Deduplicates by `(system, base_title)` to pick one ROM per game per system,
    /// preferring the given region. Returns at most `limit` results.
    pub fn series_siblings(
        &self,
        series_key: &str,
        current_base_title: &str,
        region_pref: &str,
        limit: usize,
    ) -> Result<Vec<GameEntry>> {
        if series_key.is_empty() {
            return Ok(Vec::new());
        }

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
                    FROM game_library
                    WHERE series_key = ?1
                      AND series_key != ''
                      AND base_title != ?3
                      AND is_clone = 0
                      AND is_translation = 0
                      AND is_hack = 0
                      AND is_special = 0
                )
                SELECT system, rom_filename, rom_path, display_name, size_bytes,
                        is_m3u, box_art_url, driver_status, genre, genre_group, players, rating,
                        is_clone, base_title, region, is_translation, is_hack, is_special,
                        crc32, hash_mtime, hash_matched_name, series_key
                FROM deduped WHERE rn = 1
                ORDER BY display_name
                LIMIT ?4",
            )
            .map_err(|e| Error::Other(format!("Prepare series_siblings: {e}")))?;

        let rows = stmt
            .query_map(
                params![series_key, region_pref, current_base_title, limit as i64],
                Self::row_to_game_entry,
            )
            .map_err(|e| Error::Other(format!("Query series_siblings: {e}")))?;

        Ok(rows.flatten().collect())
    }

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

    /// Find cross-name variants of a game using game_alias.
    ///
    /// Given a ROM's system and base_title, find other games in the same system
    /// that are linked via aliases (same game, different name).
    /// Returns ROMs that are either:
    /// - directly aliased to the current game's base_title, or
    /// - share an alias group (their base_title appears as an alias for the same canonical game)
    pub fn alias_variants(
        &self,
        system: &str,
        base_title: &str,
        current_filename: &str,
        region_pref: &str,
    ) -> Result<Vec<GameEntry>> {
        // Strategy: find all base_titles that share an alias group with the current game.
        // The current game's base_title might appear as an alias_name for some canonical base_title,
        // or some other base_title might appear as an alias_name for the current game's base_title.
        let mut stmt = self
            .conn
            .prepare(
                "WITH related_titles AS (
                    -- Games whose base_title is an alias of the current game
                    SELECT DISTINCT gl.base_title AS bt
                    FROM game_alias ga
                    JOIN game_library gl ON gl.system = ga.system AND gl.base_title = ga.alias_name
                    WHERE ga.system = ?1 AND ga.base_title = ?2
                    UNION
                    -- The canonical base_title that the current game is an alias of
                    SELECT DISTINCT ga.base_title AS bt
                    FROM game_alias ga
                    WHERE ga.system = ?1 AND ga.alias_name = ?2
                    UNION
                    -- Games that are aliases of the same canonical title
                    SELECT DISTINCT ga2.alias_name AS bt
                    FROM game_alias ga
                    JOIN game_alias ga2 ON ga2.system = ga.system AND ga2.base_title = ga.base_title
                    WHERE ga.system = ?1 AND ga.alias_name = ?2
                ),
                deduped AS (
                    SELECT gl.*, ROW_NUMBER() OVER (
                        PARTITION BY gl.system, gl.base_title
                        ORDER BY CASE
                            WHEN gl.region = ?4 THEN 0
                            WHEN gl.region = 'world' THEN 1
                            ELSE 2
                        END
                    ) AS rn
                    FROM game_library gl
                    WHERE gl.system = ?1
                      AND gl.base_title IN (SELECT bt FROM related_titles)
                      AND gl.base_title != ?2
                      AND gl.rom_filename != ?3
                      AND gl.is_clone = 0
                      AND gl.is_translation = 0
                      AND gl.is_hack = 0
                      AND gl.is_special = 0
                )
                SELECT system, rom_filename, rom_path, display_name, size_bytes,
                        is_m3u, box_art_url, driver_status, genre, genre_group, players, rating,
                        is_clone, base_title, region, is_translation, is_hack, is_special,
                        crc32, hash_mtime, hash_matched_name, series_key
                FROM deduped WHERE rn = 1
                ORDER BY display_name",
            )
            .map_err(|e| Error::Other(format!("Prepare alias_variants: {e}")))?;

        let rows = stmt
            .query_map(
                params![system, base_title, current_filename, region_pref],
                Self::row_to_game_entry,
            )
            .map_err(|e| Error::Other(format!("Query alias_variants: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Search game aliases: find base_titles whose aliases match the query.
    ///
    /// Returns `(system, base_title)` pairs where an alias matches the query.
    /// Used by search to expand results (e.g., searching "Bare Knuckle" finds "Streets of Rage").
    pub fn search_aliases(
        &self,
        query: &str,
    ) -> Result<Vec<(String, String)>> {
        let like_pattern = format!("%{query}%");
        let mut stmt = self
            .conn
            .prepare(
                "SELECT DISTINCT system, base_title FROM game_alias
                 WHERE alias_name LIKE ?1 COLLATE NOCASE",
            )
            .map_err(|e| Error::Other(format!("Prepare search_aliases: {e}")))?;

        let rows = stmt
            .query_map(params![like_pattern], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| Error::Other(format!("Query search_aliases: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Clear all game aliases.
    pub fn clear_aliases(&self) -> Result<()> {
        self.conn
            .execute("DELETE FROM game_alias", [])
            .map_err(|e| Error::Other(format!("Clear game_alias failed: {e}")))?;
        Ok(())
    }

    /// Helper: convert a row to GameEntry (used by multiple queries).
    fn row_to_game_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<GameEntry> {
        Ok(GameEntry {
            system: row.get(0)?,
            rom_filename: row.get(1)?,
            rom_path: row.get(2)?,
            display_name: row.get(3)?,
            size_bytes: row.get::<_, i64>(4)? as u64,
            is_m3u: row.get(5)?,
            box_art_url: row.get(6)?,
            driver_status: row.get(7)?,
            genre: row.get(8)?,
            genre_group: row.get::<_, String>(9)?,
            players: row.get::<_, Option<i32>>(10)?.map(|p| p as u8),
            rating: row.get(11)?,
            is_clone: row.get(12)?,
            base_title: row.get(13)?,
            region: row.get(14)?,
            is_translation: row.get(15)?,
            is_hack: row.get(16)?,
            is_special: row.get(17)?,
            crc32: row.get::<_, Option<i64>>(18)?.map(|c| c as u32),
            hash_mtime: row.get(19)?,
            hash_matched_name: row.get(20)?,
            series_key: row.get::<_, String>(21).unwrap_or_default(),
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
            developer: None,
            genre: None,
            players: None,
            release_year: None,
            cooperative: false,
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

    fn make_game_entry(system: &str, filename: &str, is_m3u: bool) -> GameEntry {
        GameEntry {
            system: system.into(),
            rom_filename: filename.into(),
            rom_path: format!("/roms/{system}/{filename}"),
            display_name: None,
            size_bytes: 1000,
            is_m3u,
            box_art_url: None,
            driver_status: None,
            genre: None,
            genre_group: String::new(),
            players: None,
            rating: None,
            is_clone: false,
            base_title: String::new(),
            region: String::new(),
            is_translation: false,
            is_hack: false,
            is_special: false,
            crc32: None,
            hash_mtime: None,
            hash_matched_name: None,
        }
    }

    fn make_game_entry_with_genre(system: &str, filename: &str, genre: &str) -> GameEntry {
        GameEntry {
            genre: Some(genre.into()),
            genre_group: crate::genre::normalize_genre(genre).to_string(),
            ..make_game_entry(system, filename, false)
        }
    }

    #[test]
    fn genre_enrichment_fills_empty_genre_from_launchbox() {
        // When game_library has no genre but game_metadata does,
        // update_box_art_genre_rating should fill it.
        let (mut db, _dir) = open_temp_db();

        // game_metadata has genre "Platform" for Sonic
        db.bulk_upsert(&[
            ("sega_smd".into(), "Sonic.md".into(), make_metadata_with_genre("Platform")),
        ]).unwrap();

        // game_library has Sonic with no genre
        db.save_system_entries("sega_smd", &[
            make_game_entry("sega_smd", "Sonic.md", false),
        ], None).unwrap();

        // Enrich with genre from LaunchBox
        db.update_box_art_genre_rating("sega_smd", &[
            ("Sonic.md".into(), None, Some("Platform".into()), None, None),
        ]).unwrap();

        let roms = db.load_system_entries("sega_smd").unwrap();
        assert_eq!(roms[0].genre.as_deref(), Some("Platform"));
    }

    #[test]
    fn genre_enrichment_does_not_overwrite_existing_genre() {
        // When game_library already has a baked-in genre, the SQL guard
        // (genre IS NULL OR genre = '') should prevent overwriting it.
        let (mut db, _dir) = open_temp_db();

        // game_library has Sonic with baked-in "Shooter" (wrong but exists)
        db.save_system_entries("sega_smd", &[
            make_game_entry_with_genre("sega_smd", "Sonic.md", "Shooter"),
        ], None).unwrap();

        // Try to enrich with "Platform" from LaunchBox
        db.update_box_art_genre_rating("sega_smd", &[
            ("Sonic.md".into(), None, Some("Platform".into()), None, None),
        ]).unwrap();

        let roms = db.load_system_entries("sega_smd").unwrap();
        // Should still be "Shooter" — not overwritten
        assert_eq!(roms[0].genre.as_deref(), Some("Shooter"));
    }

    #[test]
    fn genre_enrichment_mixed_empty_and_existing() {
        // Multiple ROMs: some with genre, some without. Only empty ones get filled.
        let (mut db, _dir) = open_temp_db();

        db.save_system_entries("sega_smd", &[
            make_game_entry_with_genre("sega_smd", "Sonic.md", "Shooter"),
            make_game_entry("sega_smd", "Streets.md", false),
            make_game_entry("sega_smd", "Columns.md", false),
        ], None).unwrap();

        db.update_box_art_genre_rating("sega_smd", &[
            ("Sonic.md".into(), None, Some("Platform".into()), None, None),
            ("Streets.md".into(), None, Some("Beat'em Up".into()), None, None),
            // Columns has no LaunchBox genre either — no enrichment tuple
        ]).unwrap();

        let roms = db.load_system_entries("sega_smd").unwrap();
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
    fn entries_per_system_no_game_library_returns_all() {
        // When game_library is empty, entries_per_system should count all
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
    fn entries_per_system_with_game_library_deduplicates_m3u() {
        // When game_library has data, entries_per_system should only count
        // game_metadata entries that match game_library — disc files excluded
        // by M3U dedup in game_library should not be counted.
        let (mut db, _dir) = open_temp_db();

        // game_metadata has 3 entries for sega_cd: the .m3u + 2 disc files
        db.bulk_upsert(&[
            ("sega_cd".into(), "Game.m3u".into(), make_metadata(None)),
            ("sega_cd".into(), "Game (Disc 1).cue".into(), make_metadata(None)),
            ("sega_cd".into(), "Game (Disc 2).cue".into(), make_metadata(None)),
            ("snes".into(), "Mario.sfc".into(), make_metadata(None)),
        ])
        .unwrap();

        // game_library only has the .m3u (disc files were deduped out)
        db.save_system_entries(
            "sega_cd",
            &[make_game_entry("sega_cd", "Game.m3u", true)],
            None,
        )
        .unwrap();
        db.save_system_entries(
            "snes",
            &[make_game_entry("snes", "Mario.sfc", false)],
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
        // One system has game_library data (should dedup), another doesn't
        // (should fall back to full count).
        let (mut db, _dir) = open_temp_db();

        db.bulk_upsert(&[
            ("sega_cd".into(), "Game.m3u".into(), make_metadata(None)),
            ("sega_cd".into(), "Game (Disc 1).cue".into(), make_metadata(None)),
            ("snes".into(), "Mario.sfc".into(), make_metadata(None)),
            ("snes".into(), "Zelda.sfc".into(), make_metadata(None)),
        ])
        .unwrap();

        // Only sega_cd has game_library data
        db.save_system_entries(
            "sega_cd",
            &[make_game_entry("sega_cd", "Game.m3u", true)],
            None,
        )
        .unwrap();

        let counts = db.entries_per_system().unwrap();
        let sega_cd = counts.iter().find(|(s, _)| s == "sega_cd").unwrap();
        let snes = counts.iter().find(|(s, _)| s == "snes").unwrap();

        // sega_cd: deduped via game_library → 1
        assert_eq!(sega_cd.1, 1);
        // snes: no game_library, fallback to raw count → 2
        assert_eq!(snes.1, 2);
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
        // (system, count_with_box_art)
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

    #[test]
    fn specials_returns_special_roms_sharing_base_title() {
        let (mut db, _dir) = open_temp_db();

        let mut original = make_game_entry("snes", "Game (USA).sfc", false);
        original.base_title = "Game".into();
        original.region = "usa".into();

        let mut fastrom = make_game_entry("snes", "Game (USA) (FastRom).sfc", false);
        fastrom.base_title = "Game".into();
        fastrom.region = "usa".into();
        fastrom.is_special = true;

        let mut hz60 = make_game_entry("snes", "Game (Europe) (60hz).sfc", false);
        hz60.base_title = "Game".into();
        hz60.region = "europe".into();
        hz60.is_special = true;

        db.save_system_entries("snes", &[original, fastrom, hz60], None)
            .unwrap();

        let specials = db.specials("snes", "Game (USA).sfc").unwrap();
        assert_eq!(specials.len(), 2);
        let filenames: Vec<&str> = specials.iter().map(|(f, _)| f.as_str()).collect();
        assert!(filenames.contains(&"Game (USA) (FastRom).sfc"));
        assert!(filenames.contains(&"Game (Europe) (60hz).sfc"));
    }

    #[test]
    fn recommendation_queries_exclude_special_roms() {
        let (mut db, _dir) = open_temp_db();

        let mut normal = make_game_entry_with_genre("snes", "Mario (USA).sfc", "Platform");
        normal.base_title = "Mario".into();
        normal.region = "usa".into();
        normal.box_art_url = Some("/img/mario.png".into());
        normal.rating = Some(4.5);

        let mut special = make_game_entry_with_genre("snes", "Mario (USA) (FastRom).sfc", "Platform");
        special.base_title = "Mario FastRom".into();
        special.region = "usa".into();
        special.box_art_url = Some("/img/mario.png".into());
        special.rating = Some(4.5);
        special.is_special = true;

        db.save_system_entries("snes", &[normal, special], None)
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

    #[test]
    fn regional_variants_excludes_clones_and_specials() {
        let (mut db, _dir) = open_temp_db();

        let mut original = make_game_entry("snes", "Game (USA).sfc", false);
        original.base_title = "game".into();
        original.region = "usa".into();

        let mut europe = make_game_entry("snes", "Game (Europe).sfc", false);
        europe.base_title = "game".into();
        europe.region = "europe".into();

        // Clone should be excluded
        let mut clone = make_game_entry("snes", "Game (Japan).sfc", false);
        clone.base_title = "game".into();
        clone.region = "japan".into();
        clone.is_clone = true;

        // Special should be excluded
        let mut special = make_game_entry("snes", "Game (USA) (FastRom).sfc", false);
        special.base_title = "game".into();
        special.region = "usa".into();
        special.is_special = true;

        db.save_system_entries("snes", &[original, europe, clone, special], None)
            .unwrap();

        let variants = db.regional_variants("snes", "Game (USA).sfc").unwrap();
        assert_eq!(variants.len(), 2);
        let filenames: Vec<&str> = variants.iter().map(|(f, _, _)| f.as_str()).collect();
        assert!(filenames.contains(&"Game (USA).sfc"));
        assert!(filenames.contains(&"Game (Europe).sfc"));
        assert!(!filenames.contains(&"Game (Japan).sfc")); // clone excluded
        assert!(!filenames.contains(&"Game (USA) (FastRom).sfc")); // special excluded
    }
}
