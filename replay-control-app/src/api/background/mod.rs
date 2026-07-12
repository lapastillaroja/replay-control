use replay_control_core::systems::system_thumbnail_repos;
use replay_control_core_server::db_pool::rusqlite;
use replay_control_core_server::game_entry_builder::HashIdentificationMethod;
use replay_control_core_server::library_db::{IdentityState, LibraryDb};
use replay_control_core_server::roms::{RomEntry, StorageProbe};
use replay_control_core_server::storage::StorageLocation;
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

/// A system whose ROMs get runtime hashing + RA-id resolution in the identity
/// phase: cart systems (CRC + header rc_hash) or disc systems (boot-file
/// rc_hash). Both go through the same identity-job machinery; only the inner
/// hash dispatch differs.
mod tasks;
pub use tasks::{
    restart_rom_watcher, spawn_boot_tasks, spawn_library_enrichment, spawn_populate,
    spawn_rom_watcher, spawn_storage_watcher,
};

pub(crate) fn is_hash_identifiable(system: &str) -> bool {
    game_entry_builder::hash_identification_method(system) != HashIdentificationMethod::None
}

fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
}

pub(crate) fn env_duration_secs(name: &str, default_secs: u64, min_secs: u64) -> Duration {
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
pub(crate) struct IdentityJob {
    pub(crate) system: String,
    pub(crate) roms: Arc<Vec<RomEntry>>,
    pub(crate) scan_inputs: ScanInputs,
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
pub(crate) fn update_rebuild_progress(state: &AppState, f: impl FnOnce(&mut RebuildProgress)) {
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
pub(crate) async fn try_read_or_skip<T, F>(
    pool: &LibraryReadPool,
    phase: &'static str,
    f: F,
) -> Option<T>
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

/// Whether the first-run metadata seed should run before the initial
/// library scan (disabled via `REPLAY_CONTROL_SKIP_FIRST_RUN_SEED`, e.g.
/// in fast e2e containers). A startup-pipeline concern, not update-related.
fn first_run_seed_enabled() -> bool {
    !env_flag("REPLAY_CONTROL_SKIP_FIRST_RUN_SEED")
}

/// Run the ordered startup pipeline (async). Returns `true` once the boot
/// library scan has completed (the caller marks the populate done), or
/// `false` if it bailed early because the activity slot couldn't be claimed.
pub(crate) async fn run_pipeline(state: &AppState) -> bool {
    // Brief delay to let the server start accepting requests.
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Phase 0: confirm storage dirents are stable enough before any
    // scan writes stored library rows. On NFS / autofs / USB hot-plug the storage root
    // may resolve before subdirectories surface; without this, the
    // first strict scan can legitimately observe an empty filesystem
    // and reconcile stored rows to zero.
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
    if first_run_seed_enabled() {
        if let Some(_guard) = claim_startup_activity(state, StartupPhase::FetchingMetadata).await {
            phase_first_run_seed(state).await;
        }
    } else {
        tracing::debug!("phase_first_run_seed: disabled by environment");
    }

    // Phase 1: Auto-import (if launchbox XML exists + DB empty).
    // Import claims/releases its own Activity::Import via try_start_activity.
    phase_auto_import(state).await;

    // Phase 1.5: Reconcile `title_norm_version` stamps on both DBs.
    // Bumping `replay_control_core::title_utils::TITLE_NORM_VERSION` in
    // a release silently rebuilds stored normalized columns here so
    // matching benefits without user action. No-op when stamps match
    // (steady-state) or the downloaded XML is missing (fresh install
    // pre-LB-import — auto_import will write the stamp itself).
    phase_title_norm_reconcile(state).await;

    // Phase 2+3: Claim Activity::Startup for populate + thumbnail rebuild.
    // A storage swap can cancel an in-flight rebuild while this pipeline is
    // being scheduled; retry briefly so the rebuild guard can drop instead
    // of losing the new storage verification pass.
    {
        let Some(_guard) = claim_startup_activity(state, StartupPhase::Scanning).await else {
            return false;
        };

        phase_library_verification(state).await;
        phase_enrichment_inputs_reconcile(state).await;
        phase_reresolve_rc_hash_ra_ids(state).await;

        phase_auto_rebuild_thumbnail_index(state).await;
        state
            .library
            .resume_pending_thumbnail_downloads(state)
            .await;

        // _guard drops → Idle
    }

    // Reached the end: scan + enrich + thumbnail-index rebuild done. The
    // caller marks the boot populate complete.
    true
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

pub(crate) async fn scan_inputs_for_system(
    state: &AppState,
    system: &str,
    options: ScanOptions,
    generation: u64,
) -> Result<ScanInputs, replay_control_core::error::Error> {
    state.ensure_storage_generation(generation)?;

    let stored_hashes = if is_hash_identifiable(system) {
        let system_owned = system.to_string();
        match state
            .library_reader
            .read(move |conn| LibraryDb::load_stored_hashes(conn, &system_owned))
            .await
        {
            Some(Ok(hashes)) => hashes,
            Some(Err(e)) => {
                tracing::warn!(
                    "Could not load stored hashes for {system}: {e}; CRCs will be recomputed"
                );
                std::collections::HashMap::new()
            }
            None => {
                tracing::warn!(
                    "Could not load stored hashes for {system}: library DB unavailable; CRCs will be recomputed"
                );
                std::collections::HashMap::new()
            }
        }
    } else {
        std::collections::HashMap::new()
    };

    let (clean_startup_fingerprint, mtime_probe_trustworthy) = if options.skip_unchanged_startup {
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
                    trustworthy && stored_signature.as_deref() == Some(probe_signature.as_str()),
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
        stored_hashes,
        clean_startup_fingerprint,
        mtime_probe_trustworthy,
        options,
        Some(ScanCancellation::new(
            state.storage_generation.clone(),
            generation,
        )),
    ))
}

pub(crate) fn is_storage_changed(e: &replay_control_core::error::Error) -> bool {
    matches!(e, replay_control_core::error::Error::StorageChanged)
}

fn identity_worker_count() -> usize {
    std::env::var("REPLAY_CONTROL_IDENTITY_WORKERS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| (1..=4).contains(v))
        .unwrap_or(2)
}

pub(crate) fn spawn_identity_jobs(
    state: AppState,
    storage: StorageLocation,
    jobs: Vec<IdentityJob>,
    generation: u64,
) {
    if jobs.is_empty() {
        return;
    }
    let worker_count = identity_worker_count();
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
                run_identity_job(&state, &storage, job, generation).await
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
    if !wait_for_identity_window(state, generation, &job.system).await {
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
    let method = game_entry_builder::hash_identification_method(&job.system);
    let force_candidates: Vec<String> = job
        .roms
        .iter()
        .filter(|rom| match method {
            HashIdentificationMethod::Disc => true,
            HashIdentificationMethod::Cart => {
                !rom.is_m3u && rom_hash::is_file_hash_eligible(&job.system, &rom.game.rom_filename)
            }
            HashIdentificationMethod::None => false,
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
        if !wait_for_identity_window(state, generation, &job.system).await {
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
                claim_identity_filenames(state, &job.system, filenames).await
            }
        } else {
            claim_identity_batch(state, &job.system).await
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
            let finished =
                finish_identity_batch(state, &job.system, claimed, IdentityState::Pending).await;
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
            .library
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
                    let _ =
                        finish_identity_batch(state, &job.system, claimed, IdentityState::Pending)
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
                    let _ =
                        finish_identity_batch(state, &job.system, claimed, IdentityState::Pending)
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
        let unresolved = finish_identity_batch(state, &job.system, claimed, unresolved_state)
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
                let _ = post_identity_enrich(state, &job).await;
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
        && let Err(e) = post_identity_enrich(state, &job).await
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
            LibraryDb::finish_identity_batch(conn, &system_for_finish, &filenames, identity_state)
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
        .library
        .enrich_system_library_with_cancellation(
            state,
            job.system.clone(),
            job.scan_inputs.cancellation(),
        )
        .await?;
    state.library.invalidate_in_memory_views().await;
    state.invalidate_user_caches().await;
    Ok(())
}

/// Spawn a background task that re-runs `phase_auto_import`. Used by the
/// "Regenerate metadata" UI button and other on-demand triggers.
pub fn spawn_external_metadata_refresh(state: AppState) {
    tokio::spawn(async move {
        phase_auto_import(&state).await;
    });
}

/// Spawn a background task that downloads the LaunchBox `Metadata.zip`
/// into the host-global download directory, extracts the XML, then triggers
/// the standard refresh path against it.
///
/// Uses an HTTP ETag check to skip the 100+ MB download when the upstream
/// file hasn't changed since the last successful download.
pub fn spawn_external_metadata_download_and_refresh(state: AppState) {
    tokio::spawn(async move {
        let _ = download_external_metadata_and_refresh(&state).await;
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
        if needs_launchbox && !download_external_metadata_and_refresh(&state).await {
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
    let download_dir = state.data_dir.download_dir();

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
        reenrich_all_systems(state).await;
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
                &download_dir,
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
            phase_auto_import_inner(state, Some(guard)).await;
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

    if let Err(e) =
        title_norm_reconcile::reconcile_library_normalized_titles(state.library_writer.as_db_pool())
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
    let download_dir = state.data_dir.download_dir();
    drop(storage);

    let seed_check = state
        .external_metadata_reader
        .read(|conn| {
            let has_crc32 =
                external_metadata::read_meta(conn, meta_keys::LAUNCHBOX_XML_CRC32).is_some();
            let has_sources = external_metadata::get_data_source_stats(conn, "libretro-thumbnails")
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

    let xml_on_disk = resolve_launchbox_xml(&download_dir, &rc_dir).is_some();
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
        let dest = download_dir.clone();
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
        let api_key = replay_control_core_server::settings::read_github_api_key(&state.settings);
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
    phase_auto_import_inner(state, None).await;
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
    let download_dir = state.data_dir.download_dir();

    let Some(xml_path) = resolve_launchbox_xml(&download_dir, &rc_dir) else {
        tracing::debug!(
            "phase_auto_import: no LaunchBox XML in {} or {} — skipping",
            download_dir.display(),
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

    let current_platform_hash = replay_control_core::systems::launchbox_platform_map_fingerprint();

    // Re-parse on any input change:
    //   - XML content (hash) — upstream LaunchBox data changed
    //   - normalizer version — `title_utils::normalize_title_for_metadata`
    //     output changed, so stored `provider_alternate.normalized_alternate`
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
                conn, prepared,
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
    reenrich_all_systems(state).await;

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
        state.library.enrich_system_library(state, system).await;
    }
    state.invalidate_user_caches().await;
}

/// Phase 2: strict startup reconciliation across every visible system.
///
/// Works directly with the DB and filesystem — does NOT use UI summary
/// views or stored ROM readers to avoid circular dependencies.
async fn phase_library_verification(state: &AppState) {
    let storage = state.storage();
    let generation = state.storage_generation();
    let region_pref = state.region_preference();
    let region_secondary = state.region_preference_secondary();

    match populate_all_systems(
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
        Err(e) if is_storage_changed(&e) => {
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
        super::library_systems::active_systems(&state.library_reader, "reresolve_rc_hash").await;
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
        let Some(ra_map) = game_db::lookup_ra_id_by_rc_hash_batch(&system, &hashes).await else {
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
        .read(|conn| library_meta::read_meta(conn, library_meta::keys::ENRICHMENT_INPUTS_VERSION))
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
    // rather than re-enrich: `force_rehash: false` reuses stored CRC32s
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
    match populate_all_systems(
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
        Err(e) if is_storage_changed(&e) => {
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
            let index_count: i64 = external_metadata::thumbnail_manifest_count(conn).unwrap_or(0);
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
        let has_images_on_disk =
            replay_control_core_server::thumbnails::any_images_on_disk(&state.storage().rc_dir());
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
                let branch =
                    replay_control_core_server::thumbnail_manifest::default_branch(repo_display);
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
                        replay_control_core_server::thumbnails::libretro_source_name(extra_repo);
                    let extra_branch =
                        replay_control_core_server::thumbnail_manifest::default_branch(extra_repo);
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
        tracing::warn!("Thumbnail index disk rebuild write skipped: external_metadata unavailable");
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

/// Pre-populate durable library rows for all systems that have games. Walks ROM
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
    // storage, returns Err on NFS preserving stored state). The per-
    // system rule (in `scan_and_reconcile_system`) ensures empty walks
    // don't poison meta on partial-mount fresh boots.
    let systems: Vec<&'static replay_control_core::systems::System> =
        replay_control_core::systems::visible_systems().collect();
    let total = systems.len();

    tracing::info!("library populate: {total} visible system(s)");

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
            scan_inputs_for_system(state, sys.folder_name, options, generation).await?;
        let scan_started = Instant::now();
        let scan_result = state
            .library
            .scan_and_reconcile_system_with_inputs(
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
                        "library populate: {} — {} ROMs (scan+enrich)",
                        sys.folder_name,
                        roms.len()
                    );
                    total_roms += roms.len();
                }
                if !outcome.discovery_changed {
                    tracing::debug!(
                        "library populate: {} unchanged; skipping enrichment and identity",
                        sys.folder_name
                    );
                    tracing::info!(
                        "library system profile: {}: roms={} scan_ms={scan_ms} enrich_ms=0 total_ms={}",
                        sys.folder_name,
                        roms.len(),
                        system_started.elapsed().as_millis()
                    );
                    continue;
                }
                if !roms.is_empty() && is_hash_identifiable(sys.folder_name) {
                    identity_jobs.push(IdentityJob {
                        system: sys.folder_name.to_string(),
                        roms: roms.clone(),
                        scan_inputs: scan_inputs.clone(),
                    });
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
                // preserve-stored-state policy and move on. Scan-derived
                // fields are already persisted by the scan above, and
                // hash identity work has already been queued. Storage swaps
                // still abort (the caller redoes the pass on the new storage).
                if let Err(e) = state
                    .library
                    .enrich_system_library_with_cancellation(
                        state,
                        sys.folder_name.to_string(),
                        scan_inputs.cancellation(),
                    )
                    .await
                {
                    if is_storage_changed(&e) {
                        return Err(e);
                    }
                    tracing::warn!(
                        "library populate: {} enrichment skipped (preserving stored state): {e}",
                        sys.folder_name
                    );
                    continue;
                }
                let enrich_ms = enrich_started.elapsed().as_millis();
                tracing::info!(
                    "library system profile: {}: roms={} scan_ms={scan_ms} enrich_ms={enrich_ms} total_ms={}",
                    sys.folder_name,
                    roms.len(),
                    system_started.elapsed().as_millis()
                );
            }
            Err(e) if is_storage_changed(&e) => return Err(e),
            Err(e) => {
                tracing::warn!(
                    "library populate: {} skipped after {scan_ms}ms (preserving stored state): {e}",
                    sys.folder_name
                );
            }
        }
    }

    tracing::info!(
        "library populate: done — {} ROMs across {} systems in {:.1}s",
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
    refresh_thumbnail_media_stats(state, storage, generation, "library populate").await?;
    spawn_identity_jobs(state.clone(), storage.clone(), identity_jobs, generation);
    Ok(())
}
