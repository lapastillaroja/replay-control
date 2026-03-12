pub mod favorites;
pub mod game_detail;
pub mod games;
pub mod github;
pub mod home;
pub mod hostname;
pub mod logs;
pub mod metadata;
pub mod more;
pub mod nfs;
pub mod search;
pub mod skin;
pub mod wifi;

// Re-export shared error display component.
pub use games::ErrorDisplay;
