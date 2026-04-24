//! Native side of update handling: GitHub release polling, asset download,
//! and the `/var/tmp/replay-control-update/` state files.
//!
//! Pure types (`AvailableUpdate`, `UpdateChannel`, `UpdateState`, `is_newer`,
//! directory constants) live in `replay_control_core::update`; this module
//! adds the HTTP + filesystem side that needs reqwest and std::fs.

use std::path::Path;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio_stream::StreamExt;

pub use replay_control_core::update::{
    AvailableUpdate, UPDATE_DIR, UPDATE_LOCK, UPDATE_SCRIPT, UpdateChannel, UpdateState, is_newer,
    validate_version,
};

use crate::http::shared_client;

/// Download URLs for a specific release, resolved fresh from GitHub API.
#[derive(Debug)]
pub struct AssetUrls {
    pub binary_url: String,
    pub site_url: String,
}

/// GitHub API base URL. Overridable via `REPLAY_GITHUB_API_URL` for testing.
pub fn github_api_base_url() -> String {
    std::env::var("REPLAY_GITHUB_API_URL").unwrap_or_else(|_| "https://api.github.com".to_string())
}

/// Nuke the update temp directory (idempotent).
pub fn nuke_update_dir() {
    let dir = Path::new(UPDATE_DIR);
    if dir.exists() {
        let _ = std::fs::remove_dir_all(dir);
    }
    let script = Path::new(UPDATE_SCRIPT);
    if script.exists() {
        let _ = std::fs::remove_file(script);
    }
}

/// Read `available.json` from the update temp directory.
pub fn read_available_update() -> Option<AvailableUpdate> {
    let path = Path::new(UPDATE_DIR).join("available.json");
    let data = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&data).ok()
}

/// Write `available.json` to the update temp directory, creating the dir if missing.
pub fn write_available_update(update: &AvailableUpdate) -> std::io::Result<()> {
    let dir = Path::new(UPDATE_DIR);
    std::fs::create_dir_all(dir)?;
    let path = dir.join("available.json");
    let json = serde_json::to_string(update).map_err(std::io::Error::other)?;
    std::fs::write(path, json)
}

/// HTTP GET with optional Authorization for the GitHub API.
pub async fn github_get(
    url: &str,
    api_key: Option<&str>,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let mut req = shared_client()
        .get(url)
        .header("Accept", "application/vnd.github+json");

    if let Some(key) = api_key {
        req = req.header("Authorization", format!("Bearer {key}"));
    }

    let resp = req.send().await?.error_for_status()?;
    Ok(resp.json().await?)
}

/// Parse a GitHub release JSON object into an AvailableUpdate.
pub fn parse_release(json: &serde_json::Value) -> Option<AvailableUpdate> {
    let tag = json.get("tag_name")?.as_str()?;
    let version = tag.strip_prefix('v').unwrap_or(tag);

    if semver::Version::parse(version).is_err() {
        return None;
    }

    let prerelease = json
        .get("prerelease")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let html_url = json
        .get("html_url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let published_at = json
        .get("published_at")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let assets = json.get("assets").and_then(|v| v.as_array());
    let mut binary_size = 0u64;
    let mut site_size = 0u64;
    if let Some(assets) = assets {
        for asset in assets {
            let name = asset.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let size = asset.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
            if name.contains("replay-control-app") {
                binary_size = size;
            } else if name.contains("site") {
                site_size = size;
            }
        }
    }

    Some(AvailableUpdate {
        version: version.to_string(),
        tag: tag.to_string(),
        prerelease,
        release_notes_url: html_url,
        published_at,
        binary_size,
        site_size,
    })
}

/// Fetch the latest stable release via /releases/latest.
/// Returns Ok(None) if no stable release exists (GitHub returns 404).
pub async fn fetch_latest_stable(
    base_url: &str,
    repo: &str,
    api_key: Option<&str>,
) -> Result<Option<AvailableUpdate>, Box<dyn std::error::Error + Send + Sync>> {
    let url = format!("{base_url}/repos/{repo}/releases/latest");
    match github_get(&url, api_key).await {
        Ok(json) => Ok(parse_release(&json)),
        Err(e) => {
            // 404 means no stable release exists — not an error.
            if e.to_string().contains("404") {
                Ok(None)
            } else {
                Err(e)
            }
        }
    }
}

/// Fetch the latest beta by querying /releases and picking newest by semver.
pub async fn fetch_latest_beta(
    current_version: &str,
    base_url: &str,
    repo: &str,
    api_key: Option<&str>,
) -> Result<Option<AvailableUpdate>, Box<dyn std::error::Error + Send + Sync>> {
    let url = format!("{base_url}/repos/{repo}/releases?per_page=10");
    let json = github_get(&url, api_key).await?;

    let empty = vec![];
    let releases = json.as_array().unwrap_or(&empty);

    let mut best: Option<AvailableUpdate> = None;
    for release in releases {
        if let Some(parsed) = parse_release(release)
            && is_newer(current_version, &parsed.version)
            && best
                .as_ref()
                .is_none_or(|b| is_newer(&b.version, &parsed.version))
        {
            best = Some(parsed);
        }
    }
    Ok(best)
}

/// Check GitHub for a newer release than the running version.
pub async fn check_github_update(
    current_version: &str,
    base_url: &str,
    repo: &str,
    channel: &UpdateChannel,
    skipped_version: Option<&str>,
    github_api_key: Option<&str>,
) -> Result<Option<AvailableUpdate>, Box<dyn std::error::Error + Send + Sync>> {
    let release = match channel {
        UpdateChannel::Beta => {
            fetch_latest_beta(current_version, base_url, repo, github_api_key).await?
        }
        UpdateChannel::Stable => fetch_latest_stable(base_url, repo, github_api_key).await?,
    };

    let Some(release) = release else {
        return Ok(None);
    };

    if !is_newer(current_version, &release.version) {
        return Ok(None);
    }

    if let Some(skipped) = skipped_version
        && release.version == skipped
    {
        return Ok(None);
    }

    Ok(Some(release))
}

/// Resolve fresh download URLs for a given release tag.
pub async fn resolve_asset_urls(
    base_url: &str,
    repo: &str,
    tag: &str,
    api_key: Option<&str>,
) -> Result<AssetUrls, Box<dyn std::error::Error + Send + Sync>> {
    let url = format!("{base_url}/repos/{repo}/releases/tags/{tag}");
    let release = github_get(&url, api_key).await?;

    let assets = release
        .get("assets")
        .and_then(|v| v.as_array())
        .ok_or("No assets found in release")?;

    let mut binary_url = None;
    let mut site_url = None;

    for asset in assets {
        let name = asset.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let download_url = asset.get("browser_download_url").and_then(|v| v.as_str());
        if let Some(url) = download_url {
            if name.contains("replay-control-app") && name.ends_with(".tar.gz") {
                binary_url = Some(url.to_string());
            } else if name.contains("site") && name.ends_with(".tar.gz") {
                site_url = Some(url.to_string());
            }
        }
    }

    Ok(AssetUrls {
        binary_url: binary_url.ok_or("Binary asset not found in release")?,
        site_url: site_url.ok_or("Site asset not found in release")?,
    })
}

/// Download a file from a URL to a local path, reporting progress via callback.
/// Progress callback is throttled to at most once per 250ms.
pub async fn download_asset(
    url: &str,
    dest: &Path,
    progress_cb: &(dyn Fn(u64) + Send + Sync),
) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
    let resp = shared_client()
        .get(url)
        .timeout(Duration::from_secs(300))
        .send()
        .await?
        .error_for_status()?;

    let mut file = tokio::fs::File::create(dest).await?;
    let mut stream = resp.bytes_stream();
    let mut downloaded = 0u64;
    let mut last_report = std::time::Instant::now();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;

        if last_report.elapsed() >= Duration::from_millis(250) {
            progress_cb(downloaded);
            last_report = std::time::Instant::now();
        }
    }

    progress_cb(downloaded);
    file.flush().await?;
    Ok(downloaded)
}
