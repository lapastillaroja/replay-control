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
pub use game_library::SearchFilter;

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
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ImportProgress {
    pub state: ImportState,
    pub processed: usize,
    pub matched: usize,
    pub inserted: usize,
    pub elapsed_secs: u64,
    pub error: Option<String>,
    /// Bytes downloaded so far (only meaningful during `Downloading` state).
    #[serde(default)]
    pub download_bytes: u64,
    /// Total download size in bytes, if known from Content-Length.
    #[serde(default)]
    pub download_total: Option<u64>,
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
    pub rating_count: Option<u32>,
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
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
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
    pub rating_count: Option<u32>,
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
    /// Release year extracted from TOSEC filename tags or baked-in game_db.
    /// Enrichment may upgrade with LaunchBox release_year.
    pub release_year: Option<u16>,
    /// Cooperative play flag (from LaunchBox or TGDB).
    pub cooperative: bool,
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
    pub rating_count: Option<u32>,
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

/// Expected columns in the `game_library` table.
///
/// Keep in sync with the CREATE TABLE statement in [`MetadataDb::init_tables`].
/// Used by [`MetadataDb::validate_game_library_schema`] to detect stale schemas.
const GAME_LIBRARY_COLUMNS: &[&str] = &[
    "system",
    "rom_filename",
    "rom_path",
    "display_name",
    "base_title",
    "series_key",
    "region",
    "developer",
    "search_text",
    "genre",
    "genre_group",
    "rating",
    "rating_count",
    "players",
    "is_clone",
    "is_m3u",
    "is_translation",
    "is_hack",
    "is_special",
    "box_art_url",
    "driver_status",
    "size_bytes",
    "crc32",
    "hash_mtime",
    "hash_matched_name",
    "release_year",
    "cooperative",
];

/// Stateless query namespace for the metadata SQLite database.
///
/// All methods are associated functions that take `conn: &Connection` as their
/// first parameter. No connection ownership — the pool manages lifecycle.
pub struct MetadataDb;

impl MetadataDb {
    /// Tables to probe for corruption detection.
    pub const TABLES: &[&str] = &[
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
    /// Returns a raw `Connection` — the caller (or pool manager) owns it.
    pub fn open(storage_root: &Path) -> Result<(Connection, PathBuf)> {
        let dir = storage_root.join(RC_DIR);
        std::fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
        let db_path = dir.join(METADATA_DB_FILE);

        let conn = crate::db_common::open_connection(&db_path, "metadata.db")?;
        Self::init_tables(&conn)?;
        Self::validate_game_library_schema(&conn);

        if let Err(detail) = crate::db_common::probe_tables(&conn, Self::TABLES) {
            tracing::warn!("Metadata DB corrupt ({detail}), deleting and recreating");
            drop(conn);
            crate::db_common::delete_db_files(&db_path);
            let conn = crate::db_common::open_connection(&db_path, "metadata.db")?;
            Self::init_tables(&conn)?;
            // Fresh DB after corruption recovery — no need to validate schema.
            return Ok((conn, db_path));
        }

        Ok((conn, db_path))
    }

    /// Create all tables if they don't exist.
    pub fn init_tables(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS game_metadata (
                    system TEXT NOT NULL,
                    rom_filename TEXT NOT NULL,
                    description TEXT,
                    genre TEXT,
                    developer TEXT,
                    publisher TEXT,
                    release_year INTEGER,
                    rating REAL,
                    rating_count INTEGER,
                    cooperative INTEGER NOT NULL DEFAULT 0,
                    players INTEGER,
                    box_art_path TEXT,
                    screenshot_path TEXT,
                    title_path TEXT,
                    source TEXT NOT NULL,
                    fetched_at INTEGER NOT NULL,
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
                -- PK (repo_name, kind, filename) already covers repo_name-only lookups,
                -- so no separate idx_thumbidx_repo index is needed.

                CREATE TABLE IF NOT EXISTS game_library (
                    system TEXT NOT NULL,
                    rom_filename TEXT NOT NULL,
                    rom_path TEXT NOT NULL,
                    display_name TEXT,
                    base_title TEXT NOT NULL DEFAULT '',
                    series_key TEXT NOT NULL DEFAULT '',
                    region TEXT NOT NULL DEFAULT '',
                    developer TEXT NOT NULL DEFAULT '',
                    search_text TEXT NOT NULL DEFAULT '',
                    genre TEXT,
                    genre_group TEXT NOT NULL DEFAULT '',
                    rating REAL,
                    rating_count INTEGER,
                    players INTEGER,
                    is_clone INTEGER NOT NULL DEFAULT 0,
                    is_m3u INTEGER NOT NULL DEFAULT 0,
                    is_translation INTEGER NOT NULL DEFAULT 0,
                    is_hack INTEGER NOT NULL DEFAULT 0,
                    is_special INTEGER NOT NULL DEFAULT 0,
                    box_art_url TEXT,
                    driver_status TEXT,
                    size_bytes INTEGER NOT NULL DEFAULT 0,
                    crc32 INTEGER,
                    hash_mtime INTEGER,
                    hash_matched_name TEXT,
                    release_year INTEGER,
                    cooperative INTEGER NOT NULL DEFAULT 0,
                    PRIMARY KEY (system, rom_filename)
                );

                CREATE TABLE IF NOT EXISTS game_library_meta (
                    system TEXT PRIMARY KEY,
                    dir_mtime_secs INTEGER,
                    scanned_at INTEGER NOT NULL,
                    rom_count INTEGER NOT NULL DEFAULT 0,
                    total_size_bytes INTEGER NOT NULL DEFAULT 0
                );

                -- Covers: similar_by_genre (system + genre/genre_group), system_genre_groups,
                -- developer_genre_groups with system filter
                CREATE INDEX IF NOT EXISTS idx_game_library_genre
                  ON game_library (system, genre)
                  WHERE genre IS NOT NULL AND genre != '';

                CREATE INDEX IF NOT EXISTS idx_game_library_genre_group
                  ON game_library (system, genre_group)
                  WHERE genre_group != '';

                -- Covers: series_siblings (WHERE series_key = ?)
                CREATE INDEX IF NOT EXISTS idx_game_library_series_key
                  ON game_library (series_key)
                  WHERE series_key != '';

                -- Covers: find_developer_matches, games_by_developer,
                -- developer_games, developer_systems, developer_genre_groups,
                -- top_developers (COUNT(DISTINCT base_title) GROUP BY developer)
                CREATE INDEX IF NOT EXISTS idx_game_library_developer_title
                  ON game_library (developer, base_title)
                  WHERE developer != '';

                -- Covers: regional_variants, translations, hacks, specials (all filter
                -- by system + base_title), alias_variants (JOIN on system + base_title),
                -- wikidata_series_siblings (JOIN gl ON base_title COLLATE NOCASE),
                -- find_best_rom (WHERE base_title = ? COLLATE NOCASE)
                CREATE INDEX IF NOT EXISTS idx_game_library_base_title
                  ON game_library (system, base_title)
                  WHERE base_title != '';

                -- Covers: search filter coop_only, random_coop_games recommendation
                CREATE INDEX IF NOT EXISTS idx_game_library_cooperative
                  ON game_library (system, cooperative)
                  WHERE cooperative = 1;

                -- Covers: data_sources queries by source_type (get_data_source_stats,
                -- clear_thumbnail_index)
                CREATE INDEX IF NOT EXISTS idx_data_sources_type
                  ON data_sources (source_type);

                -- Drop the redundant idx_thumbidx_repo if it exists from older schema.
                -- The PK (repo_name, kind, filename) already covers repo_name prefix lookups.
                DROP INDEX IF EXISTS idx_thumbidx_repo;

                CREATE TABLE IF NOT EXISTS game_alias (
                    system TEXT NOT NULL,
                    base_title TEXT NOT NULL,
                    alias_name TEXT NOT NULL,
                    alias_region TEXT NOT NULL DEFAULT '',
                    source TEXT NOT NULL,
                    PRIMARY KEY (system, base_title, alias_name)
                );
                -- Covers: search_aliases (LIKE on alias_name)
                CREATE INDEX IF NOT EXISTS idx_game_alias_name
                    ON game_alias(alias_name COLLATE NOCASE);
                -- Covers: alias_variants, alias_base_titles (WHERE system = ? AND alias_name = ?)
                CREATE INDEX IF NOT EXISTS idx_game_alias_system_alias
                    ON game_alias(system, alias_name);

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
                -- Covers: game_series neighbor lookups (series_name + series_order),
                -- max series_order queries
                CREATE INDEX IF NOT EXISTS idx_game_series_order
                    ON game_series (series_name, series_order)
                    WHERE series_order IS NOT NULL;

",
        )
        .map_err(|e| Error::Other(format!("Failed to create tables: {e}")))?;

        // ── Legacy index cleanup ─────────────────────────────────────
        // (Column migrations are now handled by validate_game_library_schema.)

        // Replace single-column developer index with compound (developer, base_title)
        // to cover top_developers query (COUNT(DISTINCT base_title) GROUP BY developer).
        let _ = conn.execute_batch("DROP INDEX IF EXISTS idx_game_library_developer");

        Ok(())
    }

    /// Check that the `game_library` table has all expected columns.
    ///
    /// If any column is missing (schema outdated), drops the table and its
    /// companion `game_library_meta` so the next scan rebuilds them.
    /// This is safe because game_library is entirely derived data.
    fn validate_game_library_schema(conn: &Connection) {
        let actual: std::collections::HashSet<String> =
            match conn.prepare("PRAGMA table_info(game_library)") {
                Ok(mut stmt) => match stmt
                    .query_map([], |row| row.get::<_, String>(1))
                    .and_then(|rows| rows.collect::<std::result::Result<_, _>>())
                {
                    Ok(cols) => cols,
                    Err(e) => {
                        tracing::warn!("Failed to read game_library schema: {e}");
                        return;
                    }
                },
                Err(e) => {
                    tracing::warn!("Failed to prepare PRAGMA table_info: {e}");
                    return;
                }
            };

        if actual.is_empty() {
            // Table doesn't exist yet — nothing to validate.
            return;
        }

        let missing: Vec<&str> = GAME_LIBRARY_COLUMNS
            .iter()
            .filter(|col| !actual.contains(**col))
            .copied()
            .collect();

        if missing.is_empty() {
            return;
        }

        tracing::warn!(
            "game_library schema outdated, rebuilding table (missing columns: {})",
            missing.join(", ")
        );
        let _ = conn.execute_batch(
            "DROP TABLE IF EXISTS game_library; DROP TABLE IF EXISTS game_library_meta;",
        );
        // Recreate with current schema.
        let _ = conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS game_library (
                system TEXT NOT NULL,
                rom_filename TEXT NOT NULL,
                rom_path TEXT NOT NULL,
                display_name TEXT,
                base_title TEXT NOT NULL DEFAULT '',
                series_key TEXT NOT NULL DEFAULT '',
                region TEXT NOT NULL DEFAULT '',
                developer TEXT NOT NULL DEFAULT '',
                search_text TEXT NOT NULL DEFAULT '',
                genre TEXT,
                genre_group TEXT NOT NULL DEFAULT '',
                rating REAL,
                rating_count INTEGER,
                players INTEGER,
                is_clone INTEGER NOT NULL DEFAULT 0,
                is_m3u INTEGER NOT NULL DEFAULT 0,
                is_translation INTEGER NOT NULL DEFAULT 0,
                is_hack INTEGER NOT NULL DEFAULT 0,
                is_special INTEGER NOT NULL DEFAULT 0,
                box_art_url TEXT,
                driver_status TEXT,
                size_bytes INTEGER NOT NULL DEFAULT 0,
                crc32 INTEGER,
                hash_mtime INTEGER,
                hash_matched_name TEXT,
                release_year INTEGER,
                cooperative INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (system, rom_filename)
            );
            CREATE TABLE IF NOT EXISTS game_library_meta (
                system TEXT PRIMARY KEY,
                dir_mtime_secs INTEGER,
                scanned_at INTEGER NOT NULL,
                rom_count INTEGER NOT NULL DEFAULT 0,
                total_size_bytes INTEGER NOT NULL DEFAULT 0
            );",
        );
    }

    /// Helper: convert a row to GameEntry (used by multiple queries).
    ///
    /// Column order must match all SELECT statements that use this helper:
    ///   system, rom_filename, rom_path, display_name, base_title, series_key,
    ///   region, developer, genre, genre_group, rating, rating_count, players,
    ///   is_clone, is_m3u, is_translation, is_hack, is_special, box_art_url,
    ///   driver_status, size_bytes, crc32, hash_mtime, hash_matched_name,
    ///   release_year, cooperative
    pub(crate) fn row_to_game_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<GameEntry> {
        Ok(GameEntry {
            system: row.get(0)?,
            rom_filename: row.get(1)?,
            rom_path: row.get(2)?,
            display_name: row.get(3)?,
            base_title: row.get::<_, String>(4).unwrap_or_default(),
            series_key: row.get::<_, String>(5).unwrap_or_default(),
            region: row.get::<_, String>(6).unwrap_or_default(),
            developer: row.get::<_, String>(7).unwrap_or_default(),
            genre: row.get(8)?,
            genre_group: row.get::<_, String>(9).unwrap_or_default(),
            rating: row.get(10)?,
            rating_count: row
                .get::<_, Option<i64>>(11)
                .unwrap_or_default()
                .map(|c| c as u32),
            players: row
                .get::<_, Option<i32>>(12)
                .unwrap_or_default()
                .map(|p| p as u8),
            is_clone: row.get(13).unwrap_or_default(),
            is_m3u: row.get(14).unwrap_or_default(),
            is_translation: row.get(15).unwrap_or_default(),
            is_hack: row.get(16).unwrap_or_default(),
            is_special: row.get(17).unwrap_or_default(),
            box_art_url: row.get(18).unwrap_or_default(),
            driver_status: row.get(19).unwrap_or_default(),
            size_bytes: row
                .get::<_, Option<i64>>(20)
                .unwrap_or_default()
                .unwrap_or(0) as u64,
            crc32: row
                .get::<_, Option<i64>>(21)
                .unwrap_or_default()
                .map(|c| c as u32),
            hash_mtime: row.get(22).unwrap_or_default(),
            hash_matched_name: row.get(23).unwrap_or_default(),
            release_year: row
                .get::<_, Option<i32>>(24)
                .unwrap_or_default()
                .map(|y| y as u16),
            cooperative: row.get::<_, bool>(25).unwrap_or_default(),
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

    /// Open a metadata DB connection backed by a temp directory.
    /// Returns a mutable `Connection` so tests can call both read and write methods.
    pub(crate) fn open_temp_db() -> (Connection, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let (conn, _path) = MetadataDb::open(dir.path()).unwrap();
        (conn, dir)
    }

    pub(crate) fn make_metadata(box_art: Option<&str>) -> GameMetadata {
        GameMetadata {
            description: None,
            rating: Some(4.0),
            rating_count: None,
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
            rating_count: None,
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
            release_year: None,
            cooperative: false,
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
