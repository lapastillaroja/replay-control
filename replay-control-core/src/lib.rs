pub mod error;
pub mod locale;
pub mod search_scoring;
pub mod update;

mod platform;
pub use platform::systems;

pub mod game;
pub use game::date_precision;
pub use game::date_precision::DatePrecision;
pub use game::developer;
pub use game::game_ref;
pub use game::genre;
pub use game::ra_types;
pub use game::rom_tags;
pub use game::title_utils;

pub mod library;
pub use library::db as library_db;
pub use library::favorites;
pub use library::manuals::game_docs;
pub use library::manuals::retrokit as retrokit_manuals;
pub use library::recents;
pub use library::roms;

pub mod user_data;
pub use user_data as user_data_db;

pub mod settings;
pub use settings::skins;

pub mod want_to_play;

mod capture;
pub use capture::video_url;

pub mod stats;
pub use stats::*;
