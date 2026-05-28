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

/// `resource_type` for box-art image URLs contributed via the community
/// metadata source. The runtime download wiring is a follow-up; the catalog
/// holds the URLs today.
pub const BOXART: &str = "boxart";

/// `resource_type` for title-screen image URLs from community metadata.
pub const TITLE_IMAGE: &str = "title";

/// `resource_type` for in-game screenshot URLs from community metadata.
/// Multiple rows per `(system, normalized_title)` are expected.
pub const SCREENSHOT: &str = "snap";

/// `source` value identifying rows derived from the Shmups Wiki catalog
/// resource index.
pub const SHMUPS_WIKI_SOURCE: &str = "shmups_wiki";

/// `source` value identifying manual rows from MiSTer Manual Downloader.
pub const MISTER_MANUALS_SOURCE: &str = "mister_manuals";

/// `source` value identifying manual rows from the Retrokit manuals archive.
pub const RETROKIT_SOURCE: &str = "retrokit";

/// `source` value identifying rows from the community-curated JSON files in
/// `data/community/`. Used for both `canonical_game.source` and
/// `catalog_game_resource.source`.
pub const COMMUNITY_SOURCE: &str = "community";

/// `source` value identifying canonical-game rows derived from No-Intro DATs.
pub const NOINTRO_SOURCE: &str = "no-intro";

/// `catalog_game_resource.system` value for resources that can match any
/// library system.
pub const GLOBAL_SYSTEM: &str = "*";
