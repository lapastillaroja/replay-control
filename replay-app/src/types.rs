/// Mirror types used on the client (hydrate) side.
/// These match the replay-core types that server functions serialize.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemSummary {
    pub folder_name: String,
    pub display_name: String,
    pub manufacturer: String,
    pub category: String,
    pub game_count: usize,
    pub total_size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RomEntry {
    pub system: String,
    pub system_display: String,
    pub filename: String,
    pub relative_path: String,
    pub size_bytes: u64,
    pub is_m3u: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Favorite {
    pub filename: String,
    pub system: String,
    pub system_display: String,
    pub rom_filename: String,
    pub rom_path: String,
    pub subfolder: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentEntry {
    pub filename: String,
    pub system: String,
    pub system_display: String,
    pub rom_filename: String,
    pub rom_path: String,
    pub last_played: u64,
}
