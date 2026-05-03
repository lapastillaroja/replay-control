//! Wire types for user-data DB records (videos, game status, etc.).
//! Native SQL operations live in `replay_control_core_server::user_data_db`.

use serde::{Deserialize, Serialize};

/// A saved video reference (YouTube link, longplay, trailer, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoEntry {
    pub id: String,
    pub url: String,
    pub platform: String,
    pub video_id: String,
    pub title: Option<String>,
    pub added_at: u64,
    pub from_recommendation: bool,
    pub tag: Option<String>,
    pub rom_filename: String,
}

/// User-defined play status for a game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GameStatus {
    /// Want to play (backlog)
    WantToPlay,
    /// Currently playing
    InProgress,
    /// Finished the game
    Completed,
    /// 100% completion (all RetroAchievements, etc.)
    Platinum,
}

impl GameStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            GameStatus::WantToPlay => "want_to_play",
            GameStatus::InProgress => "in_progress",
            GameStatus::Completed => "completed",
            GameStatus::Platinum => "platinum",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "want_to_play" => Some(GameStatus::WantToPlay),
            "in_progress" => Some(GameStatus::InProgress),
            "completed" => Some(GameStatus::Completed),
            "platinum" => Some(GameStatus::Platinum),
            _ => None,
        }
    }
}

/// A game with its user-assigned status, enriched with display info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusGameEntry {
    pub system: String,
    pub rom_filename: String,
    pub display_name: String,
    pub status: GameStatus,
    pub box_art_url: Option<String>,
    pub genre: Option<String>,
    pub updated_at: u64,
}
