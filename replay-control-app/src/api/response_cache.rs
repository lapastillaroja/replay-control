use std::sync::RwLock;
use std::time::{Duration, Instant};

use crate::server_fns::{GameSection, RecommendationData};

/// TTL for response-level cache entries.
const RESPONSE_TTL: Duration = Duration::from_secs(10);

/// Response-level cache for assembled recommendation payloads.
///
/// Caches the final serializable data returned by `get_recommendations` and
/// `get_favorites_recommendations` so that back-navigation and rapid reloads
/// (within the TTL window) skip all DB queries and box-art resolution.
///
/// Lives on `AppState` (not inside `LibraryService`) because it caches the fully
/// assembled server-function response, not raw library data.
pub struct ResponseCache {
    recommendations: RwLock<Option<(Instant, RecommendationData)>>,
    favorites_recommendations: RwLock<Option<(Instant, Vec<GameSection>)>>,
}

impl ResponseCache {
    pub fn new() -> Self {
        Self {
            recommendations: RwLock::new(None),
            favorites_recommendations: RwLock::new(None),
        }
    }

    /// Return cached recommendations if still within TTL.
    pub fn get_recommendations(&self) -> Option<RecommendationData> {
        let guard = self.recommendations.read().ok()?;
        let (instant, data) = guard.as_ref()?;
        if instant.elapsed() < RESPONSE_TTL {
            Some(data.clone())
        } else {
            None
        }
    }

    /// Store a fresh recommendations response.
    pub fn set_recommendations(&self, data: &RecommendationData) {
        if let Ok(mut guard) = self.recommendations.write() {
            *guard = Some((Instant::now(), data.clone()));
        }
    }

    /// Return cached favorites recommendations if still within TTL.
    pub fn get_favorites_recommendations(&self) -> Option<Vec<GameSection>> {
        let guard = self.favorites_recommendations.read().ok()?;
        let (instant, data) = guard.as_ref()?;
        if instant.elapsed() < RESPONSE_TTL {
            Some(data.clone())
        } else {
            None
        }
    }

    /// Store a fresh favorites recommendations response.
    pub fn set_favorites_recommendations(&self, data: &[GameSection]) {
        if let Ok(mut guard) = self.favorites_recommendations.write() {
            *guard = Some((Instant::now(), data.to_vec()));
        }
    }

    /// Clear all cached responses.
    pub fn invalidate_all(&self) {
        self.invalidate_recommendations();
        self.invalidate_favorites_recommendations();
    }

    /// Clear only the home page recommendations cache.
    pub fn invalidate_recommendations(&self) {
        if let Ok(mut guard) = self.recommendations.write() {
            *guard = None;
        }
    }

    /// Clear only the favorites recommendations cache.
    pub fn invalidate_favorites_recommendations(&self) {
        if let Ok(mut guard) = self.favorites_recommendations.write() {
            *guard = None;
        }
    }
}
