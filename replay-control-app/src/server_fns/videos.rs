use super::*;
#[cfg(feature = "ssr")]
use replay_control_core::resource_kind;
#[cfg(feature = "ssr")]
use replay_control_core::systems;
#[cfg(feature = "ssr")]
use replay_control_core_server::user_data_db::UserDataDb;

pub use replay_control_core::user_data_db::VideoEntry;

/// A video recommendation from Piped search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoRecommendation {
    pub url: String,
    pub title: String,
    pub thumbnail_url: Option<String>,
    pub duration_text: Option<String>,
    pub channel: Option<String>,
}

/// Get saved videos for a game, shared across regional variants via base_title.
#[server(prefix = "/sfn")]
pub async fn get_game_videos(
    system: String,
    base_title: String,
) -> Result<Vec<VideoEntry>, ServerFnError> {
    let state = super::app_state()?;
    let all_titles = super::resolve_shared_titles(&state, &system, &base_title).await;
    videos_for_titles(&state, &system, all_titles).await
}

/// User-saved videos for a pre-resolved title set — one user_data read.
#[cfg(feature = "ssr")]
pub(crate) async fn videos_for_titles(
    state: &crate::api::AppState,
    system: &str,
    all_titles: Vec<String>,
) -> Result<Vec<VideoEntry>, ServerFnError> {
    state
        .user_data_reader
        .read({
            let system = system.to_string();
            move |conn| {
                let title_refs: Vec<&str> = all_titles.iter().map(|s| s.as_str()).collect();
                UserDataDb::get_game_videos(conn, &system, &title_refs)
            }
        })
        .await
        .ok_or_else(|| ServerFnError::new("Cannot open user data DB"))?
        .map_err(|e| ServerFnError::new(e.to_string()))
}

/// Get provider-supplied video suggestions copied into the library DB.
#[server(prefix = "/sfn")]
pub async fn get_provider_game_videos(
    system: String,
    rom_filename: String,
) -> Result<Vec<VideoRecommendation>, ServerFnError> {
    let state = super::app_state()?;
    let rows =
        super::game_resource_rows(&state, &system, &rom_filename, resource_kind::VIDEO).await?;
    Ok(provider_videos_from_links(rows))
}

/// Provider video suggestions from pre-fetched `library_game_resource` VIDEO
/// rows. Pure assembly — the detail-page bundle hands it the partition of
/// rows it already loaded.
#[cfg(feature = "ssr")]
pub(crate) fn provider_videos_from_links(
    rows: Vec<super::LibraryResourceLink>,
) -> Vec<VideoRecommendation> {
    let mut out = Vec::new();
    for row in rows {
        let Ok(parsed) = replay_control_core::video_url::parse_video_url(&row.url) else {
            continue;
        };
        if parsed.platform.as_str() != "youtube" {
            continue;
        }
        out.push(VideoRecommendation {
            url: parsed.canonical_url,
            title: row.title.unwrap_or_else(|| "Provider video".to_string()),
            thumbnail_url: Some(format!(
                "https://i.ytimg.com/vi/{}/mqdefault.jpg",
                parsed.video_id
            )),
            duration_text: None,
            channel: Some(row.source),
        });
    }
    out
}

/// Add a video to a game (from manual paste or recommendation pin).
#[server(prefix = "/sfn")]
pub async fn add_game_video(
    system: String,
    rom_filename: String,
    base_title: String,
    url: String,
    title: Option<String>,
    from_recommendation: bool,
    tag: Option<String>,
) -> Result<VideoEntry, ServerFnError> {
    let state = super::app_state()?;
    super::require_storage_mutation_allowed(&state, "add videos").await?;

    let parsed =
        replay_control_core::video_url::parse_video_url(&url).map_err(ServerFnError::new)?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let entry = VideoEntry {
        id: format!("{}-{}", parsed.platform, parsed.video_id),
        url: parsed.canonical_url,
        platform: parsed.platform.as_str().to_string(),
        video_id: parsed.video_id,
        title,
        added_at: now,
        from_recommendation,
        tag,
        rom_filename: rom_filename.clone(),
    };

    state
        .user_data_writer
        .try_write({
            let entry = entry.clone();
            move |conn| {
                UserDataDb::add_game_video(conn, &system, &rom_filename, &base_title, &entry)
            }
        })
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    Ok(entry)
}

/// Remove a saved video from a game.
#[server(prefix = "/sfn")]
pub async fn remove_game_video(
    system: String,
    rom_filename: String,
    video_id: String,
) -> Result<(), ServerFnError> {
    let state = super::app_state()?;
    super::require_storage_mutation_allowed(&state, "remove videos").await?;
    state
        .user_data_writer
        .try_write(move |conn| {
            UserDataDb::remove_game_video(conn, &system, &rom_filename, &video_id)
        })
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?
        .map_err(|e| ServerFnError::new(e.to_string()))
}

/// Search for video recommendations via the Piped API.
#[server(prefix = "/sfn")]
pub async fn search_game_videos(
    system: String,
    display_name: String,
    query_type: String,
) -> Result<Vec<VideoRecommendation>, ServerFnError> {
    // Normalize the title: strip parenthesized tags like "(USA)", "(World 910522)"
    let clean_title = {
        let mut s = display_name.as_str();
        // Repeatedly strip trailing parenthesized/bracketed tags
        loop {
            let trimmed = s.trim();
            if let Some(pos) = trimmed.rfind(" (")
                && trimmed.ends_with(')')
            {
                s = &trimmed[..pos];
                continue;
            }
            if let Some(pos) = trimmed.rfind(" [")
                && trimmed.ends_with(']')
            {
                s = &trimmed[..pos];
                continue;
            }
            break;
        }
        s.trim().to_string()
    };

    // Keep arcade video searches broad; non-arcade systems use their platform name.
    let system_label = if systems::is_arcade_system(&system) {
        "arcade".to_string()
    } else {
        systems::system_display_name(&system)
    };

    let query_suffix = match query_type.as_str() {
        "trailer" => "official trailer",
        "gameplay" => "gameplay",
        "1cc" => "1cc one credit clear",
        _ => "",
    };

    let query = format!("{clean_title} {system_label} {query_suffix}");
    let encoded_query = urlencoding::encode(&query);
    tracing::info!("Video search: query=\"{query}\"");

    // Try Piped instances first, then Invidious instances
    let piped_instances = [
        "https://pipedapi.kavin.rocks",
        "https://pipedapi.leptons.xyz",
        "https://pipedapi-libre.kavin.rocks",
    ];
    let invidious_instances = [
        "https://invidious.materialio.us",
        "https://yewtu.be",
        "https://inv.tux.pizza",
    ];

    // Try Piped instances
    for base_url in &piped_instances {
        let api_url = format!("{base_url}/search?q={encoded_query}&filter=videos");
        match http_get_json(&api_url, 8).await {
            Ok(body) => {
                let items = body
                    .get("items")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                if !items.is_empty() {
                    tracing::info!(
                        "Video search: Piped {base_url} returned {} results",
                        items.len()
                    );
                    return Ok(parse_piped_results(&items));
                }
                tracing::warn!("Video search: Piped {base_url} returned empty results");
            }
            Err(e) => {
                tracing::warn!("Video search: Piped {base_url} failed: {e}");
            }
        }
    }

    // Try Invidious instances
    for base_url in &invidious_instances {
        let api_url = format!("{base_url}/api/v1/search?q={encoded_query}&type=video");
        match http_get_json(&api_url, 8).await {
            Ok(body) => {
                let items = match body.as_array() {
                    Some(arr) => arr.clone(),
                    None => Vec::new(),
                };
                if !items.is_empty() {
                    tracing::info!(
                        "Video search: Invidious {base_url} returned {} results",
                        items.len()
                    );
                    return Ok(parse_invidious_results(&items));
                }
                tracing::warn!("Video search: Invidious {base_url} returned empty results");
            }
            Err(e) => {
                tracing::warn!("Video search: Invidious {base_url} failed: {e}");
            }
        }
    }

    tracing::error!("Video search: all instances failed for query \"{query}\"");
    Err(ServerFnError::new(
        "Video search unavailable. Paste URLs directly.".to_string(),
    ))
}

/// Fetch a URL and parse the response as JSON.
#[cfg(feature = "ssr")]
async fn http_get_json(url: &str, timeout_secs: u64) -> Result<serde_json::Value, String> {
    replay_control_core_server::http::get_json_with_timeout(
        url,
        std::time::Duration::from_secs(timeout_secs),
    )
    .await
    .map_err(|e| e.to_string())
}

#[cfg(feature = "ssr")]
pub(crate) fn parse_piped_results(items: &[serde_json::Value]) -> Vec<VideoRecommendation> {
    items
        .iter()
        .filter_map(|item| {
            let url_path = item.get("url")?.as_str()?;
            let full_url = if url_path.starts_with("http") {
                url_path.to_string()
            } else {
                format!("https://www.youtube.com{url_path}")
            };
            let title = item
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Untitled")
                .to_string();
            let thumbnail_url = item
                .get("thumbnail")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let duration_secs = item.get("duration").and_then(|v| v.as_i64());
            let duration_text = duration_secs.map(|secs| {
                let mins = secs / 60;
                let s = secs % 60;
                format!("{mins}:{s:02}")
            });
            let channel = item
                .get("uploaderName")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            Some(VideoRecommendation {
                url: full_url,
                title,
                thumbnail_url,
                duration_text,
                channel,
            })
        })
        .take(10)
        .collect()
}

#[cfg(feature = "ssr")]
pub(crate) fn parse_invidious_results(items: &[serde_json::Value]) -> Vec<VideoRecommendation> {
    items
        .iter()
        .filter_map(|item| {
            let video_id = item.get("videoId")?.as_str()?;
            let full_url = format!("https://www.youtube.com/watch?v={video_id}");
            let title = item
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Untitled")
                .to_string();
            // Use medium-quality thumbnail from YouTube directly
            let thumbnail_url = Some(format!("https://i.ytimg.com/vi/{video_id}/mqdefault.jpg"));
            let duration_secs = item.get("lengthSeconds").and_then(|v| v.as_i64());
            let duration_text = duration_secs.map(|secs| {
                let mins = secs / 60;
                let s = secs % 60;
                format!("{mins}:{s:02}")
            });
            let channel = item
                .get("author")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            Some(VideoRecommendation {
                url: full_url,
                title,
                thumbnail_url,
                duration_text,
                channel,
            })
        })
        .take(10)
        .collect()
}
