use std::sync::RwLock;
use std::time::{Duration, Instant};

use crate::server_fns::{GameSection, RecommendationData};

/// TTL for response-level cache entries. Short on purpose: long enough to
/// absorb back-navigation and rapid reloads, short enough that the UI still
/// feels live when background activity (enrichment, downloads, scans) updates
/// the underlying data without going through `invalidate_all()`.
const RESPONSE_TTL: Duration = Duration::from_secs(10);

/// Single TTL-gated slot holding at most one value.
pub struct TtlSlot<T: Clone> {
    inner: RwLock<Option<(Instant, T)>>,
}

impl<T: Clone> Default for TtlSlot<T> {
    fn default() -> Self {
        Self {
            inner: RwLock::new(None),
        }
    }
}

impl<T: Clone> TtlSlot<T> {
    pub fn get(&self) -> Option<T> {
        let guard = self.inner.read().ok()?;
        let (instant, data) = guard.as_ref()?;
        (instant.elapsed() < RESPONSE_TTL).then(|| data.clone())
    }

    pub fn set(&self, data: T) {
        if let Ok(mut guard) = self.inner.write() {
            *guard = Some((Instant::now(), data));
        }
    }

    pub fn invalidate(&self) {
        if let Ok(mut guard) = self.inner.write() {
            *guard = None;
        }
    }
}

/// Response-level cache for assembled recommendation payloads.
///
/// Caches the final serializable data returned by `get_recommendations` and
/// `get_favorites_recommendations` so that back-navigation and rapid reloads
/// skip all DB queries and box-art resolution.
///
/// Lives on `AppState` (not inside `LibraryService`) because it caches the fully
/// assembled server-function response, not raw library data.
#[derive(Default)]
pub struct ResponseCache {
    pub recommendations: TtlSlot<RecommendationData>,
    pub favorites_recommendations: TtlSlot<Vec<GameSection>>,
}

impl ResponseCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn invalidate_all(&self) {
        self.recommendations.invalidate();
        self.favorites_recommendations.invalidate();
    }
}
