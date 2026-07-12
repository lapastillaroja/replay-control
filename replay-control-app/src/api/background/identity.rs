//! Background ROM identity matching: hashing scanned ROMs (CRC / header rc_hash /
//! disc boot-file) and resolving RetroAchievements ids, run in a bounded worker
//! pool after scan/rebuild. Free functions over `AppState`, part of `background`.

use replay_control_core_server::game_entry_builder::HashIdentificationMethod;
use replay_control_core_server::library_db::{IdentityState, LibraryDb};
use replay_control_core_server::roms::RomEntry;
use replay_control_core_server::storage::StorageLocation;
use replay_control_core_server::{game_entry_builder, rom_hash};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::api::AppState;
use crate::api::activity::{Activity, IdentityPhase, IdentityProgress};
use crate::api::db_pools::LIBRARY_MAINTENANCE_WRITE_TIMEOUT;

use super::{IdentityJob, IdentityJobOutcome, is_hash_identifiable, update_identity_progress};

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
                let end = (force_offset + super::IDENTITY_BATCH_SIZE).min(force_candidates.len());
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
            LibraryDb::claim_identity_batch(
                conn,
                &system_for_claim,
                false,
                super::IDENTITY_BATCH_SIZE,
            )
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
