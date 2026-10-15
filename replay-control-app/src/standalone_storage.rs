//! Standalone package storage selection. The same small server-rendered page is
//! served before the library exists and from Settings after normal startup.
//! RePlayOS never mounts these routes.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ashpd::desktop::ResponseError;
use ashpd::desktop::file_chooser::SelectedFiles;
use ashpd::{Error as AshpdError, PortalError};
use axum::extract::{ConnectInfo, Query, State};
use axum::http::Request;
use axum::http::{HeaderMap, StatusCode, header::HOST};
use axum::middleware::Next;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_server::tls_rustls::RustlsConfig;
use replay_control_app::i18n::{Key, resolve_locale_from_accept_language, t};
use replay_control_core::locale::Locale;
use replay_control_core_server::data_dir::DataDir;
use replay_control_core_server::security::tls::ensure_self_signed_certificate;
use replay_control_core_server::settings::{SettingsStore, read_locale};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use url::Url;

const SWITCH_HELPER: &str = "/usr/lib/replay-control/switch-storage";
static SWITCH_REQUEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static RESTART_REQUESTED: tokio::sync::Notify = tokio::sync::Notify::const_new();

fn data_root() -> PathBuf {
    std::env::var_os("REPLAY_CONTROL_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/var/lib/replay-control"))
}

fn data_file(name: &str) -> PathBuf {
    data_root().join(name)
}

pub fn portable() -> bool {
    std::env::var_os("REPLAY_CONTROL_PORTABLE").is_some()
}

pub async fn wait_for_restart() {
    RESTART_REQUESTED.notified().await;
}

pub fn finish_portable_restart() {
    if portable()
        && std::fs::read_to_string(data_file("storage-switch-status"))
            .is_ok_and(|status| status.starts_with("running\n"))
    {
        let _ = std::fs::write(data_file("storage-switch-status"), "ok\n");
        let _ = std::fs::remove_file(data_file("restart-requested"));
    }
}

fn fail_portable_restart(storage_error: &str) {
    if portable()
        && std::fs::read_to_string(data_file("storage-switch-status"))
            .is_ok_and(|status| status.starts_with("running\n"))
    {
        let _ = std::fs::write(
            data_file("storage-switch-status"),
            format!("error\nSelected storage is unavailable: {storage_error}\n"),
        );
        let _ = std::fs::remove_file(data_file("restart-requested"));
    }
}

#[derive(Clone)]
struct SetupState {
    current: Option<String>,
    storage_error: Option<String>,
}

#[derive(Serialize)]
struct DirectoryInfo {
    path: String,
    parent: Option<String>,
    directories: Vec<String>,
    folders: Vec<String>,
    valid: bool,
    error: Option<String>,
}

#[derive(Deserialize)]
struct DirectoryQuery {
    path: Option<String>,
}

#[derive(Deserialize)]
struct SelectionRequest {
    path: String,
}

#[derive(Serialize)]
struct SelectionResponse {
    current: Option<String>,
    status: String,
    message: Option<String>,
    storage_error: Option<String>,
}

/// A browser choice, when present, wins over the optional service environment.
pub fn resolve_storage_path(packaged: bool, preconfigured: Option<String>) -> Option<String> {
    if !packaged {
        return preconfigured;
    }
    let saved = std::fs::read_to_string(data_file("standalone-storage-path"))
        .ok()
        .and_then(|content| parse_saved_path(&content));
    saved.or(preconfigured)
}

fn parse_saved_path(content: &str) -> Option<String> {
    let path = content.trim_end_matches('\n');
    (Path::new(path).is_absolute() && !path.contains(['\n', '\r', '\0'])).then(|| path.to_string())
}

/// Check an existing selection before AppState opens any storage-scoped files.
/// A late or read-only mount fails startup instead of opening its mountpoint
/// as an empty library; the service's restart policy retries when it returns.
pub fn check_configured_storage(path: &str) -> Result<(), String> {
    validate_selection(path).map(|_| ())
}

pub fn router(current: Option<String>) -> Router {
    router_with_state(SetupState {
        current,
        storage_error: None,
    })
}

fn router_with_state(state: SetupState) -> Router {
    Router::new()
        .route("/storage-setup", get(page))
        .route("/storage-setup/app.js", get(script))
        .route("/storage-setup/app.css", get(stylesheet))
        .route("/storage-setup/messages", get(messages))
        .route("/storage-setup/list", get(list_directory))
        .route("/storage-setup/pick", post(pick_directory))
        .route("/storage-setup/select", post(select_directory))
        .route("/storage-setup/status", get(selection_status))
        .route("/storage-setup/active-root", get(active_root))
        .layer(axum::middleware::from_fn(require_loopback))
        .with_state(state)
}

async fn require_loopback(request: Request<axum::body::Body>, next: Next) -> Response {
    if request_is_loopback(&request) {
        next.run(request).await
    } else {
        (
            StatusCode::FORBIDDEN,
            "Open this storage selector on the computer running Replay Control (localhost).",
        )
            .into_response()
    }
}

fn request_is_loopback(request: &Request<axum::body::Body>) -> bool {
    request
        .extensions()
        .get::<ConnectInfo<std::net::SocketAddr>>()
        .is_some_and(|ConnectInfo(address)| address.ip().is_loopback())
}

async fn page() -> Html<&'static str> {
    Html(include_str!("../static/storage-setup.html"))
}

fn storage_locale(headers: &HeaderMap) -> Locale {
    let settings_root = std::env::var_os("REPLAY_CONTROL_SETTINGS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/etc/replay-control"));
    read_locale(&SettingsStore::new(settings_root))
        .unwrap_or_else(|| resolve_locale_from_accept_language(headers))
}

async fn messages(headers: HeaderMap) -> Json<Value> {
    let locale = storage_locale(&headers);
    Json(json!({
        "locale": locale.code(),
        "title": t(locale, Key::StorageSetupTitle),
        "back": t(locale, Key::StorageSetupBack),
        "step": t(locale, Key::StorageSetupStep),
        "intro": t(locale, Key::StorageSetupIntro),
        "next": t(locale, Key::StorageSetupNext),
        "path_label": t(locale, Key::StorageSetupPathLabel),
        "browse": t(locale, Key::StorageSetupBrowse),
        "parent": t(locale, Key::StorageSetupParent),
        "select": t(locale, Key::StorageSetupSelect),
        "current": t(locale, Key::StorageSetupCurrent),
        "none": t(locale, Key::StorageSetupNone),
        "unavailable": t(locale, Key::StorageSetupUnavailable),
        "found": t(locale, Key::StorageSetupFound),
        "invalid": t(locale, Key::StorageSetupInvalid),
        "browse_failed": t(locale, Key::StorageSetupBrowseFailed),
        "switching": t(locale, Key::StorageSetupSwitching),
        "switch_timeout": t(locale, Key::StorageSetupSwitchTimeout),
        "switch_failed": t(locale, Key::StorageSetupSwitchFailed),
        "could_not_switch": t(locale, Key::StorageSetupCouldNotSwitch),
        "select_directory": t(locale, Key::StorageSetupSelectDirectory),
        "already_selected": t(locale, Key::StorageSetupAlreadySelected),
        "invalid_path": t(locale, Key::StorageSetupInvalidPath),
        "switch_busy": t(locale, Key::StorageSetupSwitchBusy),
        "validation_required": t(locale, Key::StorageSetupValidationRequired),
        "switch_restored": t(locale, Key::StorageSetupSwitchRestored),
        "switch_interrupted": t(locale, Key::StorageSetupSwitchInterrupted),
        "switch_start_failed": t(locale, Key::StorageSetupSwitchStartFailed),
        "package_only": t(locale, Key::StorageSetupPackageOnly),
        "portable": portable(),
    }))
}

async fn script() -> impl IntoResponse {
    (
        [("content-type", "application/javascript; charset=utf-8")],
        include_str!("../static/storage-setup.js"),
    )
}

async fn stylesheet() -> impl IntoResponse {
    (
        [("content-type", "text/css; charset=utf-8")],
        include_str!("../static/storage-setup.css"),
    )
}

fn inspect_directory(input: &str) -> Result<DirectoryInfo, String> {
    let requested = if input.trim().is_empty() {
        "/"
    } else {
        input.trim()
    };
    let path = std::fs::canonicalize(requested)
        .map_err(|error| format!("Cannot open {requested}: {error}"))?;
    if !path.is_dir() {
        return Err("Select a directory.".to_string());
    }
    let mut directories = Vec::new();
    let entries = std::fs::read_dir(&path)
        .map_err(|error| format!("Cannot read {}: {error}", path.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        if entry.path().is_dir() {
            directories.push(entry.file_name().to_string_lossy().to_string());
        }
    }
    directories.sort_unstable_by_key(|name| name.to_lowercase());
    directories.truncate(200);
    let folders = ["roms", "bios", "saves", "captures", "config", "skins"]
        .into_iter()
        .filter(|folder| path.join(folder).is_dir())
        .map(str::to_string)
        .collect::<Vec<_>>();
    let valid = path.join("roms").is_dir() && std::fs::read_dir(path.join("roms")).is_ok();
    Ok(DirectoryInfo {
        path: path.to_string_lossy().to_string(),
        parent: path
            .parent()
            .map(|parent| parent.to_string_lossy().to_string()),
        directories,
        folders,
        valid,
        error: None,
    })
}

async fn list_directory(Query(query): Query<DirectoryQuery>) -> Response {
    if portable() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let requested = query.path.unwrap_or_else(|| "/".to_string());
    let inspected = tokio::task::spawn_blocking({
        let requested = requested.clone();
        move || inspect_directory(&requested)
    })
    .await
    .unwrap_or_else(|error| Err(format!("Folder inspection failed: {error}")));
    Json(inspected.unwrap_or_else(|error| DirectoryInfo {
        path: requested,
        parent: None,
        directories: Vec::new(),
        folders: Vec::new(),
        valid: false,
        error: Some(error),
    }))
    .into_response()
}

async fn pick_directory(headers: HeaderMap) -> Response {
    if !portable() {
        return StatusCode::NOT_FOUND.into_response();
    }
    if headers
        .get("x-replay-control-portal")
        .is_none_or(|value| value != "pick-storage")
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let request = match SelectedFiles::open_file()
        .title(t(storage_locale(&headers), Key::StorageSetupTitle))
        .directory(true)
        .send()
        .await
    {
        Ok(request) => request,
        Err(error) => {
            return (
                StatusCode::BAD_GATEWAY,
                format!("Cannot open folder chooser: {error}"),
            )
                .into_response();
        }
    };
    let selection = match request.response() {
        Ok(selection) => selection,
        Err(AshpdError::Response(ResponseError::Cancelled))
        | Err(AshpdError::Portal(PortalError::Cancelled(_))) => {
            return StatusCode::NO_CONTENT.into_response();
        }
        Err(error) => {
            return (
                StatusCode::BAD_GATEWAY,
                format!("Folder chooser failed: {error}"),
            )
                .into_response();
        }
    };
    let Some(uri) = selection.uris().first() else {
        return StatusCode::NO_CONTENT.into_response();
    };
    let path = match portal_uri_path(uri.as_str()) {
        Some(path) => path,
        None => {
            return (
                StatusCode::BAD_GATEWAY,
                "Folder chooser returned an invalid path.",
            )
                .into_response();
        }
    };
    let info =
        tokio::task::spawn_blocking(move || inspect_directory(&path.to_string_lossy())).await;
    match info {
        Ok(Ok(info)) => Json(info).into_response(),
        Ok(Err(error)) => (StatusCode::BAD_REQUEST, error).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

fn portal_uri_path(uri: &str) -> Option<PathBuf> {
    Url::parse(uri).ok()?.to_file_path().ok()
}

fn validate_selection(input: &str) -> Result<PathBuf, String> {
    if input.contains(['\n', '\r', '\0']) {
        return Err("The path contains an invalid character.".to_string());
    }
    let info = inspect_directory(input)?;
    if !info.valid {
        return Err("This folder must contain a readable roms/ directory.".to_string());
    }
    let root = PathBuf::from(info.path);
    // Probe the filesystem used by the app data, before changing the running
    // service. A read-only mount is actionable here instead of after restart.
    let probe = root.join(format!(
        ".replay-control-write-probe-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .map_err(|error| format!("This storage root is not writable: {error}"))?;
    std::fs::remove_file(&probe)
        .map_err(|error| format!("Cannot remove storage write probe: {error}"))?;
    Ok(root)
}

async fn select_directory(
    State(state): State<SetupState>,
    Json(request): Json<SelectionRequest>,
) -> Response {
    let _request_guard = SWITCH_REQUEST_LOCK.lock().await;
    if switch_is_running() {
        return (
            StatusCode::CONFLICT,
            "A storage switch is already in progress. Try again after it finishes.",
        )
            .into_response();
    }
    let root = match tokio::task::spawn_blocking(move || validate_selection(&request.path)).await {
        Ok(Ok(root)) => root,
        Ok(Err(message)) => return (StatusCode::BAD_REQUEST, message).into_response(),
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Folder validation failed: {error}"),
            )
                .into_response();
        }
    };
    if !portable() && !Path::new(SWITCH_HELPER).is_file() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "Storage switching is available in packaged installs.",
        )
            .into_response();
    }
    if state.current.as_deref() == root.to_str() {
        return (StatusCode::OK, "This folder is already selected.").into_response();
    }
    if let Err(error) = std::fs::write(data_file("storage-switch-status"), "running\n") {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Cannot save switch status: {error}"),
        )
            .into_response();
    }
    if portable() {
        let choice = data_file("standalone-storage-path");
        let pending = data_file("standalone-storage-path.next");
        if let Err(error) = std::fs::write(data_file("restart-requested"), "1\n") {
            let _ = std::fs::write(
                data_file("storage-switch-status"),
                format!("error\nCannot request a restart: {error}\n"),
            );
            return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
        }
        if let Err(error) = std::fs::write(&pending, format!("{}\n", root.display()))
            .and_then(|()| std::fs::rename(&pending, &choice))
        {
            let _ = std::fs::remove_file(data_file("restart-requested"));
            let _ = std::fs::write(
                data_file("storage-switch-status"),
                format!("error\nCannot save storage selection: {error}\n"),
            );
            return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
        }
        tokio::spawn(async {
            tokio::time::sleep(Duration::from_millis(250)).await;
            RESTART_REQUESTED.notify_one();
        });
        return StatusCode::ACCEPTED.into_response();
    }
    let unit = format!(
        "replay-control-storage-switch-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    );
    let result = tokio::process::Command::new("systemd-run")
        .args([
            "--quiet",
            "--collect",
            "--no-block",
            &format!("--unit={unit}"),
            SWITCH_HELPER,
        ])
        .arg(&root)
        .output()
        .await;
    match result {
        Ok(output) if output.status.success() => StatusCode::ACCEPTED.into_response(),
        Ok(output) => {
            let error = String::from_utf8_lossy(&output.stderr);
            let _ = std::fs::write(
                data_file("storage-switch-status"),
                "error\nCould not start the storage switch.\n",
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Cannot start storage switch: {error}"),
            )
                .into_response()
        }
        Err(error) => {
            let _ = std::fs::write(
                data_file("storage-switch-status"),
                "error\nCould not start the storage switch.\n",
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Cannot start storage switch: {error}"),
            )
                .into_response()
        }
    }
}

fn switch_is_running() -> bool {
    if data_file("storage-switch-pending").exists() {
        return true;
    }
    if !std::fs::read_to_string(data_file("storage-switch-status"))
        .is_ok_and(|status| status.starts_with("running\n"))
    {
        return false;
    }
    std::fs::metadata(data_file("storage-switch-status"))
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|age| age < Duration::from_secs(300))
}

async fn selection_status(State(state): State<SetupState>) -> Json<SelectionResponse> {
    let current = state.current;
    let status_text =
        std::fs::read_to_string(data_file("storage-switch-status")).unwrap_or_default();
    let mut lines = status_text.lines();
    let mut status = lines.next().unwrap_or("idle").to_string();
    let mut message = lines.next().map(str::to_string);
    if status == "running" && !switch_is_running() {
        status = "delayed".to_string();
        message = Some("The storage switch stopped before reporting a result. Restart Replay Control to restore the previous selection.".to_string());
    }
    Json(SelectionResponse {
        current,
        status,
        message,
        storage_error: state.storage_error,
    })
}

async fn active_root(State(state): State<SetupState>) -> Response {
    match state.current {
        Some(root) => (StatusCode::OK, root).into_response(),
        None => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

fn https_redirect(headers: &HeaderMap, port: u16) -> Redirect {
    let host = headers
        .get(HOST)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<axum::http::uri::Authority>().ok())
        .map(|authority| authority.host().to_string())
        .unwrap_or_else(|| "localhost".to_string());
    let host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host
    };
    Redirect::temporary(&format!("https://{host}:{port}/storage-setup"))
}

fn bootstrap_router(current: Option<String>, storage_error: Option<String>) -> Router {
    router_with_state(SetupState {
        current,
        storage_error,
    })
    .route(
        "/api/version",
        get(|| async {
            Json(json!({
                "version": replay_control_app::VERSION,
                "git_hash": replay_control_app::GIT_HASH,
            }))
        }),
    )
    .route(
        "/api/update-result",
        get(|| async {
            std::fs::read_to_string("/var/lib/replay-control/update-result").unwrap_or_default()
        }),
    )
    .route(
        "/settings",
        get(|| async { Redirect::temporary("/storage-setup") }),
    )
    .route("/", get(|| async { Redirect::temporary("/storage-setup") }))
}

/// Serve the selector before AppState can be created. The normal application
/// takes over the same ports after a successful storage switch.
pub async fn serve_bootstrap(
    http_port: u16,
    https_port: u16,
    serve_https: bool,
    current: Option<String>,
    storage_error: Option<String>,
) -> Result<(), String> {
    if let Some(ref error) = storage_error {
        fail_portable_restart(error);
    }
    let recovery_path = current.clone();
    let app = bootstrap_router(current, storage_error);
    let bind_host = if portable() { "127.0.0.1" } else { "0.0.0.0" };
    if !serve_https {
        let listener = tokio::net::TcpListener::bind((bind_host, http_port))
            .await
            .map_err(|error| error.to_string())?;
        return tokio::select! {
            result = axum::serve(
                listener,
                app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            ) => result.map_err(|error| error.to_string()),
            () = wait_for_recovery(recovery_path) => restart_after_recovery(),
            () = wait_for_restart(), if portable() => Ok(()),
        };
    }
    let data_dir = DataDir::new(data_root());
    let certificates = ensure_self_signed_certificate(&data_dir)
        .map_err(|error| format!("Cannot prepare setup HTTPS certificate: {error}"))?;
    let tls = RustlsConfig::from_pem_file(&certificates.cert, &certificates.key)
        .await
        .map_err(|error| format!("Cannot load setup HTTPS certificate: {error}"))?;
    let http = Router::new().fallback(get(move |headers: HeaderMap| async move {
        https_redirect(&headers, https_port)
    }));
    let http_listener = tokio::net::TcpListener::bind((bind_host, http_port))
        .await
        .map_err(|error| error.to_string())?;
    let http_server = axum::serve(http_listener, http);
    let https_addr = std::net::SocketAddr::from((
        if portable() {
            [127, 0, 0, 1]
        } else {
            [0, 0, 0, 0]
        },
        https_port,
    ));
    let https_server = axum_server::bind_rustls(https_addr, tls)
        .serve(app.into_make_service_with_connect_info::<std::net::SocketAddr>());
    tokio::select! {
        result = http_server => result.map_err(|error| error.to_string()),
        result = https_server => result.map_err(|error| error.to_string()),
        () = wait_for_recovery(recovery_path) => restart_after_recovery(),
        () = wait_for_restart(), if portable() => Ok(()),
    }
}

fn restart_after_recovery() -> Result<(), String> {
    if !portable() {
        return Err("Configured storage returned; restarting the package service".to_string());
    }
    request_portable_restart(&data_root())
}

fn request_portable_restart(data_dir: &Path) -> Result<(), String> {
    std::fs::write(data_dir.join("storage-switch-status"), "running\n")
        .map_err(|error| format!("Cannot record storage recovery: {error}"))?;
    std::fs::write(data_dir.join("restart-requested"), "1\n")
        .map_err(|error| format!("Cannot request storage recovery restart: {error}"))
}

async fn wait_for_recovery(path: Option<String>) {
    let Some(path) = path else {
        std::future::pending::<()>().await;
        return;
    };
    loop {
        tokio::time::sleep(Duration::from_secs(5)).await;
        let candidate = path.clone();
        if tokio::task::spawn_blocking(move || check_configured_storage(&candidate))
            .await
            .is_ok_and(|result| result.is_ok())
        {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        bootstrap_router, inspect_directory, parse_saved_path, portal_uri_path,
        request_is_loopback, request_portable_restart,
    };
    use axum::body::Body;
    use axum::extract::ConnectInfo;
    use axum::http::{Request, StatusCode};
    use std::net::SocketAddr;
    use std::path::PathBuf;
    use tower::ServiceExt;

    #[test]
    fn recovered_flatpak_storage_requests_launcher_restart() {
        let temp = tempfile::tempdir().unwrap();
        request_portable_restart(temp.path()).unwrap();
        assert_eq!(
            std::fs::read_to_string(temp.path().join("storage-switch-status")).unwrap(),
            "running\n"
        );
        assert!(temp.path().join("restart-requested").is_file());
    }

    #[test]
    fn requires_readable_roms_but_reports_optional_siblings() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("bios")).unwrap();
        let before = inspect_directory(temp.path().to_str().unwrap()).unwrap();
        assert!(!before.valid);
        assert_eq!(before.folders, vec!["bios"]);
        std::fs::create_dir(temp.path().join("roms")).unwrap();
        let after = inspect_directory(temp.path().to_str().unwrap()).unwrap();
        assert!(after.valid);
        assert_eq!(after.folders, vec!["roms", "bios"]);
    }

    #[test]
    fn saved_path_must_be_one_absolute_line() {
        assert_eq!(parse_saved_path("/mnt/games\n"), Some("/mnt/games".into()));
        assert_eq!(parse_saved_path("games\n"), None);
        assert_eq!(parse_saved_path("/mnt/games\n/other\n"), None);
    }

    #[test]
    fn portal_folder_uri_decodes_to_document_path() {
        assert_eq!(
            portal_uri_path("file:///run/user/1000/doc/abc123/Game%20Library"),
            Some(PathBuf::from("/run/user/1000/doc/abc123/Game Library"))
        );
        assert_eq!(portal_uri_path("https://example.com/library"), None);
    }

    #[test]
    fn local_setup_uses_socket_peer_not_host_header() {
        let mut local = Request::builder().body(Body::empty()).unwrap();
        local
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 8443))));
        assert!(request_is_loopback(&local));

        let mut remote = Request::builder()
            .header("host", "localhost:8443")
            .header("x-forwarded-for", "127.0.0.1")
            .body(Body::empty())
            .unwrap();
        remote
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([192, 168, 1, 40], 8443))));
        assert!(!request_is_loopback(&remote));
    }

    #[tokio::test]
    async fn recovery_server_exposes_update_status_to_local_browser() {
        let app = bootstrap_router(
            Some("/srv/offline".into()),
            Some("Storage unavailable".into()),
        );
        for path in ["/api/version", "/api/update-result"] {
            let mut request = Request::builder().uri(path).body(Body::empty()).unwrap();
            request
                .extensions_mut()
                .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 8443))));
            let response = app.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{path}");
        }
        let mut settings = Request::builder()
            .uri("/settings")
            .body(Body::empty())
            .unwrap();
        settings
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 8443))));
        let response = app.oneshot(settings).await.unwrap();
        assert_eq!(response.status(), StatusCode::TEMPORARY_REDIRECT);
        assert_eq!(response.headers()["location"], "/storage-setup");
    }
}
