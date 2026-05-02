//! HowLongToBeat integration.
//!
//! Fetches completion-time data from the unofficial HLTB API.
//! The API requires a dynamic key extracted from their JS bundle, so this
//! module handles key discovery, caching it in memory for 24 h.
//!
//! Results are cached in `user_data.db` (hltb_cache table) for 7 days to
//! avoid hitting HLTB on every page load.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use replay_control_core::error::{Error, Result};
use replay_control_core::want_to_play::HltbData;

/// In-memory cache for the HLTB API key (extracted from their JS bundle).
static KEY_CACHE: Mutex<Option<(String, Instant)>> = Mutex::new(None);

const KEY_TTL: Duration = Duration::from_secs(24 * 3600);
const HLTB_BASE: &str = "https://howlongtobeat.com";

/// Fetch completion-time data for a game from HowLongToBeat.
///
/// Tries the search API with the given `display_name`. Returns `None` if HLTB
/// is unreachable or returns no results (not an error — the caller should just
/// skip showing HLTB data rather than surfacing an error to the user).
pub async fn fetch_hltb(display_name: &str) -> Option<HltbData> {
    let key = match get_search_key().await {
        Ok(k) => k,
        Err(e) => {
            tracing::warn!("HLTB key fetch failed: {e}");
            return None;
        }
    };

    let clean_name = strip_rom_tags(display_name);
    let terms: Vec<&str> = clean_name.split_whitespace().collect();

    let body = serde_json::json!({
        "searchTerms": terms,
        "searchPage": 1,
        "size": 5,
        "searchOptions": {
            "games": {
                "userId": 0,
                "platform": "",
                "sortCategory": "popular",
                "rangeCategory": "main",
                "rangeTime": { "min": null, "max": null },
                "gameplay": { "perspective": "", "flow": "", "genre": "" },
                "rangeYear": { "min": "", "max": "" },
                "modifier": ""
            },
            "users": { "sortCategory": "postcount" },
            "lists": { "sortCategory": "follows" },
            "filter": "",
            "sort": 0,
            "randomizer": 0
        }
    });

    let url = format!("{HLTB_BASE}/api/search/{key}");
    let result = crate::http::shared_client()
        .post(&url)
        .header("Referer", HLTB_BASE)
        .header("Content-Type", "application/json")
        .timeout(Duration::from_secs(10))
        .json(&body)
        .send()
        .await;

    let resp = match result {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("HLTB search request failed: {e}");
            return None;
        }
    };

    let json: serde_json::Value = match resp.json().await {
        Ok(j) => j,
        Err(e) => {
            tracing::warn!("HLTB response parse failed: {e}");
            return None;
        }
    };

    parse_hltb_response(&json, display_name)
}

fn parse_hltb_response(json: &serde_json::Value, query: &str) -> Option<HltbData> {
    let data = json.get("data")?.as_array()?;
    if data.is_empty() {
        tracing::debug!("HLTB: no results for \"{query}\"");
        return None;
    }

    let entry = &data[0];
    let game_id = entry.get("game_id")?.as_u64()?;

    let main_secs = entry
        .get("comp_main")
        .and_then(|v| v.as_u64())
        .filter(|&s| s > 0);
    let main_extra_secs = entry
        .get("comp_plus")
        .and_then(|v| v.as_u64())
        .filter(|&s| s > 0);
    let completionist_secs = entry
        .get("comp_100")
        .and_then(|v| v.as_u64())
        .filter(|&s| s > 0);

    tracing::debug!(
        "HLTB: matched \"{query}\" → game_id={game_id} main={main_secs:?}s"
    );

    Some(HltbData {
        game_id,
        main_secs,
        main_extra_secs,
        completionist_secs,
    })
}

/// Obtain the HLTB API search key, using the in-memory cache when fresh.
async fn get_search_key() -> Result<String> {
    {
        let guard = KEY_CACHE.lock().unwrap();
        if let Some((ref key, ref at)) = *guard {
            if at.elapsed() < KEY_TTL {
                return Ok(key.clone());
            }
        }
    }

    let key = discover_search_key().await?;
    *KEY_CACHE.lock().unwrap() = Some((key.clone(), Instant::now()));
    Ok(key)
}

/// Discover the HLTB search key by scanning JS chunks from their site.
///
/// HLTB uses Next.js/Turbopack with hashed chunk names that change each
/// deploy. We fetch the game page (which loads search-related bundles) and
/// scan each referenced chunk until we find the embedded key.
async fn discover_search_key() -> Result<String> {
    // The game page loads search-related chunks that the homepage may not.
    let probe_urls = [
        format!("{HLTB_BASE}/game/2380"),
        HLTB_BASE.to_string(),
    ];

    for page_url in &probe_urls {
        let html = match crate::http::get_text_with_timeout(page_url, Duration::from_secs(10)).await {
            Ok(h) => h,
            Err(e) => {
                tracing::debug!("HLTB: failed to fetch {page_url}: {e}");
                continue;
            }
        };

        // Fast path: key embedded in an inline script on the page itself.
        if let Some(key) = extract_search_key(&html) {
            tracing::debug!("HLTB: found key in inline script of {page_url}");
            return Ok(key);
        }

        let chunks = extract_chunk_paths(&html);
        tracing::debug!("HLTB: scanning {} chunks from {page_url}", chunks.len());

        for path in &chunks {
            let url = format!("{HLTB_BASE}{path}");
            match crate::http::get_text_with_timeout(&url, Duration::from_secs(8)).await {
                Ok(js) => {
                    if let Some(key) = extract_search_key(&js) {
                        tracing::debug!("HLTB: found key in chunk {path}");
                        return Ok(key);
                    }
                }
                Err(e) => tracing::debug!("HLTB: chunk {path} fetch failed: {e}"),
            }
        }
    }

    Err(Error::Other(
        "HLTB: could not find search key in any JS chunk".to_string(),
    ))
}

/// Collect all unique `/_next/static/chunks/*.js` paths from an HTML page.
fn extract_chunk_paths(html: &str) -> Vec<String> {
    let needle = "/_next/static/chunks/";
    let mut seen = std::collections::HashSet::new();
    let mut paths = Vec::new();
    let mut pos = 0;

    while let Some(rel) = html[pos..].find(needle) {
        let start = pos + rel;
        let rest = &html[start..];
        let end = rest
            .find(|c: char| matches!(c, '"' | '\'' | '`' | ' ' | ')' | '\\'))
            .unwrap_or(rest.len());
        let path = &rest[..end];
        if path.ends_with(".js") && seen.insert(path.to_string()) {
            paths.push(path.to_string());
        }
        pos = start + 1;
    }

    paths
}

/// Extract the API key from a JS string (inline HTML or a chunk file).
///
/// Tries multiple patterns covering old Next.js pages router and newer
/// Turbopack/app-router minification styles.
fn extract_search_key(js: &str) -> Option<String> {
    // Pattern 1 (pages router): "/api/search/".concat("KEY")
    if let Some(idx) = js.find("\"/api/search/\".concat(\"") {
        let after = &js[idx + "\"/api/search/\".concat(\"".len()..];
        if let Some(end) = after.find('"') {
            let key = &after[..end];
            if is_valid_key(key) {
                return Some(key.to_string());
            }
        }
    }

    // Pattern 2 (app router): "/api/search/KEY" as a string literal
    if let Some(idx) = js.find("\"/api/search/") {
        let after = &js[idx + "\"/api/search/".len()..];
        let end = after.find('"').unwrap_or(after.len());
        let key = &after[..end];
        if is_valid_key(key) {
            return Some(key.to_string());
        }
    }

    // Pattern 3: `/api/search/KEY` (template literal or backtick)
    if let Some(idx) = js.find("`/api/search/") {
        let after = &js[idx + "`/api/search/".len()..];
        let end = after.find('`').unwrap_or(after.len());
        let key = &after[..end];
        if is_valid_key(key) {
            return Some(key.to_string());
        }
    }

    // Pattern 4 (fallback): bare `api/search/KEY` anywhere
    if let Some(idx) = js.find("api/search/") {
        let after = &js[idx + "api/search/".len()..];
        let end = after
            .find(|c: char| c == '"' || c == '\'' || c == '`' || c == '/' || c.is_whitespace())
            .unwrap_or(after.len());
        let key = &after[..end];
        if is_valid_key(key) {
            return Some(key.to_string());
        }
    }

    None
}

fn is_valid_key(s: &str) -> bool {
    s.len() >= 4 && s.len() <= 64 && s.chars().all(|c| c.is_alphanumeric())
}

/// Remove ROM filename tags like "(USA)", "[!]", "(Rev 1)" from a display name.
fn strip_rom_tags(name: &str) -> String {
    let mut s = name.trim();
    loop {
        let prev = s;
        if let Some(pos) = s.rfind(" (") {
            if s.ends_with(')') {
                s = s[..pos].trim();
                continue;
            }
        }
        if let Some(pos) = s.rfind(" [") {
            if s.ends_with(']') {
                s = s[..pos].trim();
                continue;
            }
        }
        if s == prev {
            break;
        }
    }
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_tags_removes_region_suffixes() {
        assert_eq!(strip_rom_tags("Super Mario World (USA)"), "Super Mario World");
        assert_eq!(
            strip_rom_tags("Zelda (USA) (Rev 1)"),
            "Zelda"
        );
        assert_eq!(strip_rom_tags("Sonic the Hedgehog [!]"), "Sonic the Hedgehog");
        assert_eq!(strip_rom_tags("Bare Title"), "Bare Title");
    }

    #[test]
    fn extract_search_key_concat_pattern() {
        let js = r#"var x="/api/search/".concat("abc123def");"#;
        assert_eq!(extract_search_key(js), Some("abc123def".to_string()));
    }

    #[test]
    fn extract_search_key_string_literal_pattern() {
        let js = r#"fetch("/api/search/xyz789abc","#;
        assert_eq!(extract_search_key(js), Some("xyz789abc".to_string()));
    }

    #[test]
    fn extract_search_key_bare_pattern() {
        let js = r#"fetch("https://howlongtobeat.com/api/search/xyz789","#;
        assert_eq!(extract_search_key(js), Some("xyz789".to_string()));
    }

    #[test]
    fn extract_chunk_paths_finds_js_urls() {
        let html = r#"<script src="/_next/static/chunks/framework-abc.js"></script><script src="/_next/static/chunks/main-def.js"></script>"#;
        let paths = extract_chunk_paths(html);
        assert!(paths.contains(&"/_next/static/chunks/framework-abc.js".to_string()));
        assert!(paths.contains(&"/_next/static/chunks/main-def.js".to_string()));
    }

    #[test]
    fn extract_chunk_paths_deduplicates() {
        let html = r#"src="/_next/static/chunks/abc.js" src="/_next/static/chunks/abc.js""#;
        let paths = extract_chunk_paths(html);
        assert_eq!(paths.len(), 1);
    }
}
