//! Generic single-flight cached snapshot.
//!
//! Tier 2 of the pool-design plan. Generalises the pattern used by
//! `metadata_snapshot.rs` so any future SSR page that wants
//! "compute-once-per-write-cycle, share across concurrent requests, fall
//! back to stale on transient unavailability" can opt in with a few lines.
//!
//! ## Why this shape
//!
//! - **Single-flight** via `RwLock<Option<T>>` + double-check inside the
//!   write lock means only one concurrent miss does the work; siblings
//!   wake up after the writer drops the guard and read the fresh value.
//! - **Stale-on-`None`** preserves whatever was previously cached when the
//!   builder returns `None` (DB transiently unavailable). Pages keep
//!   rendering during long writes instead of degrading to empty.
//! - **No background timer** — the cache is purely event-driven via
//!   `invalidate()`; entries don't decay by themselves. This avoids
//!   periodic recompute pressure on the pool.
//!
//! ## Usage
//!
//! ```rust,ignore
//! pub struct LibraryService {
//!     pub(super) my_page: SsrSnapshot<MyPagePayload>,
//! }
//!
//! impl LibraryService {
//!     pub async fn my_page_snapshot(&self, state: &AppState) -> MyPagePayload {
//!         self.my_page
//!             .get_or_compute("my_page", || async {
//!                 // Single pool.read closure for all DB queries.
//!                 // Off-pool synthesis afterwards.
//!                 build_my_page(state).await
//!             })
//!             .await
//!     }
//! }
//! ```
//!
//! Hook `invalidate()` calls into the same write-completion sites that
//! invalidate other caches (see `metadata_snapshot.rs` for an example).

use std::time::Instant;

use tokio::sync::RwLock;

/// Single-flight cached snapshot of `T`.
pub struct SsrSnapshot<T> {
    inner: RwLock<Option<T>>,
}

impl<T> Default for SsrSnapshot<T> {
    fn default() -> Self {
        Self {
            inner: RwLock::new(None),
        }
    }
}

impl<T: Clone + Default> SsrSnapshot<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Read-locked fast path: returns the cached value if present.
    #[allow(dead_code)] // Public API for future callers; only test-used today.
    pub async fn get_cached(&self) -> Option<T> {
        self.inner.read().await.clone()
    }

    /// Single-flight rebuild on miss. `compute` is called at most once per
    /// invalidation window; concurrent callers serialize on the write lock
    /// and the first arrival's result is shared.
    ///
    /// **Stale-on-`None`**: when `compute` returns `None`, keeps the
    /// previously-cached value in place rather than overwriting with the
    /// type's `Default`. The next caller after the underlying issue
    /// resolves will trigger a fresh rebuild.
    ///
    /// `label` is a static tag used for the rebuild-elapsed log line.
    pub async fn get_or_compute<F, Fut>(&self, label: &'static str, compute: F) -> T
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Option<T>>,
    {
        if let Some(cached) = self.inner.read().await.clone() {
            return cached;
        }
        let mut guard = self.inner.write().await;
        if let Some(cached) = guard.clone() {
            return cached;
        }
        let started = Instant::now();
        match compute().await {
            Some(fresh) => {
                let elapsed = started.elapsed();
                if elapsed.as_millis() > 200 {
                    tracing::info!("{label}: snapshot rebuilt in {elapsed:?}");
                } else {
                    tracing::debug!("{label}: snapshot rebuilt in {elapsed:?}");
                }
                *guard = Some(fresh.clone());
                fresh
            }
            None => match guard.clone() {
                Some(stale) => stale,
                None => T::default(),
            },
        }
    }

    /// Drop the cached value. Next `get_or_compute` rebuilds.
    pub async fn invalidate(&self) {
        *self.inner.write().await = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn cold_miss_rebuilds_once() {
        let cache: SsrSnapshot<u32> = SsrSnapshot::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let calls2 = calls.clone();
        let v = cache
            .get_or_compute("t", || async move {
                calls2.fetch_add(1, Ordering::SeqCst);
                Some(42)
            })
            .await;
        assert_eq!(v, 42);
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        // Warm path doesn't invoke compute.
        let v2 = cache
            .get_or_compute("t", || async {
                panic!("warm path must not call compute");
            })
            .await;
        assert_eq!(v2, 42);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn invalidate_drops_value() {
        let cache: SsrSnapshot<u32> = SsrSnapshot::new();
        cache.get_or_compute("t", || async { Some(7) }).await;
        assert_eq!(cache.get_cached().await, Some(7));
        cache.invalidate().await;
        assert_eq!(cache.get_cached().await, None);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn cold_miss_with_none_returns_default() {
        let cache: SsrSnapshot<u32> = SsrSnapshot::new();
        let v = cache.get_or_compute("t", || async { None }).await;
        assert_eq!(v, 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_misses_compute_once() {
        let cache: Arc<SsrSnapshot<u32>> = Arc::new(SsrSnapshot::new());
        let calls = Arc::new(AtomicUsize::new(0));

        // Fire several concurrent get_or_compute calls. The double-check
        // pattern means only one should actually run the compute; the rest
        // wake up and read the just-written value.
        let mut handles = Vec::new();
        for _ in 0..10 {
            let cache = cache.clone();
            let calls = calls.clone();
            handles.push(tokio::spawn(async move {
                cache
                    .get_or_compute("t", || {
                        let calls = calls.clone();
                        async move {
                            calls.fetch_add(1, Ordering::SeqCst);
                            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                            Some(99)
                        }
                    })
                    .await
            }));
        }
        for h in handles {
            assert_eq!(h.await.unwrap(), 99);
        }
        // Only the first arrival should have actually executed compute.
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "single-flight must coalesce concurrent misses"
        );
    }
}
