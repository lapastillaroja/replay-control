use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use replay_control_core::metadata_db::MetadataDb;
use replay_control_core::recents::RecentEntry;
use replay_control_core::rom_tags::RegionPreference;
use replay_control_core::roms::{RomEntry, SystemSummary};
use replay_control_core::storage::StorageLocation;

/// Hard TTL — even if mtime hasn't changed, re-scan after this long.
const CACHE_HARD_TTL: Duration = Duration::from_secs(300);

/// Read the mtime of a directory (single stat call).
pub(crate) fn dir_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok().and_then(|m| m.modified().ok())
}

/// Cached result with mtime-based + hard-TTL invalidation.
struct CacheEntry<T> {
    data: T,
    dir_mtime: Option<SystemTime>,
    expires: Instant,
}

impl<T: Clone> CacheEntry<T> {
    fn new(data: T, dir: &Path) -> Self {
        Self {
            data,
            dir_mtime: dir_mtime(dir),
            expires: Instant::now() + CACHE_HARD_TTL,
        }
    }

    /// Check if cached data is still fresh.
    /// Fresh = hard TTL not expired AND directory mtime unchanged.
    fn is_fresh(&self, dir: &Path) -> bool {
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

/// In-memory cache for filesystem scan results.
/// Uses mtime-based invalidation (one stat call) with a hard TTL fallback.
/// Cached per-system image directory index for batch box art resolution.
/// Maps normalized base title → actual filename (without directory prefix).
pub struct ImageIndex {
    /// exact thumbnail_filename stem → "boxart/{filename}.png"
    pub exact: HashMap<String, String>,
    /// fuzzy base_title (lowercase, tags stripped) → "boxart/{filename}.png"
    pub fuzzy: HashMap<String, String>,
    /// version-stripped base_title → "boxart/{filename}.png"
    pub version: HashMap<String, String>,
    /// DB paths: rom_filename → "boxart/{path}"
    pub db_paths: HashMap<String, String>,
    dir_mtime: Option<SystemTime>,
    expires: Instant,
}

impl ImageIndex {
    fn is_fresh(&self, boxart_dir: &Path) -> bool {
        if Instant::now() >= self.expires {
            return false;
        }
        match (self.dir_mtime, dir_mtime(boxart_dir)) {
            (Some(cached), Some(current)) => cached == current,
            _ => true,
        }
    }
}

pub struct RomCache {
    systems: std::sync::RwLock<Option<CacheEntry<Vec<SystemSummary>>>>,
    roms: std::sync::RwLock<HashMap<String, CacheEntry<Vec<RomEntry>>>>,
    favorites: std::sync::RwLock<Option<FavoritesCache>>,
    recents: std::sync::RwLock<Option<CacheEntry<Vec<RecentEntry>>>>,
    /// Per-system image index for batch box art resolution.
    images: std::sync::RwLock<HashMap<String, ImageIndex>>,
    /// Shared reference to the metadata DB for L2 persistent cache.
    db: Arc<Mutex<Option<MetadataDb>>>,
}

/// Cached favorites: per-system set of favorited filenames.
struct FavoritesCache {
    /// system → set of ROM filenames that are favorited.
    data: HashMap<String, HashSet<String>>,
    dir_mtime: Option<SystemTime>,
    expires: Instant,
}

impl FavoritesCache {
    fn new(storage: &StorageLocation) -> Self {
        let favs_dir = storage.favorites_dir();
        let all_favs = replay_control_core::favorites::list_favorites(storage).unwrap_or_default();
        let mut data: HashMap<String, HashSet<String>> = HashMap::new();
        for fav in all_favs {
            data.entry(fav.game.system.clone())
                .or_default()
                .insert(fav.game.rom_filename.clone());
        }
        Self {
            data,
            dir_mtime: dir_mtime(&favs_dir),
            expires: Instant::now() + CACHE_HARD_TTL,
        }
    }

    fn is_fresh(&self, favs_dir: &Path) -> bool {
        if Instant::now() >= self.expires {
            return false;
        }
        match (self.dir_mtime, dir_mtime(favs_dir)) {
            (Some(cached), Some(current)) => cached == current,
            _ => true,
        }
    }
}

impl RomCache {
    pub(crate) fn new(db: Arc<Mutex<Option<MetadataDb>>>) -> Self {
        Self {
            systems: std::sync::RwLock::new(None),
            roms: std::sync::RwLock::new(HashMap::new()),
            favorites: std::sync::RwLock::new(None),
            recents: std::sync::RwLock::new(None),
            images: std::sync::RwLock::new(HashMap::new()),
            db,
        }
    }

    /// Try to open the DB if not yet open, then run a read-only closure.
    fn with_db<F, R>(&self, storage: &StorageLocation, f: F) -> Option<R>
    where
        F: FnOnce(&MetadataDb) -> R,
    {
        let mut guard = self.db.lock().ok()?;
        if guard.is_none() {
            match MetadataDb::open(&storage.root) {
                Ok(db) => *guard = Some(db),
                Err(e) => {
                    tracing::debug!("Could not open metadata DB for cache: {e}");
                    return None;
                }
            }
        }
        guard.as_ref().map(f)
    }

    /// Try to open the DB if not yet open, then run a mutable closure.
    fn with_db_mut<F, R>(&self, storage: &StorageLocation, f: F) -> Option<R>
    where
        F: FnOnce(&mut MetadataDb) -> R,
    {
        let mut guard = self.db.lock().ok()?;
        if guard.is_none() {
            match MetadataDb::open(&storage.root) {
                Ok(db) => *guard = Some(db),
                Err(e) => {
                    tracing::debug!("Could not open metadata DB for cache: {e}");
                    return None;
                }
            }
        }
        guard.as_mut().map(f)
    }

    /// Get cached systems or scan and cache.
    /// L1 (in-memory) → L2 (SQLite rom_cache_meta) → L3 (filesystem scan).
    pub fn get_systems(&self, storage: &StorageLocation) -> Vec<SystemSummary> {
        let roms_dir = storage.roms_dir();

        // L1: Try in-memory cache.
        if let Ok(guard) = self.systems.read() {
            if let Some(ref entry) = *guard {
                if entry.is_fresh(&roms_dir) {
                    return entry.data.clone();
                }
            }
        }

        // L2: Try SQLite rom_cache_meta (reconstructs SystemSummary from cached metadata).
        if let Some(summaries) = self.load_systems_from_db(storage) {
            if !summaries.is_empty() {
                // Store in L1.
                if let Ok(mut guard) = self.systems.write() {
                    *guard = Some(CacheEntry::new(summaries.clone(), &roms_dir));
                }
                return summaries;
            }
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

    /// Try to reconstruct SystemSummary list from SQLite rom_cache_meta.
    fn load_systems_from_db(&self, storage: &StorageLocation) -> Option<Vec<SystemSummary>> {
        use replay_control_core::systems;

        let cached_meta = self.with_db(storage, |db| db.load_all_system_meta())?;
        let cached_meta = cached_meta.ok()?;

        if cached_meta.is_empty() {
            return None;
        }

        // Build a lookup map from cached data.
        let meta_map: HashMap<String, &replay_control_core::metadata_db::CachedSystemMeta> =
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

    /// Write system summaries to SQLite rom_cache_meta.
    fn save_systems_to_db(&self, storage: &StorageLocation, summaries: &[SystemSummary]) {
        let roms_dir = storage.roms_dir();
        self.with_db(storage, |db| {
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
                let _ = db.save_system_meta(
                    &summary.folder_name,
                    mtime_secs,
                    summary.game_count,
                    summary.total_size_bytes,
                );
            }
        });
    }

    /// Get cached ROM list for a system, or scan and cache.
    /// L1 (in-memory) → L2 (SQLite rom_cache) → L3 (filesystem scan).
    pub fn get_roms(
        &self,
        storage: &StorageLocation,
        system: &str,
        region_pref: RegionPreference,
    ) -> Result<Vec<RomEntry>, replay_control_core::error::Error> {
        let key = system.to_string();
        let system_dir = storage.roms_dir().join(system);

        // L1: Try in-memory cache.
        if let Ok(guard) = self.roms.read() {
            if let Some(entry) = guard.get(&key) {
                if entry.is_fresh(&system_dir) {
                    return Ok(entry.data.clone());
                }
            }
        }

        // L2: Try SQLite rom_cache.
        if let Some(roms) = self.load_roms_from_db(storage, system, &system_dir) {
            // Store in L1.
            if let Ok(mut guard) = self.roms.write() {
                guard.insert(key, CacheEntry::new(roms.clone(), &system_dir));
            }
            return Ok(roms);
        }

        // L3: Cache miss — full filesystem scan.
        let roms = replay_control_core::roms::list_roms(storage, system, region_pref)?;
        if let Ok(mut guard) = self.roms.write() {
            guard.insert(key.clone(), CacheEntry::new(roms.clone(), &system_dir));
        }

        // Write-through to L2.
        self.save_roms_to_db(storage, system, &roms, &system_dir);

        Ok(roms)
    }

    /// Try to load ROMs from SQLite rom_cache, validating via mtime.
    fn load_roms_from_db(
        &self,
        storage: &StorageLocation,
        system: &str,
        system_dir: &Path,
    ) -> Option<Vec<RomEntry>> {
        use replay_control_core::metadata_db::CachedSystemMeta;

        let meta: CachedSystemMeta =
            self.with_db(storage, |db| db.load_system_meta(system))?.ok()??;

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
            .with_db(storage, |db| db.load_system_roms(system))?
            .ok()?;

        if cached_roms.is_empty() && meta.rom_count > 0 {
            // Meta says we have ROMs but rom_cache is empty — need L3 scan.
            return None;
        }

        // Convert CachedRom → RomEntry.
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

    /// Write ROM list to SQLite rom_cache for persistent storage.
    /// Enriches with genre/players from the baked-in game databases during write.
    fn save_roms_to_db(
        &self,
        storage: &StorageLocation,
        system: &str,
        roms: &[RomEntry],
        system_dir: &Path,
    ) {
        use replay_control_core::metadata_db::CachedRom;
        use replay_control_core::{arcade_db, game_db};
        use replay_control_core::systems::{self, SystemCategory};

        let mtime_secs = dir_mtime(system_dir).and_then(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|d| d.as_secs() as i64)
        });

        let is_arcade = systems::find_system(system)
            .is_some_and(|s| s.category == SystemCategory::Arcade);

        let cached_roms: Vec<CachedRom> = roms
            .iter()
            .map(|r| {
                let rom_filename = &r.game.rom_filename;
                let (genre, players_lookup) = if is_arcade {
                    let stem = rom_filename.strip_suffix(".zip").unwrap_or(rom_filename);
                    match arcade_db::lookup_arcade_game(stem) {
                        Some(info) => (
                            Some(info.normalized_genre.to_string()),
                            Some(info.players),
                        ),
                        None => (None, None),
                    }
                } else {
                    let stem = rom_filename
                        .rfind('.')
                        .map(|i| &rom_filename[..i])
                        .unwrap_or(rom_filename);
                    let entry = game_db::lookup_game(system, stem);
                    let game = entry.map(|e| e.game).or_else(|| {
                        let normalized = game_db::normalize_filename(stem);
                        game_db::lookup_by_normalized_title(system, &normalized)
                    });
                    match game {
                        Some(g) => (
                            if g.normalized_genre.is_empty() { None } else { Some(g.normalized_genre.to_string()) },
                            if g.players > 0 { Some(g.players) } else { None },
                        ),
                        None => (None, None),
                    }
                };

                CachedRom {
                    system: r.game.system.clone(),
                    rom_filename: rom_filename.clone(),
                    rom_path: r.game.rom_path.clone(),
                    display_name: r.game.display_name.clone(),
                    size_bytes: r.size_bytes,
                    is_m3u: r.is_m3u,
                    box_art_url: r.box_art_url.clone(),
                    driver_status: r.driver_status.clone(),
                    genre,
                    players: players_lookup.or(r.players),
                    rating: r.rating,
                }
            })
            .collect();

        self.with_db_mut(storage, |db| {
            if let Err(e) = db.save_system_roms(system, &cached_roms, mtime_secs) {
                tracing::debug!("Failed to write L2 cache for {system}: {e}");
            }
        });
    }

    /// Get the set of favorited filenames for a system.
    /// Uses a cached favorites list to avoid per-request filesystem reads.
    pub fn get_favorites_set(
        &self,
        storage: &StorageLocation,
        system: &str,
    ) -> HashSet<String> {
        let favs_dir = storage.favorites_dir();

        // Try read lock first.
        if let Ok(guard) = self.favorites.read() {
            if let Some(ref cache) = *guard {
                if cache.is_fresh(&favs_dir) {
                    return cache
                        .data
                        .get(system)
                        .cloned()
                        .unwrap_or_default();
                }
            }
        }

        // Cache miss — rebuild.
        let new_cache = FavoritesCache::new(storage);
        let result = new_cache
            .data
            .get(system)
            .cloned()
            .unwrap_or_default();
        if let Ok(mut guard) = self.favorites.write() {
            *guard = Some(new_cache);
        }
        result
    }

    /// Get the total count of favorited games (all systems).
    /// Uses the cached favorites to avoid filesystem traversal.
    pub fn get_favorites_count(&self, storage: &StorageLocation) -> usize {
        let favs_dir = storage.favorites_dir();

        if let Ok(guard) = self.favorites.read() {
            if let Some(ref cache) = *guard {
                if cache.is_fresh(&favs_dir) {
                    return cache.data.values().map(|s| s.len()).sum();
                }
            }
        }

        let new_cache = FavoritesCache::new(storage);
        let count = new_cache.data.values().map(|s| s.len()).sum();
        if let Ok(mut guard) = self.favorites.write() {
            *guard = Some(new_cache);
        }
        count
    }

    /// Get cached recents or scan and cache.
    /// Recents are created by RePlayOS on game launch, so mtime-based
    /// invalidation detects new entries without explicit invalidation.
    pub fn get_recents(
        &self,
        storage: &StorageLocation,
    ) -> Result<Vec<RecentEntry>, replay_control_core::error::Error> {
        let recents_dir = storage.recents_dir();

        if let Ok(guard) = self.recents.read() {
            if let Some(ref entry) = *guard {
                if entry.is_fresh(&recents_dir) {
                    return Ok(entry.data.clone());
                }
            }
        }

        let entries = replay_control_core::recents::list_recents(storage)?;
        if let Ok(mut guard) = self.recents.write() {
            *guard = Some(CacheEntry::new(entries.clone(), &recents_dir));
        }
        Ok(entries)
    }

    /// Get or build the image index for a system.
    /// The index maps normalized image names to actual paths, enabling O(1) box art lookups.
    pub fn get_image_index(
        &self,
        state: &crate::api::AppState,
        system: &str,
    ) -> std::sync::Arc<ImageIndex> {
        use replay_control_core::thumbnails::strip_version;

        let media_base = state.storage().rc_dir().join("media").join(system);
        let boxart_dir = media_base.join("boxart");

        // Check cache freshness.
        if let Ok(guard) = self.images.read() {
            if let Some(idx) = guard.get(system) {
                if idx.is_fresh(&boxart_dir) {
                    // Return a reference-counted clone so we don't hold the lock.
                    // For now, rebuild is cheap enough that we just clone the maps.
                    return std::sync::Arc::new(ImageIndex {
                        exact: idx.exact.clone(),
                        fuzzy: idx.fuzzy.clone(),
                        version: idx.version.clone(),
                        db_paths: idx.db_paths.clone(),
                        dir_mtime: idx.dir_mtime,
                        expires: idx.expires,
                    });
                }
            }
        }

        // Build the index.
        let base_title = |s: &str| -> String {
            let s = s.rsplit_once(" ~ ").map(|(_, r)| r).unwrap_or(s);
            s.find(" (")
                .or_else(|| s.find(" ["))
                .map(|i| &s[..i])
                .unwrap_or(s)
                .trim()
                .to_lowercase()
        };

        let mut exact = HashMap::new();
        let mut fuzzy = HashMap::new();
        let mut version = HashMap::new();

        if let Ok(entries) = std::fs::read_dir(&boxart_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if let Some(img_stem) = name_str.strip_suffix(".png") {
                    let path = format!("boxart/{name_str}");
                    // Only index valid images (skip fake symlinks < 200 bytes).
                    let full = entry.path();
                    let valid = full.metadata().map(|m| m.len() >= 200).unwrap_or(false);
                    if !valid {
                        // Try resolving fake symlink.
                        if let Some(resolved) =
                            crate::server_fns::try_resolve_fake_symlink(&full, &boxart_dir)
                        {
                            let resolved_path = format!("boxart/{resolved}");
                            exact.insert(img_stem.to_string(), resolved_path.clone());
                            let bt = base_title(img_stem);
                            let vs = strip_version(&bt).to_string();
                            fuzzy.entry(bt.clone()).or_insert_with(|| resolved_path.clone());
                            if vs.len() < bt.len() {
                                version.entry(vs).or_insert(resolved_path);
                            }
                        }
                        continue;
                    }
                    exact.insert(img_stem.to_string(), path.clone());
                    let bt = base_title(img_stem);
                    let vs = strip_version(&bt).to_string();
                    fuzzy.entry(bt.clone()).or_insert_with(|| path.clone());
                    if vs.len() < bt.len() {
                        version.entry(vs).or_insert(path);
                    }
                }
            }
        }

        // Load DB paths for this system.
        let db_paths = if let Some(guard) = state.metadata_db() {
            if let Some(db) = guard.as_ref() {
                db.system_box_art_paths(system).unwrap_or_default()
            } else {
                HashMap::new()
            }
        } else {
            HashMap::new()
        };

        let index = ImageIndex {
            exact,
            fuzzy,
            version,
            db_paths,
            dir_mtime: dir_mtime(&boxart_dir),
            expires: Instant::now() + CACHE_HARD_TTL,
        };

        let arc = std::sync::Arc::new(ImageIndex {
            exact: index.exact.clone(),
            fuzzy: index.fuzzy.clone(),
            version: index.version.clone(),
            db_paths: index.db_paths.clone(),
            dir_mtime: index.dir_mtime,
            expires: index.expires,
        });

        if let Ok(mut guard) = self.images.write() {
            guard.insert(system.to_string(), index);
        }

        arc
    }

    /// Resolve a box art URL for a single ROM using the cached image index.
    pub fn resolve_box_art(
        &self,
        index: &ImageIndex,
        system: &str,
        rom_filename: &str,
    ) -> Option<String> {
        use replay_control_core::thumbnails::{strip_version, thumbnail_filename};

        // 1. Try DB path first (already validated during index build).
        if let Some(db_path) = index.db_paths.get(rom_filename) {
            // Check if this path exists in our exact index (validates file on disk).
            let stem = db_path.strip_prefix("boxart/").unwrap_or(db_path);
            let stem = stem.strip_suffix(".png").unwrap_or(stem);
            if index.exact.contains_key(stem) {
                return Some(format!("/media/{system}/{db_path}"));
            }
        }

        // 2. Exact thumbnail name match.
        let stem = rom_filename
            .rfind('.')
            .map(|i| &rom_filename[..i])
            .unwrap_or(rom_filename);
        let stem = stem.strip_prefix("N64DD - ").unwrap_or(stem);
        let thumb_name = thumbnail_filename(stem);

        if let Some(path) = index.exact.get(&thumb_name) {
            return Some(format!("/media/{system}/{path}"));
        }

        // 3. Fuzzy match (strip tags).
        let base_title = |s: &str| -> String {
            let s = s.rsplit_once(" ~ ").map(|(_, r)| r).unwrap_or(s);
            s.find(" (")
                .or_else(|| s.find(" ["))
                .map(|i| &s[..i])
                .unwrap_or(s)
                .trim()
                .to_lowercase()
        };

        let rom_base = base_title(&thumb_name);
        if let Some(path) = index.fuzzy.get(&rom_base) {
            return Some(format!("/media/{system}/{path}"));
        }

        // 4. Version-stripped match.
        let rom_base_no_version = strip_version(&rom_base);
        if rom_base_no_version.len() < rom_base.len() {
            if let Some(path) = index.version.get(rom_base_no_version) {
                return Some(format!("/media/{system}/{path}"));
            }
        }

        None
    }

    /// Invalidate all caches (after delete, rename, upload).
    /// Clears both L1 (in-memory) and L2 (SQLite rom_cache).
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
        // L2: Clear SQLite rom_cache.
        if let Ok(guard) = self.db.lock() {
            if let Some(ref db) = *guard {
                let _ = db.clear_all_rom_cache();
            }
        }
    }

    /// Invalidate cache for a specific system.
    /// Clears both L1 (in-memory) and L2 (SQLite rom_cache) for the system.
    pub fn invalidate_system(&self, system: &str) {
        if let Ok(mut guard) = self.systems.write() {
            *guard = None;
        }
        if let Ok(mut guard) = self.roms.write() {
            guard.remove(system);
        }
        // L2: Clear SQLite rom_cache for this system.
        if let Ok(guard) = self.db.lock() {
            if let Some(ref db) = *guard {
                let _ = db.clear_system_rom_cache(system);
            }
        }
    }

    /// Invalidate only the favorites cache (after add/remove favorite).
    pub fn invalidate_favorites(&self) {
        if let Ok(mut guard) = self.favorites.write() {
            *guard = None;
        }
    }
}
