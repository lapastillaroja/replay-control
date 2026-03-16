use std::sync::atomic::Ordering;
use std::time::Duration;

use super::AppState;
use super::cache::dir_mtime;

/// How often the background task re-checks storage (in seconds).
const STORAGE_CHECK_INTERVAL: u64 = 60;

/// Simple RAII guard that runs a closure on drop.
mod guard {
    pub struct Guard<F: FnOnce()>(Option<F>);
    impl<F: FnOnce()> Guard<F> {
        pub fn new(f: F) -> Self { Self(Some(f)) }
    }
    impl<F: FnOnce()> Drop for Guard<F> {
        fn drop(&mut self) {
            if let Some(f) = self.0.take() { f(); }
        }
    }
}

/// Orchestrates the ordered background startup pipeline and long-running watchers.
///
/// Pipeline phases (sequential, blocking):
///   1. Auto-import — if a LaunchBox XML file exists and the DB is empty
///   2. Cache populate/verify — scan all systems, enrich box art + ratings
///
/// Filesystem watchers (config file, ROM directory) run independently.
pub struct BackgroundManager;

impl BackgroundManager {
    /// Start the ordered background pipeline.
    pub fn start(state: AppState) {
        // Spawn the ordered pipeline in a blocking thread.
        let pipeline_state = state.clone();
        tokio::spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                Self::run_pipeline(&pipeline_state);
            })
            .await;

            if let Err(e) = result {
                tracing::warn!("Background startup pipeline failed: {e}");
            }
        });

        // Start watchers immediately (they're independent of the pipeline).
        state.clone().spawn_storage_watcher();
        state.spawn_rom_watcher();
    }

    /// Run the ordered startup pipeline (blocking).
    fn run_pipeline(state: &AppState) {
        // Brief delay to let the server start accepting requests.
        std::thread::sleep(Duration::from_secs(2));

        // Set warmup flag for the pipeline duration. Cleared on drop via guard,
        // even if a phase panics.
        state
            .cache
            .warmup_in_progress
            .store(true, Ordering::SeqCst);
        let warmup_flag = state.cache.warmup_in_progress.clone();
        let _warmup_guard = guard::Guard::new(move || {
            warmup_flag.store(false, Ordering::SeqCst);
        });

        // Phase 1: Auto-import (if launchbox XML exists + DB empty).
        Self::phase_auto_import(state);

        // Wait for auto-import to finish before proceeding.
        while state.import.is_busy() {
            std::thread::sleep(Duration::from_millis(500));
        }

        // Phase 2: Cache verification / populate (enrichment is inline).
        Self::phase_cache_verification(state);
    }

    /// Phase 1: Auto-import metadata on startup if `launchbox-metadata.xml` exists and DB is empty.
    fn phase_auto_import(state: &AppState) {
        use replay_control_core::metadata_db::LAUNCHBOX_XML;

        let storage = state.storage();
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

        let should_import = if let Some(guard) = state.metadata_db() {
            guard
                .as_ref()
                .and_then(|db| db.is_empty().ok())
                .unwrap_or(false)
        } else {
            false
        };

        if should_import {
            tracing::info!("Auto-importing metadata from {}", xml_path.display());
            state.import.start_import(xml_path, state.clone());
        }
    }

    /// Phase 2: Verify L2 cache freshness on startup and pre-populate if empty.
    fn phase_cache_verification(state: &AppState) {
        let storage = state.storage();
        let roms_dir = storage.roms_dir();
        let region_pref = state.region_preference();
        let region_secondary = state.region_preference_secondary();

        // Load all cached system metadata from L2.
        let cached_meta = {
            let guard = state.metadata_db();
            guard.and_then(|g| g.as_ref()?.load_all_system_meta().ok())
        };

        let cached_meta = cached_meta.unwrap_or_default();

        if cached_meta.is_empty() {
            // Fresh DB -- pre-populate L2 for all systems with games.
            // warmup_in_progress is already set by run_pipeline().
            Self::populate_all_systems(state, &storage, region_pref, region_secondary);
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
                (Some(_), None) => false, // Can't read -- trust cache
                (None, _) => true,        // No mtime stored -- re-scan
            };

            if is_stale {
                tracing::info!("Background re-scan: {} (mtime changed)", meta.system);
                let _ =
                    state
                        .cache
                        .scan_and_cache_system(&storage, &meta.system, region_pref, region_secondary);
                state.cache.enrich_system_cache(state, &meta.system);
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
    }

    /// Pre-populate L2 cache for all systems that have games.
    /// Called on startup when the game library is empty (fresh DB or after clear).
    /// After populating ROMs, enriches box art URLs and ratings.
    pub(crate) fn populate_all_systems(
        state: &AppState,
        storage: &replay_control_core::storage::StorageLocation,
        region_pref: replay_control_core::rom_tags::RegionPreference,
        region_secondary: Option<replay_control_core::rom_tags::RegionPreference>,
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
            match state
                .cache
                .scan_and_cache_system(storage, &sys.folder_name, region_pref, region_secondary)
            {
                Ok(roms) => {
                    tracing::debug!("L2 warmup: {} — {} ROMs", sys.folder_name, roms.len());
                    total_roms += roms.len();
                }
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
            "L2 warmup: done -- {} ROMs across {} systems in {:.1}s",
            total_roms,
            with_games.len(),
            start.elapsed().as_secs_f64()
        );
    }
}

// ── Methods that remain on AppState ────────────────────────────────
//
// These are the long-running watchers and the cache enrichment helper
// that various parts of the code still call on AppState.
impl AppState {
    /// Re-enrich game library for all systems after a metadata or thumbnail import.
    /// If game library is empty (e.g., DB was deleted and recreated during import),
    /// does a full populate first (scan ROMs + enrich). Otherwise just enriches
    /// existing entries with updated box art URLs and ratings.
    pub fn spawn_cache_enrichment(&self) {
        self.spawn_cache_enrichment_inner(None);
    }

    /// Like `spawn_cache_enrichment`, but clears an `AtomicBool` flag when the
    /// background work completes. Used by `rebuild_game_library` to signal that
    /// the metadata operation slot is free again.
    pub fn spawn_cache_enrichment_with_flag(
        &self,
        flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) {
        self.spawn_cache_enrichment_inner(Some(flag));
    }

    fn spawn_cache_enrichment_inner(
        &self,
        done_flag: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    ) {
        let state = self.clone();
        std::thread::spawn(move || {
            // Use catch_unwind to guarantee the done_flag is cleared even if
            // anything in the enrichment pipeline panics. Without this, a panic
            // leaves the busy flag stuck forever.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let storage = state.storage();
                let region_pref = state.region_preference();
                let region_secondary = state.region_preference_secondary();

                // Check if game library is empty -- if so, populate before enriching.
                let is_empty = state
                    .cache
                    .with_db_read(&storage, |db| {
                        db.load_all_system_meta()
                            .map(|m| m.is_empty())
                            .unwrap_or(true)
                    })
                    .unwrap_or(true);

                if is_empty {
                    tracing::info!("Post-import: game library is empty, running full populate");
                    BackgroundManager::populate_all_systems(
                        &state,
                        &storage,
                        region_pref,
                        region_secondary,
                    );
                } else {
                    let systems = state.cache.get_systems(&storage);
                    let with_games: Vec<_> =
                        systems.iter().filter(|s| s.game_count > 0).collect();
                    tracing::info!(
                        "Post-import enrichment: updating {} system(s)",
                        with_games.len()
                    );
                    let start = std::time::Instant::now();
                    for sys in &with_games {
                        state.cache.enrich_system_cache(&state, &sys.folder_name);
                    }
                    tracing::info!(
                        "Post-import enrichment: done in {:.1}s",
                        start.elapsed().as_secs_f64()
                    );
                }
            }));

            if let Err(panic) = result {
                let msg = panic
                    .downcast_ref::<String>()
                    .map(|s| s.as_str())
                    .or_else(|| panic.downcast_ref::<&str>().copied())
                    .unwrap_or("unknown panic");
                tracing::error!("Cache enrichment panicked: {msg}");
            }

            // Always clear the busy flag, even after a panic.
            if let Some(flag) = done_flag {
                flag.store(false, Ordering::SeqCst);
                tracing::debug!("Cache enrichment: cleared busy flag");
            }
        });
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
            // Skip the first (immediate) tick -- we just initialized.
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
    async fn try_start_config_watcher(state: AppState, config_path: std::path::PathBuf) -> bool {
        use notify::{RecursiveMode, Watcher, recommended_watcher};

        // Watch the parent directory -- the file itself may not exist yet, and
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
        // it into this task -- dropping it would stop watching.
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
                            // Timeout -- debounce window expired
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

    /// Spawn a filesystem watcher on the `roms/` directory for local storage.
    ///
    /// Only starts for local storage kinds (`Sd`, `Usb`, `Nvme`) where
    /// inotify works reliably. NFS is excluded because inotify does not
    /// detect changes made by other NFS clients. For NFS, users trigger
    /// rescans manually via the metadata page "Update" button.
    pub fn spawn_rom_watcher(&self) {
        let storage = self.storage();
        if !storage.kind.is_local() {
            tracing::debug!(
                "ROM watcher skipped for {:?} storage (inotify unreliable on NFS)",
                storage.kind
            );
            return;
        }

        let roms_dir = storage.roms_dir();
        if !roms_dir.exists() {
            tracing::debug!(
                "ROM watcher skipped: roms directory does not exist ({})",
                roms_dir.display()
            );
            return;
        }

        let state = self.clone();
        tokio::spawn(async move {
            let watcher_active = Self::try_start_rom_watcher(state, roms_dir).await;
            if watcher_active {
                tracing::info!("ROM directory watcher active");
            } else {
                tracing::warn!(
                    "ROM directory watcher could not be started; \
                     new ROMs will be detected on page visit or next restart"
                );
            }
        });
    }

    /// Try to set up a `notify` filesystem watcher on the `roms/` directory.
    /// Returns `true` if the watcher was started successfully.
    ///
    /// Watches recursively for create/modify/remove events. On change,
    /// extracts the affected system folder name from the event path and
    /// triggers `get_roms` + `enrich_system_cache` after a debounce window.
    ///
    /// When a top-level change is detected in the `roms/` directory itself
    /// (new system directory created), triggers a `get_systems` refresh.
    async fn try_start_rom_watcher(state: AppState, roms_dir: std::path::PathBuf) -> bool {
        use notify::{RecursiveMode, Watcher, recommended_watcher};

        let (tx, mut rx) = tokio::sync::mpsc::channel(64);

        let mut watcher =
            match recommended_watcher(move |res: Result<notify::Event, notify::Error>| match res {
                Ok(event) => {
                    let _ = tx.blocking_send(event);
                }
                Err(e) => {
                    tracing::warn!("ROM watcher error: {e}");
                }
            }) {
                Ok(w) => w,
                Err(e) => {
                    tracing::warn!("Failed to create ROM watcher: {e}");
                    return false;
                }
            };

        if let Err(e) = watcher.watch(&roms_dir, RecursiveMode::Recursive) {
            tracing::warn!(
                "Failed to watch roms directory {}: {e}",
                roms_dir.display()
            );
            return false;
        }

        tracing::info!("Watching {} for ROM changes", roms_dir.display());

        tokio::spawn(async move {
            let _watcher = watcher; // prevent drop

            // Debounce: batch rapid filesystem events (e.g., bulk copy) before
            // triggering a rescan. 3 seconds balances responsiveness vs thrashing.
            const DEBOUNCE: Duration = Duration::from_secs(3);

            loop {
                // Wait for the next event.
                let Some(event) = rx.recv().await else {
                    tracing::warn!("ROM watcher channel closed");
                    break;
                };

                if !Self::is_rom_event(&event) {
                    continue;
                }

                // Collect affected system folder names from this and subsequent
                // events within the debounce window.
                let mut affected_systems = std::collections::HashSet::new();
                let mut roms_dir_changed = false;
                Self::collect_rom_event_systems(
                    &event,
                    &roms_dir,
                    &mut affected_systems,
                    &mut roms_dir_changed,
                );

                tracing::debug!(
                    "ROM change detected ({:?}), debouncing...",
                    event.kind
                );

                // Drain further events within the debounce window.
                let deadline = tokio::time::Instant::now() + DEBOUNCE;
                loop {
                    match tokio::time::timeout_at(deadline, rx.recv()).await {
                        Ok(Some(ev)) => {
                            if Self::is_rom_event(&ev) {
                                Self::collect_rom_event_systems(
                                    &ev,
                                    &roms_dir,
                                    &mut affected_systems,
                                    &mut roms_dir_changed,
                                );
                            }
                        }
                        Ok(None) => break, // Channel closed
                        Err(_) => break,   // Debounce window expired
                    }
                }

                // Skip if startup warmup or a metadata operation is running.
                if state.cache.warmup_in_progress.load(Ordering::Acquire)
                    || state.import.is_busy()
                {
                    tracing::debug!(
                        "Background operation in progress, skipping ROM watcher rescan"
                    );
                    continue;
                }

                // Run the rescan in a blocking thread to avoid blocking the
                // async event loop.
                let state_clone = state.clone();
                let affected = affected_systems.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    let storage = state_clone.storage();
                    let region_pref = state_clone.region_preference();
                    let region_secondary = state_clone.region_preference_secondary();

                    // Invalidate L1+L2 for each affected system so get_roms
                    // does a fresh L3 filesystem scan.
                    for system in &affected {
                        state_clone.cache.invalidate_system(system);
                    }

                    // Re-scan each affected system.
                    if !affected.is_empty() {
                        tracing::info!(
                            "ROM watcher: re-scanning {} system(s): {}",
                            affected.len(),
                            affected.iter().cloned().collect::<Vec<_>>().join(", ")
                        );
                        for system in &affected {
                            let _ = state_clone.cache.scan_and_cache_system(
                                &storage,
                                system,
                                region_pref,
                                region_secondary,
                            );
                            state_clone.cache.enrich_system_cache(&state_clone, system);
                        }
                    }

                    // If the roms/ directory itself changed (new subdirectory
                    // created or removed), refresh the systems list to discover
                    // new systems and update game counts.
                    if roms_dir_changed {
                        tracing::info!(
                            "ROM watcher: roms/ directory changed, refreshing systems"
                        );
                        let systems = state_clone.cache.get_systems(&storage);
                        for sys in &systems {
                            if sys.game_count > 0 && !affected.contains(&sys.folder_name) {
                                let _ = state_clone.cache.scan_and_cache_system(
                                    &storage,
                                    &sys.folder_name,
                                    region_pref,
                                    region_secondary,
                                );
                                state_clone
                                    .cache
                                    .enrich_system_cache(&state_clone, &sys.folder_name);
                            }
                        }
                    } else if !affected.is_empty() {
                        let _ = state_clone.cache.get_systems(&storage);
                    }
                })
                .await;
            }
        });

        true
    }

    /// Check whether a notify event is relevant to ROM files/directories.
    fn is_rom_event(event: &notify::Event) -> bool {
        use notify::EventKind;

        matches!(
            event.kind,
            EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
        )
    }

    /// Extract system folder names from event paths and detect top-level
    /// roms/ directory changes.
    fn collect_rom_event_systems(
        event: &notify::Event,
        roms_dir: &std::path::Path,
        affected_systems: &mut std::collections::HashSet<String>,
        roms_dir_changed: &mut bool,
    ) {
        for path in &event.paths {
            let relative = match path.strip_prefix(roms_dir) {
                Ok(rel) => rel,
                Err(_) => continue,
            };

            // Get the first path component (the system folder name).
            let mut components = relative.components();
            let Some(first) = components.next() else {
                // Event on roms/ directory itself.
                *roms_dir_changed = true;
                continue;
            };

            let system_name = first.as_os_str().to_string_lossy();

            // Skip internal directories (e.g., _favorites, _recent).
            if system_name.starts_with('_') {
                continue;
            }

            // If the event path has only one component (no further child),
            // it's a direct child of roms/ -- either a new system directory
            // was created or an entry was removed.
            if components.next().is_none() {
                *roms_dir_changed = true;
            }

            affected_systems.insert(system_name.into_owned());
        }
    }
}
