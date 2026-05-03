//! RetroAchievements API types and console ID mapping.
//!
//! Contains pure, wasm-safe types for RetroAchievements data.
//! The actual HTTP calls happen in `replay-control-core-server` or server functions.

use serde::{Deserialize, Serialize};

/// Maps a RePlayOS system folder name to a RetroAchievements console ID.
/// Returns `None` if the system is not supported by RetroAchievements.
pub fn system_to_ra_console_id(system: &str) -> Option<u32> {
    match system {
        "nintendo_nes" => Some(7),
        "nintendo_snes" => Some(3),
        "nintendo_n64" => Some(2),
        "nintendo_gb" => Some(4),
        "nintendo_gbc" => Some(6),
        "nintendo_gba" => Some(5),
        "nintendo_ds" => Some(78),
        "sega_sms" => Some(11),
        "sega_smd" => Some(1),
        "sega_gg" => Some(15),
        "sega_32x" => Some(14),
        "sega_cd" => Some(9),
        "sega_saturn" => Some(39),
        "sega_dc" => Some(40),
        "sony_psx" => Some(12),
        "sony_ps2" => Some(25),
        "sony_psp" => Some(41),
        "nintendo_ngc" => Some(16),
        "nintendo_wii" => Some(18),
        "nintendo_3ds" => Some(43),
        "atari_2600" => Some(26),
        "atari_7800" => Some(51),
        "atari_jaguar" => Some(17),
        "atari_lynx" => Some(13),
        "nec_tg16" => Some(8),
        "nec_pce_cd" => Some(52),
        "snk_ngp" => Some(14),
        "snk_ngpc" => Some(56),
        "snk_neogeo" => Some(56),
        "wonderswan" => Some(53),
        "wonderswan_color" => Some(54),
        "arcade_fbneo" => Some(27),
        "arcade_mame" => Some(44),
        "virtualboy" => Some(28),
        "apple2" => Some(37),
        "msx" => Some(11),
        "amstrad_cpc" => Some(42),
        _ => None,
    }
}

/// A game entry from the RetroAchievements game list API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaGame {
    #[serde(rename = "GameID")]
    pub game_id: u32,
    #[serde(rename = "GameTitle")]
    pub title: String,
    #[serde(rename = "ConsoleID")]
    pub console_id: u32,
    #[serde(rename = "ConsoleName")]
    pub console_name: String,
    #[serde(rename = "ImageIcon", default)]
    pub image_icon: String,
    #[serde(rename = "NumAchievements", default)]
    pub num_achievements: u32,
    #[serde(rename = "NumLeaderboards", default)]
    pub num_leaderboards: u32,
    #[serde(rename = "PointsTotal", default)]
    pub points_total: u32,
}

/// A single achievement from RetroAchievements.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaAchievement {
    #[serde(rename = "ID")]
    pub id: u32,
    #[serde(rename = "NumAwarded", default)]
    pub num_awarded: u32,
    #[serde(rename = "NumAwardedHardcore", default)]
    pub num_awarded_hardcore: u32,
    #[serde(rename = "Title")]
    pub title: String,
    #[serde(rename = "Description")]
    pub description: String,
    #[serde(rename = "Points")]
    pub points: u32,
    #[serde(rename = "TrueRatio", default)]
    pub true_ratio: u32,
    #[serde(rename = "Author")]
    pub author: String,
    #[serde(rename = "DateModified")]
    pub date_modified: String,
    #[serde(rename = "DateCreated")]
    pub date_created: String,
    #[serde(rename = "BadgeName")]
    pub badge_name: String,
    #[serde(rename = "DisplayOrder")]
    pub display_order: i32,
    #[serde(rename = "MemAddr", default)]
    pub mem_addr: String,
    #[serde(rename = "Flags", default)]
    pub flags: u32,
    #[serde(rename = "Type")]
    pub r#type: Option<String>,
}

/// Extended game info from RetroAchievements, including achievements.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaGameExtended {
    #[serde(rename = "ID")]
    pub id: u32,
    #[serde(rename = "Title")]
    pub title: String,
    #[serde(rename = "ConsoleID")]
    pub console_id: u32,
    #[serde(rename = "ConsoleName")]
    pub console_name: String,
    #[serde(rename = "ForumTopicID", default)]
    pub forum_topic_id: u32,
    #[serde(rename = "Flags", default)]
    pub flags: u32,
    #[serde(rename = "ImageIcon")]
    pub image_icon: String,
    #[serde(rename = "ImageTitle")]
    pub image_title: String,
    #[serde(rename = "ImageIngame")]
    pub image_ingame: String,
    #[serde(rename = "ImageBoxArt")]
    pub image_box_art: String,
    #[serde(rename = "Publisher")]
    pub publisher: String,
    #[serde(rename = "Developer")]
    pub developer: String,
    #[serde(rename = "Genre")]
    pub genre: String,
    #[serde(rename = "Released")]
    pub released: String,
    #[serde(rename = "IsFinal")]
    pub is_final: bool,
    #[serde(rename = "RichPresencePatch", default)]
    pub rich_presence_patch: String,
    #[serde(rename = "GuideURL", default)]
    pub guide_url: String,
    #[serde(rename = "Updated")]
    pub updated: String,
    #[serde(rename = "Claims", default)]
    pub claims: Vec<RaClaim>,
    #[serde(rename = "Achievements")]
    pub achievements: std::collections::HashMap<String, RaAchievement>,
}

/// A claim on a RetroAchievements game.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaClaim {
    #[serde(rename = "ID")]
    pub id: u32,
    #[serde(rename = "User")]
    pub user: String,
    #[serde(rename = "ClaimType")]
    pub claim_type: i32,
    #[serde(rename = "SetType")]
    pub set_type: i32,
    #[serde(rename = "Status")]
    pub status: i32,
    #[serde(rename = "Extension")]
    pub extension: i32,
    #[serde(rename = "Special")]
    pub special: i32,
    #[serde(rename = "Minutes")]
    pub minutes: u32,
    #[serde(rename = "Created")]
    pub created: String,
    #[serde(rename = "DoneTime")]
    pub done_time: String,
    #[serde(rename = "Updated")]
    pub updated: String,
    #[serde(rename = "GameID")]
    pub game_id: u32,
}

/// Result of searching for a game in RetroAchievements.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaSearchResult {
    pub game_id: u32,
    pub title: String,
    pub console_name: String,
    pub image_icon: String,
    pub match_score: f64,
    pub num_achievements: u32,
    pub points_total: u32,
}
