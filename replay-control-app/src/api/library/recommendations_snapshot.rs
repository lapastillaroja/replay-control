//! Single-flight cached snapshot of the home-page recommendation payload.
//!
//! Mirrors `metadata_snapshot.rs` shape: this module just delegates to
//! `compute_recommendations` in the server-fns module (where the queries
//! and helpers already live) and is plumbed into the generic
//! `SsrSnapshot<T>` helper on `LibraryService`.
//!
//! Replaces the previous `response_cache.recommendations: TtlSlot<...>`
//! with strictly better caching: event-driven invalidation via the same
//! write-completion sites that already invalidate the metadata snapshot,
//! single-flight rebuild on miss, and stale-on-`None` so the home page
//! keeps rendering during long writes.

use crate::api::AppState;
use crate::server_fns::RecommendationData;

/// Build the snapshot. Returns `None` only when the underlying compute
/// can't make progress — `SsrSnapshot::get_or_compute` then keeps the
/// previous (stale) snapshot rather than caching `None`.
///
/// `count = 6` matches the home page's canonical request and the previous
/// TtlSlot's first-caller-wins behaviour (the only caller in the codebase
/// today is `home.rs` with `count=6`).
pub(super) async fn compute(state: &AppState) -> Option<RecommendationData> {
    crate::server_fns::recommendations::compute_recommendations(state, 6).await
}
