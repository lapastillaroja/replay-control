//! Local SQLite database for the user's game library.
//!
//! Stored centrally at `<data_dir>/storages/<storage_id>/library.db` (the
//! `data_dir` defaults to `/var/lib/replay-control` on Pi). Rebuildable from
//! the ROM filesystem plus the bundled catalog and optional LaunchBox XML
//! import. The companion `user_data.db` stays per-storage and is not managed
//! here.

mod aliases_series;
pub mod game_description;
mod game_library;
mod game_library_system_stats;
pub mod game_resource;
pub mod library_meta;
mod recommendations;
mod relationships;
pub mod release_dates;

pub use aliases_series::SequelChainInfo;
pub use game_description::GameDescription;
pub use game_library::{DISCOVERY_SAVE_CHUNK_ROWS, DiscoveryFinalizeStats, SearchFilter};
pub use game_library_system_stats::StatsRefreshState;
pub use release_dates::{
    ReleaseDateMirrorUpdate, ReleaseDateRow, StaticReleaseData, fetch_static_release_data,
    region_pref_to_db_region,
};

use std::path::Path;

use rusqlite::Connection;

use replay_control_core::arcade_board::ArcadeBoard;
use replay_control_core::error::{Error, Result};

// Re-export RC_DIR from storage (the canonical definition).
pub use crate::storage::RC_DIR;

/// Filename for the SQLite library database.
pub const LIBRARY_DB_FILE: &str = "library.db";
/// Filename for the LaunchBox XML dump.
pub const LAUNCHBOX_XML: &str = "launchbox-metadata.xml";

/// Find the LaunchBox XML on disk. Searches in priority order:
///   1. host-global cache (`/var/lib/replay-control/cache/launchbox-metadata.xml`)
///      — where `download_metadata` writes
///   2. per-storage legacy location (`<storage>/.replay-control/launchbox-metadata.xml`)
///      — pre-v2 location, kept for users who manually placed the XML there
///   3. per-storage legacy upstream filename (`<storage>/.replay-control/Metadata.xml`)
///
/// Returns the first existing path. None when no XML is present.
pub fn resolve_launchbox_xml(
    cache_dir: &Path,
    storage_rc_dir: &Path,
) -> Option<std::path::PathBuf> {
    let candidates = [
        cache_dir.join(LAUNCHBOX_XML),
        storage_rc_dir.join(LAUNCHBOX_XML),
        storage_rc_dir.join("Metadata.xml"),
    ];
    candidates.into_iter().find(|p| p.exists())
}

pub use replay_control_core::library_db::{
    CountBucket, DownloadedThumbnailStats, DriverStatusCounts, ImportProgress, ImportState,
    ImportStats, LibraryResourceLink, LibrarySummary, MetadataStats, SystemCoverage,
    SystemStatsRefreshState,
};

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
    /// Used with `hash_size_bytes` as a cache key for the hash result.
    pub hash_mtime: Option<i64>,
    /// File size observed when the CRC32 was computed.
    pub hash_size_bytes: Option<u64>,
    /// No-Intro canonical name if CRC32 matched the DAT data.
    /// NULL means either not hashed, or hashed but no match.
    pub hash_matched_name: Option<String>,
    /// Durable ROM identity phase state for resumable hash matching.
    pub identity_state: IdentityState,
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
    /// Cached normalized title (`normalize_title_for_metadata` of the canonical
    /// stem/display name). Stored at scan time so the enrichment matcher does
    /// hashmap lookups instead of normalizing per ROM. Reconciled by the
    /// `TITLE_NORM_VERSION` boot check; empty until the next scan/reconcile.
    pub normalized_title: String,
    /// Secondary normalized title — arcade clone's parent display name. Empty
    /// for console ROMs and arcade parents. Lets the matcher fall back to the
    /// parent's metadata without a separate per-ROM lookup at enrich time.
    pub normalized_title_alt: String,
    /// Curated arcade board (CPS-2, Neo Geo MVS, Taito F3, …). `None` for
    /// console ROMs, BIOS rows, and arcade ROMs whose driver sourcefile isn't
    /// in `ArcadeBoard::from_sourcefile`. Populated at scan time from
    /// `arcade_db::ArcadeGameInfo::board`.
    pub board: Option<ArcadeBoard>,
    /// RetroAchievements game id, hash-matched per dump: whole-file carts via
    /// the CRC-matched `rom_entry.ra_id`, header carts/discs via `rc_hash` →
    /// catalog `ra_hash`. Empty when the dump has no RA set. Used by the "has
    /// achievements" search filter and the game-detail RetroAchievements pill.
    pub ra_id: String,
    /// RetroAchievements `rc_hash` computed from the ROM bytes (header carts:
    /// NES/SNES/N64). Persisted so a catalog-only refresh can re-resolve `ra_id`
    /// against the new `ra_hash` table without re-reading the file. `None` for
    /// whole-file carts (they resolve via CRC) and non-hash systems.
    pub rc_hash: Option<String>,
}

/// Durable identity state for ROM hash matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityState {
    Unknown = 0,
    NotApplicable = 1,
    Pending = 2,
    Running = 3,
    CompleteMatched = 4,
    CompleteUnmatched = 5,
    Failed = 6,
}

impl IdentityState {
    pub fn as_i64(self) -> i64 {
        self as i64
    }

    pub fn from_i64(value: i64) -> Self {
        match value {
            1 => Self::NotApplicable,
            2 => Self::Pending,
            3 => Self::Running,
            4 => Self::CompleteMatched,
            5 => Self::CompleteUnmatched,
            6 => Self::Failed,
            _ => Self::Unknown,
        }
    }

    pub fn from_hash_match(matched_name: Option<&str>) -> Self {
        if matched_name.is_some() {
            Self::CompleteMatched
        } else {
            Self::CompleteUnmatched
        }
    }
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

/// A derived per-ROM game resource shown on the game-detail page.
#[derive(Debug, Clone)]
pub struct LibraryGameResource {
    pub rom_filename: String,
    pub source: String,
    pub resource_type: String,
    pub resource_id: String,
    pub url: String,
    pub title: Option<String>,
    pub languages: Option<String>,
    pub platform: Option<String>,
    pub mime_type: Option<String>,
}

/// Strip the redundant `rom_filename` field (already scoped by the caller's
/// per-ROM payload) and hand back the wire-shape view consumed by SSR.
impl From<LibraryGameResource> for LibraryResourceLink {
    fn from(r: LibraryGameResource) -> Self {
        LibraryResourceLink {
            source: r.source,
            resource_type: r.resource_type,
            resource_id: r.resource_id,
            url: r.url,
            title: r.title,
            languages: r.languages,
            platform: r.platform,
            mime_type: r.mime_type,
        }
    }
}

/// Durable thumbnail download job stored in `library.db`.
#[derive(Debug, Clone)]
pub struct ThumbnailDownloadJob {
    pub system: String,
    pub rom_filename: String,
    pub kind: crate::thumbnails::ThumbnailKind,
    pub manifest: crate::thumbnail_manifest::ManifestMatch,
}

impl ThumbnailDownloadJob {
    pub fn priority(&self) -> i64 {
        thumbnail_priority(self.kind)
    }
}

fn thumbnail_priority(kind: crate::thumbnails::ThumbnailKind) -> i64 {
    match kind {
        crate::thumbnails::ThumbnailKind::Boxart => 0,
        crate::thumbnails::ThumbnailKind::Title => 1,
        crate::thumbnails::ThumbnailKind::Snap => 2,
    }
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
    pub discovery_state: PhaseState,
    pub enrichment_state: PhaseState,
    pub thumbnail_state: ThumbnailPhaseState,
}

/// Durable per-system phase state for discovery/enrichment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhaseState {
    Unknown = 0,
    Pending = 1,
    Running = 2,
    Complete = 3,
    Failed = 4,
}

impl PhaseState {
    pub fn as_i64(self) -> i64 {
        self as i64
    }

    pub fn from_i64(value: i64) -> Self {
        match value {
            1 => Self::Pending,
            2 => Self::Running,
            3 => Self::Complete,
            4 => Self::Failed,
            _ => Self::Unknown,
        }
    }
}

/// Durable per-system thumbnail queue state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbnailPhaseState {
    Unknown = 0,
    Pending = 1,
    Queued = 2,
    Running = 3,
    Complete = 4,
    Failed = 5,
}

impl ThumbnailPhaseState {
    pub fn as_i64(self) -> i64 {
        self as i64
    }

    pub fn from_i64(value: i64) -> Self {
        match value {
            1 => Self::Pending,
            2 => Self::Queued,
            3 => Self::Running,
            4 => Self::Complete,
            5 => Self::Failed,
            _ => Self::Unknown,
        }
    }
}

/// Durable per-ROM thumbnail download job state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbnailJobState {
    Queued = 1,
    Running = 2,
    Failed = 3,
}

impl ThumbnailJobState {
    pub fn as_i64(self) -> i64 {
        self as i64
    }
}

/// Shared SQL `ORDER BY` prefix that ranks rows by release date (oldest first,
/// undated last, day-precision before month before year). Used by series,
/// recommendation, and cross-system queries; each call site appends its own
/// trailing tie-breaker columns (e.g. `display_name`).
pub(crate) const ORDER_BY_RELEASE_DATE: &str = "release_date IS NULL,
    substr(release_date, 1, 4),
    release_date,
    CASE release_precision
        WHEN 'day' THEN 0
        WHEN 'month' THEN 1
        WHEN 'year' THEN 2
        ELSE 3
    END";

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
        hash_size_bytes INTEGER,
        hash_matched_name TEXT,
        scan_token INTEGER,
        identity_state INTEGER NOT NULL DEFAULT 0,
        release_date TEXT,
        release_precision TEXT,
        release_region_used TEXT,
        cooperative INTEGER NOT NULL DEFAULT 0,
        normalized_title TEXT NOT NULL DEFAULT '',
        normalized_title_alt TEXT NOT NULL DEFAULT '',
        board TEXT NOT NULL DEFAULT '',
        ra_id TEXT NOT NULL DEFAULT '',
        rc_hash TEXT,
        PRIMARY KEY (system, rom_filename)
    );
    CREATE INDEX IF NOT EXISTS idx_gl_board ON game_library(system, board) WHERE board != '';
";

/// SQL to create the `game_library_meta` table.
const CREATE_GAME_LIBRARY_META_SQL: &str = "
    CREATE TABLE IF NOT EXISTS game_library_meta (
        system TEXT PRIMARY KEY,
        dir_mtime_secs INTEGER,
        scanned_at INTEGER NOT NULL,
        rom_count INTEGER NOT NULL DEFAULT 0,
        total_size_bytes INTEGER NOT NULL DEFAULT 0,
        discovery_state INTEGER NOT NULL DEFAULT 0,
        enrichment_state INTEGER NOT NULL DEFAULT 0,
        thumbnail_state INTEGER NOT NULL DEFAULT 0
    );
";

/// SQL to create the `game_detail_metadata` table.
///
/// Denormalizes long-form description + publisher per ROM so the
/// game-detail server fn doesn't have to acquire the host-global
/// `external_metadata_pool`. Populated at enrichment from provider rows.
const CREATE_GAME_DETAIL_METADATA_SQL: &str = "
    CREATE TABLE IF NOT EXISTS game_detail_metadata (
        system TEXT NOT NULL,
        rom_filename TEXT NOT NULL,
        description TEXT,
        publisher TEXT,
        PRIMARY KEY (system, rom_filename),
        FOREIGN KEY (system, rom_filename)
            REFERENCES game_library(system, rom_filename)
            ON DELETE CASCADE
    );
";

const GAME_DETAIL_METADATA_COLUMNS: &[&str] =
    &["system", "rom_filename", "description", "publisher"];

const CREATE_LIBRARY_GAME_RESOURCE_SQL: &str = "
    CREATE TABLE IF NOT EXISTS library_game_resource (
        system TEXT NOT NULL,
        rom_filename TEXT NOT NULL,
        source TEXT NOT NULL,
        resource_type TEXT NOT NULL,
        resource_id TEXT NOT NULL,
        url TEXT NOT NULL,
        title TEXT,
        languages TEXT,
        platform TEXT,
        mime_type TEXT,
        PRIMARY KEY (system, rom_filename, source, resource_type, resource_id),
        FOREIGN KEY (system, rom_filename)
            REFERENCES game_library(system, rom_filename)
            ON DELETE CASCADE
    );
    CREATE INDEX IF NOT EXISTS library_game_resource_idx_rom_type
        ON library_game_resource(system, rom_filename, resource_type);
";

const CREATE_GAME_DETAIL_METADATA_STAGE_SQL: &str = "
    CREATE TABLE IF NOT EXISTS game_detail_metadata_stage (
        system TEXT NOT NULL,
        stage_token INTEGER NOT NULL,
        rom_filename TEXT NOT NULL,
        description TEXT,
        publisher TEXT,
        PRIMARY KEY (system, stage_token, rom_filename)
    );
";

const CREATE_LIBRARY_GAME_RESOURCE_STAGE_SQL: &str = "
    CREATE TABLE IF NOT EXISTS library_game_resource_stage (
        system TEXT NOT NULL,
        stage_token INTEGER NOT NULL,
        rom_filename TEXT NOT NULL,
        source TEXT NOT NULL,
        resource_type TEXT NOT NULL,
        resource_id TEXT NOT NULL,
        url TEXT NOT NULL,
        title TEXT,
        languages TEXT,
        platform TEXT,
        mime_type TEXT,
        PRIMARY KEY (system, stage_token, rom_filename, source, resource_type, resource_id)
    );
";

const CREATE_LIBRARY_THUMBNAIL_JOB_SQL: &str = "
    CREATE TABLE IF NOT EXISTS library_thumbnail_job (
        system TEXT NOT NULL,
        rom_filename TEXT NOT NULL,
        kind TEXT NOT NULL,
        filename TEXT NOT NULL,
        repo_url_name TEXT NOT NULL,
        branch TEXT NOT NULL,
        is_symlink INTEGER NOT NULL DEFAULT 0,
        state INTEGER NOT NULL DEFAULT 1,
        attempts INTEGER NOT NULL DEFAULT 0,
        priority INTEGER NOT NULL DEFAULT 0,
        updated_at INTEGER NOT NULL,
        PRIMARY KEY (system, rom_filename, kind, filename),
        FOREIGN KEY (system, rom_filename)
            REFERENCES game_library(system, rom_filename)
            ON DELETE CASCADE
    );
    CREATE INDEX IF NOT EXISTS idx_library_thumbnail_job_state
        ON library_thumbnail_job(state, system);
    CREATE INDEX IF NOT EXISTS idx_library_thumbnail_job_key
        ON library_thumbnail_job(system, kind, filename);
    CREATE INDEX IF NOT EXISTS idx_library_thumbnail_job_system_priority_status
        ON library_thumbnail_job(system, priority, state);
";

const CREATE_LIBRARY_BUILD_SEQUENCE_SQL: &str = "
    CREATE TABLE IF NOT EXISTS library_build_sequence (
        name TEXT PRIMARY KEY,
        next_value INTEGER NOT NULL
    );
    INSERT OR IGNORE INTO library_build_sequence (name, next_value)
    VALUES ('scan_token', 1);
";

const CREATE_GAME_LIBRARY_SYSTEM_STATS_SQL: &str = "
    CREATE TABLE IF NOT EXISTS game_library_system_stats (
        system TEXT PRIMARY KEY,
        rom_count INTEGER NOT NULL DEFAULT 0,
        total_size_bytes INTEGER NOT NULL DEFAULT 0,
        clone_count INTEGER NOT NULL DEFAULT 0,
        hack_count INTEGER NOT NULL DEFAULT 0,
        translation_count INTEGER NOT NULL DEFAULT 0,
        homebrew_count INTEGER NOT NULL DEFAULT 0,
        unlicensed_count INTEGER NOT NULL DEFAULT 0,
        special_count INTEGER NOT NULL DEFAULT 0,
        region_counts_json TEXT,
        release_year_min INTEGER,
        release_year_max INTEGER,
        release_date_known_count INTEGER NOT NULL DEFAULT 0,
        genre_counts_json TEXT,
        genre_group_counts_json TEXT,
        developer_known_count INTEGER NOT NULL DEFAULT 0,
        publisher_known_count INTEGER NOT NULL DEFAULT 0,
        player_count_distribution_json TEXT,
        rating_known_count INTEGER NOT NULL DEFAULT 0,
        description_count INTEGER NOT NULL DEFAULT 0,
        boxart_count INTEGER NOT NULL DEFAULT 0,
        snap_count INTEGER NOT NULL DEFAULT 0,
        title_screen_count INTEGER NOT NULL DEFAULT 0,
        thumbnail_total_size_bytes INTEGER NOT NULL DEFAULT 0,
        thumbnail_file_count INTEGER NOT NULL DEFAULT 0,
        thumbnail_boxart_file_count INTEGER NOT NULL DEFAULT 0,
        thumbnail_snap_file_count INTEGER NOT NULL DEFAULT 0,
        thumbnail_title_file_count INTEGER NOT NULL DEFAULT 0,
        manual_count INTEGER NOT NULL DEFAULT 0,
        video_count INTEGER NOT NULL DEFAULT 0,
        resource_count INTEGER NOT NULL DEFAULT 0,
        coop_count INTEGER NOT NULL DEFAULT 0,
        verified_count INTEGER NOT NULL DEFAULT 0,
        driver_status_json TEXT,
        refresh_state INTEGER NOT NULL DEFAULT 0,
        updated_at INTEGER,
        ra_id_count INTEGER NOT NULL DEFAULT 0
    );
";

const LIBRARY_GAME_RESOURCE_COLUMNS: &[&str] = &[
    "system",
    "rom_filename",
    "source",
    "resource_type",
    "resource_id",
    "url",
    "title",
    "languages",
    "platform",
    "mime_type",
];

/// SQL to create the per-storage `library_meta` k/v table.
///
/// First inhabitant: `title_norm_version` (set to the current
/// `replay_control_core::title_utils::TITLE_NORM_VERSION` once a scan or
/// reconcile finishes populating the normalized columns). Free-form for
/// future per-storage knobs that don't deserve their own column.
const CREATE_LIBRARY_META_SQL: &str = "
    CREATE TABLE IF NOT EXISTS library_meta (
        key   TEXT NOT NULL PRIMARY KEY,
        value TEXT
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
    "hash_size_bytes",
    "hash_matched_name",
    "scan_token",
    "identity_state",
    "release_date",
    "release_precision",
    "release_region_used",
    "cooperative",
    "normalized_title",
    "normalized_title_alt",
    "board",
    "ra_id",
    "rc_hash",
];

/// Column set of `game_library_meta`. Must match [`CREATE_GAME_LIBRARY_META_SQL`].
/// Drives the drift-rebuild check so a meta-shape change self-heals on its own,
/// not only as a side-effect of a `game_library` rebuild.
const GAME_LIBRARY_META_COLUMNS: &[&str] = &[
    "system",
    "dir_mtime_secs",
    "scanned_at",
    "rom_count",
    "total_size_bytes",
    "discovery_state",
    "enrichment_state",
    "thumbnail_state",
];

const LIBRARY_THUMBNAIL_JOB_COLUMNS: &[&str] = &[
    "system",
    "rom_filename",
    "kind",
    "filename",
    "repo_url_name",
    "branch",
    "is_symlink",
    "state",
    "attempts",
    "priority",
    "updated_at",
];

const LIBRARY_BUILD_SEQUENCE_COLUMNS: &[&str] = &["name", "next_value"];

const GAME_LIBRARY_SYSTEM_STATS_COLUMNS: &[&str] = &[
    "system",
    "rom_count",
    "total_size_bytes",
    "clone_count",
    "hack_count",
    "translation_count",
    "homebrew_count",
    "unlicensed_count",
    "special_count",
    "region_counts_json",
    "release_year_min",
    "release_year_max",
    "release_date_known_count",
    "genre_counts_json",
    "genre_group_counts_json",
    "developer_known_count",
    "publisher_known_count",
    "player_count_distribution_json",
    "rating_known_count",
    "description_count",
    "boxart_count",
    "snap_count",
    "title_screen_count",
    "thumbnail_total_size_bytes",
    "thumbnail_file_count",
    "thumbnail_boxart_file_count",
    "thumbnail_snap_file_count",
    "thumbnail_title_file_count",
    "manual_count",
    "video_count",
    "resource_count",
    "coop_count",
    "verified_count",
    "driver_status_json",
    "refresh_state",
    "updated_at",
];

const GAME_DETAIL_METADATA_STAGE_COLUMNS: &[&str] = &[
    "system",
    "stage_token",
    "rom_filename",
    "description",
    "publisher",
];

const LIBRARY_GAME_RESOURCE_STAGE_COLUMNS: &[&str] = &[
    "system",
    "stage_token",
    "rom_filename",
    "source",
    "resource_type",
    "resource_id",
    "url",
    "title",
    "languages",
    "platform",
    "mime_type",
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
        "game_library",
        "game_library_meta",
        "game_detail_metadata",
        "game_detail_metadata_stage",
        "library_game_resource",
        "library_game_resource_stage",
        "game_library_system_stats",
        "library_thumbnail_job",
        "library_build_sequence",
        "game_release_date",
        "game_alias",
        "game_series",
        "library_meta",
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
        // No schema migrations: `game_library` and the other derived tables are
        // fully reproducible (filesystem scans + catalog + LaunchBox import), so
        // `init_tables` drops and recreates any table whose column set drifts
        // from the current shape. A schema change therefore self-heals via a
        // rebuild rather than a hand-written ALTER ladder.
        Self::init_tables(&conn)?;

        if let Err(detail) = crate::sqlite::probe_tables(&conn, Self::TABLES) {
            tracing::warn!("Library DB corrupt ({detail}), deleting and recreating");
            drop(conn);
            crate::sqlite::delete_db_files(db_path);
            let conn = crate::sqlite::open_connection(db_path, "library.db")?;
            Self::init_tables(&conn)?;
            Self::recover_running_states(&conn)?;
            Self::backfill_missing_game_library_system_stats(&conn)?;
            return Ok(conn);
        }

        Self::recover_running_states(&conn)?;
        Self::backfill_missing_game_library_system_stats(&conn)?;
        Ok(conn)
    }

    /// Back-compat shim: open under `<storage_root>/.replay-control/library.db`.
    /// Used by tests and the `library_report` CLI; production goes through
    /// `open_at` directly with the central data dir path.
    pub fn open(storage_root: &Path) -> Result<Connection> {
        let dir = storage_root.join(RC_DIR);
        std::fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
        Self::open_at(&dir.join(LIBRARY_DB_FILE))
    }

    /// Move an existing per-storage `library.db` (and its WAL/SHM/journal
    /// sidecars) from `<storage>/.replay-control/library.db` to `dest`.
    ///
    /// Mirrors [`crate::settings::SettingsStore::migrate_from_storage`]:
    /// no-op if `dest` already exists; no-op if the old file is missing;
    /// atomic rename when possible, copy + delete fallback across filesystems.
    pub fn migrate_from_storage(storage_root: &Path, dest: &Path) -> Result<()> {
        let old_dir = storage_root.join(RC_DIR);

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

    fn recover_running_states(conn: &Connection) -> Result<()> {
        conn.execute(
            "UPDATE game_library
             SET identity_state = ?1
             WHERE identity_state = ?2",
            rusqlite::params![
                IdentityState::Pending.as_i64(),
                IdentityState::Running.as_i64()
            ],
        )
        .map_err(|e| Error::Other(format!("recover running identity states: {e}")))?;
        conn.execute(
            "UPDATE game_library_meta
             SET discovery_state = ?1
             WHERE discovery_state = ?2",
            rusqlite::params![PhaseState::Pending.as_i64(), PhaseState::Running.as_i64()],
        )
        .map_err(|e| Error::Other(format!("recover running discovery states: {e}")))?;
        conn.execute(
            "UPDATE game_library_meta
             SET enrichment_state = ?1
             WHERE enrichment_state = ?2",
            rusqlite::params![PhaseState::Pending.as_i64(), PhaseState::Running.as_i64()],
        )
        .map_err(|e| Error::Other(format!("recover running enrichment states: {e}")))?;
        conn.execute(
            "UPDATE game_library_meta
             SET thumbnail_state = ?1
             WHERE thumbnail_state = ?2",
            rusqlite::params![
                ThumbnailPhaseState::Pending.as_i64(),
                ThumbnailPhaseState::Running.as_i64()
            ],
        )
        .map_err(|e| Error::Other(format!("recover running thumbnail states: {e}")))?;
        Ok(())
    }

    /// Create all tables if they don't exist.
    ///
    /// This is the **only** library schema mechanism — there are no ALTER
    /// migrations. On column-set mismatch, the rebuildable derived tables
    /// (`game_library` and friends) are dropped so `CREATE TABLE IF NOT EXISTS`
    /// recreates them at the new shape. Their content comes from filesystem
    /// scans, LaunchBox import, and build-time seed — all reproducible, so the
    /// drop is a cache flush, not data loss (favorites live on the filesystem;
    /// user data lives in separate, non-dropped tables). The next scan rebuilds
    /// the dropped tables, which re-hashes ROMs and re-enriches from the
    /// current catalog. A schema change therefore self-heals into fresh data.
    pub fn init_tables(conn: &Connection) -> Result<()> {
        if Self::table_needs_rebuild(conn, "game_library", GAME_LIBRARY_COLUMNS) {
            let _ = conn.execute_batch(
                "DROP TABLE IF EXISTS library_thumbnail_job;
                 DROP TABLE IF EXISTS game_library_system_stats;
                 DROP TABLE IF EXISTS library_game_resource;
                 DROP TABLE IF EXISTS game_detail_metadata;
                 DROP TABLE IF EXISTS game_library;
                 DROP TABLE IF EXISTS game_library_meta;",
            );
        }
        if Self::table_needs_rebuild(conn, "game_library_meta", GAME_LIBRARY_META_COLUMNS) {
            let _ = conn.execute_batch("DROP TABLE IF EXISTS game_library_meta;");
        }
        if Self::table_needs_rebuild(conn, "game_release_date", GAME_RELEASE_DATE_COLUMNS) {
            let _ = conn.execute_batch("DROP TABLE IF EXISTS game_release_date;");
        }
        if Self::table_needs_rebuild(conn, "game_detail_metadata", GAME_DETAIL_METADATA_COLUMNS) {
            let _ = conn.execute_batch("DROP TABLE IF EXISTS game_detail_metadata;");
        }
        if Self::table_needs_rebuild(conn, "library_game_resource", LIBRARY_GAME_RESOURCE_COLUMNS) {
            let _ = conn.execute_batch("DROP TABLE IF EXISTS library_game_resource;");
        }
        if Self::table_needs_rebuild(
            conn,
            "game_detail_metadata_stage",
            GAME_DETAIL_METADATA_STAGE_COLUMNS,
        ) {
            let _ = conn.execute_batch("DROP TABLE IF EXISTS game_detail_metadata_stage;");
        }
        if Self::table_needs_rebuild(
            conn,
            "library_game_resource_stage",
            LIBRARY_GAME_RESOURCE_STAGE_COLUMNS,
        ) {
            let _ = conn.execute_batch("DROP TABLE IF EXISTS library_game_resource_stage;");
        }
        if Self::table_needs_rebuild(conn, "library_thumbnail_job", LIBRARY_THUMBNAIL_JOB_COLUMNS) {
            let _ = conn.execute_batch("DROP TABLE IF EXISTS library_thumbnail_job;");
        }
        if Self::table_needs_rebuild(
            conn,
            "library_build_sequence",
            LIBRARY_BUILD_SEQUENCE_COLUMNS,
        ) {
            let _ = conn.execute_batch("DROP TABLE IF EXISTS library_build_sequence;");
        }
        if Self::table_needs_rebuild(
            conn,
            "game_library_system_stats",
            GAME_LIBRARY_SYSTEM_STATS_COLUMNS,
        ) {
            let _ = conn.execute_batch("DROP TABLE IF EXISTS game_library_system_stats;");
        }
        conn.execute_batch(CREATE_GAME_RELEASE_DATE_SQL)
            .map_err(|e| Error::Other(format!("Failed to create game_release_date: {e}")))?;
        conn.execute_batch(CREATE_GAME_DETAIL_METADATA_SQL)
            .map_err(|e| Error::Other(format!("Failed to create game_detail_metadata: {e}")))?;

        conn.execute_batch(CREATE_GAME_LIBRARY_SQL)
            .map_err(|e| Error::Other(format!("Failed to create game_library: {e}")))?;
        conn.execute_batch(CREATE_GAME_LIBRARY_META_SQL)
            .map_err(|e| Error::Other(format!("Failed to create game_library_meta: {e}")))?;
        conn.execute_batch(CREATE_LIBRARY_META_SQL)
            .map_err(|e| Error::Other(format!("Failed to create library_meta: {e}")))?;
        conn.execute_batch(CREATE_LIBRARY_GAME_RESOURCE_SQL)
            .map_err(|e| Error::Other(format!("Failed to create library_game_resource: {e}")))?;
        conn.execute_batch(CREATE_GAME_DETAIL_METADATA_STAGE_SQL)
            .map_err(|e| {
                Error::Other(format!("Failed to create game_detail_metadata_stage: {e}"))
            })?;
        conn.execute_batch(CREATE_LIBRARY_GAME_RESOURCE_STAGE_SQL)
            .map_err(|e| {
                Error::Other(format!("Failed to create library_game_resource_stage: {e}"))
            })?;
        conn.execute_batch(CREATE_LIBRARY_THUMBNAIL_JOB_SQL)
            .map_err(|e| Error::Other(format!("Failed to create library_thumbnail_job: {e}")))?;
        conn.execute_batch(CREATE_LIBRARY_BUILD_SEQUENCE_SQL)
            .map_err(|e| Error::Other(format!("Failed to create library_build_sequence: {e}")))?;
        conn.execute_batch(CREATE_GAME_LIBRARY_SYSTEM_STATS_SQL)
            .map_err(|e| {
                Error::Other(format!("Failed to create game_library_system_stats: {e}"))
            })?;
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
                CREATE INDEX IF NOT EXISTS idx_game_library_identity_pending
                  ON game_library(system, identity_state)
                  WHERE identity_state IN (2, 3, 6);

                CREATE INDEX IF NOT EXISTS idx_game_library_meta_discovery_state
                  ON game_library_meta(discovery_state)
                  WHERE discovery_state IN (1, 2, 4);

                CREATE INDEX IF NOT EXISTS idx_game_library_meta_enrichment_state
                  ON game_library_meta(enrichment_state)
                  WHERE enrichment_state IN (1, 2, 4);

                CREATE INDEX IF NOT EXISTS idx_game_library_cooperative
                  ON game_library (system, cooperative)
                  WHERE cooperative = 1;

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

    /// Returns `true` if the table needs to be dropped and recreated.
    /// Wraps `crate::sqlite::table_columns_diverge` with library-specific
    /// rebuild-intent logging.
    fn table_needs_rebuild(conn: &Connection, table: &str, expected: &[&str]) -> bool {
        if crate::sqlite::table_columns_diverge(conn, table, expected) {
            tracing::warn!("{table} schema differs from expected, rebuilding");
            true
        } else {
            false
        }
    }

    /// Helper: convert a row to GameEntry (used by multiple queries).
    ///
    /// Column order must match all SELECT statements that use this helper:
    ///   system, rom_filename, rom_path, display_name, base_title, series_key,
    ///   region, developer, genre, genre_group, rating, rating_count, players,
    ///   is_clone, is_m3u, is_translation, is_hack, is_special, box_art_url,
    ///   driver_status, size_bytes, crc32, hash_mtime, hash_size_bytes, hash_matched_name,
    ///   identity_state, release_date, release_precision, release_region_used, cooperative,
    ///   normalized_title, normalized_title_alt, board, ra_id
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
            hash_size_bytes: row
                .get::<_, Option<i64>>(23)
                .unwrap_or_default()
                .map(|s| s as u64),
            hash_matched_name: row.get(24).unwrap_or_default(),
            identity_state: IdentityState::from_i64(row.get::<_, i64>(25).unwrap_or_default()),
            release_date: row.get::<_, Option<String>>(26).unwrap_or_default(),
            release_precision: row
                .get::<_, Option<DpSql>>(27)
                .unwrap_or_default()
                .map(|DpSql(d)| d),
            release_region_used: row.get::<_, Option<String>>(28).unwrap_or_default(),
            cooperative: row.get::<_, bool>(29).unwrap_or_default(),
            normalized_title: row.get::<_, String>(30).unwrap_or_default(),
            normalized_title_alt: row.get::<_, String>(31).unwrap_or_default(),
            board: row
                .get::<_, Option<String>>(32)
                .unwrap_or_default()
                .and_then(|tag| ArcadeBoard::from_tag(&tag)),
            ra_id: row.get::<_, String>(33).unwrap_or_default(),
            rc_hash: row.get::<_, Option<String>>(34).unwrap_or_default(),
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

    /// Open a library DB connection backed by a temp directory.
    /// Returns a mutable `Connection` so tests can call both read and write methods.
    pub(crate) fn open_temp_db() -> (Connection, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let conn = LibraryDb::open(dir.path()).unwrap();
        (conn, dir)
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
            hash_size_bytes: None,
            hash_matched_name: None,
            identity_state: IdentityState::Unknown,
            series_key: String::new(),
            developer: String::new(),
            release_date: None,
            release_precision: None,
            release_region_used: None,
            cooperative: false,
            normalized_title: String::new(),
            normalized_title_alt: String::new(),
            board: None,
            ra_id: String::new(),
            rc_hash: None,
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
    fn open_recovers_interrupted_running_identity_rows() {
        let (mut conn, dir) = open_temp_db();
        let mut running = make_game_entry("nintendo_snes", "Running.sfc", false);
        running.identity_state = IdentityState::Running;
        let mut complete = make_game_entry("nintendo_snes", "Complete.sfc", false);
        complete.identity_state = IdentityState::CompleteMatched;
        LibraryDb::save_system_entries(&mut conn, "nintendo_snes", &[running, complete], None)
            .unwrap();
        drop(conn);

        let conn = LibraryDb::open(dir.path()).unwrap();
        let roms = LibraryDb::load_system_entries(&conn, "nintendo_snes").unwrap();
        let running = roms
            .iter()
            .find(|rom| rom.rom_filename == "Running.sfc")
            .unwrap();
        let complete = roms
            .iter()
            .find(|rom| rom.rom_filename == "Complete.sfc")
            .unwrap();
        assert_eq!(running.identity_state, IdentityState::Pending);
        assert_eq!(complete.identity_state, IdentityState::CompleteMatched);
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
    fn cross_system_availability_orders_by_release_date_before_system_kind() {
        let (mut conn, _dir) = open_temp_db();

        let mut current = make_game_entry("nintendo_nes", "Game.nes", false);
        current.base_title = "game".into();

        let mut later_arcade = make_game_entry("arcade_fbneo", "zgame.zip", false);
        later_arcade.base_title = "game".into();
        later_arcade.display_name = Some("Z Game".into());
        later_arcade.release_date = Some("1991".into());
        later_arcade.release_precision = Some(DatePrecision::Year);

        let mut earlier_console = make_game_entry("nintendo_snes", "A Game.sfc", false);
        earlier_console.base_title = "game".into();
        earlier_console.display_name = Some("A Game".into());
        earlier_console.release_date = Some("1984".into());
        earlier_console.release_precision = Some(DatePrecision::Year);

        LibraryDb::save_system_entries(&mut conn, "nintendo_nes", &[current], None).unwrap();
        LibraryDb::save_system_entries(&mut conn, "arcade_fbneo", &[later_arcade], None).unwrap();
        LibraryDb::save_system_entries(&mut conn, "nintendo_snes", &[earlier_console], None)
            .unwrap();

        let results =
            LibraryDb::cross_system_availability(&conn, "nintendo_nes", "game", "usa", 10).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].system, "nintendo_snes");
        assert_eq!(results[1].system, "arcade_fbneo");
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
