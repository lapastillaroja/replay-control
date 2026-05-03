use super::*;

use replay_control_core::game::ra_types::{RaGame, RaGameExtended};

/// Lightweight achievement info returned to the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AchievementInfo {
    pub id: u32,
    pub title: String,
    pub description: String,
    pub points: u32,
    pub badge_url: String,
    pub author: String,
    pub r#type: Option<String>,
}

/// Full game info with achievements returned to the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaGameInfo {
    pub game_id: u32,
    pub title: String,
    pub console_name: String,
    pub image_icon: String,
    pub image_title: String,
    pub image_ingame: String,
    pub image_box_art: String,
    pub developer: String,
    pub publisher: String,
    pub genre: String,
    pub released: String,
    pub achievements: Vec<AchievementInfo>,
    pub total_points: u32,
}

/// Result of a game search in RetroAchievements.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaSearchResult {
    pub game_id: u32,
    pub title: String,
    pub console_name: String,
    pub image_icon: String,
    pub num_achievements: u32,
    pub points_total: u32,
}

#[cfg(feature = "ssr")]
const RA_API_BASE: &str = "https://api.retroachievements.org/API";

/// Compute a simple token-based similarity score between two strings.
/// Returns a value between 0.0 and 1.0.
#[cfg(feature = "ssr")]
fn similarity_score(a: &str, b: &str) -> f64 {
    let a_lower = a.to_lowercase();
    let b_lower = b.to_lowercase();

    if a_lower == b_lower {
        return 1.0;
    }

    let a_tokens: std::collections::HashSet<&str> = a_lower
        .split_whitespace()
        .filter(|t| t.len() > 2)
        .collect();
    let b_tokens: std::collections::HashSet<&str> = b_lower
        .split_whitespace()
        .filter(|t| t.len() > 2)
        .collect();

    if a_tokens.is_empty() || b_tokens.is_empty() {
        return 0.0;
    }

    let common: usize = a_tokens.intersection(&b_tokens).count();
    let max_len = a_tokens.len().max(b_tokens.len());
    common as f64 / max_len as f64
}

/// Strip common suffixes and clean up a game title for matching.
#[cfg(feature = "ssr")]
fn normalize_title(title: &str) -> String {
    let mut result = title.to_string();

    let suffixes = [
        " (USA)",
        " (Europe)",
        " (Japan)",
        " (Japan, USA)",
        " (USA, Europe)",
        " (World)",
        " (EU)",
        " (JP)",
        " (US)",
        " (UE)",
        " [!]",
        " [b]",
        " [f]",
        " [o]",
        " (Rev A)",
        " (Rev B)",
        " (Rev 1)",
        " (Version 1)",
    ];

    for suffix in &suffixes {
        if result.ends_with(suffix) {
            result = result[..result.len() - suffix.len()].to_string();
            break;
        }
    }

    result.trim().to_string()
}

/// Search for a game in RetroAchievements by title and console.
/// Returns a list of potential matches sorted by relevance.
#[cfg(feature = "ssr")]
async fn search_ra_games(
    api_key: &str,
    console_id: u32,
    title: &str,
) -> Result<Vec<RaSearchResult>, ServerFnError> {
    use replay_control_core_server::http::get_json_with_timeout;

    let url = format!(
        "{}/GetGameList.php?z={}&i={}&f=1",
        RA_API_BASE, api_key, console_id
    );

    let json = get_json_with_timeout(
        &url,
        std::time::Duration::from_secs(15),
    )
    .await
    .map_err(|e| ServerFnError::new(format!("Failed to fetch RA game list: {e}")))?;

    let games: Vec<RaGame> = if let Some(arr) = json.as_array() {
        arr.iter()
            .filter_map(|v| serde_json::from_value(v.clone()).ok())
            .collect()
    } else {
        Vec::new()
    };

    let search_title = normalize_title(title);
    let mut results: Vec<RaSearchResult> = games
        .into_iter()
        .map(|g| {
            RaSearchResult {
                game_id: g.game_id,
                title: g.title.clone(),
                console_name: g.console_name,
                image_icon: g.image_icon,
                num_achievements: g.num_achievements,
                points_total: g.points_total,
            }
        })
        .filter(|r| r.num_achievements > 0)
        .collect();

    results.sort_by(|a, b| {
        let score_a = similarity_score(&search_title, &a.title);
        let score_b = similarity_score(&search_title, &b.title);
        score_b
            .partial_cmp(&score_a)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Keep only results with a reasonable match score
    results.retain(|r| {
        similarity_score(&search_title, &r.title) >= 0.3
    });

    Ok(results)
}

/// Get the RetroAchievements API key from settings or environment.
#[cfg(feature = "ssr")]
fn get_ra_api_key(state: &crate::api::AppState) -> Option<String> {
    // Check environment variable first
    if let Ok(key) = std::env::var("RA_API_KEY")
        && !key.is_empty() {
        return Some(key);
    }

    // Check settings
    let prefs = state.prefs.read().ok()?;
    prefs.ra_api_key.clone()
}

/// Fetch achievements for a game by system and ROM filename.
/// This searches RetroAchievements by title match and returns the best match's achievements.
#[server(prefix = "/sfn")]
pub async fn get_game_achievements(
    system: String,
    _rom_filename: String,
    game_title: String,
) -> Result<Option<RaGameInfo>, ServerFnError> {
    use replay_control_core::game::ra_types::system_to_ra_console_id;
    use replay_control_core_server::http::get_json_with_timeout;

    let state = expect_context::<crate::api::AppState>();
    let api_key = get_ra_api_key(&state).ok_or_else(|| {
        ServerFnError::new("RetroAchievements API key not configured. Set RA_API_KEY environment variable or configure in settings.")
    })?;

    let console_id = system_to_ra_console_id(&system).ok_or_else(|| {
        ServerFnError::new(format!("System '{system}' is not supported by RetroAchievements"))
    })?;

    // Search for the game
    let results = search_ra_games(&api_key, console_id, &game_title).await?;

    if results.is_empty() {
        return Ok(None);
    }

    // Take the best match
    let best = &results[0];

    // Fetch extended game info with achievements
    let url = format!(
        "{}/GetGameExtended.php?z={}&i={}",
        RA_API_BASE, api_key, best.game_id
    );

    let json = get_json_with_timeout(
        &url,
        std::time::Duration::from_secs(15),
    )
    .await
    .map_err(|e| ServerFnError::new(format!("Failed to fetch RA game info: {e}")))?;

    let game_ext: RaGameExtended = serde_json::from_value(json)
        .map_err(|e| ServerFnError::new(format!("Failed to parse RA game info: {e}")))?;

    let mut achievements: Vec<AchievementInfo> = game_ext
        .achievements
        .into_values()
        .map(|a| AchievementInfo {
            id: a.id,
            title: a.title,
            description: a.description,
            points: a.points,
            badge_url: format!("https://media.retroachievements.org/Badge/{}.png", a.badge_name),
            author: a.author,
            r#type: a.r#type,
        })
        .collect();

    achievements.sort_by_key(|a| a.id);

    let total_points: u32 = achievements.iter().map(|a| a.points).sum();

    Ok(Some(RaGameInfo {
        game_id: best.game_id,
        title: game_ext.title,
        console_name: game_ext.console_name,
        image_icon: game_ext.image_icon,
        image_title: game_ext.image_title,
        image_ingame: game_ext.image_ingame,
        image_box_art: game_ext.image_box_art,
        developer: game_ext.developer,
        publisher: game_ext.publisher,
        genre: game_ext.genre,
        released: game_ext.released,
        achievements,
        total_points,
    }))
}

/// Check if a game has achievements available (without fetching full data).
/// Returns the number of achievements if found.
#[server(prefix = "/sfn")]
pub async fn check_game_achievements(
    system: String,
    _rom_filename: String,
    game_title: String,
) -> Result<Option<u32>, ServerFnError> {
    use replay_control_core::game::ra_types::system_to_ra_console_id;

    let state = expect_context::<crate::api::AppState>();
    let api_key = get_ra_api_key(&state).ok_or_else(|| {
        ServerFnError::new("RetroAchievements API key not configured")
    })?;

    let console_id = system_to_ra_console_id(&system).ok_or_else(|| {
        ServerFnError::new(format!("System '{system}' is not supported by RetroAchievements"))
    })?;

    let results = search_ra_games(&api_key, console_id, &game_title).await?;

    if results.is_empty() {
        return Ok(None);
    }

    Ok(Some(results[0].num_achievements))
}

/// Search for games in RetroAchievements by title.
/// Returns a list of potential matches.
#[server(prefix = "/sfn")]
pub async fn search_ra_games_api(
    system: String,
    title: String,
) -> Result<Vec<RaSearchResult>, ServerFnError> {
    use replay_control_core::game::ra_types::system_to_ra_console_id;

    let state = expect_context::<crate::api::AppState>();
    let api_key = get_ra_api_key(&state).ok_or_else(|| {
        ServerFnError::new("RetroAchievements API key not configured")
    })?;

    let console_id = system_to_ra_console_id(&system).ok_or_else(|| {
        ServerFnError::new(format!("System '{system}' is not supported by RetroAchievements"))
    })?;

    search_ra_games(&api_key, console_id, &title).await
}
