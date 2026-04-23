use std::collections::{HashMap, HashSet};

use replay_control_core_server::storage::StorageLocation;

use super::LibraryService;

/// Per-system set of favorited filenames. Populated from `_favorites/*.fav`
/// and kept warm until invalidated (explicitly by mutation server_fns, or
/// by the inotify watcher on `_favorites/` changes).
pub(in crate::api) struct FavoritesCache {
    pub(in crate::api) data: HashMap<String, HashSet<String>>,
}

impl FavoritesCache {
    pub(in crate::api) async fn load(storage: &StorageLocation) -> Self {
        let all_favs = replay_control_core_server::favorites::list_favorites(storage)
            .await
            .unwrap_or_default();
        let mut data: HashMap<String, HashSet<String>> = HashMap::new();
        for fav in all_favs {
            data.entry(fav.game.system.clone())
                .or_default()
                .insert(fav.game.rom_filename.clone());
        }
        Self { data }
    }
}

impl LibraryService {
    /// Ensure the favorites cache is populated and apply `f` to its data.
    ///
    /// Single-flight on miss: only the first concurrent caller walks the
    /// favorites directory; the rest wait on the write lock and read the
    /// freshly-populated cache.
    async fn with_favorites<R>(
        &self,
        storage: &StorageLocation,
        f: impl Fn(&HashMap<String, HashSet<String>>) -> R,
    ) -> R {
        if let Some(ref cache) = *self.favorites.read().await {
            return f(&cache.data);
        }

        let mut guard = self.favorites.write().await;
        if let Some(ref cache) = *guard {
            return f(&cache.data);
        }

        let new_cache = FavoritesCache::load(storage).await;
        let result = f(&new_cache.data);
        *guard = Some(new_cache);
        result
    }

    /// Get the set of favorited filenames for a system.
    /// Uses a cached favorites list to avoid per-request filesystem reads.
    pub async fn get_favorites_set(
        &self,
        storage: &StorageLocation,
        system: &str,
    ) -> HashSet<String> {
        self.with_favorites(storage, |data| {
            data.get(system).cloned().unwrap_or_default()
        })
        .await
    }

    /// Get the most-favorited system and its favorited filenames.
    /// Uses the cached favorites — no filesystem access on cache hit.
    pub async fn get_top_favorited_system(
        &self,
        storage: &StorageLocation,
    ) -> Option<(String, Vec<String>)> {
        self.with_favorites(storage, Self::top_system_from_data)
            .await
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
    pub async fn get_all_favorited_systems(
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
        .await
    }

    /// Get the total count of favorited games (all systems).
    /// Uses the cached favorites to avoid filesystem traversal.
    pub async fn get_favorites_count(&self, storage: &StorageLocation) -> usize {
        self.with_favorites(storage, |data| data.values().map(|s| s.len()).sum())
            .await
    }
}
