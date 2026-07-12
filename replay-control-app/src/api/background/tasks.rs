//! Startup orchestration and device watchers.
//!
//! Free functions over `&AppState`: boot-task spawning, populate/enrichment
//! kickoff, and the storage/ROM `notify` watchers. The heavier pipeline /
//! identity / metadata function bodies live in the parent `background` module;
//! this layer drives them via their `pub(crate)` entry points.

use replay_control_core_server::library_db::LibraryDb;
use replay_control_core_server::update as update_io;
use std::sync::atomic::Ordering;
use std::time::Duration;

use super::{
    IdentityJob, PopulateProgress, is_hash_identifiable, try_read_or_skip, update_rebuild_progress,
};
use crate::api::AppState;
use crate::api::activity::RebuildPhase;
use crate::api::library::ScanOptions;
use crate::types::RomWatcherStatus;

/// Re-enrich game library after a metadata or thumbnail import.
///
/// If the library is empty (e.g. fresh DB), `populate_all_systems`
/// runs scan + inline enrich for every visible system; nothing more
/// is needed.
///
/// Otherwise the per-system enrich loop is the *primary* job here —
/// it's the path that picks up newly-imported external_metadata for
/// already-stored systems.
pub fn spawn_library_enrichment(state: &AppState) {
    let state = state.clone();
    tokio::spawn(async move {
        let storage = state.storage();
        let region_pref = state.region_preference();
        let region_secondary = state.region_preference_secondary();

        let Some(is_empty) = try_read_or_skip(&state.library_reader, "post_import", |conn| {
            LibraryDb::load_all_system_meta(conn).map(|m| m.is_empty())
        })
        .await
        else {
            return;
        };

        if is_empty {
            tracing::info!("Post-import: game library is empty, running full populate");
            match super::populate_all_systems(
                &state,
                &storage,
                region_pref,
                region_secondary,
                PopulateProgress::Startup,
                ScanOptions {
                    force_rehash: false,
                    skip_unchanged_startup: true,
                },
                state.storage_generation(),
            )
            .await
            {
                Ok(()) => {}
                Err(e) if super::is_storage_changed(&e) => {
                    tracing::info!("Post-import populate cancelled because storage changed");
                }
                Err(e) => tracing::warn!("Post-import populate failed: {e}"),
            }
        } else {
            // Enrichment-only re-pass: pick up the newly-imported
            // external_metadata for already-stored systems. NOT
            // gated because enrich_system_library reads from the DB
            // and the write gate blocks ALL reads on the same pool.
            // Enrichment writes are small per-system UPDATEs.
            let with_games = crate::api::library_systems::active_systems(
                &state.library_reader,
                "post_import_enrichment",
            )
            .await;

            if !with_games.is_empty() {
                tracing::info!(
                    "Post-import enrichment: updating {} system(s)",
                    with_games.len()
                );
                let enrich_start = std::time::Instant::now();
                for system in with_games {
                    state.library.enrich_system_library(&state, system).await;
                }
                tracing::info!(
                    "Post-import enrichment: done in {:.1}s",
                    enrich_start.elapsed().as_secs_f64()
                );
            }
        }
    });
}

/// Strict-reconcile every visible system in place (preserving stored
/// rows on per-system FS errors), then mark complete. Inline enrichment
/// is part of `populate_all_systems`, so there's no separate post-loop
/// enrichment pass. Rescan and rebuild share this body; the `is_rescan`
/// flag on `RebuildProgress` (set by the calling server fn before
/// spawning) only changes UI verbiage.
pub fn spawn_populate(
    state: &AppState,
    guard: crate::api::activity::ActivityGuard,
    scan_hint: bool,
) {
    let state = state.clone();
    let start = std::time::Instant::now();

    tokio::spawn(async move {
        let storage = state.storage();
        let region_pref = state.region_preference();
        let region_secondary = state.region_preference_secondary();

        // Walking ROM directories on a slow NFS share can take seconds
        // to minutes before per-system progress starts firing.
        if scan_hint {
            update_rebuild_progress(&state, |p| {
                p.phase = RebuildPhase::Scanning;
                p.current_system = "scanning ROM directories".to_string();
                p.elapsed_secs = start.elapsed().as_secs();
            });
        }

        let populate_result = super::populate_all_systems(
            &state,
            &storage,
            region_pref,
            region_secondary,
            PopulateProgress::Rebuild { start },
            ScanOptions {
                force_rehash: !scan_hint,
                skip_unchanged_startup: false,
            },
            state.storage_generation(),
        )
        .await;

        match populate_result {
            Ok(()) => {
                update_rebuild_progress(&state, |p| {
                    p.phase = RebuildPhase::Complete;
                    p.current_system = String::new();
                    p.systems_done = p.systems_total;
                    p.elapsed_secs = start.elapsed().as_secs();
                    p.error = None;
                });
            }
            Err(e) if super::is_storage_changed(&e) => {
                tracing::info!("Populate cancelled because storage changed");
                update_rebuild_progress(&state, |p| {
                    p.phase = RebuildPhase::Cancelled;
                    p.current_system = String::new();
                    p.elapsed_secs = start.elapsed().as_secs();
                    p.error = None;
                });
            }
            Err(e) => {
                tracing::warn!("Populate failed: {e}");
                update_rebuild_progress(&state, |p| {
                    p.phase = RebuildPhase::Failed;
                    p.elapsed_secs = start.elapsed().as_secs();
                    p.error = Some(e.to_string());
                });
            }
        }

        drop(guard);
    });
}

/// Spawn every long-running background task at boot (or when storage first
/// appears): the startup library pipeline, the storage + ROM watchers, the
/// update checker, the now-playing detector, and RePlayOS API maintenance.
/// App-startup orchestration; the parent `background` module provides the
/// individual task bodies.
pub fn spawn_boot_tasks(state: &AppState) {
    // Clean up stale update temp files from a previous run.
    update_io::nuke_update_dir();

    state.spawn_startup_pipeline();

    // Watchers are independent of the pipeline.
    spawn_storage_watcher(state.clone());
    spawn_rom_watcher(state);

    // Update checker (independent of the pipeline, no activity lock needed).
    let update_state = state.clone();
    tokio::spawn(async move {
        crate::api::updates::update_check_loop(update_state).await;
    });

    // Now-playing detector loop. API-based: exits immediately in standalone
    // mode, where `replay_api` is `None`.
    let now_playing_state = state.clone();
    tokio::spawn(async move {
        crate::api::now_playing::run_now_playing_loop(now_playing_state).await;
    });

    // RePlayOS API integration: startup probe + self-recovery maintenance.
    // No-ops immediately in standalone mode (`replay_api` is None).
    let replay_api_state = state.clone();
    tokio::spawn(async move {
        crate::api::replay_api::run_replay_api_maintenance(replay_api_state).await;
    });
}

/// Spawn the device-side storage-detection watchers: a `notify` watcher
/// on `replay.cfg` (config-file edits) and a `/proc/self/mountinfo`
/// POLLPRI watcher (mount-table changes — Linux only). Standalone has
/// nothing to watch: replay.cfg is RePlayOS-owned and irrelevant
/// off-device, and the `--storage-path` folder's liveness surfaces at
/// request time via IO errors. The mountinfo watcher does a full
/// `reload_config_and_redetect_storage` so the boot-recovery case
/// (booted `ConfigUnavailable`, then the SD appears) adopts the new
/// `replay.cfg` along with the storage change.
pub fn spawn_storage_watcher(state: AppState) {
    if !state.mode.is_device() {
        return;
    }

    crate::api::mountinfo_watcher::spawn(state.clone());

    let config_path = state.config_file_path();
    tokio::spawn(async move {
        if try_start_config_watcher(state, config_path).await {
            tracing::info!("Config file watcher active");
        } else {
            tracing::warn!(
                "Config file watcher unavailable; replay.cfg edits will only \
                     be picked up via mount-table events or user-triggered refresh"
            );
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

            if !is_config_event(&event, &config_filename) {
                continue;
            }

            tracing::debug!("Config change detected ({:?}), debouncing...", event.kind);

            // Drain any further events that arrive within the debounce window.
            let deadline = tokio::time::Instant::now() + DEBOUNCE;
            loop {
                match tokio::time::timeout_at(deadline, rx.recv()).await {
                    Ok(Some(ev)) => {
                        if is_config_event(&ev, &config_filename) {
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

            tracing::info!("Config file changed; reloading config and re-detecting storage");
            match state.reload_config_and_redetect_storage().await {
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
pub fn spawn_rom_watcher(state: &AppState) {
    let generation = state.rom_watcher_generation.load(Ordering::Relaxed);
    let storage = state.storage();
    if !storage.kind.is_local() {
        let reason = format!(
            "Filesystem events are unreliable on {} storage; trigger manual rescan after copying ROMs",
            storage.kind.as_str().to_uppercase()
        );
        tracing::debug!("ROM watcher skipped: {reason}");
        state.set_rom_watcher_status(RomWatcherStatus::Skipped { reason });
        return;
    }

    let roms_dir = storage.roms_dir();
    if !roms_dir.exists() {
        let reason = format!("ROM directory not found at {}", roms_dir.display());
        tracing::debug!("ROM watcher skipped: {reason}");
        state.set_rom_watcher_status(RomWatcherStatus::Skipped { reason });
        return;
    }

    let state = state.clone();
    tokio::spawn(async move {
        match try_start_rom_watcher(state.clone(), roms_dir, generation).await {
            Ok(()) => {
                tracing::info!("ROM directory watcher active");
                state.set_rom_watcher_status(RomWatcherStatus::Active);
            }
            Err(reason) => {
                tracing::warn!(
                    "ROM directory watcher could not be started ({reason}); \
                         use manual rescan or restart to detect new ROMs"
                );
                state.set_rom_watcher_status(RomWatcherStatus::Failed { reason });
            }
        }
    });
}

/// Stop any existing local ROM watcher and start one for the current
/// storage if that storage supports inotify. Does not touch the storage
/// watcher, update checker, or now-playing detector.
pub fn restart_rom_watcher(state: &AppState) {
    state.rom_watcher_generation.fetch_add(1, Ordering::Relaxed);
    spawn_rom_watcher(state);
}

/// Try to set up a `notify` filesystem watcher on the `roms/` directory.
/// Returns an error string if the watcher could not be created or registered.
///
/// Watches recursively for create/modify/remove events. On change,
/// extracts the affected system folder name from the event path and
/// triggers `get_roms` + `enrich_system_library` after a debounce window.
///
/// When a top-level change is detected in the `roms/` directory itself
/// (new system directory created), triggers a `get_systems` refresh.
async fn try_start_rom_watcher(
    state: AppState,
    roms_dir: std::path::PathBuf,
    generation: u64,
) -> Result<(), String> {
    use notify::{RecursiveMode, Watcher, recommended_watcher};

    let (tx, mut rx) = tokio::sync::mpsc::channel(64);

    let mut watcher =
        recommended_watcher(move |res: Result<notify::Event, notify::Error>| match res {
            Ok(event) => {
                let _ = tx.blocking_send(event);
            }
            Err(e) => {
                tracing::warn!("ROM watcher error: {e}");
            }
        })
        .map_err(|e| format!("Could not create filesystem watcher: {e}"))?;

    watcher
        .watch(&roms_dir, RecursiveMode::Recursive)
        .map_err(|e| {
            format!(
                "Could not watch {} (likely inotify max_user_watches exhausted): {e}",
                roms_dir.display()
            )
        })?;

    tracing::info!("Watching {} for ROM changes", roms_dir.display());

    tokio::spawn(async move {
        let _watcher = watcher; // prevent drop

        // Debounce: batch rapid filesystem events (e.g., bulk copy) before
        // triggering a rescan. 3 seconds balances responsiveness vs thrashing.
        const DEBOUNCE: Duration = Duration::from_secs(3);

        loop {
            if state.rom_watcher_generation.load(Ordering::Relaxed) != generation {
                tracing::info!("ROM watcher generation changed; stopping old watcher");
                break;
            }

            // Wait for the next event.
            let event = tokio::select! {
                event = rx.recv() => {
                    let Some(event) = event else {
                        tracing::warn!("ROM watcher channel closed");
                        break;
                    };
                    event
                }
                _ = tokio::time::sleep(Duration::from_secs(1)) => {
                    continue;
                }
            };

            if !is_rom_event(&event) {
                continue;
            }

            // Collect affected system folder names from this and subsequent
            // events within the debounce window.
            let mut affected_systems = std::collections::HashSet::new();
            let mut roms_dir_changed = false;
            let mut favorites_changed = false;
            let mut recents_changed = false;
            collect_rom_event_systems(
                &event,
                &roms_dir,
                &mut affected_systems,
                &mut roms_dir_changed,
                &mut favorites_changed,
                &mut recents_changed,
            );

            tracing::debug!("ROM change detected ({:?}), debouncing...", event.kind);

            // Drain further events within the debounce window.
            let deadline = tokio::time::Instant::now() + DEBOUNCE;
            loop {
                match tokio::time::timeout_at(deadline, rx.recv()).await {
                    Ok(Some(ev)) => {
                        if is_rom_event(&ev) {
                            collect_rom_event_systems(
                                &ev,
                                &roms_dir,
                                &mut affected_systems,
                                &mut roms_dir_changed,
                                &mut favorites_changed,
                                &mut recents_changed,
                            );
                        }
                    }
                    Ok(None) => break, // Channel closed
                    Err(_) => break,   // Debounce window expired
                }
            }

            // Favorites/recents invalidation is cheap (just clears the
            // cache; the next request rebuilds). Do it regardless of
            // whether a background scan is in progress, and regardless of
            // whether any ROM-file systems were affected in this batch.
            if favorites_changed {
                tracing::debug!("ROM watcher: _favorites/ changed, invalidating cache");
                state.library.invalidate_favorites().await;
                state.invalidate_user_caches().await;
            }
            if recents_changed {
                tracing::debug!("ROM watcher: _recent/ changed, invalidating cache");
                state.library.invalidate_after_launch().await;
            }

            // Skip the heavier system rescan if any activity is running
            // (startup, import, etc.).
            if !state.is_idle() {
                if !affected_systems.is_empty() || roms_dir_changed {
                    tracing::debug!(
                        "Background operation in progress, skipping ROM watcher rescan"
                    );
                }
                continue;
            }

            // Run the rescan as an async task.
            let storage = state.storage();
            let storage_generation = state.storage_generation();
            let region_pref = state.region_preference();
            let region_secondary = state.region_preference_secondary();

            let mut to_scan: Vec<String> = affected_systems.iter().cloned().collect();
            if roms_dir_changed {
                tracing::info!("ROM watcher: roms/ directory changed, refreshing systems");
                for sys in replay_control_core::systems::visible_systems() {
                    if !affected_systems.contains(sys.folder_name) {
                        to_scan.push(sys.folder_name.to_string());
                    }
                }
            } else if !affected_systems.is_empty() {
                tracing::info!(
                    "ROM watcher: re-scanning {} system(s): {}",
                    to_scan.len(),
                    to_scan.join(", ")
                );
            }

            if !to_scan.is_empty() {
                state.library.invalidate_in_memory_views().await;
                state.invalidate_user_caches().await;
            }

            for system in &to_scan {
                let scan_inputs = match super::scan_inputs_for_system(
                    &state,
                    system,
                    ScanOptions {
                        force_rehash: false,
                        skip_unchanged_startup: false,
                    },
                    storage_generation,
                )
                .await
                {
                    Ok(inputs) => inputs,
                    Err(e) if super::is_storage_changed(&e) => {
                        tracing::info!("ROM watcher: storage changed, cancelling rescan");
                        break;
                    }
                    Err(e) => {
                        tracing::warn!("ROM watcher: could not prepare scan for {system}: {e}");
                        continue;
                    }
                };
                match state
                    .library
                    .scan_and_reconcile_system_with_inputs(
                        &storage,
                        system,
                        region_pref,
                        region_secondary,
                        &state.library_writer,
                        &scan_inputs,
                    )
                    .await
                {
                    Ok(outcome) => {
                        let roms = outcome.roms;
                        if state.ensure_storage_generation(storage_generation).is_err() {
                            tracing::info!("ROM watcher: storage changed before enrichment");
                            break;
                        }
                        match state
                            .library
                            .enrich_system_library_with_cancellation(
                                &state,
                                system.clone(),
                                scan_inputs.cancellation(),
                            )
                            .await
                        {
                            Ok(()) => {}
                            Err(e) if super::is_storage_changed(&e) => {
                                tracing::info!("ROM watcher: storage changed during enrichment");
                                break;
                            }
                            Err(e) => {
                                tracing::warn!("ROM watcher: enrichment failed for {system}: {e}")
                            }
                        }
                        if !roms.is_empty() && is_hash_identifiable(system) {
                            super::spawn_identity_jobs(
                                state.clone(),
                                storage.clone(),
                                vec![IdentityJob {
                                    system: system.clone(),
                                    roms,
                                    scan_inputs: scan_inputs.clone(),
                                }],
                                storage_generation,
                            );
                        }
                    }
                    Err(e) if super::is_storage_changed(&e) => {
                        tracing::info!("ROM watcher: storage changed, cancelling rescan");
                        break;
                    }
                    Err(e) => tracing::warn!(
                        "ROM watcher: scan failed for {system}, preserving stored state: {e}"
                    ),
                }
            }

            if !to_scan.is_empty() {
                state.library.invalidate_in_memory_views().await;
                state.invalidate_user_caches().await;
            }
        }
    });

    Ok(())
}

/// Check whether a notify event is relevant to ROM files/directories.
fn is_rom_event(event: &notify::Event) -> bool {
    use notify::EventKind;

    matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    )
}

/// Extract system folder names from event paths, detect top-level roms/
/// directory changes, and flag favorites/recents changes.
fn collect_rom_event_systems(
    event: &notify::Event,
    roms_dir: &std::path::Path,
    affected_systems: &mut std::collections::HashSet<String>,
    roms_dir_changed: &mut bool,
    favorites_changed: &mut bool,
    recents_changed: &mut bool,
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

        // Internal marker directories trigger targeted in-memory invalidation
        // so remote/OS-level edits (e.g. a .fav symlink written outside
        // the UI) propagate without a restart.
        match system_name.as_ref() {
            "_favorites" => {
                *favorites_changed = true;
                continue;
            }
            "_recent" => {
                *recents_changed = true;
                continue;
            }
            _ => {}
        }

        // Skip any other internal directory.
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
