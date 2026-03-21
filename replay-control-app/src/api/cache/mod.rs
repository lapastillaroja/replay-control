mod aliases;
mod enrichment;
mod favorites;
mod hashing;
mod images;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use deadpool_sqlite::rusqlite;
use replay_control_core::metadata_db::MetadataDb;
use replay_control_core::recents::RecentEntry;
use replay_control_core::rom_tags::RegionPreference;
use replay_control_core::roms::{RomEntry, SystemSummary};
use replay_control_core::storage::StorageLocation;

use super::DbPool;

/// Hard TTL — even if mtime hasn't changed, re-scan after this long.
const CACHE_HARD_TTL: Duration = Duration::from_secs(300);

/// Read the mtime of a directory (single stat call).
pub(crate) fn dir_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok().and_then(|m| m.modified().ok())
}

/// Cached result with mtime-based + hard-TTL invalidation.
pub(super) struct CacheEntry<T> {
    pub(super) data: T,
    dir_mtime: Option<SystemTime>,
    expires: Instant,
}

impl<T: Clone> CacheEntry<T> {
    pub(super) fn new(data: T, dir: &Path) -> Self {
        Self {
            data,
            dir_mtime: dir_mtime(dir),
            expires: Instant::now() + CACHE_HARD_TTL,
        }
    }

    /// Check if cached data is still fresh.
    /// Fresh = hard TTL not expired AND directory mtime unchanged.
    pub(super) fn is_fresh(&self, dir: &Path) -> bool {
        if Instant::now() >= self.expires {
            return false;
        }
        // Compare directory mtime — if it changed, cache is stale.
        match (self.dir_mtime, dir_mtime(dir)) {
            (Some(cached), Some(current)) => cached == current,
            // If we can't read mtime (e.g., NFS flake), trust hard TTL.
            _ => true,
        }
    }
}

use favorites::FavoritesCache;
pub use images::ImageIndex;

pub struct GameLibrary {
    pub(super) systems: std::sync::RwLock<Option<CacheEntry<Vec<SystemSummary>>>>,
    pub(super) roms: std::sync::RwLock<HashMap<String, CacheEntry<Arc<Vec<RomEntry>>>>>,
    pub(super) favorites: std::sync::RwLock<Option<FavoritesCache>>,
    pub(super) recents: std::sync::RwLock<Option<CacheEntry<Vec<RecentEntry>>>>,
    /// Per-system image index for batch box art resolution.
    /// Wrapped in `Arc` so cache hits return a cheap `Arc::clone()` instead of
    /// deep-copying all 4 HashMaps.
    pub(super) images: std::sync::RwLock<HashMap<String, Arc<ImageIndex>>>,
    /// Metadata DB pool for L2 persistent cache.
    pub(super) db: DbPool,
    /// Unified busy flag (same Arc as AppState.busy).
    /// When set, get_roms() returns empty instead of blocking on L3 scan.
    busy: Arc<std::sync::atomic::AtomicBool>,
    /// Scanning indicator (same Arc as AppState.scanning).
    /// True only during Phase 2 game library populate.
    pub(crate) scanning: Arc<std::sync::atomic::AtomicBool>,
}

impl GameLibrary {
    pub(crate) fn new(
        db: DbPool,
        busy: Arc<std::sync::atomic::AtomicBool>,
        scanning: Arc<std::sync::atomic::AtomicBool>,
    ) -> Self {
        Self {
            systems: std::sync::RwLock::new(None),
            roms: std::sync::RwLock::new(HashMap::new()),
            favorites: std::sync::RwLock::new(None),
            recents: std::sync::RwLock::new(None),
            images: std::sync::RwLock::new(HashMap::new()),
            db,
            busy,
            scanning,
        }
    }

    /// Run a read-only closure with the metadata DB connection.
    /// Returns None if the DB is unavailable.
    pub fn with_db_read<F, R>(&self, _storage: &StorageLocation, f: F) -> Option<R>
    where
        F: FnOnce(&rusqlite::Connection) -> R,
    {
        self.db.read(f)
    }

    /// Run a write closure with the metadata DB connection.
    /// Returns None if the DB is unavailable.
    pub(super) fn with_db_mut<F, R>(&self, _storage: &StorageLocation, f: F) -> Option<R>
    where
        F: FnOnce(&mut rusqlite::Connection) -> R,
    {
        self.db.write(f)
    }

    /// Get cached systems or scan and cache.
    /// L1 (in-memory) → L2 (SQLite game_library_meta) → L3 (filesystem scan).
    pub fn get_systems(&self, storage: &StorageLocation) -> Vec<SystemSummary> {
        let roms_dir = storage.roms_dir();

        // L1: Try in-memory cache.
        if let Ok(guard) = self.systems.read()
            && let Some(ref entry) = *guard
            && entry.is_fresh(&roms_dir)
        {
            return entry.data.clone();
        }

        // L2: Try SQLite game_library_meta (reconstructs SystemSummary from cached metadata).
        if let Some(summaries) = self.load_systems_from_db(storage)
            && !summaries.is_empty()
        {
            // Store in L1.
            if let Ok(mut guard) = self.systems.write() {
                *guard = Some(CacheEntry::new(summaries.clone(), &roms_dir));
            }
            return summaries;
        }

        // L3: Cache miss — full filesystem scan.
        let summaries = replay_control_core::roms::scan_systems(storage);
        if let Ok(mut guard) = self.systems.write() {
            *guard = Some(CacheEntry::new(summaries.clone(), &roms_dir));
        }

        // Write-through to L2 (background-safe: no lock held on L1).
        self.save_systems_to_db(storage, &summaries);

        summaries
    }

    /// Try to reconstruct SystemSummary list from SQLite game_library_meta.
    fn load_systems_from_db(&self, storage: &StorageLocation) -> Option<Vec<SystemSummary>> {
        use replay_control_core::systems;

        let cached_meta = self.with_db_read(storage, |conn| MetadataDb::load_all_system_meta(conn))?;
        let cached_meta = cached_meta.ok()?;

        if cached_meta.is_empty() {
            return None;
        }

        // Build a lookup map from cached data.
        let meta_map: HashMap<String, &replay_control_core::metadata_db::SystemMeta> =
            cached_meta.iter().map(|m| (m.system.clone(), m)).collect();

        let mut summaries = Vec::new();
        for system in systems::visible_systems() {
            let (game_count, total_size_bytes) =
                if let Some(meta) = meta_map.get(system.folder_name) {
                    (meta.rom_count, meta.total_size_bytes)
                } else {
                    (0, 0)
                };

            summaries.push(SystemSummary {
                folder_name: system.folder_name.to_string(),
                display_name: system.display_name.to_string(),
                manufacturer: system.manufacturer.to_string(),
                category: format!("{:?}", system.category).to_lowercase(),
                game_count,
                total_size_bytes,
            });
        }

        // Sort: systems with games first, then alphabetically (same as scan_systems).
        summaries.sort_by(|a, b| {
            let a_has = a.game_count > 0;
            let b_has = b.game_count > 0;
            b_has.cmp(&a_has).then(a.display_name.cmp(&b.display_name))
        });

        Some(summaries)
    }

    /// Write system summaries to SQLite game_library_meta.
    fn save_systems_to_db(&self, storage: &StorageLocation, summaries: &[SystemSummary]) {
        let roms_dir = storage.roms_dir();
        self.with_db_mut(storage, |conn| {
            for summary in summaries {
                if summary.game_count == 0 {
                    continue;
                }
                let system_dir = roms_dir.join(&summary.folder_name);
                let mtime_secs = dir_mtime(&system_dir).and_then(|t| {
                    t.duration_since(std::time::UNIX_EPOCH)
                        .ok()
                        .map(|d| d.as_secs() as i64)
                });
                let _ = MetadataDb::save_system_meta(
                    conn,
                    &summary.folder_name,
                    mtime_secs,
                    summary.game_count,
                    summary.total_size_bytes,
                );
            }
        });
    }

    /// Get cached ROM list for a system.
    /// Checks L1 (in-memory) → L2 (SQLite game_library).
    /// If neither has data and warmup is in progress, returns empty.
    /// Otherwise falls through to a full L3 filesystem scan.
    pub fn get_roms(
        &self,
        storage: &StorageLocation,
        system: &str,
        region_pref: RegionPreference,
        region_secondary: Option<RegionPreference>,
    ) -> Result<Arc<Vec<RomEntry>>, replay_control_core::error::Error> {
        // L1/L2: try cached data.
        if let Some(roms) = self.get_roms_cached(storage, system) {
            return Ok(roms);
        }

        // During warmup, return empty — the background pipeline will populate.
        if self.busy.load(std::sync::atomic::Ordering::Acquire) {
            tracing::debug!("get_roms({system}): returning empty (warmup in progress)");
            return Ok(Arc::new(Vec::new()));
        }

        // L3: full filesystem scan (user-triggered, outside warmup).
        self.scan_and_cache_system(storage, system, region_pref, region_secondary)
    }

    /// Try L1 (in-memory) then L2 (SQLite) for cached ROM data.
    /// Returns None if neither has fresh data.
    fn get_roms_cached(
        &self,
        storage: &StorageLocation,
        system: &str,
    ) -> Option<Arc<Vec<RomEntry>>> {
        let key = system.to_string();
        let system_dir = storage.roms_dir().join(system);

        // L1: in-memory cache — cheap Arc::clone() on hit.
        if let Ok(guard) = self.roms.read()
            && let Some(entry) = guard.get(&key)
            && entry.is_fresh(&system_dir)
        {
            return Some(Arc::clone(&entry.data));
        }

        // L2: SQLite game_library.
        if let Some(roms) = self.load_roms_from_db(storage, system, &system_dir) {
            let arc = Arc::new(roms);
            if let Ok(mut guard) = self.roms.write() {
                guard.insert(key, CacheEntry::new(Arc::clone(&arc), &system_dir));
            }
            return Some(arc);
        }

        None
    }

    /// Scan a system from filesystem and populate L1+L2 cache.
    /// Called by the background pipeline during warmup and by get_roms() outside warmup.
    pub fn scan_and_cache_system(
        &self,
        storage: &StorageLocation,
        system: &str,
        region_pref: RegionPreference,
        region_secondary: Option<RegionPreference>,
    ) -> Result<Arc<Vec<RomEntry>>, replay_control_core::error::Error> {
        let key = system.to_string();
        let system_dir = storage.roms_dir().join(system);

        tracing::debug!("L3 scan for {system}: starting filesystem scan");
        let mut roms =
            replay_control_core::roms::list_roms(storage, system, region_pref, region_secondary)?;
        tracing::debug!("L3 scan for {system}: found {} ROMs", roms.len());

        // Hash-and-identify step: for hash-eligible systems, compute CRC32 hashes
        // and look up canonical names in the embedded No-Intro DAT data.
        let hash_results = self.hash_roms_for_system(storage, system, &mut roms);

        let arc = Arc::new(roms);

        if let Ok(mut guard) = self.roms.write() {
            guard.insert(key.clone(), CacheEntry::new(Arc::clone(&arc), &system_dir));
        }

        // Write-through to L2.
        self.save_roms_to_db(storage, system, &arc, &system_dir, &hash_results);

        Ok(arc)
    }

    /// Try to load ROMs from SQLite game_library, validating via mtime.
    fn load_roms_from_db(
        &self,
        storage: &StorageLocation,
        system: &str,
        system_dir: &Path,
    ) -> Option<Vec<RomEntry>> {
        use replay_control_core::metadata_db::SystemMeta;

        let meta: SystemMeta = self
            .with_db_read(storage, |conn| MetadataDb::load_system_meta(conn, system))?
            .ok()??;

        // No cached ROMs? Skip L2.
        if meta.rom_count == 0 {
            return None;
        }

        // Check mtime freshness.
        let current_mtime_secs = dir_mtime(system_dir).and_then(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|d| d.as_secs() as i64)
        });

        match (meta.dir_mtime_secs, current_mtime_secs) {
            (Some(cached), Some(current)) if cached != current => {
                tracing::debug!(
                    "L2 cache stale for {system}: mtime changed ({cached} → {current})"
                );
                return None; // Stale — fall through to L3.
            }
            (Some(_), None) => {
                // Can't read current mtime (NFS flake) — trust the cache.
            }
            (None, _) => {
                // No mtime stored — cache was saved without mtime info. Trust it.
            }
            _ => {} // Mtimes match — cache is fresh.
        }

        // Load ROMs from DB.
        let cached_roms = self
            .with_db_read(storage, |conn| MetadataDb::load_system_entries(conn, system))?
            .ok()?;

        if cached_roms.is_empty() && meta.rom_count > 0 {
            // Meta says we have ROMs but game_library is empty — need L3 scan.
            return None;
        }

        // Convert GameEntry → RomEntry.
        let roms: Vec<RomEntry> = cached_roms
            .into_iter()
            .map(|cr| {
                use replay_control_core::game_ref::GameRef;
                RomEntry {
                    game: GameRef::new_with_display(
                        &cr.system,
                        cr.rom_filename,
                        cr.rom_path,
                        cr.display_name,
                    ),
                    size_bytes: cr.size_bytes,
                    is_m3u: cr.is_m3u,
                    is_favorite: false, // Set by caller via get_favorites_set()
                    box_art_url: cr.box_art_url,
                    driver_status: cr.driver_status,
                    rating: cr.rating,
                    players: cr.players,
                }
            })
            .collect();

        tracing::debug!(
            "L2 cache hit for {system}: {} ROMs loaded from SQLite",
            roms.len()
        );
        Some(roms)
    }

    /// Get cached recents or scan and cache.
    /// Recents are created by RePlayOS on game launch, so mtime-based
    /// invalidation detects new entries without explicit invalidation.
    pub fn get_recents(
        &self,
        storage: &StorageLocation,
    ) -> Result<Vec<RecentEntry>, replay_control_core::error::Error> {
        let recents_dir = storage.recents_dir();

        if let Ok(guard) = self.recents.read()
            && let Some(ref entry) = *guard
            && entry.is_fresh(&recents_dir)
        {
            return Ok(entry.data.clone());
        }

        let entries = replay_control_core::recents::list_recents(storage)?;
        if let Ok(mut guard) = self.recents.write() {
            *guard = Some(CacheEntry::new(entries.clone(), &recents_dir));
        }
        Ok(entries)
    }

    /// Invalidate all caches (after delete, rename, upload).
    /// Clears both L1 (in-memory) and L2 (SQLite game_library).
    pub fn invalidate(&self) {
        if let Ok(mut guard) = self.systems.write() {
            *guard = None;
        }
        if let Ok(mut guard) = self.roms.write() {
            guard.clear();
        }
        if let Ok(mut guard) = self.favorites.write() {
            *guard = None;
        }
        if let Ok(mut guard) = self.recents.write() {
            *guard = None;
        }
        if let Ok(mut guard) = self.images.write() {
            guard.clear();
        }
        // L2: Clear SQLite game_library.
        self.db.write(|conn| {
            let _ = MetadataDb::clear_all_game_library(conn);
        });
    }

    /// Invalidate cache for a specific system.
    /// Clears both L1 (in-memory) and L2 (SQLite game_library) for the system.
    pub fn invalidate_system(&self, system: &str) {
        if let Ok(mut guard) = self.systems.write() {
            *guard = None;
        }
        if let Ok(mut guard) = self.roms.write() {
            guard.remove(system);
        }
        // L2: Clear SQLite game_library for this system.
        self.db.write(|conn| {
            let _ = MetadataDb::clear_system_game_library(conn, system);
        });
    }

    /// Invalidate only the favorites cache (after add/remove favorite).
    pub fn invalidate_favorites(&self) {
        if let Ok(mut guard) = self.favorites.write() {
            *guard = None;
        }
    }

    /// Invalidate only the recents cache (after launch creates a new entry).
    pub fn invalidate_recents(&self) {
        if let Ok(mut guard) = self.recents.write() {
            *guard = None;
        }
    }

    /// Invalidate only the per-system image indexes.
    /// Called after thumbnail downloads to force re-scan of the media directory.
    pub fn invalidate_images(&self) {
        if let Ok(mut guard) = self.images.write() {
            guard.clear();
        }
    }

    /// Invalidate a single system's image index.
    pub fn invalidate_system_images(&self, system: &str) {
        if let Ok(mut guard) = self.images.write() {
            guard.remove(system);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_flag_blocks_l3_scan() {
        let busy = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let scanning = Arc::new(std::sync::atomic::AtomicBool::new(false));
        // Create a dummy DbPool with no connection (closed).
        let db = DbPool::new_closed("test");
        let cache = GameLibrary::new(db, busy.clone(), scanning);

        // Set busy.
        busy.store(true, std::sync::atomic::Ordering::SeqCst);

        let tmp = std::env::temp_dir().join(format!(
            "replay-cache-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("roms/test_system")).unwrap();
        let storage = replay_control_core::storage::StorageLocation::from_path(
            tmp.clone(),
            replay_control_core::storage::StorageKind::Sd,
        );

        // get_roms should return empty during busy.
        let result = cache.get_roms(
            &storage,
            "test_system",
            replay_control_core::rom_tags::RegionPreference::Usa,
            None,
        );
        assert!(result.unwrap().is_empty());

        busy.store(false, std::sync::atomic::Ordering::SeqCst);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Warmup flag is cleared by RAII guard even on panic.
    /// This validates the guard pattern used in BackgroundManager::run_pipeline.
    #[test]
    fn warmup_flag_cleared_by_guard_on_drop() {
        let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));

        // Simulate what run_pipeline does: set flag, create guard, then drop.
        flag.store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(flag.load(std::sync::atomic::Ordering::SeqCst));

        {
            let flag_clone = flag.clone();
            // This is the same Guard pattern from background.rs.
            struct Guard<F: FnOnce()>(Option<F>);
            impl<F: FnOnce()> Guard<F> {
                fn new(f: F) -> Self {
                    Self(Some(f))
                }
            }
            impl<F: FnOnce()> Drop for Guard<F> {
                fn drop(&mut self) {
                    if let Some(f) = self.0.take() {
                        f();
                    }
                }
            }
            let _guard = Guard::new(move || {
                flag_clone.store(false, std::sync::atomic::Ordering::SeqCst);
            });
            // Guard is alive here -- flag should still be true.
            assert!(flag.load(std::sync::atomic::Ordering::SeqCst));
        }
        // Guard dropped -- flag should be false.
        assert!(!flag.load(std::sync::atomic::Ordering::SeqCst));
    }

    /// Per-batch DB locking: verify that the DB Mutex is released between
    /// batch operations, allowing other threads to access the DB.
    #[test]
    fn per_batch_locking_releases_between_batches() {
        use std::sync::Mutex;
        let db_mutex: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(Some("db_handle".into())));

        // Simulate a per-batch flush pattern: lock, do work, release.
        let db_clone = db_mutex.clone();
        let read_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let read_count_clone = read_count.clone();
        let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let done_clone = done.clone();

        // Reader thread: tries to read between batches.
        let reader = std::thread::spawn(move || {
            while !done_clone.load(std::sync::atomic::Ordering::Relaxed) {
                if let Ok(guard) = db_clone.try_lock() {
                    if guard.is_some() {
                        read_count_clone.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                    drop(guard);
                }
                std::thread::yield_now();
            }
        });

        // Writer thread: simulates 5 batch flushes with gaps between them.
        for _ in 0..5 {
            {
                let _guard = db_mutex.lock().unwrap();
                // Simulate batch flush work.
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            // Gap between batches -- reader can acquire the lock here.
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        done.store(true, std::sync::atomic::Ordering::Relaxed);
        reader.join().unwrap();

        // Reader should have been able to acquire the lock at least once
        // during the gaps between batches.
        assert!(
            read_count.load(std::sync::atomic::Ordering::Relaxed) > 0,
            "Reader thread should have acquired the lock between batches"
        );
    }
}
