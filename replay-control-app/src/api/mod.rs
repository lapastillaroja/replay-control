pub mod activity;
pub(crate) mod background;
pub(crate) mod cache;
pub(crate) mod core_api;
pub mod favorites;
pub mod import;
pub mod recents;
pub mod roms;
pub mod system_info;
pub mod upload;

pub use activity::{Activity, ActivityGuard, MaintenanceKind, StartupPhase};
pub use background::BackgroundManager;
pub use cache::GameLibrary;
pub use import::{ImportPipeline, ThumbnailPipeline};

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use deadpool_sqlite::rusqlite;
use replay_control_core::config::ReplayConfig;
use replay_control_core::db_common::JournalMode;
use replay_control_core::storage::{StorageKind, StorageLocation};

// ── Custom deadpool Manager ───────────────────────────────────────

use deadpool::managed::{self, Metrics, RecycleError};
use deadpool_sync::SyncWrapper;

/// Custom deadpool Manager that uses `db_common::open_connection()` for
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
    last_optimize: std::sync::Arc<std::sync::Mutex<std::time::Instant>>,
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
            let conn =
                replay_control_core::db_common::open_connection(&db_path, &label).map_err(|e| {
                    rusqlite::Error::SqliteFailure(
                        rusqlite::ffi::Error::new(1),
                        Some(e.to_string()),
                    )
                })?;

            // Per-role PRAGMAs (on top of the base PRAGMAs from open_connection):
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

// ── DbPool ────────────────────────────────────────────────────────

/// Connection pool for a single SQLite database.
///
/// Uses `deadpool` for true concurrent reads (WAL mode allows multiple readers)
/// with separate read and write pools.
///
/// - **Read pool**: `max_size=3` for WAL mode (concurrent readers), `1` for DELETE mode
/// - **Write pool**: `max_size=1` (SQLite serialises writes)
///
/// Provides async `read()` / `write()` helpers that use deadpool's native async
/// API: `pool.get().await` for connection acquisition and `conn.interact()` for
/// running closures on a blocking thread. This ensures waiting for a connection
/// never pins a tokio worker thread.
///
/// The pools are wrapped in `Arc<RwLock<>>` so that `close()` / `reopen()` can
/// swap them across all clones of the same `DbPool`.
#[derive(Clone)]
pub struct DbPool {
    /// Multiple read connections (WAL concurrent readers).
    read_pool: Arc<std::sync::RwLock<Option<SqlitePool>>>,
    /// Single write connection (SQLite serialises writes).
    write_pool: Arc<std::sync::RwLock<Option<SqlitePool>>>,
    db_path: Arc<std::sync::RwLock<PathBuf>>,
    label: &'static str,
    /// Opener function for creating additional connections (used by `reopen()`
    /// to verify the DB is accessible before rebuilding pools).
    opener:
        fn(&std::path::Path) -> replay_control_core::error::Result<(rusqlite::Connection, PathBuf)>,
    /// Set when a query returns SQLITE_CORRUPT (error code 11).
    /// Once set, the pool is closed and all reads/writes return None until
    /// the DB is rebuilt/repaired and the flag is cleared.
    corrupt: Arc<AtomicBool>,
}

/// Build a deadpool `SqlitePool` with the given size.
fn build_pool(
    db_path: &std::path::Path,
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
        last_optimize: std::sync::Arc::new(std::sync::Mutex::new(std::time::Instant::now())),
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
    /// Create a new pool. Opens the DB eagerly (via `opener`) to fail fast at
    /// startup, then queries the actual journal mode to size pools correctly.
    fn new(
        db_path: PathBuf,
        label: &'static str,
        opener: fn(
            &std::path::Path,
        ) -> replay_control_core::error::Result<(rusqlite::Connection, PathBuf)>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // Open a warmup connection to detect the actual journal mode.
        // open_connection() picks WAL or DELETE based on filesystem capabilities,
        // so we query the result rather than guessing.
        let warmup = replay_control_core::db_common::open_connection(&db_path, label)
            .map_err(|e| format!("{label}: failed to open warmup connection: {e}"))?;
        let journal_mode = query_journal_mode(&warmup);
        drop(warmup);

        let read_size = match journal_mode {
            JournalMode::Wal => 3,
            JournalMode::Delete => 3, // DELETE mode supports concurrent readers when no writer is active
        };
        let read_pool = build_pool(
            &db_path,
            journal_mode,
            false,
            &format!("{label}_read"),
            read_size,
        )?;
        let write_pool = build_pool(&db_path, journal_mode, true, &format!("{label}_write"), 1)?;

        // Warm one read + one write connection eagerly. If this fails, the DB
        // is inaccessible and there is no point starting the server.
        drop(
            Self::warmup_get(&read_pool)
                .ok_or_else(|| format!("{label}: failed to warm read connection"))?,
        );
        drop(
            Self::warmup_get(&write_pool)
                .ok_or_else(|| format!("{label}: failed to warm write connection"))?,
        );
        // Remaining read connections (2 more on WAL) created lazily on demand.

        Ok(Self {
            read_pool: Arc::new(std::sync::RwLock::new(Some(read_pool))),
            write_pool: Arc::new(std::sync::RwLock::new(Some(write_pool))),
            db_path: Arc::new(std::sync::RwLock::new(db_path)),
            label,
            opener,
            corrupt: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Create a closed (empty) pool for tests. All reads/writes return `None`.
    #[cfg(test)]
    pub(crate) fn new_closed(label: &'static str) -> Self {
        Self {
            read_pool: Arc::new(std::sync::RwLock::new(None)),
            write_pool: Arc::new(std::sync::RwLock::new(None)),
            db_path: Arc::new(std::sync::RwLock::new(PathBuf::new())),
            label,
            opener: |_| Err(replay_control_core::error::Error::Other("test".into())),
            corrupt: Arc::new(AtomicBool::new(false)),
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
        let pool = self.read_pool.read().ok()?.as_ref()?.clone();
        let conn = pool.get().await.ok()?;
        let corrupt_flag = self.corrupt.clone();
        let result = conn
            .interact(move |conn| {
                let result = f(conn);
                // SAFETY: sqlite3_errcode reads the error code of the most recent
                // API call on this connection. It's a single integer read from the
                // db handle struct — no side effects, no memory issues.
                let rc = unsafe { rusqlite::ffi::sqlite3_errcode(conn.handle()) };
                if rc == rusqlite::ffi::SQLITE_CORRUPT {
                    corrupt_flag.store(true, Ordering::Relaxed);
                }
                result
            })
            .await
            .ok();
        if self.is_corrupt() {
            self.close();
        }
        result
    }

    /// Run a mutable closure with the single write connection.
    ///
    /// Returns `None` if the pool is closed (DB unavailable).
    pub async fn write<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&mut rusqlite::Connection) -> R + Send + 'static,
        R: Send + 'static,
    {
        let pool = self.write_pool.read().ok()?.as_ref()?.clone();
        let conn = pool.get().await.ok()?;
        let corrupt_flag = self.corrupt.clone();
        let result = conn
            .interact(move |conn| {
                let result = f(conn);
                let rc = unsafe { rusqlite::ffi::sqlite3_errcode(conn.handle()) };
                if rc == rusqlite::ffi::SQLITE_CORRUPT {
                    corrupt_flag.store(true, Ordering::Relaxed);
                }
                result
            })
            .await
            .ok();
        if self.is_corrupt() {
            self.close();
        }
        result
    }

    /// Close the pools (e.g., after storage change).
    /// Next call to `read`/`write` will return `None` until `reopen` is called.
    pub(crate) fn close(&self) {
        if let Ok(mut guard) = self.read_pool.write() {
            *guard = None;
        }
        if let Ok(mut guard) = self.write_pool.write() {
            *guard = None;
        }
    }

    /// Re-open at a new storage root. Rebuilds both pools with fresh connections.
    pub(crate) fn reopen(&self, storage_root: &std::path::Path) -> bool {
        // Verify we can open the DB at the new location.
        match (self.opener)(storage_root) {
            Ok((conn, path)) => {
                let journal_mode = query_journal_mode(&conn);
                drop(conn);

                let read_size = match journal_mode {
                    JournalMode::Wal => 3,
                    JournalMode::Delete => 1,
                };
                let new_read = match build_pool(
                    &path,
                    journal_mode,
                    false,
                    &format!("{}_read", self.label),
                    read_size,
                ) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::debug!("Could not rebuild {} read pool: {e}", self.label);
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
                if let Ok(mut guard) = self.write_pool.write() {
                    *guard = Some(new_write);
                }
                if let Ok(mut guard) = self.db_path.write() {
                    *guard = path;
                }
                self.corrupt.store(false, Ordering::Relaxed);
                true
            }
            Err(e) => {
                tracing::debug!("Could not re-open {} DB: {e}", self.label);
                false
            }
        }
    }

    /// Check if the DB has been flagged as corrupt.
    pub fn is_corrupt(&self) -> bool {
        self.corrupt.load(Ordering::Relaxed)
    }

    /// Flag the DB as corrupt and close all connections.
    /// Idempotent: safe to call from multiple threads simultaneously.
    pub(crate) fn mark_corrupt(&self) {
        tracing::error!("{}: database flagged as corrupt", self.label);
        self.corrupt.store(true, Ordering::Relaxed);
        self.close();
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

    /// Synchronously warm a connection from a deadpool pool at startup.
    ///
    /// **Only for use during `DbPool::new()`** — before the server starts
    /// accepting requests. Production read/write paths use the async API
    /// (`pool.get().await` + `interact()`).
    fn warmup_get(pool: &SqlitePool) -> Option<managed::Object<SqliteManager>> {
        let handle = tokio::runtime::Handle::try_current().ok()?;
        let result = tokio::task::block_in_place(|| handle.block_on(pool.get()));
        result.ok()
    }
}

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub storage: Arc<std::sync::RwLock<StorageLocation>>,
    pub config: Arc<std::sync::RwLock<ReplayConfig>>,
    pub config_path: Option<PathBuf>,
    pub cache: Arc<GameLibrary>,
    /// When set, --storage-path was given on the CLI and auto-detection is skipped.
    pub storage_path_override: Option<PathBuf>,
    /// When Some, the app uses this skin index (persisted in `settings.cfg`).
    /// When None, defers to `replay.cfg`'s `system_skin` (sync mode).
    pub skin_override: Arc<std::sync::RwLock<Option<u32>>>,
    /// Metadata DB pool (deadpool-backed, concurrent reads).
    pub metadata_pool: DbPool,
    /// User data DB pool (deadpool-backed, concurrent reads).
    pub user_data_pool: DbPool,
    /// Import pipeline (metadata import operations).
    pub import: Arc<ImportPipeline>,
    /// Thumbnail pipeline (index + download operations).
    pub thumbnails: Arc<ThumbnailPipeline>,
    /// Track in-flight on-demand thumbnail downloads to avoid duplicates.
    pub pending_downloads: Arc<std::sync::RwLock<std::collections::HashSet<String>>>,
    /// Unified activity state: at most one activity at a time.
    /// Replaces `busy`, `busy_label`, `scanning`, and `rebuild_progress`.
    pub(crate) activity: Arc<std::sync::RwLock<Activity>>,
}

/// Opener for metadata DB.
fn open_metadata_db(
    storage_root: &std::path::Path,
) -> replay_control_core::error::Result<(rusqlite::Connection, PathBuf)> {
    replay_control_core::metadata_db::MetadataDb::open(storage_root)
}

/// Opener for user data DB.
fn open_user_data_db(
    storage_root: &std::path::Path,
) -> replay_control_core::error::Result<(rusqlite::Connection, PathBuf)> {
    let (conn, path, _corrupt) = replay_control_core::user_data_db::UserDataDb::open(storage_root)?;
    Ok((conn, path))
}

impl AppState {
    pub fn new(
        storage_path: Option<String>,
        config_path: Option<String>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let config_path = config_path.map(PathBuf::from);
        let storage_path_override = storage_path.as_ref().map(PathBuf::from);

        let (storage, config) = if let Some(path) = storage_path {
            let storage_root = PathBuf::from(&path);
            if !storage_root.exists() {
                return Err(format!("Storage path does not exist: {path}").into());
            }

            let config = config_path
                .as_ref()
                .and_then(|p| ReplayConfig::from_file(p).ok())
                .or_else(|| ReplayConfig::from_file(&storage_root.join("config/replay.cfg")).ok())
                .unwrap_or_else(|| ReplayConfig::parse("").unwrap());

            let kind = match config.storage_mode() {
                "usb" => StorageKind::Usb,
                "nvme" => StorageKind::Nvme,
                "nfs" => StorageKind::Nfs,
                _ => StorageKind::Sd,
            };

            (StorageLocation::from_path(storage_root, kind), config)
        } else {
            // Auto-detect: try to read config from default location
            let default_config = PathBuf::from("/media/sd/config/replay.cfg");
            let config = if default_config.exists() {
                ReplayConfig::from_file(&default_config)?
            } else {
                ReplayConfig::parse("")?
            };

            let storage = StorageLocation::detect(&config)?;
            (storage, config)
        };

        tracing::info!("Storage: {:?} at {}", storage.kind, storage.root.display());

        // Open DBs eagerly at startup so they're ready for the first request.
        // Fail-fast: if DB creation/open fails here, the service can't function.

        // Open the DB files eagerly to create schema + run migrations, then
        // drop the connections. The pool will create its own connections.
        let (_meta_conn, meta_path) =
            replay_control_core::metadata_db::MetadataDb::open(&storage.root)
                .map_err(|e| format!("Failed to open metadata DB: {e}"))?;
        tracing::info!("Metadata DB ready at {}", meta_path.display());
        let metadata_pool = DbPool::new(meta_path.clone(), "metadata_db", open_metadata_db)?;

        let (_ud_conn, ud_path, ud_corrupt) =
            replay_control_core::user_data_db::UserDataDb::open(&storage.root)
                .map_err(|e| format!("Failed to open user data DB: {e}"))?;
        tracing::info!("User data DB ready at {}", ud_path.display());
        let user_data_pool = DbPool::new(ud_path.clone(), "user_data_db", open_user_data_db)?;

        if ud_corrupt {
            // Mark the pool as corrupt so the banner shows immediately.
            // Don't backup — the file is damaged. The user can restore
            // from a previous backup or repair (fresh schema, loses data).
            tracing::warn!("User data DB is corrupt — marking pool, awaiting user action");
            user_data_pool.mark_corrupt();
        } else {
            // Back up user_data.db while it's known-healthy. This enables
            // restore-from-backup if corruption occurs at runtime.
            let backup_path = ud_path.with_extension("db.bak");
            match std::fs::copy(&ud_path, &backup_path) {
                Ok(_) => tracing::info!("User data backup saved to {}", backup_path.display()),
                Err(e) => tracing::debug!("Could not back up user_data.db: {e}"),
            }
        }

        let activity = Arc::new(std::sync::RwLock::new(Activity::Idle));

        let import = Arc::new(ImportPipeline::new());
        let thumbnails = Arc::new(ThumbnailPipeline::new());

        // Read skin preference from `.replay-control/settings.cfg` before
        // `storage` is moved into the Arc below.
        let initial_skin = replay_control_core::settings::read_skin(&storage.root);

        Ok(Self {
            storage: Arc::new(std::sync::RwLock::new(storage)),
            config: Arc::new(std::sync::RwLock::new(config)),
            config_path,
            cache: Arc::new(GameLibrary::new(metadata_pool.clone(), activity.clone())),
            storage_path_override,
            skin_override: Arc::new(std::sync::RwLock::new(initial_skin)),
            metadata_pool,
            user_data_pool,
            import,
            thumbnails,
            pending_downloads: Arc::new(std::sync::RwLock::new(std::collections::HashSet::new())),
            activity,
        })
    }

    /// Read-lock storage and clone the current StorageLocation.
    /// Panics only if the lock is poisoned (program bug).
    pub fn storage(&self) -> StorageLocation {
        self.storage.read().expect("storage lock poisoned").clone()
    }

    /// Check if either database has been flagged as corrupt.
    /// Returns `(metadata_corrupt, user_data_corrupt)`.
    pub fn is_db_corrupt(&self) -> (bool, bool) {
        (
            self.metadata_pool.is_corrupt(),
            self.user_data_pool.is_corrupt(),
        )
    }

    /// Get the user's region preference from `.replay-control/settings.cfg`.
    pub fn region_preference(&self) -> replay_control_core::rom_tags::RegionPreference {
        let storage = self.storage();
        replay_control_core::settings::read_region_preference(&storage.root)
    }

    /// Get the user's secondary (fallback) region preference from `.replay-control/settings.cfg`.
    /// Returns `None` if not set.
    pub fn region_preference_secondary(
        &self,
    ) -> Option<replay_control_core::rom_tags::RegionPreference> {
        let storage = self.storage();
        replay_control_core::settings::read_region_preference_secondary(&storage.root)
    }

    /// Get the effective skin index: app preference from `settings.cfg` if set,
    /// otherwise fall back to `replay.cfg`'s `system_skin` (sync mode).
    pub fn effective_skin(&self) -> u32 {
        if let Some(index) = *self.skin_override.read().expect("skin lock poisoned") {
            index
        } else {
            self.config
                .read()
                .expect("config lock poisoned")
                .system_skin()
        }
    }

    /// Update replay.cfg: apply the updater closure, then write back to disk.
    pub fn update_config<F>(&self, updater: F) -> Result<(), Box<dyn std::error::Error>>
    where
        F: FnOnce(&mut ReplayConfig),
    {
        let config_path = self.config_file_path();
        let mut config = self.config.write().expect("config lock poisoned");
        updater(&mut config);
        config.write_to_file(&config_path, &config_path)?;
        Ok(())
    }

    /// Re-detect storage from config (unless a CLI override was given).
    /// Returns `true` if the storage location actually changed.
    pub async fn refresh_storage(&self) -> Result<bool, Box<dyn std::error::Error>> {
        // Re-read config from disk so system-level settings (wifi, NFS,
        // system_skin for sync mode, etc.) are picked up on next SSR render.
        let config_path = self.config_file_path();
        let config = if config_path.exists() {
            ReplayConfig::from_file(&config_path)?
        } else {
            ReplayConfig::parse("")?
        };

        {
            let mut guard = self.config.write().expect("config lock poisoned");
            *guard = config.clone();
        }

        // Skip storage re-detection when an explicit path was given.
        if self.storage_path_override.is_some() {
            return Ok(false);
        }

        let new_storage = StorageLocation::detect(&config)?;

        let changed = {
            let current = self.storage.read().expect("storage lock poisoned");
            current.root != new_storage.root || current.kind != new_storage.kind
        };

        if changed {
            tracing::info!(
                "Storage changed: {:?} at {}",
                new_storage.kind,
                new_storage.root.display()
            );

            {
                let mut guard = self.storage.write().expect("storage lock poisoned");
                *guard = new_storage;
            }

            // Close old DB connections so they re-open at the new storage root.
            self.metadata_pool.close();
            self.user_data_pool.close();
            // Re-open at the new storage root.
            let new_storage_ref = self.storage();
            self.metadata_pool.reopen(&new_storage_ref.root);
            self.user_data_pool.reopen(&new_storage_ref.root);

            self.cache.invalidate().await;
        }

        Ok(changed)
    }

    /// Resolve the path to `replay.cfg` that `refresh_storage()` will read.
    pub(crate) fn config_file_path(&self) -> PathBuf {
        if let Some(ref p) = self.config_path {
            p.clone()
        } else if let Some(ref p) = self.storage_path_override {
            p.join("config/replay.cfg")
        } else {
            PathBuf::from("/media/sd/config/replay.cfg")
        }
    }
}

/// Build the application router with API routes, server function handler,
/// and SSR fallback. Extracted from main.rs so integration tests can reuse
/// the same router construction.
pub fn build_router(
    app_state: AppState,
    leptos_options: leptos::config::LeptosOptions,
) -> axum::Router {
    use axum::Router;
    use leptos::prelude::*;

    let api_routes = Router::new()
        .merge(system_info::routes())
        .merge(roms::routes())
        .merge(favorites::routes())
        .merge(upload::routes())
        .merge(recents::routes())
        .nest("/core", core_api::routes());

    let state_for_ssr = app_state.clone();
    let opts_for_ssr = leptos_options.clone();

    let ssr_handler = leptos_axum::render_app_to_stream_with_context(
        move || {
            provide_context(state_for_ssr.clone());
        },
        move || {
            let opts = opts_for_ssr.clone();
            view! { <crate::Shell options=opts /> }
        },
    );

    let state_for_sfn = app_state.clone();

    Router::new()
        .nest("/api", api_routes)
        .route(
            "/sfn/*fn_name",
            axum::routing::post(move |req: axum::http::Request<axum::body::Body>| {
                let state = state_for_sfn.clone();
                async move {
                    let ctx_state = state.clone();
                    leptos_axum::handle_server_fns_with_context(
                        move || provide_context(ctx_state.clone()),
                        req,
                    )
                    .await
                }
            }),
        )
        .route(
            "/style.css",
            axum::routing::get(|| async {
                (
                    [
                        ("content-type", "text/css"),
                        ("cache-control", "public, max-age=3600"),
                    ],
                    include_str!(concat!(env!("OUT_DIR"), "/style.css")),
                )
            }),
        )
        .fallback(ssr_handler)
        .with_state(app_state)
}
