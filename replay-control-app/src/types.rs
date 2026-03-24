/// Mirror types used on the client (hydrate) side.
/// These match the replay-core types that server functions serialize.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrganizeCriteria {
    System,
    Genre,
    Players,
    Rating,
    Alphabetical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemSummary {
    pub folder_name: String,
    pub display_name: String,
    pub manufacturer: String,
    pub category: String,
    pub game_count: usize,
    pub total_size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameRef {
    pub system: String,
    pub system_display: String,
    pub rom_filename: String,
    pub display_name: Option<String>,
    pub rom_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RomEntry {
    #[serde(flatten)]
    pub game: GameRef,
    pub size_bytes: u64,
    pub is_m3u: bool,
    #[serde(default)]
    pub is_favorite: bool,
    #[serde(default)]
    pub box_art_url: Option<String>,
    #[serde(default)]
    pub driver_status: Option<String>,
    #[serde(default)]
    pub rating: Option<f32>,
    #[serde(default)]
    pub players: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Favorite {
    #[serde(flatten)]
    pub game: GameRef,
    pub marker_filename: String,
    pub subfolder: String,
    pub date_added: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentEntry {
    #[serde(flatten)]
    pub game: GameRef,
    pub marker_filename: String,
    pub last_played: u64,
}

/// Mirror of `replay_control_core::metadata_db::ImportStats` for WASM.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ImportStats {
    pub total_source: usize,
    pub matched: usize,
    pub inserted: usize,
    pub skipped: usize,
}

/// Mirror of `replay_control_core::metadata_db::MetadataStats` for WASM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataStats {
    pub total_entries: usize,
    pub with_description: usize,
    pub with_rating: usize,
    pub db_size_bytes: u64,
    pub last_updated_text: String,
}

/// Mirror of `replay_control_core::metadata_db::ImportState` for WASM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImportState {
    Downloading,
    BuildingIndex,
    Parsing,
    Complete,
    Failed,
}

/// Mirror of `replay_control_core::metadata_db::ImportProgress` for WASM.
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

/// Per-system metadata coverage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemCoverage {
    pub system: String,
    pub display_name: String,
    pub total_games: usize,
    pub with_metadata: usize,
    pub with_thumbnail: usize,
}

/// Mirror of `replay_control_core::game_docs::GameDocument` for WASM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameDocument {
    pub relative_path: String,
    pub label: String,
    pub extension: String,
    pub size_bytes: u64,
    pub category: DocumentCategory,
}

/// Mirror of `replay_control_core::game_docs::DocumentCategory` for WASM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum DocumentCategory {
    Manual,
    Walkthrough,
    Reference,
    Extra,
}

/// Mirror of `replay_control_core::retrokit_manuals::ManualRecommendation` for WASM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManualRecommendation {
    pub source: String,
    pub title: String,
    pub url: String,
    pub size_bytes: Option<u64>,
    pub language: Option<String>,
    pub source_id: String,
}

/// Mirror of `replay_control_core::user_data_db::VideoEntry` for WASM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoEntry {
    pub id: String,
    pub url: String,
    pub platform: String,
    pub video_id: String,
    pub title: Option<String>,
    pub added_at: u64,
    pub from_recommendation: bool,
    pub tag: Option<String>,
    pub rom_filename: String,
}

// ── Activity types (WASM mirrors) ──────────────────────────────────

/// Client-side mirror of the unified Activity enum.
/// Matches the server's `api::activity::Activity` but without `CancellationToken`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Activity {
    Idle,
    Startup { phase: StartupPhase, system: String },
    Import { progress: ImportProgress },
    ThumbnailUpdate { progress: ThumbnailProgress },
    Rebuild { progress: RebuildProgress },
    Maintenance { kind: MaintenanceKind },
}

impl Activity {
    /// Check if this activity represents a terminal (completed/failed/cancelled) state.
    pub fn is_terminal(&self) -> bool {
        match self {
            Self::Import { progress } => {
                matches!(progress.state, ImportState::Complete | ImportState::Failed)
            }
            Self::ThumbnailUpdate { progress } => matches!(
                progress.phase,
                ThumbnailPhase::Complete | ThumbnailPhase::Failed | ThumbnailPhase::Cancelled
            ),
            Self::Rebuild { progress } => matches!(
                progress.phase,
                RebuildPhase::Complete | RebuildPhase::Failed
            ),
            _ => false,
        }
    }

    /// Extract a human-readable terminal message from a completed activity.
    pub fn terminal_message(&self) -> String {
        match self {
            Self::Import { progress } => match progress.state {
                ImportState::Complete => format!(
                    "Import complete: {} matched, {} inserted ({}s)",
                    progress.matched, progress.inserted, progress.elapsed_secs,
                ),
                ImportState::Failed => format!(
                    "Import failed: {}",
                    progress.error.as_deref().unwrap_or("unknown error"),
                ),
                _ => String::new(),
            },
            Self::ThumbnailUpdate { progress } => match progress.phase {
                ThumbnailPhase::Complete => format!(
                    "Complete: {} indexed, {} downloaded ({}s)",
                    progress.entries_indexed, progress.downloaded, progress.elapsed_secs,
                ),
                ThumbnailPhase::Cancelled => format!(
                    "Cancelled after {}s ({} downloaded)",
                    progress.elapsed_secs, progress.downloaded,
                ),
                ThumbnailPhase::Failed => format!(
                    "Failed: {}",
                    progress.error.as_deref().unwrap_or("unknown error"),
                ),
                _ => String::new(),
            },
            Self::Rebuild { progress } => match progress.phase {
                RebuildPhase::Complete => format!("Rebuild complete ({}s)", progress.elapsed_secs,),
                RebuildPhase::Failed => format!(
                    "Rebuild failed: {}",
                    progress.error.as_deref().unwrap_or("unknown error"),
                ),
                _ => String::new(),
            },
            _ => String::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StartupPhase {
    Scanning,
    RebuildingIndex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaintenanceKind {
    ClearMetadata,
    ClearImages,
    ClearThumbnailIndex,
    CleanupOrphans,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThumbnailPhase {
    Indexing,
    Downloading,
    Complete,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThumbnailProgress {
    pub phase: ThumbnailPhase,
    pub current_label: String,
    pub step_done: usize,
    pub step_total: usize,
    pub downloaded: usize,
    pub entries_indexed: usize,
    pub elapsed_secs: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RebuildPhase {
    Scanning,
    Enriching,
    Complete,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebuildProgress {
    pub phase: RebuildPhase,
    pub current_system: String,
    pub systems_done: usize,
    pub systems_total: usize,
    pub elapsed_secs: u64,
    pub error: Option<String>,
}
