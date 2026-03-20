//! Local SQLite cache for external game metadata (descriptions, ratings, etc.).
//!
//! Stored at `<rom_storage>/.replay-control/metadata.db`.

mod aliases_series;
mod data_sources;
mod game_library;
mod game_metadata;
mod recommendations;
mod relationships;

pub use aliases_series::SequelChainInfo;
pub use game_library::DeveloperGamesFilter;

use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::error::{Error, Result};

// Re-export RC_DIR from storage (the canonical definition).
pub use crate::storage::RC_DIR;

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
    pub title_path: Option<String>,
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
    /// Developer/manufacturer name.
    /// For arcade: populated from arcade_db manufacturer at scan time.
    /// For console: populated from game_metadata.developer via enrichment.
    pub developer: String,
}

/// Full enrichment update for a ROM in game_library (including driver_status).
#[derive(Debug, Clone)]
pub struct RomEnrichment {
    pub rom_filename: String,
    pub box_art_url: Option<String>,
    pub genre: Option<String>,
    pub players: Option<u8>,
    pub rating: Option<f32>,
    pub driver_status: Option<String>,
}

/// Lightweight enrichment update for a ROM in game_library (no driver_status).
#[derive(Debug, Clone)]
pub struct BoxArtGenreRating {
    pub rom_filename: String,
    pub box_art_url: Option<String>,
    pub genre: Option<String>,
    pub players: Option<u8>,
    pub rating: Option<f32>,
}

/// A game alias entry for bulk insertion into the `game_alias` table.
#[derive(Debug, Clone)]
pub struct AliasInsert {
    pub system: String,
    pub base_title: String,
    pub alias_name: String,
    pub alias_region: String,
    pub source: String,
}

/// An image path update for bulk insertion via `bulk_update_image_paths`.
#[derive(Debug, Clone)]
pub struct ImagePathUpdate {
    pub system: String,
    pub rom_filename: String,
    pub box_art_path: Option<String>,
    pub screenshot_path: Option<String>,
    pub title_path: Option<String>,
}

/// A game series entry for bulk insertion into the `game_series` table.
#[derive(Debug, Clone)]
pub struct SeriesInsert {
    pub system: String,
    pub base_title: String,
    pub series_name: String,
    pub series_order: Option<i32>,
    pub source: String,
    pub follows_base_title: Option<String>,
    pub followed_by_base_title: Option<String>,
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
        "game_series",
    ];

    /// Open (or create) the metadata database at `<storage_root>/.replay-control/metadata.db`.
    ///
    /// Opens the metadata DB with strategy appropriate for the filesystem.
    /// Runs table init, probes for corruption, auto-recreates if corrupt.
    pub fn open(storage_root: &Path, is_local: bool) -> Result<Self> {
        let dir = storage_root.join(RC_DIR);
        std::fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
        let db_path = dir.join(METADATA_DB_FILE);

        let conn = crate::db_common::open_connection(&db_path, "metadata.db", is_local)?;
        let db = Self {
            conn,
            db_path: db_path.clone(),
        };
        db.init()?;

        if let Err(detail) = crate::db_common::probe_tables(&db.conn, Self::TABLES) {
            tracing::warn!("Metadata DB corrupt ({detail}), deleting and recreating");
            drop(db);
            crate::db_common::delete_db_files(&db_path);
            let conn = crate::db_common::open_connection(&db_path, "metadata.db", is_local)?;
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
                    title_path TEXT,
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
                    developer TEXT NOT NULL DEFAULT '',
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

                CREATE INDEX IF NOT EXISTS idx_game_library_developer
                  ON game_library (developer)
                  WHERE developer != '';

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

                CREATE TABLE IF NOT EXISTS game_series (
                    system TEXT NOT NULL,
                    base_title TEXT NOT NULL,
                    series_name TEXT NOT NULL,
                    series_order INTEGER,
                    source TEXT NOT NULL,
                    follows_base_title TEXT,
                    followed_by_base_title TEXT,
                    PRIMARY KEY (system, base_title, series_name)
                );
                CREATE INDEX IF NOT EXISTS idx_game_series_name
                    ON game_series(series_name COLLATE NOCASE);
                CREATE INDEX IF NOT EXISTS idx_game_series_system
                    ON game_series(system, series_name);

",
            )
            .map_err(|e| Error::Other(format!("Failed to create tables: {e}")))?;

        Ok(())
    }

    /// Provide a reference to the raw connection (for use by thumbnail_manifest).
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Get path to the database file.
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    /// Helper: convert a row to GameEntry (used by multiple queries).
    pub(crate) fn row_to_game_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<GameEntry> {
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
            developer: row.get::<_, String>(22).unwrap_or_default(),
        })
    }
}

pub(crate) fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Open a MetadataDb backed by a temp directory.
    pub(crate) fn open_temp_db() -> (MetadataDb, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db = MetadataDb::open(dir.path(), true).unwrap();
        (db, dir)
    }

    pub(crate) fn make_metadata(box_art: Option<&str>) -> GameMetadata {
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
            title_path: None,
        }
    }

    pub(crate) fn make_metadata_with_genre(genre: &str) -> GameMetadata {
        GameMetadata {
            genre: Some(genre.into()),
            ..make_metadata(None)
        }
    }

    /// Create test metadata with a description (and optionally a box_art_path).
    pub(crate) fn make_metadata_with_desc(desc: &str, box_art: Option<&str>) -> GameMetadata {
        GameMetadata {
            description: Some(desc.into()),
            ..make_metadata(box_art)
        }
    }

    pub(crate) fn make_game_entry(system: &str, filename: &str, is_m3u: bool) -> GameEntry {
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
            series_key: String::new(),
            developer: String::new(),
        }
    }

    pub(crate) fn make_game_entry_with_genre(
        system: &str,
        filename: &str,
        genre: &str,
    ) -> GameEntry {
        GameEntry {
            genre: Some(genre.into()),
            genre_group: crate::genre::normalize_genre(genre).to_string(),
            ..make_game_entry(system, filename, false)
        }
    }
}
