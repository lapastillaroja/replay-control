use std::collections::HashMap;
use std::time::{Duration, Instant};

use replay_control_core::roms::{RomEntry, SystemSummary};
use replay_control_core::storage::StorageLocation;

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
    pub(crate) fn new() -> Self {
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
