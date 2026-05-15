mod aliases;
mod enrichment;
mod favorites;
pub mod metadata_snapshot;
pub(crate) mod query;
mod recommendations_snapshot;
mod scan_pipeline;
pub(crate) use scan_pipeline::{ScanCancellation, ScanInputs, ScanOptions};
pub mod ssr_snapshot;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Instant, SystemTime};

use replay_control_core::rom_tags::RegionPreference;
use replay_control_core_server::db_pool::DbError;
use replay_control_core_server::library_db::LibraryDb;
use replay_control_core_server::recents::RecentEntry;
use replay_control_core_server::roms::{RomEntry, SystemSummary};
use replay_control_core_server::storage::StorageLocation;
use tokio::sync::RwLock;

use super::db_pools::{LibraryReadPool, LibraryWritePool};

/// Compute the max mtime across a directory and its immediate subdirectories
/// (maxdepth 2). This detects changes inside organizational subdirectories
/// like `00 Clean Romset/` without the cost of a full recursive scan.
///
/// Blocking filesystem metadata read. Call from scan/rebuild paths, outside
/// async request handlers.
pub(crate) fn dir_mtime(path: &Path) -> Option<SystemTime> {
    let mut max_mtime = std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())?;

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

/// `dir_mtime` rendered as Unix seconds for storage in `game_library_meta`.
pub(crate) fn dir_mtime_secs(path: &Path) -> Option<i64> {
    dir_mtime(path).and_then(|t| {
        t.duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|d| d.as_secs() as i64)
    })
}

use crate::server_fns::RecommendationData;
use favorites::FavoritesCache;
use metadata_snapshot::MetadataPageSnapshot;
use ssr_snapshot::SsrSnapshot;

pub struct LibraryService {
    pub(crate) query_cache: query::QueryCache,
    pub(super) systems: RwLock<Option<Vec<SystemSummary>>>,
    pub(super) favorites: RwLock<Option<FavoritesCache>>,
    pub(super) recents: RwLock<Option<Vec<RecentEntry>>>,
    /// In-memory snapshot of the `/settings/metadata` page payload. Backed
    /// by the generic `SsrSnapshot<T>` helper so future SSR pages can opt
    /// into the same single-flight + stale-on-`None` semantics with one
    /// new field + one accessor (see `ssr_snapshot.rs`).
    pub(super) metadata_page: SsrSnapshot<MetadataPageSnapshot>,
    /// In-memory snapshot of the home-page recommendation payload.
    /// Replaces the previous TtlSlot — strictly better caching (event-
    /// driven invalidation, single-flight rebuild, stale-on-`None`).
    pub(super) recommendations: SsrSnapshot<RecommendationData>,
}

impl LibraryService {
    pub(crate) fn new() -> Self {
        let query_cache = query::QueryCache::new();
        Self {
            systems: RwLock::new(None),
            favorites: RwLock::new(None),
            recents: RwLock::new(None),
            metadata_page: SsrSnapshot::new(),
            recommendations: SsrSnapshot::new(),
            query_cache,
        }
    }

    /// Get the metadata-page snapshot, rebuilding on miss via the generic
    /// `SsrSnapshot<T>` helper (single-flight RwLock + double-check, with
    /// stale-on-`None` so the page keeps rendering when the DB is briefly
    /// unavailable — e.g. write gate on non-WAL FS).
    pub async fn metadata_page_snapshot(&self, state: &super::AppState) -> MetadataPageSnapshot {
        self.metadata_page
            .get_or_compute("metadata_page_snapshot", || async {
                metadata_snapshot::compute(state).await
            })
            .await
    }

    /// Invalidate just the metadata-page snapshot. Hooked into the same
    /// write-completion sites that already invalidate the other caches.
    pub async fn invalidate_metadata_page(&self) {
        self.metadata_page.invalidate().await;
    }

    /// Get the home-page recommendations snapshot, rebuilding on miss.
    /// Cold case returns `RecommendationData::default()` (empty carousels)
    /// instantly — same UX as an empty library — while a background
    /// rebuild populates the snapshot. Subsequent callers get the cached
    /// data until the next invalidation.
    pub async fn recommendations_snapshot(&self, state: &super::AppState) -> RecommendationData {
        self.recommendations
            .get_or_compute("recommendations_snapshot", || async {
                recommendations_snapshot::compute(state).await
            })
            .await
    }

    /// Invalidate the recommendations snapshot. Routed through the same
    /// post-write helper that invalidates the other user-facing caches
    /// (`AppState::invalidate_user_caches`).
    pub async fn invalidate_recommendations(&self) {
        self.recommendations.invalidate().await;
    }

    /// Cached systems list from L1 (in-memory) → L2 (`game_library_meta`).
    /// Read-only: never falls through to a filesystem scan and never
    /// writes to L2. Discovery and persistence are owned by the
    /// background pipeline. See `docs/architecture/database-schema.md`.
    ///
    /// Single-flight on miss; misses and unavailable-pool states return
    /// an empty vec without caching so the next call retries.
    pub async fn cached_systems(
        &self,
        _storage: &StorageLocation,
        db: &LibraryReadPool,
    ) -> Vec<SystemSummary> {
        if let Some(ref cached) = *self.systems.read().await {
            return cached.clone();
        }

        let mut guard = self.systems.write().await;
        if let Some(ref cached) = *guard {
            return cached.clone();
        }

        match self.load_systems_from_db(db).await {
            Some(summaries) if !summaries.is_empty() => {
                *guard = Some(summaries.clone());
                summaries
            }
            _ => Vec::new(),
        }
    }

    /// Reconstruct the SystemSummary list from `game_library_meta`.
    /// `None` = pool unavailable (closed / write-gated / SQL error);
    /// `Some(empty)` = DB reachable, no rows yet; `Some(non-empty)` = hit.
    async fn load_systems_from_db(&self, db: &LibraryReadPool) -> Option<Vec<SystemSummary>> {
        use replay_control_core::systems;

        let cached_meta = db.read(LibraryDb::load_all_system_meta).await?;
        let cached_meta = cached_meta.ok()?;

        if cached_meta.is_empty() {
            return Some(Vec::new());
        }

        // Build a lookup map from cached data.
        let meta_map: HashMap<String, &replay_control_core_server::library_db::SystemMeta> =
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

        // Sort: systems with games first, then alphabetically.
        summaries.sort_by(|a, b| {
            let a_has = a.game_count > 0;
            let b_has = b.game_count > 0;
            b_has.cmp(&a_has).then(a.display_name.cmp(&b.display_name))
        });

        Some(summaries)
    }

    /// Strict-reconcile per-system scan + write-through to L2.
    ///
    /// One rule: a successful filesystem read replaces L2 for that system;
    /// a failed filesystem read returns `Err` and preserves existing L2.
    ///
    /// - `Ok(non-empty)` → replace rows + meta with the scan result.
    /// - `Ok(empty)` → reconcile to empty (delete rows, upsert meta with
    ///   `rom_count=0`). A successful empty walk is a real reconcile, not
    ///   a failure.
    /// - `Err` → callers preserve existing L2 state for that system.
    ///
    /// Missing top-level system directory:
    /// - Local storage: treat as a successful empty scan (user-initiated
    ///   deletion); reconcile to empty.
    /// - NFS: treat as ambiguous storage failure; return `Err` so callers
    ///   preserve cached state.
    pub async fn scan_and_cache_system(
        &self,
        storage: &StorageLocation,
        system: &str,
        region_pref: RegionPreference,
        region_secondary: Option<RegionPreference>,
        db: &LibraryWritePool,
    ) -> Result<Arc<Vec<RomEntry>>, replay_control_core::error::Error> {
        self.scan_and_cache_system_with_inputs(
            storage,
            system,
            region_pref,
            region_secondary,
            db,
            &ScanInputs::default(),
        )
        .await
    }

    pub(crate) async fn scan_and_cache_system_with_inputs(
        &self,
        storage: &StorageLocation,
        system: &str,
        region_pref: RegionPreference,
        region_secondary: Option<RegionPreference>,
        db: &LibraryWritePool,
        scan_inputs: &ScanInputs,
    ) -> Result<Arc<Vec<RomEntry>>, replay_control_core::error::Error> {
        scan_inputs.ensure_current()?;
        let system_dir = storage.roms_dir().join(system);

        // NFS missing dir is ambiguous (transient mount blip vs remote
        // delete) — preserve L2. Local missing dir is a user-initiated
        // deletion: fall through to `list_roms`, which returns
        // `Ok(empty)` and reconciles to zero per the strict rule.
        let exists = system_dir.try_exists().map_err(|e| {
            replay_control_core::error::Error::Other(format!(
                "reconcile scan skipped for {system}: could not check {}: {e}",
                system_dir.display()
            ))
        })?;
        if !exists && !storage.kind.is_local() {
            return Err(replay_control_core::error::Error::Other(format!(
                "reconcile scan skipped for {system}: top-level system dir missing on NFS storage (preserving cached state)"
            )));
        }

        tracing::debug!("L3 reconcile scan for {system}: starting filesystem scan");
        let total_started = Instant::now();
        let list_started = Instant::now();
        let mut roms = match replay_control_core_server::roms::list_roms(
            storage,
            system,
            region_pref,
            region_secondary,
        )
        .await
        {
            Ok(roms) => roms,
            Err(e) => {
                tracing::warn!(
                    "L2 scan profile: {system}: list failed after {}ms: {e}",
                    list_started.elapsed().as_millis()
                );
                return Err(e);
            }
        };
        let list_ms = list_started.elapsed().as_millis();
        tracing::debug!("L3 reconcile scan for {system}: found {} ROMs", roms.len());
        scan_inputs.ensure_current()?;

        let hash_started = Instant::now();
        let hash_results = self
            .hash_roms_for_system(storage, system, &mut roms, scan_inputs)
            .await;
        let hash_ms = hash_started.elapsed().as_millis();
        scan_inputs.ensure_current()?;

        let arc = Arc::new(roms);

        let save_started = Instant::now();
        let save_result = self
            .save_roms_to_db(
                storage,
                system,
                &arc,
                &system_dir,
                &hash_results,
                region_pref,
                region_secondary,
                db,
                scan_inputs,
            )
            .await;
        let save_ms = save_started.elapsed().as_millis();
        if let Err(e) = save_result {
            tracing::warn!(
                "L2 scan profile: {system}: save failed after {save_ms}ms (roms={}, list_ms={list_ms}, hash_ms={hash_ms}): {e}",
                arc.len()
            );
            return Err(e);
        }

        tracing::info!(
            "L2 scan profile: {system}: roms={} list_ms={list_ms} hash_ms={hash_ms} save_ms={save_ms} total_ms={}",
            arc.len(),
            total_started.elapsed().as_millis()
        );

        Ok(arc)
    }

    /// Try to load ROMs from SQLite game_library, validating via mtime.
    pub(crate) async fn load_roms_from_db(
        &self,
        _storage: &StorageLocation,
        system: &str,
        system_dir: &Path,
        db: &LibraryReadPool,
    ) -> Option<Vec<RomEntry>> {
        use replay_control_core_server::library_db::SystemMeta;

        let sys = system.to_string();
        let meta: SystemMeta = db
            .read(move |conn| LibraryDb::load_system_meta(conn, &sys))
            .await?
            .ok()??;

        // No cached ROMs? Skip L2.
        if meta.rom_count == 0 {
            return None;
        }

        // Check mtime freshness.
        let current_mtime_secs = dir_mtime_secs(system_dir);

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
        let cached_roms = db
            .read(move |conn| LibraryDb::load_system_entries(conn, &sys))
            .await?
            .ok()?;

        if cached_roms.is_empty() && meta.rom_count > 0 {
            // Meta says we have ROMs but game_library is empty — need L3 scan.
            return None;
        }

        // Convert GameEntry -> RomEntry.
        let roms: Vec<RomEntry> = cached_roms
            .into_iter()
            .map(|cr| {
                use replay_control_core_server::game_ref::GameRef;
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
    ///
    /// Invalidated explicitly by the launch server_fn and by the inotify
    /// watcher on `_recents/` changes. Single-flight on miss.
    pub async fn get_recents(
        &self,
        storage: &StorageLocation,
    ) -> Result<Vec<RecentEntry>, replay_control_core::error::Error> {
        if let Some(ref cached) = *self.recents.read().await {
            return Ok(cached.clone());
        }

        let mut guard = self.recents.write().await;
        if let Some(ref cached) = *guard {
            return Ok(cached.clone());
        }

        let entries = replay_control_core_server::recents::list_recents(storage).await?;
        *guard = Some(entries.clone());
        Ok(entries)
    }

    /// Invalidate L1 (in-memory) caches only — does NOT touch the L2
    /// (SQLite) game_library tables. Use this for non-destructive flows like
    /// rescan that reconcile rows in place without clearing the whole
    /// library. Destructive flows should call `invalidate()` instead.
    pub async fn invalidate_l1(&self) {
        *self.systems.write().await = None;
        *self.favorites.write().await = None;
        *self.recents.write().await = None;
        self.metadata_page.invalidate().await;
        self.query_cache.invalidate_all();
    }

    /// Invalidate all caches (after delete, rename, upload). Clears L1
    /// in-memory caches *and* L2 (SQLite). Returns `Ok(())` only if the
    /// L2 clear actually ran — caller-driven destructive flows (rebuild,
    /// re-import) must propagate the error rather than proceeding to write
    /// over a not-actually-cleared table.
    pub async fn invalidate(&self, db: &LibraryWritePool) -> Result<(), DbError> {
        self.invalidate_l1().await;
        db.try_write(|conn| LibraryDb::clear_all_game_library(conn))
            .await?
            .map_err(|e| DbError::Other(format!("clear_all_game_library: {e}")))
    }

    /// Invalidate cache for a specific system. Same semantics as
    /// `invalidate()` — typed error so destructive callers can detect a
    /// no-op clear.
    pub async fn invalidate_system(
        &self,
        system: String,
        db: &LibraryWritePool,
    ) -> Result<(), DbError> {
        *self.systems.write().await = None;
        self.metadata_page.invalidate().await;
        self.query_cache.invalidate_all();
        let sys = system.clone();
        db.try_write(move |conn| LibraryDb::clear_system_game_library(conn, &sys))
            .await?
            .map_err(|e| DbError::Other(format!("clear_system_game_library({system}): {e}")))
    }

    /// Invalidate only the favorites cache (after add/remove favorite).
    pub async fn invalidate_favorites(&self) {
        *self.favorites.write().await = None;
    }

    /// Invalidate only the recents cache (after launch creates a new entry).
    pub async fn invalidate_recents(&self) {
        *self.recents.write().await = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use replay_control_core::game_ref::GameRef;
    use replay_control_core::rom_tags::RegionPreference;
    use replay_control_core_server::rom_hash::{CachedHash, HashResult};
    use replay_control_core_server::roms::RomEntry;
    use replay_control_core_server::test_utils::build_library_pool;
    use replay_control_core_server::{library_db::LibraryDb, storage::StorageKind};
    use std::sync::atomic::AtomicU64;

    /// `cached_systems` is strictly read-only: an empty `game_library_meta`
    /// must return an empty list rather than fall through to a filesystem
    /// scan. Discovering and persisting systems is the job of the
    /// background pipeline (`populate_all_systems` walks `visible_systems()`
    /// per-system) — letting a request handler do it was the cold-NFS
    /// poisoning vector traced in the write-isolation investigation.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cached_systems_returns_empty_on_empty_db_without_writing() {
        use replay_control_core_server::storage::{StorageKind, StorageLocation};
        use std::path::Path;

        // Populated roms_dir so a hypothetical L3 fallback would find ROMs
        // (we then assert it does NOT and the DB stays empty).
        let tmp = tempfile::tempdir().unwrap();
        let roms = tmp.path().join("roms");
        std::fs::create_dir_all(roms.join("nintendo_nes")).unwrap();
        std::fs::write(roms.join("nintendo_nes/Game.nes"), b"x").unwrap();

        // Empty library DB (no game_library_meta rows, no game_library rows).
        let db_path = tmp.path().join("library.db");
        fn opener(
            db_path: &Path,
        ) -> replay_control_core::error::Result<
            replay_control_core_server::db_pool::rusqlite::Connection,
        > {
            let conn = replay_control_core_server::sqlite::open_connection(db_path, "test_lib")
                .map_err(|e| replay_control_core::error::Error::Other(format!("open: {e}")))?;
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS game_library_meta (
                    system TEXT PRIMARY KEY,
                    dir_mtime_secs INTEGER,
                    scanned_at INTEGER NOT NULL,
                    rom_count INTEGER NOT NULL,
                    total_size_bytes INTEGER NOT NULL
                );",
            )
            .map_err(|e| replay_control_core::error::Error::Other(format!("schema: {e}")))?;
            Ok(conn)
        }
        let _ = opener(&db_path).unwrap();
        let pool = replay_control_core_server::db_pool::DbPool::new(db_path, "test_lib", opener, 1)
            .unwrap();
        let reader = LibraryReadPool::from_pool(pool);

        let storage = StorageLocation::from_path(tmp.path().to_path_buf(), StorageKind::Sd);
        let svc = LibraryService::new();

        let summaries = svc.cached_systems(&storage, &reader).await;
        assert!(
            summaries.is_empty(),
            "cached_systems must not fall through to L3 scan; got {summaries:?}"
        );

        // Confirm no rows were persisted.
        let meta = reader
            .read(LibraryDb::load_all_system_meta)
            .await
            .unwrap()
            .unwrap();
        assert!(
            meta.is_empty(),
            "cached_systems must not write to game_library_meta from a read path"
        );
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reconcile_scan_replaces_system_rows_when_roms_are_removed() {
        use replay_control_core_server::storage::StorageLocation;

        let (pool, _db_tmp) = build_library_pool();
        let reader = LibraryReadPool::from_pool(pool.clone());
        let writer = LibraryWritePool::from_pool(pool);
        let storage_tmp = tempfile::tempdir().unwrap();
        let roms_dir = storage_tmp.path().join("roms").join("nintendo_nes");
        std::fs::create_dir_all(&roms_dir).unwrap();
        let rom_path = roms_dir.join("Game.nes");
        std::fs::write(&rom_path, b"rom").unwrap();

        let storage = StorageLocation::from_path(storage_tmp.path().to_path_buf(), StorageKind::Sd);
        let svc = LibraryService::new();

        svc.scan_and_cache_system(
            &storage,
            "nintendo_nes",
            RegionPreference::default(),
            None,
            &writer,
        )
        .await
        .unwrap();

        let filenames = reader
            .read(|conn| LibraryDb::visible_filenames(conn, "nintendo_nes"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(filenames, vec!["Game.nes".to_string()]);

        std::fs::remove_file(&rom_path).unwrap();

        svc.scan_and_cache_system(
            &storage,
            "nintendo_nes",
            RegionPreference::default(),
            None,
            &writer,
        )
        .await
        .unwrap();

        let filenames = reader
            .read(|conn| LibraryDb::visible_filenames(conn, "nintendo_nes"))
            .await
            .unwrap()
            .unwrap();
        assert!(
            filenames.is_empty(),
            "reconcile rescan should drop removed ROMs"
        );

        let meta = reader
            .read(LibraryDb::load_all_system_meta)
            .await
            .unwrap()
            .unwrap();
        let nes = meta
            .iter()
            .find(|row| row.system == "nintendo_nes")
            .expect("system meta row should remain present");
        assert_eq!(nes.rom_count, 0);
        assert_eq!(nes.total_size_bytes, 0);
    }

    /// Local storage (SD/USB/NVMe): a missing top-level system dir means
    /// the user deleted the folder. Reconcile to empty so the cache
    /// matches disk — phantom ROMs that can't be launched would be a
    /// worse UX than an honest empty list.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reconcile_local_missing_dir_with_cached_rows_drops_to_empty() {
        use replay_control_core_server::storage::StorageLocation;

        let (pool, _db_tmp) = build_library_pool();
        let reader = LibraryReadPool::from_pool(pool.clone());
        let writer = LibraryWritePool::from_pool(pool);
        let storage_tmp = tempfile::tempdir().unwrap();
        let roms_dir = storage_tmp.path().join("roms").join("nintendo_nes");
        std::fs::create_dir_all(&roms_dir).unwrap();
        let rom_path = roms_dir.join("Game.nes");
        std::fs::write(&rom_path, b"rom").unwrap();

        let storage = StorageLocation::from_path(storage_tmp.path().to_path_buf(), StorageKind::Sd);
        let svc = LibraryService::new();

        // Seed: scan once so meta + rows exist for nintendo_nes.
        svc.scan_and_cache_system(
            &storage,
            "nintendo_nes",
            RegionPreference::default(),
            None,
            &writer,
        )
        .await
        .unwrap();

        // Remove the entire system folder (simulates user deletion via
        // file manager / SSH / share unmount).
        std::fs::remove_dir_all(&roms_dir).unwrap();

        // Strict reconcile on local storage: missing dir → Ok(empty) →
        // reconcile to empty.
        let arc = svc
            .scan_and_cache_system(
                &storage,
                "nintendo_nes",
                RegionPreference::default(),
                None,
                &writer,
            )
            .await
            .expect("local missing-dir must succeed (Ok(empty))");
        assert!(arc.is_empty(), "scan returned roms for a missing dir");

        // Rows are gone, meta updated to rom_count=0.
        let filenames = reader
            .read(|conn| LibraryDb::visible_filenames(conn, "nintendo_nes"))
            .await
            .unwrap()
            .unwrap();
        assert!(filenames.is_empty(), "cached rows should be dropped");

        let meta = reader
            .read(LibraryDb::load_all_system_meta)
            .await
            .unwrap()
            .unwrap();
        let nes = meta
            .iter()
            .find(|row| row.system == "nintendo_nes")
            .expect("meta row should still exist (now reflecting empty)");
        assert_eq!(nes.rom_count, 0);
    }

    /// NFS storage: a missing top-level system dir is ambiguous (could be
    /// transient mount blip or remote-side delete). Strict reconcile
    /// returns `Err` so cached state is preserved — the
    /// failure-preserves-L2 contract.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reconcile_nfs_missing_dir_returns_err_and_preserves_cache() {
        use replay_control_core_server::storage::StorageLocation;

        let (pool, _db_tmp) = build_library_pool();
        let reader = LibraryReadPool::from_pool(pool.clone());
        let writer = LibraryWritePool::from_pool(pool);
        let storage_tmp = tempfile::tempdir().unwrap();
        let roms_dir = storage_tmp.path().join("roms").join("nintendo_nes");
        std::fs::create_dir_all(&roms_dir).unwrap();
        let rom_path = roms_dir.join("Game.nes");
        std::fs::write(&rom_path, b"rom").unwrap();

        // Seed via local storage first (NFS path needs the dir to exist
        // for the seed scan); we'll switch to NFS-flavored storage for
        // the second call.
        let storage_local =
            StorageLocation::from_path(storage_tmp.path().to_path_buf(), StorageKind::Sd);
        let svc = LibraryService::new();
        svc.scan_and_cache_system(
            &storage_local,
            "nintendo_nes",
            RegionPreference::default(),
            None,
            &writer,
        )
        .await
        .unwrap();

        // Remove the dir, switch storage kind to NFS, reconcile again.
        std::fs::remove_dir_all(&roms_dir).unwrap();
        let storage_nfs =
            StorageLocation::from_path(storage_tmp.path().to_path_buf(), StorageKind::Nfs);

        let err = svc
            .scan_and_cache_system(
                &storage_nfs,
                "nintendo_nes",
                RegionPreference::default(),
                None,
                &writer,
            )
            .await
            .expect_err("NFS missing-dir must return Err to preserve cached state");
        assert!(
            err.to_string().contains("NFS storage"),
            "unexpected error: {err}"
        );

        // Cached rows + meta unchanged.
        let filenames = reader
            .read(|conn| LibraryDb::visible_filenames(conn, "nintendo_nes"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(filenames, vec!["Game.nes".to_string()]);

        let meta = reader
            .read(LibraryDb::load_all_system_meta)
            .await
            .unwrap()
            .unwrap();
        let nes = meta
            .iter()
            .find(|row| row.system == "nintendo_nes")
            .expect("system meta row should remain present");
        assert_eq!(nes.rom_count, 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn migrated_hash_cache_self_heals_hash_size_on_scan_save() {
        use replay_control_core_server::storage::StorageLocation;

        let (pool, _db_tmp) = build_library_pool();
        let reader = LibraryReadPool::from_pool(pool.clone());
        let writer = LibraryWritePool::from_pool(pool);
        let storage_tmp = tempfile::tempdir().unwrap();
        let roms_dir = storage_tmp.path().join("roms").join("nintendo_snes");
        std::fs::create_dir_all(&roms_dir).unwrap();
        let rom_path = roms_dir.join("Mario.sfc");
        std::fs::write(&rom_path, b"rom").unwrap();
        let mtime_secs = std::fs::metadata(&rom_path)
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let mut cached_hashes = HashMap::new();
        cached_hashes.insert(
            "Mario.sfc".to_string(),
            CachedHash {
                crc32: 0x1234_ABCD,
                hash_mtime: mtime_secs,
                hash_size_bytes: None,
                matched_name: Some("Super Mario World (USA)".to_string()),
            },
        );
        let scan_inputs = ScanInputs::new(
            cached_hashes,
            ScanOptions {
                force_rehash: false,
            },
            None,
        );

        let storage = StorageLocation::from_path(storage_tmp.path().to_path_buf(), StorageKind::Sd);
        let svc = LibraryService::new();
        svc.scan_and_cache_system_with_inputs(
            &storage,
            "nintendo_snes",
            RegionPreference::default(),
            None,
            &writer,
            &scan_inputs,
        )
        .await
        .unwrap();

        let hashes = reader
            .read(|conn| LibraryDb::load_cached_hashes(conn, "nintendo_snes"))
            .await
            .unwrap()
            .unwrap();
        let cached = hashes
            .get("Mario.sfc")
            .expect("cached hash should round-trip after scan save");
        assert_eq!(cached.crc32, 0x1234_ABCD);
        assert_eq!(cached.hash_size_bytes, Some(3));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stale_storage_generation_prevents_scan_save_write() {
        use replay_control_core_server::storage::StorageLocation;

        let (pool, _db_tmp) = build_library_pool();
        let reader = LibraryReadPool::from_pool(pool.clone());
        let writer = LibraryWritePool::from_pool(pool);
        let storage_tmp = tempfile::tempdir().unwrap();
        let roms_dir = storage_tmp.path().join("roms").join("nintendo_snes");
        std::fs::create_dir_all(&roms_dir).unwrap();
        let seed_path = roms_dir.join("Seed.sfc");
        std::fs::write(&seed_path, b"seed").unwrap();

        let storage = StorageLocation::from_path(storage_tmp.path().to_path_buf(), StorageKind::Sd);
        let svc = LibraryService::new();
        svc.scan_and_cache_system(
            &storage,
            "nintendo_snes",
            RegionPreference::default(),
            None,
            &writer,
        )
        .await
        .unwrap();

        let current_generation = Arc::new(AtomicU64::new(2));
        let stale_inputs = ScanInputs::new(
            HashMap::new(),
            ScanOptions::default(),
            Some(ScanCancellation::new(current_generation, 1)),
        );
        let roms = vec![RomEntry {
            game: GameRef::from_parts(
                "nintendo_snes",
                "Replacement.sfc".to_string(),
                "/roms/nintendo_snes/Replacement.sfc".to_string(),
                None,
            ),
            size_bytes: 11,
            is_m3u: false,
            is_favorite: false,
            box_art_url: None,
            driver_status: None,
            rating: None,
            players: None,
        }];
        let mut hash_results = HashMap::new();
        hash_results.insert(
            "Replacement.sfc".to_string(),
            HashResult {
                rom_filename: "Replacement.sfc".to_string(),
                crc32: 0xDEAD_BEEF,
                mtime_secs: 123,
                size_bytes: 11,
                matched_name: None,
            },
        );

        let err = svc
            .save_roms_to_db(
                &storage,
                "nintendo_snes",
                &roms,
                &roms_dir,
                &hash_results,
                RegionPreference::default(),
                None,
                &writer,
                &stale_inputs,
            )
            .await
            .expect_err("stale generation should cancel before DB writes");
        assert!(matches!(
            err,
            replay_control_core::error::Error::StorageChanged
        ));

        let filenames = reader
            .read(|conn| LibraryDb::visible_filenames(conn, "nintendo_snes"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(filenames, vec!["Seed.sfc".to_string()]);
    }

    /// File-level deletion (the realistic RePlayOS deletion case): user
    /// empties `roms/<sys>/` of ROM files but the folder remains (RePlayOS
    /// auto-recreates system folders at boot, so user-visible deletion is
    /// almost always at file level rather than folder level). Reconcile
    /// must drop rows and update meta to `rom_count=0`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn reconcile_file_level_deletion_reconciles_to_empty() {
        use replay_control_core_server::storage::StorageLocation;

        let (pool, _db_tmp) = build_library_pool();
        let reader = LibraryReadPool::from_pool(pool.clone());
        let writer = LibraryWritePool::from_pool(pool);
        let storage_tmp = tempfile::tempdir().unwrap();
        let roms_dir = storage_tmp.path().join("roms").join("nintendo_nes");
        std::fs::create_dir_all(&roms_dir).unwrap();
        let rom_path = roms_dir.join("Game.nes");
        std::fs::write(&rom_path, b"rom").unwrap();

        let storage = StorageLocation::from_path(storage_tmp.path().to_path_buf(), StorageKind::Sd);
        let svc = LibraryService::new();

        // Seed scan: cached rows + meta with rom_count=1.
        svc.scan_and_cache_system(
            &storage,
            "nintendo_nes",
            RegionPreference::default(),
            None,
            &writer,
        )
        .await
        .unwrap();

        // Remove ROM files only — folder remains (mirrors RePlayOS
        // production: user deletes ROMs via file manager but the system
        // folder structure stays in place).
        std::fs::remove_file(&rom_path).unwrap();
        assert!(roms_dir.exists(), "folder must remain for this scenario");

        // Reconcile: walk returns Ok(empty) → reconcile-to-empty.
        let arc = svc
            .scan_and_cache_system(
                &storage,
                "nintendo_nes",
                RegionPreference::default(),
                None,
                &writer,
            )
            .await
            .expect("Ok(empty) walk should succeed");
        assert!(arc.is_empty());

        let filenames = reader
            .read(|conn| LibraryDb::visible_filenames(conn, "nintendo_nes"))
            .await
            .unwrap()
            .unwrap();
        assert!(filenames.is_empty(), "cached rows should be dropped");

        let meta = reader
            .read(LibraryDb::load_all_system_meta)
            .await
            .unwrap()
            .unwrap();
        let nes = meta
            .iter()
            .find(|row| row.system == "nintendo_nes")
            .expect("meta row should remain (now reflecting empty)");
        assert_eq!(nes.rom_count, 0);
    }
}
