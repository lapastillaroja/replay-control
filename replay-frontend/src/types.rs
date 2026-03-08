use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SystemInfo {
    pub storage_kind: String,
    pub storage_root: String,
    pub disk_total_bytes: u64,
    pub disk_used_bytes: u64,
    pub disk_available_bytes: u64,
    pub total_systems: usize,
    pub systems_with_games: usize,
    pub total_games: usize,
    pub total_favorites: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SystemSummary {
    pub folder_name: String,
    pub display_name: String,
    pub manufacturer: String,
    pub category: String,
    pub game_count: usize,
    pub total_size_bytes: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RomEntry {
    pub system: String,
    pub system_display: String,
    pub filename: String,
    pub relative_path: String,
    pub size_bytes: u64,
    pub is_m3u: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Favorite {
    pub filename: String,
    pub system: String,
    pub system_display: String,
    pub rom_filename: String,
    pub rom_path: String,
    pub subfolder: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RecentEntry {
    pub filename: String,
    pub system: String,
    pub system_display: String,
    pub rom_filename: String,
    pub rom_path: String,
    pub last_played: u64,
}
