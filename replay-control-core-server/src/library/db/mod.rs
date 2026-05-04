//! Local SQLite database for the user's game library.
//!
//! Stored centrally at `<data_dir>/storages/<storage_id>/library.db` (the
//! `data_dir` defaults to `/var/lib/replay-control` on Pi). Rebuildable from
//! the ROM filesystem plus the bundled catalog and optional LaunchBox XML
//! import. The companion `user_data.db` stays per-storage and is not managed
//! here.

mod aliases_series;
mod data_sources;
mod game_library;
mod game_metadata;
mod recommendations;
mod relationships;
pub mod release_dates;
pub mod stats;

pub use aliases_series::SequelChainInfo;
pub use game_library::SearchFilter;
pub use release_dates::{
    ReleaseDateRow, StaticReleaseData, fetch_static_release_data, region_pref_to_db_region,
};

use std::path::Path;

use rusqlite::Connection;

use replay_control_core::error::{Error, Result};

// Re-export RC_DIR from storage (the canonical definition).
pub use crate::storage::RC_DIR;

/// Filename for the SQLite library database.
pub const LIBRARY_DB_FILE: &str = "library.db";
/// Legacy filename for the pre-0.5 library database. Removed on first open
/// of the new `library.db` — see [`cleanup_legacy_metadata_db`].
const LEGACY_METADATA_DB_FILE: &str = "metadata.db";
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

pub use replay_control_core::library_db::{
    DriverStatusCounts, ImportProgress, ImportState, ImportStats, LibrarySummary, MetadataStats,
    SystemCoverage,
};

/// Per-system coverage stats from a single `GROUP BY system` pass over `game_library`.
#[derive(Debug, Clone, Default)]
pub struct SystemCoverageStats {
    pub system: String,
    pub with_genre: usize,
    pub with_developer: usize,
    pub with_rating: usize,
    pub size_bytes: u64,
    pub clone_count: usize,
    pub hack_count: usize,
    pub translation_count: usize,
    pub special_count: usize,
    pub coop_count: usize,
    pub verified_count: usize,
    pub min_year: Option<u16>,
    pub max_year: Option<u16>,
}

pub use replay_control_core::DatePrecision;

/// Newtype SQL bridge for [`DatePrecision`].
///
/// `DatePrecision` is a pure type in `replay-control-core` (wasm-safe). We
/// can't `impl rusqlite::ToSql for DatePrecision` here — both the trait and
/// the type are foreign. This newtype sidesteps the orphan rule and is
/// scoped to the one crate that persists `DatePrecision` to SQLite.
pub(crate) struct DpSql(pub DatePrecision);

impl rusqlite::ToSql for DpSql {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(self.0.as_str().into())
    }
}

impl rusqlite::types::FromSql for DpSql {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        let s = value.as_str()?;
        DatePrecision::from_str(s).map(DpSql).ok_or_else(|| {
            rusqlite::types::FromSqlError::Other(format!("invalid DatePrecision: {s}").into())
        })
    }
}

/// Extract the year from an ISO 8601 partial/full date string (`"YYYY"`, `"YYYY-MM"`, `"YYYY-MM-DD"`).
pub fn year_from_release_date(date: &str) -> Option<u16> {
    date.get(..4).and_then(|y| y.parse().ok())
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
    /// Release date in ISO 8601 partial/full format: `"YYYY"`, `"YYYY-MM"`, or `"YYYY-MM-DD"`.
    pub release_date: Option<String>,
    /// Precision of `release_date`.
    pub release_precision: Option<DatePrecision>,
    /// Region the resolver picked for this date (`"usa" | "japan" | "europe" | "world" | ...`).
    pub release_region_used: Option<String>,
    pub cooperative: bool,
    pub fetched_at: i64,
    pub box_art_path: Option<String>,
    pub screenshot_path: Option<String>,
    pub title_path: Option<String>,
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
    /// Release date (ISO 8601 partial/full: `"YYYY"`, `"YYYY-MM"`, or `"YYYY-MM-DD"`).
    /// Resolved from `game_release_date` for the user's preferred region, then mirrored here.
    pub release_date: Option<String>,
    /// Precision of `release_date`.
    pub release_precision: Option<DatePrecision>,
    /// Region the resolver picked for this date (UI hint when falling back).
    pub release_region_used: Option<String>,
    /// Cooperative play flag (from imported metadata).
    pub cooperative: bool,
}

impl GameEntry {
    /// Derive the release year from `release_date` (if any).
    pub fn release_year(&self) -> Option<u16> {
        self.release_date
            .as_deref()
            .and_then(year_from_release_date)
    }
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

/// SQL to create the `game_metadata` table. Single source of truth used by
/// `init_tables()` and `validate_game_metadata_schema()`.
const CREATE_GAME_METADATA_SQL: &str = "
    CREATE TABLE IF NOT EXISTS game_metadata (
        system TEXT NOT NULL,
        rom_filename TEXT NOT NULL,
        description TEXT,
        genre TEXT,
        developer TEXT,
        publisher TEXT,
        release_date TEXT,
        release_precision TEXT,
        release_region_used TEXT,
        rating REAL,
        rating_count INTEGER,
        cooperative INTEGER NOT NULL DEFAULT 0,
        players INTEGER,
        box_art_path TEXT,
        screenshot_path TEXT,
        title_path TEXT,
        fetched_at INTEGER NOT NULL,
        PRIMARY KEY (system, rom_filename)
    );
";

/// SQL to create the `game_release_date` table (multi-region, full-precision).
const CREATE_GAME_RELEASE_DATE_SQL: &str = "
    CREATE TABLE IF NOT EXISTS game_release_date (
        system       TEXT NOT NULL,
        base_title   TEXT NOT NULL,
        region       TEXT NOT NULL,
        release_date TEXT NOT NULL,
        precision    TEXT NOT NULL,
        source       TEXT NOT NULL,
        PRIMARY KEY (system, base_title, region)
    );
    CREATE INDEX IF NOT EXISTS idx_release_date_lookup ON game_release_date(system, base_title);
    CREATE INDEX IF NOT EXISTS idx_release_date_chrono ON game_release_date(release_date);
";

/// SQL to create the `game_library` table. Single source of truth used by
/// `init_tables()`, `validate_game_library_schema()`, and tests.
const CREATE_GAME_LIBRARY_SQL: &str = "
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
        release_date TEXT,
        release_precision TEXT,
        release_region_used TEXT,
        cooperative INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (system, rom_filename)
    );
";

/// SQL to create the `game_library_meta` table.
const CREATE_GAME_LIBRARY_META_SQL: &str = "
    CREATE TABLE IF NOT EXISTS game_library_meta (
        system TEXT PRIMARY KEY,
        dir_mtime_secs INTEGER,
        scanned_at INTEGER NOT NULL,
        rom_count INTEGER NOT NULL DEFAULT 0,
        total_size_bytes INTEGER NOT NULL DEFAULT 0
    );
";

/// Expected columns in the `game_library` table.
/// Used by [`LibraryDb::validate_game_library_schema`] to detect stale schemas.
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
    "release_date",
    "release_precision",
    "release_region_used",
    "cooperative",
];

/// Expected columns in the `game_metadata` table.
/// Used by [`LibraryDb::validate_game_metadata_schema`] to detect stale schemas.
const GAME_METADATA_COLUMNS: &[&str] = &[
    "system",
    "rom_filename",
    "description",
    "genre",
    "developer",
    "publisher",
    "release_date",
    "release_precision",
    "release_region_used",
    "rating",
    "rating_count",
    "cooperative",
    "players",
    "box_art_path",
    "screenshot_path",
    "title_path",
    "fetched_at",
];

/// Expected columns in the `game_release_date` table.
const GAME_RELEASE_DATE_COLUMNS: &[&str] = &[
    "system",
    "base_title",
    "region",
    "release_date",
    "precision",
    "source",
];

/// Stateless query namespace for the metadata SQLite database.
///
/// All methods are associated functions that take `conn: &Connection` as their
/// first parameter. No connection ownership — the pool manages lifecycle.
pub struct LibraryDb;

impl LibraryDb {
    /// Tables to probe for corruption detection.
    pub const TABLES: &[&str] = &[
        "game_metadata",
        "game_library",
        "game_release_date",
        "data_sources",
        "thumbnail_index",
        "game_alias",
        "game_series",
    ];

    /// Open (or create) the library database at the given file path.
    /// Library is rebuildable cache, so a bad-header / probe-failure file
    /// is silently deleted and recreated; runtime corruption is surfaced
    /// via the pool's corruption banner instead.
    pub fn open_at(db_path: &Path) -> Result<Connection> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }

        if crate::sqlite::has_invalid_sqlite_header(db_path) {
            tracing::warn!(
                "Library DB at {} has invalid SQLite header — deleting and recreating",
                db_path.display()
            );
            crate::sqlite::delete_db_files(db_path);
        }

        let conn = crate::sqlite::open_connection(db_path, "library.db")?;
        Self::init_tables(&conn)?;
        Self::run_migrations(&conn)?;

        if let Err(detail) = crate::sqlite::probe_tables(&conn, Self::TABLES) {
            tracing::warn!("Library DB corrupt ({detail}), deleting and recreating");
            drop(conn);
            crate::sqlite::delete_db_files(db_path);
            let conn = crate::sqlite::open_connection(db_path, "library.db")?;
            Self::init_tables(&conn)?;
            Self::run_migrations(&conn)?;
            return Ok(conn);
        }

        Ok(conn)
    }

    /// Back-compat shim: open under `<storage_root>/.replay-control/library.db`.
    /// Used by tests and the `library_report` CLI; production goes through
    /// `open_at` directly with the central data dir path.
    pub fn open(storage_root: &Path) -> Result<Connection> {
        let dir = storage_root.join(RC_DIR);
        std::fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
        cleanup_legacy_metadata_db(&dir);
        Self::open_at(&dir.join(LIBRARY_DB_FILE))
    }

    /// Move an existing per-storage `library.db` (and its WAL/SHM/journal
    /// sidecars) from `<storage>/.replay-control/library.db` to `dest`.
    ///
    /// Mirrors [`crate::settings::SettingsStore::migrate_from_storage`]:
    /// no-op if `dest` already exists; no-op if the old file is missing;
    /// atomic rename when possible, copy + delete fallback across filesystems.
    /// The legacy `metadata.db` sidecar is cleaned up regardless.
    pub fn migrate_from_storage(storage_root: &Path, dest: &Path) -> Result<()> {
        let old_dir = storage_root.join(RC_DIR);
        cleanup_legacy_metadata_db(&old_dir);

        if dest.exists() {
            tracing::debug!(
                "Library DB already at {}, skipping migration",
                dest.display()
            );
            return Ok(());
        }

        let old_path = old_dir.join(LIBRARY_DB_FILE);
        if !old_path.exists() {
            return Ok(());
        }

        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }

        // Move the main DB file. Try rename first; fall back to copy+delete
        // when the source and destination are on different filesystems
        // (the common case here — old is on ROM storage, new is on the OS SD).
        if std::fs::rename(&old_path, dest).is_err() {
            std::fs::copy(&old_path, dest).map_err(|e| Error::io(dest, e))?;
            if let Err(e) = std::fs::remove_file(&old_path) {
                tracing::warn!(
                    "Failed to delete old library.db at {}: {e}",
                    old_path.display()
                );
            }
        }

        // SQLite would recover any sidecars left behind, but moving them
        // alongside the main file keeps the storage clean.
        for ext in ["db-wal", "db-shm", "db-journal"] {
            let src = old_path.with_extension(ext);
            let dst = dest.with_extension(ext);
            match std::fs::rename(&src, &dst) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => {
                    if std::fs::copy(&src, &dst).is_ok()
                        && let Err(e) = std::fs::remove_file(&src)
                    {
                        tracing::warn!("Failed to delete sidecar {}: {e}", src.display());
                    }
                }
            }
        }

        tracing::info!(
            "Library DB migrated: {} -> {}",
            old_path.display(),
            dest.display()
        );
        Ok(())
    }

    /// Create all tables if they don't exist.
    ///
    /// On column-set mismatch, drop the four rebuildable derived tables
    /// (`game_library`, `game_library_meta`, `game_metadata`,
    /// `game_release_date`) so `CREATE TABLE IF NOT EXISTS` recreates
    /// them at the new shape. Their content comes from filesystem scans,
    /// LaunchBox import, and build-time seed — all reproducible, so the
    /// drop is a cache flush, not data loss.
    ///
    /// Real *additive* schema upgrades (new column on an existing table
    /// the user has populated and we don't want to wipe) should go
    /// through `run_migrations` instead.
    pub fn init_tables(conn: &Connection) -> Result<()> {
        if Self::table_needs_rebuild(conn, "game_library", GAME_LIBRARY_COLUMNS) {
            let _ = conn.execute_batch(
                "DROP TABLE IF EXISTS game_library; DROP TABLE IF EXISTS game_library_meta;",
            );
        }
        if Self::table_needs_rebuild(conn, "game_metadata", GAME_METADATA_COLUMNS) {
            let _ = conn.execute_batch("DROP TABLE IF EXISTS game_metadata;");
        }
        if Self::table_needs_rebuild(conn, "game_release_date", GAME_RELEASE_DATE_COLUMNS) {
            let _ = conn.execute_batch("DROP TABLE IF EXISTS game_release_date;");
        }

        conn.execute_batch(CREATE_GAME_METADATA_SQL)
            .map_err(|e| Error::Other(format!("Failed to create game_metadata: {e}")))?;
        conn.execute_batch(CREATE_GAME_RELEASE_DATE_SQL)
            .map_err(|e| Error::Other(format!("Failed to create game_release_date: {e}")))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS data_sources (
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
            ",
        )
        .map_err(|e| Error::Other(format!("Failed to create tables: {e}")))?;

        conn.execute_batch(CREATE_GAME_LIBRARY_SQL)
            .map_err(|e| Error::Other(format!("Failed to create game_library: {e}")))?;
        conn.execute_batch(CREATE_GAME_LIBRARY_META_SQL)
            .map_err(|e| Error::Other(format!("Failed to create game_library_meta: {e}")))?;

        conn.execute_batch(
            "-- Covers: similar_by_genre (system + genre/genre_group), system_genre_groups,
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

    /// Current schema version. Bump when adding a new `migrate_v{N-1}_v{N}`
    /// step in [`Self::run_migrations`].
    pub const SCHEMA_VERSION: i64 = 1;

    /// Run pending additive migrations.
    ///
    /// Reads the stored version from `schema_version`, applies each
    /// `migrate_v{N-1}_v{N}` step in a single transaction, stamps on
    /// success. Use this for `ADD COLUMN` / `CREATE INDEX` upgrades on
    /// tables we don't want to drop (e.g. user-settings). The four
    /// derived tables in `init_tables` use the simpler drop-and-recreate
    /// path on column-set mismatch since their content is reproducible.
    pub fn run_migrations(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_version (
                version INTEGER PRIMARY KEY,
                applied_at INTEGER NOT NULL
            )",
        )
        .map_err(|e| Error::Other(format!("create schema_version: {e}")))?;

        let current: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_version",
                [],
                |r| r.get(0),
            )
            .map_err(|e| Error::Other(format!("read schema_version: {e}")))?;

        if current >= Self::SCHEMA_VERSION {
            return Ok(());
        }

        // Each step is gated by `if current < N` so a brand-new DB (which
        // already had `init_tables` build the v1 shape) just stamps to
        // SCHEMA_VERSION; an upgrade from N-1 runs the bridge step.
        // No destructive `DROP TABLE` paths — if a future migration
        // genuinely needs to clear data, log a WARN above the SQL.
        //
        // (No migrations defined yet — SCHEMA_VERSION = 1 is the
        // current shape that `init_tables` builds. Future changes:
        // `if current < 2 { migrate_v1_v2(conn)?; }`.)

        let now = unix_now();
        conn.execute(
            "INSERT INTO schema_version (version, applied_at) VALUES (?1, ?2)",
            rusqlite::params![Self::SCHEMA_VERSION, now],
        )
        .map_err(|e| Error::Other(format!("stamp schema_version: {e}")))?;

        if current > 0 {
            tracing::info!(
                "Library DB migrated from v{current} to v{}",
                Self::SCHEMA_VERSION
            );
        }
        Ok(())
    }

    /// Check if a table's schema matches the expected columns.
    /// Returns `true` if the table needs to be dropped and recreated.
    fn table_needs_rebuild(conn: &Connection, table: &str, expected: &[&str]) -> bool {
        let actual: std::collections::HashSet<String> =
            match conn.prepare(&format!("PRAGMA table_info({table})")) {
                Ok(mut stmt) => match stmt
                    .query_map([], |row| row.get::<_, String>(1))
                    .and_then(|rows| rows.collect::<std::result::Result<_, _>>())
                {
                    Ok(cols) => cols,
                    Err(e) => {
                        tracing::warn!("Failed to read {table} schema: {e}");
                        return false;
                    }
                },
                Err(e) => {
                    tracing::warn!("Failed to prepare PRAGMA table_info({table}): {e}");
                    return false;
                }
            };

        if actual.is_empty() {
            return false; // Table doesn't exist yet.
        }

        let missing: Vec<&str> = expected
            .iter()
            .filter(|col| !actual.contains(**col))
            .copied()
            .collect();

        if missing.is_empty() && actual.len() == expected.len() {
            return false; // Schema matches exactly.
        }

        if missing.is_empty() {
            tracing::warn!(
                "{table} schema has extra columns ({} actual vs {} expected), rebuilding",
                actual.len(),
                expected.len(),
            );
        } else {
            tracing::warn!(
                "{table} schema outdated, rebuilding (missing: {})",
                missing.join(", ")
            );
        }
        true
    }

    /// Helper: convert a row to GameEntry (used by multiple queries).
    ///
    /// Column order must match all SELECT statements that use this helper:
    ///   system, rom_filename, rom_path, display_name, base_title, series_key,
    ///   region, developer, genre, genre_group, rating, rating_count, players,
    ///   is_clone, is_m3u, is_translation, is_hack, is_special, box_art_url,
    ///   driver_status, size_bytes, crc32, hash_mtime, hash_matched_name,
    ///   release_date, release_precision, release_region_used, cooperative
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
            release_date: row.get::<_, Option<String>>(24).unwrap_or_default(),
            release_precision: row
                .get::<_, Option<DpSql>>(25)
                .unwrap_or_default()
                .map(|DpSql(d)| d),
            release_region_used: row.get::<_, Option<String>>(26).unwrap_or_default(),
            cooperative: row.get::<_, bool>(27).unwrap_or_default(),
        })
    }
}

pub(crate) fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Remove legacy pre-0.5 `metadata.db` files from the `.replay-control` directory.
///
/// The library DB used to be called `metadata.db`. It was rebuildable then and
/// remains rebuildable now, so we delete the file outright on upgrade rather
/// than migrating schema: the startup pipeline rescans ROMs, re-imports
/// LaunchBox data from `launchbox-metadata.xml`, and rebuilds the thumbnail
/// index from disk. No data is lost.
///
/// Idempotent: silent no-op once the legacy files are gone.
fn cleanup_legacy_metadata_db(dir: &Path) {
    let legacy = dir.join(LEGACY_METADATA_DB_FILE);
    if !legacy.exists() {
        return;
    }
    tracing::info!(
        "Removing legacy {} (rebuilding as library.db)",
        legacy.display()
    );
    crate::sqlite::delete_db_files(&legacy);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Open a library DB connection backed by a temp directory.
    /// Returns a mutable `Connection` so tests can call both read and write methods.
    pub(crate) fn open_temp_db() -> (Connection, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let conn = LibraryDb::open(dir.path()).unwrap();
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
            release_date: None,
            release_precision: None,
            release_region_used: None,
            cooperative: false,
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
            release_date: None,
            release_precision: None,
            release_region_used: None,
            cooperative: false,
        }
    }

    #[test]
    fn schema_rebuild_on_missing_column() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("library.db");

        // Intentionally incomplete schema (missing most columns) to simulate
        // an outdated DB. Does NOT use CREATE_GAME_LIBRARY_SQL.
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE game_library (
                    system TEXT NOT NULL,
                    rom_filename TEXT NOT NULL,
                    rom_path TEXT NOT NULL,
                    PRIMARY KEY (system, rom_filename)
                );
                INSERT INTO game_library (system, rom_filename, rom_path)
                VALUES ('snes', 'Mario.sfc', '/roms/snes/Mario.sfc');",
            )
            .unwrap();

            // Verify the row exists.
            let count: i64 = conn
                .query_row("SELECT COUNT(*) FROM game_library", [], |r| r.get(0))
                .unwrap();
            assert_eq!(count, 1);
        }

        // Open via LibraryDb — this runs validate_game_library_schema.
        let conn = LibraryDb::open(dir.path()).unwrap();

        // The old row should be gone (table was dropped and recreated).
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM game_library", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0, "old rows should be gone after schema rebuild");

        // The table should have the cooperative column now.
        let has_cooperative: bool = conn
            .prepare("PRAGMA table_info(game_library)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .flatten()
            .any(|col| col == "cooperative");
        assert!(
            has_cooperative,
            "rebuilt table should have cooperative column"
        );
    }

    #[test]
    fn open_recovers_from_clobbered_header() {
        // Header is checked pre-open because `Connection::open` errors on a
        // bad header before `probe_tables`-based recovery can run.
        let dir = tempfile::tempdir().unwrap();
        let rc = dir.path().join(RC_DIR);
        std::fs::create_dir_all(&rc).unwrap();

        let lib_path = rc.join(LIBRARY_DB_FILE);
        std::fs::write(&lib_path, [0xDEu8; 4096]).unwrap();

        let conn = LibraryDb::open(dir.path()).expect("open must recover from clobbered header");
        assert!(lib_path.exists());

        // Fresh DB → expected tables exist and are empty.
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM game_library", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn migrate_from_storage_moves_db_and_sidecars() {
        let storage = tempfile::tempdir().unwrap();
        let central = tempfile::tempdir().unwrap();

        // Plant a per-storage library.db with WAL/SHM sidecars.
        let old_dir = storage.path().join(RC_DIR);
        std::fs::create_dir_all(&old_dir).unwrap();
        let old_path = old_dir.join(LIBRARY_DB_FILE);
        std::fs::write(&old_path, b"old-db").unwrap();
        std::fs::write(old_path.with_extension("db-wal"), b"wal").unwrap();
        std::fs::write(old_path.with_extension("db-shm"), b"shm").unwrap();

        let dest = central.path().join("library.db");
        LibraryDb::migrate_from_storage(storage.path(), &dest).unwrap();

        assert!(dest.exists(), "destination library.db should exist");
        assert_eq!(std::fs::read(&dest).unwrap(), b"old-db");
        assert!(
            dest.with_extension("db-wal").exists(),
            "WAL sidecar should follow"
        );
        assert!(
            dest.with_extension("db-shm").exists(),
            "SHM sidecar should follow"
        );
        assert!(!old_path.exists(), "old library.db should be gone");
    }

    #[test]
    fn migrate_from_storage_skips_when_dest_exists() {
        let storage = tempfile::tempdir().unwrap();
        let central = tempfile::tempdir().unwrap();

        let old_dir = storage.path().join(RC_DIR);
        std::fs::create_dir_all(&old_dir).unwrap();
        std::fs::write(old_dir.join(LIBRARY_DB_FILE), b"old-db").unwrap();

        let dest = central.path().join("library.db");
        std::fs::write(&dest, b"newer-db").unwrap();

        LibraryDb::migrate_from_storage(storage.path(), &dest).unwrap();

        // Destination is unchanged; old file is left alone.
        assert_eq!(std::fs::read(&dest).unwrap(), b"newer-db");
        assert!(old_dir.join(LIBRARY_DB_FILE).exists());
    }

    #[test]
    fn migrate_from_storage_noop_when_no_old_file() {
        let storage = tempfile::tempdir().unwrap();
        let central = tempfile::tempdir().unwrap();
        let dest = central.path().join("library.db");
        LibraryDb::migrate_from_storage(storage.path(), &dest).unwrap();
        assert!(!dest.exists());
    }

    #[test]
    fn open_at_creates_parent_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp
            .path()
            .join("storages")
            .join("a-b-c-d")
            .join("library.db");
        let _conn = LibraryDb::open_at(&nested).unwrap();
        assert!(nested.exists());
    }

    #[test]
    fn cleanup_legacy_metadata_db_removes_all_siblings() {
        let dir = tempfile::tempdir().unwrap();
        let rc = dir.path().join(RC_DIR);
        std::fs::create_dir_all(&rc).unwrap();

        // Plant a legacy metadata.db + every sidecar file.
        let legacy = rc.join("metadata.db");
        std::fs::write(&legacy, b"legacy-db").unwrap();
        std::fs::write(legacy.with_extension("db-wal"), b"wal").unwrap();
        std::fs::write(legacy.with_extension("db-shm"), b"shm").unwrap();
        std::fs::write(legacy.with_extension("db-journal"), b"journal").unwrap();

        let _conn = LibraryDb::open(dir.path()).unwrap();
        let lib_path = dir.path().join(RC_DIR).join(LIBRARY_DB_FILE);

        assert!(lib_path.exists(), "new library.db should be created");
        assert!(!legacy.exists(), "legacy metadata.db should be gone");
        assert!(
            !legacy.with_extension("db-wal").exists(),
            "metadata.db-wal should be gone"
        );
        assert!(
            !legacy.with_extension("db-shm").exists(),
            "metadata.db-shm should be gone"
        );
        assert!(
            !legacy.with_extension("db-journal").exists(),
            "metadata.db-journal should be gone"
        );
    }

    pub(crate) fn make_game_entry_with_genre(
        system: &str,
        filename: &str,
        genre: &str,
    ) -> GameEntry {
        GameEntry {
            genre: Some(genre.into()),
            genre_group: replay_control_core::genre::normalize_genre(genre).to_string(),
            ..make_game_entry(system, filename, false)
        }
    }
}
