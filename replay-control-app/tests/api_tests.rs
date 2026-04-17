#![cfg(feature = "ssr")]

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use common::{TestEnv, assert_json_ok, test_api_router};

#[tokio::test(flavor = "multi_thread")]
async fn api_systems_returns_ok_with_json() {
    let env = TestEnv::new();
    let app = test_api_router(env.state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/systems")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let json = assert_json_ok(resp).await;
    let systems = json.as_array().expect("should be an array");

    assert!(
        systems
            .iter()
            .any(|s| s["game_count"].as_u64().unwrap() > 0),
        "at least one system should have games"
    );

    let nes = systems
        .iter()
        .find(|s| s["folder_name"] == "nintendo_nes")
        .expect("nintendo_nes should be in systems list");
    assert_eq!(nes["game_count"].as_u64().unwrap(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn api_system_roms_returns_roms_for_valid_system() {
    let env = TestEnv::new();
    let app = test_api_router(env.state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/systems/nintendo_nes/roms")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let json = assert_json_ok(resp).await;
    let roms = json.as_array().expect("should be an array");
    assert_eq!(roms.len(), 2, "nintendo_nes should have 2 ROMs");

    let filenames: Vec<&str> = roms
        .iter()
        .map(|r| r["rom_filename"].as_str().unwrap())
        .collect();
    assert!(filenames.contains(&"TestGame.nes"));
    assert!(filenames.contains(&"AnotherGame (USA).nes"));
}

#[tokio::test(flavor = "multi_thread")]
async fn api_system_roms_returns_not_found_for_nonexistent_system() {
    let env = TestEnv::new();
    let app = test_api_router(env.state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/systems/nonexistent_system/roms")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test(flavor = "multi_thread")]
async fn api_info_returns_system_info() {
    let env = TestEnv::new();
    let app = test_api_router(env.state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/info")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let json = assert_json_ok(resp).await;

    assert_eq!(json["storage_kind"].as_str().unwrap(), "sd");
    assert!(
        json["storage_root"]
            .as_str()
            .unwrap()
            .contains("replay-integ-")
    );
    assert!(json["total_systems"].as_u64().unwrap() > 0);
    assert_eq!(json["total_games"].as_u64().unwrap(), 3);
    assert_eq!(json["total_favorites"].as_u64().unwrap(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn api_favorites_empty_initially() {
    let env = TestEnv::new();
    let app = test_api_router(env.state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/favorites")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let json = assert_json_ok(resp).await;
    let favs = json.as_array().expect("should be an array");
    assert!(favs.is_empty(), "favorites should be empty initially");
}

#[tokio::test(flavor = "multi_thread")]
async fn api_recents_empty_initially() {
    let env = TestEnv::new();
    let app = test_api_router(env.state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/recents")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let json = assert_json_ok(resp).await;
    let recents = json.as_array().expect("should be an array");
    assert!(recents.is_empty(), "recents should be empty initially");
}

#[tokio::test(flavor = "multi_thread")]
async fn api_systems_includes_multiple_systems() {
    let env = TestEnv::new();
    let app = test_api_router(env.state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/systems")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let json = assert_json_ok(resp).await;
    let systems = json.as_array().unwrap();

    let with_games: Vec<&str> = systems
        .iter()
        .filter(|s| s["game_count"].as_u64().unwrap() > 0)
        .map(|s| s["folder_name"].as_str().unwrap())
        .collect();
    assert!(with_games.contains(&"nintendo_nes"));
    assert!(with_games.contains(&"sega_smd"));
}
