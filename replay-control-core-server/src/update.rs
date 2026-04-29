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

/// Classification of a release asset by filename.
/// Order of checks matters: `replay-control-app` and `replay-catalog` are
/// disjoint, but `site` would partially match against future asset names —
/// keeping it last avoids false positives if a new asset is ever introduced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AssetKind {
    Binary,
    Catalog,
    Site,
}

fn classify_asset(name: &str) -> Option<AssetKind> {
    if name.contains("replay-control-app") {
        Some(AssetKind::Binary)
    } else if name.contains("catalog") {
        Some(AssetKind::Catalog)
    } else if name.contains("site") {
        Some(AssetKind::Site)
    } else {
        None
    }
}

/// Download URLs for a specific release, resolved fresh from GitHub API.
#[derive(Debug)]
pub struct AssetUrls {
    pub binary_url: String,
    pub site_url: String,
    /// `None` for releases that predate the catalog asset (< v0.4.0-beta.3).
    /// The updater skips the catalog swap entirely in that case.
    pub catalog_url: Option<String>,
}

/// GitHub API base URL. Overridable via `REPLAY_GITHUB_API_URL` for testing.
pub fn github_api_base_url() -> String {
    std::env::var("REPLAY_GITHUB_API_URL").unwrap_or_else(|_| "https://api.github.com".to_string())
}

/// Nuke the update temp directory (idempotent).
pub fn nuke_update_dir() {
    nuke_paths(Path::new(UPDATE_DIR), Path::new(UPDATE_SCRIPT));
}

fn nuke_paths(dir: &Path, script: &Path) {
    if dir.exists() {
        let _ = std::fs::remove_dir_all(dir);
    }
    if script.exists() {
        let _ = std::fs::remove_file(script);
    }
}

/// Read `available.json` from the update temp directory.
pub fn read_available_update() -> Option<AvailableUpdate> {
    read_available_in(Path::new(UPDATE_DIR))
}

fn read_available_in(dir: &Path) -> Option<AvailableUpdate> {
    let path = dir.join("available.json");
    let data = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&data).ok()
}

/// Write `available.json` to the update temp directory, creating the dir if missing.
pub fn write_available_update(update: &AvailableUpdate) -> std::io::Result<()> {
    write_available_in(Path::new(UPDATE_DIR), update)
}

fn write_available_in(dir: &Path, update: &AvailableUpdate) -> std::io::Result<()> {
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
    let mut catalog_size = 0u64;
    if let Some(assets) = assets {
        for asset in assets {
            let name = asset.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let size = asset.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
            match classify_asset(name) {
                Some(AssetKind::Binary) => binary_size = size,
                Some(AssetKind::Site) => site_size = size,
                Some(AssetKind::Catalog) => catalog_size = size,
                None => {}
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
        catalog_size,
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
    let mut catalog_url = None;

    for asset in assets {
        let name = asset.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let download_url = asset.get("browser_download_url").and_then(|v| v.as_str());
        if !name.ends_with(".tar.gz") {
            continue;
        }
        let Some(url) = download_url else { continue };
        match classify_asset(name) {
            Some(AssetKind::Binary) => binary_url = Some(url.to_string()),
            Some(AssetKind::Site) => site_url = Some(url.to_string()),
            Some(AssetKind::Catalog) => catalog_url = Some(url.to_string()),
            None => {}
        }
    }

    Ok(AssetUrls {
        binary_url: binary_url.ok_or("Binary asset not found in release")?,
        site_url: site_url.ok_or("Site asset not found in release")?,
        catalog_url,
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

#[cfg(test)]
mod tests {
    use super::*;

    const REPO: &str = "lapastillaroja/replay-control";

    // ── available.json roundtrip ────────────────────────────────────

    fn sample_update(version: &str) -> AvailableUpdate {
        AvailableUpdate {
            version: version.to_string(),
            tag: format!("v{version}"),
            prerelease: false,
            release_notes_url: format!("https://example.com/{version}"),
            published_at: "2026-04-01T00:00:00Z".to_string(),
            binary_size: 10_000_000,
            site_size: 4_000_000,
            catalog_size: 8_000_000,
        }
    }

    #[test]
    fn write_then_read_available_roundtrips() {
        let tmp = tempfile::tempdir().unwrap();
        let want = sample_update("0.5.0");
        write_available_in(tmp.path(), &want).unwrap();
        let got = read_available_in(tmp.path()).unwrap();
        assert_eq!(got.version, want.version);
        assert_eq!(got.tag, want.tag);
        assert_eq!(got.binary_size, want.binary_size);
        assert_eq!(got.site_size, want.site_size);
    }

    #[test]
    fn write_creates_missing_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("does/not/exist/yet");
        assert!(!nested.exists());
        write_available_in(&nested, &sample_update("0.1.0")).unwrap();
        assert!(nested.join("available.json").exists());
    }

    #[test]
    fn read_missing_file_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(read_available_in(tmp.path()).is_none());
    }

    #[test]
    fn nuke_paths_removes_dir_and_script() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("update_dir");
        let script = tmp.path().join("do-update.sh");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("available.json"), "{}").unwrap();
        std::fs::write(&script, "#!/bin/sh\n").unwrap();
        assert!(dir.exists() && script.exists());

        nuke_paths(&dir, &script);

        assert!(!dir.exists());
        assert!(!script.exists());
    }

    #[test]
    fn nuke_paths_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("never_existed");
        let script = tmp.path().join("ghost.sh");
        // No panic, no error — just a no-op.
        nuke_paths(&dir, &script);
        nuke_paths(&dir, &script);
    }

    fn make_release_json(tag: &str, prerelease: bool) -> serde_json::Value {
        serde_json::json!({
            "tag_name": tag,
            "prerelease": prerelease,
            "html_url": format!("https://github.com/test/releases/tag/{tag}"),
            "published_at": "2026-04-01T00:00:00Z",
            "assets": [
                {"name": "replay-control-app-aarch64.tar.gz", "size": 10000000,
                 "browser_download_url": format!("https://example.com/{tag}/binary.tar.gz")},
                {"name": "site.tar.gz", "size": 4000000,
                 "browser_download_url": format!("https://example.com/{tag}/site.tar.gz")},
                {"name": "replay-catalog.tar.gz", "size": 8000000,
                 "browser_download_url": format!("https://example.com/{tag}/catalog.tar.gz")}
            ]
        })
    }

    /// Stub the `/releases/latest` and `/releases` GitHub endpoints.
    /// Pass `latest = None` to make `/releases/latest` return 404.
    /// Returned mocks must be kept alive for the duration of the test —
    /// dropping a Mock removes it from the server.
    struct GhMocks {
        server: mockito::ServerGuard,
        _mocks: Vec<mockito::Mock>,
    }

    async fn mock_github(
        latest: Option<serde_json::Value>,
        releases: serde_json::Value,
    ) -> GhMocks {
        let mut server = mockito::Server::new_async().await;
        let latest_path = format!("/repos/{REPO}/releases/latest");
        let m_latest = match latest {
            Some(json) => {
                server
                    .mock("GET", latest_path.as_str())
                    .with_status(200)
                    .with_header("content-type", "application/json")
                    .with_body(json.to_string())
                    .create_async()
                    .await
            }
            None => {
                server
                    .mock("GET", latest_path.as_str())
                    .with_status(404)
                    .create_async()
                    .await
            }
        };
        let m_releases = server
            .mock(
                "GET",
                mockito::Matcher::Regex(format!("^/repos/{REPO}/releases(\\?.*)?$")),
            )
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(releases.to_string())
            .create_async()
            .await;
        GhMocks {
            server,
            _mocks: vec![m_latest, m_releases],
        }
    }

    // ── parse_release ───────────────────────────────────────────────

    #[test]
    fn parse_release_valid() {
        let json = make_release_json("v0.3.0", false);
        let update = parse_release(&json).unwrap();
        assert_eq!(update.version, "0.3.0");
        assert_eq!(update.tag, "v0.3.0");
        assert!(!update.prerelease);
        assert_eq!(update.binary_size, 10000000);
        assert_eq!(update.site_size, 4000000);
        assert_eq!(update.catalog_size, 8000000);
    }

    #[test]
    fn parse_release_prerelease() {
        let json = make_release_json("v0.3.0-beta.1", true);
        let update = parse_release(&json).unwrap();
        assert_eq!(update.version, "0.3.0-beta.1");
        assert!(update.prerelease);
    }

    #[test]
    fn parse_release_no_v_prefix() {
        let json = serde_json::json!({
            "tag_name": "1.0.0",
            "prerelease": false,
            "html_url": "https://example.com",
            "published_at": "2026-01-01T00:00:00Z",
            "assets": []
        });
        let update = parse_release(&json).unwrap();
        assert_eq!(update.version, "1.0.0");
        assert_eq!(update.tag, "1.0.0");
    }

    #[test]
    fn parse_release_invalid_semver() {
        let json = serde_json::json!({
            "tag_name": "not-semver",
            "prerelease": false,
            "html_url": "",
            "published_at": "",
            "assets": []
        });
        assert!(parse_release(&json).is_none());
    }

    #[test]
    fn parse_release_missing_tag() {
        let json = serde_json::json!({"prerelease": false});
        assert!(parse_release(&json).is_none());
    }

    #[test]
    fn parse_release_no_assets() {
        let json = serde_json::json!({
            "tag_name": "v1.0.0",
            "prerelease": false,
            "html_url": "",
            "published_at": ""
        });
        let update = parse_release(&json).unwrap();
        assert_eq!(update.binary_size, 0);
        assert_eq!(update.site_size, 0);
        assert_eq!(update.catalog_size, 0);
    }

    // ── classify_asset ──────────────────────────────────────────────

    #[test]
    fn classify_asset_recognises_binary_site_catalog() {
        assert_eq!(
            classify_asset("replay-control-app-aarch64-linux.tar.gz"),
            Some(AssetKind::Binary)
        );
        assert_eq!(
            classify_asset("replay-catalog.tar.gz"),
            Some(AssetKind::Catalog)
        );
        assert_eq!(
            classify_asset("replay-control-site.tar.gz"),
            Some(AssetKind::Site)
        );
        assert_eq!(classify_asset("checksums.sha256"), None);
        assert_eq!(classify_asset("install.sh"), None);
    }

    // ── resolve_asset_urls ──────────────────────────────────────────

    fn mock_release_endpoint(server: &mut mockito::ServerGuard, body: serde_json::Value) {
        let path = format!("/repos/{REPO}/releases/tags/v0.5.0");
        server
            .mock("GET", path.as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body.to_string())
            .create();
    }

    #[tokio::test]
    async fn resolve_asset_urls_includes_catalog_when_present() {
        let mut server = mockito::Server::new_async().await;
        mock_release_endpoint(&mut server, make_release_json("v0.5.0", false));
        let assets = resolve_asset_urls(&server.url(), REPO, "v0.5.0", None)
            .await
            .unwrap();
        assert!(assets.binary_url.ends_with("/binary.tar.gz"));
        assert!(assets.site_url.ends_with("/site.tar.gz"));
        assert_eq!(
            assets.catalog_url.as_deref(),
            Some("https://example.com/v0.5.0/catalog.tar.gz")
        );
    }

    #[tokio::test]
    async fn resolve_asset_urls_catalog_none_for_legacy_release() {
        // Releases before v0.4.0-beta.3 don't ship a catalog asset.
        let legacy = serde_json::json!({
            "tag_name": "v0.4.0-beta.2",
            "prerelease": true,
            "html_url": "",
            "published_at": "",
            "assets": [
                {"name": "replay-control-app-aarch64.tar.gz", "size": 1,
                 "browser_download_url": "https://example.com/binary.tar.gz"},
                {"name": "replay-site.tar.gz", "size": 1,
                 "browser_download_url": "https://example.com/site.tar.gz"}
            ]
        });
        let mut server = mockito::Server::new_async().await;
        let path = format!("/repos/{REPO}/releases/tags/v0.4.0-beta.2");
        server
            .mock("GET", path.as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(legacy.to_string())
            .create();
        let assets = resolve_asset_urls(&server.url(), REPO, "v0.4.0-beta.2", None)
            .await
            .unwrap();
        assert!(assets.catalog_url.is_none());
    }

    #[tokio::test]
    async fn resolve_asset_urls_errors_when_binary_missing() {
        let no_binary = serde_json::json!({
            "tag_name": "v0.5.0",
            "prerelease": false,
            "html_url": "",
            "published_at": "",
            "assets": [
                {"name": "replay-site.tar.gz", "size": 1,
                 "browser_download_url": "https://example.com/site.tar.gz"}
            ]
        });
        let mut server = mockito::Server::new_async().await;
        mock_release_endpoint(&mut server, no_binary);
        let err = resolve_asset_urls(&server.url(), REPO, "v0.5.0", None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("Binary asset not found"));
    }

    // ── check_github_update ─────────────────────────────────────────

    #[tokio::test]
    async fn check_finds_newer_stable() {
        let gh = mock_github(
            Some(make_release_json("v0.3.0", false)),
            serde_json::json!([]),
        )
        .await;
        let result = check_github_update(
            "0.1.0",
            &gh.server.url(),
            REPO,
            &UpdateChannel::Stable,
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(result.unwrap().version, "0.3.0");
    }

    #[tokio::test]
    async fn check_no_update_when_current() {
        let gh = mock_github(
            Some(make_release_json("v0.1.0", false)),
            serde_json::json!([]),
        )
        .await;
        let result = check_github_update(
            "0.1.0",
            &gh.server.url(),
            REPO,
            &UpdateChannel::Stable,
            None,
            None,
        )
        .await
        .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn check_no_update_when_newer_current() {
        let gh = mock_github(
            Some(make_release_json("v0.1.0", false)),
            serde_json::json!([]),
        )
        .await;
        let result = check_github_update(
            "0.2.0",
            &gh.server.url(),
            REPO,
            &UpdateChannel::Stable,
            None,
            None,
        )
        .await
        .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn check_skipped_version_ignored() {
        let gh = mock_github(
            Some(make_release_json("v0.3.0", false)),
            serde_json::json!([]),
        )
        .await;
        let result = check_github_update(
            "0.1.0",
            &gh.server.url(),
            REPO,
            &UpdateChannel::Stable,
            Some("0.3.0"),
            None,
        )
        .await
        .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn check_skipped_version_superseded() {
        let gh = mock_github(
            Some(make_release_json("v0.3.0", false)),
            serde_json::json!([]),
        )
        .await;
        let result = check_github_update(
            "0.1.0",
            &gh.server.url(),
            REPO,
            &UpdateChannel::Stable,
            Some("0.2.0"),
            None,
        )
        .await
        .unwrap();
        assert_eq!(result.unwrap().version, "0.3.0");
    }

    #[tokio::test]
    async fn check_beta_channel_finds_prerelease() {
        let gh = mock_github(
            Some(make_release_json("v0.1.0", false)),
            serde_json::json!([
                make_release_json("v0.2.0-beta.1", true),
                make_release_json("v0.1.0", false),
            ]),
        )
        .await;
        let result = check_github_update(
            "0.1.0",
            &gh.server.url(),
            REPO,
            &UpdateChannel::Beta,
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(result.unwrap().version, "0.2.0-beta.1");
    }

    #[tokio::test]
    async fn check_beta_channel_prefers_newest() {
        let gh = mock_github(
            Some(make_release_json("v0.1.0", false)),
            serde_json::json!([
                make_release_json("v0.3.0-beta.2", true),
                make_release_json("v0.3.0-beta.1", true),
                make_release_json("v0.2.0", false),
                make_release_json("v0.1.0", false),
            ]),
        )
        .await;
        let result = check_github_update(
            "0.1.0",
            &gh.server.url(),
            REPO,
            &UpdateChannel::Beta,
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(result.unwrap().version, "0.3.0-beta.2");
    }

    #[tokio::test]
    async fn check_beta_prerelease_not_newer_than_current() {
        let gh = mock_github(
            Some(make_release_json("v0.1.0", false)),
            serde_json::json!([
                make_release_json("v0.1.0-beta.4", true),
                make_release_json("v0.1.0", false),
            ]),
        )
        .await;
        let result = check_github_update(
            "0.1.0",
            &gh.server.url(),
            REPO,
            &UpdateChannel::Beta,
            None,
            None,
        )
        .await
        .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn check_stable_no_releases_returns_none() {
        let gh = mock_github(None, serde_json::json!([])).await;
        let result = check_github_update(
            "0.1.0",
            &gh.server.url(),
            REPO,
            &UpdateChannel::Stable,
            None,
            None,
        )
        .await
        .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn check_beta_with_no_stable_finds_prerelease() {
        let gh = mock_github(
            None,
            serde_json::json!([make_release_json("v0.2.0-beta.1", true)]),
        )
        .await;
        let result = check_github_update(
            "0.1.0",
            &gh.server.url(),
            REPO,
            &UpdateChannel::Beta,
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(result.unwrap().version, "0.2.0-beta.1");
    }
}
