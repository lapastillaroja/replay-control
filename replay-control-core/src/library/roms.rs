//! Wire types for ROM entries and system summaries.

use serde::{Deserialize, Serialize};

use crate::game_ref::GameRef;

/// Summary of a system's ROM collection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemSummary {
    pub folder_name: String,
    pub display_name: String,
    pub manufacturer: String,
    pub category: String,
    pub game_count: usize,
    pub total_size_bytes: u64,
}

/// A ROM file on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RomEntry {
    #[serde(flatten)]
    pub game: GameRef,
    /// File size in bytes
    pub size_bytes: u64,
    /// Whether this is an M3U playlist file
    pub is_m3u: bool,
    /// Whether this ROM is in the user's favorites
    #[serde(default)]
    pub is_favorite: bool,
    /// Box art image URL (relative path under /media/), populated by the app layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub box_art_url: Option<String>,
    /// Arcade driver emulation status (Working/Imperfect/Preliminary/Unknown).
    /// Only populated for arcade systems.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub driver_status: Option<String>,
    /// Game rating (0.0–5.0 scale), from metadata DB or game_db.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<f32>,
    /// Maximum number of players, from game_db or arcade_db.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub players: Option<u8>,
}
