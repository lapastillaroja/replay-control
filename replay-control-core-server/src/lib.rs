// replay-control-core-server — native (linux) server-side implementation.
//
// Holds everything that touches rusqlite, deadpool-sqlite, tokio, reqwest,
// std::fs, std::process, or quick-xml. Pure types + wire contracts live in
// `replay-control-core` and are either re-exported at each module level here
// or referenced directly via `replay_control_core::*`.

pub mod catalog_pool;
pub mod launch;
pub mod settings;

pub use catalog_pool::{CatalogInitError, init_catalog, with_catalog};

pub mod capture;
pub use capture::screenshots;

#[cfg(feature = "http")]
pub mod http;

pub mod platform;
pub use platform::config;
pub use platform::storage;

pub mod game;
pub use game::arcade_db;
pub use game::game_db;
pub use game::game_ref;
pub use game::series_db;

pub mod library;
pub use library::favorites;
pub use library::recents;
pub use library::rom_hash;
pub use library::roms;

#[cfg(feature = "metadata")]
pub mod metadata;
#[cfg(feature = "metadata")]
pub use metadata::alias_matching;
#[cfg(feature = "metadata")]
pub use metadata::db_common;
#[cfg(feature = "metadata")]
pub use metadata::enrichment;
#[cfg(feature = "metadata")]
pub use metadata::game_docs;
#[cfg(feature = "metadata")]
pub use metadata::game_entry_builder;
#[cfg(feature = "metadata")]
pub use metadata::image_matching;
#[cfg(feature = "metadata")]
pub use metadata::image_resolution;
#[cfg(feature = "metadata")]
pub use metadata::launchbox;
#[cfg(feature = "metadata")]
pub use metadata::metadata_db;
#[cfg(feature = "metadata")]
pub use metadata::metadata_matching;
#[cfg(feature = "metadata")]
pub use metadata::retrokit_manuals;
#[cfg(feature = "metadata")]
pub use metadata::thumbnail_manifest;
#[cfg(feature = "metadata")]
pub use metadata::thumbnails;
#[cfg(feature = "metadata")]
pub use metadata::user_data_db;
