use serde::{Deserialize, Serialize};

/// Per-system game counts and storage usage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemStat {
    pub system: String,
    pub display_name: String,
    pub game_count: usize,
    pub size_bytes: u64,
    pub favorite_count: usize,
}

/// Genre distribution entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenreStat {
    pub genre: String,
    pub count: usize,
    /// Percentage of total games with known genre (0-100).
    pub percentage: f64,
}

/// Games grouped by decade.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecadeStat {
    pub decade: u16,
    pub count: usize,
}

/// Developer distribution entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeveloperStat {
    pub developer: String,
    pub count: usize,
    pub game_count: usize,
}

/// Player mode breakdown.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerModeStat {
    pub single_player: usize,
    pub multiplayer: usize,
    pub cooperative: usize,
    pub unknown: usize,
}

/// Library quality/variant breakdown.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariantStat {
    pub clones: usize,
    pub hacks: usize,
    pub translations: usize,
    pub special: usize,
    pub verified: usize,
}

/// Metadata coverage percentages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataCoverage {
    pub with_genre: usize,
    pub genre_pct: f64,
    pub with_developer: usize,
    pub developer_pct: f64,
    pub with_rating: usize,
    pub rating_pct: f64,
    pub with_boxart: usize,
    pub boxart_pct: f64,
    pub with_screenshot: usize,
    pub screenshot_pct: f64,
}

/// Top-level library summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibrarySummary {
    pub total_games: usize,
    pub total_systems: usize,
    pub total_size_bytes: u64,
    pub total_favorites: usize,
    pub arcade_count: usize,
}

/// Complete stats dashboard payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsDashboard {
    pub summary: LibrarySummary,
    pub systems: Vec<SystemStat>,
    pub genres: Vec<GenreStat>,
    pub decades: Vec<DecadeStat>,
    pub developers: Vec<DeveloperStat>,
    pub player_modes: PlayerModeStat,
    pub variants: VariantStat,
    pub metadata_coverage: MetadataCoverage,
}
