#![cfg(feature = "ssr")]

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::routing::{get, post, put};
use http_body_util::BodyExt;
use replay_control_app::server_fns;
use replay_control_core::auth::AuthRole;
use replay_control_core::runtime_env::Mode;
use replay_control_core_server::auth::AuthStore;
use replay_control_core_server::settings::{write_first_setup_done, write_replay_api_token};
use server_fn::ServerFn;
use tower::ServiceExt;

use common::{TestEnv, init_executor, register_server_fns, test_guarded_router, test_router};
use replay_control_app::api::{StorageStatus, with_auth_guard};

/// SSR tests require the Leptos executor and server function registration.
fn setup() {
    init_executor();
    register_server_fns();
}

fn mark_first_setup_done(state: &replay_control_app::api::AppState) {
    write_first_setup_done(&state.settings, true).unwrap();
    state
        .prefs
        .write()
        .expect("prefs lock poisoned")
        .first_setup_done = true;
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
async fn game_detail_page_renders_info_table_rows() {
    setup();
    let env = TestEnv::new().await;
    let app = test_router(env.state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/games/nintendo_nes/TestGame.nes")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8(body.to_vec()).unwrap();

    assert!(html.contains("game-info-table"));
    assert!(html.contains("game-info-row"));
    assert!(html.contains("TestGame.nes"));
}

#[tokio::test(flavor = "multi_thread")]
async fn login_page_renders_net_control_code_instructions() {
    setup();
    let mut env = TestEnv::new().await;
    env.state.mode = Mode::Device;
    let app = test_router(env.state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/login")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8(body.to_vec()).unwrap();

    assert!(html.contains("Net Control code"));
    assert!(html.contains("SYSTEM &gt; OPTIONS"));
    assert!(html.contains("SYSTEM &gt; INFORMATION"));
    assert!(html.contains("Device password"));
    assert!(
        html.contains(r#"id="login-net-control-code""#)
            && html.contains(r#"type="text""#)
            && html.contains(r#"inputmode="numeric""#)
            && html.contains(r#"maxlength="6""#),
        "Net Control code input should use mobile numeric text entry"
    );
    assert!(
        html.contains("login-standalone"),
        "login page should render as a standalone page"
    );
    assert!(
        !html.contains("top-bar"),
        "login page should not render the main app header"
    );
    assert!(
        !html.contains("bottom-nav"),
        "login page should not render the main app footer navigation"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn login_page_in_standalone_mode_skips_credentials() {
    setup();
    let env = TestEnv::new().await;
    let app = test_router(env.state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/login")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8(body.to_vec()).unwrap();

    assert!(html.contains("Open standalone mode"));
    assert!(!html.contains("login-net-control-code"));
    assert!(!html.contains("login-admin-password"));
}

#[tokio::test(flavor = "multi_thread")]
async fn first_setup_page_renders_device_password_guidance() {
    setup();
    let mut env = TestEnv::new().await;
    env.state.mode = Mode::Device;
    let app = test_router(env.state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/first-setup")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8(body.to_vec()).unwrap();

    assert!(html.contains("First setup"));
    assert!(html.contains("default password is replayos"));
    assert!(html.contains(r#"id="first-setup-password""#));
    assert!(
        html.contains("login-standalone"),
        "first setup page should render as a standalone page"
    );
    assert!(
        !html.contains("top-bar"),
        "first setup page should not render the main app header"
    );
    assert!(
        !html.contains("bottom-nav"),
        "first setup page should not render the main app footer navigation"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn access_security_page_returns_200() {
    setup();
    let env = TestEnv::new().await;
    let app = test_router(env.state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/settings/access")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread")]
async fn access_security_page_in_standalone_mode_skips_session_controls() {
    setup();
    let env = TestEnv::new().await;
    let app = test_router(env.state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/settings/access")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8(body.to_vec()).unwrap();

    assert!(html.contains("Open access"));
    assert!(html.contains("Open standalone mode"));
    assert!(!html.contains("access-admin-password"));
    assert!(!html.contains("Sign out"));
}

#[tokio::test(flavor = "multi_thread")]
async fn settings_page_authenticated_user_links_to_access_without_sign_in_item() {
    setup();
    let mut env = TestEnv::new().await;
    env.state.mode = Mode::Device;
    mark_first_setup_done(&env.state);
    write_replay_api_token(&env.state.settings, "123456").unwrap();
    let session = env
        .state
        .auth
        .store
        .create_user_session(&env.state.settings)
        .unwrap();
    let app = test_router(env.state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/settings")
                .header(
                    header::COOKIE,
                    format!("ReplayControlSession={}", session.token),
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8(body.to_vec()).unwrap();

    assert!(html.contains("Access &amp; Security"));
    assert!(!html.contains(">Sign in<"));
    assert!(html.contains("Region Preferences"));
    assert!(html.contains(">Language<"));
    assert!(html.contains("Sign in as admin to change these settings."));
    assert!(html.matches("disabled").count() >= 4);
    assert!(!html.contains("href=\"/settings/game-library\""));
    assert!(!html.contains("href=\"/settings/replayos\""));
    assert!(!html.contains("href=\"/settings/logs\""));
}

#[tokio::test(flavor = "multi_thread")]
async fn authenticated_login_request_redirects_to_home() {
    setup();
    let mut env = TestEnv::new().await;
    env.state.mode = Mode::Device;
    mark_first_setup_done(&env.state);
    write_replay_api_token(&env.state.settings, "123456").unwrap();
    let session = env
        .state
        .auth
        .store
        .create_user_session(&env.state.settings)
        .unwrap();
    let app = with_auth_guard(test_router(env.state.clone()), env.state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/login")
                .header(header::ACCEPT, "text/html")
                .header(header::COOKIE, session_cookie(&session.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(resp.headers().get(header::LOCATION).unwrap(), "/");
}

#[tokio::test(flavor = "multi_thread")]
async fn login_page_authenticated_component_state_does_not_render_login_ui() {
    setup();
    let mut env = TestEnv::new().await;
    env.state.mode = Mode::Device;
    mark_first_setup_done(&env.state);
    write_replay_api_token(&env.state.settings, "123456").unwrap();
    let session = env
        .state
        .auth
        .store
        .create_user_session(&env.state.settings)
        .unwrap();
    let app = test_router(env.state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/login")
                .header(header::COOKIE, session_cookie(&session.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8(body.to_vec()).unwrap();

    assert!(html.contains("Replay Control"));
    assert!(!html.contains("Current access"));
    assert!(!html.contains("Normal user"));
    assert!(!html.contains("Access &amp; Security"));
    assert!(!html.contains("login-net-control-code"));
    assert!(!html.contains("login-admin-password"));
}

#[tokio::test(flavor = "multi_thread")]
async fn access_security_requires_auth_only_in_device_mode() {
    setup();
    let mut env = TestEnv::new().await;
    env.state.mode = Mode::Device;
    mark_first_setup_done(&env.state);
    write_replay_api_token(&env.state.settings, "123456").unwrap();
    let session = env
        .state
        .auth
        .store
        .create_user_session(&env.state.settings)
        .unwrap();
    let app = with_auth_guard(test_router(env.state.clone()), env.state.clone());

    let anonymous = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/settings/access")
                .header(header::ACCEPT, "text/html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(anonymous.status(), StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(
        anonymous.headers().get(header::LOCATION).unwrap(),
        "/login?next=%2Fsettings%2Faccess"
    );

    let authenticated = app
        .oneshot(
            Request::builder()
                .uri("/settings/access")
                .header(header::ACCEPT, "text/html")
                .header(header::COOKIE, session_cookie(&session.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(authenticated.status(), StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread")]
async fn first_setup_pending_redirects_login_and_blocks_other_auth_bootstrap() {
    setup();
    let mut env = TestEnv::new().await;
    env.state.mode = Mode::Device;
    let app = with_auth_guard(auth_probe_router(), env.state.clone());

    let home = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/")
                .header(header::ACCEPT, "text/html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(home.status(), StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(
        home.headers().get(header::LOCATION).unwrap(),
        "/first-setup"
    );

    let login = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/login")
                .header(header::ACCEPT, "text/html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(login.status(), StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(
        login.headers().get(header::LOCATION).unwrap(),
        "/first-setup"
    );

    let app_page = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/settings/wifi")
                .header(header::ACCEPT, "text/html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(app_page.status(), StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(
        app_page.headers().get(header::LOCATION).unwrap(),
        "/first-setup"
    );

    let first_setup = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/first-setup")
                .header(header::ACCEPT, "text/html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(first_setup.status(), StatusCode::OK);

    let normal_login = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/sfn/login_admin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(normal_login.status(), StatusCode::UNAUTHORIZED);

    let allowed_completion = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/sfn/complete_first_setup")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(allowed_completion.status(), StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread")]
async fn access_security_page_normal_user_renders_inline_admin_unlock_form() {
    setup();
    let mut env = TestEnv::new().await;
    env.state.mode = Mode::Device;
    write_replay_api_token(&env.state.settings, "123456").unwrap();
    let session = env
        .state
        .auth
        .store
        .create_user_session(&env.state.settings)
        .unwrap();
    let app = test_router(env.state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/settings/access")
                .header(
                    header::COOKIE,
                    format!("ReplayControlSession={}", session.token),
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8(body.to_vec()).unwrap();

    assert!(html.contains("<form"));
    assert!(html.contains("access-inline-admin"));
    assert!(html.contains("access-admin-password"));
    assert!(html.contains("Admin access"));
    assert!(html.contains("SHA-256 fingerprint"));
    // The regenerate button is now always rendered, disabled for non-admins.
    assert!(html.contains("Regenerate certificate"));
}

#[tokio::test(flavor = "multi_thread")]
async fn access_security_page_direct_admin_shows_disabled_downgrade() {
    setup();
    let mut env = TestEnv::new().await;
    env.state.mode = Mode::Device;
    write_replay_api_token(&env.state.settings, "123456").unwrap();
    let shadow_path = env.tmp.join("shadow");
    std::fs::write(&shadow_path, "root:$y$j9T$testhash:19793:0:99999:7:::\n").unwrap();
    env.state.auth.store = AuthStore::open_at_with_shadow(
        env.tmp.join(".replay-control-data/auth-cookie.key"),
        &shadow_path,
    )
    .unwrap();
    let session = env
        .state
        .auth
        .store
        .create_admin_session(None, &env.state.settings)
        .unwrap();
    let app = test_router(env.state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/settings/access")
                .header(header::COOKIE, session_cookie(&session.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8(body.to_vec()).unwrap();

    assert!(html.contains("Admin"));
    assert!(html.contains("Sign out"));
    assert!(html.contains("Sign out all sessions"));
    // A direct admin has no user session to return to, so the downgrade control
    // is shown but disabled, with a hint explaining when it is available.
    assert!(html.contains("Switch to normal user"));
    assert!(html.contains("Available only when you sign in with the access code"));
}

#[tokio::test(flavor = "multi_thread")]
async fn access_security_page_elevated_admin_offers_downgrade() {
    setup();
    let mut env = TestEnv::new().await;
    env.state.mode = Mode::Device;
    write_replay_api_token(&env.state.settings, "123456").unwrap();
    let shadow_path = env.tmp.join("shadow");
    std::fs::write(&shadow_path, "root:$y$j9T$testhash:19793:0:99999:7:::\n").unwrap();
    env.state.auth.store = AuthStore::open_at_with_shadow(
        env.tmp.join(".replay-control-data/auth-cookie.key"),
        &shadow_path,
    )
    .unwrap();
    let session = env
        .state
        .auth
        .store
        .create_admin_session(Some(AuthRole::User), &env.state.settings)
        .unwrap();
    let app = test_router(env.state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/settings/access")
                .header(header::COOKIE, session_cookie(&session.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8(body.to_vec()).unwrap();

    assert!(html.contains("Switch to normal user"));
    assert!(html.contains("Sign out all sessions"));
}

#[tokio::test(flavor = "multi_thread")]
async fn retroachievements_settings_page_returns_200() {
    setup();
    let env = TestEnv::new().await;
    let app = test_router(env.state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/settings/retroachievements")
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

#[tokio::test(flavor = "multi_thread")]
async fn auth_bootstrap_survives_storage_guard_but_waiting_reboot_requires_auth() {
    setup();
    let mut env = TestEnv::new().await;
    env.state.mode = Mode::Device;
    mark_first_setup_done(&env.state);
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
    let app = with_auth_guard(test_guarded_router(env.state.clone()), env.state.clone());

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/login")
                .header(header::ACCEPT, "text/html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = app
        .clone()
        .oneshot(server_fn_request::<server_fns::GetAuthStatus>())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/waiting/reboot")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// Once storage comes back (USB inserted, NFS configured, etc.), the
/// /waiting page's meta-refresh re-hits the handler. The handler must
/// redirect to / so the user escapes the waiting page — /waiting is
/// plain server-rendered HTML with no Leptos hydration, so the
/// SSE-driven reload listener doesn't run there.
#[tokio::test(flavor = "multi_thread")]
async fn waiting_page_redirects_to_root_when_storage_is_available() {
    setup();
    let env = TestEnv::new().await;
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

#[tokio::test(flavor = "multi_thread")]
async fn auth_guard_does_not_require_auth_in_standalone_mode() {
    let env = TestEnv::new().await;
    assert!(matches!(env.state.mode, Mode::Standalone { .. }));
    let app = with_auth_guard(auth_probe_router(), env.state.clone());

    let resp = app
        .oneshot(
            same_origin_request("PUT", "/api/roms/rename")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread")]
async fn standalone_auth_status_ignores_stale_admin_cookie() {
    setup();
    let mut env = TestEnv::new().await;
    assert!(matches!(env.state.mode, Mode::Standalone { .. }));
    let shadow_path = env.tmp.join("shadow");
    std::fs::write(&shadow_path, "root:$y$j9T$testhash:19793:0:99999:7:::\n").unwrap();
    env.state.auth.store = AuthStore::open_at_with_shadow(
        env.tmp.join(".replay-control-data/auth-cookie.key"),
        &shadow_path,
    )
    .unwrap();
    let admin = env
        .state
        .auth
        .store
        .create_admin_session(None, &env.state.settings)
        .unwrap();
    env.state.auth.store = AuthStore::open_at_with_shadow(
        env.tmp.join(".replay-control-data/auth-cookie.key"),
        env.tmp.join("missing-shadow"),
    )
    .unwrap();
    let app = test_router(env.state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(server_fns::GetAuthStatus::PATH)
                .header("content-type", "application/x-www-form-urlencoded")
                .header("accept", "application/x-www-form-urlencoded")
                .header(header::COOKIE, session_cookie(&admin.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let clear_cookies = resp
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter(|value| value.starts_with("ReplayControlSession=") && value.contains("Max-Age=0"))
        .collect::<Vec<_>>();
    assert!(
        clear_cookies.iter().any(|value| value.contains(" Secure;")),
        "standalone auth status should expire stale secure device session cookies"
    );
    assert!(
        clear_cookies
            .iter()
            .any(|value| !value.contains(" Secure;")),
        "standalone auth status should expire stale insecure debug session cookies"
    );
    assert!(
        clear_cookies
            .iter()
            .all(|value| value.contains("Expires=Thu, 01 Jan 1970 00:00:00 GMT")),
        "standalone auth status should expire stale device session cookies"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn live_stats_and_library_playtime_server_functions_are_registered() {
    setup();
    let env = TestEnv::new().await;
    let app = test_router(env.state.clone());

    let resp = app
        .clone()
        .oneshot(server_fn_request::<server_fns::GetLiveStats>())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = app
        .oneshot(server_fn_request::<server_fns::GetLibraryPlaytime>())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread")]
async fn auth_bootstrap_ignores_invalid_cookie_for_csrf() {
    setup();
    let mut env = TestEnv::new().await;
    env.state.mode = Mode::Device;
    let app = with_auth_guard(test_router(env.state.clone()), env.state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(server_fns::GetAuthStatus::PATH)
                .header("content-type", "application/x-www-form-urlencoded")
                .header("accept", "application/x-www-form-urlencoded")
                .header(header::COOKIE, "ReplayControlSession=stale.invalid")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread")]
async fn auth_guard_requires_csrf_for_valid_session_mutations() {
    let mut env = TestEnv::new().await;
    env.state.mode = Mode::Device;
    mark_first_setup_done(&env.state);
    write_replay_api_token(&env.state.settings, "123456").unwrap();
    let user = env
        .state
        .auth
        .store
        .create_user_session(&env.state.settings)
        .unwrap();
    let app = with_auth_guard(auth_probe_router(), env.state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/sfn/add_favorite")
                .header(header::COOKIE, session_cookie(&user.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test(flavor = "multi_thread")]
async fn auth_guard_csrf_protects_authenticated_bootstrap_actions() {
    let mut env = TestEnv::new().await;
    env.state.mode = Mode::Device;
    mark_first_setup_done(&env.state);
    write_replay_api_token(&env.state.settings, "123456").unwrap();
    let user = env
        .state
        .auth
        .store
        .create_user_session(&env.state.settings)
        .unwrap();
    let app = with_auth_guard(auth_probe_router(), env.state.clone());

    let anonymous_login = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/sfn/login_admin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(anonymous_login.status(), StatusCode::OK);

    let authenticated_cross_site = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/sfn/login_admin")
                .header(header::COOKIE, session_cookie(&user.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(authenticated_cross_site.status(), StatusCode::FORBIDDEN);

    let authenticated_same_origin = app
        .oneshot(
            same_origin_request("POST", "/sfn/login_admin")
                .header(header::COOKIE, session_cookie(&user.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(authenticated_same_origin.status(), StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread")]
async fn auth_guard_enforces_device_mode_default_roles() {
    let mut env = TestEnv::new().await;
    env.state.mode = Mode::Device;
    mark_first_setup_done(&env.state);
    write_replay_api_token(&env.state.settings, "123456").unwrap();
    let shadow_path = env.tmp.join("shadow");
    std::fs::write(&shadow_path, "root:$y$j9T$testhash:19793:0:99999:7:::\n").unwrap();
    env.state.auth.store = AuthStore::open_at_with_shadow(
        env.tmp.join(".replay-control-data/auth-cookie.key"),
        shadow_path,
    )
    .unwrap();
    let user = env
        .state
        .auth
        .store
        .create_user_session(&env.state.settings)
        .unwrap();
    let admin = env
        .state
        .auth
        .store
        .create_admin_session(None, &env.state.settings)
        .unwrap();
    let app = with_auth_guard(auth_probe_router(), env.state.clone());

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/settings/wifi")
                .header(header::ACCEPT, "text/html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(
        resp.headers().get(header::LOCATION).unwrap(),
        "/login?next=%2Fsettings%2Fwifi"
    );

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/sfn/save_wifi_config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/settings/wifi")
                .header(header::COOKIE, session_cookie(&user.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(
        resp.headers().get(header::LOCATION).unwrap(),
        "/settings/access?next=%2Fsettings%2Fwifi"
    );

    let resp = app
        .clone()
        .oneshot(
            same_origin_request("PUT", "/api/roms/rename")
                .header(header::COOKIE, session_cookie(&user.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    let resp = app
        .oneshot(
            same_origin_request("PUT", "/api/roms/rename")
                .header(header::COOKIE, session_cookie(&admin.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread")]
async fn auth_guard_uses_real_server_function_paths_for_bootstrap() {
    setup();
    let mut env = TestEnv::new().await;
    env.state.mode = Mode::Device;
    mark_first_setup_done(&env.state);
    let app = with_auth_guard(test_router(env.state.clone()), env.state.clone());

    let resp = app
        .clone()
        .oneshot(server_fn_request::<server_fns::GetAuthStatus>())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = app
        .oneshot(server_fn_request::<server_fns::SaveWifiConfig>())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

fn auth_probe_router() -> axum::Router {
    axum::Router::new()
        .route("/first-setup", get(|| async { "first setup" }))
        .route("/settings/wifi", get(|| async { "admin page" }))
        .route("/games/nintendo_nes", get(|| async { "library page" }))
        .route("/sfn/login_admin", post(|| async { "login admin" }))
        .route(
            "/sfn/complete_first_setup",
            post(|| async { "complete first setup" }),
        )
        .route("/sfn/save_wifi_config", post(|| async { "saved" }))
        .route("/sfn/add_favorite", post(|| async { "favorited" }))
        .route("/api/roms/rename", put(|| async { "renamed" }))
        .route("/api/favorites", post(|| async { "favorited" }))
}

fn same_origin_request(method: &str, uri: &str) -> http::request::Builder {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, "replay.local:8443")
        .header(header::ORIGIN, "https://replay.local:8443")
}

fn session_cookie(token: &str) -> String {
    format!("ReplayControlSession={token}")
}

fn server_fn_request<F: ServerFn>() -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(F::PATH)
        .header("content-type", "application/x-www-form-urlencoded")
        .header("accept", "application/x-www-form-urlencoded")
        .body(Body::empty())
        .unwrap()
}
