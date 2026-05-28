//! Community-curated metadata source.
//!
//! Pure types shared between `tools/build-catalog` (the loader) and the
//! native server crate (downstream consumers). No fs, no rusqlite — see
//! `tools/build-catalog/src/community.rs` for the loader that reads
//! `data/community/<system>.json` and writes catalog rows.

pub mod schema;

pub use schema::{
    CommunityEntry, CommunityFile, LinkResource, LocalizedText, ManualResource, VideoResource,
};
