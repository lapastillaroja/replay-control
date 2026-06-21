// replay-control-core-server — native (linux) server-side implementation.
//
// Holds everything that touches rusqlite, deadpool-sqlite, tokio, reqwest,
// std::fs, std::process, or quick-xml. Pure types + wire contracts live in
// `replay-control-core` and are either re-exported at each module level here
// or referenced directly via `replay_control_core::*`.

pub mod catalog_pool;
pub mod data_dir;
pub mod db_pool;
pub mod external_metadata;
pub mod launch;
pub mod replay_api;
pub mod replay_service;
pub mod security;
pub mod settings;
pub mod sqlite;
pub mod storage_id;

pub use catalog_pool::{CatalogInitError, init_catalog, with_catalog};
pub use db_pool::{DbError, DbPool};

pub mod capture;
pub use capture::screenshots;

pub mod http;

pub mod update;

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
pub use library::save_states;

#[cfg(feature = "library")]
pub use library::boxart;
#[cfg(feature = "library")]
pub use library::db as library_db;
#[cfg(feature = "library")]
pub use library::enrichment;
#[cfg(feature = "library")]
pub use library::game_entry_builder;
#[cfg(feature = "library")]
pub use library::imports::launchbox;
#[cfg(feature = "library")]
pub use library::manuals::game_docs;
#[cfg(feature = "library")]
pub use library::manuals::retrokit as retrokit_manuals;
#[cfg(feature = "library")]
pub use library::matching::alias as alias_matching;
#[cfg(feature = "library")]
pub use library::rc_hash_disc;
#[cfg(feature = "library")]
pub use library::thumbnails;
#[cfg(feature = "library")]
pub use library::thumbnails::manifest as thumbnail_manifest;
#[cfg(feature = "library")]
pub use library::thumbnails::matching as image_matching;
#[cfg(feature = "library")]
pub use library::thumbnails::resolution as image_resolution;
#[cfg(feature = "library")]
pub use library::title_norm_reconcile;

#[cfg(feature = "library")]
pub mod user_data;
#[cfg(feature = "library")]
pub use user_data::db as user_data_db;

pub mod test_utils;
