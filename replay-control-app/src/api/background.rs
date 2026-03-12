use std::path::PathBuf;
use std::time::Duration;

use super::AppState;
use super::cache::dir_mtime;

/// How often the background task re-checks storage (in seconds).
const STORAGE_CHECK_INTERVAL: u64 = 60;

impl AppState {
    /// Verify L2 cache freshness on startup and pre-populate if empty.
    ///
    /// - If L2 is empty (fresh DB): scan all systems with games to populate rom_cache.
    ///   This ensures recommendations work on first visit without waiting for the user
    ///   to browse every system page.
    /// - If L2 has data: verify stored mtimes against filesystem, re-scan stale systems.
    pub fn spawn_cache_verification(&self) {
        let state = self.clone();
        tokio::spawn(async move {
            // Brief delay to let the server start accepting requests.
            tokio::time::sleep(Duration::from_secs(2)).await;

            let result = tokio::task::spawn_blocking(move || {
                let storage = state.storage();
                let roms_dir = storage.roms_dir();
                let region_pref = state.region_preference();

                // Skip if a metadata operation (e.g. auto-import) is already
                // running — opening a second nolock connection to the same DB
                // file would cause corruption.
                if state
                    .metadata_operation_in_progress
                    .load(std::sync::atomic::Ordering::SeqCst)
                {
                    tracing::debug!(
                        "Metadata operation in progress, skipping cache verification"
                    );
                    return;
                }

                // Load all cached system metadata from L2.
                // Use metadata_db() accessor (which handles lazy open) instead
                // of locking the raw mutex, so we go through the standard path.
                let cached_meta = {
                    let guard = state.metadata_db();
                    guard.and_then(|g| g.as_ref()?.load_all_system_meta().ok())
                };

                let cached_meta = cached_meta.unwrap_or_default();

                if cached_meta.is_empty() {
                    // Fresh DB — pre-populate L2 for all systems with games.
                    Self::populate_all_systems(&state, &storage, region_pref);
                    return;
                }

                let mut stale_count = 0usize;
                for meta in &cached_meta {
                    let system_dir = roms_dir.join(&meta.system);
                    let current_mtime_secs = dir_mtime(&system_dir).and_then(|t| {
                        t.duration_since(std::time::UNIX_EPOCH)
                            .ok()
                            .map(|d| d.as_secs() as i64)
                    });

                    let is_stale = match (meta.dir_mtime_secs, current_mtime_secs) {
                        (Some(cached), Some(current)) => cached != current,
                        (Some(_), None) => false, // Can't read — trust cache
                        (None, _) => true,        // No mtime stored — re-scan
                    };

                    if is_stale {
                        tracing::info!("Background re-scan: {} (mtime changed)", meta.system);
                        // Trigger L3 scan by calling get_roms (which writes through to L1+L2).
                        let _ = state.cache.get_roms(&storage, &meta.system, region_pref);
                        state.cache.enrich_system_cache(&state, &meta.system);
                        stale_count += 1;
                    }
                }

                if stale_count > 0 {
                    tracing::info!(
                        "Background cache verification: re-scanned {stale_count} stale system(s)"
                    );
                    // Also refresh the systems list since counts may have changed.
                    let _ = state.cache.get_systems(&storage);
                } else {
                    tracing::debug!(
                        "Background cache verification: all {} system(s) fresh",
                        cached_meta.len()
                    );
                }
            })
            .await;

            if let Err(e) = result {
                tracing::warn!("Background cache verification failed: {e}");
            }
        });
    }

    /// Pre-populate L2 cache for all systems that have games.
    /// Called on startup when the rom_cache is empty (fresh DB or after clear).
    /// After populating ROMs, enriches box art URLs and ratings.
    fn populate_all_systems(
        state: &AppState,
        storage: &replay_control_core::storage::StorageLocation,
        region_pref: replay_control_core::rom_tags::RegionPreference,
    ) {
        let systems = state.cache.get_systems(storage);
        let with_games: Vec<_> = systems.iter().filter(|s| s.game_count > 0).collect();
        tracing::info!(
            "L2 warmup: populating {} system(s) with games",
            with_games.len()
        );

        let start = std::time::Instant::now();
        let mut total_roms = 0usize;
        for sys in &with_games {
            match state.cache.get_roms(storage, &sys.folder_name, region_pref) {
                Ok(roms) => total_roms += roms.len(),
                Err(e) => tracing::warn!("L2 warmup: failed to scan {}: {e}", sys.folder_name),
            }
        }

        tracing::info!(
            "L2 warmup: scanned {} ROMs across {} systems in {:.1}s, enriching...",
            total_roms,
            with_games.len(),
            start.elapsed().as_secs_f64()
        );

        // Enrich box art URLs and ratings for all systems.
        for sys in &with_games {
            state.cache.enrich_system_cache(state, &sys.folder_name);
        }

        tracing::info!(
            "L2 warmup: done — {} ROMs across {} systems in {:.1}s",
            total_roms,
            with_games.len(),
            start.elapsed().as_secs_f64()
        );
    }

    /// Auto-import metadata on startup if `launchbox-metadata.xml` exists and DB is empty.
    pub fn spawn_auto_import(&self) {
        use replay_control_core::metadata_db::LAUNCHBOX_XML;

        let storage = self.storage();
        let rc_dir = storage.rc_dir();
        let xml_path = rc_dir.join(LAUNCHBOX_XML);
        // Backwards-compat: fall back to old upstream name if user placed it manually.
        let xml_path = if xml_path.exists() {
            xml_path
        } else {
            let old_path = rc_dir.join("Metadata.xml");
            if old_path.exists() {
                old_path
            } else {
                xml_path
            }
        };

        if !xml_path.exists() {
            tracing::debug!(
                "No {} at {}, skipping auto-import",
                LAUNCHBOX_XML,
                xml_path.display()
            );
            return;
        }

        let should_import = if let Some(guard) = self.metadata_db() {
            guard
                .as_ref()
                .and_then(|db| db.is_empty().ok())
                .unwrap_or(false)
        } else {
            false
        };

        if should_import {
            tracing::info!("Auto-importing metadata from {}", xml_path.display());
            self.start_import(xml_path);
        }
    }

    /// Spawn a background task that watches `replay.cfg` for changes and
    /// periodically re-checks storage as a fallback.
    ///
    /// Uses `notify` (inotify on Linux) to react immediately when the config
    /// file is modified. Falls back to the 60-second poll if filesystem
    /// watching cannot be set up (e.g., on NFS).
    pub fn spawn_storage_watcher(self) {
        let config_path = self.config_file_path();
        let state = self.clone();

        // Spawn the filesystem watcher in a blocking thread (notify uses
        // its own event loop that blocks the thread).
        let watcher_state = self.clone();
        let watcher_config_path = config_path.clone();

        tokio::spawn(async move {
            let watcher_active =
                Self::try_start_config_watcher(watcher_state, watcher_config_path).await;

            if watcher_active {
                tracing::info!("Config file watcher active; 60s poll runs as fallback");
            } else {
                tracing::info!("Config file watcher unavailable; using 60s poll only");
            }

            // The 60-second poll always runs as a fallback.
            let mut interval =
                tokio::time::interval(std::time::Duration::from_secs(STORAGE_CHECK_INTERVAL));
            // Skip the first (immediate) tick — we just initialized.
            interval.tick().await;
            loop {
                interval.tick().await;
                match state.refresh_storage() {
                    Ok(true) => tracing::info!("Background storage re-detection: storage changed"),
                    Ok(false) => {}
                    Err(e) => tracing::warn!("Background storage re-detection failed: {e}"),
                }
            }
        });
    }

    /// Try to set up a `notify` filesystem watcher on the config file.
    /// Returns `true` if the watcher was started successfully.
    async fn try_start_config_watcher(state: AppState, config_path: PathBuf) -> bool {
        use notify::{RecursiveMode, Watcher, recommended_watcher};

        // Watch the parent directory — the file itself may not exist yet, and
        // some editors write to a temp file then rename, which only shows up as
        // an event on the directory.
        let watch_dir = match config_path.parent() {
            Some(dir) if dir.exists() => dir.to_path_buf(),
            Some(dir) => {
                tracing::warn!(
                    "Config directory does not exist ({}), cannot set up file watcher",
                    dir.display()
                );
                return false;
            }
            None => {
                tracing::warn!("Cannot determine parent directory of config path");
                return false;
            }
        };

        let config_filename = config_path
            .file_name()
            .map(|n| n.to_os_string())
            .unwrap_or_default();

        let (tx, mut rx) = tokio::sync::mpsc::channel(16);

        // Create the watcher. The callback sends events through the channel
        // so we can process them on the async side.
        let mut watcher =
            match recommended_watcher(move |res: Result<notify::Event, notify::Error>| match res {
                Ok(event) => {
                    let _ = tx.blocking_send(event);
                }
                Err(e) => {
                    tracing::warn!("File watcher error: {e}");
                }
            }) {
                Ok(w) => w,
                Err(e) => {
                    tracing::warn!("Failed to create file watcher: {e}");
                    return false;
                }
            };

        if let Err(e) = watcher.watch(&watch_dir, RecursiveMode::NonRecursive) {
            tracing::warn!("Failed to watch directory {}: {e}", watch_dir.display());
            return false;
        }

        tracing::info!("Watching {} for config changes", watch_dir.display());

        // Spawn the event-processing loop. We keep `watcher` alive by moving
        // it into this task — dropping it would stop watching.
        tokio::spawn(async move {
            let _watcher = watcher; // prevent drop

            // Debounce: after the first relevant event, wait before refreshing
            // so that rapid successive writes (common with text editors) only
            // trigger a single refresh.
            const DEBOUNCE: Duration = Duration::from_secs(2);

            loop {
                // Wait for the next event.
                let Some(event) = rx.recv().await else {
                    tracing::warn!("Config file watcher channel closed");
                    break;
                };

                if !Self::is_config_event(&event, &config_filename) {
                    continue;
                }

                tracing::debug!("Config change detected ({:?}), debouncing...", event.kind);

                // Drain any further events that arrive within the debounce window.
                let deadline = tokio::time::Instant::now() + DEBOUNCE;
                loop {
                    match tokio::time::timeout_at(deadline, rx.recv()).await {
                        Ok(Some(ev)) => {
                            if Self::is_config_event(&ev, &config_filename) {
                                tracing::debug!(
                                    "Additional config event during debounce ({:?})",
                                    ev.kind
                                );
                            }
                        }
                        Ok(None) => {
                            // Channel closed
                            break;
                        }
                        Err(_) => {
                            // Timeout — debounce window expired
                            break;
                        }
                    }
                }

                tracing::info!("Config file changed, refreshing storage");
                match state.refresh_storage() {
                    Ok(true) => tracing::info!("Storage updated after config change"),
                    Ok(false) => tracing::debug!("Config changed but storage unchanged"),
                    Err(e) => tracing::warn!("Failed to refresh storage after config change: {e}"),
                }
            }
        });

        true
    }

    /// Check whether a notify event is relevant to our config file.
    fn is_config_event(event: &notify::Event, config_filename: &std::ffi::OsStr) -> bool {
        use notify::EventKind;

        // Only react to creates, modifications, and renames (some editors
        // write a temp file then rename it over the original).
        matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_))
            && event
                .paths
                .iter()
                .any(|p| p.file_name().is_some_and(|n| n == config_filename))
    }
}
