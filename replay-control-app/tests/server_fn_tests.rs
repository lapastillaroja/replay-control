#![cfg(feature = "ssr")]

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use server_fn::ServerFn;
use tower::ServiceExt;

use common::{
    cleanup_test_storage, create_test_storage, init_executor, register_server_fns, test_app_state,
    test_router,
};
use replay_control_app::server_fns;

/// Server function tests require the Leptos executor and server function
/// registration. These are process-global and safe to call multiple times.
fn setup() {
    init_executor();
    register_server_fns();
}

#[tokio::test]
async fn sfn_get_systems_returns_test_systems() {
    setup();
    let tmp = create_test_storage();
    let state = test_app_state(&tmp);
    let app = test_router(state);

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

    cleanup_test_storage(&tmp);
}

#[tokio::test]
async fn sfn_get_roms_page_returns_roms() {
    setup();
    let tmp = create_test_storage();
    let state = test_app_state(&tmp);
    let app = test_router(state);

    let path = <server_fns::GetRomsPage as ServerFn>::PATH;

    // URL-encode the server function parameters.
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

    cleanup_test_storage(&tmp);
}

#[tokio::test]
async fn sfn_get_info_returns_system_info() {
    setup();
    let tmp = create_test_storage();
    let state = test_app_state(&tmp);
    let app = test_router(state);

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

    cleanup_test_storage(&tmp);
}

#[tokio::test]
async fn sfn_nonexistent_function_returns_error() {
    setup();
    let tmp = create_test_storage();
    let state = test_app_state(&tmp);
    let app = test_router(state);

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

    // Non-existent server function should return 400 or 404.
    assert!(
        resp.status() == StatusCode::BAD_REQUEST || resp.status() == StatusCode::NOT_FOUND,
        "non-existent server function should return 400 or 404, got {}",
        resp.status()
    );

    cleanup_test_storage(&tmp);
}
