//! Wire types for library DB progress, stats, and coverage reports.
//! Native SQL operations live in `replay_control_core_server::library_db`.

use serde::{Deserialize, Serialize};

/// Aggregate counts from a single metadata import run.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ImportStats {
    pub total_source: usize,
    pub matched: usize,
    pub inserted: usize,
    pub skipped: usize,
}

/// High-level state of an ongoing metadata import.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImportState {
    Downloading,
    BuildingIndex,
    Parsing,
    Complete,
    Failed,
}

/// Progress snapshot emitted during metadata import.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

/// Cache-level stats about the library DB as a whole.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MetadataStats {
    pub total_entries: usize,
    pub with_description: usize,
    pub with_rating: usize,
    pub db_size_bytes: u64,
    pub last_updated_text: String,
}

/// Per-system metadata coverage report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemCoverage {
    pub system: String,
    pub display_name: String,
    pub total_games: usize,
    pub with_thumbnail: usize,
    #[serde(default)]
    pub with_snap: usize,
    #[serde(default)]
    pub with_title_screen: usize,
    #[serde(default)]
    pub with_manual: usize,
    #[serde(default)]
    pub with_video: usize,
    #[serde(default)]
    pub with_resource: usize,
    #[serde(default)]
    pub with_genre: usize,
    #[serde(default)]
    pub with_developer: usize,
    #[serde(default)]
    pub with_publisher: usize,
    #[serde(default)]
    pub with_rating: usize,
    #[serde(default)]
    pub with_release_date: usize,
    #[serde(default)]
    pub size_bytes: u64,
    #[serde(default)]
    pub with_description: usize,
    #[serde(default)]
    pub clone_count: usize,
    #[serde(default)]
    pub hack_count: usize,
    #[serde(default)]
    pub translation_count: usize,
    #[serde(default)]
    pub homebrew_count: usize,
    #[serde(default)]
    pub unlicensed_count: usize,
    #[serde(default)]
    pub special_count: usize,
    #[serde(default)]
    pub coop_count: usize,
    #[serde(default)]
    pub verified_count: usize,
    /// ROMs carrying a RetroAchievements id (`ra_id != ''`) — hash-matched, so
    /// every one is a precise link, not a title guess.
    #[serde(default)]
    pub with_ra_id: usize,
    #[serde(default)]
    pub min_year: Option<u16>,
    #[serde(default)]
    pub max_year: Option<u16>,
    /// Driver status counts for arcade systems. `None` for non-arcade systems.
    #[serde(default)]
    pub driver_status: Option<DriverStatusCounts>,
    #[serde(default)]
    pub downloaded_thumbnail_files: usize,
    #[serde(default)]
    pub downloaded_boxart_files: usize,
    #[serde(default)]
    pub downloaded_snap_files: usize,
    #[serde(default)]
    pub downloaded_title_files: usize,
    #[serde(default)]
    pub downloaded_thumbnail_bytes: u64,
    #[serde(default)]
    pub stats_refresh_state: SystemStatsRefreshState,
    #[serde(default)]
    pub stats_updated_at: Option<i64>,
    #[serde(default)]
    pub region_counts: Vec<CountBucket>,
    #[serde(default)]
    pub genre_group_counts: Vec<CountBucket>,
    #[serde(default)]
    pub player_count_distribution: Vec<CountBucket>,
}

/// A named count used by small metadata-page distributions.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CountBucket {
    pub label: String,
    pub count: usize,
}

/// Per-system counts for arcade `driver_status` values.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DriverStatusCounts {
    pub working: usize,
    pub imperfect: usize,
    pub preliminary: usize,
    pub unknown: usize,
}

/// Downloaded thumbnail media totals.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct DownloadedThumbnailStats {
    pub total_files: usize,
    pub boxart_files: usize,
    pub snap_files: usize,
    pub title_files: usize,
    pub total_size_bytes: u64,
}

/// Refresh state for materialized per-system library stats.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SystemStatsRefreshState {
    #[default]
    Unknown,
    Fresh,
    Stale,
    Refreshing,
    Failed,
}

/// Aggregate summary of the entire game library.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LibrarySummary {
    pub total_games: usize,
    pub system_count: usize,
    pub with_genre: usize,
    pub with_developer: usize,
    pub with_publisher: usize,
    pub with_rating: usize,
    pub with_release_date: usize,
    pub with_box_art: usize,
    pub with_snap: usize,
    pub with_title_screen: usize,
    pub with_manual: usize,
    pub with_video: usize,
    pub with_resource: usize,
    pub coop_games: usize,
    pub min_year: Option<u16>,
    pub max_year: Option<u16>,
    pub total_size_bytes: u64,
    pub downloaded_thumbnail_files: usize,
    pub downloaded_boxart_files: usize,
    pub downloaded_snap_files: usize,
    pub downloaded_title_files: usize,
    pub downloaded_thumbnail_bytes: u64,
}

/// Wire-shape mirror of `library_game_resource` rows — the per-ROM payload
/// of `get_rom_detail`. `rom_filename` is dropped from the wire because
/// rows are already scoped to a single ROM. UI code partitions by
/// `resource_type` (manual / video / strategy_guide / …) and `source`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryResourceLink {
    pub source: String,
    pub resource_type: String,
    pub resource_id: String,
    pub url: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub languages: Option<String>,
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
}
