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

use replay_control_core::config::ReplayConfig;
use replay_control_core::storage::{StorageKind, StorageLocation};

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub storage: Arc<std::sync::RwLock<StorageLocation>>,
    pub config: Arc<std::sync::RwLock<ReplayConfig>>,
    pub config_path: Option<PathBuf>,
    pub cache: Arc<GameLibrary>,
    /// When set, --storage-path was given on the CLI and auto-detection is skipped.
    pub storage_path_override: Option<PathBuf>,
    /// When Some, the app uses this skin index instead of reading from replay.cfg.
    /// Set via the skin page when "Sync with ReplayOS" is disabled.
    pub skin_override: Arc<std::sync::RwLock<Option<u32>>>,
    /// Metadata DB handle (lazily opened on first access).
    pub(crate) metadata_db:
        Arc<std::sync::Mutex<Option<replay_control_core::metadata_db::MetadataDb>>>,
    /// User data DB handle (lazily opened on first access).
    pub(crate) user_data_db:
        Arc<std::sync::Mutex<Option<replay_control_core::user_data_db::UserDataDb>>>,
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
        let metadata_db =
            replay_control_core::metadata_db::MetadataDb::open(&storage.root, is_local)
                .map_err(|e| format!("Failed to open metadata DB: {e}"))?;
        tracing::info!("Metadata DB ready at {}", metadata_db.db_path().display());
        let metadata_db = Arc::new(std::sync::Mutex::new(Some(metadata_db)));

        let user_data_db =
            replay_control_core::user_data_db::UserDataDb::open(&storage.root, is_local)
                .map_err(|e| format!("Failed to open user data DB: {e}"))?;
        tracing::info!("User data DB ready at {}", user_data_db.db_path().display());
        let user_data_db = Arc::new(std::sync::Mutex::new(Some(user_data_db)));

        // Unified busy flag shared across all background operations.
        let busy = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let scanning = Arc::new(std::sync::atomic::AtomicBool::new(false));

        let import = Arc::new(ImportPipeline::new(busy.clone()));
        let thumbnails = Arc::new(ThumbnailPipeline::new(busy.clone()));

        Ok(Self {
            storage: Arc::new(std::sync::RwLock::new(storage)),
            config: Arc::new(std::sync::RwLock::new(config)),
            config_path,
            cache: Arc::new(GameLibrary::new(
                metadata_db.clone(),
                busy.clone(),
                scanning.clone(),
            )),
            storage_path_override,
            skin_override: Arc::new(std::sync::RwLock::new(None)),
            metadata_db,
            user_data_db,
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

    /// Get a lock on the metadata DB, lazily opening it on first access.
    /// Returns None if the DB can't be opened (e.g., storage not available).
    /// Re-opens automatically if the DB file was deleted externally.
    pub fn metadata_db(
        &self,
    ) -> Option<std::sync::MutexGuard<'_, Option<replay_control_core::metadata_db::MetadataDb>>>
    {
        let mut guard = self.metadata_db.lock().expect("metadata_db lock poisoned");
        // Drop stale connection if the DB file was deleted externally.
        if let Some(ref db) = *guard
            && !db.db_path().exists()
        {
            tracing::warn!("Metadata DB file deleted externally, re-opening");
            *guard = None;
        }
        if guard.is_none() {
            let storage = self.storage();
            match replay_control_core::metadata_db::MetadataDb::open(
                &storage.root,
                storage.kind.is_local(),
            ) {
                Ok(db) => {
                    *guard = Some(db);
                }
                Err(e) => {
                    tracing::debug!("Could not open metadata DB: {e}");
                    return None;
                }
            }
        }
        Some(guard)
    }

    /// Get a lock on the user data DB, lazily opening it on first access.
    /// Returns None if the DB can't be opened.
    /// Re-opens automatically if the DB file was deleted externally.
    pub fn user_data_db(
        &self,
    ) -> Option<std::sync::MutexGuard<'_, Option<replay_control_core::user_data_db::UserDataDb>>>
    {
        let mut guard = self
            .user_data_db
            .lock()
            .expect("user_data_db lock poisoned");
        // Drop stale connection if the DB file was deleted externally.
        if let Some(ref db) = *guard
            && !db.db_path().exists()
        {
            tracing::warn!("User data DB file deleted externally, re-opening");
            *guard = None;
        }
        if guard.is_none() {
            let storage = self.storage();
            match replay_control_core::user_data_db::UserDataDb::open(
                &storage.root,
                storage.kind.is_local(),
            ) {
                Ok(db) => {
                    *guard = Some(db);
                }
                Err(e) => {
                    tracing::debug!("Could not open user_data DB: {e}");
                    return None;
                }
            }
        }
        Some(guard)
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

    /// Get the effective skin index: override if set, otherwise from replay.cfg.
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
        // Re-read config from disk so non-storage settings (system_skin,
        // wifi, etc.) are picked up on next SSR render.
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

            // Close old DB connections so they re-open at the new storage root
            // on next access.  The cache's `db` field is the same Arc, so
            // setting metadata_db to None also affects cache.with_db().
            {
                let mut guard = self.metadata_db.lock().expect("metadata_db lock poisoned");
                *guard = None;
            }
            {
                let mut guard = self
                    .user_data_db
                    .lock()
                    .expect("user_data_db lock poisoned");
                *guard = None;
            }

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
