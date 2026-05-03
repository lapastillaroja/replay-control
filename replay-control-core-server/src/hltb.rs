//! HowLongToBeat integration.
//!
//! Fetches completion-time data from the unofficial HLTB API.
//!
//! HLTB uses a dynamic search endpoint that changes with each deploy.
//! The endpoint is discovered by scanning their Next.js/Turbopack JS chunks
//! for a `fetch('/api/...', {method: 'POST'})` call, then an auth token is
//! obtained from `{endpoint}/init` before each search.
//!
//! Results are cached in `user_data.db` (hltb_cache table) for 7 days to
//! avoid hitting HLTB on every page load.

use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use replay_control_core::error::{Error, Result};
use replay_control_core::want_to_play::HltbData;

/// In-memory cache for the discovered search endpoint.
static ENDPOINT_CACHE: Mutex<Option<(String, Instant)>> = Mutex::new(None);

const ENDPOINT_TTL: Duration = Duration::from_secs(24 * 3600);
const HLTB_BASE: &str = "https://howlongtobeat.com";
const HLTB_REFERER: &str = "https://howlongtobeat.com/";

/// Fetch completion-time data for a game from HowLongToBeat.
///
/// Returns `None` if HLTB is unreachable or returns no results.
pub async fn fetch_hltb(display_name: &str) -> Option<HltbData> {
    let endpoint = match get_search_endpoint().await {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("HLTB endpoint discovery failed: {e}");
            return None;
        }
    };

    let auth = match get_auth_token(&endpoint).await {
        Ok(a) => a,
        Err(e) => {
            tracing::warn!("HLTB auth token fetch failed: {e}");
            return None;
        }
    };

    let clean_name = strip_rom_tags(display_name);
    let terms: Vec<&str> = clean_name.split_whitespace().collect();

    // The auth key-value pair must also be embedded in the request body.
    let mut body = serde_json::json!({
        "searchType": "games",
        "searchTerms": terms,
        "searchPage": 1,
        "size": 5,
        "searchOptions": {
            "games": {
                "userId": 0,
                "platform": "",
                "sortCategory": "popular",
                "rangeCategory": "main",
                "rangeTime": { "min": 0, "max": 0 },
                "gameplay": { "perspective": "", "flow": "", "genre": "", "difficulty": "" },
                "rangeYear": { "min": "", "max": "" },
                "modifier": ""
            },
            "users": { "sortCategory": "postcount" },
            "lists": { "sortCategory": "follows" },
            "filter": "",
            "sort": 0,
            "randomizer": 0
        },
        "useCache": true
    });
    if !auth.hp_key.is_empty() {
        body[&auth.hp_key] = serde_json::Value::String(auth.hp_val.clone());
    }

    let search_url = format!("{HLTB_BASE}{endpoint}");
    let result = crate::http::shared_client()
        .post(&search_url)
        .header("Referer", HLTB_REFERER)
        .header("Origin", HLTB_REFERER)
        .header("Content-Type", "application/json")
        .header("accept", "*/*")
        .header("x-auth-token", &auth.token)
        .header("x-hp-key", &auth.hp_key)
        .header("x-hp-val", &auth.hp_val)
        .timeout(Duration::from_secs(10))
        .json(&body)
        .send()
        .await;

    let resp = match result {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => {
            tracing::warn!("HLTB search returned status {}", r.status());
            return None;
        }
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

struct AuthToken {
    token: String,
    hp_key: String,
    hp_val: String,
}

/// Call `{endpoint}/init?t={ms}` to get a short-lived auth token.
async fn get_auth_token(endpoint: &str) -> Result<AuthToken> {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let init_url = format!("{HLTB_BASE}{endpoint}/init?t={ts}");

    let resp = crate::http::shared_client()
        .get(&init_url)
        .header("Referer", HLTB_REFERER)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| Error::Other(format!("HLTB init request failed: {e}")))?
        .error_for_status()
        .map_err(|e| Error::Other(format!("HLTB init HTTP error: {e}")))?;

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| Error::Other(format!("HLTB init parse failed: {e}")))?;

    let token = json
        .get("token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::Other("HLTB init: missing 'token' field".to_string()))?
        .to_string();

    // Key and value fields may be named hpKey/hpVal or contain "key"/"val".
    let hp_key = find_string_field(&json, &["hpKey", "key"]).unwrap_or_default();
    let hp_val = find_string_field(&json, &["hpVal", "val"]).unwrap_or_default();

    tracing::debug!("HLTB auth token obtained (key={hp_key})");
    Ok(AuthToken { token, hp_key, hp_val })
}

fn find_string_field(json: &serde_json::Value, candidates: &[&str]) -> Option<String> {
    for &name in candidates {
        if let Some(v) = json.get(name).and_then(|v| v.as_str()) {
            return Some(v.to_string());
        }
    }
    // Fallback: scan all fields for a name containing any candidate substring
    if let Some(obj) = json.as_object() {
        for (k, v) in obj {
            let kl = k.to_lowercase();
            for &cand in candidates {
                if kl.contains(cand) {
                    if let Some(s) = v.as_str() {
                        return Some(s.to_string());
                    }
                }
            }
        }
    }
    None
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

/// Return the cached search endpoint, or discover it fresh.
async fn get_search_endpoint() -> Result<String> {
    {
        let guard = ENDPOINT_CACHE.lock().unwrap();
        if let Some((ref ep, ref at)) = *guard {
            if at.elapsed() < ENDPOINT_TTL {
                return Ok(ep.clone());
            }
        }
    }

    let ep = discover_search_endpoint().await?;
    *ENDPOINT_CACHE.lock().unwrap() = Some((ep.clone(), Instant::now()));
    Ok(ep)
}

/// Discover the HLTB search endpoint by scanning all JS chunks.
///
/// Looks for `fetch('/api/PATH', ... 'POST' ...)` in each chunk to find
/// the current API path (e.g. `/api/bleed`, `/api/search`, etc.).
async fn discover_search_endpoint() -> Result<String> {
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

        // Fast path: endpoint referenced inline on the page itself.
        if let Some(ep) = extract_search_endpoint(&html) {
            tracing::debug!("HLTB: found endpoint in inline script of {page_url}");
            return Ok(ep);
        }

        let chunks = extract_chunk_paths(&html);
        tracing::debug!("HLTB: scanning {} chunks from {page_url}", chunks.len());

        for path in &chunks {
            let url = format!("{HLTB_BASE}{path}");
            match crate::http::get_text_with_timeout(&url, Duration::from_secs(8)).await {
                Ok(js) => {
                    if let Some(ep) = extract_search_endpoint(&js) {
                        tracing::debug!("HLTB: found endpoint in chunk {path}");
                        return Ok(ep);
                    }
                }
                Err(e) => tracing::debug!("HLTB: chunk {path} fetch failed: {e}"),
            }
        }
    }

    Err(Error::Other(
        "HLTB: could not find search endpoint in any JS chunk".to_string(),
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

/// Extract the HLTB search endpoint path from a JS string.
///
/// Matches `fetch("/api/ENDPOINT",{method:"POST"` or the single-quote
/// equivalent, which is the pattern Next.js/Turbopack uses for the search call.
/// The match must be tight (path and method within 50 chars) to avoid matching
/// GET fetch calls that happen to appear before a POST call.
fn extract_search_endpoint(js: &str) -> Option<String> {
    for quote in ['"', '\''] {
        let needle = format!("{quote}/api/");
        let mut pos = 0;
        while let Some(rel) = js[pos..].find(&needle) {
            let abs = pos + rel;
            let after_api = &js[abs + needle.len()..];

            // Extract the endpoint word (alphanumeric/dash/underscore only)
            let end = after_api
                .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-'))
                .unwrap_or(after_api.len());
            let path_part = &after_api[..end];
            if path_part.is_empty() {
                pos = abs + 1;
                continue;
            }

            // The closing quote must follow immediately (no sub-paths)
            let after_path = &after_api[end..];
            if !after_path.starts_with(quote) {
                pos = abs + 1;
                continue;
            }

            // "POST" must appear within 25 chars after the closing quote
            // (the real pattern is `",{method:"POST"` = 15 chars)
            let lookahead = &after_path[..std::cmp::min(25, after_path.len())];
            if lookahead.contains("\"POST\"") || lookahead.contains("'POST'") {
                return Some(format!("/api/{path_part}"));
            }

            pos = abs + 1;
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
        assert_eq!(strip_rom_tags("Zelda (USA) (Rev 1)"), "Zelda");
        assert_eq!(strip_rom_tags("Sonic the Hedgehog [!]"), "Sonic the Hedgehog");
        assert_eq!(strip_rom_tags("Bare Title"), "Bare Title");
    }

    #[test]
    fn extract_endpoint_from_post_fetch() {
        let js = r#"fetch("/api/bleed",{method:"POST",headers:e,body:JSON.stringify(t)})"#;
        assert_eq!(extract_search_endpoint(js), Some("/api/bleed".to_string()));
    }

    #[test]
    fn extract_endpoint_old_search_pattern() {
        let js = r#"fetch("/api/search",{method:"POST",body:JSON.stringify(n)})"#;
        assert_eq!(extract_search_endpoint(js), Some("/api/search".to_string()));
    }

    #[test]
    fn extract_endpoint_ignores_get_fetch() {
        // GET fetch calls should be ignored
        let js = r#"fetch("/api/user",{method:"GET"})fetch("/api/bleed",{method:"POST"})"#;
        assert_eq!(extract_search_endpoint(js), Some("/api/bleed".to_string()));
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
