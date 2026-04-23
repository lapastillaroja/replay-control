pub mod error;
pub mod locale;
pub mod search_scoring;
pub mod update;

mod platform;
pub use platform::systems;

mod game;
pub use game::date_precision;
pub use game::date_precision::DatePrecision;
pub use game::developer;
pub use game::genre;
pub use game::rom_tags;
pub use game::title_utils;

pub mod settings;
pub use settings::skins;

mod capture;
pub use capture::video_url;
