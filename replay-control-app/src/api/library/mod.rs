mod aliases;
mod enrichment;
mod favorites;
pub mod metadata_snapshot;
pub(crate) mod query;
mod scan_pipeline;
pub mod ssr_snapshot;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

use replay_control_core::rom_tags::RegionPreference;
use replay_control_core_server::library_db::LibraryDb;
use replay_control_core_server::recents::RecentEntry;
use replay_control_core_server::roms::{RomEntry, SystemSummary};
use replay_control_core_server::storage::StorageLocation;
use tokio::sync::RwLock;

use super::DbPool;

/// Compute the max mtime across a directory and its immediate subdirectories
/// (maxdepth 2). This detects changes inside organizational subdirectories
/// like `00 Clean Romset/` without the cost of a full recursive scan.
///
/// Blocking — only called from within `db.write(|conn| ...)` closures, which
/// already run on a deadpool blocking thread.
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
}

impl LibraryService {
    pub(crate) fn new() -> Self {
        let query_cache = query::QueryCache::new();
        Self {
            systems: RwLock::new(None),
            favorites: RwLock::new(None),
            recents: RwLock::new(None),
            metadata_page: SsrSnapshot::new(),
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

    /// Get cached systems or scan and cache.
    /// L1 (in-memory) → L2 (SQLite game_library_meta) → L3 (filesystem scan).
    ///
    /// Single-flight on miss: concurrent callers acquire the write lock and
    /// re-check before rebuilding, so only the first arrival performs L2/L3.
    pub async fn cached_systems(
        &self,
        storage: &StorageLocation,
        db: &DbPool,
    ) -> Vec<SystemSummary> {
        if let Some(ref cached) = *self.systems.read().await {
            return cached.clone();
        }

        let mut guard = self.systems.write().await;
        if let Some(ref cached) = *guard {
            return cached.clone();
        }

        // L2: Try SQLite game_library_meta. `Some(non-empty)` is a hit;
        // `Some(empty)` means the DB is reachable but has no systems cached
        // (true cache miss → fall through to L3); `None` means the pool was
        // unavailable (closed, or briefly write-gated). In the unavailable
        // case we deliberately skip L3 — kicking off a multi-thousand-ROM
        // filesystem scan because of a transient DB-unavailable would pin
        // the L1 write lock for minutes and starve every concurrent SSR
        // request that needs the systems list.
        match self.load_systems_from_db(storage, db).await {
            Some(summaries) if !summaries.is_empty() => {
                *guard = Some(summaries.clone());
                summaries
            }
            Some(_) => {
                // L3: full filesystem scan and write-through to L2.
                // On a racy NFS / autofs / USB hot-plug, scan_systems may
                // return Err (storage not ready). Don't cache — let the next
                // call retry once storage settles. Don't write-through either:
                // the DB-level zero-overwrite guard would normally save us,
                // but on a fresh DB there's no existing row to protect, so
                // a partially-mounted scan would still poison persistent state.
                match replay_control_core_server::roms::scan_systems(storage).await {
                    Ok(summaries) => {
                        *guard = Some(summaries.clone());
                        drop(guard);
                        self.save_systems_to_db(storage, &summaries, db).await;
                        summaries
                    }
                    Err(e) => {
                        tracing::warn!(
                            "cached_systems: L3 scan rejected ({e}); not caching, will retry on next call"
                        );
                        Vec::new()
                    }
                }
            }
            None => {
                // DB unavailable; return empty without caching so the next
                // caller retries once the pool is reachable.
                Vec::new()
            }
        }
    }

    /// Try to reconstruct SystemSummary list from SQLite game_library_meta.
    async fn load_systems_from_db(
        &self,
        _storage: &StorageLocation,
        db: &DbPool,
    ) -> Option<Vec<SystemSummary>> {
        use replay_control_core::systems;

        let cached_meta = db.read(LibraryDb::load_all_system_meta).await?;
        let cached_meta = cached_meta.ok()?;

        if cached_meta.is_empty() {
            return None;
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

        // Sort: systems with games first, then alphabetically (same as scan_systems).
        summaries.sort_by(|a, b| {
            let a_has = a.game_count > 0;
            let b_has = b.game_count > 0;
            b_has.cmp(&a_has).then(a.display_name.cmp(&b.display_name))
        });

        Some(summaries)
    }

    /// Write system summaries to SQLite game_library_meta.
    ///
    /// `save_system_meta` itself enforces the zero-overwrite guard at SQL
    /// level. We additionally log a warning here when the guard fires so a
    /// racy scan is *visible* in the journal rather than silently absorbed.
    async fn save_systems_to_db(
        &self,
        storage: &StorageLocation,
        summaries: &[SystemSummary],
        db: &DbPool,
    ) {
        let roms_dir = storage.roms_dir();
        let summaries: Vec<_> = summaries.to_vec();
        db.write(move |conn| {
            for summary in &summaries {
                let system_dir = roms_dir.join(&summary.folder_name);
                let mtime_secs = dir_mtime(&system_dir).and_then(|t| {
                    t.duration_since(std::time::UNIX_EPOCH)
                        .ok()
                        .map(|d| d.as_secs() as i64)
                });
                match LibraryDb::save_system_meta(
                    conn,
                    &summary.folder_name,
                    mtime_secs,
                    summary.game_count,
                    summary.total_size_bytes,
                ) {
                    Ok(stored) if stored != summary.game_count => {
                        tracing::warn!(
                            "Refusing to overwrite system {} rom_count: existing={}, scanned=0 (likely scan-time race)",
                            summary.folder_name,
                            stored,
                        );
                    }
                    Ok(_) => {}
                    Err(e) => tracing::warn!(
                        "Failed to save system meta for {}: {e}",
                        summary.folder_name
                    ),
                }
            }
        })
        .await;
    }

    /// Scan a system from filesystem and write to L2 (SQLite).
    /// Called by the background pipeline during warmup and by REST API on L2 miss.
    pub async fn scan_and_cache_system(
        &self,
        storage: &StorageLocation,
        system: &str,
        region_pref: RegionPreference,
        region_secondary: Option<RegionPreference>,
        db: &DbPool,
    ) -> Result<Arc<Vec<RomEntry>>, replay_control_core::error::Error> {
        let system_dir = storage.roms_dir().join(system);

        tracing::debug!("L3 scan for {system}: starting filesystem scan");
        let mut roms = replay_control_core_server::roms::list_roms(
            storage,
            system,
            region_pref,
            region_secondary,
        )
        .await?;
        tracing::debug!("L3 scan for {system}: found {} ROMs", roms.len());

        // Hash-and-identify step: for hash-eligible systems, compute CRC32 hashes
        // and look up canonical names in the embedded No-Intro DAT data.
        let hash_results = self
            .hash_roms_for_system(storage, system, &mut roms, db)
            .await;

        let arc = Arc::new(roms);

        // Write to L2.
        self.save_roms_to_db(
            storage,
            system,
            &arc,
            &system_dir,
            &hash_results,
            region_pref,
            region_secondary,
            db,
        )
        .await;

        Ok(arc)
    }

    /// Try to load ROMs from SQLite game_library, validating via mtime.
    pub(crate) async fn load_roms_from_db(
        &self,
        _storage: &StorageLocation,
        system: &str,
        system_dir: &Path,
        db: &DbPool,
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

    /// Invalidate all caches (after delete, rename, upload).
    /// Clears L1 in-memory caches and L2 (SQLite game_library).
    pub async fn invalidate(&self, db: &DbPool) {
        *self.systems.write().await = None;
        *self.favorites.write().await = None;
        *self.recents.write().await = None;
        self.metadata_page.invalidate().await;
        self.query_cache.invalidate_all();
        // L2: Clear SQLite game_library.
        db.write(|conn| {
            if let Err(e) = LibraryDb::clear_all_game_library(conn) {
                tracing::error!("Failed to clear game library: {e}");
            }
        })
        .await;
    }

    /// Invalidate cache for a specific system.
    /// Clears L1 systems cache and L2 (SQLite game_library) for the system.
    pub async fn invalidate_system(&self, system: String, db: &DbPool) {
        *self.systems.write().await = None;
        self.metadata_page.invalidate().await;
        self.query_cache.invalidate_all();
        // L2: Clear SQLite game_library for this system.
        db.write(move |conn| {
            if let Err(e) = LibraryDb::clear_system_game_library(conn, &system) {
                tracing::error!("Failed to clear game library for {system}: {e}");
            }
        })
        .await;
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
