use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

// Re-export progress types used in Activity variants.
pub use replay_control_core::metadata_db::ImportProgress;

/// What the server is doing right now. At most one activity at a time.
/// Serialized over SSE as tagged JSON for the client to consume.
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Activity {
    /// No activity running. All buttons enabled.
    Idle,

    /// Startup pipeline (Phases 2+3: cache verify/populate + thumbnail index rebuild).
    /// Phase 1 (auto-import) uses the Import variant instead.
    Startup { phase: StartupPhase, system: String },

    /// Metadata import (LaunchBox XML parse or download + parse).
    Import { progress: ImportProgress },

    /// Thumbnail update (index refresh + image download).
    /// The `cancel` token enables cooperative cancellation -- the blocking loop
    /// checks it between systems. Only this variant carries a cancel token.
    ThumbnailUpdate {
        progress: ThumbnailProgress,
        #[serde(skip, default = "default_cancel")]
        cancel: Arc<AtomicBool>,
    },

    /// Game library rebuild (invalidate + rescan + enrich).
    Rebuild { progress: RebuildProgress },

    /// A short DB/filesystem operation (clear, cleanup) that still requires
    /// exclusive access. No detailed progress -- just a kind discriminant.
    Maintenance { kind: MaintenanceKind },
}

fn default_cancel() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

impl Activity {
    /// Check if this activity represents a terminal (completed/failed/cancelled) state.
    pub fn is_terminal(&self) -> bool {
        use replay_control_core::metadata_db::ImportState;
        match self {
            Self::Import { progress } => {
                matches!(progress.state, ImportState::Complete | ImportState::Failed)
            }
            Self::ThumbnailUpdate { progress, .. } => matches!(
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
    /// Returns an empty string if the activity is not in a terminal state.
    pub fn terminal_message(&self) -> String {
        use replay_control_core::metadata_db::ImportState;
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
            Self::ThumbnailUpdate { progress, .. } => match progress.phase {
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
    /// Scanning ROM directories, populating game library.
    Scanning,
    /// Rebuilding thumbnail index from disk.
    RebuildingIndex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaintenanceKind {
    ClearMetadata,
    ClearImages,
    ClearThumbnailIndex,
    CleanupOrphans,
}

/// Phase of the thumbnail pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThumbnailPhase {
    /// Fetching file listings from GitHub API.
    Indexing,
    /// Downloading images from raw.githubusercontent.com.
    Downloading,
    Complete,
    Failed,
    Cancelled,
}

/// Progress for the two-phase thumbnail pipeline (index + download).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThumbnailProgress {
    pub phase: ThumbnailPhase,
    /// Display name of the current repo/system being processed.
    pub current_label: String,
    /// For index phase: repos done. For download phase: ROMs processed.
    pub step_done: usize,
    /// For index phase: total repos. For download phase: total ROMs.
    pub step_total: usize,
    /// Running count of images downloaded (download phase).
    pub downloaded: usize,
    /// Running count of index entries (index phase).
    pub entries_indexed: usize,
    pub elapsed_secs: u64,
    pub error: Option<String>,
}

/// Phase of the game library rebuild operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RebuildPhase {
    /// Scanning ROM directories and populating game library.
    Scanning,
    /// Enriching game entries with metadata, box art URLs, ratings.
    Enriching,
    /// Rebuild completed successfully.
    Complete,
    /// Rebuild failed.
    Failed,
}

/// Progress for the game library rebuild operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RebuildProgress {
    pub phase: RebuildPhase,
    pub current_system: String,
    pub systems_done: usize,
    pub systems_total: usize,
    pub elapsed_secs: u64,
    pub error: Option<String>,
}

/// RAII guard that resets activity to Idle on drop.
/// Panic-safe: if the operation panics, the guard still cleans up.
pub struct ActivityGuard {
    state: Arc<RwLock<Activity>>,
}

impl ActivityGuard {
    /// Update the activity in-place through the guard.
    pub fn update<F: FnOnce(&mut Activity)>(&self, f: F) {
        let mut guard = self.state.write().expect("activity lock");
        f(&mut guard);
    }
}

impl Drop for ActivityGuard {
    fn drop(&mut self) {
        *self.state.write().expect("activity lock") = Activity::Idle;
    }
}

#[cfg(test)]
impl ActivityGuard {
    /// Create a guard for testing (not through try_start_activity).
    pub fn new_for_test(state: Arc<RwLock<Activity>>) -> Self {
        Self { state }
    }
}

/// Methods on the activity state for AppState integration.
impl super::AppState {
    /// Try to start a new activity. Returns Err if another activity is active.
    pub fn try_start_activity(&self, initial: Activity) -> Result<ActivityGuard, &'static str> {
        let mut state = self.activity.write().expect("activity lock");
        if !matches!(*state, Activity::Idle) {
            return Err("Another operation is already running");
        }
        *state = initial;
        Ok(ActivityGuard {
            state: self.activity.clone(),
        })
    }

    /// Read current activity (for SSE, server fns, banner).
    pub fn activity(&self) -> Activity {
        self.activity.read().expect("activity lock").clone()
    }

    /// Update the activity in-place.
    pub fn update_activity<F: FnOnce(&mut Activity)>(&self, f: F) {
        let mut guard = self.activity.write().expect("activity lock");
        f(&mut guard);
    }

    /// Check if idle (replaces is_busy -- inverted sense).
    pub fn is_idle(&self) -> bool {
        matches!(
            *self.activity.read().expect("activity lock"),
            Activity::Idle
        )
    }

    /// Check if startup scanning is active (replaces is_scanning).
    /// Used by GameLibrary::get_roms() to suppress L3 scans.
    pub fn is_startup_scanning(&self) -> bool {
        matches!(
            *self.activity.read().expect("activity lock"),
            Activity::Startup {
                phase: StartupPhase::Scanning,
                ..
            }
        )
    }

    /// Request cancellation of the current activity, if it supports it.
    /// Returns true if cancellation was requested, false if the current
    /// activity does not support cancellation.
    pub fn request_cancel(&self) -> bool {
        let state = self.activity.read().expect("activity lock");
        match &*state {
            Activity::ThumbnailUpdate { cancel, .. } => {
                cancel.store(true, Ordering::Relaxed);
                true
            }
            _ => false,
        }
    }
}
