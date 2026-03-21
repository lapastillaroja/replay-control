pub(crate) mod background;
pub(crate) mod cache;
pub(crate) mod core_api;
pub mod favorites;
pub mod import;
pub mod recents;
pub mod roms;
pub mod system_info;
pub mod upload;

pub use background::BackgroundManager;
pub use cache::GameLibrary;
pub use import::{ImportPipeline, ThumbnailPipeline};

use std::path::PathBuf;
use std::sync::Arc;

use deadpool_sqlite::rusqlite;
use replay_control_core::config::ReplayConfig;
use replay_control_core::storage::{StorageKind, StorageLocation};

// ── Custom deadpool Manager ───────────────────────────────────────

use deadpool::managed::{self, Metrics, RecycleError};
use deadpool_sync::SyncWrapper;

/// Custom deadpool Manager that uses `db_common::open_connection()` for
/// proper WAL/nolock/PRAGMA configuration instead of plain `Connection::open()`.
struct SqliteManager {
    db_path: PathBuf,
    is_local: bool,
    label: String,
}

impl std::fmt::Debug for SqliteManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteManager")
            .field("db_path", &self.db_path)
            .field("is_local", &self.is_local)
            .field("label", &self.label)
            .finish()
    }
}

impl managed::Manager for SqliteManager {
    type Type = SyncWrapper<rusqlite::Connection>;
    type Error = rusqlite::Error;

    async fn create(&self) -> Result<SyncWrapper<rusqlite::Connection>, Self::Error> {
        let db_path = self.db_path.clone();
        let is_local = self.is_local;
        let label = self.label.clone();

        SyncWrapper::new(deadpool_sqlite::Runtime::Tokio1, move || {
            replay_control_core::db_common::open_connection(&db_path, &label, is_local)
                .map_err(|e| {
                    rusqlite::Error::SqliteFailure(
                        rusqlite::ffi::Error::new(1),
                        Some(e.to_string()),
                    )
                })
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
/// - **Read pool**: `max_size=3` for local storage (concurrent WAL reads), `1` for NFS
/// - **Write pool**: `max_size=1` (SQLite serialises writes)
///
/// Provides synchronous `read()` / `write()` helpers. Under the hood, each call
/// acquires a connection from the pool, runs the closure via `SyncWrapper::lock()`,
/// and returns the connection to the pool.
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
    opener: fn(&std::path::Path, bool) -> replay_control_core::error::Result<(rusqlite::Connection, PathBuf)>,
}

/// Build a deadpool `SqlitePool` with the given size.
fn build_pool(
    db_path: &std::path::Path,
    is_local: bool,
    label: &str,
    max_size: usize,
) -> Result<SqlitePool, Box<dyn std::error::Error>> {
    let mgr = SqliteManager {
        db_path: db_path.to_path_buf(),
        is_local,
        label: label.to_string(),
    };
    let pool = managed::Pool::builder(mgr)
        .max_size(max_size)
        .build()
        .map_err(|e| format!("{label}: failed to build pool: {e}"))?;
    Ok(pool)
}

impl DbPool {
    /// Create a new pool. Opens the DB eagerly (via `opener`) to fail fast at
    /// startup, then builds read and write pools backed by the custom manager.
    fn new(
        db_path: PathBuf,
        is_local: bool,
        label: &'static str,
        opener: fn(&std::path::Path, bool) -> replay_control_core::error::Result<(rusqlite::Connection, PathBuf)>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let read_size = if is_local { 3 } else { 1 };
        let read_pool = build_pool(&db_path, is_local, &format!("{label}_read"), read_size)?;
        let write_pool = build_pool(&db_path, is_local, &format!("{label}_write"), 1)?;

        Ok(Self {
            read_pool: Arc::new(std::sync::RwLock::new(Some(read_pool))),
            write_pool: Arc::new(std::sync::RwLock::new(Some(write_pool))),
            db_path: Arc::new(std::sync::RwLock::new(db_path)),
            label,
            opener,
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
            opener: |_, _| Err(replay_control_core::error::Error::Other("test".into())),
        }
    }

    /// Run a read-only closure with a database connection from the read pool.
    ///
    /// Multiple concurrent `read()` calls get different connections (up to
    /// `max_size`), enabling true concurrent reads under WAL mode.
    ///
    /// Returns `None` if the pool is closed (DB unavailable).
    pub fn read<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&rusqlite::Connection) -> R,
    {
        let pool_guard = self.read_pool.read().ok()?;
        let pool = pool_guard.as_ref()?;
        let obj = Self::blocking_get(pool)?;
        drop(pool_guard); // release RwLock before running the closure
        let guard = obj.lock().ok()?;
        Some(f(&guard))
    }

    /// Run a mutable closure with the single write connection.
    ///
    /// Returns `None` if the pool is closed (DB unavailable).
    pub fn write<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&mut rusqlite::Connection) -> R,
    {
        let pool_guard = self.write_pool.read().ok()?;
        let pool = pool_guard.as_ref()?;
        let obj = Self::blocking_get(pool)?;
        drop(pool_guard);
        let mut guard = obj.lock().ok()?;
        Some(f(&mut guard))
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
    pub(crate) fn reopen(&self, storage_root: &std::path::Path, is_local: bool) -> bool {
        // Verify we can open the DB at the new location.
        match (self.opener)(storage_root, is_local) {
            Ok((_conn, path)) => {
                let read_size = if is_local { 3 } else { 1 };
                let new_read = match build_pool(&path, is_local, &format!("{}_read", self.label), read_size) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::debug!("Could not rebuild {} read pool: {e}", self.label);
                        return false;
                    }
                };
                let new_write = match build_pool(&path, is_local, &format!("{}_write", self.label), 1) {
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
                true
            }
            Err(e) => {
                tracing::debug!("Could not re-open {} DB: {e}", self.label);
                false
            }
        }
    }

    /// Get the current DB file path.
    pub fn db_path(&self) -> PathBuf {
        self.db_path.read().expect("db_path lock poisoned").clone()
    }

    /// Check if the DB file still exists on disk.
    pub fn db_file_exists(&self) -> bool {
        self.db_path.read().expect("db_path lock poisoned").exists()
    }

    /// Synchronously get a connection from a deadpool pool.
    ///
    /// Uses `block_in_place` + `block_on` which works from tokio multi-thread
    /// worker threads and `spawn_blocking` threads. The production runtime is
    /// always multi-thread (`#[tokio::main]`).
    ///
    /// **Note**: Panics on `current_thread` runtime (single-thread `#[tokio::test]`).
    /// Integration tests must use `#[tokio::test(flavor = "multi_thread")]`.
    fn blocking_get(
        pool: &SqlitePool,
    ) -> Option<managed::Object<SqliteManager>> {
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
    /// Unified busy flag: true when any background operation is running.
    busy: Arc<std::sync::atomic::AtomicBool>,
    /// Human-readable label for the current background operation (empty = idle).
    pub(crate) busy_label: Arc<std::sync::RwLock<String>>,
    /// Scanning indicator: true only during Phase 2 (game library populate).
    scanning: Arc<std::sync::atomic::AtomicBool>,
}

/// Opener for metadata DB.
fn open_metadata_db(
    storage_root: &std::path::Path,
    is_local: bool,
) -> replay_control_core::error::Result<(rusqlite::Connection, PathBuf)> {
    replay_control_core::metadata_db::MetadataDb::open(storage_root, is_local)
}

/// Opener for user data DB.
fn open_user_data_db(
    storage_root: &std::path::Path,
    is_local: bool,
) -> replay_control_core::error::Result<(rusqlite::Connection, PathBuf)> {
    replay_control_core::user_data_db::UserDataDb::open(storage_root, is_local)
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
        let is_local = storage.kind.is_local();

        // Open the DB files eagerly to create schema + run migrations, then
        // drop the connections. The pool will create its own connections.
        let (_meta_conn, meta_path) =
            replay_control_core::metadata_db::MetadataDb::open(&storage.root, is_local)
                .map_err(|e| format!("Failed to open metadata DB: {e}"))?;
        tracing::info!("Metadata DB ready at {}", meta_path.display());
        let metadata_pool = DbPool::new(meta_path.clone(), is_local, "metadata_db", open_metadata_db)?;

        let (_ud_conn, ud_path) =
            replay_control_core::user_data_db::UserDataDb::open(&storage.root, is_local)
                .map_err(|e| format!("Failed to open user data DB: {e}"))?;
        tracing::info!("User data DB ready at {}", ud_path.display());
        let user_data_pool = DbPool::new(ud_path.clone(), is_local, "user_data_db", open_user_data_db)?;

        // Unified busy flag shared across all background operations.
        let busy = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let scanning = Arc::new(std::sync::atomic::AtomicBool::new(false));

        let import = Arc::new(ImportPipeline::new(busy.clone()));
        let thumbnails = Arc::new(ThumbnailPipeline::new(busy.clone()));

        // Read skin preference from `.replay-control/settings.cfg` before
        // `storage` is moved into the Arc below.
        let initial_skin = replay_control_core::settings::read_skin(&storage.root);

        Ok(Self {
            storage: Arc::new(std::sync::RwLock::new(storage)),
            config: Arc::new(std::sync::RwLock::new(config)),
            config_path,
            cache: Arc::new(GameLibrary::new(
                metadata_pool.clone(),
                busy.clone(),
                scanning.clone(),
            )),
            storage_path_override,
            skin_override: Arc::new(std::sync::RwLock::new(initial_skin)),
            metadata_pool,
            user_data_pool,
            import,
            thumbnails,
            pending_downloads: Arc::new(std::sync::RwLock::new(std::collections::HashSet::new())),
            busy,
            busy_label: Arc::new(std::sync::RwLock::new(String::new())),
            scanning,
        })
    }

    /// Read-lock storage and clone the current StorageLocation.
    /// Panics only if the lock is poisoned (program bug).
    pub fn storage(&self) -> StorageLocation {
        self.storage.read().expect("storage lock poisoned").clone()
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
    pub fn refresh_storage(&self) -> Result<bool, Box<dyn std::error::Error>> {
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
            let new_is_local = new_storage_ref.kind.is_local();
            self.metadata_pool.reopen(&new_storage_ref.root, new_is_local);
            self.user_data_pool.reopen(&new_storage_ref.root, new_is_local);

            self.cache.invalidate();
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

    /// Check if any background operation is running.
    pub fn is_busy(&self) -> bool {
        self.busy.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Atomically claim the busy slot. Returns true if successfully claimed.
    pub fn claim_busy(&self) -> bool {
        !self.busy.swap(true, std::sync::atomic::Ordering::SeqCst)
    }

    /// Get a clone of the busy flag Arc (for passing to background tasks).
    pub fn busy_flag(&self) -> Arc<std::sync::atomic::AtomicBool> {
        self.busy.clone()
    }

    /// Check if the game library scan (Phase 2) is in progress.
    pub fn is_scanning(&self) -> bool {
        self.scanning.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Set the human-readable label for the current background operation.
    pub fn set_busy_label(&self, label: &str) {
        *self.busy_label.write().expect("busy_label lock") = label.to_string();
    }

    /// Get the current busy label (empty if idle).
    pub fn get_busy_label(&self) -> String {
        self.busy_label.read().expect("busy_label lock").clone()
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
                    [("content-type", "text/css")],
                    include_str!(concat!(env!("OUT_DIR"), "/style.css")),
                )
            }),
        )
        .fallback(ssr_handler)
        .with_state(app_state)
}
