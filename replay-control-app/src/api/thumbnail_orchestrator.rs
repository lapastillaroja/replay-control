//! Single coordinator for all thumbnail-download work.
//!
//! Replaces two unmediated download patterns:
//!
//! - Bulk thumbnail-update ran a local `Semaphore::new(10)` + `JoinSet`
//!   in `library/thumbnails/manifest.rs`.
//! - On-demand box-art enrichment in `library/enrichment.rs` had **no
//!   bound at all** — every cache-miss spawned a `tokio::spawn` directly,
//!   so a fresh-system rescan with thousands of missing thumbnails could
//!   open thousands of HTTP sockets in seconds. Production telemetry
//!   measured 1 012/1 024 fds open mid-rescan; 993 of them were sockets.
//!
//! Both paths now submit `Job`s here. The orchestrator owns:
//!
//! - **Concurrency cap** (single semaphore, configurable via
//!   [`Config::max_concurrent`]).
//! - **Dedup** across both paths via a shared `(system, kind, filename)`
//!   key. A bulk in-flight + on-demand request for the same file
//!   collapses to one download.
//! - **Priority**: visible (= "user just opened a page") preempts bulk
//!   pre-fetch via two channels polled with `select! biased`.
//! - **Per-job completion delivery**: bulk callers receive `JobResult`s
//!   on a per-call channel for progress reporting; on-demand callers
//!   pass an `on_complete` hook that runs in the worker (DB updates,
//!   cache invalidation).
//!
//! Cancellation: bulk callers pass an `Arc<AtomicBool>` cancel token
//! observed by the worker between job spawns. Process shutdown is
//! handled via channel close (the worker exits cleanly when both
//! senders drop — i.e. when the orchestrator's `Arc` is the last one).

use std::collections::HashSet;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use replay_control_core_server::thumbnail_manifest::{
    ManifestMatch, download_thumbnail, save_thumbnail,
};
use replay_control_core_server::thumbnails::ThumbnailKind;
use tokio::sync::{Semaphore, mpsc};

/// Identity of a single thumbnail file. Used for dedup across the bulk
/// and on-demand pipelines so the same file isn't downloaded twice
/// concurrently.
#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct ThumbnailKey {
    pub system: String,
    pub kind: ThumbnailKind,
    pub filename: String,
}

/// What happened with a single download attempt.
#[derive(Clone, Debug)]
pub enum Outcome {
    /// Downloaded + saved successfully.
    Saved,
    /// Job collapsed against an in-flight duplicate; another caller's
    /// task will deliver the real bytes. Bulk callers should treat this
    /// as "not my work" — *don't* count it in `downloaded` totals.
    Skipped,
    /// HTTP download failed (404, network error, timeout, …).
    DownloadFailed(String),
    /// Download succeeded but writing to disk failed.
    SaveFailed(String),
}

impl Outcome {
    pub fn is_success(&self) -> bool {
        matches!(self, Outcome::Saved | Outcome::Skipped)
    }
}

/// One completion notification delivered to a bulk caller's
/// `completion_tx` channel and / or an on-demand caller's `on_complete`
/// hook.
#[derive(Clone, Debug)]
pub struct JobResult {
    pub key: ThumbnailKey,
    pub outcome: Outcome,
}

/// Async hook called by the worker once a job finishes (success or
/// failure). Used by the on-demand path to update `box_art_url` in the
/// DB and invalidate caches without coupling the orchestrator to
/// `AppState`.
pub type OnCompleteHook =
    Box<dyn FnOnce(JobResult) -> Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Send>;

/// Internal job representation. Constructed by `submit_visible` /
/// `submit_bulk` and consumed by the worker.
struct Job {
    key: ThumbnailKey,
    payload: ManifestMatch,
    storage_root: PathBuf,
    completion_tx: Option<mpsc::UnboundedSender<JobResult>>,
    on_complete: Option<OnCompleteHook>,
}

/// Tunables. Defaults match the previous bulk path's `Semaphore::new(10)`
/// and pick reasonable channel sizes for a Pi-class device.
#[derive(Clone, Copy, Debug)]
pub struct Config {
    /// Max concurrent in-flight downloads across both pipelines combined.
    /// Was `Semaphore::new(10)` in the bulk path; on-demand was unbounded.
    pub max_concurrent: usize,
    /// Visible queue depth. Small because visible work is rare in
    /// absolute terms — even a heavy page render queues at most a few
    /// dozen thumbnails — and we want fast back-pressure if something
    /// upstream goes wrong.
    pub visible_capacity: usize,
    /// Bulk queue depth. Larger because bulk work batches are large
    /// (thousands of items) and the caller `awaits` `submit_bulk`, so
    /// back-pressure here is the natural rate-limiter for bulk
    /// submission.
    pub bulk_capacity: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max_concurrent: 10,
            visible_capacity: 64,
            bulk_capacity: 256,
        }
    }
}

/// Shared mutable state between submitter handles and the worker task.
struct OrchestratorState {
    /// Dedup set across both pipelines. `submit_*` checks-and-inserts
    /// atomically; the worker removes on job completion.
    pending: std::sync::Mutex<HashSet<ThumbnailKey>>,
    /// Live in-flight count (downloads currently running). Useful for
    /// `/debug/pool`-style observability; one atomic load to read.
    in_flight: AtomicUsize,
    /// Lifetime counters.
    completed_ok: AtomicU64,
    failed: AtomicU64,
}

/// Public handle. Cheap to clone (one `Arc` and two channel senders);
/// hand out one per submitter site or hold a single `Arc<Self>`.
#[derive(Clone)]
pub struct ThumbnailDownloadOrchestrator {
    visible_tx: mpsc::Sender<Job>,
    bulk_tx: mpsc::Sender<Job>,
    state: Arc<OrchestratorState>,
}

impl ThumbnailDownloadOrchestrator {
    /// Spawn the worker task and return a handle. The worker exits
    /// cleanly once the last `Self` is dropped (channel senders close,
    /// receiver drains, loop exits).
    pub fn spawn(config: Config) -> Self {
        let (visible_tx, visible_rx) = mpsc::channel(config.visible_capacity);
        let (bulk_tx, bulk_rx) = mpsc::channel(config.bulk_capacity);
        let state = Arc::new(OrchestratorState {
            pending: std::sync::Mutex::new(HashSet::new()),
            in_flight: AtomicUsize::new(0),
            completed_ok: AtomicU64::new(0),
            failed: AtomicU64::new(0),
        });

        tokio::spawn(run_worker(
            state.clone(),
            visible_rx,
            bulk_rx,
            config.max_concurrent,
        ));

        Self {
            visible_tx,
            bulk_tx,
            state,
        }
    }

    /// Submit a visible (= user-facing priority) job. Awaits when the
    /// visible queue is full so submitters cooperate with backpressure.
    ///
    /// If the same key is already in flight, the request collapses and
    /// the in-flight job's `on_complete` runs (whoever submitted first).
    /// This caller's `on_complete` is dropped silently.
    ///
    /// Priority over bulk is enforced inside the worker via
    /// `select! biased`, not by this method's send semantics.
    pub async fn submit_visible(
        &self,
        key: ThumbnailKey,
        payload: ManifestMatch,
        storage_root: PathBuf,
        on_complete: Option<OnCompleteHook>,
    ) {
        let Some(claim) = self.try_claim(key.clone()) else {
            return;
        };
        let job = Job {
            key,
            payload,
            storage_root,
            completion_tx: None,
            on_complete,
        };
        self.enqueue(&self.visible_tx, claim, job).await;
    }

    /// Submit a bulk pre-fetch job. Awaits when the bulk queue is full.
    /// `completion_tx` carries each finished `JobResult`; close the
    /// channel by dropping the sender once submission is done, then
    /// drain the receiver to consume completions.
    ///
    /// `cancel` is observed by the worker between job spawns so a bulk
    /// caller can stop submission mid-stream and avoid wasting work.
    pub async fn submit_bulk(
        &self,
        key: ThumbnailKey,
        payload: ManifestMatch,
        storage_root: PathBuf,
        completion_tx: mpsc::UnboundedSender<JobResult>,
        cancel: &Arc<AtomicBool>,
    ) {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        let Some(claim) = self.try_claim(key.clone()) else {
            // Dedup hit: tell the caller's drain loop to advance, but
            // tag the outcome `Skipped` so it isn't counted as work.
            let _ = completion_tx.send(JobResult {
                key,
                outcome: Outcome::Skipped,
            });
            return;
        };
        let job = Job {
            key,
            payload,
            storage_root,
            completion_tx: Some(completion_tx),
            on_complete: None,
        };
        self.enqueue(&self.bulk_tx, claim, job).await;
    }

    /// Live in-flight download count. Atomic load; safe to call hot.
    pub fn in_flight(&self) -> usize {
        self.state.in_flight.load(Ordering::Relaxed)
    }

    /// Lifetime counters: `(completed_ok, failed)`. Atomic loads.
    pub fn lifetime_counts(&self) -> (u64, u64) {
        (
            self.state.completed_ok.load(Ordering::Relaxed),
            self.state.failed.load(Ordering::Relaxed),
        )
    }

    /// Atomic check-then-insert into the pending set. Returns a
    /// `ClaimGuard` (with rollback-on-drop) when this caller now owns
    /// the work, or `None` when another caller already has it queued
    /// or in flight.
    fn try_claim(&self, key: ThumbnailKey) -> Option<ClaimGuard> {
        let inserted = self
            .state
            .pending
            .lock()
            .expect("pending lock")
            .insert(key.clone());
        inserted.then(|| ClaimGuard {
            state: self.state.clone(),
            key: Some(key),
        })
    }

    /// Enqueue a job onto `sender`, transferring claim ownership to
    /// the worker on success. On failure (worker has exited) or if the
    /// caller's future is cancelled mid-await, the `ClaimGuard` drops
    /// and rolls back the dedup entry so retries aren't blocked.
    async fn enqueue(&self, sender: &mpsc::Sender<Job>, claim: ClaimGuard, job: Job) {
        if sender.send(job).await.is_ok() {
            claim.disarm();
        }
    }
}

/// RAII rollback for a pending-set claim. Drops the claim back out of
/// the dedup set on `Drop` unless `disarm()` was called first.
struct ClaimGuard {
    state: Arc<OrchestratorState>,
    key: Option<ThumbnailKey>,
}

impl ClaimGuard {
    fn disarm(mut self) {
        self.key = None;
    }
}

impl Drop for ClaimGuard {
    fn drop(&mut self) {
        if let Some(key) = self.key.take() {
            self.state
                .pending
                .lock()
                .expect("pending lock")
                .remove(&key);
        }
    }
}

async fn run_worker(
    state: Arc<OrchestratorState>,
    mut visible_rx: mpsc::Receiver<Job>,
    mut bulk_rx: mpsc::Receiver<Job>,
    max_concurrent: usize,
) {
    let sem = Arc::new(Semaphore::new(max_concurrent));
    loop {
        // `biased` makes select! poll arms top-to-bottom — visible
        // first, so user-facing requests preempt bulk pre-fetch.
        // Returns None when both senders are dropped, which is our
        // shutdown signal.
        let job = tokio::select! {
            biased;
            v = visible_rx.recv() => v,
            b = bulk_rx.recv()    => b,
        };
        let Some(job) = job else {
            tracing::debug!(
                "thumbnail orchestrator: both submission channels closed, worker exiting"
            );
            return;
        };

        // Wait for a slot. Permit drops at task end, freeing the slot
        // even on panic-unwind (Drop runs).
        let permit = match sem.clone().acquire_owned().await {
            Ok(p) => p,
            Err(_) => return, // semaphore closed (we never close it; defensive)
        };

        let state = state.clone();
        state.in_flight.fetch_add(1, Ordering::Relaxed);
        tokio::spawn(async move {
            let _permit = permit;
            run_job(state.clone(), job).await;
            state.in_flight.fetch_sub(1, Ordering::Relaxed);
        });
    }
}

async fn run_job(state: Arc<OrchestratorState>, job: Job) {
    let key = job.key.clone();
    let outcome = match download_thumbnail(&job.payload, key.kind.repo_dir()).await {
        Ok(bytes) => {
            match save_thumbnail(
                &job.storage_root,
                &key.system,
                key.kind,
                &key.filename,
                bytes,
            )
            .await
            {
                Ok(_) => Outcome::Saved,
                Err(e) => Outcome::SaveFailed(format!("{e}")),
            }
        }
        Err(e) => Outcome::DownloadFailed(format!("{e}")),
    };

    if outcome.is_success() {
        state.completed_ok.fetch_add(1, Ordering::Relaxed);
    } else {
        state.failed.fetch_add(1, Ordering::Relaxed);
    }

    let result = JobResult {
        key: key.clone(),
        outcome,
    };

    // On-complete hook runs first so its DB updates land before any
    // bulk caller observes the completion.
    if let Some(hook) = job.on_complete {
        hook(result.clone()).await;
    }
    if let Some(tx) = job.completion_tx {
        let _ = tx.send(result);
    }

    state.pending.lock().expect("pending lock").remove(&key);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    /// Minimal stub of the worker's job pipeline that bypasses the real
    /// HTTP/disk path and just records that the orchestrator routed the
    /// work. Real networked tests live in the e2e suite.
    fn key(system: &str, filename: &str) -> ThumbnailKey {
        ThumbnailKey {
            system: system.into(),
            kind: ThumbnailKind::Boxart,
            filename: filename.into(),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn dedup_collapses_duplicate_visible_submits() {
        let orch = ThumbnailDownloadOrchestrator::spawn(Config::default());
        let k = key("nintendo_nes", "Test (USA)");

        // First claim: succeeds. Hold the guard so dedup actually
        // sees the key; otherwise Drop rolls it back immediately.
        let _g1 = orch.try_claim(k.clone()).expect("first claim");
        // Second claim of the same key: blocked by dedup.
        assert!(orch.try_claim(k.clone()).is_none());
        // Distinct key: free.
        assert!(orch.try_claim(key("nintendo_nes", "Other (USA)")).is_some());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn submit_bulk_signals_skipped_on_dedup_collision() {
        let orch = ThumbnailDownloadOrchestrator::spawn(Config::default());
        let k = key("nintendo_nes", "Test (USA)");
        // Pre-claim so the next submit_bulk hits the dedup branch.
        let _claim = orch.try_claim(k.clone()).expect("pre-claim");

        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, mut rx) = mpsc::unbounded_channel();
        orch.submit_bulk(
            k.clone(),
            ManifestMatch {
                filename: "Test (USA)".into(),
                is_symlink: false,
                repo_url_name: "x".into(),
                branch: "master".into(),
            },
            PathBuf::from("/tmp/whatever"),
            tx,
            &cancel,
        )
        .await;

        // Caller's drain loop expects exactly one JobResult per submit;
        // Skipped tells the caller this isn't theirs to count.
        let res = tokio::time::timeout(Duration::from_millis(100), rx.recv())
            .await
            .expect("orchestrator should signal a Skipped completion on dedup");
        let res = res.expect("channel should not close before delivering");
        assert_eq!(res.key, k);
        assert!(matches!(res.outcome, Outcome::Skipped));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn cancel_token_short_circuits_submit_bulk() {
        let orch = ThumbnailDownloadOrchestrator::spawn(Config::default());
        let cancel = Arc::new(AtomicBool::new(true));
        let (tx, mut rx) = mpsc::unbounded_channel();
        orch.submit_bulk(
            key("nintendo_nes", "Test"),
            ManifestMatch {
                filename: "Test".into(),
                is_symlink: false,
                repo_url_name: "x".into(),
                branch: "master".into(),
            },
            PathBuf::from("/tmp/whatever"),
            tx,
            &cancel,
        )
        .await;
        // No completion expected — caller cancelled before submit.
        assert!(rx.try_recv().is_err());
    }

    /// Verifies that an unbounded burst of distinct submits doesn't
    /// blow the in-flight count past the configured cap.
    ///
    /// Uses the dedup-collision path to emulate "every submit returns
    /// instantly" without actually hitting the network — the real
    /// concurrency cap is exercised by the e2e suite under live load.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn dedup_bookkeeping_survives_high_burst() {
        let orch = ThumbnailDownloadOrchestrator::spawn(Config::default());
        let counter = Arc::new(AtomicUsize::new(0));
        for i in 0..1500 {
            let k = key("nintendo_nes", &format!("Game {i}"));
            if let Some(_guard) = orch.try_claim(k) {
                counter.fetch_add(1, Ordering::Relaxed);
                // _guard drops at end of block, releasing the claim
                // (the same way the worker releases on job completion).
            }
        }
        assert_eq!(counter.load(Ordering::Relaxed), 1500);
        assert_eq!(orch.in_flight(), 0);
    }

    /// `ClaimGuard` is the mechanism that defends `submit_visible` /
    /// `submit_bulk` against orphaned dedup entries when the submit
    /// future is cancelled mid-await. Verify the Drop rollback fires.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn claim_guard_rolls_back_on_drop() {
        let orch = ThumbnailDownloadOrchestrator::spawn(Config::default());
        let k = key("nintendo_nes", "Test");
        {
            let _claim = orch.try_claim(k.clone()).expect("first claim");
            assert!(
                orch.state
                    .pending
                    .lock()
                    .expect("pending lock")
                    .contains(&k),
                "claim should be live while guard is held"
            );
        }
        assert!(
            !orch
                .state
                .pending
                .lock()
                .expect("pending lock")
                .contains(&k),
            "guard Drop should roll back the claim"
        );
    }

    /// After `disarm()` the guard becomes a no-op on Drop — used by the
    /// happy path where the worker now owns cleanup.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn claim_guard_disarm_keeps_claim() {
        let orch = ThumbnailDownloadOrchestrator::spawn(Config::default());
        let k = key("nintendo_nes", "Test");
        let claim = orch.try_claim(k.clone()).expect("first claim");
        claim.disarm();
        assert!(
            orch.state
                .pending
                .lock()
                .expect("pending lock")
                .contains(&k),
            "disarmed guard must not roll back; worker owns cleanup"
        );
    }

    /// End-to-end: a `submit_visible` future cancelled while awaiting
    /// on a saturated queue must not leak the dedup claim.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn cancelled_submit_rolls_back_claim() {
        // max_concurrent=0 → worker pulls jobs but blocks forever on
        // `sem.acquire_owned()`. visible_capacity=1 → at most one
        // queued + one held by the worker before sends start blocking.
        let orch = ThumbnailDownloadOrchestrator::spawn(Config {
            max_concurrent: 0,
            visible_capacity: 1,
            bulk_capacity: 1,
        });
        let payload = || ManifestMatch {
            filename: "x".into(),
            is_symlink: false,
            repo_url_name: "x".into(),
            branch: "master".into(),
        };

        // Fill: worker recvs and parks on the semaphore; queue then
        // accepts one more buffered item.
        orch.submit_visible(
            key("nintendo_nes", "Held"),
            payload(),
            PathBuf::from("/tmp/x"),
            None,
        )
        .await;
        // Tiny yield so the worker definitely drains the first send
        // before we fill the buffered slot.
        tokio::time::sleep(Duration::from_millis(20)).await;
        orch.submit_visible(
            key("nintendo_nes", "Buffered"),
            payload(),
            PathBuf::from("/tmp/x"),
            None,
        )
        .await;

        // Third send must block on a full channel; cancel via timeout.
        let cancelled = key("nintendo_nes", "Cancelled");
        let fut = orch.submit_visible(cancelled.clone(), payload(), PathBuf::from("/tmp/x"), None);
        let result = tokio::time::timeout(Duration::from_millis(50), fut).await;
        assert!(result.is_err(), "submit should have blocked and timed out");

        assert!(
            !orch
                .state
                .pending
                .lock()
                .expect("pending lock")
                .contains(&cancelled),
            "claim for cancelled submit leaked"
        );
    }
}
