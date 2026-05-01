//! Async SQLite connection pool built on deadpool-sqlite.
//!
//! Two pools per DB (read N, write 1), filesystem-aware journal-mode
//! selection (WAL on POSIX, DELETE on exFAT/NFS), corruption flag, internal
//! write gate that auto-activates on DELETE-mode pools.
//!
//! Safety contract:
//! - WAL recovery runs once per `DbPool` instance before any deadpool
//!   connection exists. Per-connection opens never touch sidecar files.
//! - Destructive ops (`reset_to_empty`, `replace_with_file`, `reopen` to a
//!   new path) drain in-flight connections before mutating files; abort if
//!   drain times out so a stuck closure can't race the unlink.
//! - `try_read` / `try_write` return typed `DbError`. Cascade gates that
//!   treat unavailability as "no rows" produce silent data loss; see
//!   `investigations/2026-05-01-library-wal-unlink-under-live-connections.md`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

use deadpool::managed::{self, Metrics, RecycleError};
use deadpool_sync::SyncWrapper;
use thiserror::Error;

pub use rusqlite;

use crate::sqlite::{self, JournalMode};

/// Custom deadpool Manager that uses `sqlite::open_connection()` for
/// proper WAL/nolock/PRAGMA configuration instead of plain `Connection::open()`.
struct SqliteManager {
    db_path: PathBuf,
    label: String,
    /// Actual journal mode determined at pool creation by querying the DB.
    /// Controls WAL-specific PRAGMAs (autocheckpoint on write connections).
    journal_mode: JournalMode,
    /// Whether this manager creates write-pool connections.
    /// Write + WAL connections disable auto-checkpoint for manual control.
    /// Read connections set `query_only = ON` for safety.
    is_write: bool,
    /// Throttle `PRAGMA optimize` to at most once per hour.
    last_optimize: Arc<std::sync::Mutex<std::time::Instant>>,
}

impl std::fmt::Debug for SqliteManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteManager")
            .field("db_path", &self.db_path)
            .field("journal_mode", &self.journal_mode)
            .field("label", &self.label)
            .field("is_write", &self.is_write)
            .finish()
    }
}

impl managed::Manager for SqliteManager {
    type Type = SyncWrapper<rusqlite::Connection>;
    type Error = rusqlite::Error;

    async fn create(&self) -> Result<SyncWrapper<rusqlite::Connection>, Self::Error> {
        let db_path = self.db_path.clone();
        let is_write = self.is_write;
        let is_wal = self.journal_mode == JournalMode::Wal;
        let label = self.label.clone();

        SyncWrapper::new(deadpool_sqlite::Runtime::Tokio1, move || {
            let conn = sqlite::open_connection(&db_path, &label).map_err(|e| {
                rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(1), Some(e.to_string()))
            })?;

            // Per-role PRAGMAs (on top of the base PRAGMAs from open_connection):
            // Read connections: 1000 pages (~4 MB). Big enough that the
            // recommendations / system_coverage_stats / metadata snapshot
            // queries can hold their working set in cache without paging
            // hot indexes back from disk between calls.
            // Write connection: 500 pages (~2 MB) — its working set is
            // dominated by per-batch dirty pages and rolled into the WAL.
            // 1×4 + 1×4 + 1×2 = 10 MB per DbPool, ~20 MB across both
            // mutable DBs. Plenty of headroom on Pi 4 / Pi 5.
            // See Tier 5 of `2026-04-29-pool-design-findings.md`.
            if is_write {
                conn.execute_batch("PRAGMA cache_size = 500;")?;
            } else {
                conn.execute_batch("PRAGMA cache_size = 1000;")?;
            }
            if is_write && is_wal {
                // Disable automatic WAL checkpoints so we can checkpoint
                // manually after heavy writes (import, thumbnail rebuild).
                conn.execute_batch("PRAGMA wal_autocheckpoint = 0;")?;
            }
            if !is_write {
                // Read connections should never modify data (defense-in-depth).
                conn.execute_batch("PRAGMA query_only = ON;")?;
            }

            Ok(conn)
        })
        .await
    }

    async fn recycle(
        &self,
        conn: &mut SyncWrapper<rusqlite::Connection>,
        _metrics: &Metrics,
    ) -> managed::RecycleResult<Self::Error> {
        // Skip the SELECT health check (Matrix SDK found this 3.5x faster).
        // If the connection is broken, interact() will fail and the pool
        // will discard it automatically.
        if conn.is_mutex_poisoned() {
            return Err(RecycleError::message("mutex poisoned"));
        }

        // Run PRAGMA optimize at most once per hour to keep query planner
        // statistics fresh without adding overhead to every pool return.
        let should_optimize = self
            .last_optimize
            .lock()
            .ok()
            .is_some_and(|last| last.elapsed() > std::time::Duration::from_secs(3600));

        if should_optimize {
            let result = conn
                .interact(|conn| {
                    conn.execute_batch("PRAGMA analysis_limit = 400; PRAGMA optimize;")
                })
                .await;
            match result {
                Ok(Ok(())) => {
                    if let Ok(mut last) = self.last_optimize.lock() {
                        *last = std::time::Instant::now();
                    }
                }
                Ok(Err(e)) => {
                    tracing::debug!("PRAGMA optimize failed: {e}");
                }
                Err(e) => {
                    tracing::debug!("PRAGMA optimize interact failed: {e}");
                }
            }
        }

        Ok(())
    }
}

/// Alias for a deadpool pool using our custom manager.
type SqlitePool = managed::Pool<SqliteManager>;

/// Read vs. write — drives gate handling and metric routing in
/// [`DbPool::dispatch`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Read,
    Write,
}

impl Op {
    fn kind(self) -> &'static str {
        match self {
            Op::Read => "read",
            Op::Write => "write",
        }
    }
}

/// Why a `try_read`/`try_write` could not complete. `Closed`/`Corrupt`/`Busy`
/// are pool-state signals (treat as "skip", never as "no rows"); `Sql`,
/// `Acquire`, `Interact` carry the underlying typed error so callers can
/// match (e.g. on `rusqlite::ErrorCode::DatabaseBusy`).
#[derive(Debug, Error)]
pub enum DbError {
    #[error("DB pool is closed")]
    Closed,
    #[error("DB pool is corrupt — awaiting recovery")]
    Corrupt,
    #[error("DB is busy (write in flight)")]
    Busy,
    #[error("DB operation timed out after {0:?}")]
    Timeout(std::time::Duration),
    #[error("connection acquire failed: {0}")]
    Acquire(#[from] deadpool::managed::PoolError<rusqlite::Error>),
    #[error("interact dispatch failed: {0}")]
    Interact(#[from] deadpool_sync::InteractError),
    #[error("SQL: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("{0}")]
    Other(String),
}

/// Connection pool for a single SQLite database.
///
/// Uses `deadpool` for true concurrent reads (WAL mode allows multiple readers)
/// with separate read and write pools.
///
/// - **Read pool**: `max_size=3` (both WAL and DELETE modes support concurrent readers)
/// - **Write pool**: `max_size=1` (SQLite serialises writes)
///
/// Provides async `read()` / `write()` helpers that use deadpool's native async
/// API: `pool.get().await` for connection acquisition and `conn.interact()` for
/// running closures on a blocking thread. This ensures waiting for a connection
/// never pins a tokio worker thread.
///
/// The pools are wrapped in `Arc<RwLock<>>` so that `close()` / `reopen()` can
/// swap them across all clones of the same `DbPool`.
type CorruptionCallback = Arc<std::sync::RwLock<Option<Box<dyn Fn() + Send + Sync>>>>;

#[derive(Clone)]
pub struct DbPool {
    /// Read pool. WAL allows true concurrent reads, so multi-slot pools let
    /// SSR fan-out and long enrichment / image-index passes overlap. Sized
    /// per-pool at construction (library: 3, user_data: 1).
    read_pool: Arc<std::sync::RwLock<Option<SqlitePool>>>,
    /// Single write connection (SQLite serialises writes).
    write_pool: Arc<std::sync::RwLock<Option<SqlitePool>>>,
    db_path: Arc<std::sync::RwLock<PathBuf>>,
    label: &'static str,
    /// Read-pool size, captured for `reopen()`.
    read_size: usize,
    /// Journal mode. Atomic so `reopen()` can swap mode → pool slots in
    /// that order; a `write()` arriving mid-reopen never sees stale mode
    /// + new pool.
    journal_mode: Arc<AtomicU8>,
    /// Opener function for creating additional connections (used by `reopen()`
    /// to verify the DB is accessible before rebuilding pools).
    opener: fn(&Path) -> replay_control_core::error::Result<rusqlite::Connection>,
    /// Set when a query returns SQLITE_CORRUPT (11) or SQLITE_NOTADB (26).
    /// Once set, the pool is closed and all reads/writes return None until
    /// the DB is rebuilt/repaired and the flag is cleared. NOTADB covers the
    /// case where the file's 16-byte magic header has been overwritten and
    /// SQLite refuses to identify it as a database at all.
    corrupt: Arc<AtomicBool>,
    /// True once recovery has run for `db_path`. Reopen to a different
    /// path or to a freshly-unlinked file resets to false.
    recovered: Arc<AtomicBool>,
    /// Auto-activated by `try_write` on DELETE-mode pools. Blocks reads
    /// for the duration of a write so they don't race the rollback
    /// journal. WAL pools never set it.
    write_gate: Arc<AtomicBool>,
    /// Fires on every actual transition of the corruption flag — both
    /// false→true (`mark_corrupt`) and true→false (`reopen` success path).
    /// Lets the host crate broadcast a status event without each call site
    /// needing to remember to do so. Idempotent flag writes do not re-fire.
    on_corruption_change: CorruptionCallback,
    /// Lifetime counters per pool. Cheap (atomic adds) and surfaced via
    /// `metrics()` for the next "is the DB pool starved?" investigation.
    /// See Tier 5 of `2026-04-29-pool-design-findings.md`.
    metrics: Arc<PoolMetrics>,
}

#[derive(Debug, Default)]
pub struct PoolMetrics {
    pub reads_started: AtomicU64,
    pub reads_completed: AtomicU64,
    pub reads_returned_none: AtomicU64,
    pub reads_timed_out: AtomicU64,
    pub writes_started: AtomicU64,
    pub writes_completed: AtomicU64,
    pub writes_timed_out: AtomicU64,
    pub gate_blocked_reads: AtomicU64,
}

#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
pub struct PoolMetricsSnapshot {
    pub reads_started: u64,
    pub reads_completed: u64,
    pub reads_returned_none: u64,
    pub reads_timed_out: u64,
    pub writes_started: u64,
    pub writes_completed: u64,
    pub writes_timed_out: u64,
    pub gate_blocked_reads: u64,
}

impl PoolMetrics {
    fn snapshot(&self) -> PoolMetricsSnapshot {
        PoolMetricsSnapshot {
            reads_started: self.reads_started.load(Ordering::Relaxed),
            reads_completed: self.reads_completed.load(Ordering::Relaxed),
            reads_returned_none: self.reads_returned_none.load(Ordering::Relaxed),
            reads_timed_out: self.reads_timed_out.load(Ordering::Relaxed),
            writes_started: self.writes_started.load(Ordering::Relaxed),
            writes_completed: self.writes_completed.load(Ordering::Relaxed),
            writes_timed_out: self.writes_timed_out.load(Ordering::Relaxed),
            gate_blocked_reads: self.gate_blocked_reads.load(Ordering::Relaxed),
        }
    }
}

/// Wall-clock cap on a single `interact()` closure. The closure runs on
/// the blocking pool via `spawn_blocking`, which is **not cancellable** when
/// the awaiting future drops — a stuck closure will hold the SyncWrapper
/// inner mutex until it completes, blocking every subsequent `interact()` on
/// the same connection. This timeout doesn't actually stop the closure (we
/// can't), but it lets the awaiting caller bail with `None` instead of being
/// dragged along, and surfaces the bug loudly via tracing::error so we know
/// to find and fix the offending site.
///
/// 15 s matches the proposal in
/// `2026-04-29-ssr-cache-snapshot-vs-pool-starvation.md`. SSR-style callers
/// should layer a tighter `tokio::time::timeout` on top if they care about
/// faster fall-back.
const INTERACT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// RAII gate. While held, reads on the pool short-circuit to
/// `Err(DbError::Busy)`. Auto-activated by `try_write` on DELETE-mode pools.
pub(crate) struct WriteGate(Arc<AtomicBool>);

impl WriteGate {
    pub(crate) fn activate(flag: &Arc<AtomicBool>) -> Self {
        flag.store(true, Ordering::Release);
        tracing::debug!("WriteGate: activated");
        Self(Arc::clone(flag))
    }
}

impl Drop for WriteGate {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
        tracing::debug!("WriteGate: released");
    }
}

/// Close `pool` and wait for in-flight `Object`s to drop. Returns `true`
/// on a clean drain, `false` on timeout — destructive callers
/// (`with_fresh_file`) abort on `false` so a stuck `interact()` closure
/// doesn't race a follow-up `delete_db_files`.
async fn drain_pool(pool: SqlitePool, kind: &'static str, label: &'static str) -> bool {
    pool.close();
    let deadline = std::time::Instant::now() + INTERACT_TIMEOUT * 2;
    while pool.status().size > 0 && std::time::Instant::now() < deadline {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let remaining = pool.status().size;
    if remaining > 0 {
        tracing::warn!(
            "{label}: {kind} pool drain timed out — {remaining} connection(s) still outstanding"
        );
        return false;
    }
    true
}

/// Build a deadpool `SqlitePool` with the given size.
fn build_pool(
    db_path: &Path,
    journal_mode: JournalMode,
    is_write: bool,
    label: &str,
    max_size: usize,
) -> Result<SqlitePool, Box<dyn std::error::Error>> {
    let mgr = SqliteManager {
        db_path: db_path.to_path_buf(),
        label: label.to_string(),
        journal_mode,
        is_write,
        last_optimize: Arc::new(std::sync::Mutex::new(std::time::Instant::now())),
    };
    let pool = managed::Pool::builder(mgr)
        .max_size(max_size)
        .wait_timeout(Some(std::time::Duration::from_secs(10)))
        .runtime(deadpool_sqlite::Runtime::Tokio1)
        .build()
        .map_err(|e| format!("{label}: failed to build pool: {e}"))?;
    Ok(pool)
}

/// Query the actual journal mode from an open connection.
fn query_journal_mode(conn: &rusqlite::Connection) -> JournalMode {
    conn.query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
        .map(|m| {
            if m == "wal" {
                JournalMode::Wal
            } else {
                JournalMode::Delete
            }
        })
        .unwrap_or(JournalMode::Delete)
}

impl DbPool {
    /// Create a new pool. Opens the DB synchronously to detect journal mode and
    /// fail fast on inaccessible files. Deadpool connections are created lazily
    /// on first use — `Manager::create` only adds trivial role PRAGMAs
    /// (`cache_size`, `query_only`, `wal_autocheckpoint`) on top of the open
    /// path we just exercised, so eagerly warming them adds no error coverage.
    /// Avoiding `block_in_place + block_on` here keeps `DbPool::new` callable
    /// from any tokio runtime flavor (including `current_thread`), which lets
    /// integration tests run without the multi-thread runtime requirement.
    pub fn new(
        db_path: PathBuf,
        label: &'static str,
        opener: fn(&Path) -> replay_control_core::error::Result<rusqlite::Connection>,
        read_size: usize,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // Recovery before any connection exists — sibling deadpool opens
        // skip it (per-connection recovery would unlink WAL under live fds).
        sqlite::recover_after_unclean_shutdown(&db_path);

        let warmup = sqlite::open_connection(&db_path, label)
            .map_err(|e| format!("{label}: failed to open warmup connection: {e}"))?;
        let journal_mode = query_journal_mode(&warmup);
        drop(warmup);

        let read_pool = build_pool(
            &db_path,
            journal_mode,
            false,
            &format!("{label}_read"),
            read_size,
        )?;
        let write_pool = build_pool(&db_path, journal_mode, true, &format!("{label}_write"), 1)?;

        Ok(Self {
            read_pool: Arc::new(std::sync::RwLock::new(Some(read_pool))),
            write_pool: Arc::new(std::sync::RwLock::new(Some(write_pool))),
            db_path: Arc::new(std::sync::RwLock::new(db_path)),
            label,
            read_size,
            journal_mode: Arc::new(AtomicU8::new(journal_mode.as_u8())),
            opener,
            corrupt: Arc::new(AtomicBool::new(false)),
            recovered: Arc::new(AtomicBool::new(true)),
            write_gate: Arc::new(AtomicBool::new(false)),
            on_corruption_change: Arc::new(std::sync::RwLock::new(None)),
            metrics: Arc::new(PoolMetrics::default()),
        })
    }

    /// Create a closed (empty) pool. All reads/writes return `None`
    /// (and `try_*` variants return `Err(DbError::Closed)`).
    /// Used at startup when storage is unavailable, and in tests.
    pub fn new_closed(label: &'static str) -> Self {
        Self {
            read_pool: Arc::new(std::sync::RwLock::new(None)),
            write_pool: Arc::new(std::sync::RwLock::new(None)),
            db_path: Arc::new(std::sync::RwLock::new(PathBuf::new())),
            label,
            read_size: 1,
            // No file → mode is moot; Delete is the safer default in case
            // an accidental write reaches the pool somehow.
            journal_mode: Arc::new(AtomicU8::new(JournalMode::Delete.as_u8())),
            opener: |_| Err(replay_control_core::error::Error::Other("closed".into())),
            corrupt: Arc::new(AtomicBool::new(false)),
            recovered: Arc::new(AtomicBool::new(false)),
            write_gate: Arc::new(AtomicBool::new(false)),
            on_corruption_change: Arc::new(std::sync::RwLock::new(None)),
            metrics: Arc::new(PoolMetrics::default()),
        }
    }

    /// Create a pool that starts in the corrupt state — no live connections,
    /// `is_corrupt()` returns true. Differs from `new_closed` in that the DB
    /// path and the real opener are wired, so `reopen()` (called by recovery
    /// flows like Restore/Reset) can rebuild the pool against the file once
    /// it has been replaced or recreated.
    pub fn new_corrupt(
        db_path: PathBuf,
        label: &'static str,
        opener: fn(&Path) -> replay_control_core::error::Result<rusqlite::Connection>,
        read_size: usize,
    ) -> Self {
        Self {
            read_pool: Arc::new(std::sync::RwLock::new(None)),
            write_pool: Arc::new(std::sync::RwLock::new(None)),
            db_path: Arc::new(std::sync::RwLock::new(db_path)),
            label,
            read_size,
            // Mode learned on next `reopen`.
            journal_mode: Arc::new(AtomicU8::new(JournalMode::Delete.as_u8())),
            opener,
            corrupt: Arc::new(AtomicBool::new(true)),
            recovered: Arc::new(AtomicBool::new(false)),
            write_gate: Arc::new(AtomicBool::new(false)),
            on_corruption_change: Arc::new(std::sync::RwLock::new(None)),
            metrics: Arc::new(PoolMetrics::default()),
        }
    }

    /// Snapshot the lifetime counters. Cheap atomic loads — fine to call
    /// from a debug HTTP endpoint or test.
    pub fn metrics(&self) -> PoolMetricsSnapshot {
        self.metrics.snapshot()
    }

    /// Run a read-only closure. `None` = pool unavailable; **don't read it
    /// as "no rows"**. Use [`Self::try_read`] when the result gates
    /// destructive work and the caller must distinguish unavailable from
    /// genuinely empty.
    pub async fn read<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&rusqlite::Connection) -> R + Send + 'static,
        R: Send + 'static,
    {
        self.try_read(f).await.ok()
    }

    pub async fn try_read<F, R>(&self, f: F) -> Result<R, DbError>
    where
        F: FnOnce(&rusqlite::Connection) -> R + Send + 'static,
        R: Send + 'static,
    {
        self.metrics.reads_started.fetch_add(1, Ordering::Relaxed);
        // `dispatch` is single-typed over `&mut Connection`; reads borrow
        // immutably via reborrow. `query_only=ON` is set on read connections
        // (see `SqliteManager::create`) so a misbehaving closure can't write.
        let result = self.dispatch(Op::Read, &self.read_pool, |c| f(c)).await;
        match &result {
            Ok(_) => {
                self.metrics.reads_completed.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => {
                if matches!(e, DbError::Busy) {
                    self.metrics
                        .gate_blocked_reads
                        .fetch_add(1, Ordering::Relaxed);
                }
                self.metrics
                    .reads_returned_none
                    .fetch_add(1, Ordering::Relaxed);
            }
        };
        result
    }

    /// Run a mutable closure. On DELETE-mode pools the write gate
    /// auto-activates around this call; concurrent reads get
    /// `Err(DbError::Busy)`. On WAL pools the gate stays unset.
    pub async fn write<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&mut rusqlite::Connection) -> R + Send + 'static,
        R: Send + 'static,
    {
        self.try_write(f).await.ok()
    }

    pub async fn try_write<F, R>(&self, f: F) -> Result<R, DbError>
    where
        F: FnOnce(&mut rusqlite::Connection) -> R + Send + 'static,
        R: Send + 'static,
    {
        self.metrics.writes_started.fetch_add(1, Ordering::Relaxed);
        let _gate = self
            .is_delete_mode()
            .then(|| WriteGate::activate(&self.write_gate));
        let result = self.dispatch(Op::Write, &self.write_pool, f).await;
        if result.is_ok() {
            self.metrics
                .writes_completed
                .fetch_add(1, Ordering::Relaxed);
        }
        result
    }

    /// Pool acquisition + interact + timeout — the shared body of
    /// `try_read` and `try_write`. Pre-flight gates (`Corrupt`, write-gate
    /// `Busy`) are checked here so both paths stay aligned.
    async fn dispatch<F, R>(
        &self,
        op: Op,
        slot: &Arc<std::sync::RwLock<Option<SqlitePool>>>,
        f: F,
    ) -> Result<R, DbError>
    where
        F: FnOnce(&mut rusqlite::Connection) -> R + Send + 'static,
        R: Send + 'static,
    {
        if self.corrupt.load(Ordering::Acquire) {
            return Err(DbError::Corrupt);
        }
        if op == Op::Read && self.write_gate.load(Ordering::Acquire) {
            return Err(DbError::Busy);
        }
        let pool = slot
            .read()
            .map_err(|_| DbError::Other("pool slot lock poisoned".into()))?
            .as_ref()
            .ok_or(DbError::Closed)?
            .clone();
        let conn = pool.get().await?;
        let pool_for_corrupt = self.clone();
        let label = self.label;
        let kind = op.kind();
        let interact = conn.interact(move |c| {
            let r = f(c);
            check_for_corruption(c, &pool_for_corrupt);
            r
        });
        match tokio::time::timeout(INTERACT_TIMEOUT, interact).await {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(e)) => {
                tracing::warn!("{label}: {kind} interact failed: {e}");
                Err(DbError::Interact(e))
            }
            Err(_) => {
                tracing::error!(
                    "{label}: {kind} exceeded {:?}; closure still running on blocking pool",
                    INTERACT_TIMEOUT
                );
                let counter = if op == Op::Read {
                    &self.metrics.reads_timed_out
                } else {
                    &self.metrics.writes_timed_out
                };
                counter.fetch_add(1, Ordering::Relaxed);
                Err(DbError::Timeout(INTERACT_TIMEOUT))
            }
        }
    }

    fn is_delete_mode(&self) -> bool {
        JournalMode::from_u8(self.journal_mode.load(Ordering::Acquire)) == JournalMode::Delete
    }

    /// Close pools and drain in-flight `Object`s. Returns `true` on clean
    /// drain, `false` on timeout (in which case destructive followups
    /// must NOT unlink files — a stuck closure may still write to them).
    pub async fn close(&self) -> bool {
        let read = self.read_pool.write().ok().and_then(|mut g| g.take());
        let write = self.write_pool.write().ok().and_then(|mut g| g.take());
        let read_ok = match read {
            Some(p) => drain_pool(p, "read", self.label).await,
            None => true,
        };
        let write_ok = match write {
            Some(p) => drain_pool(p, "write", self.label).await,
            None => true,
        };
        read_ok && write_ok
    }

    /// Reopen at a new DB *file* path. Drains current connections, runs
    /// WAL recovery (skipped when same path and already recovered),
    /// rebuilds both pools. Returns `false` if any step fails — pool
    /// stays closed.
    ///
    /// `db_path` is the actual SQLite file (not a storage root). All
    /// configured openers must take a file path, return a `Connection`,
    /// and not perform any path resolution of their own — that
    /// responsibility now lives at the call site, where it's typed.
    pub async fn reopen(&self, db_path: &Path) -> bool {
        if let Err(e) = (self.opener)(db_path) {
            tracing::debug!("Could not re-open {} DB: {e}", self.label);
            return false;
        }
        let path = db_path.to_path_buf();

        self.close().await;

        let same_path = self.db_path.read().map(|p| *p == path).unwrap_or(false);
        if !same_path || !self.recovered.load(Ordering::Acquire) {
            sqlite::recover_after_unclean_shutdown(&path);
            self.recovered.store(true, Ordering::Release);
        }

        let journal_mode = match sqlite::open_connection(&path, self.label) {
            Ok(c) => {
                let m = query_journal_mode(&c);
                drop(c);
                m
            }
            Err(e) => {
                tracing::debug!("Could not warm-open {} DB: {e}", self.label);
                return false;
            }
        };

        let make = |is_write, max_size, role: &str| {
            build_pool(
                &path,
                journal_mode,
                is_write,
                &format!("{}_{role}", self.label),
                max_size,
            )
        };
        let new_read = match make(false, self.read_size, "read") {
            Ok(p) => p,
            Err(e) => {
                tracing::debug!("Could not rebuild {} read pool: {e}", self.label);
                return false;
            }
        };
        let new_write = match make(true, 1, "write") {
            Ok(p) => p,
            Err(e) => {
                tracing::debug!("Could not rebuild {} write pool: {e}", self.label);
                return false;
            }
        };
        // Mode swap precedes pool swap: a concurrent `try_write` arriving
        // mid-reopen sees (new mode, old pool=closed→retry) — never the
        // unsafe (old mode, new pool) combination on a WAL↔DELETE crossover.
        self.journal_mode
            .store(journal_mode.as_u8(), Ordering::Release);
        if let Ok(mut guard) = self.read_pool.write() {
            *guard = Some(new_read);
        }
        if let Ok(mut guard) = self.write_pool.write() {
            *guard = Some(new_write);
        }
        if let Ok(mut guard) = self.db_path.write() {
            *guard = path;
        }
        let was_corrupt = self.corrupt.swap(false, Ordering::AcqRel);
        if was_corrupt {
            self.fire_corruption_callback();
        }
        true
    }

    /// Drain → unlink → reopen empty. The supported "clear and rebuild"
    /// entry point.
    pub async fn reset_to_empty(&self) -> bool {
        self.with_fresh_file(|_| Ok(())).await
    }

    /// Drain → unlink → copy `src` over → reopen. Used by user-data
    /// restore-from-backup. If the restored file is corrupt the inner
    /// `reopen()` returns `false` and callers fall back to
    /// `reset_to_empty()`.
    pub async fn replace_with_file(&self, src: &Path) -> bool {
        let src = src.to_path_buf();
        self.with_fresh_file(move |dst| std::fs::copy(&src, dst).map(|_| ()))
            .await
    }

    /// Drain (abort on timeout — stuck closures must not race the
    /// unlink), unlink sidecars, populate via `mutate`, reopen.
    async fn with_fresh_file<F>(&self, mutate: F) -> bool
    where
        F: FnOnce(&Path) -> std::io::Result<()> + Send + 'static,
    {
        let path = self.db_path();
        if path.as_os_str().is_empty() {
            return false;
        }
        if !self.close().await {
            tracing::error!(
                "{}: aborting destructive op — pool drain timed out, refusing to unlink",
                self.label
            );
            return false;
        }
        sqlite::delete_db_files(&path);
        if let Err(e) = mutate(&path) {
            tracing::warn!("{}: with_fresh_file mutate failed: {e}", self.label);
            return false;
        }
        self.recovered.store(false, Ordering::Release);
        self.reopen(&path).await
    }

    /// Test-only access to the gate flag. Production code does not need
    /// this — `write()` auto-activates on DELETE-mode pools. Tests use it
    /// to construct a `WriteGate` and assert read-side behaviour.
    #[cfg(test)]
    pub(crate) fn write_gate_flag(&self) -> &Arc<AtomicBool> {
        &self.write_gate
    }

    /// Check if the DB has been flagged as corrupt.
    pub fn is_corrupt(&self) -> bool {
        self.corrupt.load(Ordering::Relaxed)
    }

    /// Flag corrupt. Subsequent `try_*` return `Err(DbError::Corrupt)`;
    /// pool slots stay populated (mark_corrupt is sync, called from
    /// inside `interact()`). Idempotent. Recovery is the host crate's
    /// callback: `reset_to_empty()` or `replace_with_file()`.
    pub fn mark_corrupt(&self) {
        if self
            .corrupt
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            tracing::error!("{}: database flagged as corrupt", self.label);
            self.fire_corruption_callback();
        }
    }

    /// Cloneable handle to the corruption flag. Used by host crates to query
    /// state from a shared callback closure without holding a `DbPool` clone
    /// (which would form a cycle through `on_corruption_change`).
    pub fn corrupt_flag(&self) -> Arc<AtomicBool> {
        self.corrupt.clone()
    }

    /// Cloneable handle to the current DB path. The path can change across
    /// `reopen()`, so callbacks that need to derive sibling paths (e.g. a
    /// `.bak` next to the DB) should hold this and read on each fire. Holding
    /// the handle (rather than a `DbPool` clone) avoids a reference cycle
    /// through `on_corruption_change`.
    pub fn db_path_handle(&self) -> Arc<std::sync::RwLock<PathBuf>> {
        self.db_path.clone()
    }

    /// Register (or replace) the callback fired on corruption-flag transitions.
    /// Fires only on actual false→true (`mark_corrupt`) and true→false
    /// (successful `reopen`) edges.
    pub fn set_corruption_callback<F>(&self, cb: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        if let Ok(mut guard) = self.on_corruption_change.write() {
            *guard = Some(Box::new(cb));
        }
    }

    fn fire_corruption_callback(&self) {
        if let Ok(guard) = self.on_corruption_change.read()
            && let Some(cb) = guard.as_ref()
        {
            cb();
        }
    }

    /// Run a passive WAL checkpoint on the write connection.
    ///
    /// PASSIVE mode does not block readers. Call after heavy write operations
    /// (import, thumbnail rebuild, metadata clear) to fold the WAL back into
    /// the main database file and prevent unbounded WAL growth.
    pub async fn checkpoint(&self) {
        self.write(|conn| {
            let _ = conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE);");
        })
        .await;
    }

    /// Get the current DB file path.
    pub fn db_path(&self) -> PathBuf {
        self.db_path.read().expect("db_path lock poisoned").clone()
    }

    /// Check if the DB file still exists on disk.
    pub fn db_file_exists(&self) -> bool {
        self.db_path.read().expect("db_path lock poisoned").exists()
    }

    /// Check if a `<db>.bak` sibling exists next to the current DB file.
    /// Re-reads the path each call so it stays correct across `reopen()`.
    pub fn backup_path_exists(&self) -> bool {
        self.db_path
            .read()
            .expect("db_path lock poisoned")
            .with_extension("db.bak")
            .exists()
    }
}

/// Inspect the connection's most recent error code and flag the pool as
/// corrupt for any code that means "this file is unusable as a SQLite DB."
///
/// We look at both the primary code (`SQLITE_CORRUPT` 11) and `SQLITE_NOTADB`
/// (26). The latter fires when the 16-byte magic header at the start of the
/// file doesn't match — e.g. a partial write, a torn page on power loss, or
/// (in our test harness) `dd` clobbering page 1.
fn check_for_corruption(conn: &rusqlite::Connection, pool: &DbPool) {
    // SAFETY: sqlite3_errcode reads the error code of the most recent API call
    // on this connection. It's a single integer read from the db handle struct
    // — no side effects, no memory issues.
    let rc = unsafe { rusqlite::ffi::sqlite3_errcode(conn.handle()) };
    if rc == rusqlite::ffi::SQLITE_CORRUPT || rc == rusqlite::ffi::SQLITE_NOTADB {
        pool.mark_corrupt();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use replay_control_core::error::{Error, Result as CoreResult};
    use std::sync::atomic::AtomicU32;

    /// Test opener that creates a `kv` table on first open. Idempotent
    /// across reopens.
    fn test_opener(path: &Path) -> CoreResult<rusqlite::Connection> {
        let conn = sqlite::open_connection(path, "test_db")
            .map_err(|e| Error::Other(format!("open: {e}")))?;
        conn.execute_batch("CREATE TABLE IF NOT EXISTS kv (k TEXT PRIMARY KEY, v TEXT);")
            .map_err(|e| Error::Other(format!("create: {e}")))?;
        Ok(conn)
    }

    fn build_test_pool(tmp: &tempfile::TempDir) -> DbPool {
        build_test_pool_with(tmp, 1)
    }

    fn build_test_pool_with(tmp: &tempfile::TempDir, read_size: usize) -> DbPool {
        let path = tmp.path().join("test.db");
        let _ = test_opener(&path).unwrap();
        DbPool::new(path, "test_db", test_opener, read_size).expect("pool::new")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn read_write_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let pool = build_test_pool(&tmp);

        let written = pool
            .write(|conn| conn.execute("INSERT INTO kv VALUES ('greeting', 'hello')", []))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(written, 1);

        let value: Option<String> = pool
            .read(|conn| {
                conn.query_row("SELECT v FROM kv WHERE k = 'greeting'", [], |r| r.get(0))
                    .ok()
            })
            .await
            .flatten();
        assert_eq!(value.as_deref(), Some("hello"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn closed_pool_returns_none() {
        let pool = DbPool::new_closed("test_db");
        assert!(pool.read(|_| 1u32).await.is_none());
        assert!(pool.write(|_| 1u32).await.is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn multi_slot_reads_run_in_parallel() {
        // With a 3-slot read pool, a long-running read must not block
        // concurrent short reads — that's the WAL-concurrency win that
        // lets SSR fan-out (3+ server fns per page) complete in parallel
        // instead of serialising on a single connection mutex.
        let tmp = tempfile::tempdir().unwrap();
        let pool = build_test_pool_with(&tmp, 3);

        let slow = pool.clone();
        let slow_handle = tokio::spawn(async move {
            slow.read(|conn| {
                std::thread::sleep(std::time::Duration::from_millis(400));
                let _ = conn.query_row("SELECT 1", [], |r| r.get::<_, i64>(0));
            })
            .await
        });

        // Give the slow read a head start so it's holding its connection.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let hot_started = std::time::Instant::now();
        let hot = pool
            .read(|conn| conn.query_row("SELECT 1", [], |r| r.get::<_, i64>(0)).ok())
            .await
            .flatten();
        let hot_elapsed = hot_started.elapsed();
        assert_eq!(hot, Some(1));
        assert!(
            hot_elapsed < std::time::Duration::from_millis(200),
            "fast read should not have queued behind the slow one; took {hot_elapsed:?}"
        );
        let _ = slow_handle.await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn metrics_track_reads_writes_and_gate_blocks() {
        let tmp = tempfile::tempdir().unwrap();
        let pool = build_test_pool(&tmp);

        // Baseline: a fresh pool starts at zero.
        let m0 = pool.metrics();
        assert_eq!(m0.reads_started, 0);
        assert_eq!(m0.writes_started, 0);
        assert_eq!(m0.gate_blocked_reads, 0);

        pool.read(|_| 1u32).await.unwrap();
        pool.write(|_| 1u32).await.unwrap();

        let m1 = pool.metrics();
        assert_eq!(m1.reads_started, 1);
        assert_eq!(m1.reads_completed, 1);
        assert_eq!(m1.writes_started, 1);
        assert_eq!(m1.writes_completed, 1);
        assert_eq!(m1.gate_blocked_reads, 0);

        // Gate-blocked read increments both gate_blocked_reads and the
        // appropriate "returned None" counter.
        let gate = WriteGate::activate(pool.write_gate_flag());
        assert!(pool.read(|_| 1u32).await.is_none());
        let m2 = pool.metrics();
        assert_eq!(m2.gate_blocked_reads, 1);
        assert!(m2.reads_returned_none >= 1);
        drop(gate);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn close_then_read_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let pool = build_test_pool(&tmp);
        // Sanity: works before close.
        assert_eq!(pool.read(|_| 42u32).await, Some(42));
        pool.close().await;
        assert!(pool.read(|_| 42u32).await.is_none());
        assert!(pool.write(|_| 42u32).await.is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reopen_after_close_resumes_traffic() {
        let tmp = tempfile::tempdir().unwrap();
        let pool = build_test_pool(&tmp);
        pool.write(|conn| conn.execute("INSERT INTO kv VALUES ('a', '1')", []))
            .await
            .unwrap()
            .unwrap();
        pool.close().await;
        assert!(pool.read(|_| 1u32).await.is_none());

        // Reopen at the same storage root (the opener resolves the path).
        // The opener's `path` arg is the *storage root*; our test_opener
        // ignores that distinction and uses whatever it was given, so we
        // pass tmp's path verbatim.
        let opened = pool.reopen(&tmp.path().join("test.db")).await;
        assert!(opened, "reopen should succeed for valid DB");

        let value: Option<String> = pool
            .read(|conn| {
                conn.query_row("SELECT v FROM kv WHERE k = 'a'", [], |r| r.get(0))
                    .ok()
            })
            .await
            .flatten();
        assert_eq!(value.as_deref(), Some("1"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn mark_corrupt_flips_flag_and_closes() {
        let tmp = tempfile::tempdir().unwrap();
        let pool = build_test_pool(&tmp);
        assert!(!pool.is_corrupt());

        pool.mark_corrupt();

        assert!(pool.is_corrupt());
        assert!(pool.read(|_| 1u32).await.is_none());
        assert!(pool.write(|_| 1u32).await.is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn corruption_callback_fires_on_transitions_only() {
        let tmp = tempfile::tempdir().unwrap();
        let pool = build_test_pool(&tmp);

        let calls = Arc::new(AtomicU32::new(0));
        {
            let calls = calls.clone();
            pool.set_corruption_callback(move || {
                calls.fetch_add(1, Ordering::Relaxed);
            });
        }
        assert_eq!(calls.load(Ordering::Relaxed), 0);

        pool.mark_corrupt();
        assert_eq!(calls.load(Ordering::Relaxed), 1, "false→true fires once");

        pool.mark_corrupt();
        assert_eq!(
            calls.load(Ordering::Relaxed),
            1,
            "idempotent re-mark does not re-fire"
        );

        let opened = pool.reopen(&tmp.path().join("test.db")).await;
        assert!(opened, "true→false fires once");
        assert_eq!(calls.load(Ordering::Relaxed), 2, "true→false fires once");

        let opened = pool.reopen(&tmp.path().join("test.db")).await;
        assert!(opened);
        assert_eq!(
            calls.load(Ordering::Relaxed),
            2,
            "reopen on healthy pool does not re-fire"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn write_gate_blocks_reads_until_dropped() {
        let tmp = tempfile::tempdir().unwrap();
        let pool = build_test_pool(&tmp);

        // Sanity: read works before gate.
        assert_eq!(pool.read(|_| 1u32).await, Some(1));

        let gate = WriteGate::activate(pool.write_gate_flag());
        assert!(
            pool.read(|_| 1u32).await.is_none(),
            "gate should block reads"
        );
        // Writes are *not* gated — they still work.
        assert!(pool.write(|_| 1u32).await.is_some());

        drop(gate);

        assert_eq!(pool.read(|_| 1u32).await, Some(1));
    }

    #[test]
    fn db_path_and_db_file_exists_track_state() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nope.db");
        let pool = DbPool::new_closed("test_db");
        // Closed pool starts with empty path and no file.
        assert!(!pool.db_file_exists());
        // db_path() reflects whatever was set at construction.
        assert_eq!(pool.db_path(), PathBuf::new());
        // Sanity: helper doesn't blow up on a non-existent path either.
        assert!(!path.exists());
    }

    // Invariant tests — each maps to a real regression.

    /// Every commit is visible to every concurrent reader, even ones whose
    /// connection deadpool created lazily. Locks in the WAL-unlink fix.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_writes_visible_to_all_readers() {
        let tmp = tempfile::tempdir().unwrap();
        let pool = build_test_pool_with(&tmp, 3);

        // Force lazy connection creation by holding several reads
        // concurrently. The first commit happens here.
        pool.write(|conn| conn.execute("INSERT INTO kv VALUES ('seed', 'one')", []))
            .await
            .unwrap()
            .unwrap();

        let reads: Vec<_> = (0..6)
            .map(|_| {
                let p = pool.clone();
                tokio::spawn(async move {
                    p.read(|conn| {
                        conn.query_row("SELECT v FROM kv WHERE k = 'seed'", [], |r| {
                            r.get::<_, String>(0)
                        })
                        .ok()
                    })
                    .await
                    .flatten()
                })
            })
            .collect();

        for r in reads {
            assert_eq!(
                r.await.unwrap().as_deref(),
                Some("one"),
                "every reader, even ones whose connection deadpool created \
                 lazily, must see committed writes"
            );
        }

        // Second commit, then read again — a fresh post-commit fan-out.
        pool.write(|conn| conn.execute("UPDATE kv SET v = 'two' WHERE k = 'seed'", []))
            .await
            .unwrap()
            .unwrap();

        let reads: Vec<_> = (0..6)
            .map(|_| {
                let p = pool.clone();
                tokio::spawn(async move {
                    p.read(|conn| {
                        conn.query_row("SELECT v FROM kv WHERE k = 'seed'", [], |r| {
                            r.get::<_, String>(0)
                        })
                        .ok()
                    })
                    .await
                    .flatten()
                })
            })
            .collect();
        for r in reads {
            assert_eq!(
                r.await.unwrap().as_deref(),
                Some("two"),
                "post-commit fan-out reads must observe the latest write"
            );
        }
    }

    /// `reset_to_empty` drains in-flight reads before unlinking files.
    /// This is the primitive `rebuild_corrupt_library` and friends must
    /// use — the previous `pool.close(); delete_db_files(); reopen()`
    /// dance unlinked while live `Object`s held the inode.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn reset_to_empty_blocks_until_drain() {
        let tmp = tempfile::tempdir().unwrap();
        let pool = build_test_pool_with(&tmp, 2);

        pool.write(|conn| conn.execute("INSERT INTO kv VALUES ('a', '1')", []))
            .await
            .unwrap()
            .unwrap();

        // Long-running read in flight.
        let pool_for_read = pool.clone();
        let read = tokio::spawn(async move {
            pool_for_read
                .read(|conn| {
                    std::thread::sleep(std::time::Duration::from_millis(250));
                    conn.query_row("SELECT v FROM kv WHERE k = 'a'", [], |r| {
                        r.get::<_, String>(0)
                    })
                    .ok()
                })
                .await
                .flatten()
        });

        // Give the read a head start so it's holding its connection.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let reset_started = std::time::Instant::now();
        let ok = pool.reset_to_empty().await;
        let reset_elapsed = reset_started.elapsed();

        assert!(ok, "reset_to_empty should succeed against valid path");
        // The read must have finished cleanly (drain semantics) and seen
        // the row that existed *before* the reset.
        assert_eq!(read.await.unwrap().as_deref(), Some("1"));
        // And reset must have actually waited for it (>~150ms remaining).
        assert!(
            reset_elapsed >= std::time::Duration::from_millis(150),
            "reset should have drained the 250ms read before unlinking; \
             took only {reset_elapsed:?}"
        );

        // Post-reset the kv table is empty (schema preserved by opener).
        let row: Option<String> = pool
            .read(|conn| {
                conn.query_row("SELECT v FROM kv WHERE k = 'a'", [], |r| r.get(0))
                    .ok()
            })
            .await
            .flatten();
        assert_eq!(row, None, "reset_to_empty must clear the user data");
    }

    /// SQLite handles its own WAL recovery on first open in WAL mode.
    /// The pool's `recover_after_unclean_shutdown` is the
    /// cross-FS-mode-migration safety net, *not* the path that recovers
    /// committed-but-not-checkpointed writes — those are recovered by
    /// SQLite itself when the next process opens the DB.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn crash_recovery_simulation() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("crash.db");
        // Pre-create the schema so the pool's warmup connection sees a
        // usable DB. In production this is `LibraryDb::open_at` running
        // before `DbPool::new`.
        let _ = test_opener(&path).unwrap();

        {
            let pool = DbPool::new(path.clone(), "crash", test_opener, 1).unwrap();
            pool.write(|conn| {
                conn.execute("INSERT INTO kv VALUES ('survive_crash', 'committed')", [])
            })
            .await
            .unwrap()
            .unwrap();
            // Drop without checkpoint — the row lives in the WAL only.
            // Simulates a process crash between commit and checkpoint.
            drop(pool);
        }

        // New "process" opens the same file. Must observe the committed row.
        let pool = DbPool::new(path, "crash", test_opener, 1).unwrap();
        let row: Option<String> = pool
            .read(|conn| {
                conn.query_row("SELECT v FROM kv WHERE k = 'survive_crash'", [], |r| {
                    r.get(0)
                })
                .ok()
            })
            .await
            .flatten();
        assert_eq!(
            row.as_deref(),
            Some("committed"),
            "committed-but-not-checkpointed row must survive a process \
             restart — SQLite rolls forward the WAL on first open"
        );
    }

    /// Gate-blocked reads return `Err(DbError::Busy)` via `try_read`,
    /// not `Ok(default)`. This is the typed signal that destructive
    /// cascade code (e.g. is-empty checks) needs to distinguish "skip"
    /// from "library is empty".
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn gate_blocked_read_returns_typed_error() {
        let tmp = tempfile::tempdir().unwrap();
        let pool = build_test_pool(&tmp);

        // Manually activate the gate to simulate a write in progress.
        let _gate = WriteGate::activate(pool.write_gate_flag());
        let result = pool.try_read(|_| 1u32).await;
        assert!(
            matches!(result, Err(DbError::Busy)),
            "try_read must surface Busy as a typed error, never silently default; got {result:?}"
        );
    }

    /// Closed pool returns `Err(DbError::Closed)` from `try_read`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn closed_pool_try_read_returns_typed_error() {
        let pool = DbPool::new_closed("test_db");
        let result = pool.try_read(|_| 1u32).await;
        assert!(
            matches!(result, Err(DbError::Closed)),
            "closed pool must return Err(Closed), not Err(Op); got {result:?}"
        );
    }

    /// Corrupt pool returns `Err(DbError::Corrupt)` from `try_read`/`try_write`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn corrupt_pool_try_read_returns_typed_error() {
        let tmp = tempfile::tempdir().unwrap();
        let pool = build_test_pool(&tmp);
        pool.mark_corrupt();
        let r = pool.try_read(|_| 1u32).await;
        assert!(matches!(r, Err(DbError::Corrupt)), "got {r:?}");
        let w = pool.try_write(|_| 1u32).await;
        assert!(matches!(w, Err(DbError::Corrupt)), "got {w:?}");
    }

    /// The auto-gate is conditional on journal mode: WAL pools never
    /// activate the gate from `write()`. This test runs on whatever FS
    /// the temp dir lives on — typically ext4 in CI, so WAL — and
    /// asserts a write doesn't briefly block reads.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn wal_writes_do_not_block_concurrent_reads() {
        let tmp = tempfile::tempdir().unwrap();
        let pool = build_test_pool_with(&tmp, 2);

        // Skip on filesystems that fall back to DELETE mode (e.g. NFS).
        let path = tmp.path().to_path_buf();
        if !crate::storage::supports_wal(&path) {
            eprintln!("skipping wal_writes_do_not_block_concurrent_reads on non-WAL FS");
            return;
        }

        // Long-running write — 200ms inside the closure.
        let writer_pool = pool.clone();
        let writer = tokio::spawn(async move {
            writer_pool
                .write(|conn| {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    conn.execute("INSERT INTO kv VALUES ('w', 'late')", [])
                })
                .await
        });

        // Give the write time to enter the closure.
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;

        let read_started = std::time::Instant::now();
        let result = pool.try_read(|_| 1u32).await;
        let read_elapsed = read_started.elapsed();
        assert!(
            matches!(result, Ok(1)),
            "WAL pool must not gate the read; got {result:?}"
        );
        assert!(
            read_elapsed < std::time::Duration::from_millis(120),
            "WAL read must overlap with the write, not queue behind it; took {read_elapsed:?}"
        );
        let _ = writer.await;
    }
}
