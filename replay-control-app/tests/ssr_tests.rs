#![cfg(feature = "ssr")]

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use tower::ServiceExt;

use common::{TestEnv, init_executor, register_server_fns, test_guarded_router, test_router};
use replay_control_app::api::StorageStatus;

/// SSR tests require the Leptos executor and server function registration.
fn setup() {
    init_executor();
    register_server_fns();
}

#[tokio::test(flavor = "multi_thread")]
async fn home_page_returns_200_with_replay_control() {
    setup();
    let env = TestEnv::new().await;
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
    let env = TestEnv::new().await;
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
    let env = TestEnv::new().await;
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
    let env = TestEnv::new().await;
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
    let env = TestEnv::new().await;
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

#[tokio::test(flavor = "multi_thread")]
async fn storage_guard_redirects_to_waiting_page_with_error_reboot_action() {
    setup();
    let env = TestEnv::new().await;
    {
        let mut storage = env.state.storage.write().expect("storage lock poisoned");
        *storage = None;
    }
    {
        let mut status = env
            .state
            .storage_status
            .write()
            .expect("storage status lock poisoned");
        *status = StorageStatus::Error {
            message: "Could not re-open library_db DB: closed".into(),
        };
    }
    let app = test_guarded_router(env.state.clone());

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/settings/metadata")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(
        resp.headers().get(header::LOCATION).unwrap(),
        "/waiting",
        "guard should redirect app routes to the waiting page when storage is unavailable"
    );

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/waiting")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8(body.to_vec()).unwrap();

    assert!(html.contains("Storage was detected, but Replay Control could not open its database."));
    assert!(html.contains("Could not re-open library_db DB: closed"));
    assert!(html.contains(r#"action="/waiting/reboot""#));
    assert!(html.contains("Reboot System"));
}

/// Once storage comes back (USB inserted, NFS configured, etc.), the
/// /waiting page's `<meta http-equiv="refresh">` re-hits the handler.
/// The handler must redirect to / so the user escapes the waiting page —
/// /waiting is plain server-rendered HTML with no Leptos hydration, so
/// the SSE-driven reload listener doesn't run there. Without this
/// redirect, users stay stuck on /waiting indefinitely.
#[tokio::test(flavor = "multi_thread")]
async fn waiting_page_redirects_to_root_when_storage_is_available() {
    setup();
    let env = TestEnv::new().await; // storage = Some by default
    let app = test_guarded_router(env.state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/waiting")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(resp.headers().get(header::LOCATION).unwrap(), "/");
}
