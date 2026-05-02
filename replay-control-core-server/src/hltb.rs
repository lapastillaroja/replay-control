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

/// Discover the HLTB search key by fetching their homepage and JS bundle.
async fn discover_search_key() -> Result<String> {
    let html = crate::http::get_text_with_timeout(HLTB_BASE, Duration::from_secs(10)).await?;

    let script_path = extract_app_script_path(&html).ok_or_else(|| {
        Error::Other("HLTB: could not find app script URL in homepage HTML".to_string())
    })?;

    let js_url = format!("{HLTB_BASE}{script_path}");
    let js = crate::http::get_text_with_timeout(&js_url, Duration::from_secs(10)).await?;

    extract_search_key(&js).ok_or_else(|| {
        Error::Other("HLTB: could not extract search key from JS bundle".to_string())
    })
}

/// Find the `/_next/static/chunks/pages/_app-HASH.js` path from the HTML.
fn extract_app_script_path(html: &str) -> Option<String> {
    // Look for the _app chunk script tag
    let needle = "/_next/static/chunks/pages/_app-";
    let start = html.find(needle)?;
    let rest = &html[start..];
    let end = rest.find('"').or_else(|| rest.find('\''))?;
    Some(rest[..end].to_string())
}

/// Extract the API key from the JS bundle.
///
/// Looks for patterns like `"/api/search/".concat("XXXXXXXX")` or
/// `api/search/XXXXXXXX` embedded in the minified Next.js output.
fn extract_search_key(js: &str) -> Option<String> {
    // Primary pattern: "/api/search/".concat("KEY")
    if let Some(idx) = js.find("\"/api/search/\".concat(\"") {
        let after = &js[idx + "\"/api/search/\".concat(\"".len()..];
        if let Some(end) = after.find('"') {
            let key = &after[..end];
            if !key.is_empty() && key.len() <= 32 {
                return Some(key.to_string());
            }
        }
    }

    // Fallback pattern: `api/search/` followed directly by the key
    if let Some(idx) = js.find("api/search/") {
        let after = &js[idx + "api/search/".len()..];
        // Key ends at a quote, slash, or whitespace
        let end = after
            .find(|c: char| c == '"' || c == '\'' || c == '/' || c.is_whitespace())
            .unwrap_or(after.len());
        let key = &after[..end];
        if key.len() >= 4 && key.len() <= 32 && key.chars().all(|c| c.is_alphanumeric()) {
            return Some(key.to_string());
        }
    }

    None
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
    fn extract_search_key_primary_pattern() {
        let js = r#"var x="/api/search/".concat("abc123def");"#;
        assert_eq!(extract_search_key(js), Some("abc123def".to_string()));
    }

    #[test]
    fn extract_search_key_fallback_pattern() {
        let js = r#"fetch("https://howlongtobeat.com/api/search/xyz789","#;
        assert_eq!(extract_search_key(js), Some("xyz789".to_string()));
    }
}
