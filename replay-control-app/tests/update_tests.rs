#![cfg(feature = "ssr")]

use replay_control_app::api::background::BackgroundManager;

/// Start a mock GitHub API server that serves canned release JSON.
async fn mock_github_server(
    latest_json: serde_json::Value,
    releases_json: serde_json::Value,
) -> (String, tokio::task::JoinHandle<()>) {
    use axum::Json;
    use axum::routing::get;

    let latest = latest_json.clone();
    let releases = releases_json.clone();

    let app = axum::Router::new()
        .route(
            "/repos/lapastillaroja/replay-control/releases/latest",
            get(move || {
                let json = latest.clone();
                async move { Json(json) }
            }),
        )
        .route(
            "/repos/lapastillaroja/replay-control/releases",
            get(move || {
                let json = releases.clone();
                async move { Json(json) }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (base_url, handle)
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
             "browser_download_url": format!("https://example.com/{tag}/site.tar.gz")}
        ]
    })
}

// ── parse_release tests ─────────────────────────────────────────────

#[test]
fn parse_release_valid() {
    let json = make_release_json("v0.3.0", false);
    let update = BackgroundManager::parse_release(&json).unwrap();
    assert_eq!(update.version, "0.3.0");
    assert_eq!(update.tag, "v0.3.0");
    assert!(!update.prerelease);
    assert_eq!(update.binary_size, 10000000);
    assert_eq!(update.site_size, 4000000);
}

#[test]
fn parse_release_prerelease() {
    let json = make_release_json("v0.3.0-beta.1", true);
    let update = BackgroundManager::parse_release(&json).unwrap();
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
    let update = BackgroundManager::parse_release(&json).unwrap();
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
    assert!(BackgroundManager::parse_release(&json).is_none());
}

#[test]
fn parse_release_missing_tag() {
    let json = serde_json::json!({"prerelease": false});
    assert!(BackgroundManager::parse_release(&json).is_none());
}

#[test]
fn parse_release_no_assets() {
    let json = serde_json::json!({
        "tag_name": "v1.0.0",
        "prerelease": false,
        "html_url": "",
        "published_at": ""
    });
    let update = BackgroundManager::parse_release(&json).unwrap();
    assert_eq!(update.binary_size, 0);
    assert_eq!(update.site_size, 0);
}

// ── check_github_update integration tests (mock HTTP) ───────────────

#[tokio::test]
async fn check_finds_newer_stable() {
    let latest = make_release_json("v0.3.0", false);
    let releases = serde_json::json!([]);
    let (base_url, handle) = mock_github_server(latest, releases).await;

    let result = BackgroundManager::check_github_update(
        "0.1.0",
        &base_url,
        &replay_control_core::update::UpdateChannel::Stable,
        None,
        None,
    )
    .await
    .unwrap();

    assert!(result.is_some());
    assert_eq!(result.unwrap().version, "0.3.0");
    handle.abort();
}

#[tokio::test]
async fn check_no_update_when_current() {
    let latest = make_release_json("v0.1.0", false);
    let releases = serde_json::json!([]);
    let (base_url, handle) = mock_github_server(latest, releases).await;

    let result = BackgroundManager::check_github_update(
        "0.1.0",
        &base_url,
        &replay_control_core::update::UpdateChannel::Stable,
        None,
        None,
    )
    .await
    .unwrap();

    assert!(result.is_none());
    handle.abort();
}

#[tokio::test]
async fn check_no_update_when_newer_current() {
    let latest = make_release_json("v0.1.0", false);
    let releases = serde_json::json!([]);
    let (base_url, handle) = mock_github_server(latest, releases).await;

    let result = BackgroundManager::check_github_update(
        "0.2.0",
        &base_url,
        &replay_control_core::update::UpdateChannel::Stable,
        None,
        None,
    )
    .await
    .unwrap();

    assert!(result.is_none());
    handle.abort();
}

#[tokio::test]
async fn check_skipped_version_ignored() {
    let latest = make_release_json("v0.3.0", false);
    let releases = serde_json::json!([]);
    let (base_url, handle) = mock_github_server(latest, releases).await;

    let result = BackgroundManager::check_github_update(
        "0.1.0",
        &base_url,
        &replay_control_core::update::UpdateChannel::Stable,
        Some("0.3.0"),
        None,
    )
    .await
    .unwrap();

    assert!(result.is_none());
    handle.abort();
}

#[tokio::test]
async fn check_skipped_version_superseded() {
    let latest = make_release_json("v0.3.0", false);
    let releases = serde_json::json!([]);
    let (base_url, handle) = mock_github_server(latest, releases).await;

    let result = BackgroundManager::check_github_update(
        "0.1.0",
        &base_url,
        &replay_control_core::update::UpdateChannel::Stable,
        Some("0.2.0"),
        None,
    )
    .await
    .unwrap();

    assert!(result.is_some());
    assert_eq!(result.unwrap().version, "0.3.0");
    handle.abort();
}

#[tokio::test]
async fn check_beta_channel_finds_prerelease() {
    let latest = make_release_json("v0.1.0", false);
    let releases = serde_json::json!([
        make_release_json("v0.2.0-beta.1", true),
        make_release_json("v0.1.0", false),
    ]);
    let (base_url, handle) = mock_github_server(latest, releases).await;

    let result = BackgroundManager::check_github_update(
        "0.1.0",
        &base_url,
        &replay_control_core::update::UpdateChannel::Beta,
        None,
        None,
    )
    .await
    .unwrap();

    assert!(result.is_some());
    assert_eq!(result.unwrap().version, "0.2.0-beta.1");
    handle.abort();
}

#[tokio::test]
async fn check_beta_channel_prefers_newest() {
    let latest = make_release_json("v0.1.0", false);
    let releases = serde_json::json!([
        make_release_json("v0.3.0-beta.2", true),
        make_release_json("v0.3.0-beta.1", true),
        make_release_json("v0.2.0", false),
        make_release_json("v0.1.0", false),
    ]);
    let (base_url, handle) = mock_github_server(latest, releases).await;

    let result = BackgroundManager::check_github_update(
        "0.1.0",
        &base_url,
        &replay_control_core::update::UpdateChannel::Beta,
        None,
        None,
    )
    .await
    .unwrap();

    assert!(result.is_some());
    assert_eq!(result.unwrap().version, "0.3.0-beta.2");
    handle.abort();
}

#[tokio::test]
async fn check_beta_prerelease_not_newer_than_current() {
    let latest = make_release_json("v0.1.0", false);
    let releases = serde_json::json!([
        make_release_json("v0.1.0-beta.4", true),
        make_release_json("v0.1.0", false),
    ]);
    let (base_url, handle) = mock_github_server(latest, releases).await;

    let result = BackgroundManager::check_github_update(
        "0.1.0",
        &base_url,
        &replay_control_core::update::UpdateChannel::Beta,
        None,
        None,
    )
    .await
    .unwrap();

    assert!(result.is_none());
    handle.abort();
}

// ── Stable 404 test ─────────────────────────────────────────────

/// Mock server that returns 404 for /releases/latest (no stable releases).
async fn mock_github_server_no_stable(
    releases_json: serde_json::Value,
) -> (String, tokio::task::JoinHandle<()>) {
    use axum::Json;
    use axum::http::StatusCode;
    use axum::routing::get;

    let releases = releases_json.clone();

    let app = axum::Router::new()
        .route(
            "/repos/lapastillaroja/replay-control/releases/latest",
            get(|| async { StatusCode::NOT_FOUND }),
        )
        .route(
            "/repos/lapastillaroja/replay-control/releases",
            get(move || {
                let json = releases.clone();
                async move { Json(json) }
            }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (base_url, handle)
}

#[tokio::test]
async fn check_stable_no_releases_returns_none() {
    let releases = serde_json::json!([]);
    let (base_url, handle) = mock_github_server_no_stable(releases).await;

    let result = BackgroundManager::check_github_update(
        "0.1.0",
        &base_url,
        &replay_control_core::update::UpdateChannel::Stable,
        None,
        None,
    )
    .await
    .unwrap();

    assert!(result.is_none());
    handle.abort();
}

#[tokio::test]
async fn check_beta_with_no_stable_finds_prerelease() {
    let releases = serde_json::json!([make_release_json("v0.2.0-beta.1", true),]);
    let (base_url, handle) = mock_github_server_no_stable(releases).await;

    let result = BackgroundManager::check_github_update(
        "0.1.0",
        &base_url,
        &replay_control_core::update::UpdateChannel::Beta,
        None,
        None,
    )
    .await
    .unwrap();

    assert!(result.is_some());
    assert_eq!(result.unwrap().version, "0.2.0-beta.1");
    handle.abort();
}
