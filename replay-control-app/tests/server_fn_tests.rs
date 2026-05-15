#![cfg(feature = "ssr")]

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use replay_control_core_server::user_data_db::{ManualOrigin, UserDataDb};
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

fn form_body(params: &[(&str, &str)]) -> String {
    params
        .iter()
        .map(|(key, value)| format!("{key}={}", urlencoding::encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

fn manual_upload_body(boundary: &str, filename: &str, content_type: &str, content: &str) -> String {
    format!(
        "--{boundary}\r\n\
Content-Disposition: form-data; name=\"rom_filename\"\r\n\r\n\
TestGame.nes\r\n\
--{boundary}\r\n\
Content-Disposition: form-data; name=\"base_title\"\r\n\r\n\
testgame\r\n\
--{boundary}\r\n\
Content-Disposition: form-data; name=\"title\"\r\n\r\n\
Uploaded Manual\r\n\
--{boundary}\r\n\
Content-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\n\
Content-Type: {content_type}\r\n\r\n\
{content}\r\n\
--{boundary}--\r\n"
    )
}

async fn invoke_server_fn<F: ServerFn>(app: axum::Router, body: String) -> StatusCode {
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(F::PATH)
                .header("content-type", "application/x-www-form-urlencoded")
                .header("accept", "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    resp.status()
}

#[tokio::test(flavor = "multi_thread")]
async fn sfn_get_systems_returns_test_systems() {
    setup();
    let env = TestEnv::new().await;
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
    let env = TestEnv::new().await;
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
    let env = TestEnv::new().await;
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
async fn sfn_rescan_game_library_is_registered() {
    setup();
    let env = TestEnv::new().await;
    let app = test_router(env.state.clone());

    let path = <server_fns::RescanGameLibrary as ServerFn>::PATH;

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
        "RescanGameLibrary should be explicitly registered"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn sfn_nonexistent_function_returns_error() {
    setup();
    let env = TestEnv::new().await;
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
    let env = TestEnv::new().await;
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
    let env = TestEnv::new().await;
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

#[tokio::test(flavor = "multi_thread")]
async fn sfn_manual_download_delete_and_redownload_preserves_provider() {
    setup();
    let env = TestEnv::new().await;
    let app = test_router(env.state.clone());
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("GET", "/manual.pdf")
        .with_status(200)
        .with_header("content-type", "application/pdf")
        .with_body(b"%PDF-1.4\n% test manual\n")
        .expect(2)
        .create_async()
        .await;

    let url = format!("{}/manual.pdf", server.url());
    let system = "nintendo_nes";
    let rom_filename = "TestGame.nes";
    let base_title = "testgame";

    for attempt in 0..2 {
        let status = invoke_server_fn::<server_fns::DownloadManual>(
            app.clone(),
            form_body(&[
                ("system", system),
                ("rom_filename", rom_filename),
                ("base_title", base_title),
                ("url", &url),
                ("language", "en"),
                ("title", "Test Manual"),
                ("source", "retrokit"),
            ]),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "download attempt {attempt} should succeed"
        );

        assert!(
            env.tmp
                .join(".replay-control/manuals")
                .join(system)
                .exists(),
            "owned manuals directory should exist"
        );

        let rows = env
            .state
            .user_data_reader
            .read({
                let system = system.to_string();
                let base_title = base_title.to_string();
                move |conn| UserDataDb::get_game_manuals(conn, &system, &[&base_title]).unwrap()
            })
            .await
            .expect("user data read should run");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title.as_deref(), Some("Test Manual"));
        assert_eq!(rows[0].origin, ManualOrigin::Downloaded);
        assert_eq!(rows[0].provider.as_deref(), Some("retrokit"));
        assert_eq!(rows[0].url.as_deref(), Some(url.as_str()));
        assert_eq!(rows[0].languages, "en");

        let storage_path = rows[0]
            .storage_path
            .as_deref()
            .expect("downloaded manual should have a storage path");
        let manual_path = env.tmp.join(".replay-control/manuals").join(storage_path);
        assert!(manual_path.exists(), "owned manual file should exist");

        let delete_id = rows[0].manual_id.clone();
        let status = invoke_server_fn::<server_fns::DeleteManual>(
            app.clone(),
            form_body(&[("system", system), ("manual_id", &delete_id)]),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "delete should succeed");

        let rows = env
            .state
            .user_data_reader
            .read({
                let system = system.to_string();
                let base_title = base_title.to_string();
                move |conn| UserDataDb::get_game_manuals(conn, &system, &[&base_title]).unwrap()
            })
            .await
            .expect("user data read should run");
        assert!(rows.is_empty(), "manual row should disappear after delete");
        assert!(!manual_path.exists(), "owned manual file should be deleted");
        let files_left = std::fs::read_dir(env.tmp.join(".replay-control/manuals").join(system))
            .map(|entries| entries.count())
            .unwrap_or(0);
        assert_eq!(files_left, 0, "owned manual file should be deleted");
    }

    _mock.assert_async().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn api_manual_upload_save_and_delete_uses_upload_origin() {
    setup();
    let env = TestEnv::new().await;
    let app = test_router(env.state.clone());
    let boundary = "replay-test-boundary";
    let body = manual_upload_body(boundary, "manual.txt", "text/plain", "manual text");

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/manuals/upload/nintendo_nes")
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "manual upload should succeed"
    );

    let rows = env
        .state
        .user_data_reader
        .read(|conn| UserDataDb::get_game_manuals(conn, "nintendo_nes", &["testgame"]).unwrap())
        .await
        .expect("user data read should run");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].title.as_deref(), Some("Uploaded Manual"));
    assert_eq!(rows[0].origin, ManualOrigin::Upload);
    assert_eq!(rows[0].provider.as_deref(), Some("user_upload"));
    assert!(rows[0].url.is_none(), "uploaded manuals must not have URL");
    assert_eq!(rows[0].mime_type, "text/plain");

    let storage_path = rows[0]
        .storage_path
        .as_deref()
        .expect("uploaded manual should have a storage path");
    let manual_path = env.tmp.join(".replay-control/manuals").join(storage_path);
    assert!(manual_path.exists(), "uploaded manual file should exist");

    let status = invoke_server_fn::<server_fns::DeleteManual>(
        app,
        form_body(&[
            ("system", "nintendo_nes"),
            ("manual_id", &rows[0].manual_id),
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "delete should succeed");
    assert!(
        !manual_path.exists(),
        "uploaded manual file should be deleted"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn api_manual_upload_rejects_disallowed_extensions() {
    setup();
    let env = TestEnv::new().await;
    let app = test_router(env.state.clone());
    let boundary = "replay-test-boundary";
    let body = manual_upload_body(boundary, "manual.html", "text/html", "<p>manual text</p>");

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/manuals/upload/nintendo_nes")
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "manual upload should reject non-PDF/non-text filenames"
    );

    let rows = env
        .state
        .user_data_reader
        .read(|conn| UserDataDb::get_game_manuals(conn, "nintendo_nes", &["testgame"]).unwrap())
        .await
        .expect("user data read should run");
    assert!(rows.is_empty(), "rejected uploads must not create rows");
}
