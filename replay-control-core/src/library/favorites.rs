//! Wire types for favorites. Native I/O lives in
//! `replay_control_core_server::library::favorites`.

use serde::{Deserialize, Serialize};

use crate::game_ref::GameRef;

/// A parsed favorite entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Favorite {
    #[serde(flatten)]
    pub game: GameRef,
    /// The .fav marker filename (e.g., "sega_smd@Sonic.md.fav")
    pub marker_filename: String,
    /// Subfolder within _favorites (empty string if at root)
    pub subfolder: String,
    /// Unix timestamp when the favorite was added (from file mtime)
    pub date_added: u64,
}

/// Criteria for organizing favorites into subfolders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrganizeCriteria {
    System,
    Genre,
    Players,
    Rating,
    Alphabetical,
    Developer,
}

/// Result of an organize or flatten operation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct OrganizeResult {
    pub organized: usize,
    pub skipped: usize,
}
