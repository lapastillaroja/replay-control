#![cfg(feature = "ssr")]
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use replay_control_app::api::AppState;

static TEST_COUNTER: AtomicU32 = AtomicU32::new(0);

/// Create a temp directory with a minimal ROM storage layout for integration tests.
/// Returns the temp directory path. Caller must call `cleanup_test_storage` when done.
pub fn create_test_storage() -> PathBuf {
    let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = std::env::temp_dir().join(format!("replay-integ-{}-{id}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);

    // Create directory structure
    for dir in &[
        "roms/_favorites",
        "roms/_recent",
        "roms/nintendo_nes",
        "roms/sega_smd",
        ".replay-control/media",
        "config",
    ] {
        std::fs::create_dir_all(tmp.join(dir)).unwrap();
    }

    // Create fake ROM files
    std::fs::write(tmp.join("roms/nintendo_nes/TestGame.nes"), b"fake").unwrap();
    std::fs::write(tmp.join("roms/nintendo_nes/AnotherGame (USA).nes"), b"fake").unwrap();
    std::fs::write(
        tmp.join("roms/sega_smd/Sonic The Hedgehog (USA).md"),
        b"fake",
    )
    .unwrap();

    // Minimal config
    std::fs::write(tmp.join("config/replay.cfg"), "storage_mode=sd\n").unwrap();

    tmp
}

/// Create an AppState pointing at the given test storage directory.
pub fn test_app_state(tmp: &std::path::Path) -> AppState {
    AppState::new(Some(tmp.to_string_lossy().into_owned()), None, None).unwrap()
}

/// Build the full application router (API + server functions + SSR fallback).
/// Requires that server functions have been registered and the Leptos executor
/// has been initialized.
pub fn test_router(state: AppState) -> axum::Router {
    let leptos_options = leptos::config::LeptosOptions::builder()
        .output_name("replay_control_app")
        .site_root("target/site")
        .site_pkg_dir("pkg")
        .build();
    replay_control_app::api::build_router(state, leptos_options)
}

/// Build an API-only router (no SSR fallback, no server function handler).
/// Useful for tests that only exercise REST API routes.
pub fn test_api_router(state: AppState) -> axum::Router {
    axum::Router::new()
        .nest(
            "/api",
            axum::Router::new()
                .merge(replay_control_app::api::system_info::routes())
                .merge(replay_control_app::api::roms::routes())
                .merge(replay_control_app::api::favorites::routes())
                .merge(replay_control_app::api::recents::routes()),
        )
        .with_state(state)
}

/// Register all server functions needed for testing.
/// Must be called once per test process (safe to call multiple times).
pub fn register_server_fns() {
    use replay_control_app::server_fns;

    server_fn::axum::register_explicit::<server_fns::GetInfo>();
    server_fn::axum::register_explicit::<server_fns::GetSystems>();
    server_fn::axum::register_explicit::<server_fns::GetFavorites>();
    server_fn::axum::register_explicit::<server_fns::GetRecents>();
    server_fn::axum::register_explicit::<server_fns::GetRomsPage>();
    server_fn::axum::register_explicit::<server_fns::GetSystemFavorites>();
    server_fn::axum::register_explicit::<server_fns::GetRomDetail>();
    server_fn::axum::register_explicit::<server_fns::RefreshStorage>();
    server_fn::axum::register_explicit::<server_fns::GetMetadataStats>();
    server_fn::axum::register_explicit::<server_fns::GetSystemCoverage>();
    server_fn::axum::register_explicit::<server_fns::GetImageStats>();
    server_fn::axum::register_explicit::<server_fns::GetSystemLogs>();
    server_fn::axum::register_explicit::<server_fns::GlobalSearch>();
    server_fn::axum::register_explicit::<server_fns::GetAllGenres>();
    server_fn::axum::register_explicit::<server_fns::GetSystemGenres>();
    server_fn::axum::register_explicit::<server_fns::RandomGame>();
    server_fn::axum::register_explicit::<server_fns::GetRegionPreference>();
    server_fn::axum::register_explicit::<server_fns::GetRecommendations>();
    server_fn::axum::register_explicit::<server_fns::GetCorruptionStatus>();
    server_fn::axum::register_explicit::<server_fns::RebuildCorruptMetadata>();
    server_fn::axum::register_explicit::<server_fns::RepairCorruptUserData>();
    server_fn::axum::register_explicit::<server_fns::RestoreUserDataBackup>();
    server_fn::axum::register_explicit::<server_fns::CheckForUpdates>();
    server_fn::axum::register_explicit::<server_fns::GetUpdateChannel>();
    server_fn::axum::register_explicit::<server_fns::SaveUpdateChannel>();
    server_fn::axum::register_explicit::<server_fns::SkipVersion>();
    server_fn::axum::register_explicit::<server_fns::StartUpdate>();
}

/// Initialize the Leptos async executor for SSR.
/// Safe to call multiple times (only the first call takes effect).
pub fn init_executor() {
    let _ = any_spawner::Executor::init_tokio();
}

/// Clean up a test storage directory.
pub fn cleanup_test_storage(tmp: &std::path::Path) {
    let _ = std::fs::remove_dir_all(tmp);
}

/// Helper to assert a response has status 200 and parse the JSON body.
pub async fn assert_json_ok(resp: axum::http::Response<axum::body::Body>) -> serde_json::Value {
    use http_body_util::BodyExt;

    assert_eq!(resp.status(), axum::http::StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&body).unwrap()
}
