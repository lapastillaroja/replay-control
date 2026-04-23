//! Wire types for user-data DB records (videos, etc.).
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
