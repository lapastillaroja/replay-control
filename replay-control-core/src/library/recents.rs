//! Wire types for recent games.

use serde::{Deserialize, Serialize};

use crate::game_ref::GameRef;

/// A recently played game entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentEntry {
    #[serde(flatten)]
    pub game: GameRef,
    /// The marker filename identifying this entry.
    pub marker_filename: String,
    /// Unix timestamp when the game was last played (from file mtime).
    pub last_played: u64,
}
