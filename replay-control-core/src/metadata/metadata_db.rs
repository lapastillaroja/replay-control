//! Wire types for metadata DB progress, stats, and coverage reports.
//! Native SQL operations live in `replay_control_core_server::metadata_db`.

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

/// Cache-level stats about the metadata DB as a whole.
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
    pub with_genre: usize,
    #[serde(default)]
    pub with_developer: usize,
    #[serde(default)]
    pub with_rating: usize,
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
    pub special_count: usize,
    #[serde(default)]
    pub coop_count: usize,
    #[serde(default)]
    pub verified_count: usize,
    #[serde(default)]
    pub min_year: Option<u16>,
    #[serde(default)]
    pub max_year: Option<u16>,
    /// Driver status counts for arcade systems. `None` for non-arcade systems.
    #[serde(default)]
    pub driver_status: Option<DriverStatusCounts>,
}

/// Per-system counts for arcade `driver_status` values.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DriverStatusCounts {
    pub working: usize,
    pub imperfect: usize,
    pub preliminary: usize,
    pub unknown: usize,
}

/// Aggregate summary of the entire game library.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LibrarySummary {
    pub total_games: usize,
    pub system_count: usize,
    pub with_genre: usize,
    pub with_developer: usize,
    pub with_rating: usize,
    pub with_box_art: usize,
    pub coop_games: usize,
    pub min_year: Option<u16>,
    pub max_year: Option<u16>,
    pub total_size_bytes: u64,
}
