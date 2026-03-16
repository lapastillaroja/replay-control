use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use replay_control_core::metadata_db::MetadataDb;
use replay_control_core::recents::RecentEntry;
use replay_control_core::rom_tags::RegionPreference;
use replay_control_core::roms::{RomEntry, SystemSummary};
use replay_control_core::storage::StorageLocation;
use replay_control_core::thumbnail_manifest::ManifestFuzzyIndex;

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
    /// Manifest-backed fallback for images not yet downloaded.
    /// None if the thumbnail_index has no entries for this system.
    pub manifest: Option<ManifestFuzzyIndex>,
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

pub struct GameLibrary {
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

impl GameLibrary {
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

    /// Run a read-only closure with the DB, opening it if needed.
    /// Public for recommendation queries that need direct DB access.
    pub fn with_db_read<F, R>(&self, storage: &StorageLocation, f: F) -> Option<R>
    where
        F: FnOnce(&MetadataDb) -> R,
    {
        self.with_db(storage, f)
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

        let cached_meta = self.with_db(storage, |db| db.load_all_system_meta())?;
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
    /// L1 (in-memory) → L2 (SQLite game_library) → L3 (filesystem scan).
    pub fn get_roms(
        &self,
        storage: &StorageLocation,
        system: &str,
        region_pref: RegionPreference,
        region_secondary: Option<RegionPreference>,
    ) -> Result<Vec<RomEntry>, replay_control_core::error::Error> {
        let key = system.to_string();
        let system_dir = storage.roms_dir().join(system);

        // L1: Try in-memory cache.
        if let Ok(guard) = self.roms.read()
            && let Some(entry) = guard.get(&key)
            && entry.is_fresh(&system_dir)
        {
            return Ok(entry.data.clone());
        }

        // L2: Try SQLite game_library.
        if let Some(roms) = self.load_roms_from_db(storage, system, &system_dir) {
            // Store in L1.
            if let Ok(mut guard) = self.roms.write() {
                guard.insert(key, CacheEntry::new(roms.clone(), &system_dir));
            }
            return Ok(roms);
        }

        // L3: Cache miss — full filesystem scan.
        tracing::debug!("L3 scan for {system}: starting filesystem scan");
        let mut roms = replay_control_core::roms::list_roms(storage, system, region_pref, region_secondary)?;
        tracing::debug!("L3 scan for {system}: found {} ROMs", roms.len());

        // Hash-and-identify step: for hash-eligible systems, compute CRC32 hashes
        // and look up canonical names in the embedded No-Intro DAT data.
        let hash_results = self.hash_roms_for_system(storage, system, &mut roms);

        if let Ok(mut guard) = self.roms.write() {
            guard.insert(key.clone(), CacheEntry::new(roms.clone(), &system_dir));
        }

        // Write-through to L2.
        self.save_roms_to_db(storage, system, &roms, &system_dir, &hash_results);

        Ok(roms)
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
            .with_db(storage, |db| db.load_system_meta(system))?
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
            .with_db(storage, |db| db.load_system_entries(system))?
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

    /// Hash ROM files for a hash-eligible system and apply identification results.
    ///
    /// For eligible systems (cartridge-based with No-Intro CRC data), this:
    /// 1. Loads cached hashes from the database
    /// 2. Computes CRC32 for new/modified files
    /// 3. Looks up CRC32 in the No-Intro index
    /// 4. Overrides display names for matched ROMs (via `GameRef::new()` with the
    ///    canonical No-Intro name)
    ///
    /// Returns a map of rom_filename -> HashResult for use by save_roms_to_db.
    fn hash_roms_for_system(
        &self,
        storage: &StorageLocation,
        system: &str,
        roms: &mut [RomEntry],
    ) -> HashMap<String, replay_control_core::rom_hash::HashResult> {
        use replay_control_core::rom_hash::{self, HashResult};

        if !rom_hash::is_hash_eligible(system) {
            return HashMap::new();
        }

        // Load cached hashes from L2 (database).
        let cached_hashes = self
            .with_db(storage, |db| db.load_cached_hashes(system))
            .and_then(|r| r.ok())
            .unwrap_or_default();

        // Build input list: (rom_filename, rom_path, size_bytes).
        let rom_files: Vec<(String, String, u64)> = roms
            .iter()
            .filter(|r| !r.is_m3u) // Skip M3U playlists
            .map(|r| {
                (
                    r.game.rom_filename.clone(),
                    r.game.rom_path.clone(),
                    r.size_bytes,
                )
            })
            .collect();

        let results =
            rom_hash::hash_and_identify(system, &rom_files, &cached_hashes, &storage.root);

        // Build a lookup map for applying results.
        let mut result_map: HashMap<String, HashResult> = HashMap::new();
        for result in results {
            result_map.insert(result.rom_filename.clone(), result);
        }

        // Apply hash-matched display names to RomEntries.
        // When a CRC match gives us a canonical No-Intro name (e.g.,
        // "Super Mario World (USA)"), re-resolve the display name through
        // GameRef::new() using that canonical name as the filename stem.
        // This gives us the proper display name with tags.
        for rom in roms.iter_mut() {
            if let Some(hash_result) = result_map.get(&rom.game.rom_filename)
                && let Some(ref matched_name) = hash_result.matched_name
            {
                // The matched_name is the No-Intro canonical filename stem
                // (e.g., "Super Mario World (USA)"). Use game_display_name()
                // to get the clean display title, then apply tags from the
                // original filename.
                let canonical_filename = format!("{matched_name}.rom");
                if let Some(display) =
                    replay_control_core::game_db::game_display_name(system, &canonical_filename)
                {
                    let with_tags = replay_control_core::rom_tags::display_name_with_tags(
                        display,
                        &rom.game.rom_filename,
                    );
                    rom.game.display_name = Some(with_tags);
                }
            }
        }

        if !result_map.is_empty() {
            let matched = result_map.values().filter(|r| r.matched_name.is_some()).count();
            tracing::debug!(
                "Hash-and-identify for {system}: {} hashed, {} matched No-Intro",
                result_map.len(),
                matched
            );
        }

        result_map
    }

    /// Write ROM list to SQLite game_library for persistent storage.
    /// Enriches with genre/players from the baked-in game databases during write.
    fn save_roms_to_db(
        &self,
        storage: &StorageLocation,
        system: &str,
        roms: &[RomEntry],
        system_dir: &Path,
        hash_results: &HashMap<String, replay_control_core::rom_hash::HashResult>,
    ) {
        use replay_control_core::metadata_db::GameEntry;
        use replay_control_core::systems::{self, SystemCategory};
        use replay_control_core::{arcade_db, game_db};

        let mtime_secs = dir_mtime(system_dir).and_then(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|d| d.as_secs() as i64)
        });

        let is_arcade =
            systems::find_system(system).is_some_and(|s| s.category == SystemCategory::Arcade);

        let cached_roms: Vec<GameEntry> = roms
            .iter()
            .filter_map(|r| {
                let rom_filename = &r.game.rom_filename;
                let stem = rom_filename
                    .rfind('.')
                    .map(|i| &rom_filename[..i])
                    .unwrap_or(rom_filename);

                // Two-tier genre: `genre` = detail/original, `genre_group` = normalized.
                let (genre, genre_group, players_lookup, is_clone, base_title) = if is_arcade {
                    let arcade_stem = rom_filename.strip_suffix(".zip").unwrap_or(rom_filename);
                    match arcade_db::lookup_arcade_game(arcade_stem) {
                        Some(info) => {
                            // Skip BIOS entries — they're not playable games
                            if info.is_bios {
                                return None;
                            }
                            // genre = raw category (e.g., "Maze / Shooter")
                            let detail = if info.category.is_empty() {
                                None
                            } else {
                                Some(info.category.to_string())
                            };
                            // genre_group = normalized (e.g., "Maze")
                            let group = replay_control_core::genre::normalize_genre(
                                info.category,
                            ).to_string();
                            (
                                detail,
                                group,
                                Some(info.players),
                                info.is_clone,
                                replay_control_core::title_utils::base_title(info.display_name),
                            )
                        }
                        None => (None, String::new(), None, false, replay_control_core::title_utils::base_title(stem)),
                    }
                } else {
                    // Try CRC-based lookup first (if we have a hash match),
                    // then fall back to filename-based lookup.
                    let hash_entry = hash_results
                        .get(rom_filename)
                        .and_then(|hr| hr.matched_name.as_ref())
                        .and_then(|name| game_db::lookup_game(system, name));
                    let entry = hash_entry.or_else(|| game_db::lookup_game(system, stem));
                    let game = entry.map(|e| e.game).or_else(|| {
                        let normalized = game_db::normalize_filename(stem);
                        game_db::lookup_by_normalized_title(system, &normalized)
                    });
                    let bt = r.game.display_name.as_deref()
                        .map(replay_control_core::title_utils::base_title)
                        .unwrap_or_else(|| replay_control_core::title_utils::base_title(stem));
                    match game {
                        Some(g) => {
                            // genre = raw genre from game_db (e.g., "Shoot'em Up")
                            let detail = if g.genre.is_empty() {
                                None
                            } else {
                                Some(g.genre.to_string())
                            };
                            // genre_group = normalized (e.g., "Shooter")
                            let group = replay_control_core::genre::normalize_genre(
                                g.genre,
                            ).to_string();
                            (
                                detail,
                                group,
                                if g.players > 0 { Some(g.players) } else { None },
                                false,
                                bt,
                            )
                        }
                        None => (None, String::new(), None, false, bt),
                    }
                };

                let (tier, region_priority, is_special) =
                    replay_control_core::rom_tags::classify(rom_filename);
                let is_translation = tier == replay_control_core::rom_tags::RomTier::Translation;
                let is_hack = tier == replay_control_core::rom_tags::RomTier::Hack;
                let region = match region_priority {
                    replay_control_core::rom_tags::RegionPriority::Usa => "usa",
                    replay_control_core::rom_tags::RegionPriority::Europe => "europe",
                    replay_control_core::rom_tags::RegionPriority::Japan => "japan",
                    replay_control_core::rom_tags::RegionPriority::World => "world",
                    replay_control_core::rom_tags::RegionPriority::Other => "other",
                    replay_control_core::rom_tags::RegionPriority::Unknown => "",
                };

                // Look up hash result for this ROM file.
                let hash = hash_results.get(rom_filename);

                // Compute series_key from base_title for franchise grouping.
                let series_key = replay_control_core::title_utils::series_key(&base_title);

                Some(GameEntry {
                    system: r.game.system.clone(),
                    rom_filename: rom_filename.clone(),
                    rom_path: r.game.rom_path.clone(),
                    display_name: r.game.display_name.clone(),
                    size_bytes: r.size_bytes,
                    is_m3u: r.is_m3u,
                    box_art_url: r.box_art_url.clone(),
                    driver_status: r.driver_status.clone(),
                    genre,
                    genre_group,
                    players: players_lookup.or(r.players),
                    rating: r.rating,
                    is_clone,
                    base_title,
                    region: region.to_string(),
                    is_translation,
                    is_hack,
                    is_special,
                    crc32: hash.map(|h| h.crc32),
                    hash_mtime: hash.map(|h| h.mtime_secs),
                    hash_matched_name: hash.and_then(|h| h.matched_name.clone()),
                    series_key,
                })
            })
            .collect();

        tracing::debug!(
            "L2 write-through: saving {} ROMs for {system} (mtime={mtime_secs:?})",
            cached_roms.len()
        );
        let result = self.with_db_mut(storage, |db| {
            db.save_system_entries(system, &cached_roms, mtime_secs)
        });
        match result {
            Some(Ok(())) => {
                tracing::debug!("L2 write-through: {system} OK ({} ROMs)", cached_roms.len());

                // Populate TGDB aliases from embedded build-time data.
                self.populate_tgdb_aliases(storage, system, &cached_roms);

                // Populate game_series from embedded Wikidata data.
                self.populate_wikidata_series(storage, system, &cached_roms);
            }
            Some(Err(e)) => tracing::warn!("L2 write-through: {system} FAILED: {e}"),
            None => tracing::warn!("L2 write-through: {system} skipped (DB unavailable)"),
        }
    }

    /// Populate game_alias table with TGDB alternate names for a system.
    ///
    /// Matches canonical games in the embedded TGDB data to `game_library`
    /// entries via normalized title, then inserts their alternate names.
    fn populate_tgdb_aliases(
        &self,
        storage: &StorageLocation,
        system: &str,
        roms: &[replay_control_core::metadata_db::GameEntry],
    ) {
        use replay_control_core::game_db;
        use replay_control_core::systems::{self, SystemCategory};

        let is_arcade =
            systems::find_system(system).is_some_and(|s| s.category == SystemCategory::Arcade);

        // TGDB alternates are only available for non-arcade systems with game_db coverage.
        if is_arcade || !game_db::has_system(system) {
            return;
        }

        let alternates = game_db::system_alternates(system);
        if alternates.is_empty() {
            return;
        }

        let games = match game_db::system_games(system) {
            Some(g) => g,
            None => return,
        };

        // Build lookup maps for matching TGDB names to library base_titles.
        use replay_control_core::title_utils::{fuzzy_match_key, resolve_to_library_title};

        let library_exact: std::collections::HashSet<&str> = roms
            .iter()
            .filter(|r| !r.base_title.is_empty())
            .map(|r| r.base_title.as_str())
            .collect();

        let library_fuzzy: std::collections::HashMap<String, &str> = roms
            .iter()
            .filter(|r| !r.base_title.is_empty())
            .map(|r| (fuzzy_match_key(&r.base_title), r.base_title.as_str()))
            .collect();

        let mut aliases: Vec<(String, String, String, String, String)> = Vec::new();

        for &(game_id, alt_names) in alternates {
            if let Some(game) = games.get(game_id as usize) {
                let resolved = resolve_to_library_title(
                    game.display_name, &library_exact, &library_fuzzy,
                );
                if !library_exact.contains(resolved.as_str())
                    && !library_fuzzy.contains_key(&fuzzy_match_key(&resolved))
                {
                    continue; // Game not in user's library
                }
                let library_bt = resolved;

                for alt in alt_names {
                    let alt_resolved = resolve_to_library_title(
                        alt, &library_exact, &library_fuzzy,
                    );
                    if alt_resolved != library_bt && !alt_resolved.is_empty() {
                        // Forward: library game -> alternate name
                        aliases.push((
                            system.to_string(),
                            library_bt.clone(),
                            alt_resolved.clone(),
                            String::new(),
                            "tgdb".to_string(),
                        ));
                        // Reverse: if the alternate is also in the library, link back
                        if library_exact.contains(alt_resolved.as_str())
                            || library_fuzzy.contains_key(&fuzzy_match_key(&alt_resolved))
                        {
                            aliases.push((
                                system.to_string(),
                                alt_resolved,
                                library_bt.clone(),
                                String::new(),
                                "tgdb".to_string(),
                            ));
                        }
                    }
                }
            }
        }

        if aliases.is_empty() {
            return;
        }

        let count = aliases.len();
        let result = self.with_db_mut(storage, |db| db.bulk_insert_aliases(&aliases));
        match result {
            Some(Ok(n)) => {
                tracing::debug!("TGDB aliases for {system}: {n}/{count} inserted")
            }
            Some(Err(e)) => tracing::warn!("TGDB aliases for {system}: insert failed: {e}"),
            None => {}
        }
    }

    /// Populate game_series table with Wikidata series data for a system.
    ///
    /// Matches embedded Wikidata entries to `game_library` rows by normalized
    /// title + system, then inserts series membership into `game_series`.
    fn populate_wikidata_series(
        &self,
        storage: &StorageLocation,
        system: &str,
        roms: &[replay_control_core::metadata_db::GameEntry],
    ) {
        use replay_control_core::series_db;
        use replay_control_core::systems::{self, SystemCategory};

        // Wikidata series data is only available for non-arcade systems with game_db coverage.
        let is_arcade =
            systems::find_system(system).is_some_and(|s| s.category == SystemCategory::Arcade);
        if is_arcade {
            return;
        }

        let wikidata_entries = series_db::system_series_entries(system);
        if wikidata_entries.is_empty() {
            return;
        }

        // Build a map of normalized_title -> base_title for games in the library.
        let mut norm_to_base: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for rom in roms {
            if rom.base_title.is_empty() {
                continue;
            }
            // Normalize the base_title the same way Wikidata titles are normalized:
            // lowercase, strip non-alphanumeric except spaces, collapse whitespace.
            let normalized = normalize_for_wikidata_match(&rom.base_title);
            if !normalized.is_empty() {
                norm_to_base.entry(normalized).or_insert_with(|| rom.base_title.clone());
            }
            // Also try with display_name for better matching
            if let Some(ref dn) = rom.display_name {
                let norm_dn = normalize_for_wikidata_match(
                    &replay_control_core::title_utils::base_title(dn),
                );
                if !norm_dn.is_empty() {
                    norm_to_base.entry(norm_dn).or_insert_with(|| rom.base_title.clone());
                }
            }
        }

        let mut series_entries: Vec<(String, String, String, Option<i32>, String)> = Vec::new();

        for entry in &wikidata_entries {
            if let Some(base_title) = norm_to_base.get(entry.normalized_title)
                && !entry.series_name.is_empty()
            {
                series_entries.push((
                    system.to_string(),
                    base_title.clone(),
                    entry.series_name.to_string(),
                    entry.series_order,
                    "wikidata".to_string(),
                ));
            }
        }

        if series_entries.is_empty() {
            return;
        }

        let count = series_entries.len();
        let result = self.with_db_mut(storage, |db| db.bulk_insert_series(&series_entries));
        match result {
            Some(Ok(n)) => {
                tracing::debug!("Wikidata series for {system}: {n}/{count} inserted")
            }
            Some(Err(e)) => {
                tracing::warn!("Wikidata series for {system}: insert failed: {e}")
            }
            None => {}
        }
    }

    /// Get the set of favorited filenames for a system.
    /// Uses a cached favorites list to avoid per-request filesystem reads.
    pub fn get_favorites_set(&self, storage: &StorageLocation, system: &str) -> HashSet<String> {
        let favs_dir = storage.favorites_dir();

        // Try read lock first.
        if let Ok(guard) = self.favorites.read()
            && let Some(ref cache) = *guard
            && cache.is_fresh(&favs_dir)
        {
            return cache.data.get(system).cloned().unwrap_or_default();
        }

        // Cache miss — rebuild.
        let new_cache = FavoritesCache::new(storage);
        let result = new_cache.data.get(system).cloned().unwrap_or_default();
        if let Ok(mut guard) = self.favorites.write() {
            *guard = Some(new_cache);
        }
        result
    }

    /// Get the most-favorited system and its favorited filenames.
    /// Uses the cached favorites — no filesystem access on cache hit.
    pub fn get_top_favorited_system(
        &self,
        storage: &StorageLocation,
    ) -> Option<(String, Vec<String>)> {
        let favs_dir = storage.favorites_dir();

        // Ensure cache is fresh.
        if let Ok(guard) = self.favorites.read()
            && let Some(ref cache) = *guard
            && cache.is_fresh(&favs_dir)
        {
            return Self::top_system_from_data(&cache.data);
        }

        // Rebuild cache.
        let new_cache = FavoritesCache::new(storage);
        let result = Self::top_system_from_data(&new_cache.data);
        if let Ok(mut guard) = self.favorites.write() {
            *guard = Some(new_cache);
        }
        result
    }

    fn top_system_from_data(
        data: &HashMap<String, HashSet<String>>,
    ) -> Option<(String, Vec<String>)> {
        data.iter()
            .max_by_key(|(_, files)| files.len())
            .map(|(system, files)| (system.clone(), files.iter().cloned().collect()))
    }

    /// Get all systems that have favorites, with their filenames.
    /// Used by recommendations to rotate across favorited systems.
    pub fn get_all_favorited_systems(
        &self,
        storage: &StorageLocation,
    ) -> Option<HashMap<String, Vec<String>>> {
        let favs_dir = storage.favorites_dir();

        let extract = |data: &HashMap<String, HashSet<String>>| -> HashMap<String, Vec<String>> {
            data.iter()
                .filter(|(_, files)| !files.is_empty())
                .map(|(system, files)| (system.clone(), files.iter().cloned().collect()))
                .collect()
        };

        if let Ok(guard) = self.favorites.read()
            && let Some(ref cache) = *guard
            && cache.is_fresh(&favs_dir)
        {
            let result = extract(&cache.data);
            return if result.is_empty() { None } else { Some(result) };
        }

        let new_cache = FavoritesCache::new(storage);
        let result = extract(&new_cache.data);
        if let Ok(mut guard) = self.favorites.write() {
            *guard = Some(new_cache);
        }
        if result.is_empty() { None } else { Some(result) }
    }

    /// Get the total count of favorited games (all systems).
    /// Uses the cached favorites to avoid filesystem traversal.
    pub fn get_favorites_count(&self, storage: &StorageLocation) -> usize {
        let favs_dir = storage.favorites_dir();

        if let Ok(guard) = self.favorites.read()
            && let Some(ref cache) = *guard
            && cache.is_fresh(&favs_dir)
        {
            return cache.data.values().map(|s| s.len()).sum();
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
        if let Ok(guard) = self.images.read()
            && let Some(idx) = guard.get(system)
            && idx.is_fresh(&boxart_dir)
        {
            return std::sync::Arc::new(ImageIndex {
                exact: idx.exact.clone(),
                fuzzy: idx.fuzzy.clone(),
                version: idx.version.clone(),
                db_paths: idx.db_paths.clone(),
                manifest: None, // Don't clone the manifest (large); rebuild if needed
                dir_mtime: idx.dir_mtime,
                expires: idx.expires,
            });
        }

        // Build the index.
        let base_title = replay_control_core::thumbnails::base_title;

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
                            replay_control_core::thumbnails::try_resolve_fake_symlink(&full, &boxart_dir)
                        {
                            let resolved_path = format!("boxart/{resolved}");
                            exact.insert(img_stem.to_string(), resolved_path.clone());
                            let bt = base_title(img_stem);
                            let vs = strip_version(&bt).to_string();
                            fuzzy
                                .entry(bt.clone())
                                .or_insert_with(|| resolved_path.clone());
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

        // Load user box art overrides first (separate lock, released before metadata_db).
        let user_overrides: HashMap<String, String> = state
            .user_data_db()
            .and_then(|guard| {
                guard
                    .as_ref()
                    .and_then(|db| db.get_system_overrides(system).ok())
            })
            .unwrap_or_default();

        // Load DB paths for this system.
        let (db_paths, manifest) = if let Some(guard) = state.metadata_db() {
            if let Some(db) = guard.as_ref() {
                let mut paths = db.system_box_art_paths(system).unwrap_or_default();

                // Inject user box art overrides (highest priority — overwrites auto-matched paths).
                for (rom_filename, override_path) in user_overrides {
                    paths.insert(rom_filename, override_path);
                }

                // Build manifest fuzzy index for on-demand downloads.
                let mfi = if let Some(repo_names) =
                    replay_control_core::thumbnails::thumbnail_repo_names(system)
                {
                    let idx = replay_control_core::thumbnail_manifest::build_manifest_fuzzy_index(
                        db,
                        repo_names,
                        "Named_Boxarts",
                    );
                    if idx.exact.is_empty() {
                        None
                    } else {
                        Some(idx)
                    }
                } else {
                    None
                };
                (paths, mfi)
            } else {
                (HashMap::new(), None)
            }
        } else {
            (HashMap::new(), None)
        };

        let index = ImageIndex {
            exact,
            fuzzy,
            version,
            db_paths,
            manifest: None, // Stored in cache without manifest (rebuilt on arc)
            dir_mtime: dir_mtime(&boxart_dir),
            expires: Instant::now() + CACHE_HARD_TTL,
        };

        let arc = std::sync::Arc::new(ImageIndex {
            exact: index.exact.clone(),
            fuzzy: index.fuzzy.clone(),
            version: index.version.clone(),
            db_paths: index.db_paths.clone(),
            manifest,
            dir_mtime: index.dir_mtime,
            expires: index.expires,
        });

        if let Ok(mut guard) = self.images.write() {
            guard.insert(system.to_string(), index);
        }

        arc
    }

    /// Resolve a box art URL for a single ROM using the cached image index.
    /// If no local image is found but the manifest has a match, a background
    /// download is queued and None is returned (image appears on next load).
    pub fn resolve_box_art(
        &self,
        state: &crate::api::AppState,
        index: &ImageIndex,
        system: &str,
        rom_filename: &str,
    ) -> Option<String> {
        use replay_control_core::thumbnails::{strip_version, thumbnail_filename};

        // 1. Try DB path first (already validated during index build).
        if let Some(db_path) = index.db_paths.get(rom_filename) {
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

        // For arcade ROMs, translate MAME codename to display name.
        let is_arcade = matches!(
            system,
            "arcade_mame" | "arcade_fbneo" | "arcade_mame_2k3p" | "arcade_dc"
        );
        let display_name = if is_arcade {
            replay_control_core::arcade_db::lookup_arcade_game(stem)
                .map(|info| info.display_name)
        } else {
            None
        };
        let thumb_name = thumbnail_filename(display_name.unwrap_or(stem));

        if let Some(path) = index.exact.get(&thumb_name) {
            return Some(format!("/media/{system}/{path}"));
        }

        // Colon variants for arcade games (e.g., "Marvel vs. Capcom: Clash of Super Heroes").
        let source = display_name.unwrap_or(stem);
        if source.contains(':') {
            let dash_variant = thumbnail_filename(&source.replace(": ", " - ").replace(':', " -"));
            if let Some(path) = index.exact.get(&dash_variant) {
                return Some(format!("/media/{system}/{path}"));
            }
            let drop_variant = thumbnail_filename(&source.replace(": ", " ").replace(':', ""));
            if let Some(path) = index.exact.get(&drop_variant) {
                return Some(format!("/media/{system}/{path}"));
            }
        }

        // 3. Fuzzy match (strip tags).
        let base_title = replay_control_core::thumbnails::base_title;

        let rom_base = base_title(&thumb_name);
        if let Some(path) = index.fuzzy.get(&rom_base) {
            return Some(format!("/media/{system}/{path}"));
        }

        // 4. Version-stripped match.
        let rom_base_no_version = strip_version(&rom_base);
        if rom_base_no_version.len() < rom_base.len()
            && let Some(path) = index.fuzzy.get(rom_base_no_version).or_else(|| index.version.get(rom_base_no_version))
        {
            return Some(format!("/media/{system}/{path}"));
        }

        // 5. On-demand: check manifest for a remote thumbnail to download.
        if let Some(ref manifest) = index.manifest
            && let Some(m) = replay_control_core::thumbnail_manifest::find_in_manifest(
                manifest,
                rom_filename,
                system,
            )
        {
            self.queue_on_demand_download(state, system, m);
        }

        None
    }

    /// Queue a background download for a single thumbnail.
    /// Deduplicates concurrent requests for the same image.
    fn queue_on_demand_download(
        &self,
        state: &crate::api::AppState,
        system: &str,
        m: &replay_control_core::thumbnail_manifest::ManifestMatch,
    ) {
        use replay_control_core::thumbnail_manifest::{download_thumbnail, save_thumbnail};
        use replay_control_core::thumbnails::ThumbnailKind;

        let download_key = format!("{system}/{}", m.filename);

        // Check and insert atomically to prevent duplicate downloads.
        {
            let mut pending = state.pending_downloads.write().expect("pending lock");
            if !pending.insert(download_key.clone()) {
                return; // Already queued.
            }
        }

        let m = m.clone();
        let storage_root = state.storage().root.clone();
        let system = system.to_string();
        let pending = state.pending_downloads.clone();
        let cache = state.cache.clone();

        std::thread::spawn(move || {
            match download_thumbnail(&m, "Named_Boxarts") {
                Ok(bytes) => {
                    if let Err(e) = save_thumbnail(
                        &storage_root,
                        &system,
                        ThumbnailKind::Boxart,
                        &m.filename,
                        &bytes,
                    ) {
                        tracing::debug!("On-demand save failed for {}: {e}", m.filename);
                    } else {
                        // Invalidate image cache so the next page load picks up the new file.
                        cache.invalidate_system_images(&system);
                    }
                }
                Err(e) => {
                    tracing::debug!("On-demand download failed for {}: {e}", m.filename);
                }
            }

            // Remove from pending set.
            if let Ok(mut guard) = pending.write() {
                guard.remove(&download_key);
            }
        });
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
        if let Ok(guard) = self.db.lock()
            && let Some(ref db) = *guard
        {
            let _ = db.clear_all_game_library();
        }
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
        if let Ok(guard) = self.db.lock()
            && let Some(ref db) = *guard
        {
            let _ = db.clear_system_game_library(system);
        }
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

    /// Enrich box_art_url (and rating) for all entries in a system's game library.
    /// Uses the image index for box art and game_metadata for ratings.
    /// Called after L2 write-through to populate fields that `list_roms()` doesn't set.
    ///
    /// Also auto-matches new ROMs (those without metadata) against existing
    /// LaunchBox entries by normalized title. Matched metadata is persisted
    /// so future lookups hit directly without re-matching.
    pub fn enrich_system_cache(&self, state: &crate::api::AppState, system: &str) {
        let storage = state.storage();
        let index = self.get_image_index(state, system);

        // Load ratings from game_metadata table (from LaunchBox import).
        let ratings: HashMap<String, f64> = state
            .metadata_db()
            .and_then(|guard| guard.as_ref()?.system_ratings(system).ok())
            .unwrap_or_default();

        // Load genres from game_metadata table (from LaunchBox import).
        // Used to fill empty game_library.genre entries.
        let lb_genres: HashMap<String, String> = state
            .metadata_db()
            .and_then(|guard| guard.as_ref()?.system_metadata_genres(system).ok())
            .unwrap_or_default();

        // Load player counts from game_metadata table (from LaunchBox import).
        // Used to fill empty game_library.players entries as a fallback.
        let lb_players: HashMap<String, u8> = state
            .metadata_db()
            .and_then(|guard| guard.as_ref()?.system_metadata_players(system).ok())
            .unwrap_or_default();

        // Load current game_library genres from L2 to know which are already set.
        let existing_genres: HashSet<String> = self
            .with_db_read(&storage, |db| {
                db.system_rom_genres(system)
                    .map(|map| map.into_keys().collect())
                    .unwrap_or_default()
            })
            .unwrap_or_default();

        // Load current game_library players from L2 to know which already have player data.
        let existing_players: HashSet<String> = self
            .with_db_read(&storage, |db| {
                db.system_rom_players(system).unwrap_or_default()
            })
            .unwrap_or_default();

        // Auto-match new ROMs: build a normalized-title index from existing
        // game_metadata entries so ROMs added after the last import can inherit
        // metadata from entries that share the same normalized title.
        let auto_matched_ratings = self.auto_match_metadata(state, system);

        // Merge auto-matched ratings into the main ratings map.
        let mut all_ratings = ratings;
        for (filename, rating) in &auto_matched_ratings {
            all_ratings.entry(filename.clone()).or_insert(*rating);
        }

        // Read current ROMs from L1 cache to get filenames.
        let rom_filenames: Vec<String> = if let Ok(guard) = self.roms.read() {
            guard
                .get(system)
                .map(|entry| {
                    entry
                        .data
                        .iter()
                        .map(|r| r.game.rom_filename.clone())
                        .collect()
                })
                .unwrap_or_default()
        } else {
            return;
        };

        if rom_filenames.is_empty() {
            return;
        }

        // Build enrichment tuples: (filename, box_art_url, genre, players, rating).
        // Genre and players are only filled from LaunchBox when game_library has no value.
        let enrichments: Vec<(String, Option<String>, Option<String>, Option<u8>, Option<f32>)> = rom_filenames
            .iter()
            .filter_map(|filename| {
                let art = self.resolve_box_art(state, &index, system, filename);
                let rating = all_ratings.get(filename).map(|&r| r as f32);
                let genre = if !existing_genres.contains(filename) {
                    lb_genres.get(filename).cloned()
                } else {
                    None
                };
                let players = if !existing_players.contains(filename) {
                    lb_players.get(filename).copied()
                } else {
                    None
                };
                if art.is_none() && rating.is_none() && genre.is_none() && players.is_none() {
                    return None;
                }
                Some((filename.clone(), art, genre, players, rating))
            })
            .collect();

        if enrichments.is_empty() {
            return;
        }

        let count = enrichments.len();
        // Use targeted SQL update for box_art_url, genre, and rating.
        self.with_db_mut(&storage, |db| {
            if let Err(e) = db.update_box_art_genre_rating(system, &enrichments) {
                tracing::warn!("Enrichment failed for {system}: {e}");
            }
        });

        // Also update L1 cache entries.
        if let Ok(mut guard) = self.roms.write()
            && let Some(entry) = guard.get_mut(system)
        {
            for rom in &mut entry.data {
                for (filename, art, _genre, players, rating) in &enrichments {
                    if rom.game.rom_filename == *filename {
                        if art.is_some() {
                            rom.box_art_url = art.clone();
                        }
                        // RomEntry doesn't carry genre — L1 genre is
                        // served via lookup_genre() which reads game_library.
                        if let Some(r) = rating {
                            rom.rating = Some(*r);
                        }
                        if rom.players.is_none() {
                            rom.players = *players;
                        }
                        break;
                    }
                }
            }
        }

        tracing::debug!("L2 enrichment: {system} — {count} ROMs updated with box art/genre/players/ratings");
    }

    /// Auto-match new ROMs against existing LaunchBox metadata by normalized title.
    ///
    /// For ROMs that have no `game_metadata` entry (not in `existing_ratings`),
    /// normalizes the ROM filename and looks for existing entries with the same
    /// normalized title. When a match is found, a new `game_metadata` row is
    /// created for the new ROM so future lookups hit directly.
    ///
    /// Returns a map of `rom_filename -> rating` for newly matched ROMs.
    fn auto_match_metadata(
        &self,
        state: &crate::api::AppState,
        system: &str,
    ) -> HashMap<String, f64> {
        use replay_control_core::launchbox::normalize_title;
        use replay_control_core::metadata_db::GameMetadata;
        use replay_control_core::systems::{self, SystemCategory};

        let storage = state.storage();
        let mut matched_ratings: HashMap<String, f64> = HashMap::new();

        // Load all existing metadata entries for this system.
        let all_metadata: Vec<(String, GameMetadata)> = state
            .metadata_db()
            .and_then(|guard| guard.as_ref()?.system_metadata_all(system).ok())
            .unwrap_or_default();

        // Nothing to match against if there's no imported metadata.
        if all_metadata.is_empty() {
            return matched_ratings;
        }

        let is_arcade =
            systems::find_system(system).is_some_and(|s| s.category == SystemCategory::Arcade);

        // Build a normalized-title -> metadata map from existing entries.
        let mut title_index: HashMap<String, &GameMetadata> = HashMap::new();
        for (rom_filename, meta) in &all_metadata {
            let stem = rom_filename
                .rfind('.')
                .map(|i| &rom_filename[..i])
                .unwrap_or(rom_filename);
            let normalized = if is_arcade {
                replay_control_core::arcade_db::lookup_arcade_game(stem)
                    .map(|info| normalize_title(info.display_name))
                    .unwrap_or_else(|| normalize_title(stem))
            } else {
                normalize_title(stem)
            };
            title_index.entry(normalized).or_insert(meta);
        }

        // Collect filenames of ROMs that already have metadata (by exact match).
        let has_metadata: HashSet<&str> = all_metadata
            .iter()
            .map(|(filename, _)| filename.as_str())
            .collect();

        // Read current ROMs from L1 cache.
        let rom_filenames: Vec<String> = if let Ok(guard) = self.roms.read() {
            guard
                .get(system)
                .map(|entry| {
                    entry
                        .data
                        .iter()
                        .map(|r| r.game.rom_filename.clone())
                        .collect()
                })
                .unwrap_or_default()
        } else {
            return matched_ratings;
        };

        // Find unmatched ROMs and try normalized-title lookup.
        let mut new_entries: Vec<(String, String, GameMetadata)> = Vec::new();
        for rom_filename in &rom_filenames {
            // Skip ROMs that already have a game_metadata entry.
            if has_metadata.contains(rom_filename.as_str()) {
                continue;
            }

            let stem = rom_filename
                .rfind('.')
                .map(|i| &rom_filename[..i])
                .unwrap_or(rom_filename);

            let normalized = if is_arcade {
                replay_control_core::arcade_db::lookup_arcade_game(stem)
                    .map(|info| normalize_title(info.display_name))
                    .unwrap_or_else(|| normalize_title(stem))
            } else {
                normalize_title(stem)
            };

            if let Some(donor_meta) = title_index.get(&normalized) {
                if let Some(rating) = donor_meta.rating {
                    matched_ratings.insert(rom_filename.clone(), rating);
                }
                // Persist the match so future lookups are direct.
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;
                new_entries.push((
                    system.to_string(),
                    rom_filename.clone(),
                    GameMetadata {
                        description: donor_meta.description.clone(),
                        rating: donor_meta.rating,
                        publisher: donor_meta.publisher.clone(),
                        developer: donor_meta.developer.clone(),
                        genre: donor_meta.genre.clone(),
                        players: donor_meta.players,
                        release_year: donor_meta.release_year,
                        cooperative: donor_meta.cooperative,
                        source: "launchbox-auto".to_string(),
                        fetched_at: now,
                        box_art_path: None,
                        screenshot_path: None,
                    },
                ));
            }
        }

        // Persist new matches to game_metadata.
        if !new_entries.is_empty() {
            let count = new_entries.len();
            self.with_db_mut(&storage, |db| {
                if let Err(e) = db.bulk_upsert(&new_entries) {
                    tracing::warn!("Auto-match metadata persist failed for {system}: {e}");
                }
            });
            tracing::info!("Auto-matched {count} new ROM(s) to existing metadata for {system}");
        }

        matched_ratings
    }
}

/// Normalize a title for matching against Wikidata entries.
///
/// Mirrors the `normalize_title_for_wikidata()` function used at build time:
/// lowercase, strip non-alphanumeric except spaces, collapse whitespace.
fn normalize_for_wikidata_match(title: &str) -> String {
    let trimmed = title.trim();
    let mut result = String::with_capacity(trimmed.len());
    for ch in trimmed.chars() {
        if ch.is_alphanumeric() || ch == ' ' {
            result.push(ch.to_ascii_lowercase());
        }
    }
    let parts: Vec<&str> = result.split_whitespace().collect();
    parts.join(" ")
}
