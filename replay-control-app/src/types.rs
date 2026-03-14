/// Mirror types used on the client (hydrate) side.
/// These match the replay-core types that server functions serialize.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrganizeCriteria {
    System,
    Genre,
    Players,
    Rating,
    Alphabetical,
}

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
    #[serde(default)]
    pub is_favorite: bool,
    #[serde(default)]
    pub box_art_url: Option<String>,
    #[serde(default)]
    pub driver_status: Option<String>,
    #[serde(default)]
    pub rating: Option<f32>,
    #[serde(default)]
    pub players: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Favorite {
    #[serde(flatten)]
    pub game: GameRef,
    pub marker_filename: String,
    pub subfolder: String,
    pub date_added: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentEntry {
    #[serde(flatten)]
    pub game: GameRef,
    pub marker_filename: String,
    pub last_played: u64,
}

/// Mirror of `replay_control_core::metadata_db::ImportStats` for WASM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportStats {
    pub total_source: usize,
    pub matched: usize,
    pub inserted: usize,
    pub skipped: usize,
}

/// Mirror of `replay_control_core::metadata_db::MetadataStats` for WASM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataStats {
    pub total_entries: usize,
    pub with_description: usize,
    pub with_rating: usize,
    pub db_size_bytes: u64,
    pub last_updated_text: String,
}

/// Mirror of `replay_control_core::metadata_db::ImportState` for WASM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImportState {
    Downloading,
    BuildingIndex,
    Parsing,
    Complete,
    Failed,
}

/// Mirror of `replay_control_core::metadata_db::ImportProgress` for WASM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportProgress {
    pub state: ImportState,
    pub processed: usize,
    pub matched: usize,
    pub inserted: usize,
    pub elapsed_secs: u64,
    pub error: Option<String>,
}

/// Per-system metadata coverage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemCoverage {
    pub system: String,
    pub display_name: String,
    pub total_games: usize,
    pub with_metadata: usize,
    pub with_thumbnail: usize,
}

/// Mirror of `replay_control_core::user_data_db::VideoEntry` for WASM.
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
}
