pub mod favorites;
pub mod recents;
pub mod roms;
pub mod system_info;
pub mod upload;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use replay_control_core::config::ReplayConfig;
use replay_control_core::roms::{RomEntry, SystemSummary};
use replay_control_core::storage::{StorageKind, StorageLocation};

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
        let summaries = replay_control_core::roms::scan_systems(storage);
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
    ) -> Result<Vec<RomEntry>, replay_control_core::error::Error> {
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
        let roms = replay_control_core::roms::list_roms(storage, system)?;
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
    /// When Some, the app uses this skin index instead of reading from replay.cfg.
    /// Set via the skin page when "Sync with ReplayOS" is disabled.
    pub skin_override: Arc<std::sync::RwLock<Option<u32>>>,
    /// Metadata DB handle (lazily opened on first access).
    metadata_db: Arc<std::sync::Mutex<Option<replay_control_core::metadata_db::MetadataDb>>>,
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
    fn config_file_path(&self) -> PathBuf {
        if let Some(ref p) = self.config_path {
            p.clone()
        } else if let Some(ref p) = self.storage_path_override {
            p.join("config/replay.cfg")
        } else {
            PathBuf::from("/media/sd/config/replay.cfg")
        }
    }

    /// Start a background metadata import from a LaunchBox XML file.
    /// Returns `false` if an import is already running.
    pub fn start_import(&self, xml_path: PathBuf) -> bool {
        use replay_control_core::metadata_db::{ImportProgress, ImportState};

        // Check if already running.
        {
            let guard = self
                .import_progress
                .read()
                .expect("import_progress lock poisoned");
            if let Some(ref p) = *guard {
                if matches!(
                    p.state,
                    ImportState::Downloading | ImportState::BuildingIndex | ImportState::Parsing
                ) {
                    return false;
                }
            }
        }

        // Set initial progress.
        {
            let mut guard = self
                .import_progress
                .write()
                .expect("import_progress lock poisoned");
            *guard = Some(ImportProgress {
                state: ImportState::BuildingIndex,
                processed: 0,
                matched: 0,
                inserted: 0,
                elapsed_secs: 0,
                error: None,
            });
        }

        let state = self.clone();
        tokio::task::spawn_blocking(move || {
            let start = std::time::Instant::now();
            state.run_import_blocking(xml_path, start);
        });

        true
    }

    /// Auto-import metadata on startup if Metadata.xml exists and DB is empty.
    pub fn spawn_auto_import(&self) {
        use replay_control_core::metadata_db::RC_DIR;

        let storage_root = self.storage().root.clone();
        let xml_path = storage_root.join(RC_DIR).join("Metadata.xml");

        if !xml_path.exists() {
            tracing::debug!(
                "No Metadata.xml at {}, skipping auto-import",
                xml_path.display()
            );
            return;
        }

        let should_import = if let Some(guard) = self.metadata_db() {
            guard
                .as_ref()
                .and_then(|db| db.is_empty().ok())
                .unwrap_or(false)
        } else {
            false
        };

        if should_import {
            tracing::info!("Auto-importing metadata from {}", xml_path.display());
            self.start_import(xml_path);
        }
    }

    /// Clear metadata DB and re-import from Metadata.xml if present.
    /// Returns an error message if Metadata.xml is not found.
    pub fn regenerate_metadata(&self) -> Result<(), String> {
        use replay_control_core::metadata_db::RC_DIR;

        // Clear existing metadata.
        if let Some(guard) = self.metadata_db() {
            if let Some(db) = guard.as_ref() {
                db.clear().map_err(|e| e.to_string())?;
            }
        }

        // Find Metadata.xml and trigger re-import.
        let storage_root = self.storage().root.clone();
        let xml_path = storage_root.join(RC_DIR).join("Metadata.xml");
        if !xml_path.exists() {
            return Err("No Metadata.xml found. Place it in the .replay-control folder to enable re-import.".to_string());
        }

        self.start_import(xml_path);
        Ok(())
    }

    /// Download LaunchBox Metadata.zip, extract, clear DB, and re-import.
    /// Runs entirely in a background thread. Returns false if an import is
    /// already running.
    pub fn start_metadata_download(&self) -> bool {
        use replay_control_core::metadata_db::{ImportProgress, ImportState, RC_DIR};

        // Check if already running.
        {
            let guard = self
                .import_progress
                .read()
                .expect("import_progress lock poisoned");
            if let Some(ref p) = *guard {
                if matches!(
                    p.state,
                    ImportState::Downloading | ImportState::BuildingIndex | ImportState::Parsing
                ) {
                    return false;
                }
            }
        }

        // Set initial progress to Downloading.
        {
            let mut guard = self
                .import_progress
                .write()
                .expect("import_progress lock poisoned");
            *guard = Some(ImportProgress {
                state: ImportState::Downloading,
                processed: 0,
                matched: 0,
                inserted: 0,
                elapsed_secs: 0,
                error: None,
            });
        }

        let state = self.clone();
        tokio::task::spawn_blocking(move || {
            let start = std::time::Instant::now();
            let storage_root = state.storage().root.clone();
            let rc_dir = storage_root.join(RC_DIR);

            // Download and extract.
            let xml_path = match replay_control_core::launchbox::download_metadata(&rc_dir) {
                Ok(path) => path,
                Err(e) => {
                    let mut guard = state
                        .import_progress
                        .write()
                        .expect("import_progress lock poisoned");
                    if let Some(ref mut p) = *guard {
                        p.state = ImportState::Failed;
                        p.error = Some(format!("Download failed: {e}"));
                        p.elapsed_secs = start.elapsed().as_secs();
                    }
                    return;
                }
            };

            // Clear existing metadata before re-import.
            if let Some(guard) = state.metadata_db() {
                if let Some(db) = guard.as_ref() {
                    if let Err(e) = db.clear() {
                        tracing::warn!("Failed to clear metadata DB before re-import: {e}");
                    }
                }
            }

            // Update elapsed before starting import.
            {
                let mut guard = state
                    .import_progress
                    .write()
                    .expect("import_progress lock poisoned");
                if let Some(ref mut p) = *guard {
                    p.elapsed_secs = start.elapsed().as_secs();
                }
            }

            // Now run the import (this updates import_progress internally).
            state.run_import_blocking(xml_path, start);
        });

        true
    }

    /// Run the metadata import synchronously (called from spawn_blocking).
    /// Separated from start_import to allow reuse from start_metadata_download.
    fn run_import_blocking(&self, xml_path: PathBuf, start: std::time::Instant) {
        use replay_control_core::metadata_db::{ImportProgress, ImportState};

        // Build ROM index.
        let storage_root = self.storage().root.clone();
        {
            let mut guard = self
                .import_progress
                .write()
                .expect("import_progress lock poisoned");
            if let Some(ref mut p) = *guard {
                p.state = ImportState::BuildingIndex;
                p.elapsed_secs = start.elapsed().as_secs();
            }
        }

        let rom_index = replay_control_core::launchbox::build_rom_index(&storage_root);

        // Update progress to Parsing.
        {
            let mut guard = self
                .import_progress
                .write()
                .expect("import_progress lock poisoned");
            if let Some(ref mut p) = *guard {
                p.state = ImportState::Parsing;
                p.elapsed_secs = start.elapsed().as_secs();
            }
        }

        // Take DB from state.
        let db = {
            let mut guard = self.metadata_db.lock().expect("metadata_db lock poisoned");
            guard.take()
        };
        let mut db = match db {
            Some(db) => db,
            None => match replay_control_core::metadata_db::MetadataDb::open(&storage_root) {
                Ok(db) => db,
                Err(e) => {
                    let mut guard = self
                        .import_progress
                        .write()
                        .expect("import_progress lock poisoned");
                    if let Some(ref mut p) = *guard {
                        p.state = ImportState::Failed;
                        p.error = Some(format!("Cannot open metadata DB: {e}"));
                        p.elapsed_secs = start.elapsed().as_secs();
                    }
                    return;
                }
            },
        };

        let progress_ref = self.import_progress.clone();
        let start_ref = start;
        let result = replay_control_core::launchbox::import_launchbox(
            &xml_path,
            &mut db,
            &rom_index,
            |processed, matched, inserted| {
                let mut guard = progress_ref.write().expect("import_progress lock poisoned");
                if let Some(ref mut p) = *guard {
                    p.processed = processed;
                    p.matched = matched;
                    p.inserted = inserted;
                    p.elapsed_secs = start_ref.elapsed().as_secs();
                }
            },
        );

        // Put DB back.
        {
            let mut guard = self.metadata_db.lock().expect("metadata_db lock poisoned");
            *guard = Some(db);
        }

        // Update final progress.
        {
            let mut guard = self
                .import_progress
                .write()
                .expect("import_progress lock poisoned");
            match result {
                Ok(stats) => {
                    *guard = Some(ImportProgress {
                        state: ImportState::Complete,
                        processed: stats.total_source,
                        matched: stats.matched,
                        inserted: stats.inserted,
                        elapsed_secs: start.elapsed().as_secs(),
                        error: None,
                    });
                }
                Err(e) => {
                    if let Some(ref mut p) = *guard {
                        p.state = ImportState::Failed;
                        p.error = Some(e.to_string());
                        p.elapsed_secs = start.elapsed().as_secs();
                    }
                }
            }
        }
    }

    /// Check if an image import is already running.
    fn is_image_import_running(&self) -> bool {
        use crate::server_fns::ImageImportState;
        let guard = self
            .image_import_progress
            .read()
            .expect("image_import_progress lock poisoned");
        guard.as_ref().is_some_and(|p| {
            matches!(
                p.state,
                ImageImportState::Cloning | ImageImportState::Copying
            )
        })
    }

    /// Import images for a single system (blocking, runs on current thread).
    /// Updates `image_import_progress` as it goes. Returns the import result.
    fn import_system_images_blocking(
        &self,
        system: &str,
        current_system: usize,
        total_systems: usize,
        start: std::time::Instant,
    ) {
        use crate::server_fns::{ImageImportProgress, ImageImportState};

        let system_display = replay_control_core::systems::find_system(system)
            .map(|s| s.display_name.to_string())
            .unwrap_or_else(|| system.to_string());

        let repo_names = match replay_control_core::thumbnails::thumbnail_repo_names(system) {
            Some(names) => names,
            None => {
                tracing::warn!("No thumbnail repo for {system}, skipping");
                return;
            }
        };

        let storage_root = self.storage().root.clone();
        let clone_base = storage_root
            .join(replay_control_core::metadata_db::RC_DIR)
            .join("tmp");
        let rom_filenames =
            replay_control_core::thumbnails::list_rom_filenames(&storage_root, system);

        // Take DB from state.
        let db = {
            let mut guard = self.metadata_db.lock().expect("metadata_db lock poisoned");
            guard.take()
        };
        let mut db = match db {
            Some(db) => db,
            None => match replay_control_core::metadata_db::MetadataDb::open(&storage_root) {
                Ok(db) => db,
                Err(e) => {
                    let mut guard = self.image_import_progress.write().expect("lock");
                    if let Some(ref mut p) = *guard {
                        p.state = ImageImportState::Failed;
                        p.error = Some(format!("Cannot open metadata DB: {e}"));
                        p.elapsed_secs = start.elapsed().as_secs();
                    }
                    return;
                }
            },
        };

        let mut total_boxart = 0usize;
        let mut total_snap = 0usize;
        let mut last_error: Option<String> = None;

        // Import from each repo in order. import_system_thumbnails skips ROMs
        // that already have images, so later repos only fill gaps.
        for (repo_idx, repo_name) in repo_names.iter().enumerate() {
            // Check for cancellation before each repo.
            if self
                .image_import_cancel
                .load(std::sync::atomic::Ordering::Relaxed)
            {
                let mut guard = self.image_import_progress.write().expect("lock");
                if let Some(ref mut p) = *guard {
                    p.state = ImageImportState::Cancelled;
                    p.elapsed_secs = start.elapsed().as_secs();
                }
                break;
            }

            let label = if repo_names.len() > 1 {
                format!("{system_display} ({repo_name})")
            } else {
                system_display.clone()
            };

            // Set cloning progress.
            {
                let mut guard = self.image_import_progress.write().expect("lock");
                *guard = Some(ImageImportProgress {
                    state: ImageImportState::Cloning,
                    system: system.to_string(),
                    system_display: label.clone(),
                    processed: 0,
                    total: rom_filenames.len(),
                    boxart_copied: total_boxart,
                    snap_copied: total_snap,
                    elapsed_secs: start.elapsed().as_secs(),
                    error: None,
                    current_system,
                    total_systems,
                });
            }

            let repo_dir = match replay_control_core::thumbnails::clone_thumbnail_repo(
                repo_name,
                Some(&clone_base),
                Some(&self.image_import_cancel),
            ) {
                Ok(dir) => dir,
                Err(e) => {
                    // If cancelled during clone, set Cancelled state and stop.
                    if self
                        .image_import_cancel
                        .load(std::sync::atomic::Ordering::Relaxed)
                    {
                        let mut guard = self.image_import_progress.write().expect("lock");
                        if let Some(ref mut p) = *guard {
                            p.state = ImageImportState::Cancelled;
                            p.elapsed_secs = start.elapsed().as_secs();
                        }
                        break;
                    }
                    tracing::warn!("Clone failed for {repo_name}: {e}");
                    // For multi-repo systems, continue to next repo instead of failing entirely
                    if repo_idx == 0 && repo_names.len() == 1 {
                        let mut guard = self.image_import_progress.write().expect("lock");
                        if let Some(ref mut p) = *guard {
                            p.state = ImageImportState::Failed;
                            p.error = Some(format!("Clone failed: {e}"));
                            p.elapsed_secs = start.elapsed().as_secs();
                        }
                    } else {
                        last_error = Some(format!("Clone failed for {repo_name}: {e}"));
                    }
                    continue;
                }
            };

            // Check for cancellation after clone.
            if self
                .image_import_cancel
                .load(std::sync::atomic::Ordering::Relaxed)
            {
                let mut guard = self.image_import_progress.write().expect("lock");
                if let Some(ref mut p) = *guard {
                    p.state = ImageImportState::Cancelled;
                    p.elapsed_secs = start.elapsed().as_secs();
                }
                let _ = std::fs::remove_dir_all(&repo_dir);
                break;
            }

            // Update progress to Copying.
            {
                let mut guard = self.image_import_progress.write().expect("lock");
                if let Some(ref mut p) = *guard {
                    p.state = ImageImportState::Copying;
                    p.system_display = label;
                    p.total = rom_filenames.len();
                    p.elapsed_secs = start.elapsed().as_secs();
                }
            }

            let progress_ref = self.image_import_progress.clone();
            let cancel_ref = self.image_import_cancel.clone();
            let prev_boxart = total_boxart;
            let result = replay_control_core::thumbnails::import_system_thumbnails(
                &repo_dir,
                system,
                &storage_root,
                &mut db,
                &rom_filenames,
                |processed, images_found| {
                    let mut guard = progress_ref.write().expect("lock");
                    if let Some(ref mut p) = *guard {
                        p.processed = processed;
                        p.boxart_copied = prev_boxart + images_found;
                        p.elapsed_secs = start.elapsed().as_secs();
                    }
                    !cancel_ref.load(std::sync::atomic::Ordering::Relaxed)
                },
            );

            match result {
                Ok(stats) => {
                    total_boxart += stats.boxart_copied;
                    total_snap += stats.snap_copied;
                }
                Err(e) => {
                    last_error = Some(e.to_string());
                }
            }

            // Clean up cloned repo to save disk space.
            let _ = std::fs::remove_dir_all(&repo_dir);

            // Check for cancellation after copy.
            if self
                .image_import_cancel
                .load(std::sync::atomic::Ordering::Relaxed)
            {
                let mut guard = self.image_import_progress.write().expect("lock");
                if let Some(ref mut p) = *guard {
                    p.state = ImageImportState::Cancelled;
                    p.boxart_copied = total_boxart;
                    p.snap_copied = total_snap;
                    p.elapsed_secs = start.elapsed().as_secs();
                }
                break;
            }
        }

        // Put DB back.
        {
            let mut guard = self.metadata_db.lock().expect("metadata_db lock poisoned");
            *guard = Some(db);
        }

        // Update final progress for this system (skip if already cancelled).
        {
            let mut guard = self.image_import_progress.write().expect("lock");
            let already_cancelled = guard
                .as_ref()
                .map(|p| p.state == ImageImportState::Cancelled)
                .unwrap_or(false);
            if !already_cancelled {
                if last_error.is_some() && total_boxart == 0 && total_snap == 0 {
                    if let Some(ref mut p) = *guard {
                        p.state = ImageImportState::Failed;
                        p.error = last_error;
                        p.elapsed_secs = start.elapsed().as_secs();
                    }
                } else {
                    *guard = Some(ImageImportProgress {
                        state: ImageImportState::Complete,
                        system: system.to_string(),
                        system_display,
                        processed: rom_filenames.len(),
                        total: rom_filenames.len(),
                        boxart_copied: total_boxart,
                        snap_copied: total_snap,
                        elapsed_secs: start.elapsed().as_secs(),
                        error: None,
                        current_system,
                        total_systems,
                    });
                }
            }
        }
    }

    /// Start a background image import for a single system.
    /// Returns `false` if an import is already running.
    pub fn start_image_import(&self, system: String) -> bool {
        if self.is_image_import_running() {
            return false;
        }

        use crate::server_fns::{ImageImportProgress, ImageImportState};
        self.image_import_cancel
            .store(false, std::sync::atomic::Ordering::Relaxed);
        // Write initial progress before spawning so the first poll never returns None.
        {
            let mut guard = self.image_import_progress.write().expect("lock");
            *guard = Some(ImageImportProgress {
                state: ImageImportState::Cloning,
                system: system.clone(),
                system_display: system.clone(),
                processed: 0,
                total: 0,
                boxart_copied: 0,
                snap_copied: 0,
                elapsed_secs: 0,
                error: None,
                current_system: 1,
                total_systems: 1,
            });
        }
        let state = self.clone();
        tokio::task::spawn_blocking(move || {
            let start = std::time::Instant::now();
            state.import_system_images_blocking(&system, 1, 1, start);
        });

        true
    }

    /// Start a background image import for all supported systems.
    /// Returns `false` if an import is already running.
    pub fn start_all_images_import(&self) -> bool {
        if self.is_image_import_running() {
            return false;
        }

        // Collect systems that have a thumbnail repo and games on disk.
        let storage = self.storage();
        let systems = self.cache.get_systems(&storage);
        let supported: Vec<String> = systems
            .into_iter()
            .filter(|s| s.game_count > 0)
            .filter(|s| {
                replay_control_core::thumbnails::thumbnail_repo_names(&s.folder_name).is_some()
            })
            .map(|s| s.folder_name)
            .collect();

        if supported.is_empty() {
            return false;
        }

        use crate::server_fns::{ImageImportProgress, ImageImportState};
        self.image_import_cancel
            .store(false, std::sync::atomic::Ordering::Relaxed);
        // Write initial progress before spawning so the first poll never returns None.
        {
            let total = supported.len();
            let mut guard = self.image_import_progress.write().expect("lock");
            *guard = Some(ImageImportProgress {
                state: ImageImportState::Cloning,
                system: supported[0].clone(),
                system_display: String::new(),
                processed: 0,
                total: 0,
                boxart_copied: 0,
                snap_copied: 0,
                elapsed_secs: 0,
                error: None,
                current_system: 1,
                total_systems: total,
            });
        }
        let state = self.clone();
        let total = supported.len();
        tokio::task::spawn_blocking(move || {
            let start = std::time::Instant::now();
            for (i, system) in supported.iter().enumerate() {
                state.import_system_images_blocking(system, i + 1, total, start);

                // If the last system failed or was cancelled, stop the whole batch.
                {
                    use crate::server_fns::ImageImportState;
                    let guard = state.image_import_progress.read().expect("lock");
                    if let Some(ref p) = *guard {
                        if matches!(
                            p.state,
                            ImageImportState::Failed | ImageImportState::Cancelled
                        ) {
                            break;
                        }
                    }
                }
            }
        });

        true
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
            let watcher_active =
                Self::try_start_config_watcher(watcher_state, watcher_config_path).await;

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
    async fn try_start_config_watcher(state: AppState, config_path: PathBuf) -> bool {
        use notify::{RecursiveMode, Watcher, recommended_watcher};

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
        let mut watcher =
            match recommended_watcher(move |res: Result<notify::Event, notify::Error>| match res {
                Ok(event) => {
                    let _ = tx.blocking_send(event);
                }
                Err(e) => {
                    tracing::warn!("File watcher error: {e}");
                }
            }) {
                Ok(w) => w,
                Err(e) => {
                    tracing::warn!("Failed to create file watcher: {e}");
                    return false;
                }
            };

        if let Err(e) = watcher.watch(&watch_dir, RecursiveMode::NonRecursive) {
            tracing::warn!("Failed to watch directory {}: {e}", watch_dir.display());
            return false;
        }

        tracing::info!("Watching {} for config changes", watch_dir.display());

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
                                tracing::debug!(
                                    "Additional config event during debounce ({:?})",
                                    ev.kind
                                );
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
        matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_))
            && event
                .paths
                .iter()
                .any(|p| p.file_name().is_some_and(|n| n == config_filename))
    }
}
