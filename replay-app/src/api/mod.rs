pub mod favorites;
pub mod recents;
pub mod roms;
pub mod system_info;
pub mod upload;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use replay_core::config::ReplayConfig;
use replay_core::roms::{RomEntry, SystemSummary};
use replay_core::storage::{StorageKind, StorageLocation};

/// Cache TTL — filesystem scans are reused for this duration.
const CACHE_TTL: Duration = Duration::from_secs(30);

/// Cached result with expiry timestamp.
struct CacheEntry<T> {
    data: T,
    expires: Instant,
}

impl<T: Clone> CacheEntry<T> {
    fn new(data: T) -> Self {
        Self {
            data,
            expires: Instant::now() + CACHE_TTL,
        }
    }

    fn get(&self) -> Option<&T> {
        if Instant::now() < self.expires {
            Some(&self.data)
        } else {
            None
        }
    }
}

/// In-memory cache for filesystem scan results.
/// Avoids repeated NFS/disk traversals on every request.
pub struct RomCache {
    systems: std::sync::RwLock<Option<CacheEntry<Vec<SystemSummary>>>>,
    roms: std::sync::RwLock<HashMap<String, CacheEntry<Vec<RomEntry>>>>,
}

impl RomCache {
    fn new() -> Self {
        Self {
            systems: std::sync::RwLock::new(None),
            roms: std::sync::RwLock::new(HashMap::new()),
        }
    }

    /// Get cached systems or scan and cache.
    pub fn get_systems(&self, storage: &StorageLocation) -> Vec<SystemSummary> {
        // Try read lock first.
        if let Ok(guard) = self.systems.read() {
            if let Some(ref entry) = *guard {
                if let Some(data) = entry.get() {
                    return data.clone();
                }
            }
        }
        // Cache miss — scan and store.
        let summaries = replay_core::roms::scan_systems(storage);
        if let Ok(mut guard) = self.systems.write() {
            *guard = Some(CacheEntry::new(summaries.clone()));
        }
        summaries
    }

    /// Get cached ROM list for a system, or scan and cache.
    pub fn get_roms(
        &self,
        storage: &StorageLocation,
        system: &str,
    ) -> Result<Vec<RomEntry>, replay_core::error::Error> {
        let key = system.to_string();
        // Try read lock first.
        if let Ok(guard) = self.roms.read() {
            if let Some(entry) = guard.get(&key) {
                if let Some(data) = entry.get() {
                    return Ok(data.clone());
                }
            }
        }
        // Cache miss — scan and store.
        let roms = replay_core::roms::list_roms(storage, system)?;
        if let Ok(mut guard) = self.roms.write() {
            guard.insert(key, CacheEntry::new(roms.clone()));
        }
        Ok(roms)
    }

    /// Invalidate all caches (after delete, rename, upload).
    pub fn invalidate(&self) {
        if let Ok(mut guard) = self.systems.write() {
            *guard = None;
        }
        if let Ok(mut guard) = self.roms.write() {
            guard.clear();
        }
    }

    /// Invalidate cache for a specific system.
    pub fn invalidate_system(&self, system: &str) {
        if let Ok(mut guard) = self.systems.write() {
            *guard = None;
        }
        if let Ok(mut guard) = self.roms.write() {
            guard.remove(system);
        }
    }
}

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub storage: Arc<StorageLocation>,
    pub config: Arc<std::sync::RwLock<ReplayConfig>>,
    pub config_path: Option<PathBuf>,
    pub cache: Arc<RomCache>,
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
            cache: Arc::new(RomCache::new()),
        })
    }
}
