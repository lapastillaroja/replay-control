//! Black-box characterization tests for the developer/board **facet pages** and
//! the region-preference **ranking** used by the recommendation/search path.
//!
//! These lock the *observable* contract (what the server functions return for a
//! seeded library) so the planned dedup of the region-preference CTE and the
//! `games_by_facet` queries can be refactored with a regression net in place.
//! They deliberately assert only on server-function output — never on SQL or
//! internal query structure — so an implementation change that preserves
//! behavior keeps them green, and one that shifts results turns them red.

#![cfg(feature = "ssr")]

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use server_fn::ServerFn;
use tower::ServiceExt;

use common::{TestEnv, init_executor, nes_entry, register_server_fns, seed_system, test_router};
use replay_control_app::server_fns::{
    GetBoardGames, GetDeveloperGames, GetRelatedGames, SearchByDeveloper,
};
use replay_control_core::arcade_board::ArcadeBoard;
use replay_control_core::rom_tags::RegionPreference;
use replay_control_core_server::library_db::GameEntry;

fn setup() {
    init_executor();
    register_server_fns();
}

/// POST a urlencoded server-fn body and parse the JSON response, asserting 200.
async fn post_form<F: ServerFn>(app: axum::Router, body: String) -> serde_json::Value {
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(F::PATH)
                .header("content-type", "application/x-www-form-urlencoded")
                .header("accept", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "{} should be 200", F::PATH);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or_else(|e| panic!("bad JSON from {}: {e}", F::PATH))
}

/// An NES entry with an explicit developer / clone flag / region.
fn dev_entry(filename: &str, base_title: &str, developer: &str, is_clone: bool) -> GameEntry {
    GameEntry {
        developer: developer.to_string(),
        is_clone,
        ..nes_entry(filename, base_title, "Action", false)
    }
}

/// An NES entry pinned to a specific region (same `base_title` across regions
/// makes the region-preference ranking observable).
fn region_entry(filename: &str, base_title: &str, developer: &str, region: &str) -> GameEntry {
    GameEntry {
        region: region.to_string(),
        ..dev_entry(filename, base_title, developer, false)
    }
}

/// An NES entry in a named series and region (for the series-siblings path).
fn series_entry(filename: &str, base_title: &str, series_key: &str, region: &str) -> GameEntry {
    GameEntry {
        series_key: series_key.to_string(),
        region: region.to_string(),
        ..dev_entry(filename, base_title, "Konami", false)
    }
}

// ── Developer facet page (games_by_facet, developer side) ──────────────────

#[tokio::test(flavor = "multi_thread")]
async fn developer_facet_scopes_to_the_named_developer_and_reports_total() {
    setup();
    let env = TestEnv::new().await;
    seed_system(
        &env.state,
        "nintendo_nes",
        vec![
            dev_entry("Contra.nes", "Contra", "Konami", false),
            dev_entry("Castlevania.nes", "Castlevania", "Konami", false),
            dev_entry("Gradius.nes", "Gradius", "Konami", false),
            dev_entry("Mega Man.nes", "Mega Man", "Capcom", false),
            dev_entry("Ghosts n Goblins.nes", "Ghosts n Goblins", "Capcom", false),
        ],
    )
    .await;
    let app = test_router(env.state.clone());

    let data =
        post_form::<GetDeveloperGames>(app, "developer=Konami&offset=0&limit=10".into()).await;

    assert_eq!(data["total"], 3, "only Konami's three games should count");
    assert_eq!(data["roms"].as_array().unwrap().len(), 3);
    assert_eq!(data["has_more"], false);
    assert_eq!(data["developer"], "Konami");
}

#[tokio::test(flavor = "multi_thread")]
async fn developer_facet_paginates_with_has_more() {
    setup();
    let env = TestEnv::new().await;
    seed_system(
        &env.state,
        "nintendo_nes",
        vec![
            dev_entry("Contra.nes", "Contra", "Konami", false),
            dev_entry("Castlevania.nes", "Castlevania", "Konami", false),
            dev_entry("Gradius.nes", "Gradius", "Konami", false),
        ],
    )
    .await;
    let app = test_router(env.state.clone());

    let data =
        post_form::<GetDeveloperGames>(app, "developer=Konami&offset=0&limit=2".into()).await;

    assert_eq!(
        data["total"], 3,
        "total counts the full match set, not the page"
    );
    assert_eq!(
        data["roms"].as_array().unwrap().len(),
        2,
        "page is capped at limit"
    );
    assert_eq!(data["has_more"], true);
}

#[tokio::test(flavor = "multi_thread")]
async fn developer_facet_hide_clones_filter() {
    setup();
    let env = TestEnv::new().await;
    seed_system(
        &env.state,
        "nintendo_nes",
        vec![
            dev_entry("Contra.nes", "Contra", "Konami", false),
            dev_entry("Castlevania.nes", "Castlevania", "Konami", false),
            dev_entry("Contra (Clone).nes", "Contra", "Konami", true),
        ],
    )
    .await;
    let app = test_router(env.state.clone());

    let all = post_form::<GetDeveloperGames>(
        app.clone(),
        "developer=Konami&offset=0&limit=10&hide_clones=false".into(),
    )
    .await;
    assert_eq!(all["total"], 3, "unfiltered includes the clone");

    let no_clones = post_form::<GetDeveloperGames>(
        app,
        "developer=Konami&offset=0&limit=10&hide_clones=true".into(),
    )
    .await;
    assert_eq!(no_clones["total"], 2, "hide_clones drops the clone row");
}

// ── Board facet page (games_by_facet, board side) ──────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn board_facet_scopes_to_the_named_board() {
    setup();
    let env = TestEnv::new().await;
    let cps2 = GameEntry {
        board: Some(ArcadeBoard::Cps2),
        ..dev_entry("sf2.zip", "Street Fighter II", "Capcom", false)
    };
    let cps2b = GameEntry {
        board: Some(ArcadeBoard::Cps2),
        ..dev_entry("dstlk.zip", "Darkstalkers", "Capcom", false)
    };
    let neogeo = GameEntry {
        board: Some(ArcadeBoard::NeoGeoMvs),
        ..dev_entry("mslug.zip", "Metal Slug", "SNK", false)
    };
    seed_system(&env.state, "nintendo_nes", vec![cps2, cps2b, neogeo]).await;
    let app = test_router(env.state.clone());

    let tag = ArcadeBoard::Cps2.as_tag();
    let data = post_form::<GetBoardGames>(app, format!("board_tag={tag}&offset=0&limit=10")).await;

    assert_eq!(data["total"], 2, "only the two CPS-2 games should count");
    assert_eq!(data["roms"].as_array().unwrap().len(), 2);
    assert_eq!(data["board_tag"], tag);
}

// ── Region-preference ranking (the region-CTE) ─────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn region_preference_prefers_the_preferred_region_dump() {
    // Assert the *invariant* — the preferred-region dump wins the region-ranked
    // dedup — without hardcoding the default preference. The preferred dump is
    // seeded in the middle so plain insertion order can't explain a win.
    setup();
    let env = TestEnv::new().await;
    let pref = env.state.region_preference().as_str().to_string();
    let others: Vec<&str> = ["japan", "usa", "europe", "world"]
        .into_iter()
        .filter(|r| *r != pref)
        .take(2)
        .collect();
    seed_system(
        &env.state,
        "nintendo_nes",
        vec![
            region_entry("Contra (other-a).nes", "Contra", "Konami", others[0]),
            region_entry("Contra (preferred).nes", "Contra", "Konami", &pref),
            region_entry("Contra (other-b).nes", "Contra", "Konami", others[1]),
        ],
    )
    .await;
    let app = test_router(env.state.clone());

    let data = post_form::<SearchByDeveloper>(app, "query=Konami&limit=10".into()).await;

    let games = data["games"].as_array().expect("games array");
    // Region dedup collapses the three dumps of the same base_title to one row.
    let contra: Vec<_> = games
        .iter()
        .filter(|g| g["display_name"].as_str() == Some("Contra"))
        .collect();
    assert_eq!(contra.len(), 1, "region dedup yields a single Contra row");
    let picked = contra[0]["rom_filename"].as_str().unwrap_or_default();
    assert_eq!(
        picked, "Contra (preferred).nes",
        "the preferred-region ({pref}) dump should win the dedup"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn series_siblings_honor_secondary_region() {
    // Regression: series/related surfaces used to ignore the secondary-region
    // preference (they only ranked primary + world). With primary=Japan (no dump
    // for the sibling) and secondary=USA, the sibling's USA dump must win over an
    // other-region (Europe) dump. Europe is seeded first, so a rank that ignores
    // the secondary would surface Europe by insertion order.
    setup();
    let env = TestEnv::new().await;
    {
        let mut prefs = env.state.prefs.write().unwrap();
        prefs.region = RegionPreference::Japan;
        prefs.region_secondary = Some(RegionPreference::Usa);
    }
    seed_system(
        &env.state,
        "nintendo_nes",
        vec![
            series_entry("Contra.nes", "Contra", "contra-series", "japan"),
            series_entry("Super C (Europe).nes", "Super C", "contra-series", "europe"),
            series_entry("Super C (USA).nes", "Super C", "contra-series", "usa"),
        ],
    )
    .await;
    let app = test_router(env.state.clone());

    let data =
        post_form::<GetRelatedGames>(app, "system=nintendo_nes&filename=Contra.nes".into()).await;

    let siblings = data["series_siblings"]
        .as_array()
        .expect("series_siblings array");
    let super_c: Vec<_> = siblings
        .iter()
        .filter(|g| g["display_name"].as_str() == Some("Super C"))
        .collect();
    assert_eq!(super_c.len(), 1, "the sibling dedups to one row");
    assert_eq!(
        super_c[0]["rom_filename"].as_str().unwrap_or_default(),
        "Super C (USA).nes",
        "the secondary-region (USA) dump should surface, not the other-region (Europe) one"
    );
}
