pub mod developer;
pub mod favorites;
pub mod game_detail;
pub mod games;
pub mod github;
pub mod home;
pub mod hostname;
pub mod logs;
pub mod metadata;
pub mod my_games;
pub mod nfs;
pub mod password;
pub mod search;
pub mod settings;
pub mod skin;
pub mod updating;
pub mod wifi;

// Re-export shared error display component.
pub use games::ErrorDisplay;
