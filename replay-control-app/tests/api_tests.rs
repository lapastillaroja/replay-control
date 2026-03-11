#![cfg(feature = "ssr")]

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use common::{assert_json_ok, cleanup_test_storage, create_test_storage, test_api_router, test_app_state};

#[tokio::test]
async fn api_systems_returns_ok_with_json() {
    let tmp = create_test_storage();
    let state = test_app_state(&tmp);
    let app = test_api_router(state);

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

    // Should contain at least one system with games (nintendo_nes has 2 ROMs).
    assert!(
        systems.iter().any(|s| s["game_count"].as_u64().unwrap() > 0),
        "at least one system should have games"
    );

    // Verify nintendo_nes is present with the correct game count.
    let nes = systems
        .iter()
        .find(|s| s["folder_name"] == "nintendo_nes")
        .expect("nintendo_nes should be in systems list");
    assert_eq!(nes["game_count"].as_u64().unwrap(), 2);

    cleanup_test_storage(&tmp);
}

#[tokio::test]
async fn api_system_roms_returns_roms_for_valid_system() {
    let tmp = create_test_storage();
    let state = test_app_state(&tmp);
    let app = test_api_router(state);

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

    // GameRef is flattened into RomEntry, so rom_filename is at the top level.
    let filenames: Vec<&str> = roms
        .iter()
        .map(|r| r["rom_filename"].as_str().unwrap())
        .collect();
    assert!(filenames.contains(&"TestGame.nes"));
    assert!(filenames.contains(&"AnotherGame (USA).nes"));

    cleanup_test_storage(&tmp);
}

#[tokio::test]
async fn api_system_roms_returns_not_found_for_nonexistent_system() {
    let tmp = create_test_storage();
    let state = test_app_state(&tmp);
    let app = test_api_router(state);

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

    cleanup_test_storage(&tmp);
}

#[tokio::test]
async fn api_info_returns_system_info() {
    let tmp = create_test_storage();
    let state = test_app_state(&tmp);
    let app = test_api_router(state);

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

    // Should report correct storage info.
    assert_eq!(json["storage_kind"].as_str().unwrap(), "sd");
    assert!(json["storage_root"].as_str().unwrap().contains("replay-integ-"));
    assert!(json["total_systems"].as_u64().unwrap() > 0);
    assert_eq!(json["total_games"].as_u64().unwrap(), 3); // 2 NES + 1 SMD
    assert_eq!(json["total_favorites"].as_u64().unwrap(), 0);

    cleanup_test_storage(&tmp);
}

#[tokio::test]
async fn api_favorites_empty_initially() {
    let tmp = create_test_storage();
    let state = test_app_state(&tmp);
    let app = test_api_router(state);

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

    cleanup_test_storage(&tmp);
}

#[tokio::test]
async fn api_recents_empty_initially() {
    let tmp = create_test_storage();
    let state = test_app_state(&tmp);
    let app = test_api_router(state);

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

    cleanup_test_storage(&tmp);
}

#[tokio::test]
async fn api_systems_includes_multiple_systems() {
    let tmp = create_test_storage();
    let state = test_app_state(&tmp);
    let app = test_api_router(state);

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

    // Both test systems should be present with games.
    let with_games: Vec<&str> = systems
        .iter()
        .filter(|s| s["game_count"].as_u64().unwrap() > 0)
        .map(|s| s["folder_name"].as_str().unwrap())
        .collect();
    assert!(with_games.contains(&"nintendo_nes"));
    assert!(with_games.contains(&"sega_smd"));

    cleanup_test_storage(&tmp);
}
