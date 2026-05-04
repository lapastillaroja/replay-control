//! App-side progress/phase types used by the Activity SSE stream. These are
//! consumed by both SSR (via `api::activity`) and hydrate (deserialized from
//! Activity SSE events) — the SSR-side `api::activity::Activity` constructs
//! values using these types.
//!
//! Wire types that originally lived here as mirrors of `replay-control-core`
//! types have been removed — those types now live in `replay-control-core`
//! directly and are imported unconditionally by consumers.

use serde::{Deserialize, Serialize};

pub use replay_control_core::library_db::{ImportProgress, ImportState};

// ── Activity types ──────────────────────────────────

/// Client-side mirror of the unified Activity enum.
/// Matches the server's `api::activity::Activity` but without `CancellationToken`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Activity {
    Idle,
    Startup { phase: StartupPhase, system: String },
    Import { progress: ImportProgress },
    RefreshExternalMetadata { progress: RefreshMetadataProgress },
    ThumbnailUpdate { progress: ThumbnailProgress },
    Rebuild { progress: RebuildProgress },
    Maintenance { kind: MaintenanceKind },
    Update { progress: UpdateProgress },
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
            Self::Update { progress } => {
                matches!(progress.phase, UpdatePhase::Complete | UpdatePhase::Failed)
            }
            Self::RefreshExternalMetadata { progress } => matches!(
                progress.phase,
                RefreshMetadataPhase::Complete
                    | RefreshMetadataPhase::Failed
                    | RefreshMetadataPhase::UpToDate
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
            Self::Rebuild { progress } => {
                let label = if progress.is_rescan {
                    "Rescan"
                } else {
                    "Rebuild"
                };
                match progress.phase {
                    RebuildPhase::Complete => {
                        format!("{label} complete ({}s)", progress.elapsed_secs)
                    }
                    RebuildPhase::Failed => format!(
                        "{label} failed: {}",
                        progress.error.as_deref().unwrap_or("unknown error"),
                    ),
                    _ => String::new(),
                }
            }
            Self::Update { progress } => match progress.phase {
                UpdatePhase::Complete => format!("Update complete ({}s)", progress.elapsed_secs),
                UpdatePhase::Failed => format!(
                    "Update failed: {}",
                    progress.error.as_deref().unwrap_or("unknown error"),
                ),
                _ => String::new(),
            },
            Self::RefreshExternalMetadata { progress } => match progress.phase {
                RefreshMetadataPhase::Complete => {
                    if progress.source_entries > 0 {
                        format!(
                            "Metadata refresh complete ({}s, {} source entries)",
                            progress.elapsed_secs, progress.source_entries
                        )
                    } else {
                        format!("Metadata refresh complete ({}s)", progress.elapsed_secs)
                    }
                }
                RefreshMetadataPhase::Failed => format!(
                    "Metadata refresh failed: {}",
                    progress.error.as_deref().unwrap_or("unknown error"),
                ),
                RefreshMetadataPhase::UpToDate => "Metadata already up to date".to_string(),
                _ => String::new(),
            },
            _ => String::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RefreshMetadataPhase {
    Checking,
    Downloading,
    Parsing,
    Enriching,
    Complete,
    Failed,
    UpToDate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshMetadataProgress {
    pub phase: RefreshMetadataPhase,
    pub source_entries: usize,
    pub downloaded_bytes: u64,
    pub elapsed_secs: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StartupPhase {
    FetchingMetadata,
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
    #[serde(default)]
    pub is_rescan: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpdatePhase {
    Downloading,
    Installing,
    Restarting,
    Complete,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateProgress {
    pub phase: UpdatePhase,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub phase_detail: String,
    pub elapsed_secs: u64,
    pub error: Option<String>,
}
