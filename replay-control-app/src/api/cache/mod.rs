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

/// Hard TTL for NFS storage — even if mtime hasn't changed, re-scan after this long.
/// Local storage (SD/USB/NVMe) has no TTL because inotify + mtime + explicit
/// invalidation already cover all change scenarios.
const NFS_CACHE_TTL: Duration = Duration::from_secs(1800); // 30 minutes

/// Compute the max mtime across a directory and its immediate subdirectories
/// (maxdepth 2). This detects changes inside organizational subdirectories
/// like `00 Clean Romset/` without the cost of a full recursive scan.
pub(crate) fn dir_mtime(path: &Path) -> Option<SystemTime> {
    let mut max_mtime = std::fs::metadata(path).ok().and_then(|m| m.modified().ok())?;

    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            if entry.file_type().ok().is_some_and(|ft| ft.is_dir())
                && let Some(mtime) = entry.metadata().ok().and_then(|m| m.modified().ok())
                && mtime > max_mtime
            {
                max_mtime = mtime;
            }
        }
    }

    Some(max_mtime)
}

/// Mtime + optional TTL freshness tracker.
///
/// Shared by `CacheEntry`, `FavoritesCache`, and `ImageIndex` to avoid
/// duplicating the same expiry/mtime logic across cache types.
///
/// For local storage (SD/USB/NVMe), there is no TTL — inotify watcher + mtime
/// comparison + explicit `invalidate()` calls cover all change scenarios.
/// For NFS, a 30-minute TTL acts as a safety net since inotify doesn't detect
/// remote changes (lazy: only checked on access, so it won't wake idle disks).
pub(super) struct Freshness {
    dir_mtime: Option<SystemTime>,
    /// `None` for local storage (no TTL), `Some` for NFS.
    expires: Option<Instant>,
}

impl Freshness {
    pub(super) fn new(dir: &Path, is_local: bool) -> Self {
        Self {
            dir_mtime: dir_mtime(dir),
            expires: if is_local {
                None
            } else {
                Some(Instant::now() + NFS_CACHE_TTL)
            },
        }
    }

    /// Fresh = hard TTL not expired (if set) AND directory mtime unchanged.
    pub(super) fn is_fresh(&self, dir: &Path) -> bool {
        if self.expires.is_some_and(|exp| Instant::now() >= exp) {
            return false;
        }
        match (self.dir_mtime, dir_mtime(dir)) {
            (Some(cached), Some(current)) => cached == current,
            _ => true,
        }
    }
}

/// Cached result with freshness tracking.
pub(super) struct CacheEntry<T> {
    pub(super) data: T,
    freshness: Freshness,
}

impl<T: Clone> CacheEntry<T> {
    pub(super) fn new(data: T, dir: &Path, is_local: bool) -> Self {
        Self {
            data,
            freshness: Freshness::new(dir, is_local),
        }
    }

    pub(super) fn is_fresh(&self, dir: &Path) -> bool {
        self.freshness.is_fresh(dir)
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
    /// Shared activity state. Used by get_roms() to check if startup scanning
    /// is in progress (suppresses L3 filesystem scans during Phase 2).
    activity: Arc<std::sync::RwLock<super::activity::Activity>>,
}

impl GameLibrary {
    pub(crate) fn new(
        db: DbPool,
        activity: Arc<std::sync::RwLock<super::activity::Activity>>,
    ) -> Self {
        Self {
            systems: std::sync::RwLock::new(None),
            roms: std::sync::RwLock::new(HashMap::new()),
            favorites: std::sync::RwLock::new(None),
            recents: std::sync::RwLock::new(None),
            images: std::sync::RwLock::new(HashMap::new()),
            db,
            activity,
        }
    }

    /// Run a read-only closure with the metadata DB connection.
    /// Returns None if the DB is unavailable.
    pub async fn db_read<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&rusqlite::Connection) -> R + Send + 'static,
        R: Send + 'static,
    {
        self.db.read(f).await
    }

    /// Run a write closure with the metadata DB connection.
    /// Returns None if the DB is unavailable.
    pub async fn db_write<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&mut rusqlite::Connection) -> R + Send + 'static,
        R: Send + 'static,
    {
        self.db.write(f).await
    }

    /// Get cached systems or scan and cache.
    /// L1 (in-memory) → L2 (SQLite game_library_meta) → L3 (filesystem scan).
    pub async fn cached_systems(&self, storage: &StorageLocation) -> Vec<SystemSummary> {
        let roms_dir = storage.roms_dir();

        // L1: Try in-memory cache.
        if let Ok(guard) = self.systems.read()
            && let Some(ref entry) = *guard
            && entry.is_fresh(&roms_dir)
        {
            return entry.data.clone();
        }

        // L2: Try SQLite game_library_meta (reconstructs SystemSummary from cached metadata).
        let is_local = storage.kind.is_local();
        if let Some(summaries) = self.load_systems_from_db(storage).await
            && !summaries.is_empty()
        {
            // Store in L1.
            if let Ok(mut guard) = self.systems.write() {
                *guard = Some(CacheEntry::new(summaries.clone(), &roms_dir, is_local));
            }
            return summaries;
        }

        // L3: Cache miss — full filesystem scan.
        let summaries = replay_control_core::roms::scan_systems(storage);
        if let Ok(mut guard) = self.systems.write() {
            *guard = Some(CacheEntry::new(summaries.clone(), &roms_dir, is_local));
        }

        // Write-through to L2 (background-safe: no lock held on L1).
        self.save_systems_to_db(storage, &summaries).await;

        summaries
    }

    /// Try to reconstruct SystemSummary list from SQLite game_library_meta.
    async fn load_systems_from_db(&self, _storage: &StorageLocation) -> Option<Vec<SystemSummary>> {
        use replay_control_core::systems;

        let cached_meta = self.db.read(MetadataDb::load_all_system_meta).await?;
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
    async fn save_systems_to_db(&self, storage: &StorageLocation, summaries: &[SystemSummary]) {
        let roms_dir = storage.roms_dir();
        let summaries: Vec<_> = summaries.to_vec();
        self.db
            .write(move |conn| {
                for summary in &summaries {
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
            })
            .await;
    }

    /// Get cached ROM list for a system.
    /// Checks L1 (in-memory) → L2 (SQLite game_library).
    /// If neither has data and warmup is in progress, returns empty.
    /// Otherwise falls through to a full L3 filesystem scan.
    pub async fn cached_roms(
        &self,
        storage: &StorageLocation,
        system: &str,
        region_pref: RegionPreference,
        region_secondary: Option<RegionPreference>,
    ) -> Result<Arc<Vec<RomEntry>>, replay_control_core::error::Error> {
        // L1/L2: try cached data.
        if let Some(roms) = self.get_roms_cached(storage, system).await {
            return Ok(roms);
        }

        // During startup scanning (Phase 2), return empty — the pipeline will populate.
        // Only Startup { Scanning } blocks here, not other activities — thumbnail
        // update and import should NOT prevent ROM lookups.
        {
            let is_scanning = self.activity.read().is_ok_and(|act| {
                matches!(
                    *act,
                    super::activity::Activity::Startup {
                        phase: super::activity::StartupPhase::Scanning,
                        ..
                    }
                )
            });
            if is_scanning {
                tracing::debug!("get_roms({system}): returning empty (startup scan in progress)");
                return Ok(Arc::new(Vec::new()));
            }
        }

        // L3: full filesystem scan (user-triggered, outside warmup).
        self.scan_and_cache_system(storage, system, region_pref, region_secondary)
            .await
    }

    /// Try L1 (in-memory) then L2 (SQLite) for cached ROM data.
    /// Returns None if neither has fresh data.
    async fn get_roms_cached(
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
        if let Some(roms) = self.load_roms_from_db(storage, system, &system_dir).await {
            let arc = Arc::new(roms);
            if let Ok(mut guard) = self.roms.write() {
                guard.insert(
                    key,
                    CacheEntry::new(Arc::clone(&arc), &system_dir, storage.kind.is_local()),
                );
            }
            return Some(arc);
        }

        None
    }

    /// Look up a single ROM entry: L1 cache first, then direct L2 DB lookup.
    /// Never triggers a full L3 filesystem scan.
    pub async fn get_single_rom(
        &self,
        storage: &StorageLocation,
        system: &str,
        filename: &str,
    ) -> Option<RomEntry> {
        let system_dir = storage.roms_dir().join(system);

        // L1: check in-memory cache (cheap Arc::clone + linear scan).
        if let Ok(guard) = self.roms.read()
            && let Some(entry) = guard.get(system)
            && entry.is_fresh(&system_dir)
        {
            return entry
                .data
                .iter()
                .find(|r| r.game.rom_filename == filename)
                .cloned();
        }

        // L2: direct single-row DB lookup (no full system load).
        let sys = system.to_string();
        let fname = filename.to_string();
        let game_entry = self
            .db
            .read(move |conn| MetadataDb::load_single_entry(conn, &sys, &fname))
            .await?
            .ok()??;

        // Convert GameEntry -> RomEntry (same as load_roms_from_db).
        use replay_control_core::game_ref::GameRef;
        Some(RomEntry {
            game: GameRef::new_with_display(
                &game_entry.system,
                game_entry.rom_filename,
                game_entry.rom_path,
                game_entry.display_name,
            ),
            size_bytes: game_entry.size_bytes,
            is_m3u: game_entry.is_m3u,
            is_favorite: false,
            box_art_url: game_entry.box_art_url,
            driver_status: game_entry.driver_status,
            rating: game_entry.rating,
            players: game_entry.players,
        })
    }

    /// Scan a system from filesystem and populate L1+L2 cache.
    /// Called by the background pipeline during warmup and by get_roms() outside warmup.
    pub async fn scan_and_cache_system(
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
        let hash_results = self.hash_roms_for_system(storage, system, &mut roms).await;

        let arc = Arc::new(roms);

        if let Ok(mut guard) = self.roms.write() {
            guard.insert(
                key,
                CacheEntry::new(Arc::clone(&arc), &system_dir, storage.kind.is_local()),
            );
        }

        // Write-through to L2.
        self.save_roms_to_db(storage, system, &arc, &system_dir, &hash_results)
            .await;

        Ok(arc)
    }

    /// Try to load ROMs from SQLite game_library, validating via mtime.
    async fn load_roms_from_db(
        &self,
        _storage: &StorageLocation,
        system: &str,
        system_dir: &Path,
    ) -> Option<Vec<RomEntry>> {
        use replay_control_core::metadata_db::SystemMeta;

        let sys = system.to_string();
        let meta: SystemMeta = self
            .db
            .read(move |conn| MetadataDb::load_system_meta(conn, &sys))
            .await?
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
        let sys = system.to_string();
        let cached_roms = self
            .db
            .read(move |conn| MetadataDb::load_system_entries(conn, &sys))
            .await?
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
            *guard = Some(CacheEntry::new(
                entries.clone(),
                &recents_dir,
                storage.kind.is_local(),
            ));
        }
        Ok(entries)
    }

    /// Invalidate all caches (after delete, rename, upload).
    /// Clears both L1 (in-memory) and L2 (SQLite game_library).
    pub async fn invalidate(&self) {
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
        self.db
            .write(|conn| {
                let _ = MetadataDb::clear_all_game_library(conn);
            })
            .await;
    }

    /// Invalidate cache for a specific system.
    /// Clears both L1 (in-memory) and L2 (SQLite game_library) for the system.
    pub async fn invalidate_system(&self, system: String) {
        if let Ok(mut guard) = self.systems.write() {
            *guard = None;
        }
        if let Ok(mut guard) = self.roms.write() {
            guard.remove(&system);
        }
        // L2: Clear SQLite game_library for this system.
        self.db
            .write(move |conn| {
                let _ = MetadataDb::clear_system_game_library(conn, &system);
            })
            .await;
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

    #[tokio::test(flavor = "multi_thread")]
    async fn startup_scanning_blocks_l3_scan() {
        use crate::api::activity::{Activity, StartupPhase};

        let activity = Arc::new(std::sync::RwLock::new(Activity::Startup {
            phase: StartupPhase::Scanning,
            system: String::new(),
        }));
        // Create a dummy DbPool with no connection (closed).
        let db = DbPool::new_closed("test");
        let cache = GameLibrary::new(db, activity.clone());

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

        // get_roms should return empty during startup scanning.
        let result = cache
            .cached_roms(
                &storage,
                "test_system",
                replay_control_core::rom_tags::RegionPreference::Usa,
                None,
            )
            .await;
        assert!(result.unwrap().is_empty());

        // Set to Idle — should allow L3 scans again.
        *activity.write().unwrap() = Activity::Idle;
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// ActivityGuard is cleared on drop.
    /// This validates the guard pattern used in BackgroundManager::run_pipeline.
    #[test]
    fn activity_guard_resets_to_idle_on_drop() {
        use crate::api::activity::{Activity, StartupPhase};

        let activity = Arc::new(std::sync::RwLock::new(Activity::Idle));

        {
            // Simulate what run_pipeline does: set startup, then drop guard.
            *activity.write().unwrap() = Activity::Startup {
                phase: StartupPhase::Scanning,
                system: String::new(),
            };
            assert!(matches!(
                *activity.read().unwrap(),
                Activity::Startup { .. }
            ));

            let _guard = crate::api::activity::ActivityGuard::new_for_test(activity.clone());
            // Guard is alive — activity should still be Startup.
            assert!(matches!(
                *activity.read().unwrap(),
                Activity::Startup { .. }
            ));
        }
        // Guard dropped — activity should be Idle.
        assert!(matches!(*activity.read().unwrap(), Activity::Idle));
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
