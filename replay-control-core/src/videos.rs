use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::storage::{RC_DIR, VIDEOS_FILE};

/// A single saved video entry for a game.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoEntry {
    /// Unique ID: "{platform}-{video_id}"
    pub id: String,
    /// Sanitized canonical URL
    pub url: String,
    /// Platform name (e.g., "youtube")
    pub platform: String,
    /// Platform-specific video ID
    pub video_id: String,
    /// Human-readable title (from user or search results)
    pub title: Option<String>,
    /// Unix timestamp when the video was added
    pub added_at: u64,
    /// Whether this was pinned from a recommendation search
    pub from_recommendation: bool,
    /// Tag: "trailer", "gameplay", or None for manual
    pub tag: Option<String>,
}

/// All saved videos, keyed by "{system}/{rom_filename}".
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GameVideos {
    pub games: HashMap<String, Vec<VideoEntry>>,
}

/// Load the videos file from storage. Returns empty if file doesn't exist.
pub fn load_videos(storage_root: &Path) -> GameVideos {
    let path = storage_root.join(RC_DIR).join(VIDEOS_FILE);
    match std::fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
        Err(_) => GameVideos::default(),
    }
}

/// Save the videos file to storage using atomic write (tmp + rename).
pub fn save_videos(storage_root: &Path, videos: &GameVideos) -> Result<(), String> {
    let dir = storage_root.join(RC_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create directory: {e}"))?;

    let path = dir.join(VIDEOS_FILE);
    let tmp_path = dir.join(format!("{VIDEOS_FILE}.tmp"));

    let json =
        serde_json::to_string_pretty(videos).map_err(|e| format!("Failed to serialize: {e}"))?;

    std::fs::write(&tmp_path, json).map_err(|e| format!("Failed to write tmp file: {e}"))?;

    std::fs::rename(&tmp_path, &path).map_err(|e| format!("Failed to rename tmp file: {e}"))?;

    Ok(())
}

/// Add a video to a game's list. Returns an error if a duplicate exists
/// (same platform + video_id).
pub fn add_video(storage_root: &Path, game_key: &str, entry: VideoEntry) -> Result<(), String> {
    let mut videos = load_videos(storage_root);
    let list = videos.games.entry(game_key.to_string()).or_default();

    // Check for duplicate by (platform, video_id)
    if list
        .iter()
        .any(|v| v.platform == entry.platform && v.video_id == entry.video_id)
    {
        return Err("This video is already saved.".to_string());
    }

    list.insert(0, entry); // newest first
    save_videos(storage_root, &videos)
}

/// Remove a video by its ID from a game's list.
pub fn remove_video(storage_root: &Path, game_key: &str, video_id: &str) -> Result<(), String> {
    let mut videos = load_videos(storage_root);
    if let Some(list) = videos.games.get_mut(game_key) {
        list.retain(|v| v.id != video_id);
        // Clean up empty entries
        if list.is_empty() {
            videos.games.remove(game_key);
        }
    }
    save_videos(storage_root, &videos)
}

/// Get all saved videos for a game. Returns empty vec if none.
pub fn get_videos(storage_root: &Path, game_key: &str) -> Vec<VideoEntry> {
    let videos = load_videos(storage_root);
    videos.games.get(game_key).cloned().unwrap_or_default()
}
