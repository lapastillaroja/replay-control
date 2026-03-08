pub mod favorites;
pub mod recents;
pub mod roms;
pub mod system_info;
pub mod upload;

use std::path::PathBuf;
use std::sync::Arc;

use replay_core::config::ReplayConfig;
use replay_core::storage::{StorageKind, StorageLocation};

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub storage: Arc<StorageLocation>,
    pub config: Arc<std::sync::RwLock<ReplayConfig>>,
    pub config_path: Option<PathBuf>,
}

impl AppState {
    pub fn new(
        storage_path: Option<String>,
        config_path: Option<String>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let config_path = config_path.map(PathBuf::from);

        let (storage, config) = if let Some(path) = storage_path {
            let storage_root = PathBuf::from(&path);
            if !storage_root.exists() {
                return Err(format!("Storage path does not exist: {path}").into());
            }

            let config = config_path
                .as_ref()
                .and_then(|p| ReplayConfig::from_file(p).ok())
                .or_else(|| {
                    ReplayConfig::from_file(&storage_root.join("config/replay.cfg")).ok()
                })
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

        tracing::info!(
            "Storage: {:?} at {}",
            storage.kind,
            storage.root.display()
        );

        Ok(Self {
            storage: Arc::new(storage),
            config: Arc::new(std::sync::RwLock::new(config)),
            config_path,
        })
    }
}
