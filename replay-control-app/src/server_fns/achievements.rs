use super::*;

#[cfg(feature = "ssr")]
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
    pub unlocked: bool,
    pub unlocked_date: Option<String>,
    pub unlocked_hardcore: bool,
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
    pub earned_points: u32,
    pub earned_count: u32,
    pub completion_percentage: f64,
    pub is_complete: bool,
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
const RA_API_BASE: &str = "https://retroachievements.org/API";

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
        .filter(|t| t.len() >= 2)
        .collect();
    let b_tokens: std::collections::HashSet<&str> = b_lower
        .split_whitespace()
        .filter(|t| t.len() >= 2)
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
    username: &str,
    console_id: u32,
    title: &str,
) -> Result<Vec<RaSearchResult>, ServerFnError> {
    use replay_control_core_server::http::get_json_with_timeout;

    let url = format!(
        "{}/API_GetGameList.php?z={}&y={}&i={}&f=1",
        RA_API_BASE, username, api_key, console_id
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

    results.retain(|r| {
        similarity_score(&search_title, &r.title) >= 0.3
    });

    Ok(results)
}

/// Get RetroAchievements credentials (api_key, username) from settings or environment.
/// Both are required to make any RA API call (z=username&y=api_key).
#[cfg(feature = "ssr")]
fn get_ra_credentials(state: &crate::api::AppState) -> Option<(String, String)> {
    let key = std::env::var("RA_API_KEY")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| state.prefs.read().ok()?.ra_api_key.clone())?;

    let username = std::env::var("RA_USERNAME")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| state.prefs.read().ok()?.ra_username.clone())?;

    Some((key, username))
}


/// Fetch achievements for a game by system and ROM filename.
/// If user credentials are configured, also fetches personal progress.
#[server(prefix = "/sfn")]
pub async fn get_game_achievements(
    system: String,
    _rom_filename: String,
    game_title: String,
) -> Result<Option<RaGameInfo>, ServerFnError> {
    use replay_control_core::game::ra_types::system_to_ra_console_id;
    use replay_control_core_server::http::get_json_with_timeout;

    let state = expect_context::<crate::api::AppState>();
    let (api_key, username) = get_ra_credentials(&state).ok_or_else(|| {
        ServerFnError::new("RetroAchievements credentials not configured. Set API key and username in settings.")
    })?;

    let console_id = system_to_ra_console_id(&system).ok_or_else(|| {
        ServerFnError::new(format!("System '{system}' is not supported by RetroAchievements"))
    })?;

    let results = search_ra_games(&api_key, &username, console_id, &game_title).await?;

    if results.is_empty() {
        return Ok(None);
    }

    let best = &results[0];

    let url = format!(
        "{}/API_GetGameInfoAndUserProgress.php?z={}&y={}&g={}&u={}",
        RA_API_BASE, username, api_key, best.game_id, username
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
        .map(|a| {
            let unlocked = !a.date_earned.is_empty();
            let unlocked_date = if unlocked { Some(a.date_earned) } else { None };
            let unlocked_hardcore = !a.date_earned_hardcore.is_empty();

            AchievementInfo {
                id: a.id,
                title: a.title,
                description: a.description,
                points: a.points,
                badge_url: format!("https://media.retroachievements.org/Badge/{}.png", a.badge_name),
                author: a.author,
                r#type: a.r#type,
                unlocked,
                unlocked_date,
                unlocked_hardcore,
            }
        })
        .collect();

    achievements.sort_by_key(|a| a.id);

    let total_points: u32 = achievements.iter().map(|a| a.points).sum();
    let earned_points: u32 = achievements.iter().filter(|a| a.unlocked).map(|a| a.points).sum();
    let earned_count: u32 = achievements.iter().filter(|a| a.unlocked).count() as u32;
    let completion_percentage = if !achievements.is_empty() {
        (earned_count as f64 / achievements.len() as f64) * 100.0
    } else {
        0.0
    };
    let is_complete = earned_count > 0 && earned_count == achievements.len() as u32;

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
        earned_points,
        earned_count,
        completion_percentage,
        is_complete,
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
    let (api_key, username) = get_ra_credentials(&state).ok_or_else(|| {
        ServerFnError::new("RetroAchievements credentials not configured")
    })?;

    let console_id = system_to_ra_console_id(&system).ok_or_else(|| {
        ServerFnError::new(format!("System '{system}' is not supported by RetroAchievements"))
    })?;

    let results = search_ra_games(&api_key, &username, console_id, &game_title).await?;

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
    let (api_key, username) = get_ra_credentials(&state).ok_or_else(|| {
        ServerFnError::new("RetroAchievements credentials not configured")
    })?;

    let console_id = system_to_ra_console_id(&system).ok_or_else(|| {
        ServerFnError::new(format!("System '{system}' is not supported by RetroAchievements"))
    })?;

    search_ra_games(&api_key, &username, console_id, &title).await
}

/// Save RetroAchievements user credentials.
#[server(prefix = "/sfn")]
pub async fn save_ra_credentials(
    username: String,
    web_token: String,
) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let store = state.settings.clone();

    replay_control_core_server::settings::write_ra_username(&store, &username)
        .map_err(|e| ServerFnError::new(format!("Failed to save username: {e}")))?;
    replay_control_core_server::settings::write_ra_web_token(&store, &web_token)
        .map_err(|e| ServerFnError::new(format!("Failed to save token: {e}")))?;

    let mut prefs = state.prefs.write().map_err(|_| ServerFnError::new("Prefs lock poisoned"))?;
    prefs.ra_username = if username.is_empty() { None } else { Some(username) };
    prefs.ra_web_token = if web_token.is_empty() { None } else { Some(web_token) };

    Ok(())
}
