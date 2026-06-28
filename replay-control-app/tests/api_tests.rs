#![cfg(feature = "ssr")]

mod common;

use axum::body::Body;
use axum::http::Request;
use tower::ServiceExt;

use common::{TestEnv, assert_json_ok, test_api_router};

#[tokio::test(flavor = "multi_thread")]
async fn api_systems_returns_ok_with_json() {
    let env = TestEnv::new().await;
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
    let env = TestEnv::new().await;
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
async fn api_system_roms_returns_empty_for_nonexistent_system() {
    // The handler is read-only (L2-only) since the write-isolation work:
    // an unknown system has no L2 rows and the handler returns an empty
    // list rather than 404. Clients distinguish "no roms" from "system
    // doesn't exist" via /api/systems.
    let env = TestEnv::new().await;
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

    let json = assert_json_ok(resp).await;
    let roms = json.as_array().expect("should be an array");
    assert!(roms.is_empty(), "unknown system should produce empty list");
}

#[tokio::test(flavor = "multi_thread")]
async fn api_info_returns_system_info() {
    let env = TestEnv::new().await;
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

    // Standalone mode (--storage-path was given) always reports "folder";
    // sd/usb/nvme/nfs are RePlayOS-only kinds resolved from the device's mount table.
    assert_eq!(json["storage_kind"].as_str().unwrap(), "folder");
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
    let env = TestEnv::new().await;
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
    let env = TestEnv::new().await;
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
    let env = TestEnv::new().await;
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

/// The library CSV export returns a per-ROM, attachment-headed CSV with one
/// data row per ROM in the requested system, prefixed by the column header.
#[tokio::test(flavor = "multi_thread")]
async fn api_export_library_csv_returns_per_rom_rows() {
    use replay_control_core_server::coverage_export::{CSV_COLUMNS, csv_header_line};

    let env = TestEnv::new().await;
    let app = axum::Router::new()
        .nest("/api", replay_control_app::api::export::routes())
        .with_state(env.state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/export/library.csv?system=nintendo_nes")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers().get("content-type").unwrap(),
        "text/csv; charset=utf-8"
    );
    let disposition = resp
        .headers()
        .get("content-disposition")
        .unwrap()
        .to_str()
        .unwrap();
    // Filename carries an ISO-basic UTC timestamp, e.g.
    // library-metadata-nintendo_nes-20260628T143005Z.csv
    assert!(
        disposition.starts_with("attachment; filename=\"library-metadata-nintendo_nes-"),
        "unexpected disposition: {disposition}"
    );
    assert!(disposition.ends_with(".csv\""));

    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let csv = String::from_utf8(bytes.to_vec()).unwrap();

    // Header first, then one CRLF-terminated row per seeded ROM.
    assert!(csv.starts_with(&csv_header_line()));
    assert!(csv.contains("TestGame.nes"));
    assert!(csv.contains("AnotherGame (USA).nes"));

    let lines: Vec<&str> = csv.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 3, "header + 2 ROM rows");
    // Every row carries the full column set.
    for line in &lines[1..] {
        assert_eq!(line.split(',').count(), CSV_COLUMNS.len());
    }
}

/// "All systems" export (no `system` param, or an empty one) covers every
/// active system — a regression guard for the empty-string `?system=` that the
/// "All systems" <option> submits being mistaken for a system named "".
#[tokio::test(flavor = "multi_thread")]
async fn api_export_library_csv_all_systems_covers_every_rom() {
    let env = TestEnv::new().await;

    let data_line_count = |csv: &str| csv.lines().filter(|l| !l.is_empty()).count();

    for uri in ["/api/export/library.csv", "/api/export/library.csv?system="] {
        let app = axum::Router::new()
            .nest("/api", replay_control_app::api::export::routes())
            .with_state(env.state.clone());

        let resp = app
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), 200, "uri={uri}");
        let disposition = resp
            .headers()
            .get("content-disposition")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert!(
            disposition.starts_with("attachment; filename=\"library-metadata-all-"),
            "uri={uri} disposition={disposition}"
        );

        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let csv = String::from_utf8(bytes.to_vec()).unwrap();

        // Header + one row per ROM across all seeded systems (total_games == 3).
        assert_eq!(data_line_count(&csv), 4, "uri={uri}: header + 3 ROM rows");
        assert!(csv.contains("nintendo_nes"), "uri={uri}");
        assert!(csv.contains("sega_smd"), "uri={uri}");
    }
}
