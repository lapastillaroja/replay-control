#![cfg(feature = "ssr")]

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use server_fn::ServerFn;
use tower::ServiceExt;

use common::{TestEnv, init_executor, register_server_fns, test_router};
use replay_control_app::server_fns;

/// Server function tests require the Leptos executor and server function
/// registration. These are process-global and safe to call multiple times.
fn setup() {
    init_executor();
    register_server_fns();
}

#[tokio::test(flavor = "multi_thread")]
async fn sfn_get_systems_returns_test_systems() {
    setup();
    let env = TestEnv::new();
    let app = test_router(env.state.clone());

    let path = <server_fns::GetSystems as ServerFn>::PATH;

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .header("content-type", "application/x-www-form-urlencoded")
                .header("accept", "application/x-www-form-urlencoded")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "GetSystems should return 200"
    );

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert!(!body.is_empty(), "response body should not be empty");
}

#[tokio::test(flavor = "multi_thread")]
async fn sfn_get_roms_page_returns_roms() {
    setup();
    let env = TestEnv::new();
    let app = test_router(env.state.clone());

    let path = <server_fns::GetRomsPage as ServerFn>::PATH;
    let params = "system=nintendo_nes&offset=0&limit=50&search=";

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .header("content-type", "application/x-www-form-urlencoded")
                .header("accept", "application/x-www-form-urlencoded")
                .body(Body::from(params))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "GetRomsPage should return 200"
    );

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert!(!body.is_empty(), "response body should not be empty");
}

#[tokio::test(flavor = "multi_thread")]
async fn sfn_get_info_returns_system_info() {
    setup();
    let env = TestEnv::new();
    let app = test_router(env.state.clone());

    let path = <server_fns::GetInfo as ServerFn>::PATH;

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .header("content-type", "application/x-www-form-urlencoded")
                .header("accept", "application/x-www-form-urlencoded")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK, "GetInfo should return 200");

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert!(!body.is_empty(), "response body should not be empty");
}

#[tokio::test(flavor = "multi_thread")]
async fn sfn_nonexistent_function_returns_error() {
    setup();
    let env = TestEnv::new();
    let app = test_router(env.state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/sfn/no_such_function")
                .header("content-type", "application/x-www-form-urlencoded")
                .header("accept", "application/x-www-form-urlencoded")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert!(
        resp.status() == StatusCode::BAD_REQUEST || resp.status() == StatusCode::NOT_FOUND,
        "non-existent server function should return 400 or 404, got {}",
        resp.status()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn sfn_get_setup_status_returns_200() {
    setup();
    let env = TestEnv::new();
    let app = test_router(env.state.clone());

    let path = <server_fns::GetSetupStatus as ServerFn>::PATH;

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .header("content-type", "application/x-www-form-urlencoded")
                .header("accept", "application/x-www-form-urlencoded")
                .body(Body::from("force=false"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "GetSetupStatus should return 200"
    );

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert!(!body.is_empty(), "response body should not be empty");
}

#[tokio::test(flavor = "multi_thread")]
async fn sfn_dismiss_setup_returns_200() {
    setup();
    let env = TestEnv::new();
    let app = test_router(env.state.clone());

    let path = <server_fns::DismissSetup as ServerFn>::PATH;

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .header("content-type", "application/x-www-form-urlencoded")
                .header("accept", "application/x-www-form-urlencoded")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "DismissSetup should return 200"
    );
}
