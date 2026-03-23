use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock, RwLockWriteGuard};

use replay_control_core::metadata_db::MetadataDb;

use super::activity::{Activity, ActivityGuard, ThumbnailPhase, ThumbnailProgress};
use super::AppState;

/// Acquire a write lock, panicking on poison with a standard message.
fn write_lock<'a, T>(lock: &'a RwLock<T>, label: &str) -> RwLockWriteGuard<'a, T> {
    lock.write()
        .unwrap_or_else(|e| panic!("{label} write lock poisoned: {e}"))
}

// ── ImportPipeline ─────────────────────────────────────────────────

/// Manages metadata imports (LaunchBox XML → metadata DB).
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
        use replay_control_core::metadata_db::ImportState;
        matches!(
            state.activity(),
            Activity::Import {
                progress: replay_control_core::metadata_db::ImportProgress {
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
        use replay_control_core::metadata_db::{ImportProgress, ImportState};

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
        tokio::task::spawn_blocking(move || {
            let start = std::time::Instant::now();
            Self::run_import_blocking(&state, xml_path, start, skip_enrichment, guard);
        });

        true
    }

    /// Clear metadata DB and re-import from `launchbox-metadata.xml` if present.
    /// Returns an error message if the XML file is not found.
    pub fn regenerate_metadata(&self, state: &AppState) -> Result<(), String> {
        use replay_control_core::metadata_db::LAUNCHBOX_XML;

        // Check if another activity is running BEFORE clearing the DB.
        // Otherwise we'd wipe metadata but fail to re-import.
        if !state.is_idle() {
            return Err("Another operation is already running".to_string());
        }

        // Clear existing metadata.
        if let Some(result) = state.metadata_pool.write(|conn| MetadataDb::clear(conn)) {
            result.map_err(|e| e.to_string())?;
        }

        // Find launchbox-metadata.xml (with fallback to old name) and trigger re-import.
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

        self.start_import(xml_path, state.clone());
        Ok(())
    }

    /// Download LaunchBox Metadata.zip, extract, clear DB, and re-import.
    /// Runs entirely in a background thread. Returns false if another metadata
    /// operation is already running.
    pub fn start_metadata_download(&self, state: &AppState) -> bool {
        use replay_control_core::metadata_db::{ImportProgress, ImportState};

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
        tokio::task::spawn_blocking(move || {
            let start = std::time::Instant::now();
            let storage = state.storage();
            let rc_dir = storage.rc_dir();

            // Download and extract with streaming progress.
            let activity_lock = state.activity.clone();
            let start_ref = start;
            let xml_path = match replay_control_core::launchbox::download_metadata(
                &rc_dir,
                |downloaded, total| {
                    let mut guard = write_lock(&activity_lock, "activity");
                    if let Activity::Import { progress } = &mut *guard {
                        progress.download_bytes = downloaded;
                        progress.download_total = total;
                        progress.elapsed_secs = start_ref.elapsed().as_secs();
                    }
                },
            ) {
                Ok(path) => path,
                Err(e) => {
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
            };

            // Clear existing metadata before re-import.
            if let Some(Err(e)) = state.metadata_pool.write(|conn| MetadataDb::clear(conn)) {
                tracing::warn!("Failed to clear metadata DB before re-import: {e}");
            }

            // Update elapsed before starting import.
            state.update_activity(|act| {
                if let Activity::Import { progress } = act {
                    progress.elapsed_secs = start.elapsed().as_secs();
                }
            });

            // Now run the import (this updates activity internally).
            Self::run_import_blocking(&state, xml_path, start, false, guard);
        });

        true
    }

    /// Run the metadata import synchronously (called from spawn_blocking).
    ///
    /// DB locking is per-batch: the lock is acquired for each ~500-entry batch
    /// flush and then released, giving other threads millisecond-scale gaps to
    /// read the DB between batches.
    fn run_import_blocking(
        state: &AppState,
        xml_path: PathBuf,
        start: std::time::Instant,
        skip_enrichment: bool,
        _guard: ActivityGuard,
    ) {
        use replay_control_core::metadata_db::{ImportState};

        // Build ROM index (no DB needed).
        let storage_root = state.storage().root.clone();
        state.update_activity(|act| {
            if let Activity::Import { progress } = act {
                progress.state = ImportState::BuildingIndex;
                progress.elapsed_secs = start.elapsed().as_secs();
            }
        });

        let rom_index = replay_control_core::launchbox::build_rom_index(&storage_root);

        // Verify DB is available before starting the parse.
        {
            let db_available = state.metadata_pool.read(|_conn| true).unwrap_or(false);
            if !db_available {
                tracing::error!("Metadata DB unavailable at import start (pool closed)");
                state.update_activity(|act| {
                    if let Activity::Import { progress } = act {
                        progress.state = ImportState::Failed;
                        progress.error = Some("Metadata DB unavailable".to_string());
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

        // Per-batch flush closure: acquires a write connection from the pool,
        // calls bulk_upsert, then releases the connection.
        let pool_ref = state.metadata_pool.clone();
        let flush_batch = |batch: &[(
            String,
            String,
            replay_control_core::metadata_db::GameMetadata,
        )]| {
            pool_ref
                .write(|db| MetadataDb::bulk_upsert(db, batch))
                .ok_or_else(|| {
                    replay_control_core::error::Error::Other(
                        "Metadata DB unavailable during import".to_string(),
                    )
                })?
        };

        let activity_lock = state.activity.clone();
        let start_ref = start;
        let result = replay_control_core::launchbox::import_launchbox(
            &xml_path,
            &rom_index,
            |processed, matched, inserted| {
                let mut guard = write_lock(&activity_lock, "activity");
                if let Activity::Import { progress } = &mut *guard {
                    progress.processed = processed;
                    progress.matched = matched;
                    progress.inserted = inserted;
                    progress.elapsed_secs = start_ref.elapsed().as_secs();
                }
            },
            flush_batch,
        );

        // Checkpoint WAL after the heavy batch writes.
        state.metadata_pool.checkpoint();

        // Invalidate image cache so updated metadata paths are picked up.
        state.cache.invalidate_images();

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
            Self::import_launchbox_aliases(state, pr);
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

    /// Import LaunchBox alternate names into the `game_alias` table.
    ///
    /// Uses the `ParseResult` from the single-pass XML parse — no re-reading.
    /// Acquires/releases the DB lock per-operation (read base_titles, then
    /// write aliases) so other threads can access the DB between operations.
    fn import_launchbox_aliases(
        state: &AppState,
        parse_result: &replay_control_core::launchbox::ParseResult,
    ) {
        if parse_result.alternate_names.is_empty() {
            return;
        }

        // Read base_titles from DB via pool.
        tracing::debug!("LaunchBox aliases: loading base_titles from game_library...");
        let base_titles: std::collections::HashMap<String, Vec<String>> = match state
            .metadata_pool
            .read(|conn| {
                let systems = MetadataDb::active_systems(conn).unwrap_or_default();
                let mut map: std::collections::HashMap<String, Vec<String>> =
                    std::collections::HashMap::new();
                for system in &systems {
                    if let Ok(entries) = MetadataDb::load_system_entries(conn, system) {
                        for entry in entries {
                            if !entry.base_title.is_empty() {
                                map.entry(entry.base_title.clone())
                                    .or_default()
                                    .push(system.clone());
                            }
                        }
                    }
                }
                map
            }) {
            Some(map) => map,
            None => {
                tracing::warn!("LaunchBox aliases: DB unavailable for reading base_titles");
                return;
            }
        };

        // Call pure core matching function.
        let aliases = replay_control_core::alias_matching::resolve_launchbox_aliases(
            &parse_result.alternate_names,
            &parse_result.game_names,
            &base_titles,
        );

        if aliases.is_empty() {
            tracing::debug!("LaunchBox aliases: no matches found");
            return;
        }

        // Write aliases to DB via pool.
        let count = aliases.len();
        if let Some(result) = state
            .metadata_pool
            .write(|db| MetadataDb::bulk_insert_aliases(db, &aliases))
        {
            match result {
                Ok(n) => tracing::info!("LaunchBox aliases: {n}/{count} inserted"),
                Err(e) => tracing::warn!("LaunchBox aliases: insert failed: {e}"),
            }
        } else {
            tracing::warn!("LaunchBox aliases: DB unavailable for inserting aliases");
        }
    }
}

// ── ThumbnailPipeline ──────────────────────────────────────────────

/// Manages the two-phase thumbnail pipeline (index + download).
///
/// No longer owns its own busy flag, progress, or cancel --
/// those live in `AppState.activity` as `Activity::ThumbnailUpdate { progress, cancel }`.
#[derive(Default)]
pub struct ThumbnailPipeline;

impl ThumbnailPipeline {
    pub fn new() -> Self {
        Self
    }

    /// Start the two-phase thumbnail pipeline in the background.
    /// Returns `false` if another metadata operation is already running.
    pub fn start_thumbnail_update(&self, state: &AppState) -> bool {
        let cancel = Arc::new(AtomicBool::new(false));

        let guard = match state.try_start_activity(Activity::ThumbnailUpdate {
            progress: ThumbnailProgress {
                phase: ThumbnailPhase::Indexing,
                current_label: String::new(),
                step_done: 0,
                step_total: 0,
                downloaded: 0,
                entries_indexed: 0,
                elapsed_secs: 0,
                error: None,
            },
            cancel: cancel.clone(),
        }) {
            Ok(g) => g,
            Err(_) => return false,
        };

        let state = state.clone();
        tokio::task::spawn_blocking(move || {
            let start = std::time::Instant::now();
            Self::run_thumbnail_update_blocking(&state, start, cancel, guard);
        });

        true
    }

    /// Run the two-phase thumbnail pipeline (blocking, called from spawn_blocking).
    fn run_thumbnail_update_blocking(
        state: &AppState,
        start: std::time::Instant,
        cancel: Arc<AtomicBool>,
        _guard: ActivityGuard,
    ) {
        use replay_control_core::thumbnail_manifest;
        use replay_control_core::thumbnails::ALL_THUMBNAIL_KINDS;

        let storage_root = state.storage().root.clone();

        // Verify DB is available before starting.
        {
            let db_available = state.metadata_pool.read(|_conn| true).unwrap_or(false);
            if !db_available {
                tracing::error!(
                    "Metadata DB unavailable at thumbnail update start (pool closed)"
                );
                state.update_activity(|act| {
                    if let Activity::ThumbnailUpdate { progress, .. } = act {
                        progress.phase = ThumbnailPhase::Failed;
                        progress.error = Some("Metadata DB unavailable".to_string());
                        progress.elapsed_secs = start.elapsed().as_secs();
                    }
                });
                return;
            }
        }

        // ── Phase 1: Index refresh ──────────────────────────────────
        let activity_lock = state.activity.clone();

        // Read GitHub API key from settings (if configured).
        let api_key = replay_control_core::settings::read_github_api_key(&storage_root);

        let index_result = {
            let cancel_flag = cancel.clone();
            let api_key_owned = api_key.clone();
            let activity_ref = activity_lock.clone();
            state
                .metadata_pool
                .write(|db| {
                    thumbnail_manifest::import_all_manifests(
                        db,
                        &|repos_done, repos_total, current_repo| {
                            let mut guard = write_lock(&activity_ref, "activity");
                            if let Activity::ThumbnailUpdate { progress, .. } = &mut *guard {
                                progress.phase = ThumbnailPhase::Indexing;
                                progress.step_done = repos_done;
                                progress.step_total = repos_total;
                                progress.current_label = current_repo.to_string();
                                progress.elapsed_secs = start.elapsed().as_secs();
                            }
                        },
                        &cancel_flag,
                        api_key_owned.as_deref(),
                    )
                })
                .unwrap_or_else(|| {
                    Err(replay_control_core::error::Error::Other(
                        "Metadata DB unavailable during thumbnail index".to_string(),
                    ))
                })
        };

        let index_stats = match index_result {
            Ok(stats) => {
                if !stats.errors.is_empty() {
                    tracing::warn!(
                        "Thumbnail index: {} errors: {:?}",
                        stats.errors.len(),
                        stats.errors
                    );
                }

                // If ALL repos failed (0 entries), report as Failed.
                if stats.total_entries == 0 && !stats.errors.is_empty() {
                    let is_rate_limited = stats
                        .errors
                        .iter()
                        .any(|e| e.contains("403") || e.contains("rate"));
                    let msg = if is_rate_limited {
                        format!(
                            "GitHub API rate limit exceeded ({}/{} repos failed). Wait ~1 hour and try again.",
                            stats.errors.len(),
                            stats.errors.len() + stats.repos_fetched,
                        )
                    } else {
                        format!(
                            "All repos failed to index ({} errors). First: {}",
                            stats.errors.len(),
                            stats
                                .errors
                                .first()
                                .map(|s| s.as_str())
                                .unwrap_or("unknown"),
                        )
                    };
                    state.update_activity(|act| {
                        if let Activity::ThumbnailUpdate { progress, .. } = act {
                            progress.phase = ThumbnailPhase::Failed;
                            progress.error = Some(msg);
                            progress.elapsed_secs = start.elapsed().as_secs();
                        }
                    });
                    return;
                }

                // Update progress with index results.
                state.update_activity(|act| {
                    if let Activity::ThumbnailUpdate { progress, .. } = act {
                        progress.entries_indexed = stats.total_entries;
                        progress.elapsed_secs = start.elapsed().as_secs();
                    }
                });
                stats
            }
            Err(e) => {
                state.update_activity(|act| {
                    if let Activity::ThumbnailUpdate { progress, .. } = act {
                        progress.phase = ThumbnailPhase::Failed;
                        progress.error = Some(format!("Index failed: {e}"));
                        progress.elapsed_secs = start.elapsed().as_secs();
                    }
                });
                return;
            }
        };

        // Checkpoint WAL after the index phase's bulk writes.
        state.metadata_pool.checkpoint();

        // Check cancellation between phases.
        if cancel.load(Ordering::Relaxed) {
            state.update_activity(|act| {
                if let Activity::ThumbnailUpdate { progress, .. } = act {
                    progress.phase = ThumbnailPhase::Cancelled;
                    progress.elapsed_secs = start.elapsed().as_secs();
                }
            });
            return;
        }

        // ── Phase 2: Download images ────────────────────────────────
        state.update_activity(|act| {
            if let Activity::ThumbnailUpdate { progress, .. } = act {
                progress.phase = ThumbnailPhase::Downloading;
                progress.step_done = 0;
                progress.step_total = 0;
                progress.downloaded = 0;
                progress.elapsed_secs = start.elapsed().as_secs();
            }
        });

        // Collect systems that have ROMs and a thumbnail repo.
        let storage = state.storage();
        let systems = state.cache.get_systems(&storage);
        let supported: Vec<String> = systems
            .into_iter()
            .filter(|s| s.game_count > 0)
            .filter(|s| {
                replay_control_core::thumbnails::thumbnail_repo_names(&s.folder_name).is_some()
            })
            .map(|s| s.folder_name)
            .collect();

        let total_systems = supported.len();
        let mut total_downloaded = 0usize;
        let mut total_failed = 0usize;

        for (i, system) in supported.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                break;
            }

            let system_display = replay_control_core::systems::find_system(system)
                .map(|s| s.display_name.to_string())
                .unwrap_or_else(|| system.to_string());

            // Update progress for this system.
            state.update_activity(|act| {
                if let Activity::ThumbnailUpdate { progress, .. } = act {
                    progress.current_label = system_display.clone();
                    progress.step_done = i;
                    progress.step_total = total_systems;
                    progress.elapsed_secs = start.elapsed().as_secs();
                }
            });

            let activity_ref = activity_lock.clone();

            for kind in ALL_THUMBNAIL_KINDS {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                let prev_downloaded = total_downloaded;
                let cancel_flag = cancel.clone();
                let activity_ref2 = activity_ref.clone();
                if let Some(result) = state.metadata_pool.read(|conn| {
                    thumbnail_manifest::download_system_thumbnails(
                        conn,
                        &storage_root,
                        system,
                        *kind,
                        &|processed, total, downloaded| {
                            let mut guard = write_lock(&activity_ref2, "activity");
                            if let Activity::ThumbnailUpdate { progress, .. } = &mut *guard {
                                progress.step_done = i;
                                progress.step_total = total_systems;
                                progress.downloaded = prev_downloaded + downloaded;
                                progress.elapsed_secs = start.elapsed().as_secs();
                                if total > 0 {
                                    let display_n = (processed + 1).min(total);
                                    progress.current_label =
                                        format!("{system_display}: {display_n}/{total}");
                                }
                            }
                        },
                        &cancel_flag,
                    )
                }) {
                    match result {
                        Ok(stats) => {
                            total_downloaded += stats.downloaded;
                            total_failed += stats.failed;
                        }
                        Err(e) => {
                            let kind_name = kind.media_dir();
                            tracing::warn!("{kind_name} download failed for {system}: {e}");
                        }
                    }
                }
            }

            // Lock DB for image path update, then release.
            Self::update_image_paths_from_disk(state, &storage_root, system);
        }

        // Invalidate the image cache so new thumbnails are picked up.
        state.cache.invalidate_images();

        let cancelled = cancel.load(Ordering::Relaxed);

        // Re-enrich game library with freshly downloaded box art.
        if !cancelled {
            state.spawn_cache_enrichment();
        }

        // Set final progress (terminal state).
        state.update_activity(|act| {
            if let Activity::ThumbnailUpdate { progress, .. } = act {
                if cancelled {
                    progress.phase = ThumbnailPhase::Cancelled;
                    progress.downloaded = total_downloaded;
                    progress.elapsed_secs = start.elapsed().as_secs();
                } else {
                    progress.phase = ThumbnailPhase::Complete;
                    progress.current_label = String::new();
                    progress.step_done = total_systems;
                    progress.step_total = total_systems;
                    progress.downloaded = total_downloaded;
                    progress.entries_indexed = index_stats.total_entries;
                    progress.elapsed_secs = start.elapsed().as_secs();
                    progress.error = {
                        let mut parts = Vec::new();
                        if !index_stats.errors.is_empty() {
                            parts.push(format!(
                                "{} repos failed to index",
                                index_stats.errors.len()
                            ));
                        }
                        if total_failed > 0 {
                            parts.push(format!("{total_failed} images failed to download"));
                        }
                        if parts.is_empty() {
                            None
                        } else {
                            Some(parts.join("; "))
                        }
                    };
                }
            }
        });

        // _guard drops here → Idle
    }

    /// Scan the media directory for a system and update game_metadata image paths.
    fn update_image_paths_from_disk(
        state: &AppState,
        storage_root: &std::path::Path,
        system: &str,
    ) {
        use replay_control_core::image_matching::{build_dir_index, find_best_match};

        // Read visible filenames from DB via pool.
        let rom_filenames = match state.metadata_pool.read(|conn| {
            MetadataDb::visible_filenames(conn, system).unwrap_or_default()
        }) {
            Some(filenames) => filenames,
            None => return,
        };

        // Build dir indexes (filesystem scan, no DB needed).
        use replay_control_core::thumbnails::ALL_THUMBNAIL_KINDS;
        let media_base = storage_root
            .join(replay_control_core::storage::RC_DIR)
            .join("media")
            .join(system);

        let indexes: Vec<_> = ALL_THUMBNAIL_KINDS
            .iter()
            .map(|kind| {
                let dir = media_base.join(kind.media_dir());
                build_dir_index(&dir, kind.media_dir())
            })
            .collect();
        let box_index = &indexes[0];
        let snap_index = &indexes[1];
        let title_index = &indexes[2];

        let is_arcade = replay_control_core::systems::is_arcade_system(system);

        // Match ROM filenames to images (in-memory, no DB needed).
        let mut updates: Vec<replay_control_core::metadata_db::ImagePathUpdate> = Vec::new();

        for rom_filename in &rom_filenames {
            let arcade_display = if is_arcade {
                let stem = rom_filename
                    .rfind('.')
                    .map(|i| &rom_filename[..i])
                    .unwrap_or(rom_filename);
                replay_control_core::arcade_db::lookup_arcade_game(stem)
                    .map(|info| info.display_name)
            } else {
                None
            };
            let boxart_rel = find_best_match(box_index, rom_filename, arcade_display, None);
            let snap_rel = find_best_match(snap_index, rom_filename, arcade_display, None);
            let title_rel = find_best_match(title_index, rom_filename, arcade_display, None);

            if boxart_rel.is_some() || snap_rel.is_some() || title_rel.is_some() {
                updates.push(replay_control_core::metadata_db::ImagePathUpdate {
                    system: system.to_string(),
                    rom_filename: rom_filename.clone(),
                    box_art_path: boxart_rel,
                    screenshot_path: snap_rel,
                    title_path: title_rel,
                });
            }
        }

        // Write image path updates to DB via pool.
        if !updates.is_empty()
            && let Some(Err(e)) = state
                .metadata_pool
                .write(|db| MetadataDb::bulk_update_image_paths(db, &updates))
        {
            tracing::warn!("Failed to update image paths for {system}: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipelines_create_without_panicking() {
        let _import = ImportPipeline::new();
        let _thumbnails = ThumbnailPipeline::new();
    }
}
