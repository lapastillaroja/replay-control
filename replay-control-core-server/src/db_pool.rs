//! Async SQLite connection pool built on deadpool-sqlite.
//!
//! Separate read and write pools per database, with filesystem-aware journal
//! mode selection (WAL on POSIX, DELETE on exFAT/NFS) via `sqlite::open_connection`.
//! Includes a corruption flag (set on SQLITE_CORRUPT and closes the pool) and a
//! `WriteGate` RAII guard that prevents concurrent reads during heavy writes
//! on non-WAL filesystems.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use deadpool::managed::{self, Metrics, RecycleError};
use deadpool_sync::SyncWrapper;

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
            // Reduce page cache from default 2000 pages (8MB) to 500 (2MB).
            // With 4 connections per pool this saves ~24MB of RSS.
            conn.execute_batch("PRAGMA cache_size = 500;")?;
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
    /// Hot-path read connection (SSR pages, server fns).
    read_pool: Arc<std::sync::RwLock<Option<SqlitePool>>>,
    /// Background-read connection: a dedicated slot for long-running reads
    /// (`build_image_index`, `plan_system_thumbnails`, the manifest fuzzy-
    /// index build) so they don't park on the SSR-serving connection's
    /// mutex while the planning loop walks tens of thousands of rows.
    /// One connection on WAL is plenty (writers don't block readers); on
    /// non-WAL the gate path covers correctness, and this slot still
    /// keeps the SSR connection free for SSR fan-out.
    /// See Tier 3 of `2026-04-29-pool-design-findings.md`.
    read_bg_pool: Arc<std::sync::RwLock<Option<SqlitePool>>>,
    /// Single write connection (SQLite serialises writes).
    write_pool: Arc<std::sync::RwLock<Option<SqlitePool>>>,
    db_path: Arc<std::sync::RwLock<PathBuf>>,
    label: &'static str,
    /// Opener function for creating additional connections (used by `reopen()`
    /// to verify the DB is accessible before rebuilding pools).
    opener: fn(&Path) -> replay_control_core::error::Result<(rusqlite::Connection, PathBuf)>,
    /// Set when a query returns SQLITE_CORRUPT (11) or SQLITE_NOTADB (26).
    /// Once set, the pool is closed and all reads/writes return None until
    /// the DB is rebuilt/repaired and the flag is cleared. NOTADB covers the
    /// case where the file's 16-byte magic header has been overwritten and
    /// SQLite refuses to identify it as a database at all.
    corrupt: Arc<AtomicBool>,
    /// When set, `read()` returns `None` immediately without acquiring a
    /// connection. Prevents SQLite corruption on exFAT (DELETE journal mode)
    /// during heavy write operations (import, rebuild, thumbnail index).
    write_gate: Arc<AtomicBool>,
    /// Fires on every actual transition of the corruption flag — both
    /// false→true (`mark_corrupt`) and true→false (`reopen` success path).
    /// Lets the host crate broadcast a status event without each call site
    /// needing to remember to do so. Idempotent flag writes do not re-fire.
    on_corruption_change: CorruptionCallback,
}

/// Number of read connections per pool. Load tests on USB storage (DELETE journal
/// mode, no WAL) showed no performance improvement with more than 1 reader — the
/// single-user access pattern and fast queries (<50ms) don't benefit from
/// concurrent readers. Keeping 1 reduces memory by ~2MB per saved connection.
const READ_POOL_SIZE: usize = 1;

/// Number of read connections in the **background** pool, used for long-
/// running reads (`build_image_index`, `plan_system_thumbnails`, manifest
/// fuzzy-index builds). One slot is enough — these calls are sequential
/// inside the import / thumbnail pipelines — and it keeps the SSR-serving
/// `read_pool` free of long-held connections. ~2 MB additional RSS.
const READ_BG_POOL_SIZE: usize = 1;

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

/// RAII guard that gates DB reads during heavy writes.
///
/// While held, `DbPool::read()` returns `None` for the gated pool.
/// Automatically clears the gate on drop (including panic).
pub struct WriteGate(Arc<AtomicBool>);

impl WriteGate {
    pub fn activate(flag: &Arc<AtomicBool>) -> Self {
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
        opener: fn(&Path) -> replay_control_core::error::Result<(rusqlite::Connection, PathBuf)>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let warmup = sqlite::open_connection(&db_path, label)
            .map_err(|e| format!("{label}: failed to open warmup connection: {e}"))?;
        let journal_mode = query_journal_mode(&warmup);
        drop(warmup);

        let read_pool = build_pool(
            &db_path,
            journal_mode,
            false,
            &format!("{label}_read"),
            READ_POOL_SIZE,
        )?;
        let read_bg_pool = build_pool(
            &db_path,
            journal_mode,
            false,
            &format!("{label}_read_bg"),
            READ_BG_POOL_SIZE,
        )?;
        let write_pool = build_pool(&db_path, journal_mode, true, &format!("{label}_write"), 1)?;

        Ok(Self {
            read_pool: Arc::new(std::sync::RwLock::new(Some(read_pool))),
            read_bg_pool: Arc::new(std::sync::RwLock::new(Some(read_bg_pool))),
            write_pool: Arc::new(std::sync::RwLock::new(Some(write_pool))),
            db_path: Arc::new(std::sync::RwLock::new(db_path)),
            label,
            opener,
            corrupt: Arc::new(AtomicBool::new(false)),
            write_gate: Arc::new(AtomicBool::new(false)),
            on_corruption_change: Arc::new(std::sync::RwLock::new(None)),
        })
    }

    /// Create a closed (empty) pool. All reads/writes return `None`.
    /// Used at startup when storage is unavailable, and in tests.
    pub fn new_closed(label: &'static str) -> Self {
        Self {
            read_pool: Arc::new(std::sync::RwLock::new(None)),
            read_bg_pool: Arc::new(std::sync::RwLock::new(None)),
            write_pool: Arc::new(std::sync::RwLock::new(None)),
            db_path: Arc::new(std::sync::RwLock::new(PathBuf::new())),
            label,
            opener: |_| Err(replay_control_core::error::Error::Other("closed".into())),
            corrupt: Arc::new(AtomicBool::new(false)),
            write_gate: Arc::new(AtomicBool::new(false)),
            on_corruption_change: Arc::new(std::sync::RwLock::new(None)),
        }
    }

    /// Create a pool that starts in the corrupt state — no live connections,
    /// `is_corrupt()` returns true. Differs from `new_closed` in that the DB
    /// path and the real opener are wired, so `reopen()` (called by recovery
    /// flows like Restore/Reset) can rebuild the pool against the file once
    /// it has been replaced or recreated.
    ///
    /// Used at startup when the user-data DB on disk has an invalid SQLite
    /// header (NOTADB-class corruption) and `open_connection` would otherwise
    /// crash the service.
    pub fn new_corrupt(
        db_path: PathBuf,
        label: &'static str,
        opener: fn(&Path) -> replay_control_core::error::Result<(rusqlite::Connection, PathBuf)>,
    ) -> Self {
        Self {
            read_pool: Arc::new(std::sync::RwLock::new(None)),
            read_bg_pool: Arc::new(std::sync::RwLock::new(None)),
            write_pool: Arc::new(std::sync::RwLock::new(None)),
            db_path: Arc::new(std::sync::RwLock::new(db_path)),
            label,
            opener,
            corrupt: Arc::new(AtomicBool::new(true)),
            write_gate: Arc::new(AtomicBool::new(false)),
            on_corruption_change: Arc::new(std::sync::RwLock::new(None)),
        }
    }

    /// Run a read-only closure with a database connection from the read pool.
    ///
    /// Multiple concurrent `read()` calls get different connections (up to
    /// `max_size`), enabling true concurrent reads under WAL mode.
    ///
    /// Uses deadpool's async API: `pool.get().await` suspends the task without
    /// pinning a tokio worker, and `interact()` runs the closure via
    /// `spawn_blocking`. This prevents worker thread starvation when many
    /// resources compete for a small pool.
    ///
    /// Returns `None` if the pool is closed (DB unavailable).
    pub async fn read<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&rusqlite::Connection) -> R + Send + 'static,
        R: Send + 'static,
    {
        // Write gate: heavy writes in progress, return None to prevent
        // concurrent reads that corrupt the DB on exFAT (DELETE journal mode).
        if self.write_gate.load(Ordering::Acquire) {
            return None;
        }
        self.run_read(&self.read_pool, "read", f).await
    }

    /// Run a read-only closure on the **background** pool slot.
    ///
    /// Use for long-running reads (image-index build, manifest planning,
    /// fuzzy-index build) so they don't park on the SSR-serving read
    /// connection's mutex. Same write-gate semantics as `read()` — on
    /// non-WAL filesystems we still refuse to overlap reads with an
    /// in-flight writer.
    pub async fn read_bg<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&rusqlite::Connection) -> R + Send + 'static,
        R: Send + 'static,
    {
        if self.write_gate.load(Ordering::Acquire) {
            return None;
        }
        self.run_read(&self.read_bg_pool, "read_bg", f).await
    }

    async fn run_read<F, R>(
        &self,
        pool_slot: &Arc<std::sync::RwLock<Option<SqlitePool>>>,
        kind: &'static str,
        f: F,
    ) -> Option<R>
    where
        F: FnOnce(&rusqlite::Connection) -> R + Send + 'static,
        R: Send + 'static,
    {
        let pool = pool_slot.read().ok()?.as_ref()?.clone();
        let conn = pool.get().await.ok()?;
        let pool_for_corrupt = self.clone();
        let label = self.label;
        let interact = conn.interact(move |conn| {
            let result = f(conn);
            check_for_corruption(conn, &pool_for_corrupt);
            result
        });
        match tokio::time::timeout(INTERACT_TIMEOUT, interact).await {
            Ok(Ok(value)) => Some(value),
            Ok(Err(e)) => {
                tracing::warn!("{label}: {kind} interact failed: {e}");
                None
            }
            Err(_) => {
                tracing::error!(
                    "{label}: {kind} interact exceeded {:?}; closure still running on \
                     blocking pool — connection mutex pinned until it completes",
                    INTERACT_TIMEOUT
                );
                None
            }
        }
    }

    /// Run a mutable closure with the single write connection.
    ///
    /// While the write is in flight, `read()` callers will see `None` so
    /// concurrent readers can't race the writer on exFAT (DELETE journal).
    /// The gate is held only for the duration of this single write — long
    /// write *sequences* should call `write()` per logical write, not hold
    /// an outer gate, so SSR readers stay responsive between calls.
    ///
    /// Returns `None` if the pool is closed (DB unavailable).
    pub async fn write<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&mut rusqlite::Connection) -> R + Send + 'static,
        R: Send + 'static,
    {
        let _gate = WriteGate::activate(&self.write_gate);
        let pool = self.write_pool.read().ok()?.as_ref()?.clone();
        let conn = pool.get().await.ok()?;
        let pool_for_corrupt = self.clone();
        let label = self.label;
        let interact = conn.interact(move |conn| {
            let result = f(conn);
            check_for_corruption(conn, &pool_for_corrupt);
            result
        });
        match tokio::time::timeout(INTERACT_TIMEOUT, interact).await {
            Ok(Ok(value)) => Some(value),
            Ok(Err(e)) => {
                tracing::warn!("{label}: write interact failed: {e}");
                None
            }
            Err(_) => {
                tracing::error!(
                    "{label}: write interact exceeded {:?}; closure still running on \
                     blocking pool — connection mutex pinned until it completes",
                    INTERACT_TIMEOUT
                );
                None
            }
        }
    }

    /// Close the pools (e.g., after storage change).
    /// Next call to `read`/`write` will return `None` until `reopen` is called.
    pub fn close(&self) {
        if let Ok(mut guard) = self.read_pool.write() {
            *guard = None;
        }
        if let Ok(mut guard) = self.read_bg_pool.write() {
            *guard = None;
        }
        if let Ok(mut guard) = self.write_pool.write() {
            *guard = None;
        }
    }

    /// Re-open at a new storage root. Rebuilds both pools with fresh connections.
    pub fn reopen(&self, storage_root: &Path) -> bool {
        // Verify we can open the DB at the new location.
        match (self.opener)(storage_root) {
            Ok((conn, path)) => {
                let journal_mode = query_journal_mode(&conn);
                drop(conn);

                let new_read = match build_pool(
                    &path,
                    journal_mode,
                    false,
                    &format!("{}_read", self.label),
                    READ_POOL_SIZE,
                ) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::debug!("Could not rebuild {} read pool: {e}", self.label);
                        return false;
                    }
                };
                let new_read_bg = match build_pool(
                    &path,
                    journal_mode,
                    false,
                    &format!("{}_read_bg", self.label),
                    READ_BG_POOL_SIZE,
                ) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::debug!("Could not rebuild {} read_bg pool: {e}", self.label);
                        return false;
                    }
                };
                let new_write = match build_pool(
                    &path,
                    journal_mode,
                    true,
                    &format!("{}_write", self.label),
                    1,
                ) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::debug!("Could not rebuild {} write pool: {e}", self.label);
                        return false;
                    }
                };
                // Swap pools (old connections drain naturally when Objects are returned).
                if let Ok(mut guard) = self.read_pool.write() {
                    *guard = Some(new_read);
                }
                if let Ok(mut guard) = self.read_bg_pool.write() {
                    *guard = Some(new_read_bg);
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
            Err(e) => {
                tracing::debug!("Could not re-open {} DB: {e}", self.label);
                false
            }
        }
    }

    /// Get the write gate flag for use with `WriteGate::activate()`.
    pub fn write_gate_flag(&self) -> &Arc<AtomicBool> {
        &self.write_gate
    }

    /// Check if the DB has been flagged as corrupt.
    pub fn is_corrupt(&self) -> bool {
        self.corrupt.load(Ordering::Relaxed)
    }

    /// Flag the DB as corrupt and close all connections.
    /// Idempotent: safe to call from multiple threads simultaneously. The
    /// corruption callback fires only on the actual false→true transition,
    /// not on subsequent redundant calls.
    pub fn mark_corrupt(&self) {
        if self
            .corrupt
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            tracing::error!("{}: database flagged as corrupt", self.label);
            self.close();
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

    /// Test opener that creates a tiny `kv` table on first open.
    /// Returns the same path it was given. Used by `reopen` tests.
    fn test_opener(path: &Path) -> CoreResult<(rusqlite::Connection, PathBuf)> {
        let conn = sqlite::open_connection(path, "test_db")
            .map_err(|e| Error::Other(format!("open: {e}")))?;
        conn.execute_batch("CREATE TABLE IF NOT EXISTS kv (k TEXT PRIMARY KEY, v TEXT);")
            .map_err(|e| Error::Other(format!("create: {e}")))?;
        Ok((conn, path.to_path_buf()))
    }

    /// Build a pool over a fresh DB inside `tmp`, returning (pool, db_path).
    /// Pool is fully warmed and ready for read/write.
    fn build_test_pool(tmp: &tempfile::TempDir) -> DbPool {
        let path = tmp.path().join("test.db");
        // Pre-create the schema so DbPool::new()'s warmup connection sees a
        // valid DB (otherwise the empty file would still work, but writes
        // below need the kv table).
        let (_, _) = test_opener(&path).unwrap();
        DbPool::new(path, "test_db", test_opener).expect("pool::new")
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
        assert!(pool.read_bg(|_| 1u32).await.is_none());
        assert!(pool.write(|_| 1u32).await.is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn read_bg_pool_serves_separate_connection() {
        let tmp = tempfile::tempdir().unwrap();
        let pool = build_test_pool(&tmp);

        // Seed via write.
        pool.write(|conn| conn.execute("INSERT INTO kv VALUES ('k', 'v')", []))
            .await
            .unwrap()
            .unwrap();

        // The hot-path read pool sees the row.
        let v_hot: Option<String> = pool
            .read(|conn| {
                conn.query_row("SELECT v FROM kv WHERE k = 'k'", [], |r| r.get(0))
                    .ok()
            })
            .await
            .flatten();
        assert_eq!(v_hot.as_deref(), Some("v"));

        // The background read pool sees the same data (separate connection,
        // same DB file).
        let v_bg: Option<String> = pool
            .read_bg(|conn| {
                conn.query_row("SELECT v FROM kv WHERE k = 'k'", [], |r| r.get(0))
                    .ok()
            })
            .await
            .flatten();
        assert_eq!(v_bg.as_deref(), Some("v"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn read_bg_runs_in_parallel_with_read() {
        // The whole point of the background pool: a long read on read_bg
        // must not block a concurrent SSR-style read on the hot pool.
        // (WAL allows true concurrency; on non-WAL/DELETE the writer gate
        // is what enforces correctness, not this pool split — but reads
        // still don't block other reads.)
        let tmp = tempfile::tempdir().unwrap();
        let pool = build_test_pool(&tmp);

        let pool_bg = pool.clone();
        let bg = tokio::spawn(async move {
            pool_bg
                .read_bg(|conn| {
                    // Simulate a multi-second read by sleeping inside the
                    // closure. The hot read should still complete fast.
                    std::thread::sleep(std::time::Duration::from_millis(400));
                    let _ = conn.query_row("SELECT 1", [], |r| r.get::<_, i64>(0));
                })
                .await
        });

        // Give bg a head start so it's holding its connection.
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
            "hot read should not have waited for the background read; took {hot_elapsed:?}"
        );
        let _ = bg.await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn write_gate_also_blocks_read_bg() {
        let tmp = tempfile::tempdir().unwrap();
        let pool = build_test_pool(&tmp);
        // Sanity: read_bg works before gate.
        assert_eq!(pool.read_bg(|_| 1u32).await, Some(1));
        let gate = WriteGate::activate(pool.write_gate_flag());
        assert!(
            pool.read_bg(|_| 1u32).await.is_none(),
            "background reads also gated — non-WAL correctness"
        );
        drop(gate);
        assert_eq!(pool.read_bg(|_| 1u32).await, Some(1));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn close_then_read_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let pool = build_test_pool(&tmp);
        // Sanity: works before close.
        assert_eq!(pool.read(|_| 42u32).await, Some(42));
        pool.close();
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
        pool.close();
        assert!(pool.read(|_| 1u32).await.is_none());

        // Reopen at the same storage root (the opener resolves the path).
        // The opener's `path` arg is the *storage root*; our test_opener
        // ignores that distinction and uses whatever it was given, so we
        // pass tmp's path verbatim.
        let opened = pool.reopen(&tmp.path().join("test.db"));
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

        let opened = pool.reopen(&tmp.path().join("test.db"));
        assert!(opened);
        assert_eq!(calls.load(Ordering::Relaxed), 2, "true→false fires once");

        let opened = pool.reopen(&tmp.path().join("test.db"));
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
}
