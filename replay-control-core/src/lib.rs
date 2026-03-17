pub mod error;
pub mod launch;

mod platform;
pub use platform::config;
pub use platform::storage;
pub use platform::systems;

mod game;
pub use game::arcade_db;
pub use game::game_db;
pub use game::game_ref;
pub use game::genre;
pub use game::rom_tags;
pub use game::series_db;
pub use game::title_utils;

mod library;
pub use library::favorites;
pub use library::recents;
pub use library::rom_hash;
pub use library::roms;

pub mod settings;
pub use settings::skins;

mod capture;
pub use capture::screenshots;
pub use capture::video_url;

#[cfg(feature = "metadata")]
mod metadata;
#[cfg(feature = "metadata")]
pub use metadata::db_common;
#[cfg(feature = "metadata")]
pub use metadata::image_matching;
#[cfg(feature = "metadata")]
pub use metadata::launchbox;
#[cfg(feature = "metadata")]
pub use metadata::metadata_db;
#[cfg(feature = "metadata")]
pub use metadata::thumbnail_manifest;
#[cfg(feature = "metadata")]
pub use metadata::thumbnails;
#[cfg(feature = "metadata")]
pub use metadata::user_data_db;
