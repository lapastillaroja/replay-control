use replay_control_core::systems::system_thumbnail_repos;
use replay_control_core_server::db_pool::rusqlite;
use replay_control_core_server::library_db::{IdentityState, LibraryDb};
use replay_control_core_server::roms::{RomEntry, StorageProbe};
use replay_control_core_server::storage::StorageLocation;
use replay_control_core_server::update as update_io;
use replay_control_core_server::{game_db, game_entry_builder, rc_hash_disc, rom_hash};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::AppState;
use super::activity::{
    Activity, IdentityPhase, IdentityProgress, RebuildPhase, RebuildProgress, RefreshMetadataPhase,
    RefreshMetadataProgress, StartupPhase,
};
use super::db_pools::{LIBRARY_MAINTENANCE_WRITE_TIMEOUT, LibraryReadPool};
use super::library::{ScanCancellation, ScanInputs, ScanOptions};
use crate::types::RomWatcherStatus;

/// A system whose ROMs get runtime hashing + RA-id resolution in the identity
/// phase: cart systems (CRC + header rc_hash) or disc systems (boot-file
/// rc_hash). Both go through the same identity-job machinery; only the inner
/// hash dispatch differs.
fn is_hash_identifiable(system: &str) -> bool {
    rom_hash::is_hash_eligible(system) || rc_hash_disc::is_disc_rc_hash_system(system)
}

fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
}

fn env_duration_secs(name: &str, default_secs: u64, min_secs: u64) -> Duration {
    let secs = std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(|secs| secs.max(min_secs))
        .unwrap_or(default_secs);
    Duration::from_secs(secs)
}

const EXTERNAL_METADATA_REFRESH_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const PIPELINE_ACTIVITY_RETRY_DELAY: Duration = Duration::from_millis(250);
const PIPELINE_ACTIVITY_RETRY_ATTEMPTS: usize = 240;
const IDENTITY_BATCH_SIZE: usize = 200;
const UPDATE_INITIAL_DELAY_SECS: u64 = 60;
const UPDATE_INTERVAL_SECS: u64 = 24 * 60 * 60;

#[derive(Clone, Copy)]
pub(crate) enum PopulateProgress {
    /// Boot pipeline / post-import. Updates `Activity::Startup` if active.
    Startup,
    /// Explicit user action: rebuild or rescan. Updates `Activity::Rebuild`.
    /// The rescan-vs-rebuild distinction lives on `Activity::Rebuild`'s
    /// `RebuildProgress.is_rescan` flag, set by the calling server fn before
    /// spawning. `populate_all_systems` doesn't see that flag and doesn't
    /// need to — strict reconcile is the same operation either way.
    Rebuild { start: std::time::Instant },
}

#[derive(Clone)]
struct IdentityJob {
    system: String,
    roms: Arc<Vec<RomEntry>>,
    scan_inputs: ScanInputs,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IdentityJobOutcome {
    Completed {
        rows_done: usize,
    },
    Cancelled {
        work_system_done: bool,
    },
    Failed {
        work_system_done: bool,
    },
    Skipped {
        rows_done: usize,
        work_system_done: bool,
    },
}

impl PopulateProgress {
    fn rebuild_start(&self) -> Option<std::time::Instant> {
        match self {
            Self::Startup => None,
            Self::Rebuild { start } => Some(*start),
        }
    }
}

/// Update `Activity::Rebuild`'s progress in place. No-op if a different
/// activity variant is currently active (e.g. someone replaced the guard).
fn update_rebuild_progress(state: &AppState, f: impl FnOnce(&mut RebuildProgress)) {
    state.update_activity(|act| {
        if let Activity::Rebuild { progress } = act {
            f(progress);
        }
    });
}

fn fail_refresh_metadata(state: &AppState, start: Instant, error: impl Into<String>) {
    let error = error.into();
    state.update_activity(|act| {
        if let Activity::RefreshExternalMetadata { progress } = act {
            progress.phase = RefreshMetadataPhase::Failed;
            progress.error = Some(error);
            progress.elapsed_secs = start.elapsed().as_secs();
        }
    });
}

/// Push per-system progress into whichever Activity variant the caller
/// owns. Startup carries a single label string; Rebuild carries counters
/// + elapsed seconds. The `enriching` flag is forwarded as a structured
///   field on the activity so consumers (banner, page hint) format the
///   per-system phase via i18n instead of receiving a baked English suffix.
fn report_system(
    state: &AppState,
    progress: PopulateProgress,
    i: usize,
    display_name: &str,
    enriching: bool,
) {
    match progress {
        PopulateProgress::Startup => state.update_activity(|act| {
            if let Activity::Startup {
                system,
                enriching: e,
                ..
            } = act
            {
                *system = display_name.to_string();
                *e = enriching;
            }
        }),
        PopulateProgress::Rebuild { start } => {
            update_rebuild_progress(state, |p| {
                p.current_system = display_name.to_string();
                p.systems_done = i;
                p.elapsed_secs = start.elapsed().as_secs();
                p.phase = if enriching {
                    RebuildPhase::Enriching
                } else {
                    RebuildPhase::Scanning
                };
                p.enriching = enriching;
            });
        }
    }
}

fn update_identity_progress(state: &AppState, f: impl FnOnce(&mut IdentityProgress)) {
    state.update_activity(|act| {
        if let Activity::Identity { progress } = act {
            f(progress);
        }
    });
}

/// Read a value from the pool or skip the calling phase. Pool unavailability
/// is logged at `debug` ("transient — retry later"), inner SQL errors at
/// `warn`. Used by destructive cascade gates that must distinguish
/// "DB unavailable" from "DB has no rows" — defaulting unavailability to
/// "empty" is what triggered the
/// `2026-05-01-library-wal-unlink-under-live-connections` regression.
async fn try_read_or_skip<T, F>(pool: &LibraryReadPool, phase: &'static str, f: F) -> Option<T>
where
    F: FnOnce(&rusqlite::Connection) -> replay_control_core::error::Result<T> + Send + 'static,
    T: Send + 'static,
{
    match pool.try_read(f).await {
        Ok(Ok(value)) => Some(value),
        Ok(Err(e)) => {
            tracing::warn!("{phase}: SQL failed: {e}");
            None
        }
        Err(e) => {
            tracing::debug!("{phase}: pool unavailable ({e}); skipping");
            None
        }
    }
}

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
        // Clean up stale update temp files from a previous run.
        update_io::nuke_update_dir();

        Self::spawn_pipeline(state.clone());

        // Start watchers immediately (they're independent of the pipeline).
        state.clone().spawn_storage_watcher();
        state.spawn_rom_watcher();

        // Spawn update checker (independent of pipeline, no activity lock needed).
        let update_state = state.clone();
        tokio::spawn(async move {
            Self::update_check_loop(update_state).await;
        });

        // Spawn the now-playing detector loop (independent of the startup
        // pipeline). API-based: exits immediately in standalone mode, where
        // `state.replay_api` is `None`.
        let now_playing_state = state.clone();
        tokio::spawn(async move {
            super::now_playing::run_now_playing_loop(now_playing_state).await;
        });

        // RePlayOS API integration: startup probe + self-recovery maintenance.
        // No-ops immediately in standalone mode (`state.replay_api` is None).
        let replay_api_state = state.clone();
        tokio::spawn(async move {
            super::replay_api::run_replay_api_maintenance(replay_api_state).await;
        });
    }

    /// Spawn only the ordered pipeline. Used after an already-running app
    /// swaps from one available storage device to another; the long-running
    /// watchers already exist and must not be duplicated.
    pub fn spawn_pipeline(state: AppState) {
        let pipeline_state = state.clone();
        tokio::spawn(async move {
            Self::run_pipeline(&pipeline_state).await;
        });
    }

    /// Run the ordered startup pipeline (async).
    async fn run_pipeline(state: &AppState) {
        // Brief delay to let the server start accepting requests.
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Phase 0: confirm storage dirents are stable enough before any
        // scan writes L2. On NFS / autofs / USB hot-plug the storage root
        // may resolve before subdirectories surface; without this, the
        // first strict scan can legitimately observe an empty filesystem
        // and reconcile cached rows to zero.
        let storage = state.storage();
        let mut next_probe_warn = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            match replay_control_core_server::roms::probe_storage_ready(&storage).await {
                StorageProbe::HasVisibleEntries | StorageProbe::StableEmpty => break,
                StorageProbe::NotReady => {
                    let now = std::time::Instant::now();
                    if now >= next_probe_warn {
                        tracing::warn!(
                            "Startup: storage at {} not ready; still waiting before startup scan",
                            storage.root.display()
                        );
                        next_probe_warn = now + Duration::from_secs(30);
                    } else {
                        tracing::debug!(
                            "Startup: storage at {} not ready; retrying before startup scan",
                            storage.root.display()
                        );
                    }
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
        drop(storage);

        // Phase 0.5: On first boot, fetch optional source metadata before the
        // library scan. This is a one-time cost, and waiting avoids building a
        // partial first library that needs immediate re-enrichment.
        if Self::first_run_seed_enabled() {
            if let Some(_guard) =
                Self::claim_startup_activity(state, StartupPhase::FetchingMetadata).await
            {
                Self::phase_first_run_seed(state).await;
            }
        } else {
            tracing::debug!("phase_first_run_seed: disabled by environment");
        }

        // Phase 1: Auto-import (if launchbox XML exists + DB empty).
        // Import claims/releases its own Activity::Import via try_start_activity.
        Self::phase_auto_import(state).await;

        // Phase 1.5: Reconcile `title_norm_version` stamps on both DBs.
        // Bumping `replay_control_core::title_utils::TITLE_NORM_VERSION` in
        // a release silently rebuilds stored normalized columns here so
        // matching benefits without user action. No-op when stamps match
        // (steady-state) or the cached XML is missing (fresh install
        // pre-LB-import — auto_import will write the stamp itself).
        Self::phase_title_norm_reconcile(state).await;

        // Phase 2+3: Claim Activity::Startup for populate + thumbnail rebuild.
        // A storage swap can cancel an in-flight rebuild while this pipeline is
        // being scheduled; retry briefly so the rebuild guard can drop instead
        // of losing the new storage verification pass.
        {
            let Some(_guard) = Self::claim_startup_activity(state, StartupPhase::Scanning).await
            else {
                return;
            };

            Self::phase_cache_verification(state).await;
            Self::phase_enrichment_inputs_reconcile(state).await;
            Self::phase_reresolve_rc_hash_ra_ids(state).await;

            Self::phase_auto_rebuild_thumbnail_index(state).await;
            state.cache.resume_pending_thumbnail_downloads(state).await;

            // _guard drops → Idle
        }
    }

    async fn claim_startup_activity(
        state: &AppState,
        phase: StartupPhase,
    ) -> Option<crate::api::ActivityGuard> {
        for attempt in 0..PIPELINE_ACTIVITY_RETRY_ATTEMPTS {
            match state.try_start_activity(Activity::Startup {
                phase,
                system: String::new(),
                enriching: false,
            }) {
                Ok(guard) => return Some(guard),
                Err(e) => {
                    if attempt == 0 {
                        tracing::info!("Startup pipeline waiting for active operation: {e}");
                    }
                    tokio::time::sleep(PIPELINE_ACTIVITY_RETRY_DELAY).await;
                }
            }
        }
        tracing::warn!("Could not start startup pipeline: activity stayed busy");
        None
    }

    async fn scan_inputs_for_system(
        state: &AppState,
        system: &str,
        options: ScanOptions,
        generation: u64,
    ) -> Result<ScanInputs, replay_control_core::error::Error> {
        state.ensure_storage_generation(generation)?;

        let cached_hashes = if is_hash_identifiable(system) {
            let system_owned = system.to_string();
            match state
                .library_reader
                .read(move |conn| LibraryDb::load_cached_hashes(conn, &system_owned))
                .await
            {
                Some(Ok(hashes)) => hashes,
                Some(Err(e)) => {
                    tracing::warn!(
                        "Could not load cached hashes for {system}: {e}; CRCs will be recomputed"
                    );
                    std::collections::HashMap::new()
                }
                None => {
                    tracing::warn!(
                        "Could not load cached hashes for {system}: library DB unavailable; CRCs will be recomputed"
                    );
                    std::collections::HashMap::new()
                }
            }
        } else {
            std::collections::HashMap::new()
        };

        let (clean_startup_fingerprint, mtime_probe_trustworthy) = if options.skip_unchanged_startup
        {
            let system_owned = system.to_string();
            let probe_signature = state.storage().mtime_probe_signature();
            match state
                .library_reader
                .read(move |conn| {
                    use replay_control_core_server::library_db::library_meta;

                    let fingerprint =
                        LibraryDb::clean_startup_discovery_fingerprint(conn, &system_owned)?;
                    let stored_signature = library_meta::read_meta_result(
                        conn,
                        library_meta::keys::MTIME_PROBE_SIGNATURE,
                    )?;
                    let trustworthy = library_meta::read_meta_result(
                        conn,
                        library_meta::keys::MTIME_PROBE_TRUSTWORTHY,
                    )?
                    .as_deref()
                        == Some("true");
                    Ok::<_, replay_control_core::error::Error>((
                        fingerprint,
                        trustworthy
                            && stored_signature.as_deref() == Some(probe_signature.as_str()),
                    ))
                })
                .await
            {
                Some(Ok((fingerprint, trustworthy))) => {
                    if !trustworthy {
                        tracing::info!(
                            "Startup scan skip disabled for {system}: storage mtime probe missing, failed, or stale"
                        );
                    }
                    (fingerprint, trustworthy)
                }
                Some(Err(e)) => {
                    tracing::warn!(
                        "Could not load startup scan fingerprint for {system}: {e}; system will be reconciled"
                    );
                    (None, false)
                }
                None => {
                    tracing::warn!(
                        "Could not load startup scan fingerprint for {system}: library DB unavailable; system will be reconciled"
                    );
                    (None, false)
                }
            }
        } else {
            (None, false)
        };

        Ok(ScanInputs::new(
            cached_hashes,
            clean_startup_fingerprint,
            mtime_probe_trustworthy,
            options,
            Some(ScanCancellation::new(
                state.storage_generation.clone(),
                generation,
            )),
        ))
    }

    fn is_storage_changed(e: &replay_control_core::error::Error) -> bool {
        matches!(e, replay_control_core::error::Error::StorageChanged)
    }

    fn identity_worker_count() -> usize {
        std::env::var("REPLAY_CONTROL_IDENTITY_WORKERS")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|v| (1..=4).contains(v))
            .unwrap_or(2)
    }

    fn spawn_identity_jobs(
        state: AppState,
        storage: StorageLocation,
        jobs: Vec<IdentityJob>,
        generation: u64,
    ) {
        if jobs.is_empty() {
            return;
        }
        let worker_count = Self::identity_worker_count();
        tokio::spawn(async move {
            let _phase_guard = state.identity_phase.lock().await;
            let eligible_systems = jobs.len();
            let jobs_for_count = jobs
                .iter()
                .map(|job| (job.system.clone(), job.scan_inputs.force_rehash()))
                .collect::<Vec<_>>();
            let force_rehash_systems = jobs_for_count
                .iter()
                .filter(|(_, force_rehash)| *force_rehash)
                .count();
            let (work_systems, work_rows) = state
                .library_reader
                .read(move |conn| LibraryDb::identity_work_counts(conn, &jobs_for_count))
                .await
                .and_then(Result::ok)
                .unwrap_or_default();
            tracing::info!(
                "Identity phase: queued eligible_systems={eligible_systems} force_rehash_systems={force_rehash_systems} work_systems={work_systems} work_rows={work_rows} workers={} storage={}",
                worker_count,
                storage.kind.as_str()
            );
            if work_rows == 0 {
                tracing::info!(
                    "Identity phase: queued work finished completed=0 cancelled=0 failed=0 skipped={eligible_systems}"
                );
                return;
            }

            let identity_started = Instant::now();
            let guard = loop {
                if state.ensure_storage_generation(generation).is_err() {
                    tracing::info!("Identity phase: storage changed before activity claim");
                    return;
                }
                if state.is_idle() {
                    match state.try_start_activity(Activity::Identity {
                        progress: IdentityProgress::initial(work_rows, work_systems),
                    }) {
                        Ok(guard) => break guard,
                        Err(_) => {
                            tokio::time::sleep(Duration::from_millis(200)).await;
                        }
                    }
                } else {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            };

            let semaphore = Arc::new(tokio::sync::Semaphore::new(worker_count));
            let mut handles = tokio::task::JoinSet::new();
            for job in jobs {
                let state = state.clone();
                let storage = storage.clone();
                let semaphore = semaphore.clone();
                handles.spawn(async move {
                    let permit = match semaphore.acquire_owned().await {
                        Ok(permit) => permit,
                        Err(_) => {
                            return IdentityJobOutcome::Cancelled {
                                work_system_done: false,
                            };
                        }
                    };
                    let _permit = permit;
                    Self::run_identity_job(&state, &storage, job, generation).await
                });
            }
            let mut completed = 0usize;
            let mut cancelled = 0usize;
            let mut failed = 0usize;
            let mut skipped = 0usize;
            let mut rows_done = 0usize;
            let mut work_systems_done = 0usize;
            while let Some(handle_result) = handles.join_next().await {
                match handle_result {
                    Ok(IdentityJobOutcome::Completed {
                        rows_done: job_rows,
                    }) => {
                        completed += 1;
                        rows_done = rows_done.saturating_add(job_rows);
                        work_systems_done += 1;
                    }
                    Ok(IdentityJobOutcome::Cancelled { work_system_done }) => {
                        cancelled += 1;
                        if work_system_done {
                            work_systems_done += 1;
                        }
                    }
                    Ok(IdentityJobOutcome::Failed { work_system_done }) => {
                        failed += 1;
                        if work_system_done {
                            work_systems_done += 1;
                        }
                    }
                    Ok(IdentityJobOutcome::Skipped {
                        rows_done: job_rows,
                        work_system_done,
                    }) => {
                        skipped += 1;
                        rows_done = rows_done.saturating_add(job_rows);
                        if work_system_done {
                            work_systems_done += 1;
                        }
                    }
                    Err(e) => {
                        failed += 1;
                        tracing::warn!("Identity worker task failed to join: {e}");
                    }
                }
                update_identity_progress(&state, |progress| {
                    progress.rows_done = progress.rows_done.max(rows_done).min(progress.rows_total);
                    progress.systems_done = work_systems_done.min(progress.systems_total);
                    progress.elapsed_secs = identity_started.elapsed().as_secs();
                });
            }
            tracing::info!(
                "Identity phase: queued work finished completed={completed} cancelled={cancelled} failed={failed} skipped={skipped}"
            );
            guard.update(|activity| {
                if let Activity::Identity { progress } = activity {
                    progress.elapsed_secs = identity_started.elapsed().as_secs();
                    progress.rows_done = rows_done.min(progress.rows_total);
                    progress.systems_done = work_systems_done.min(progress.systems_total);
                    progress.phase = if cancelled > 0 {
                        IdentityPhase::Cancelled
                    } else if failed > 0 {
                        IdentityPhase::Failed
                    } else {
                        progress.rows_done = progress.rows_total;
                        progress.systems_done = progress.systems_total;
                        IdentityPhase::Complete
                    };
                }
            });
            drop(guard);
        });
    }

    async fn wait_for_identity_window(state: &AppState, generation: u64, system: &str) -> bool {
        let mut logged = false;
        loop {
            if state.ensure_storage_generation(generation).is_err() {
                tracing::info!("Identity phase: storage changed before {system}");
                return false;
            }
            if state.identity_can_run() {
                return true;
            }
            if !logged {
                tracing::info!("Identity phase: waiting for foreground activity before {system}");
                logged = true;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    async fn run_identity_job(
        state: &AppState,
        storage: &StorageLocation,
        job: IdentityJob,
        generation: u64,
    ) -> IdentityJobOutcome {
        if !Self::wait_for_identity_window(state, generation, &job.system).await {
            return IdentityJobOutcome::Cancelled {
                work_system_done: false,
            };
        }
        if state.ensure_storage_generation(generation).is_err() {
            tracing::info!("Identity phase: storage changed before {}", job.system);
            return IdentityJobOutcome::Cancelled {
                work_system_done: false,
            };
        }
        if !is_hash_identifiable(&job.system) {
            return IdentityJobOutcome::Skipped {
                rows_done: 0,
                work_system_done: false,
            };
        }

        let started = Instant::now();
        let force_rehash = job.scan_inputs.force_rehash();
        let force_candidates: Vec<String> = job
            .roms
            .iter()
            .filter(|rom| {
                !rom.is_m3u
                    && (rom_hash::is_file_hash_eligible(&job.system, &rom.game.rom_filename)
                        || rc_hash_disc::is_disc_rc_hash_system(&job.system))
            })
            .map(|rom| rom.game.rom_filename.clone())
            .collect();
        let mut force_offset = 0usize;
        let mut rows_done = 0usize;
        let mut updated_rows = 0usize;
        let mut claimed_any = false;
        // Filenames attempted in this run. `claim_identity_batch` re-claims Failed
        // rows, so without this a deterministically-failing file would be claimed
        // -> Failed -> re-claimed forever (see the forward-progress guard below).
        let mut attempted: std::collections::HashSet<String> = std::collections::HashSet::new();

        loop {
            if !Self::wait_for_identity_window(state, generation, &job.system).await {
                return IdentityJobOutcome::Cancelled {
                    work_system_done: claimed_any,
                };
            }
            if state.ensure_storage_generation(generation).is_err() {
                return IdentityJobOutcome::Cancelled {
                    work_system_done: claimed_any,
                };
            }

            let claimed = if force_rehash {
                if force_offset >= force_candidates.len() {
                    Ok(Vec::new())
                } else {
                    let end = (force_offset + IDENTITY_BATCH_SIZE).min(force_candidates.len());
                    let filenames = force_candidates[force_offset..end].to_vec();
                    force_offset = end;
                    Self::claim_identity_filenames(state, &job.system, filenames).await
                }
            } else {
                Self::claim_identity_batch(state, &job.system).await
            };

            let claimed = match claimed {
                Ok(claimed) => claimed,
                Err(e) => {
                    tracing::warn!("Identity phase: could not claim {} batch: {e}", job.system);
                    return IdentityJobOutcome::Failed {
                        work_system_done: claimed_any,
                    };
                }
            };
            if claimed.is_empty() {
                if force_rehash && force_offset < force_candidates.len() {
                    continue;
                }
                break;
            }
            claimed_any = true;

            // Forward-progress guard: if a batch is entirely rows we've already
            // attempted this run, the claim is just re-serving Failed rows that
            // can't resolve (e.g. a .cue whose .bin is missing). Stop rather than
            // spin — they stay Failed and retry on the next scan, not in a tight
            // loop that floods the log (which previously filled log2ram and
            // crashed the service).
            if claimed.iter().all(|f| attempted.contains(f)) {
                tracing::warn!(
                    "Identity phase: {} made no progress; {} unresolved row(s) re-claimed — stopping to avoid a reclaim loop",
                    job.system,
                    claimed.len()
                );
                break;
            }
            attempted.extend(claimed.iter().cloned());

            let claimed_set: std::collections::HashSet<&str> =
                claimed.iter().map(String::as_str).collect();
            let mut batch_roms: Vec<RomEntry> = job
                .roms
                .iter()
                .filter(|rom| claimed_set.contains(rom.game.rom_filename.as_str()))
                .cloned()
                .collect();
            if batch_roms.is_empty() {
                let finished = Self::finish_identity_batch(
                    state,
                    &job.system,
                    claimed,
                    IdentityState::Pending,
                )
                .await;
                rows_done += finished.unwrap_or(0);
                continue;
            }

            let hash_cancel = Arc::new(AtomicBool::new(false));
            let hash_cancel_watcher = hash_cancel.clone();
            let watcher_state = state.clone();
            let watcher_system = job.system.clone();
            let watcher = tokio::spawn(async move {
                loop {
                    if watcher_state.ensure_storage_generation(generation).is_err() {
                        hash_cancel_watcher.store(true, Ordering::Relaxed);
                        break;
                    }
                    if !watcher_state.identity_can_run() {
                        tracing::info!(
                            "Identity phase: pausing hash for {watcher_system} because foreground activity started"
                        );
                        hash_cancel_watcher.store(true, Ordering::Relaxed);
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            });
            let batch_started = Instant::now();
            let (hash_results, _stats) = state
                .cache
                .hash_roms_for_system(
                    storage,
                    &job.system,
                    &mut batch_roms,
                    &job.scan_inputs,
                    Some(hash_cancel.clone()),
                )
                .await;
            watcher.abort();

            if state.ensure_storage_generation(generation).is_err() {
                tracing::info!(
                    "Identity phase: storage changed after hashing {}",
                    job.system
                );
                return IdentityJobOutcome::Cancelled {
                    work_system_done: true,
                };
            }

            let mut batch_updated = 0usize;
            if !hash_results.is_empty() {
                let identity_entries =
                    game_entry_builder::build_game_entries(&job.system, &batch_roms, &hash_results)
                        .await;
                let identity_entries: Vec<_> = identity_entries
                    .into_iter()
                    .filter(|entry| hash_results.contains_key(&entry.rom_filename))
                    .collect();
                let system_for_update = job.system.clone();
                let identity_update_result = state
                    .library_writer
                    .try_write_with_timeout(LIBRARY_MAINTENANCE_WRITE_TIMEOUT, move |conn| {
                        LibraryDb::update_running_identity_entries(
                            conn,
                            &system_for_update,
                            &identity_entries,
                        )
                    })
                    .await;
                batch_updated = match identity_update_result {
                    Ok(Ok(updated)) => updated,
                    Ok(Err(e)) => {
                        tracing::warn!(
                            "Identity phase: identity batch update failed for {}: {e}",
                            job.system
                        );
                        let _ = Self::finish_identity_batch(
                            state,
                            &job.system,
                            claimed,
                            IdentityState::Pending,
                        )
                        .await;
                        return IdentityJobOutcome::Failed {
                            work_system_done: true,
                        };
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Identity phase: writer unavailable while updating {}: {e}",
                            job.system
                        );
                        let _ = Self::finish_identity_batch(
                            state,
                            &job.system,
                            claimed,
                            IdentityState::Pending,
                        )
                        .await;
                        return IdentityJobOutcome::Failed {
                            work_system_done: true,
                        };
                    }
                };
            }

            let unresolved_state = if hash_cancel.load(Ordering::Relaxed) {
                IdentityState::Pending
            } else {
                IdentityState::Failed
            };
            let unresolved =
                Self::finish_identity_batch(state, &job.system, claimed, unresolved_state)
                    .await
                    .unwrap_or(0);
            rows_done = rows_done.saturating_add(batch_updated + unresolved);
            updated_rows = updated_rows.saturating_add(batch_updated);
            update_identity_progress(state, |progress| {
                progress.rows_done = progress
                    .rows_done
                    .saturating_add(batch_updated + unresolved);
                progress.rows_done = progress.rows_done.min(progress.rows_total);
                progress.elapsed_secs = started.elapsed().as_secs();
            });
            tracing::info!(
                "Identity phase: {} batch complete rows={} updated={} unresolved={} cancelled={} batch_ms={}",
                job.system,
                batch_roms.len(),
                batch_updated,
                unresolved,
                hash_cancel.load(Ordering::Relaxed),
                batch_started.elapsed().as_millis()
            );

            if hash_cancel.load(Ordering::Relaxed) {
                if updated_rows > 0 {
                    let _ = Self::post_identity_enrich(state, &job).await;
                }
                return IdentityJobOutcome::Cancelled {
                    work_system_done: true,
                };
            }
        }

        if !claimed_any {
            tracing::debug!("Identity phase: {} has no rows to claim", job.system);
            return IdentityJobOutcome::Skipped {
                rows_done: 0,
                work_system_done: false,
            };
        }

        if updated_rows > 0
            && let Err(e) = Self::post_identity_enrich(state, &job).await
        {
            tracing::warn!(
                "Identity phase: post-hash enrichment failed for {}: {e}",
                job.system
            );
            return IdentityJobOutcome::Failed {
                work_system_done: true,
            };
        }

        // Refresh this system's coverage stats now that the identity phase has
        // set ra_id / rc_hash. Stats are otherwise refreshed only on
        // enrichment-complete (which runs *before* hashing) and by the startup
        // `phase_reresolve` (next boot), so without this the metadata page shows
        // stale RA coverage (often 0) for a freshly-hashed system until restart.
        if updated_rows > 0 {
            let sys = job.system.clone();
            match state
                .library_writer
                .try_write_with_timeout(LIBRARY_MAINTENANCE_WRITE_TIMEOUT, move |conn| {
                    LibraryDb::refresh_game_library_system_stats(conn, &sys)
                })
                .await
            {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    tracing::warn!(
                        "Identity phase: stats refresh failed for {}: {e}",
                        job.system
                    )
                }
                Err(e) => tracing::warn!(
                    "Identity phase: stats writer unavailable for {}: {e}",
                    job.system
                ),
            }
        }

        tracing::info!(
            "Identity phase: {} complete in {}ms (updated={updated_rows}, rows_done={rows_done})",
            job.system,
            started.elapsed().as_millis(),
        );
        IdentityJobOutcome::Completed { rows_done }
    }

    async fn claim_identity_batch(
        state: &AppState,
        system: &str,
    ) -> replay_control_core::error::Result<Vec<String>> {
        let system_for_claim = system.to_string();
        let result = state
            .library_writer
            .try_write_with_timeout(LIBRARY_MAINTENANCE_WRITE_TIMEOUT, move |conn| {
                LibraryDb::claim_identity_batch(conn, &system_for_claim, false, IDENTITY_BATCH_SIZE)
            })
            .await;
        match result {
            Ok(Ok(claimed)) => Ok(claimed),
            Ok(Err(e)) => Err(e),
            Err(e) => Err(replay_control_core::error::Error::Other(e.to_string())),
        }
    }

    async fn claim_identity_filenames(
        state: &AppState,
        system: &str,
        filenames: Vec<String>,
    ) -> replay_control_core::error::Result<Vec<String>> {
        let system_for_claim = system.to_string();
        let result = state
            .library_writer
            .try_write_with_timeout(LIBRARY_MAINTENANCE_WRITE_TIMEOUT, move |conn| {
                LibraryDb::claim_identity_filenames(conn, &system_for_claim, &filenames)
            })
            .await;
        match result {
            Ok(Ok(claimed)) => Ok(claimed),
            Ok(Err(e)) => Err(e),
            Err(e) => Err(replay_control_core::error::Error::Other(e.to_string())),
        }
    }

    async fn finish_identity_batch(
        state: &AppState,
        system: &str,
        filenames: Vec<String>,
        identity_state: IdentityState,
    ) -> replay_control_core::error::Result<usize> {
        let system_for_finish = system.to_string();
        let result = state
            .library_writer
            .try_write_with_timeout(LIBRARY_MAINTENANCE_WRITE_TIMEOUT, move |conn| {
                LibraryDb::finish_identity_batch(
                    conn,
                    &system_for_finish,
                    &filenames,
                    identity_state,
                )
            })
            .await;
        match result {
            Ok(Ok(count)) => Ok(count),
            Ok(Err(e)) => Err(e),
            Err(e) => Err(replay_control_core::error::Error::Other(e.to_string())),
        }
    }

    async fn post_identity_enrich(
        state: &AppState,
        job: &IdentityJob,
    ) -> replay_control_core::error::Result<()> {
        state
            .cache
            .enrich_system_cache_with_cancellation(
                state,
                job.system.clone(),
                job.scan_inputs.cancellation(),
            )
            .await?;
        state.cache.invalidate_l1().await;
        state.invalidate_user_caches().await;
        Ok(())
    }

    /// Spawn a background task that re-runs `phase_auto_import`. Used by the
    /// "Regenerate metadata" UI button and other on-demand triggers.
    pub fn spawn_external_metadata_refresh(state: AppState) {
        tokio::spawn(async move {
            Self::phase_auto_import(&state).await;
        });
    }

    /// Spawn a background task that downloads the LaunchBox `Metadata.zip`
    /// into the host-global cache directory, extracts the XML, then triggers
    /// the standard refresh path against it.
    ///
    /// Uses an HTTP ETag check to skip the 100+ MB download when the upstream
    /// file hasn't changed since the last successful download.
    pub fn spawn_external_metadata_download_and_refresh(state: AppState) {
        tokio::spawn(async move {
            let _ = Self::download_external_metadata_and_refresh(&state).await;
        });
    }

    /// First-run setup wrapper: one UI action can fill both external metadata
    /// sources. LaunchBox runs first; when it releases the activity slot, the
    /// thumbnail manifest update starts from the same click.
    pub fn spawn_setup_metadata_downloads(
        state: AppState,
        needs_launchbox: bool,
        needs_thumbnail_index: bool,
    ) {
        tokio::spawn(async move {
            if needs_launchbox && !Self::download_external_metadata_and_refresh(&state).await {
                return;
            }

            if needs_thumbnail_index && !state.thumbnails.start_thumbnail_update(&state) {
                tracing::warn!("setup metadata: thumbnail update could not start; activity busy");
            }
        });
    }

    async fn download_external_metadata_and_refresh(state: &AppState) -> bool {
        use replay_control_core_server::external_metadata::{self, meta_keys};

        // Claim the slot. Start at Checking so the banner shows while we
        // do the HEAD request before committing to a full download.
        let guard = match state.try_start_activity(Activity::RefreshExternalMetadata {
            progress: RefreshMetadataProgress {
                phase: RefreshMetadataPhase::Checking,
                ..RefreshMetadataProgress::initial()
            },
        }) {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!("download+refresh: activity busy: {e}");
                return false;
            }
        };

        let start = std::time::Instant::now();
        let cache_dir = state.data_dir.cache_dir();

        let stored_etag = state
            .external_metadata_reader
            .read(|conn| external_metadata::read_meta(conn, meta_keys::LAUNCHBOX_UPSTREAM_ETAG))
            .await
            .flatten();

        // Single HEAD request — captures ETag (freshness check) and Content-Length
        // (passed to download_metadata to avoid a redundant second HEAD).
        let upstream_head =
            tokio::task::spawn_blocking(replay_control_core_server::launchbox::fetch_upstream_head)
                .await
                .unwrap_or(replay_control_core_server::launchbox::HeadHeaders {
                    content_length: None,
                    etag: None,
                });

        if stored_etag.is_some() && stored_etag == upstream_head.etag {
            tracing::info!(
                "LaunchBox ETag matches ({}) — skipping download, re-enriching",
                upstream_head.etag.as_deref().unwrap_or("")
            );
            // Skip the download and XML re-parse, but still enrich so any
            // ROMs added since the last refresh pick up their metadata.
            state.update_activity(|act| {
                if let Activity::RefreshExternalMetadata { progress } = act {
                    progress.phase = RefreshMetadataPhase::Enriching;
                }
            });
            Self::reenrich_all_systems(state).await;
            state.update_activity(|act| {
                if let Activity::RefreshExternalMetadata { progress } = act {
                    progress.phase = RefreshMetadataPhase::Complete;
                    progress.elapsed_secs = start.elapsed().as_secs();
                }
            });
            return true; // guard drops → Activity::Idle
        }

        // ETags differ (or unavailable) — proceed with the full download.
        state.update_activity(|act| {
            if let Activity::RefreshExternalMetadata { progress } = act {
                progress.phase = RefreshMetadataPhase::Downloading;
            }
        });

        let upstream_etag = upstream_head.etag;
        let upstream_content_length = upstream_head.content_length;
        let download_result = {
            let state_for_progress = state.clone();
            tokio::task::spawn_blocking(move || {
                // Throttle: each curl read is ~64 KB; updating activity per
                // chunk is 3000+ RwLock+broadcast cycles per 200 MB
                // download. Only fire when we cross a 1 MiB boundary.
                // `download_metadata` takes `Fn`, so we need interior
                // mutability for the watermark.
                use std::sync::atomic::{AtomicU64, Ordering};
                const THROTTLE_BYTES: u64 = 1024 * 1024;
                let last_reported = AtomicU64::new(0);
                replay_control_core_server::launchbox::download_metadata(
                    &cache_dir,
                    upstream_content_length,
                    |bytes, total| {
                        let prev = last_reported.load(Ordering::Relaxed);
                        if bytes - prev < THROTTLE_BYTES && bytes != 0 {
                            return;
                        }
                        last_reported.store(bytes, Ordering::Relaxed);
                        state_for_progress.update_activity(|act| {
                            if let Activity::RefreshExternalMetadata { progress } = act {
                                progress.downloaded_bytes = bytes;
                                progress.total_bytes = total;
                            }
                        });
                    },
                )
            })
            .await
        };

        match download_result {
            Ok(Ok(xml_path)) => {
                tracing::info!("LaunchBox metadata downloaded to {}", xml_path.display());
                // Store the upstream ETag so the next "Refresh metadata" can
                // detect an unchanged file without re-downloading.
                if let Some(etag) = upstream_etag {
                    match state
                        .external_metadata_writer
                        .try_write(move |conn| {
                            external_metadata::write_meta(
                                conn,
                                meta_keys::LAUNCHBOX_UPSTREAM_ETAG,
                                Some(&etag),
                            )
                        })
                        .await
                    {
                        Ok(Ok(())) => {}
                        Ok(Err(e)) => {
                            tracing::warn!("LaunchBox upstream ETag SQL failed: {e}");
                        }
                        Err(e) => {
                            tracing::warn!("LaunchBox upstream ETag write failed: {e}");
                        }
                    }
                }
                Self::phase_auto_import_inner(state, Some(guard)).await;
                true
            }
            Ok(Err(e)) => {
                tracing::warn!("LaunchBox download failed: {e}");
                state.update_activity(|act| {
                    if let Activity::RefreshExternalMetadata { progress } = act {
                        progress.phase = RefreshMetadataPhase::Failed;
                        progress.error = Some(e.to_string());
                        progress.elapsed_secs = start.elapsed().as_secs();
                    }
                });
                false
            }
            Err(e) => {
                tracing::warn!("LaunchBox download task panicked: {e}");
                state.update_activity(|act| {
                    if let Activity::RefreshExternalMetadata { progress } = act {
                        progress.phase = RefreshMetadataPhase::Failed;
                        progress.error = Some(format!("task panicked: {e}"));
                        progress.elapsed_secs = start.elapsed().as_secs();
                    }
                });
                false
            }
        }
    }

    /// Phase 1.5: Reconcile per-storage `title_norm_version` on `library.db`.
    ///
    /// `replay_control_core::title_utils::TITLE_NORM_VERSION` bumps when
    /// `normalize_title_for_metadata` changes its output for any input.
    /// The host-global `external_metadata.db` is reconciled by
    /// `phase_auto_import` (it already gates re-parse on the stamp).
    /// This phase only handles the per-storage library DB: its stored
    /// `normalized_title` columns are rebuilt from `rom_filename` /
    /// arcade lookup when the stamp lags. Idempotent on success; a
    /// failure leaves the stale stamp so the next boot retries.
    async fn phase_title_norm_reconcile(state: &AppState) {
        use replay_control_core_server::title_norm_reconcile;

        if let Err(e) = title_norm_reconcile::reconcile_library_normalized_titles(
            state.library_writer.as_db_pool(),
        )
        .await
        {
            tracing::warn!("title_norm reconcile (library) failed: {e}");
        }
    }

    /// Phase 0.5: On first boot, download the LaunchBox XML and the libretro
    /// thumbnail manifest before scanning so first-pass enrichment has source
    /// data available.
    ///
    /// First-run conditions (checked independently):
    ///   - LaunchBox: no `launchbox_xml_crc32` in `external_meta` AND no XML on disk.
    ///   - Libretro: `data_source` has no rows.
    ///
    /// Any network failure is warn-logged and the pipeline continues normally.
    /// Phase 1 will detect and parse the downloaded XML via its usual hash check.
    async fn phase_first_run_seed(state: &AppState) {
        use replay_control_core_server::external_metadata::{self, meta_keys};
        use replay_control_core_server::library_db::resolve_launchbox_xml;

        let storage = state.storage();
        let rc_dir = storage.rc_dir();
        let cache_dir = state.data_dir.cache_dir();
        drop(storage);

        let seed_check = state
            .external_metadata_reader
            .read(|conn| {
                let has_crc32 =
                    external_metadata::read_meta(conn, meta_keys::LAUNCHBOX_XML_CRC32).is_some();
                let has_sources =
                    external_metadata::get_data_source_stats(conn, "libretro-thumbnails")
                        .ok()
                        .map(|s| s.repo_count > 0)
                        .unwrap_or(false);
                (has_crc32, has_sources)
            })
            .await;

        let (has_crc32, has_libretro_sources) = match seed_check {
            Some(v) => v,
            None => {
                tracing::warn!("phase_first_run_seed: pool unavailable, skipping");
                return;
            }
        };

        let xml_on_disk = resolve_launchbox_xml(&cache_dir, &rc_dir).is_some();
        let needs_launchbox = !has_crc32 && !xml_on_disk;
        let needs_libretro = !has_libretro_sources;

        if !needs_launchbox && !needs_libretro {
            tracing::debug!("phase_first_run_seed: not a first-run install, skipping");
            return;
        }

        tracing::info!(
            "phase_first_run_seed: first-run detected \
             (launchbox={needs_launchbox}, libretro={needs_libretro})"
        );

        if needs_launchbox {
            let dest = cache_dir.clone();
            let result = tokio::task::spawn_blocking(move || {
                replay_control_core_server::launchbox::download_metadata(&dest, None, |_, _| {})
            })
            .await;
            match result {
                Ok(Ok(p)) => {
                    tracing::info!(
                        "phase_first_run_seed: LaunchBox XML downloaded to {}",
                        p.display()
                    );
                }
                Ok(Err(e)) => {
                    tracing::warn!("phase_first_run_seed: LaunchBox download failed: {e}")
                }
                Err(e) => tracing::warn!("phase_first_run_seed: LaunchBox task panicked: {e}"),
            }
        }

        if needs_libretro {
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let api_key =
                replay_control_core_server::settings::read_github_api_key(&state.settings);
            match replay_control_core_server::thumbnail_manifest::import_all_manifests(
                state.external_metadata_writer.as_db_pool(),
                &|_, _, _| {},
                &cancel,
                api_key.as_deref(),
            )
            .await
            {
                Ok(stats) => tracing::info!(
                    "phase_first_run_seed: libretro manifest fetched \
                     ({} repos, {} entries{})",
                    stats.repos_fetched,
                    stats.total_entries,
                    if stats.rate_limited {
                        ", rate-limited"
                    } else {
                        ""
                    }
                ),
                Err(e) => tracing::warn!("phase_first_run_seed: libretro manifest failed: {e}"),
            }
        }
    }

    /// Phase 1: Refresh `external_metadata.db` from the LaunchBox XML when its
    /// content has changed (or the DB has never been populated).
    ///
    /// Freshness is content-derived: hash the XML, compare against the stored
    /// `external_meta.launchbox_xml_crc32`. mtime is unreliable across copies /
    /// rsync / clock skew. Skips entirely when no XML is present — users can
    /// still get scan-time + catalog enrichment.
    async fn phase_auto_import(state: &AppState) {
        Self::phase_auto_import_inner(state, None).await;
    }

    /// Inner entry point with optional caller-owned activity guard. Used by
    /// `spawn_external_metadata_download_and_refresh` to thread its
    /// `Downloading`-phase guard into the parse step without releasing it
    /// (avoiding an Idle flicker on the SSE stream).
    async fn phase_auto_import_inner(
        state: &AppState,
        existing_guard: Option<crate::api::ActivityGuard>,
    ) {
        use replay_control_core_server::external_metadata::{self, meta_keys};
        use replay_control_core_server::library_db::resolve_launchbox_xml;

        let storage = state.storage();
        let rc_dir = storage.rc_dir();
        let cache_dir = state.data_dir.cache_dir();

        let Some(xml_path) = resolve_launchbox_xml(&cache_dir, &rc_dir) else {
            tracing::debug!(
                "phase_auto_import: no LaunchBox XML in {} or {} — skipping",
                cache_dir.display(),
                rc_dir.display()
            );
            return;
        };

        // Claim the activity slot first (or reuse the caller's guard) so the
        // hash check itself is single-flight — two concurrent boots can't
        // both pass the hash mismatch and then race on the write.
        let guard = match existing_guard {
            Some(g) => g,
            None => match state.try_start_activity(Activity::RefreshExternalMetadata {
                progress: RefreshMetadataProgress::initial(),
            }) {
                Ok(g) => g,
                Err(e) => {
                    tracing::info!("phase_auto_import: another refresh in flight: {e}");
                    return;
                }
            },
        };

        let start = std::time::Instant::now();

        // Hash + stamp-read are independent — let the slowest dictate the
        // wall-clock instead of running them back-to-back. Both stamps live
        // in `external_meta`; one read pulls them together.
        let xml_for_hash = xml_path.clone();
        let hash_fut =
            tokio::task::spawn_blocking(move || external_metadata::hash_file_crc32(&xml_for_hash));
        let stamp_fut = state.external_metadata_reader.read(|conn| {
            (
                external_metadata::read_meta(conn, meta_keys::LAUNCHBOX_XML_CRC32),
                external_metadata::read_meta(conn, meta_keys::TITLE_NORM_VERSION),
                external_metadata::read_meta(conn, meta_keys::LAUNCHBOX_PLATFORM_MAP_HASH),
            )
        });
        let (hash_join, stamps) = tokio::join!(hash_fut, stamp_fut);
        let (stored_hash, stored_norm_version, stored_platform_hash) =
            stamps.unwrap_or((None, None, None));
        let stored_norm_version: Option<u32> = stored_norm_version.and_then(|s| s.parse().ok());

        let current_hash = match hash_join {
            Ok(Ok(h)) => h,
            Ok(Err(e)) => {
                tracing::warn!(
                    "phase_auto_import: hash failed for {}: {e}",
                    xml_path.display()
                );
                return;
            }
            Err(e) => {
                tracing::warn!("phase_auto_import: hash task panicked: {e}");
                return;
            }
        };

        let current_platform_hash =
            replay_control_core::systems::launchbox_platform_map_fingerprint();

        // Re-parse on any input change:
        //   - XML content (hash) — upstream LaunchBox data changed
        //   - normalizer version — `title_utils::normalize_title_for_metadata`
        //     output changed, so cached `provider_alternate.normalized_alternate`
        //     is stale
        //   - platform-map fingerprint — a system was added/removed or its
        //     `launchbox_platforms` field changed (e.g. arcade_stv joining),
        //     so games previously dropped for "no matching system" need a
        //     second look
        // `refresh_launchbox` writes all three stamps in one transaction, so a
        // single pass clears every gate.
        let hash_matches = stored_hash.as_deref() == Some(current_hash.as_str());
        let version_matches =
            stored_norm_version == Some(replay_control_core::title_utils::TITLE_NORM_VERSION);
        let platform_hash_matches =
            stored_platform_hash.as_deref() == Some(current_platform_hash.as_str());
        if hash_matches && version_matches && platform_hash_matches {
            tracing::debug!(
                "phase_auto_import: LaunchBox XML hash + title_norm_version + platform_map_hash all match — skipping refresh"
            );
            return;
        }

        let reason = if !hash_matches {
            format!("hash {current_hash} differs from stored {:?}", stored_hash)
        } else if !version_matches {
            format!(
                "title_norm_version {} differs from stored {:?}",
                replay_control_core::title_utils::TITLE_NORM_VERSION,
                stored_norm_version
            )
        } else {
            format!(
                "platform_map_hash {current_platform_hash} differs from stored {:?}",
                stored_platform_hash
            )
        };
        tracing::info!(
            "phase_auto_import: refreshing external_metadata.db from {} ({reason})",
            xml_path.display()
        );

        state.update_activity(|act| {
            if let Activity::RefreshExternalMetadata { progress } = act {
                progress.phase = RefreshMetadataPhase::Parsing;
            }
        });

        // Surface parse progress to SSE so the UI banner doesn't sit frozen
        // for the 30–90 s parse on Pi. Parse/build runs on the blocking pool
        // before the SQLite writer is acquired, so the external_metadata
        // writer slot is held only while rows are applied.
        let xml_for_task = xml_path.clone();
        let progress_state = state.clone();
        let prepared = match tokio::task::spawn_blocking(move || {
            replay_control_core_server::library::external_metadata_refresh::prepare_launchbox_refresh(
                &xml_for_task,
                move |processed| {
                    progress_state.update_activity(|act| {
                        if let Activity::RefreshExternalMetadata { progress } = act {
                            progress.source_entries = processed;
                        }
                    });
                },
            )
        })
        .await
        {
            Ok(Ok(prepared)) => prepared,
            Ok(Err(e)) => {
                tracing::warn!("phase_auto_import: refresh prepare failed: {e}");
                fail_refresh_metadata(state, start, e.to_string());
                return;
            }
            Err(e) => {
                tracing::warn!("phase_auto_import: refresh prepare task panicked: {e}");
                fail_refresh_metadata(state, start, e.to_string());
                return;
            }
        };

        let result = state
            .external_metadata_writer
            .try_write_with_timeout(EXTERNAL_METADATA_REFRESH_TIMEOUT, move |conn| {
                replay_control_core_server::library::external_metadata_refresh::apply_launchbox_refresh(
                    conn,
                    prepared,
                )
            })
            .await;

        let stats = match result {
            Ok(Ok(stats)) => stats,
            Ok(Err(e)) => {
                tracing::warn!("phase_auto_import: refresh failed: {e}");
                fail_refresh_metadata(state, start, e.to_string());
                return;
            }
            Err(e) => {
                tracing::warn!("phase_auto_import: external_metadata pool write failed: {e}");
                fail_refresh_metadata(state, start, e.to_string());
                return;
            }
        };

        tracing::info!(
            "phase_auto_import: refresh complete — {} games, {} alternates from {} source entries",
            stats.games_written,
            stats.alternates_written,
            stats.source_entries
        );

        // Re-enrichment: provider data just changed, so flush it through
        // game_library + game_detail_metadata for every system the user has.
        // Without this, the request path keeps showing pre-refresh data
        // until something else triggers enrichment (storage swap, rebuild).
        state.update_activity(|act| {
            if let Activity::RefreshExternalMetadata { progress } = act {
                progress.phase = RefreshMetadataPhase::Enriching;
                progress.source_entries = stats.source_entries;
            }
        });
        Self::reenrich_all_systems(state).await;

        state.update_activity(|act| {
            if let Activity::RefreshExternalMetadata { progress } = act {
                progress.phase = RefreshMetadataPhase::Complete;
                progress.source_entries = stats.source_entries;
                progress.elapsed_secs = start.elapsed().as_secs();
            }
        });
        // `guard` drops at end of scope → ActivityGuard::Drop broadcasts Idle.
        drop(guard);
    }

    /// After an external_metadata refresh, iterate every system in the
    /// library DB that has ROM rows and re-run enrichment so the new provider data
    /// flows into `game_library` + `game_detail_metadata`. Does nothing on a
    /// fresh / empty library.
    async fn reenrich_all_systems(state: &AppState) {
        let active =
            super::library_systems::active_systems(&state.library_reader, "post_refresh_reenrich")
                .await;
        if active.is_empty() {
            return;
        }
        tracing::info!(
            "post-refresh re-enrichment starting for {} system(s)",
            active.len()
        );
        for system in active {
            state.cache.enrich_system_cache(state, system).await;
        }
        state.invalidate_user_caches().await;
    }

    /// Phase 2: strict startup reconciliation across every visible system.
    ///
    /// Works directly with the DB and filesystem — does NOT use UI summary
    /// views or cached ROM readers to avoid circular dependencies.
    async fn phase_cache_verification(state: &AppState) {
        let storage = state.storage();
        let generation = state.storage_generation();
        let region_pref = state.region_preference();
        let region_secondary = state.region_preference_secondary();

        match Self::populate_all_systems(
            state,
            &storage,
            region_pref,
            region_secondary,
            PopulateProgress::Startup,
            ScanOptions {
                force_rehash: false,
                skip_unchanged_startup: true,
            },
            generation,
        )
        .await
        {
            Ok(()) => {}
            Err(e) if Self::is_storage_changed(&e) => {
                tracing::info!("Startup library scan cancelled because storage changed");
            }
            Err(e) => tracing::warn!("Startup library scan failed: {e}"),
        }
    }

    /// Phase: re-resolve header-cart `ra_id` (NES/SNES/N64) from each row's
    /// persisted `rc_hash` against the current catalog `ra_hash` table.
    ///
    /// Runs every startup and is idempotent (writes only changed rows). The
    /// enrichment-inputs reconcile rescan skips systems whose ROM files are
    /// unchanged, so a catalog-only RA refresh would otherwise never update a
    /// header cart's `ra_id` — its value is scan-derived (rc_hash → ra_hash),
    /// not catalog-lookup-derived like genre. Needs no file I/O: the `rc_hash`
    /// is already stored, so this is a catalog batch lookup + targeted UPDATE.
    async fn phase_reresolve_rc_hash_ra_ids(state: &AppState) {
        let systems =
            super::library_systems::active_systems(&state.library_reader, "reresolve_rc_hash")
                .await;
        for system in systems {
            if !rom_hash::needs_rc_hash(&system) && !rc_hash_disc::is_disc_rc_hash_system(&system) {
                continue;
            }
            let sys_read = system.clone();
            let Some(pairs) =
                try_read_or_skip(&state.library_reader, "reresolve_rc_hash", move |conn| {
                    LibraryDb::rc_hash_pairs(conn, &sys_read)
                })
                .await
            else {
                continue;
            };
            if pairs.is_empty() {
                continue;
            }
            let hashes: Vec<String> = pairs.iter().map(|(_, h)| h.clone()).collect();
            // `None` = the catalog query failed. Skip the write entirely: turning
            // a failed lookup into empty `ra_id`s would WIPE good data for every
            // row (a transient catalog error is not "no RA set"). `Some(empty)`
            // is fine — it legitimately clears rows whose RA set went away.
            let Some(ra_map) = game_db::lookup_ra_id_by_rc_hash_batch(&system, &hashes).await
            else {
                tracing::warn!("reresolve_rc_hash: {system}: catalog lookup failed; skipping");
                continue;
            };
            let updates: Vec<(String, String)> = pairs
                .into_iter()
                .map(|(rom, h)| (rom, ra_map.get(&h).cloned().unwrap_or_default()))
                .collect();
            let sys_write = system.clone();
            let result = state
                .library_writer
                .try_write_with_timeout(LIBRARY_MAINTENANCE_WRITE_TIMEOUT, move |conn| {
                    let changed = LibraryDb::set_ra_ids(conn, &sys_write, &updates)?;
                    // This phase runs AFTER the startup stats refresh, so when it
                    // changes ra_id it must re-refresh the system's stats — the
                    // metadata page reads RA coverage from game_library_system_stats.
                    if changed > 0 {
                        LibraryDb::refresh_game_library_system_stats(conn, &sys_write)?;
                    }
                    Ok::<_, replay_control_core::error::Error>(changed)
                })
                .await;
            match result {
                Ok(Ok(changed)) if changed > 0 => {
                    tracing::info!("reresolve_rc_hash: {system}: {changed} ra_id(s) updated")
                }
                Ok(Ok(_)) => {}
                Ok(Err(e)) => tracing::warn!("reresolve_rc_hash: {system}: {e}"),
                Err(e) => tracing::warn!("reresolve_rc_hash: {system}: writer unavailable: {e}"),
            }
        }
    }

    /// Phase: detect changes in bundled enrichment inputs (catalog DB
    /// rows + Shmups Wiki page index + matcher) and rescan affected systems
    /// when the per-storage stamp is stale. The composite version comes from
    /// [`replay_control_core_server::library::enrichment::enrichment_inputs_version`]
    /// so every input that affects scan/enrichment output is rolled into one
    /// stamp.
    ///
    /// A rescan (not just a re-enrich) is required because several catalog-
    /// derived per-ROM fields — `ra_id`, `board`, `developer`,
    /// normalized titles — are populated by the scan path, not enrichment, so
    /// re-enriching alone would leave them stale after a catalog refresh.
    async fn phase_enrichment_inputs_reconcile(state: &AppState) {
        use replay_control_core_server::library::enrichment;
        use replay_control_core_server::library_db::library_meta;

        let Some(current_version) = enrichment::enrichment_inputs_version().await else {
            return;
        };

        let stored_version = state
            .library_reader
            .read(|conn| {
                library_meta::read_meta(conn, library_meta::keys::ENRICHMENT_INPUTS_VERSION)
            })
            .await
            .unwrap_or_default();
        if stored_version.as_deref() == Some(current_version.as_str()) {
            return;
        }

        let with_games = super::library_systems::active_systems(
            &state.library_reader,
            "enrichment_inputs_reconcile",
        )
        .await;
        if with_games.is_empty() {
            return;
        }

        // The catalog's enrichment inputs changed (e.g. a refreshed
        // RetroAchievements extract or detail metadata). Rescan every system
        // rather than re-enrich: `force_rehash: false` reuses the cached CRC32s
        // (no ROM re-streaming on NFS), while `skip_unchanged_startup: false`
        // rebuilds each system's `game_library` rows against the new catalog
        // even when its ROMs are unchanged — the steady-state case for a
        // catalog-only refresh.
        tracing::info!(
            "Enrichment inputs version changed; rescanning {} system(s)",
            with_games.len()
        );
        let storage = state.storage();
        let generation = state.storage_generation();
        let region_pref = state.region_preference();
        let region_secondary = state.region_preference_secondary();
        match Self::populate_all_systems(
            state,
            &storage,
            region_pref,
            region_secondary,
            PopulateProgress::Startup,
            ScanOptions {
                force_rehash: false,
                skip_unchanged_startup: false,
            },
            generation,
        )
        .await
        {
            Ok(()) => {}
            Err(e) if Self::is_storage_changed(&e) => {
                tracing::info!("Enrichment reconcile rescan cancelled because storage changed");
                return;
            }
            Err(e) => {
                tracing::warn!(
                    "Enrichment reconcile rescan failed: {e}; inputs version not stamped, startup will retry"
                );
                return;
            }
        }

        let version = current_version.clone();
        let write_result = state
            .library_writer
            .try_write_with_timeout(LIBRARY_MAINTENANCE_WRITE_TIMEOUT, move |conn| {
                library_meta::write_meta(
                    conn,
                    library_meta::keys::ENRICHMENT_INPUTS_VERSION,
                    Some(&version),
                )
            })
            .await;
        match write_result {
            Ok(Ok(())) => {
                tracing::info!("Enrichment reconcile stamped inputs version {current_version}")
            }
            Ok(Err(e)) => tracing::warn!("Enrichment inputs version write failed: {e}"),
            Err(e) => tracing::warn!("Enrichment inputs version write failed: {e}"),
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
        use replay_control_core_server::external_metadata;

        // Check data_source for libretro-thumbnails entries and thumbnail_manifest emptiness.
        let (has_sources, index_empty) = match state
            .external_metadata_reader
            .read(|conn| {
                let stats =
                    external_metadata::get_data_source_stats(conn, "libretro-thumbnails").ok()?;
                let index_count: i64 =
                    external_metadata::thumbnail_manifest_count(conn).unwrap_or(0);
                Some((stats.repo_count > 0, index_count == 0))
            })
            .await
            .flatten()
        {
            Some(result) => result,
            None => return, // pool unavailable
        };

        if !has_sources {
            // No data_sources entries. Check if images exist on disk — if so,
            // someone previously downloaded thumbnails but the DB was deleted.
            let has_images_on_disk = replay_control_core_server::thumbnails::any_images_on_disk(
                &state.storage().rc_dir(),
            );
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

        state.update_activity(|act| {
            if let Activity::Startup { phase, .. } = act {
                *phase = StartupPhase::RebuildingIndex;
            }
        });

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

            let Some(repo_names) = system_thumbnail_repos(&system_str) else {
                continue;
            };

            let all_entries =
                replay_control_core_server::thumbnails::scan_system_images(&system_entry.path());

            if all_entries.is_empty() {
                continue;
            }

            system_data.push(SystemImageData {
                system_str,
                repo_names,
                entries: all_entries,
            });
        }

        // Now write all collected data to external_metadata.db in one txn.
        let write_result = state
            .external_metadata_writer
            .try_write(move |db| {
                let tx = match db.transaction() {
                    Ok(t) => t,
                    Err(e) => {
                        tracing::warn!("phase_auto_rebuild_thumbnail_index: begin failed: {e}");
                        return (0usize, 0usize);
                    }
                };
                let mut w_total_entries = 0usize;
                let mut w_total_repos = 0usize;

                for data in &system_data {
                    let repo_display = data.repo_names[0];
                    let source_name =
                        replay_control_core_server::thumbnails::libretro_source_name(repo_display);
                    let branch = replay_control_core_server::thumbnail_manifest::default_branch(
                        repo_display,
                    );
                    let entry_count = data.entries.len();

                    if let Err(e) = external_metadata::upsert_data_source(
                        &tx,
                        &source_name,
                        "libretro-thumbnails",
                        "disk-rebuild",
                        branch,
                        entry_count,
                    ) {
                        tracing::warn!("Failed to upsert data source {source_name}: {e}");
                    }

                    let _ = external_metadata::delete_thumbnail_manifest(&tx, &source_name);
                    match external_metadata::insert_thumbnail_manifest_rows(
                        &tx,
                        &source_name,
                        &data.entries,
                    ) {
                        Ok(_) => w_total_entries += entry_count,
                        Err(e) => tracing::warn!(
                            "Failed to insert disk-based index for {}: {e}",
                            data.system_str
                        ),
                    }

                    // Register additional repos for multi-repo systems (e.g., arcade_dc → Naomi + Naomi 2).
                    for extra_repo in &data.repo_names[1..] {
                        let extra_source =
                            replay_control_core_server::thumbnails::libretro_source_name(
                                extra_repo,
                            );
                        let extra_branch =
                            replay_control_core_server::thumbnail_manifest::default_branch(
                                extra_repo,
                            );
                        if let Err(e) = external_metadata::upsert_data_source(
                            &tx,
                            &extra_source,
                            "libretro-thumbnails",
                            "disk-rebuild",
                            extra_branch,
                            0,
                        ) {
                            tracing::warn!("Failed to upsert data source {extra_source}: {e}");
                        }
                    }
                    w_total_repos += data.repo_names.len();
                }

                if let Err(e) = tx.commit() {
                    tracing::warn!("phase_auto_rebuild_thumbnail_index: commit failed: {e}");
                }
                (w_total_entries, w_total_repos)
            })
            .await;

        let Ok((total_entries, total_repos)) = write_result else {
            tracing::warn!(
                "Thumbnail index disk rebuild write skipped: external_metadata unavailable"
            );
            return; // DB unavailable
        };

        if total_entries > 0 {
            tracing::info!(
                "Thumbnail index rebuilt from disk: {total_entries} entries across {total_repos} repos"
            );
        }
    }

    async fn refresh_thumbnail_media_stats(
        state: &AppState,
        storage: &StorageLocation,
        generation: u64,
        phase: &'static str,
    ) -> Result<(), replay_control_core::error::Error> {
        state.ensure_storage_generation(generation)?;
        let root = storage.root.clone();
        let started = Instant::now();
        let stats = tokio::task::spawn_blocking(move || {
            replay_control_core_server::thumbnails::scan_media_stats(&root)
        })
        .await
        .map_err(|e| {
            replay_control_core::error::Error::Other(format!(
                "{phase}: thumbnail media stat scan task panicked: {e}"
            ))
        })?;
        state.ensure_storage_generation(generation)?;
        let system_count = stats.len();
        let file_count: usize = stats.iter().map(|s| s.file_count).sum();
        let total_size_bytes: u64 = stats.iter().map(|s| s.total_size_bytes).sum();
        let write = state
            .library_writer
            .try_write(move |conn| LibraryDb::replace_thumbnail_media_stats(conn, &stats))
            .await;
        match write {
            Ok(Ok(())) => tracing::info!(
                "{phase}: thumbnail media stats refreshed systems={system_count} files={file_count} bytes={total_size_bytes} in {}ms",
                started.elapsed().as_millis()
            ),
            Ok(Err(e)) => tracing::warn!("{phase}: thumbnail media stats SQL failed: {e}"),
            Err(e) => tracing::warn!("{phase}: thumbnail media stats write failed: {e}"),
        }
        Ok(())
    }

    /// Pre-populate L2 cache for all systems that have games. Walks ROM
    /// directories, hashes new files, and enriches box art / ratings.
    pub(crate) async fn populate_all_systems(
        state: &AppState,
        storage: &replay_control_core_server::storage::StorageLocation,
        region_pref: replay_control_core::rom_tags::RegionPreference,
        region_secondary: Option<replay_control_core::rom_tags::RegionPreference>,
        progress: PopulateProgress,
        options: ScanOptions,
        generation: u64,
    ) -> Result<(), replay_control_core::error::Error> {
        // Iterate every visible_systems() platform — strict reconcile is
        // safe to call on systems we don't have on disk (it early-returns
        // cheaply via list_roms's missing-dir branch on local
        // storage, returns Err on NFS preserving cached state). The per-
        // system rule (in `scan_and_cache_system`) ensures empty walks
        // don't poison meta on partial-mount fresh boots.
        let systems: Vec<&'static replay_control_core::systems::System> =
            replay_control_core::systems::visible_systems().collect();
        let total = systems.len();

        tracing::info!("L2 populate: {total} visible system(s)");

        let start = std::time::Instant::now();

        if let Some(rb_start) = progress.rebuild_start() {
            update_rebuild_progress(state, |p| {
                p.phase = RebuildPhase::Scanning;
                p.current_system = String::new();
                p.systems_done = 0;
                p.systems_total = total;
                p.elapsed_secs = rb_start.elapsed().as_secs();
            });
        }

        let mut total_roms = 0usize;
        let mut identity_jobs = Vec::new();
        for (i, sys) in systems.iter().enumerate() {
            let system_started = Instant::now();
            state.ensure_storage_generation(generation)?;
            report_system(state, progress, i, sys.display_name, false);
            let scan_inputs =
                Self::scan_inputs_for_system(state, sys.folder_name, options, generation).await?;
            let scan_started = Instant::now();
            let scan_result = state
                .cache
                .scan_and_cache_system_with_inputs(
                    storage,
                    sys.folder_name,
                    region_pref,
                    region_secondary,
                    &state.library_writer,
                    &scan_inputs,
                )
                .await;
            let scan_ms = scan_started.elapsed().as_millis();
            match scan_result {
                Ok(outcome) => {
                    let roms = outcome.roms;
                    if !roms.is_empty() {
                        tracing::debug!(
                            "L2 populate: {} — {} ROMs (scan+enrich)",
                            sys.folder_name,
                            roms.len()
                        );
                        total_roms += roms.len();
                    }
                    if !outcome.discovery_changed {
                        tracing::debug!(
                            "L2 populate: {} unchanged; skipping enrichment and identity",
                            sys.folder_name
                        );
                        tracing::info!(
                            "L2 system profile: {}: roms={} scan_ms={scan_ms} enrich_ms=0 total_ms={}",
                            sys.folder_name,
                            roms.len(),
                            system_started.elapsed().as_millis()
                        );
                        continue;
                    }
                    // Inline enrichment runs on every Ok (including
                    // Ok(empty), which clears stale game_detail_metadata rows
                    // when a previously-populated system goes empty).
                    state.ensure_storage_generation(generation)?;
                    report_system(state, progress, i, sys.display_name, true);
                    let enrich_started = Instant::now();
                    // Per-system enrichment is best-effort: a transient failure
                    // (e.g. a flaky box-art lookup) must not abort the whole
                    // rebuild and block later systems — mirror the scan step's
                    // preserve-cached-state policy and move on. Scan-derived
                    // fields (ra_id, board, developer) are already persisted by
                    // the scan above, so they're unaffected. Storage swaps still
                    // abort (the caller redoes the pass on the new storage).
                    if let Err(e) = state
                        .cache
                        .enrich_system_cache_with_cancellation(
                            state,
                            sys.folder_name.to_string(),
                            scan_inputs.cancellation(),
                        )
                        .await
                    {
                        if Self::is_storage_changed(&e) {
                            return Err(e);
                        }
                        tracing::warn!(
                            "L2 populate: {} enrichment skipped (preserving cached state): {e}",
                            sys.folder_name
                        );
                        continue;
                    }
                    let enrich_ms = enrich_started.elapsed().as_millis();
                    if !roms.is_empty() && is_hash_identifiable(sys.folder_name) {
                        identity_jobs.push(IdentityJob {
                            system: sys.folder_name.to_string(),
                            roms: roms.clone(),
                            scan_inputs: scan_inputs.clone(),
                        });
                    }
                    tracing::info!(
                        "L2 system profile: {}: roms={} scan_ms={scan_ms} enrich_ms={enrich_ms} total_ms={}",
                        sys.folder_name,
                        roms.len(),
                        system_started.elapsed().as_millis()
                    );
                }
                Err(e) if Self::is_storage_changed(&e) => return Err(e),
                Err(e) => {
                    tracing::warn!(
                        "L2 populate: {} skipped after {scan_ms}ms (preserving cached state): {e}",
                        sys.folder_name
                    );
                }
            }
        }

        tracing::info!(
            "L2 populate: done — {} ROMs across {} systems in {:.1}s",
            total_roms,
            total,
            start.elapsed().as_secs_f64()
        );

        if let Some(start) = progress.rebuild_start() {
            update_rebuild_progress(state, |p| {
                p.phase = RebuildPhase::MediaStats;
                p.current_system = String::new();
                p.systems_done = p.systems_total;
                p.elapsed_secs = start.elapsed().as_secs();
                p.enriching = false;
            });
        } else if matches!(progress, PopulateProgress::Startup) {
            state.update_activity(|act| {
                if let Activity::Startup {
                    phase,
                    system,
                    enriching,
                } = act
                {
                    *phase = StartupPhase::MediaStats;
                    system.clear();
                    *enriching = false;
                }
            });
        }
        Self::refresh_thumbnail_media_stats(state, storage, generation, "L2 populate").await?;
        Self::spawn_identity_jobs(state.clone(), storage.clone(), identity_jobs, generation);
        Ok(())
    }
    // ── Update system ─────────────────────────────────────────────────

    /// GitHub repository for release checks and downloads.
    const REPO: &'static str = "lapastillaroja/replay-control";
    /// Maximum time for the entire StartUpdate operation (5 minutes).
    const UPDATE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

    /// Periodically checks GitHub for new releases.
    async fn update_check_loop(state: AppState) {
        // Delay first check to let WiFi come up on Pi.
        tokio::time::sleep(Self::update_initial_delay()).await;

        let analytics = super::analytics::AnalyticsClient::new(
            replay_control_core_server::http::shared_client().clone(),
            super::analytics::ENDPOINT,
        );

        loop {
            if state.has_storage() {
                match Self::perform_update_check_background(&state).await {
                    Ok(_) => {}
                    Err(e) => tracing::debug!("Background update check failed: {e}"),
                }

                // Analytics ping — independent from update check, same 24h cadence.
                if let Some((ping, is_install)) =
                    super::analytics::build_analytics_ping(&state.settings)
                {
                    let success = analytics.send(&ping).await;
                    if is_install && success {
                        super::analytics::mark_version_reported(&state.settings);
                    }
                }
            }

            tokio::time::sleep(Self::update_interval()).await;
        }
    }

    fn first_run_seed_enabled() -> bool {
        !env_flag("REPLAY_CONTROL_SKIP_FIRST_RUN_SEED")
    }

    fn update_initial_delay() -> Duration {
        env_duration_secs(
            "REPLAY_CONTROL_UPDATE_INITIAL_DELAY_SECS",
            UPDATE_INITIAL_DELAY_SECS,
            0,
        )
    }

    fn update_interval() -> Duration {
        env_duration_secs(
            "REPLAY_CONTROL_UPDATE_INTERVAL_SECS",
            UPDATE_INTERVAL_SECS,
            1,
        )
    }

    /// Background check variant: does NOT nuke before checking (preserves existing
    /// available.json on error). On success: nuke then write. On no-update: nuke.
    async fn perform_update_check_background(
        state: &AppState,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let settings = state.settings.load();
        let channel =
            replay_control_core::update::UpdateChannel::from_str_value(settings.update_channel());
        let skipped = settings.skipped_version().map(|s| s.to_string());
        let github_key = settings.github_api_key().map(|s| s.to_string());
        drop(settings);

        match update_io::check_github_update(
            crate::VERSION,
            &update_io::github_api_base_url(),
            Self::REPO,
            &channel,
            skipped.as_deref(),
            github_key.as_deref(),
        )
        .await?
        {
            Some(available) => {
                // Race guard: verify channel still matches before writing.
                let current_channel =
                    replay_control_core_server::settings::read_update_channel(&state.settings);
                if current_channel != channel {
                    tracing::debug!(
                        "Update channel changed during check ({} -> {}), discarding result",
                        channel.as_str(),
                        current_channel.as_str()
                    );
                    return Ok(());
                }
                update_io::nuke_update_dir();
                update_io::write_available_update(&available).ok();
                let _ = state
                    .events_tx
                    .send(super::ConfigEvent::UpdateAvailable { update: available });
            }
            None => {
                // No update found — nuke stale state.
                update_io::nuke_update_dir();
            }
        }
        Ok(())
    }

    /// Manual check: nukes first, checks, writes if found, broadcasts SSE.
    pub async fn perform_update_check(
        state: &AppState,
    ) -> Result<
        Option<replay_control_core::update::AvailableUpdate>,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        update_io::nuke_update_dir();

        let settings = state.settings.load();
        let channel =
            replay_control_core::update::UpdateChannel::from_str_value(settings.update_channel());
        let skipped = settings.skipped_version().map(|s| s.to_string());
        let github_key = settings.github_api_key().map(|s| s.to_string());

        match update_io::check_github_update(
            crate::VERSION,
            &update_io::github_api_base_url(),
            Self::REPO,
            &channel,
            skipped.as_deref(),
            github_key.as_deref(),
        )
        .await?
        {
            Some(available) => {
                update_io::write_available_update(&available).ok();
                let _ = state.events_tx.send(super::ConfigEvent::UpdateAvailable {
                    update: available.clone(),
                });
                Ok(Some(available))
            }
            None => Ok(None),
        }
    }

    /// Generate the helper shell script that performs the actual file swap + restart.
    /// `catalog_path` is `None` for releases that don't ship a catalog asset
    /// (< v0.4.0-beta.3); the script then leaves the existing catalog in place.
    pub fn generate_update_script(
        binary_path: &std::path::Path,
        site_path: &std::path::Path,
        catalog_path: Option<&std::path::Path>,
        version: &str,
    ) -> String {
        let catalog_src = catalog_path
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        format!(
            r#"#!/bin/bash

# Auto-generated by replay-control {version}
# Performs file swap, restart, health check, and rollback on failure.

validate_version() {{
    local v="$1"
    if ! echo "$v" | grep -qE '^[0-9a-zA-Z._-]+$'; then
        echo "Invalid version string: $v"
        exit 1
    fi
}}

validate_version "{version}"

# Source environment for PORT variable
if [ -f /etc/default/replay-control ]; then
    . /etc/default/replay-control
fi
PORT="${{PORT:-8080}}"

BINARY_SRC="{binary_src}"
SITE_SRC="{site_src}"
CATALOG_SRC="{catalog_src}"
BINARY_DST="/usr/local/bin/replay-control-app"
SITE_DST="/usr/local/share/replay/site"
CATALOG_DST="/usr/local/bin/catalog.sqlite"

# Asset helpers — applied to each (src, dst) pair.
# An empty SRC means "skip this asset" (e.g. catalog on releases < v0.4.0-beta.3).
backup()  {{ local dst="$1"; [ -e "$dst" ] && cp -a "$dst" "${{dst}}.bak" 2>/dev/null || true; }}
# swap returns non-zero when src is empty so callers' `&& chmod` is skipped.
swap()    {{ local src="$1" dst="$2"; [ -n "$src" ] || return 1; rm -rf "$dst"; mv "$src" "$dst"; }}
unbak()   {{ rm -rf "$1.bak"; }}
restore() {{ local dst="$1"; [ -e "${{dst}}.bak" ] || return 0; rm -rf "$dst"; mv "${{dst}}.bak" "$dst"; }}

# Wait for the HTTP response to reach the client
sleep 2

# Back up current files
backup "$BINARY_DST"
backup "$SITE_DST"
backup "$CATALOG_DST"

# Swap files
swap "$BINARY_SRC" "$BINARY_DST" && chmod +x "$BINARY_DST"
swap "$SITE_SRC"   "$SITE_DST"
swap "$CATALOG_SRC" "$CATALOG_DST" && chmod 644 "$CATALOG_DST"

# Restart service
systemctl restart replay-control

# Health check: poll every 2s, up to 30 attempts (60s total)
ATTEMPT=0
MAX_ATTEMPTS=30
while [ "$ATTEMPT" -lt "$MAX_ATTEMPTS" ]; do
    sleep 2
    ATTEMPT=$((ATTEMPT + 1))
    if curl -sf --max-time 10 "http://localhost:${{PORT}}/api/version" > /dev/null 2>&1; then
        # Success: remove backups
        unbak "$BINARY_DST"
        unbak "$SITE_DST"
        unbak "$CATALOG_DST"
        rm -rf "{update_dir}"
        rm -f "$0"
        exit 0
    fi
done

# Failure: restore backups
restore "$BINARY_DST"
restore "$SITE_DST"
restore "$CATALOG_DST"
systemctl restart replay-control
rm -rf "{update_dir}"
rm -f "$0"
exit 1
"#,
            version = version,
            binary_src = binary_path.display(),
            site_src = site_path.display(),
            catalog_src = catalog_src,
            update_dir = replay_control_core::update::UPDATE_DIR,
        )
    }

    /// Execute the full update flow: resolve URLs, download, extract, generate + spawn helper.
    pub async fn start_update(
        state: &super::AppState,
        tag: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if !replay_control_core::update::validate_version(tag) {
            return Err(format!("Invalid version tag: {tag}").into());
        }

        match tokio::time::timeout(Self::UPDATE_TIMEOUT, Self::start_update_inner(state, tag)).await
        {
            Ok(result) => result,
            Err(_) => {
                update_io::nuke_update_dir();
                Err("Update timed out after 5 minutes".into())
            }
        }
    }

    async fn start_update_inner(
        state: &super::AppState,
        tag: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use super::activity::{UpdatePhase, UpdateProgress};

        let start_time = std::time::Instant::now();

        // Acquire activity lock.
        let guard = state
            .try_start_activity(super::activity::Activity::Update {
                progress: UpdateProgress {
                    phase: UpdatePhase::Downloading,
                    downloaded_bytes: 0,
                    total_bytes: 0,
                    phase_detail: "Resolving download URLs...".to_string(),
                    elapsed_secs: 0,
                    error: None,
                },
            })
            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.into() })?;

        let result = Self::start_update_download(state, tag, &guard, start_time).await;

        match result {
            Ok(()) => {
                // Set Restarting and leak guard — process will be killed by helper script.
                state.update_activity(|act| {
                    if let super::activity::Activity::Update { progress } = act {
                        progress.phase = UpdatePhase::Restarting;
                        progress.phase_detail = "Restarting service...".to_string();
                        progress.elapsed_secs = start_time.elapsed().as_secs();
                    }
                });
                std::mem::forget(guard);
                Ok(())
            }
            Err(ref e) => {
                let error_msg = e.to_string();
                guard.update(|act| {
                    if let super::activity::Activity::Update { progress } = act {
                        progress.phase = UpdatePhase::Failed;
                        progress.phase_detail = error_msg.clone();
                        progress.error = Some(error_msg.clone());
                        progress.elapsed_secs = start_time.elapsed().as_secs();
                    }
                });
                update_io::nuke_update_dir();
                result
            }
        }
    }

    async fn start_update_download(
        state: &super::AppState,
        tag: &str,
        guard: &super::activity::ActivityGuard,
        start_time: std::time::Instant,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use super::activity::UpdatePhase;
        use replay_control_core::update::{UPDATE_DIR, UPDATE_LOCK, UPDATE_SCRIPT};

        let github_key = replay_control_core_server::settings::read_github_api_key(&state.settings);
        let base_url = update_io::github_api_base_url();
        let update_dir = std::path::PathBuf::from(UPDATE_DIR);

        // Acquire file lock (outside update dir, survives nukes).
        let lock_file = std::fs::File::create(UPDATE_LOCK)?;
        use std::os::unix::io::AsRawFd;
        let fd = lock_file.as_raw_fd();
        if unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err("Another update is already in progress".into());
        }

        // Nuke update dir before starting.
        update_io::nuke_update_dir();
        tokio::fs::create_dir_all(&update_dir).await?;

        // Resolve asset URLs.
        let assets =
            update_io::resolve_asset_urls(&base_url, Self::REPO, tag, github_key.as_deref())
                .await?;

        // Use actual sizes from available.json for progress reporting.
        let stored_update = update_io::read_available_update();
        let binary_size = stored_update.as_ref().map(|u| u.binary_size).unwrap_or(0);
        let site_size = stored_update.as_ref().map(|u| u.site_size).unwrap_or(0);
        // Catalog may be absent on releases < v0.4.0-beta.3 — assets.catalog_url
        // is the source of truth; the size hint just feeds progress reporting.
        let catalog_size = stored_update
            .as_ref()
            .map(|u| u.catalog_size)
            .filter(|_| assets.catalog_url.is_some())
            .unwrap_or(0);
        let total_bytes = binary_size + site_size + catalog_size;

        // Check disk space (require 2x total for archives + extracted).
        if total_bytes > 0 {
            let stat = nix::sys::statvfs::statvfs(update_dir.to_str().unwrap_or("/var/tmp"))?;
            let available = stat.blocks_available() as u64 * stat.fragment_size() as u64;
            let required = total_bytes * 2;
            if available < required {
                return Err(format!(
                    "Insufficient disk space: need {} MB, have {} MB",
                    required / (1024 * 1024),
                    available / (1024 * 1024),
                )
                .into());
            }
        }

        // Download binary.
        let binary_archive = update_dir.join("binary.tar.gz");
        {
            let activity_state = state.activity.clone();
            let activity_tx = state.activity_tx.clone();
            let start = start_time;

            update_io::download_asset(&assets.binary_url, &binary_archive, &move |bytes| {
                let mut act = activity_state.write().expect("activity lock");
                if let super::activity::Activity::Update { progress } = &mut *act {
                    progress.downloaded_bytes = bytes;
                    progress.total_bytes = total_bytes;
                    progress.phase_detail = "Downloading binary...".to_string();
                    progress.elapsed_secs = start.elapsed().as_secs();
                }
                let activity = act.clone();
                drop(act);
                let _ = activity_tx.send(activity);
            })
            .await?;
        }

        // Download site archive.
        let site_archive = update_dir.join("site.tar.gz");
        {
            let activity_state = state.activity.clone();
            let activity_tx = state.activity_tx.clone();
            let start = start_time;

            update_io::download_asset(&assets.site_url, &site_archive, &move |bytes| {
                let mut act = activity_state.write().expect("activity lock");
                if let super::activity::Activity::Update { progress } = &mut *act {
                    progress.downloaded_bytes = binary_size + bytes;
                    progress.total_bytes = total_bytes;
                    progress.phase_detail = "Downloading site assets...".to_string();
                    progress.elapsed_secs = start.elapsed().as_secs();
                }
                let activity = act.clone();
                drop(act);
                let _ = activity_tx.send(activity);
            })
            .await?;
        }

        let catalog_archive = update_dir.join("catalog.tar.gz");
        if let Some(catalog_url) = &assets.catalog_url {
            let activity_state = state.activity.clone();
            let activity_tx = state.activity_tx.clone();
            let start = start_time;
            let downloaded_so_far = binary_size + site_size;

            update_io::download_asset(catalog_url, &catalog_archive, &move |bytes| {
                let mut act = activity_state.write().expect("activity lock");
                if let super::activity::Activity::Update { progress } = &mut *act {
                    progress.downloaded_bytes = downloaded_so_far + bytes;
                    progress.total_bytes = total_bytes;
                    progress.phase_detail = "Downloading catalog...".to_string();
                    progress.elapsed_secs = start.elapsed().as_secs();
                }
                let activity = act.clone();
                drop(act);
                let _ = activity_tx.send(activity);
            })
            .await?;
        }

        // Extract archives.
        guard.update(|act| {
            if let super::activity::Activity::Update { progress } = act {
                progress.phase = UpdatePhase::Installing;
                progress.phase_detail = "Extracting archives...".to_string();
                progress.elapsed_secs = start_time.elapsed().as_secs();
            }
        });

        let binary_dir = update_dir.join("binary");
        let site_dir = update_dir.join("site");
        tokio::fs::create_dir_all(&binary_dir).await?;
        tokio::fs::create_dir_all(&site_dir).await?;
        Self::extract_tarball(&binary_archive, &binary_dir).await?;
        Self::extract_tarball(&site_archive, &site_dir).await?;

        let mut catalog_path: Option<std::path::PathBuf> = None;
        if assets.catalog_url.is_some() {
            let catalog_dir = update_dir.join("catalog");
            tokio::fs::create_dir_all(&catalog_dir).await?;
            Self::extract_tarball(&catalog_archive, &catalog_dir).await?;
            catalog_path = Some(
                Self::find_extracted_file(&catalog_dir, "catalog.sqlite")
                    .await
                    .ok_or("Extracted catalog archive does not contain catalog.sqlite")?,
            );
        }

        // Resilient: search for the binary within extracted contents.
        let binary_path = Self::find_extracted_file(&binary_dir, "replay-control-app")
            .await
            .ok_or("Extracted binary not found")?;

        // Resilient: search for pkg/ directory within extracted site.
        let actual_site_dir = Self::find_extracted_dir_containing(&site_dir, "pkg")
            .await
            .ok_or("Extracted site directory does not contain pkg/")?;

        // Generate helper script.
        let version = tag.strip_prefix('v').unwrap_or(tag);
        let script = Self::generate_update_script(
            &binary_path,
            &actual_site_dir,
            catalog_path.as_deref(),
            version,
        );
        let script_path = std::path::PathBuf::from(UPDATE_SCRIPT);
        tokio::fs::write(&script_path, &script).await?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))
                .await?;
        }

        // Spawn the helper script via systemd-run so it survives our restart.
        std::process::Command::new("systemd-run")
            .args([
                "--scope",
                "--unit=replay-control-update",
                "--quiet",
                "/bin/bash",
                script_path.to_str().unwrap(),
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;

        // Keep lock_file alive until here.
        drop(lock_file);

        Ok(())
    }

    /// Streaming gunzip + untar of a `.tar.gz` archive into `dest`.
    async fn extract_tarball(
        archive: &std::path::Path,
        dest: &std::path::Path,
    ) -> Result<(), std::io::Error> {
        let archive = archive.to_path_buf();
        let dest = dest.to_path_buf();
        tokio::task::spawn_blocking(move || {
            let gz = flate2::read::GzDecoder::new(std::fs::File::open(&archive)?);
            tar::Archive::new(gz).unpack(&dest)
        })
        .await
        .map_err(std::io::Error::other)?
    }

    /// Search for a file by name within an extracted directory tree.
    async fn find_extracted_file(dir: &std::path::Path, name: &str) -> Option<std::path::PathBuf> {
        // Check direct child first.
        let direct = dir.join(name);
        if direct.exists() {
            return Some(direct);
        }
        // Search one level deep.
        let mut entries = tokio::fs::read_dir(dir).await.ok()?;
        while let Ok(Some(entry)) = entries.next_entry().await {
            if entry.file_type().await.ok()?.is_dir() {
                let candidate = entry.path().join(name);
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
        None
    }

    /// Search for a directory containing a specific subdirectory.
    async fn find_extracted_dir_containing(
        dir: &std::path::Path,
        subdir: &str,
    ) -> Option<std::path::PathBuf> {
        if dir.join(subdir).exists() {
            return Some(dir.to_path_buf());
        }
        let mut entries = tokio::fs::read_dir(dir).await.ok()?;
        while let Ok(Some(entry)) = entries.next_entry().await {
            if entry.file_type().await.ok()?.is_dir() && entry.path().join(subdir).exists() {
                return Some(entry.path());
            }
        }
        None
    }
}

// ── Methods that remain on AppState ────────────────────────────────
//
// These are the long-running watchers and the cache enrichment helper
// that various parts of the code still call on AppState.
impl AppState {
    /// Re-enrich game library after a metadata or thumbnail import.
    ///
    /// If the library is empty (e.g. fresh DB), `populate_all_systems`
    /// runs scan + inline enrich for every visible system; nothing more
    /// is needed.
    ///
    /// Otherwise the per-system enrich loop is the *primary* job here —
    /// it's the path that picks up newly-imported external_metadata for
    /// already-cached systems.
    pub fn spawn_cache_enrichment(&self) {
        let state = self.clone();
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
                match BackgroundManager::populate_all_systems(
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
                    Err(e) if BackgroundManager::is_storage_changed(&e) => {
                        tracing::info!("Post-import populate cancelled because storage changed");
                    }
                    Err(e) => tracing::warn!("Post-import populate failed: {e}"),
                }
            } else {
                // Enrichment-only re-pass: pick up the newly-imported
                // external_metadata for already-cached systems. NOT
                // gated because enrich_system_cache reads from the DB
                // and the write gate blocks ALL reads on the same pool.
                // Enrichment writes are small per-system UPDATEs.
                let with_games = super::library_systems::active_systems(
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
                        state.cache.enrich_system_cache(&state, system).await;
                    }
                    tracing::info!(
                        "Post-import enrichment: done in {:.1}s",
                        enrich_start.elapsed().as_secs_f64()
                    );
                }
            }
        });
    }

    /// Strict-reconcile every visible system in place (preserving cached
    /// rows on per-system FS errors), then mark complete. Inline enrichment
    /// is part of `populate_all_systems`, so there's no separate post-loop
    /// enrichment pass. Rescan and rebuild share this body; the `is_rescan`
    /// flag on `RebuildProgress` (set by the calling server fn before
    /// spawning) only changes UI verbiage.
    pub fn spawn_populate(&self, guard: super::activity::ActivityGuard, scan_hint: bool) {
        let state = self.clone();
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

            let populate_result = BackgroundManager::populate_all_systems(
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
                Err(e) if BackgroundManager::is_storage_changed(&e) => {
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

    /// Spawn the device-side storage-detection watchers: a `notify` watcher
    /// on `replay.cfg` (config-file edits) and a `/proc/self/mountinfo`
    /// POLLPRI watcher (mount-table changes — Linux only). Standalone has
    /// nothing to watch: replay.cfg is RePlayOS-owned and irrelevant
    /// off-device, and the `--storage-path` folder's liveness surfaces at
    /// request time via IO errors. The mountinfo watcher does a full
    /// `reload_config_and_redetect_storage` so the boot-recovery case
    /// (booted `ConfigUnavailable`, then the SD appears) adopts the new
    /// `replay.cfg` along with the storage change.
    pub fn spawn_storage_watcher(self) {
        if !self.mode.is_device() {
            return;
        }

        super::mountinfo_watcher::spawn(self.clone());

        let config_path = self.config_file_path();
        tokio::spawn(async move {
            if Self::try_start_config_watcher(self, config_path).await {
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
    pub fn spawn_rom_watcher(&self) {
        let generation = self.rom_watcher_generation.load(Ordering::Relaxed);
        let storage = self.storage();
        if !storage.kind.is_local() {
            let reason = format!(
                "Filesystem events are unreliable on {} storage; trigger manual rescan after copying ROMs",
                storage.kind.as_str().to_uppercase()
            );
            tracing::debug!("ROM watcher skipped: {reason}");
            self.set_rom_watcher_status(RomWatcherStatus::Skipped { reason });
            return;
        }

        let roms_dir = storage.roms_dir();
        if !roms_dir.exists() {
            let reason = format!("ROM directory not found at {}", roms_dir.display());
            tracing::debug!("ROM watcher skipped: {reason}");
            self.set_rom_watcher_status(RomWatcherStatus::Skipped { reason });
            return;
        }

        let state = self.clone();
        tokio::spawn(async move {
            match Self::try_start_rom_watcher(state.clone(), roms_dir, generation).await {
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
    pub fn restart_rom_watcher(&self) {
        self.rom_watcher_generation.fetch_add(1, Ordering::Relaxed);
        self.spawn_rom_watcher();
    }

    /// Try to set up a `notify` filesystem watcher on the `roms/` directory.
    /// Returns an error string if the watcher could not be created or registered.
    ///
    /// Watches recursively for create/modify/remove events. On change,
    /// extracts the affected system folder name from the event path and
    /// triggers `get_roms` + `enrich_system_cache` after a debounce window.
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

                if !Self::is_rom_event(&event) {
                    continue;
                }

                // Collect affected system folder names from this and subsequent
                // events within the debounce window.
                let mut affected_systems = std::collections::HashSet::new();
                let mut roms_dir_changed = false;
                let mut favorites_changed = false;
                let mut recents_changed = false;
                Self::collect_rom_event_systems(
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
                            if Self::is_rom_event(&ev) {
                                Self::collect_rom_event_systems(
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
                    state.cache.invalidate_favorites().await;
                    state.invalidate_user_caches().await;
                }
                if recents_changed {
                    tracing::debug!("ROM watcher: _recent/ changed, invalidating cache");
                    state.cache.invalidate_recents().await;
                    state.cache.invalidate_recommendations().await;
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
                    state.cache.invalidate_l1().await;
                    state.invalidate_user_caches().await;
                }

                for system in &to_scan {
                    let scan_inputs = match BackgroundManager::scan_inputs_for_system(
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
                        Err(e) if BackgroundManager::is_storage_changed(&e) => {
                            tracing::info!("ROM watcher: storage changed, cancelling rescan");
                            break;
                        }
                        Err(e) => {
                            tracing::warn!("ROM watcher: could not prepare scan for {system}: {e}");
                            continue;
                        }
                    };
                    match state
                        .cache
                        .scan_and_cache_system_with_inputs(
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
                                .cache
                                .enrich_system_cache_with_cancellation(
                                    &state,
                                    system.clone(),
                                    scan_inputs.cancellation(),
                                )
                                .await
                            {
                                Ok(()) => {}
                                Err(e) if BackgroundManager::is_storage_changed(&e) => {
                                    tracing::info!(
                                        "ROM watcher: storage changed during enrichment"
                                    );
                                    break;
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        "ROM watcher: enrichment failed for {system}: {e}"
                                    )
                                }
                            }
                            if !roms.is_empty() && is_hash_identifiable(system) {
                                BackgroundManager::spawn_identity_jobs(
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
                        Err(e) if BackgroundManager::is_storage_changed(&e) => {
                            tracing::info!("ROM watcher: storage changed, cancelling rescan");
                            break;
                        }
                        Err(e) => tracing::warn!(
                            "ROM watcher: scan failed for {system}, preserving cached state: {e}"
                        ),
                    }
                }

                if !to_scan.is_empty() {
                    state.cache.invalidate_l1().await;
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

            // Internal marker directories trigger targeted cache invalidation
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // ── generate_update_script ──────────────────────────────────────

    #[test]
    fn script_includes_catalog_handling_when_path_some() {
        let script = BackgroundManager::generate_update_script(
            Path::new("/tmp/extracted/replay-control-app"),
            Path::new("/tmp/extracted/site"),
            Some(Path::new("/tmp/extracted/catalog.sqlite")),
            "0.5.0",
        );
        assert!(script.contains(r#"CATALOG_SRC="/tmp/extracted/catalog.sqlite""#));
        assert!(script.contains(r#"CATALOG_DST="/usr/local/bin/catalog.sqlite""#));
        assert!(script.contains(r#"backup "$CATALOG_DST""#));
        assert!(script.contains(r#"swap "$CATALOG_SRC" "$CATALOG_DST""#));
        assert!(script.contains(r#"restore "$CATALOG_DST""#));
        assert!(script.contains("chmod 644"));
    }

    #[test]
    fn script_omits_catalog_swap_when_path_none() {
        let script = BackgroundManager::generate_update_script(
            Path::new("/tmp/extracted/replay-control-app"),
            Path::new("/tmp/extracted/site"),
            None,
            "0.5.0",
        );
        // Empty CATALOG_SRC makes swap return non-zero; backup/restore are
        // still emitted but become no-ops on a non-existent backup file.
        assert!(script.contains(r#"CATALOG_SRC="""#));
        // Helper functions are always declared (they're cheap).
        assert!(script.contains(r#"swap()"#));
    }

    #[test]
    fn script_validates_version() {
        let script = BackgroundManager::generate_update_script(
            Path::new("/tmp/binary"),
            Path::new("/tmp/site"),
            None,
            "0.5.0",
        );
        assert!(script.contains(r#"validate_version "0.5.0""#));
        assert!(script.contains("Invalid version string"));
    }

    // ── extract_tarball ─────────────────────────────────────────────

    fn build_tarball(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let gz = flate2::write::GzEncoder::new(&mut buf, flate2::Compression::fast());
            let mut tar = tar::Builder::new(gz);
            for (path, content) in files {
                let mut header = tar::Header::new_gnu();
                header.set_size(content.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                tar.append_data(&mut header, path, *content).unwrap();
            }
            tar.into_inner().unwrap().finish().unwrap();
        }
        buf
    }

    #[tokio::test]
    async fn extract_tarball_writes_expected_files() {
        let dir = tempfile::tempdir().unwrap();
        let archive_path = dir.path().join("test.tar.gz");
        std::fs::write(
            &archive_path,
            build_tarball(&[("hello.txt", b"hello"), ("nested/world.txt", b"world")]),
        )
        .unwrap();

        let dest = dir.path().join("out");
        std::fs::create_dir_all(&dest).unwrap();
        BackgroundManager::extract_tarball(&archive_path, &dest)
            .await
            .unwrap();

        assert_eq!(
            std::fs::read_to_string(dest.join("hello.txt")).unwrap(),
            "hello"
        );
        assert_eq!(
            std::fs::read_to_string(dest.join("nested/world.txt")).unwrap(),
            "world"
        );
    }
}
