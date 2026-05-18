//! Canonical names used in `library_game_resource.resource_type` and
//! `library_game_resource.source`.
//!
//! Defined in the pure core crate so both `ssr` (writer side) and
//! `hydrate` (UI partitioning by type/source) reference one source of
//! truth. Bare string keys for these columns are easy to typo into
//! silent "no manuals found" / "no link surfaced" bugs.

/// `resource_type` for per-ROM manual links / PDFs / scans.
pub const MANUAL: &str = "manual";

/// `resource_type` for per-ROM video recommendations.
pub const VIDEO: &str = "video";

/// `resource_type` for external strategy guide deep links (currently
/// populated only from Shmups Wiki).
pub const STRATEGY_GUIDE: &str = "strategy_guide";

/// `resource_type` for external video index deep links (currently
/// populated only from Shmups Wiki sub-pages under `Category:Video Index`).
pub const VIDEO_INDEX: &str = "video_index";

/// `source` value identifying rows derived from the bundled Shmups Wiki
/// page-title index.
pub const SHMUPS_WIKI_SOURCE: &str = "shmups_wiki";
