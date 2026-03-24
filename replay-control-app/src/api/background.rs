use replay_control_core::metadata_db::MetadataDb;
use std::time::Duration;

use super::AppState;
use super::activity::{Activity, StartupPhase};
use super::cache::dir_mtime;
use super::import::ImportPipeline;

/// How often the background task re-checks storage (in seconds).
const STORAGE_CHECK_INTERVAL: u64 = 60;

/// Orchestrates the ordered background startup pipeline and long-running watchers.
///
/// Pipeline phases (sequential, async):
///   1. Auto-import — if a LaunchBox XML file exists and the DB is empty
///   2. Cache populate/verify — scan all systems, enrich box art + ratings
///   3. Auto-rebuild thumbnail index — if data_sources exist but index is empty (data loss)
///
/// Filesystem watchers (config file, ROM directory) run independently.
pub struct BackgroundManager;

impl BackgroundManager {
    /// Start the ordered background pipeline.
    pub fn start(state: AppState) {
        // Spawn the ordered pipeline as an async task.
        let pipeline_state = state.clone();
        tokio::spawn(async move {
            Self::run_pipeline(&pipeline_state).await;
        });

        // Start watchers immediately (they're independent of the pipeline).
        state.clone().spawn_storage_watcher();
        state.spawn_rom_watcher();
    }

    /// Run the ordered startup pipeline (async).
    async fn run_pipeline(state: &AppState) {
        // Brief delay to let the server start accepting requests.
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Phase 1: Auto-import (if launchbox XML exists + DB empty).
        // Import claims/releases its own Activity::Import via try_start_activity.
        Self::phase_auto_import(state).await;

        // Wait for auto-import to finish (check activity state).
        while ImportPipeline::has_active_import(state) {
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        // Phase 2+3: Claim Activity::Startup for populate + thumbnail rebuild.
        // Guard drops → Idle on completion or panic.
        {
            let _guard = match state.try_start_activity(Activity::Startup {
                phase: StartupPhase::Scanning,
                system: String::new(),
            }) {
                Ok(g) => g,
                Err(e) => {
                    tracing::warn!("Could not start startup pipeline: {e}");
                    return;
                }
            };

            Self::phase_cache_verification(state).await;
            // Checkpoint after Phase 2 writes (game_library inserts/updates).
            state.metadata_pool.checkpoint().await;

            state.update_activity(|act| {
                if let Activity::Startup { phase, .. } = act {
                    *phase = StartupPhase::RebuildingIndex;
                }
            });
            Self::phase_auto_rebuild_thumbnail_index(state).await;

            // _guard drops → Idle
        }
    }

    /// Phase 1: Auto-import metadata on startup if `launchbox-metadata.xml` exists and DB is empty.
    async fn phase_auto_import(state: &AppState) {
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

        let should_import = state
            .metadata_pool
            .read(|conn| MetadataDb::is_empty(conn).unwrap_or(false))
            .await
            .unwrap_or(false);

        if should_import {
            tracing::info!("Auto-importing metadata from {}", xml_path.display());
            state.import.start_import_no_enrich(xml_path, state.clone());
        }
    }

    /// Phase 2: Verify L2 cache freshness on startup and pre-populate if empty.
    async fn phase_cache_verification(state: &AppState) {
        let storage = state.storage();
        let roms_dir = storage.roms_dir();
        let region_pref = state.region_preference();
        let region_secondary = state.region_preference_secondary();

        // Load all cached system metadata from L2.
        let cached_meta = state
            .metadata_pool
            .read(|conn| MetadataDb::load_all_system_meta(conn).ok())
            .await
            .flatten();

        let cached_meta = cached_meta.unwrap_or_default();

        if cached_meta.is_empty() {
            // Fresh DB -- pre-populate L2 for all systems with games.
            // Activity is already Startup { phase: Scanning } (set by run_pipeline).
            Self::populate_all_systems(state, &storage, region_pref, region_secondary).await;
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
                let _ = state
                    .cache
                    .scan_and_cache_system(&storage, &meta.system, region_pref, region_secondary)
                    .await;
                state.cache.enrich_system_cache(state, meta.system.clone()).await;
                stale_count += 1;
            }
        }

        if stale_count > 0 {
            tracing::info!(
                "Background cache verification: re-scanned {stale_count} stale system(s)"
            );
            // Also refresh the systems list since counts may have changed.
            let _ = state.cache.get_systems(&storage).await;
        } else {
            tracing::debug!(
                "Background cache verification: all {} system(s) fresh",
                cached_meta.len()
            );
        }
    }

    /// Phase 3: Rebuild thumbnail index if there's evidence of data loss.
    ///
    /// Triggers when `data_sources` has libretro-thumbnails entries (meaning the user
    /// previously ran "Update Thumbnails") but `thumbnail_index` is empty (data lost,
    /// e.g., due to DB corruption and auto-recreate). Does NOT download images — only
    /// rebuilds the index so box art variant picker and on-demand downloads work.
    ///
    /// Skips when both tables are empty (first-time setup — user hasn't configured
    /// thumbnails yet) to avoid wasting time on GitHub API calls when offline.
    async fn phase_auto_rebuild_thumbnail_index(state: &AppState) {
        // Check data_sources for libretro-thumbnails entries and thumbnail_index emptiness.
        let (has_sources, index_empty) = match state
            .metadata_pool
            .read(|conn| {
                let stats = MetadataDb::get_data_source_stats(conn, "libretro-thumbnails").ok()?;
                let index_count: i64 = MetadataDb::thumbnail_index_count(conn).unwrap_or(0);
                Some((stats.repo_count > 0, index_count == 0))
            })
            .await
            .flatten()
        {
            Some(result) => result,
            None => return, // DB unavailable
        };

        if !has_sources {
            // No data_sources entries. Check if images exist on disk — if so,
            // someone previously downloaded thumbnails but the DB was deleted.
            let has_images_on_disk =
                replay_control_core::thumbnails::any_images_on_disk(&state.storage().rc_dir());
            if !has_images_on_disk {
                tracing::debug!(
                    "No libretro-thumbnails data sources and no images on disk, skipping thumbnail index rebuild"
                );
                return;
            }
            tracing::info!(
                "Fresh DB but images exist on disk — rebuilding thumbnail index from GitHub API"
            );
        } else if !index_empty {
            tracing::debug!("Thumbnail index already populated, skipping rebuild");
            return;
        } else {
            tracing::info!(
                "Thumbnail data sources exist but index is empty (data loss?) — rebuilding index from GitHub API"
            );
        }

        // Rebuild index from images on disk — no GitHub API needed.
        // Scan media/<system>/boxart/ directories and insert filenames into thumbnail_index.
        let storage = state.storage();
        let media_dir = storage.rc_dir().join("media");

        let Ok(systems) = std::fs::read_dir(&media_dir) else {
            return;
        };

        // Collect all system image data from disk first (no DB needed).
        struct SystemImageData {
            system_str: String,
            repo_names: &'static [&'static str],
            entries: Vec<(String, String, Option<String>)>,
        }

        let mut system_data: Vec<SystemImageData> = Vec::new();
        for system_entry in systems.flatten() {
            let system_name = system_entry.file_name();
            let system_str = system_name.to_string_lossy().into_owned();

            let Some(repo_names) =
                replay_control_core::thumbnails::thumbnail_repo_names(&system_str)
            else {
                continue;
            };

            let all_entries =
                replay_control_core::thumbnails::scan_system_images(&system_entry.path());

            if all_entries.is_empty() {
                continue;
            }

            system_data.push(SystemImageData {
                system_str,
                repo_names,
                entries: all_entries,
            });
        }

        // Now write all collected data to the DB in a single write() call.
        let write_result = state
            .metadata_pool
            .write(move |db| {
                let mut w_total_entries = 0usize;
                let mut w_total_repos = 0usize;

                for data in &system_data {
                    let repo_display = data.repo_names[0];
                    let source_name =
                        replay_control_core::thumbnails::libretro_source_name(repo_display);
                    let branch =
                        replay_control_core::thumbnail_manifest::default_branch(repo_display);
                    let entry_count = data.entries.len();

                    let _ = MetadataDb::upsert_data_source(
                        db,
                        &source_name,
                        "libretro-thumbnails",
                        "disk-rebuild",
                        branch,
                        entry_count,
                    );

                    match MetadataDb::bulk_insert_thumbnail_index(db, &source_name, &data.entries) {
                        Ok(_) => w_total_entries += entry_count,
                        Err(e) => tracing::warn!(
                            "Failed to insert disk-based index for {}: {e}",
                            data.system_str
                        ),
                    }

                    // Register additional repos for multi-repo systems (e.g., arcade_dc → Naomi + Naomi 2).
                    for extra_repo in &data.repo_names[1..] {
                        let extra_source =
                            replay_control_core::thumbnails::libretro_source_name(extra_repo);
                        let extra_branch =
                            replay_control_core::thumbnail_manifest::default_branch(extra_repo);
                        let _ = MetadataDb::upsert_data_source(
                            db,
                            &extra_source,
                            "libretro-thumbnails",
                            "disk-rebuild",
                            extra_branch,
                            0,
                        );
                    }
                    w_total_repos += data.repo_names.len();
                }

                (w_total_entries, w_total_repos)
            })
            .await;

        let Some((total_entries, total_repos)) = write_result else {
            return; // DB unavailable
        };

        if total_entries > 0 {
            // Checkpoint WAL after the bulk thumbnail index writes.
            state.metadata_pool.checkpoint().await;
            tracing::info!(
                "Thumbnail index rebuilt from disk: {total_entries} entries across {total_repos} repos"
            );
        }
    }

    /// Pre-populate L2 cache for all systems that have games.
    /// Called on startup when the game library is empty (fresh DB or after clear).
    /// After populating ROMs, enriches box art URLs and ratings.
    pub(crate) async fn populate_all_systems(
        state: &AppState,
        storage: &replay_control_core::storage::StorageLocation,
        region_pref: replay_control_core::rom_tags::RegionPreference,
        region_secondary: Option<replay_control_core::rom_tags::RegionPreference>,
    ) {
        let systems = state.cache.get_systems(storage).await;
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
                .await
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
            state
                .cache
                .enrich_system_cache(state, sys.folder_name.clone())
                .await;
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
        let state = self.clone();
        tokio::spawn(async move {
            let storage = state.storage();
            let region_pref = state.region_preference();
            let region_secondary = state.region_preference_secondary();

            // Check if game library is empty -- if so, populate before enriching.
            let is_empty = state
                .cache
                .db_read(|conn| {
                    MetadataDb::load_all_system_meta(conn)
                        .map(|m| m.is_empty())
                        .unwrap_or(true)
                })
                .await
                .unwrap_or(true);

            if is_empty {
                tracing::info!("Post-import: game library is empty, running full populate");
                BackgroundManager::populate_all_systems(
                    &state,
                    &storage,
                    region_pref,
                    region_secondary,
                )
                .await;
            }

            // Enrichment phase: update box art URLs and ratings for all systems.
            let systems = state.cache.get_systems(&storage).await;
            let with_games: Vec<_> = systems.into_iter().filter(|s| s.game_count > 0).collect();

            if !with_games.is_empty() {
                tracing::info!(
                    "Post-import enrichment: updating {} system(s)",
                    with_games.len()
                );
                let enrich_start = std::time::Instant::now();
                for sys in &with_games {
                    state
                        .cache
                        .enrich_system_cache(&state, sys.folder_name.clone())
                        .await;
                }
                tracing::info!(
                    "Post-import enrichment: done in {:.1}s",
                    enrich_start.elapsed().as_secs_f64()
                );
            }
        });
    }

    /// Run cache enrichment as part of a rebuild operation (with an ActivityGuard).
    /// Updates `Activity::Rebuild` progress as it goes. The guard drops → Idle on completion.
    pub fn spawn_rebuild_enrichment(&self, guard: super::activity::ActivityGuard) {
        use super::activity::RebuildPhase;

        let state = self.clone();
        let start = std::time::Instant::now();

        tokio::spawn(async move {
            let storage = state.storage();
            let region_pref = state.region_preference();
            let region_secondary = state.region_preference_secondary();

            // Check if game library is empty -- if so, populate before enriching.
            let is_empty = state
                .cache
                .db_read(|conn| {
                    MetadataDb::load_all_system_meta(conn)
                        .map(|m| m.is_empty())
                        .unwrap_or(true)
                })
                .await
                .unwrap_or(true);

            if is_empty {
                tracing::info!("Rebuild: game library is empty, running full populate");
                state.update_activity(|act| {
                    if let Activity::Rebuild { progress } = act {
                        progress.phase = RebuildPhase::Scanning;
                        progress.elapsed_secs = start.elapsed().as_secs();
                    }
                });
                BackgroundManager::populate_all_systems(
                    &state,
                    &storage,
                    region_pref,
                    region_secondary,
                )
                .await;
            }

            // Enrichment phase: update box art URLs and ratings for all systems.
            let systems = state.cache.get_systems(&storage).await;
            let with_games: Vec<_> = systems.into_iter().filter(|s| s.game_count > 0).collect();

            state.update_activity(|act| {
                if let Activity::Rebuild { progress } = act {
                    progress.phase = RebuildPhase::Enriching;
                    progress.current_system = String::new();
                    progress.systems_done = 0;
                    progress.systems_total = with_games.len();
                    progress.elapsed_secs = start.elapsed().as_secs();
                }
            });

            if !with_games.is_empty() {
                tracing::info!(
                    "Rebuild enrichment: updating {} system(s)",
                    with_games.len()
                );
                let enrich_start = std::time::Instant::now();
                for (i, sys) in with_games.iter().enumerate() {
                    state.update_activity(|act| {
                        if let Activity::Rebuild { progress } = act {
                            progress.current_system = sys.display_name.clone();
                            progress.systems_done = i;
                            progress.elapsed_secs = start.elapsed().as_secs();
                        }
                    });
                    state
                        .cache
                        .enrich_system_cache(&state, sys.folder_name.clone())
                        .await;
                }
                tracing::info!(
                    "Rebuild enrichment: done in {:.1}s",
                    enrich_start.elapsed().as_secs_f64()
                );
            }

            // Mark rebuild complete (terminal state).
            state.update_activity(|act| {
                if let Activity::Rebuild { progress } = act {
                    progress.phase = RebuildPhase::Complete;
                    progress.current_system = String::new();
                    progress.systems_done = with_games.len();
                    progress.systems_total = with_games.len();
                    progress.elapsed_secs = start.elapsed().as_secs();
                    progress.error = None;
                }
            });

            // guard drops here → Idle
            drop(guard);
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
                match state.refresh_storage().await {
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
                match state.refresh_storage().await {
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
            tracing::warn!("Failed to watch roms directory {}: {e}", roms_dir.display());
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

                tracing::debug!("ROM change detected ({:?}), debouncing...", event.kind);

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

                // Skip if any activity is running (startup, import, etc.).
                if !state.is_idle() {
                    tracing::debug!(
                        "Background operation in progress, skipping ROM watcher rescan"
                    );
                    continue;
                }

                // Run the rescan as an async task.
                let storage = state.storage();
                let region_pref = state.region_preference();
                let region_secondary = state.region_preference_secondary();

                // Invalidate L1+L2 for each affected system so get_roms
                // does a fresh L3 filesystem scan.
                for system in &affected_systems {
                    state.cache.invalidate_system(system.clone()).await;
                }

                // Re-scan each affected system.
                if !affected_systems.is_empty() {
                    tracing::info!(
                        "ROM watcher: re-scanning {} system(s): {}",
                        affected_systems.len(),
                        affected_systems
                            .iter()
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                    for system in &affected_systems {
                        let _ = state
                            .cache
                            .scan_and_cache_system(&storage, system, region_pref, region_secondary)
                            .await;
                        state.cache.enrich_system_cache(&state, system.clone()).await;
                    }
                }

                // If the roms/ directory itself changed (new subdirectory
                // created or removed), refresh the systems list to discover
                // new systems and update game counts.
                if roms_dir_changed {
                    tracing::info!("ROM watcher: roms/ directory changed, refreshing systems");
                    let systems = state.cache.get_systems(&storage).await;
                    for sys in &systems {
                        if sys.game_count > 0 && !affected_systems.contains(&sys.folder_name) {
                            let _ = state
                                .cache
                                .scan_and_cache_system(
                                    &storage,
                                    &sys.folder_name,
                                    region_pref,
                                    region_secondary,
                                )
                                .await;
                            state
                                .cache
                                .enrich_system_cache(&state, sys.folder_name.clone())
                                .await;
                        }
                    }
                } else if !affected_systems.is_empty() {
                    let _ = state.cache.get_systems(&storage).await;
                }
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
