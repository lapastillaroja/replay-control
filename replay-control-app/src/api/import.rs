use std::path::PathBuf;
use std::sync::{RwLock, RwLockWriteGuard};

use replay_control_core_server::library_db::LibraryDb;

use super::AppState;
use super::activity::{Activity, ActivityGuard};

/// Acquire a write lock, panicking on poison with a standard message.
fn write_lock<'a, T>(lock: &'a RwLock<T>, label: &str) -> RwLockWriteGuard<'a, T> {
    lock.write()
        .unwrap_or_else(|e| panic!("{label} write lock poisoned: {e}"))
}

// ── ImportPipeline ─────────────────────────────────────────────────

/// Manages metadata imports (LaunchBox XML → library DB).
///
/// No longer owns its own busy flag or progress -- those live in
/// `AppState.activity` as `Activity::Import { progress }`.
#[derive(Default)]
pub struct ImportPipeline;

impl ImportPipeline {
    pub fn new() -> Self {
        Self
    }

    /// Check if an import is actively running by inspecting the activity.
    /// Used by the startup pipeline to wait for auto-import completion.
    pub fn has_active_import(state: &AppState) -> bool {
        use replay_control_core_server::library_db::ImportState;
        matches!(
            state.activity(),
            Activity::Import {
                progress: replay_control_core_server::library_db::ImportProgress {
                    state: ImportState::Downloading
                        | ImportState::BuildingIndex
                        | ImportState::Parsing,
                    ..
                },
            }
        )
    }

    /// Start a background metadata import from a LaunchBox XML file.
    /// Returns `false` if another metadata operation is already running.
    pub fn start_import(&self, xml_path: PathBuf, state: AppState) -> bool {
        self.start_import_inner(xml_path, state, false)
    }

    /// Start import without post-enrichment. Used by the startup pipeline
    /// which handles populate/enrichment sequentially.
    pub fn start_import_no_enrich(&self, xml_path: PathBuf, state: AppState) -> bool {
        self.start_import_inner(xml_path, state, true)
    }

    fn start_import_inner(
        &self,
        xml_path: PathBuf,
        state: AppState,
        skip_enrichment: bool,
    ) -> bool {
        use replay_control_core_server::library_db::{ImportProgress, ImportState};

        // Atomically claim the operation slot via Activity.
        let guard = match state.try_start_activity(Activity::Import {
            progress: ImportProgress {
                state: ImportState::BuildingIndex,
                processed: 0,
                matched: 0,
                inserted: 0,
                elapsed_secs: 0,
                error: None,
                download_bytes: 0,
                download_total: None,
            },
        }) {
            Ok(g) => g,
            Err(_) => return false,
        };

        let state = state.clone();
        tokio::spawn(async move {
            let start = std::time::Instant::now();
            Self::run_import(&state, xml_path, start, skip_enrichment, guard).await;
        });

        true
    }

    /// Clear library DB and re-import from `launchbox-metadata.xml` if present.
    /// Returns an error message if the XML file is not found.
    pub async fn regenerate_metadata(&self, state: &AppState) -> Result<(), String> {
        use replay_control_core_server::library_db::{ImportProgress, ImportState, LAUNCHBOX_XML};

        // Find launchbox-metadata.xml (with fallback to old name) BEFORE claiming
        // the activity slot — no point locking out other operations if the file
        // doesn't exist.
        let storage = state.storage();
        let rc_dir = storage.rc_dir();
        let xml_path = rc_dir.join(LAUNCHBOX_XML);
        let xml_path = if xml_path.exists() {
            xml_path
        } else {
            // Backwards-compat: check old upstream name.
            let old_path = rc_dir.join("Metadata.xml");
            if old_path.exists() {
                old_path
            } else {
                xml_path
            }
        };
        if !xml_path.exists() {
            return Err(format!(
                "No {LAUNCHBOX_XML} found. Place it in the .replay-control folder to enable re-import."
            ));
        }

        // Atomically claim the activity slot FIRST, then clear the DB.
        // This avoids a TOCTOU race where is_idle() succeeds, another operation
        // claims the slot, and we wipe the DB without being able to re-import.
        let guard = state
            .try_start_activity(Activity::Import {
                progress: ImportProgress {
                    state: ImportState::BuildingIndex,
                    processed: 0,
                    matched: 0,
                    inserted: 0,
                    elapsed_secs: 0,
                    error: None,
                    download_bytes: 0,
                    download_total: None,
                },
            })
            .map_err(|e| e.to_string())?;

        // Clear existing metadata (safe: we own the activity slot).
        if let Some(result) = state
            .library_pool
            .write(|conn| LibraryDb::clear(conn))
            .await
        {
            result.map_err(|e| e.to_string())?;
        }

        // Spawn the import task, passing the guard so the slot stays claimed.
        let state = state.clone();
        tokio::spawn(async move {
            let start = std::time::Instant::now();
            Self::run_import(&state, xml_path, start, false, guard).await;
        });

        Ok(())
    }

    /// Download LaunchBox Metadata.zip, extract, clear DB, and re-import.
    /// Runs entirely in a background task. Returns false if another metadata
    /// operation is already running.
    pub fn start_metadata_download(&self, state: &AppState) -> bool {
        use replay_control_core_server::library_db::{ImportProgress, ImportState};

        // Atomically claim the operation slot via Activity.
        let guard = match state.try_start_activity(Activity::Import {
            progress: ImportProgress {
                state: ImportState::Downloading,
                processed: 0,
                matched: 0,
                inserted: 0,
                elapsed_secs: 0,
                error: None,
                download_bytes: 0,
                download_total: None,
            },
        }) {
            Ok(g) => g,
            Err(_) => return false,
        };

        let state = state.clone();
        tokio::spawn(async move {
            let start = std::time::Instant::now();
            let storage = state.storage();
            let rc_dir = storage.rc_dir();

            // Download and extract with streaming progress.
            // download_metadata is blocking I/O — run in spawn_blocking.
            let activity_lock = state.activity.clone();
            let activity_tx = state.activity_tx.clone();
            let rc_dir_owned = rc_dir.to_path_buf();
            let xml_path = match tokio::task::spawn_blocking({
                move || {
                    let start = start;
                    replay_control_core_server::launchbox::download_metadata(
                        &rc_dir_owned,
                        |downloaded, total| {
                            let mut guard = write_lock(&activity_lock, "activity");
                            if let Activity::Import { progress } = &mut *guard {
                                progress.download_bytes = downloaded;
                                progress.download_total = total;
                                progress.elapsed_secs = start.elapsed().as_secs();
                            }
                            let activity = guard.clone();
                            drop(guard);
                            let _ = activity_tx.send(activity);
                        },
                    )
                }
            })
            .await
            {
                Ok(Ok(path)) => path,
                Ok(Err(e)) => {
                    state.update_activity(|act| {
                        if let Activity::Import { progress } = act {
                            progress.state = ImportState::Failed;
                            progress.error = Some(format!("Download failed: {e}"));
                            progress.elapsed_secs = start.elapsed().as_secs();
                        }
                    });
                    // Guard drops here → Idle
                    return;
                }
                Err(e) => {
                    state.update_activity(|act| {
                        if let Activity::Import { progress } = act {
                            progress.state = ImportState::Failed;
                            progress.error = Some(format!("Download task panicked: {e}"));
                            progress.elapsed_secs = start.elapsed().as_secs();
                        }
                    });
                    return;
                }
            };

            // Clear existing metadata before re-import.
            if let Some(Err(e)) = state
                .library_pool
                .write(|conn| LibraryDb::clear(conn))
                .await
            {
                tracing::warn!("Failed to clear library DB before re-import: {e}");
            }

            // Update elapsed before starting import.
            state.update_activity(|act| {
                if let Activity::Import { progress } = act {
                    progress.elapsed_secs = start.elapsed().as_secs();
                }
            });

            // Now run the import (this updates activity internally).
            Self::run_import(&state, xml_path, start, false, guard).await;
        });

        true
    }

    /// Run the metadata import asynchronously.
    ///
    /// DB locking is per-batch: the lock is acquired for each ~500-entry batch
    /// flush and then released, giving other threads millisecond-scale gaps to
    /// read the DB between batches.
    async fn run_import(
        state: &AppState,
        xml_path: PathBuf,
        start: std::time::Instant,
        skip_enrichment: bool,
        _guard: ActivityGuard,
    ) {
        use super::WriteGate;
        use replay_control_core_server::library_db::ImportState;

        // Build ROM index (no DB needed).
        let storage_root = state.storage().root.clone();
        state.update_activity(|act| {
            if let Activity::Import { progress } = act {
                progress.state = ImportState::BuildingIndex;
                progress.elapsed_secs = start.elapsed().as_secs();
            }
        });

        let rom_index = replay_control_core_server::launchbox::build_rom_index(&storage_root).await;

        // Verify DB is available before starting the parse.
        {
            let db_available = state.library_pool.read(|_conn| true).await.unwrap_or(false);
            if !db_available {
                tracing::error!("Library DB unavailable at import start (pool closed)");
                state.update_activity(|act| {
                    if let Activity::Import { progress } = act {
                        progress.state = ImportState::Failed;
                        progress.error = Some("Library DB unavailable".to_string());
                        progress.elapsed_secs = start.elapsed().as_secs();
                    }
                });
                // _guard drops → Idle
                return;
            }
        }

        // Update progress to Parsing.
        state.update_activity(|act| {
            if let Activity::Import { progress } = act {
                progress.state = ImportState::Parsing;
                progress.elapsed_secs = start.elapsed().as_secs();
            }
        });

        // Gate reads while import writes to the library DB.
        // On exFAT (DELETE journal), concurrent reads during heavy writes corrupt the DB.
        // Activated after the DB availability check; dropped after checkpoint.
        let write_gate = WriteGate::activate(state.library_pool.write_gate_flag());

        // Bridge the sync XML parser to the async pool in core-server; here we
        // just translate per-batch progress ticks into activity updates.
        let activity_lock = state.activity.clone();
        let activity_tx = state.activity_tx.clone();
        let start_ref = start;
        let result = replay_control_core_server::launchbox::run_bulk_import(
            &state.library_pool,
            &xml_path,
            rom_index,
            move |processed, matched, inserted| {
                let mut guard = write_lock(&activity_lock, "activity");
                if let Activity::Import { progress } = &mut *guard {
                    progress.processed = processed;
                    progress.matched = matched;
                    progress.inserted = inserted;
                    progress.elapsed_secs = start_ref.elapsed().as_secs();
                }
                let activity = guard.clone();
                drop(guard);
                let _ = activity_tx.send(activity);
            },
        )
        .await;

        // Checkpoint WAL after the heavy batch writes.
        state.library_pool.checkpoint().await;

        // Release the write gate — heavy writes are done. The alias import
        // below needs to read from the DB.
        drop(write_gate);

        // Image index is no longer cached — enrichment builds it fresh each run.

        let (succeeded, parse_result) = match &result {
            Ok((_, pr)) => (true, Some(pr)),
            Err(_) => (false, None),
        };

        // Import LaunchBox alternate names into game_alias table.
        if let Some(pr) = parse_result {
            tracing::debug!(
                "Starting LaunchBox alias import ({} alternates, {} game names)",
                pr.alternate_names.len(),
                pr.game_names.len()
            );
            replay_control_core_server::launchbox::import_launchbox_aliases(
                &state.library_pool,
                pr,
            )
            .await;
            tracing::debug!("LaunchBox alias import complete");
        }

        // Update final progress (terminal state).
        state.update_activity(|act| {
            if let Activity::Import { progress } = act {
                match &result {
                    Ok((stats, _)) => {
                        progress.state = ImportState::Complete;
                        progress.processed = stats.total_source;
                        progress.matched = stats.matched;
                        progress.inserted = stats.inserted;
                        progress.elapsed_secs = start.elapsed().as_secs();
                        progress.error = None;
                    }
                    Err(e) => {
                        progress.state = ImportState::Failed;
                        progress.error = Some(e.to_string());
                        progress.elapsed_secs = start.elapsed().as_secs();
                    }
                }
            }
        });

        // Re-enrich game library with freshly imported data.
        // Skip during startup auto-import: the pipeline handles populate/enrichment
        // sequentially to avoid races. For user-triggered imports, enrich immediately.
        if succeeded && !skip_enrichment {
            state.spawn_cache_enrichment();
        }

        // _guard drops here → Idle
    }
}
