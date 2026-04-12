use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use replay_control_core::metadata_db::MetadataDb;

use super::AppState;
use super::activity::{Activity, ActivityGuard, ThumbnailPhase, ThumbnailProgress};

/// Acquire a write lock, panicking on poison with a standard message.
fn write_lock<'a, T>(
    lock: &'a std::sync::RwLock<T>,
    label: &str,
) -> std::sync::RwLockWriteGuard<'a, T> {
    lock.write()
        .unwrap_or_else(|e| panic!("{label} write lock poisoned: {e}"))
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
        tokio::spawn(async move {
            let start = std::time::Instant::now();
            Self::run_thumbnail_update(&state, start, cancel, guard).await;
        });

        true
    }

    /// Run the two-phase thumbnail pipeline asynchronously.
    async fn run_thumbnail_update(
        state: &AppState,
        start: std::time::Instant,
        cancel: Arc<AtomicBool>,
        _guard: ActivityGuard,
    ) {
        use super::WriteGate;
        use replay_control_core::thumbnail_manifest;
        use replay_control_core::thumbnails::ALL_THUMBNAIL_KINDS;

        let storage_root = state.storage().root.clone();

        // Verify DB is available before starting.
        {
            let db_available = state
                .metadata_pool
                .read(|_conn| true)
                .await
                .unwrap_or(false);
            if !db_available {
                tracing::error!("Metadata DB unavailable at thumbnail update start (pool closed)");
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

        // Gate reads while thumbnail index writes to the metadata DB.
        // On exFAT (DELETE journal), concurrent reads during heavy writes corrupt the DB.
        // Activated after the DB availability check; dropped after Phase 1 checkpoint.
        let write_gate = WriteGate::activate(state.metadata_pool.write_gate_flag());

        // ── Phase 1: Index refresh ──────────────────────────────────
        let activity_lock = state.activity.clone();

        // Read GitHub API key from settings (if configured).
        let api_key = replay_control_core::settings::read_github_api_key(&state.settings);

        let index_result = {
            let cancel_flag = cancel.clone();
            let api_key_owned = api_key.clone();
            let activity_ref = activity_lock.clone();
            let activity_tx = state.activity_tx.clone();
            state
                .metadata_pool
                .write(move |db| {
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
                            let activity = guard.clone();
                            drop(guard);
                            let _ = activity_tx.send(activity);
                        },
                        &cancel_flag,
                        api_key_owned.as_deref(),
                    )
                })
                .await
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
        state.metadata_pool.checkpoint().await;

        // Release the write gate — Phase 1 heavy writes are done. Phase 2
        // (downloads) needs to read the DB for thumbnail index lookups.
        drop(write_gate);

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
        let systems = state
            .cache
            .cached_systems(&storage, &state.metadata_pool)
            .await;
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
                let activity_tx = state.activity_tx.clone();
                let system_display_owned = system_display.clone();
                let storage_root = storage_root.clone();
                let system_owned = system.clone();
                if let Some(result) = state
                    .metadata_pool
                    .read(move |conn| {
                        thumbnail_manifest::download_system_thumbnails(
                            conn,
                            &storage_root,
                            &system_owned,
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
                                            format!("{system_display_owned}: {display_n}/{total}");
                                    }
                                }
                                let activity = guard.clone();
                                drop(guard);
                                let _ = activity_tx.send(activity);
                            },
                            &cancel_flag,
                        )
                    })
                    .await
                {
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
            Self::update_image_paths_from_disk(state, &storage_root, system).await;
        }

        // Image index is no longer cached — enrichment builds it fresh each run.

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
    async fn update_image_paths_from_disk(
        state: &AppState,
        storage_root: &std::path::Path,
        system: &str,
    ) {
        use replay_control_core::image_matching::{build_dir_index, find_best_match};

        // Read visible filenames from DB via pool.
        let system_owned = system.to_string();
        let rom_filenames = match state
            .metadata_pool
            .read(move |conn| {
                MetadataDb::visible_filenames(conn, &system_owned).unwrap_or_default()
            })
            .await
        {
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
                .write(move |db| MetadataDb::bulk_update_image_paths(db, &updates))
                .await
        {
            tracing::warn!("Failed to update image paths for {system}: {e}");
        }
    }
}
