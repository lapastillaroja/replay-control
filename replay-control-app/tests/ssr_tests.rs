#![cfg(feature = "ssr")]

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

use common::{TestEnv, init_executor, register_server_fns, test_router};

/// SSR tests require the Leptos executor and server function registration.
fn setup() {
    init_executor();
    register_server_fns();
}

#[tokio::test(flavor = "multi_thread")]
async fn home_page_returns_200_with_replay_control() {
    setup();
    let env = TestEnv::new();
    let app = test_router(env.state.clone());

    let resp = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8(body.to_vec()).unwrap();

    assert!(
        html.contains("Replay Control"),
        "home page should contain 'Replay Control'"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn settings_page_returns_200() {
    setup();
    let env = TestEnv::new();
    let app = test_router(env.state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/settings")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread")]
async fn nonexistent_page_returns_200_with_not_found_message() {
    setup();
    let env = TestEnv::new();
    let app = test_router(env.state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/nonexistent-page")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8(body.to_vec()).unwrap();

    assert!(
        html.contains("Page not found"),
        "non-existent page should contain 'Page not found'"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn style_css_endpoint_returns_css() {
    setup();
    let env = TestEnv::new();
    let app = test_router(env.state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/static/style.css")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let content_type = resp
        .headers()
        .get("content-type")
        .map(|v| v.to_str().unwrap().to_string())
        .unwrap_or_default();
    assert!(
        content_type.contains("text/css"),
        "style.css should have text/css content type, got: {content_type}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn home_page_contains_setup_checklist_on_fresh_storage() {
    setup();
    let env = TestEnv::new();
    let app = test_router(env.state.clone());

    let resp = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8(body.to_vec()).unwrap();

    assert!(
        html.contains("setup-checklist"),
        "home page on fresh storage should contain the setup checklist"
    );
}
