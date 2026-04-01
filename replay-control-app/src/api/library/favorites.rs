use std::collections::{HashMap, HashSet};
use std::path::Path;

use replay_control_core::storage::StorageLocation;

use super::{Freshness, LibraryService};

/// Cached favorites: per-system set of favorited filenames.
///
/// Local storage uses no TTL (inotify + mtime + explicit invalidation suffice).
/// NFS uses a 30-minute TTL as a safety net.
pub(in crate::api) struct FavoritesCache {
    /// system -> set of ROM filenames that are favorited.
    pub(in crate::api) data: HashMap<String, HashSet<String>>,
    freshness: Freshness,
}

impl FavoritesCache {
    pub(in crate::api) fn new(storage: &StorageLocation) -> Self {
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
            freshness: Freshness::new(&favs_dir, storage.kind.is_local()),
        }
    }

    pub(in crate::api) fn is_fresh(&self, favs_dir: &Path) -> bool {
        self.freshness.is_fresh(favs_dir)
    }
}

impl LibraryService {
    /// Ensure the favorites cache is fresh and apply `f` to its data.
    /// On cache hit, reads under a read lock. On miss, rebuilds the cache,
    /// stores it, and applies `f` to the fresh data.
    fn with_favorites<R>(
        &self,
        storage: &StorageLocation,
        f: impl Fn(&HashMap<String, HashSet<String>>) -> R,
    ) -> R {
        let favs_dir = storage.favorites_dir();

        // Try read lock first.
        if let Ok(guard) = self.favorites.read()
            && let Some(ref cache) = *guard
            && cache.is_fresh(&favs_dir)
        {
            return f(&cache.data);
        }

        // Cache miss — rebuild.
        let new_cache = FavoritesCache::new(storage);
        let result = f(&new_cache.data);
        if let Ok(mut guard) = self.favorites.write() {
            *guard = Some(new_cache);
        }
        result
    }

    /// Get the set of favorited filenames for a system.
    /// Uses a cached favorites list to avoid per-request filesystem reads.
    pub fn get_favorites_set(&self, storage: &StorageLocation, system: &str) -> HashSet<String> {
        self.with_favorites(storage, |data| {
            data.get(system).cloned().unwrap_or_default()
        })
    }

    /// Get the most-favorited system and its favorited filenames.
    /// Uses the cached favorites — no filesystem access on cache hit.
    pub fn get_top_favorited_system(
        &self,
        storage: &StorageLocation,
    ) -> Option<(String, Vec<String>)> {
        self.with_favorites(storage, Self::top_system_from_data)
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
        self.with_favorites(storage, |data| {
            let result: HashMap<String, Vec<String>> = data
                .iter()
                .filter(|(_, files)| !files.is_empty())
                .map(|(system, files)| (system.clone(), files.iter().cloned().collect()))
                .collect();
            if result.is_empty() {
                None
            } else {
                Some(result)
            }
        })
    }

    /// Get the total count of favorited games (all systems).
    /// Uses the cached favorites to avoid filesystem traversal.
    pub fn get_favorites_count(&self, storage: &StorageLocation) -> usize {
        self.with_favorites(storage, |data| data.values().map(|s| s.len()).sum())
    }
}
