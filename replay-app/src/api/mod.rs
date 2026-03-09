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

/// How often the background task re-checks storage (in seconds).
const STORAGE_CHECK_INTERVAL: u64 = 60;

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
    pub storage: Arc<std::sync::RwLock<StorageLocation>>,
    pub config: Arc<std::sync::RwLock<ReplayConfig>>,
    pub config_path: Option<PathBuf>,
    pub cache: Arc<RomCache>,
    /// When set, --storage-path was given on the CLI and auto-detection is skipped.
    pub storage_path_override: Option<PathBuf>,
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
            storage: Arc::new(std::sync::RwLock::new(storage)),
            config: Arc::new(std::sync::RwLock::new(config)),
            config_path,
            cache: Arc::new(RomCache::new()),
            storage_path_override,
        })
    }

    /// Read-lock storage and clone the current StorageLocation.
    /// Panics only if the lock is poisoned (program bug).
    pub fn storage(&self) -> StorageLocation {
        self.storage.read().expect("storage lock poisoned").clone()
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
        if self.storage_path_override.is_some() {
            tracing::debug!("Storage path override is set, skipping re-detection");
            return Ok(false);
        }

        // Re-read config from disk
        let default_config = PathBuf::from("/media/sd/config/replay.cfg");
        let config = if let Some(ref p) = self.config_path {
            ReplayConfig::from_file(p)?
        } else if default_config.exists() {
            ReplayConfig::from_file(&default_config)?
        } else {
            ReplayConfig::parse("")?
        };

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
            {
                let mut guard = self.config.write().expect("config lock poisoned");
                *guard = config;
            }
            self.cache.invalidate();
        }

        Ok(changed)
    }

    /// Resolve the path to `replay.cfg` that `refresh_storage()` will read.
    fn config_file_path(&self) -> PathBuf {
        if let Some(ref p) = self.config_path {
            p.clone()
        } else if let Some(ref p) = self.storage_path_override {
            p.join("config/replay.cfg")
        } else {
            PathBuf::from("/media/sd/config/replay.cfg")
        }
    }

    /// Spawn a background task that watches `replay.cfg` for changes and
    /// periodically re-checks storage as a fallback.
    ///
    /// Uses `notify` (inotify on Linux) to react immediately when the config
    /// file is modified. Falls back to the 60-second poll if filesystem
    /// watching cannot be set up (e.g., on NFS).
    pub fn spawn_storage_watcher(self) {
        let config_path = self.config_file_path();
        let state = self.clone();

        // Spawn the filesystem watcher in a blocking thread (notify uses
        // its own event loop that blocks the thread).
        let watcher_state = self.clone();
        let watcher_config_path = config_path.clone();

        tokio::spawn(async move {
            let watcher_active = Self::try_start_config_watcher(
                watcher_state,
                watcher_config_path,
            )
            .await;

            if watcher_active {
                tracing::info!("Config file watcher active; 60s poll runs as fallback");
            } else {
                tracing::info!("Config file watcher unavailable; using 60s poll only");
            }

            // The 60-second poll always runs as a fallback.
            let mut interval =
                tokio::time::interval(std::time::Duration::from_secs(STORAGE_CHECK_INTERVAL));
            // Skip the first (immediate) tick — we just initialized.
            interval.tick().await;
            loop {
                interval.tick().await;
                match state.refresh_storage() {
                    Ok(true) => tracing::info!("Background storage re-detection: storage changed"),
                    Ok(false) => {}
                    Err(e) => tracing::warn!("Background storage re-detection failed: {e}"),
                }
            }
        });
    }

    /// Try to set up a `notify` filesystem watcher on the config file.
    /// Returns `true` if the watcher was started successfully.
    async fn try_start_config_watcher(
        state: AppState,
        config_path: PathBuf,
    ) -> bool {
        use notify::{recommended_watcher, RecursiveMode, Watcher};

        // Watch the parent directory — the file itself may not exist yet, and
        // some editors write to a temp file then rename, which only shows up as
        // an event on the directory.
        let watch_dir = match config_path.parent() {
            Some(dir) if dir.exists() => dir.to_path_buf(),
            Some(dir) => {
                tracing::warn!(
                    "Config directory does not exist ({}), cannot set up file watcher",
                    dir.display()
                );
                return false;
            }
            None => {
                tracing::warn!("Cannot determine parent directory of config path");
                return false;
            }
        };

        let config_filename = config_path
            .file_name()
            .map(|n| n.to_os_string())
            .unwrap_or_default();

        let (tx, mut rx) = tokio::sync::mpsc::channel(16);

        // Create the watcher. The callback sends events through the channel
        // so we can process them on the async side.
        let mut watcher = match recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
            match res {
                Ok(event) => {
                    let _ = tx.blocking_send(event);
                }
                Err(e) => {
                    tracing::warn!("File watcher error: {e}");
                }
            }
        }) {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!("Failed to create file watcher: {e}");
                return false;
            }
        };

        if let Err(e) = watcher.watch(&watch_dir, RecursiveMode::NonRecursive) {
            tracing::warn!(
                "Failed to watch directory {}: {e}",
                watch_dir.display()
            );
            return false;
        }

        tracing::info!(
            "Watching {} for config changes",
            watch_dir.display()
        );

        // Spawn the event-processing loop. We keep `watcher` alive by moving
        // it into this task — dropping it would stop watching.
        tokio::spawn(async move {
            let _watcher = watcher; // prevent drop

            // Debounce: after the first relevant event, wait before refreshing
            // so that rapid successive writes (common with text editors) only
            // trigger a single refresh.
            const DEBOUNCE: Duration = Duration::from_secs(2);

            loop {
                // Wait for the next event.
                let Some(event) = rx.recv().await else {
                    tracing::warn!("Config file watcher channel closed");
                    break;
                };

                if !Self::is_config_event(&event, &config_filename) {
                    continue;
                }

                tracing::debug!("Config change detected ({:?}), debouncing...", event.kind);

                // Drain any further events that arrive within the debounce window.
                let deadline = tokio::time::Instant::now() + DEBOUNCE;
                loop {
                    match tokio::time::timeout_at(deadline, rx.recv()).await {
                        Ok(Some(ev)) => {
                            if Self::is_config_event(&ev, &config_filename) {
                                tracing::debug!("Additional config event during debounce ({:?})", ev.kind);
                            }
                        }
                        Ok(None) => {
                            // Channel closed
                            break;
                        }
                        Err(_) => {
                            // Timeout — debounce window expired
                            break;
                        }
                    }
                }

                tracing::info!("Config file changed, refreshing storage");
                match state.refresh_storage() {
                    Ok(true) => tracing::info!("Storage updated after config change"),
                    Ok(false) => tracing::debug!("Config changed but storage unchanged"),
                    Err(e) => tracing::warn!("Failed to refresh storage after config change: {e}"),
                }
            }
        });

        true
    }

    /// Check whether a notify event is relevant to our config file.
    fn is_config_event(event: &notify::Event, config_filename: &std::ffi::OsStr) -> bool {
        use notify::EventKind;

        // Only react to creates, modifications, and renames (some editors
        // write a temp file then rename it over the original).
        matches!(
            event.kind,
            EventKind::Create(_) | EventKind::Modify(_)
        ) && event
            .paths
            .iter()
            .any(|p| p.file_name().is_some_and(|n| n == config_filename))
    }
}
