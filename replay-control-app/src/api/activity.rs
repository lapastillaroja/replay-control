use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

// Re-export progress types used in Activity variants.
pub use replay_control_core_server::library_db::ImportProgress;

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

    /// Refresh of the host-global `external_metadata.db` (LaunchBox XML
    /// parse, optionally preceded by a download). Single-flight: a second
    /// caller while one is in flight gets `ActivityInFlight`.
    RefreshExternalMetadata { progress: RefreshMetadataProgress },

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

    /// Software update (download + install). No cancel token — the 5-minute
    /// timeout is the only abort mechanism.
    Update { progress: UpdateProgress },
}

fn default_cancel() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

impl Activity {
    /// Check if this activity represents a terminal (completed/failed/cancelled) state.
    pub fn is_terminal(&self) -> bool {
        use replay_control_core_server::library_db::ImportState;
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
    /// Returns an empty string if the activity is not in a terminal state.
    pub fn terminal_message(&self) -> String {
        use replay_control_core_server::library_db::ImportState;
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
pub enum StartupPhase {
    /// Downloading LaunchBox XML and libretro thumbnail manifest on first boot.
    FetchingMetadata,
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

/// Phase of the external_metadata refresh path (download → parse).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RefreshMetadataPhase {
    /// Hashing the on-disk XML before deciding whether a refresh is needed.
    Checking,
    /// Downloading `Metadata.zip` from upstream.
    Downloading,
    /// Streaming the XML into `external_metadata.db`.
    Parsing,
    /// Re-running enrichment so launchbox data flows into game_library /
    /// game_description.
    Enriching,
    Complete,
    Failed,
    /// ETag matched — upstream file is unchanged; no download or parse needed.
    UpToDate,
}

/// Progress for the external_metadata refresh path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefreshMetadataProgress {
    pub phase: RefreshMetadataPhase,
    /// Source-entry counter during `Parsing`; otherwise 0.
    pub source_entries: usize,
    /// Total bytes downloaded during `Downloading`; otherwise 0.
    pub downloaded_bytes: u64,
    /// Total bytes expected (from Content-Length), if known.
    pub total_bytes: Option<u64>,
    pub elapsed_secs: u64,
    pub error: Option<String>,
}

impl RefreshMetadataProgress {
    pub fn initial() -> Self {
        Self {
            phase: RefreshMetadataPhase::Checking,
            source_entries: 0,
            downloaded_bytes: 0,
            total_bytes: None,
            elapsed_secs: 0,
            error: None,
        }
    }
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
///
/// `is_rescan` distinguishes the destructive rebuild (false) from the additive
/// rescan (true). Both share this struct because their progress shape is
/// identical; only the user-visible copy differs.
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

impl RebuildProgress {
    /// Initial progress at the moment a rebuild or rescan starts.
    pub fn initial(is_rescan: bool) -> Self {
        Self {
            phase: RebuildPhase::Scanning,
            current_system: String::new(),
            systems_done: 0,
            systems_total: 0,
            elapsed_secs: 0,
            error: None,
            is_rescan,
        }
    }
}

/// Phase of the software update operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpdatePhase {
    Downloading,
    Installing,
    Restarting,
    Complete,
    Failed,
}

/// Progress for the software update operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpdateProgress {
    pub phase: UpdatePhase,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub phase_detail: String,
    pub elapsed_secs: u64,
    pub error: Option<String>,
}

/// RAII guard that resets activity to Idle on drop.
/// Panic-safe: if the operation panics, the guard still cleans up.
pub struct ActivityGuard {
    state: Arc<RwLock<Activity>>,
    activity_tx: tokio::sync::broadcast::Sender<Activity>,
}

impl ActivityGuard {
    /// Update the activity in-place through the guard and broadcast the change.
    pub fn update<F: FnOnce(&mut Activity)>(&self, f: F) {
        let mut guard = self.state.write().expect("activity lock");
        f(&mut guard);
        let activity = guard.clone();
        drop(guard);
        let _ = self.activity_tx.send(activity);
    }
}

impl Drop for ActivityGuard {
    fn drop(&mut self) {
        *self.state.write().expect("activity lock") = Activity::Idle;
        let _ = self.activity_tx.send(Activity::Idle);
    }
}

#[cfg(test)]
impl ActivityGuard {
    /// Create a guard for testing (not through try_start_activity).
    pub fn new_for_test(state: Arc<RwLock<Activity>>) -> Self {
        let (tx, _) = tokio::sync::broadcast::channel(1);
        Self {
            state,
            activity_tx: tx,
        }
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
        let activity = state.clone();
        drop(state);
        let _ = self.activity_tx.send(activity);
        Ok(ActivityGuard {
            state: self.activity.clone(),
            activity_tx: self.activity_tx.clone(),
        })
    }

    /// Read current activity (for SSE, server fns, banner).
    pub fn activity(&self) -> Activity {
        self.activity.read().expect("activity lock").clone()
    }

    /// Broadcast the current activity state to all SSE listeners.
    pub fn broadcast_activity(&self) {
        let activity = self.activity();
        let _ = self.activity_tx.send(activity);
    }

    /// Update the activity in-place and broadcast the change.
    pub fn update_activity<F: FnOnce(&mut Activity)>(&self, f: F) {
        let mut guard = self.activity.write().expect("activity lock");
        f(&mut guard);
        let activity = guard.clone();
        drop(guard);
        let _ = self.activity_tx.send(activity);
    }

    /// Check if idle (replaces is_busy -- inverted sense).
    pub fn is_idle(&self) -> bool {
        matches!(
            *self.activity.read().expect("activity lock"),
            Activity::Idle
        )
    }

    /// Check if startup scanning is active (replaces is_scanning).
    /// Used by LibraryService::get_roms() to suppress L3 scans.
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// ActivityGuard is cleared on drop.
    /// This validates the guard pattern used in BackgroundManager::run_pipeline.
    #[test]
    fn activity_guard_resets_to_idle_on_drop() {
        let activity = Arc::new(RwLock::new(Activity::Idle));

        {
            // Simulate what run_pipeline does: set startup, then drop guard.
            *activity.write().unwrap() = Activity::Startup {
                phase: StartupPhase::Scanning,
                system: String::new(),
            };
            assert!(matches!(
                *activity.read().unwrap(),
                Activity::Startup { .. }
            ));

            let _guard = ActivityGuard::new_for_test(activity.clone());
            // Guard is alive — activity should still be Startup.
            assert!(matches!(
                *activity.read().unwrap(),
                Activity::Startup { .. }
            ));
        }
        // Guard dropped — activity should be Idle.
        assert!(matches!(*activity.read().unwrap(), Activity::Idle));
    }
}
