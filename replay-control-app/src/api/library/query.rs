use std::sync::RwLock;

/// Query-level cache for slow-changing pill data used by recommendations.
///
/// These values (top genres, developers, decades, active systems) change only
/// when the game library changes — not on every request. Caching them avoids
/// redundant aggregate queries on every home page load.
///
/// No TTL: invalidated explicitly via `invalidate_all()` when the library
/// changes (import, rebuild, ROM add/delete, region preference change).
pub(crate) struct QueryCache {
    top_genres: RwLock<Option<Vec<String>>>,
    top_developers: RwLock<Option<Vec<String>>>,
    decades: RwLock<Option<Vec<u16>>>,
    active_systems: RwLock<Option<Vec<String>>>,
}

impl QueryCache {
    pub(crate) fn new() -> Self {
        Self {
            top_genres: RwLock::new(None),
            top_developers: RwLock::new(None),
            decades: RwLock::new(None),
            active_systems: RwLock::new(None),
        }
    }

    pub(crate) fn get_top_genres(&self) -> Option<Vec<String>> {
        self.top_genres.read().ok()?.clone()
    }

    pub(crate) fn set_top_genres(&self, genres: &[String]) {
        if let Ok(mut guard) = self.top_genres.write() {
            *guard = Some(genres.to_vec());
        }
    }

    pub(crate) fn get_top_developers(&self) -> Option<Vec<String>> {
        self.top_developers.read().ok()?.clone()
    }

    pub(crate) fn set_top_developers(&self, developers: &[String]) {
        if let Ok(mut guard) = self.top_developers.write() {
            *guard = Some(developers.to_vec());
        }
    }

    pub(crate) fn get_decades(&self) -> Option<Vec<u16>> {
        self.decades.read().ok()?.clone()
    }

    pub(crate) fn set_decades(&self, decades: &[u16]) {
        if let Ok(mut guard) = self.decades.write() {
            *guard = Some(decades.to_vec());
        }
    }

    pub(crate) fn get_active_systems(&self) -> Option<Vec<String>> {
        self.active_systems.read().ok()?.clone()
    }

    pub(crate) fn set_active_systems(&self, systems: &[String]) {
        if let Ok(mut guard) = self.active_systems.write() {
            *guard = Some(systems.to_vec());
        }
    }

    /// Clear all cached query results.
    pub(crate) fn invalidate_all(&self) {
        if let Ok(mut guard) = self.top_genres.write() {
            *guard = None;
        }
        if let Ok(mut guard) = self.top_developers.write() {
            *guard = None;
        }
        if let Ok(mut guard) = self.decades.write() {
            *guard = None;
        }
        if let Ok(mut guard) = self.active_systems.write() {
            *guard = None;
        }
    }
}
