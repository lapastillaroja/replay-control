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
pub struct GameRef {
    pub system: String,
    pub system_display: String,
    pub rom_filename: String,
    pub display_name: Option<String>,
    pub rom_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RomEntry {
    #[serde(flatten)]
    pub game: GameRef,
    pub size_bytes: u64,
    pub is_m3u: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Favorite {
    #[serde(flatten)]
    pub game: GameRef,
    pub marker_filename: String,
    pub subfolder: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentEntry {
    #[serde(flatten)]
    pub game: GameRef,
    pub marker_filename: String,
    pub last_played: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RomDetail {
    pub rom: RomEntry,
    pub is_favorite: bool,
    pub arcade_info: Option<ArcadeMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArcadeMetadata {
    pub year: String,
    pub manufacturer: String,
    pub players: u8,
    pub rotation: String,
    pub category: String,
    pub is_clone: bool,
    pub parent: String,
}
