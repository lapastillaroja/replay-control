mod background;
mod cache;
mod import;
pub mod favorites;
pub mod recents;
pub mod roms;
pub mod system_info;
pub mod upload;

pub use cache::RomCache;

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
    pub cache: Arc<RomCache>,
    /// When set, --storage-path was given on the CLI and auto-detection is skipped.
    pub storage_path_override: Option<PathBuf>,
    /// When Some, the app uses this skin index instead of reading from replay.cfg.
    /// Set via the skin page when "Sync with ReplayOS" is disabled.
    pub skin_override: Arc<std::sync::RwLock<Option<u32>>>,
    /// Metadata DB handle (lazily opened on first access).
    pub(crate) metadata_db:
        Arc<std::sync::Mutex<Option<replay_control_core::metadata_db::MetadataDb>>>,
    /// Progress of the current metadata import (None = no import running).
    pub import_progress:
        Arc<std::sync::RwLock<Option<replay_control_core::metadata_db::ImportProgress>>>,
    /// Progress of the current image import (None = no import running).
    pub image_import_progress:
        Arc<std::sync::RwLock<Option<crate::server_fns::ImageImportProgress>>>,
    /// Set to `true` to request cancellation of the current image import.
    pub image_import_cancel: Arc<std::sync::atomic::AtomicBool>,
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

        Ok(Self {
            storage: Arc::new(std::sync::RwLock::new(storage)),
            config: Arc::new(std::sync::RwLock::new(config)),
            config_path,
            cache: Arc::new(RomCache::new()),
            storage_path_override,
            skin_override: Arc::new(std::sync::RwLock::new(None)),
            metadata_db: Arc::new(std::sync::Mutex::new(None)),
            import_progress: Arc::new(std::sync::RwLock::new(None)),
            image_import_progress: Arc::new(std::sync::RwLock::new(None)),
            image_import_cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        })
    }

    /// Read-lock storage and clone the current StorageLocation.
    /// Panics only if the lock is poisoned (program bug).
    pub fn storage(&self) -> StorageLocation {
        self.storage.read().expect("storage lock poisoned").clone()
    }

    /// Get a lock on the metadata DB, lazily opening it on first access.
    /// Returns None if the DB can't be opened (e.g., storage not available).
    pub fn metadata_db(
        &self,
    ) -> Option<std::sync::MutexGuard<'_, Option<replay_control_core::metadata_db::MetadataDb>>>
    {
        let mut guard = self.metadata_db.lock().expect("metadata_db lock poisoned");
        if guard.is_none() {
            let storage = self.storage();
            match replay_control_core::metadata_db::MetadataDb::open(&storage.root) {
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
}
