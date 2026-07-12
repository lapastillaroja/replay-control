//! The `/waiting` no-storage flow: the storage guard middleware that
//! redirects to `/waiting` while storage is unavailable, the waiting
//! page itself (plain server-rendered HTML, not Leptos), and its
//! reboot action.

use replay_control_core::auth::AuthRole;
use replay_control_core::skins;

use super::AppState;
use super::auth_gate::server_function_required_role;
use crate::types::{StorageStatus, storage_kind_label};
use crate::util::escape_html;

/// Paths that bypass the storage guard middleware.
/// When storage is unavailable, all other requests redirect to `/waiting`.
pub fn is_allowed_without_storage(path: &str) -> bool {
    path == "/waiting"
        || path == "/waiting/reboot"
        || path == "/login"
        || path == "/first-setup"
        || path.starts_with("/static/")
        || path == "/api/version"
        || path == "/api/core/status"
        || path
            .strip_prefix("/sfn/")
            .is_some_and(|_| server_function_required_role(path) == Some(AuthRole::Anonymous))
}

/// Render the `/waiting` page using the current storage status.
///
/// When storage is already available, redirect to `/`. The page's own
/// meta-refresh re-hits this handler every 5s, so this is the path
/// users take out of the waiting page once their mount comes back —
/// `/waiting` is plain server-rendered HTML, not Leptos-hydrated, so
/// the SSE listener in `lib.rs` does not run there.
pub fn waiting_page_response(state: AppState) -> axum::response::Response {
    use axum::response::{IntoResponse, Redirect};

    if state.is_serviceable() {
        return Redirect::temporary("/").into_response();
    }
    axum::response::Html(waiting_page_html(&state)).into_response()
}

/// Handle the reboot action exposed only on waiting-page storage errors.
/// `reboot_allowed` is captured from `AppState.mode.is_device()` when the
/// route is wired (see `with_storage_guard`); the handler itself stays
/// state-less.
pub fn waiting_reboot_response(reboot_allowed: bool) -> axum::response::Response {
    use axum::response::{IntoResponse, Redirect};

    if !reboot_allowed {
        return Redirect::temporary("/waiting").into_response();
    }

    // Fire-and-forget flush — never wait on it. A hard NFS mount can wedge
    // `sync` indefinitely when the network is down, and this reboot path runs
    // precisely when storage/config is broken (often alongside a network drop).
    // systemd syncs during the clean shutdown anyway.
    let _ = std::process::Command::new("sync").spawn();
    match std::process::Command::new("reboot").output() {
        Ok(_) => axum::response::Html(
            r#"<!DOCTYPE html><html><head><meta http-equiv="refresh" content="10;url=/waiting"><title>Rebooting</title></head><body>Rebooting...</body></html>"#,
        )
        .into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to reboot: {e}"),
        )
            .into_response(),
    }
}

/// Add the production no-storage guard and waiting routes around an app router.
pub fn with_storage_guard(app: axum::Router, app_state: AppState) -> axum::Router {
    use axum::middleware::Next;
    use axum::response::{IntoResponse, Redirect};

    let waiting_state = app_state.clone();
    let waiting_handler = axum::routing::get(move || {
        let state = waiting_state.clone();
        async move { waiting_page_response(state) }
    });
    // Captured once at route-build time; reboot is only available on Device.
    let reboot_allowed = app_state.mode.is_device();
    let waiting_reboot_handler =
        axum::routing::post(move || async move { waiting_reboot_response(reboot_allowed) });
    let guard_state = app_state.clone();

    app.route("/waiting", waiting_handler)
        .route("/waiting/reboot", waiting_reboot_handler)
        .layer(axum::middleware::from_fn(
            move |request: axum::http::Request<axum::body::Body>, next: Next| {
                let state = guard_state.clone();
                async move {
                    if state.is_serviceable() {
                        return next.run(request).await;
                    }

                    let path = request.uri().path().to_string();
                    if is_allowed_without_storage(&path) {
                        return next.run(request).await;
                    }

                    Redirect::temporary("/waiting").into_response()
                }
            },
        ))
}

pub fn waiting_page_html(state: &AppState) -> String {
    let storage_mode = state
        .replay_config
        .read()
        .expect("replay_config lock poisoned")
        .as_ref()
        .map(|c| c.storage_mode().to_string())
        .unwrap_or_default();

    let storage_label = storage_kind_label(&storage_mode);

    let skin_index = state.effective_skin();
    let skin_css = skins::theme_css(skin_index).unwrap_or_default();
    let theme_color = skins::theme_color(skin_index);
    let status = state.storage_status();
    // ConfigUnavailable isn't about a storage *type* (we have no config to know
    // it), so don't claim "Waiting for SD storage…" — give it its own title.
    let title = match &status {
        StorageStatus::ConfigUnavailable { .. } => "Configuration unavailable".to_string(),
        _ => format!("Waiting for {storage_label} storage..."),
    };
    let (subtitle, error_html) = match status {
        StorageStatus::Error { message } => (
            "Storage was detected, but Replay Control could not open its database.",
            format!(
                r#"<div class="waiting-error">
                    <p>Replay Control will keep retrying automatically.</p>
                    <p class="waiting-error-detail">{}</p>
                    <p>If storage was just attached or the network mount is still settling, rebooting the Pi may help.</p>
                    <form method="post" action="/waiting/reboot">
                        <button class="btn btn-danger" type="submit">Reboot System</button>
                    </form>
                </div>"#,
                escape_html(&message)
            ),
        ),
        StorageStatus::Activating => (
            "Storage was detected. Replay Control is opening its databases.",
            String::new(),
        ),
        StorageStatus::Misconfigured {
            wanted,
            current_kind,
            reason,
        } => {
            let wanted_label = storage_kind_label(&wanted);
            let fallback = current_kind
                .as_deref()
                .filter(|kind| *kind != wanted.as_str())
                .map(|kind| {
                    format!(
                        "<p>Replay Control is still using {} as a fallback.</p>",
                        storage_kind_label(kind)
                    )
                })
                .unwrap_or_default();
            (
                "The configured storage device is not available.",
                format!(
                    r#"<div class="waiting-error">
                        <p>Configured storage: {}</p>
                        {}
                        <p>Insert the device or change the storage selection in RePlayOS settings.</p>
                        <p class="waiting-error-detail">{}</p>
                    </div>"#,
                    escape_html(wanted_label),
                    fallback,
                    escape_html(&reason)
                ),
            )
        }
        StorageStatus::ConfigUnavailable { reason } => (
            "Replay Control could not read the system configuration.",
            format!(
                r#"<div class="waiting-error">
                    <p>The RePlayOS configuration file is missing or unreadable.</p>
                    <p>Replay Control will keep retrying automatically.</p>
                    <p class="waiting-error-detail">{}</p>
                    <form method="post" action="/waiting/reboot">
                        <button class="btn btn-danger" type="submit">Reboot System</button>
                    </form>
                </div>"#,
                escape_html(&reason)
            ),
        ),
        StorageStatus::WaitingForMount | StorageStatus::Ready => (
            "The configured storage device is not available yet.",
            String::new(),
        ),
    };

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0, viewport-fit=cover">
    <meta name="theme-color" content="{theme_color}">
    <meta http-equiv="refresh" content="5">
    <title>Replay Control — Waiting for Storage</title>
    <link rel="stylesheet" href="/static/style.css">
    <style id="skin-theme">{skin_css}</style>
    <style>
        .waiting-page {{
            display: flex;
            flex-direction: column;
            align-items: center;
            justify-content: center;
            min-height: 80vh;
            padding: 2rem;
            text-align: center;
        }}
        .waiting-icon {{
            font-size: 4rem;
            margin-bottom: 1rem;
            animation: pulse 2s ease-in-out infinite;
        }}
        @keyframes pulse {{
            0%, 100% {{ opacity: 1; }}
            50% {{ opacity: 0.4; }}
        }}
        .waiting-title {{
            font-size: 1.5rem;
            margin-bottom: 0.5rem;
        }}
        .waiting-subtitle {{
            color: var(--text-secondary);
            margin-bottom: 2rem;
        }}
        .waiting-error {{
            max-width: 440px;
            margin-bottom: 2rem;
        }}
        .waiting-error-detail {{
            color: var(--text-secondary);
            font-size: 0.9rem;
            overflow-wrap: anywhere;
        }}
        .waiting-tips {{
            text-align: left;
            max-width: 400px;
        }}
        .waiting-tips h4 {{
            margin-bottom: 0.5rem;
        }}
        .waiting-tips ul {{
            padding-left: 1.2rem;
            line-height: 1.8;
        }}
        .waiting-auto {{
            color: var(--text-secondary);
            font-size: 0.85rem;
            margin-top: 2rem;
        }}
    </style>
</head>
<body>
    <div class="app">
        <header class="top-bar">
            <h1 class="app-title">Replay Control</h1>
        </header>
        <main class="content">
            <div class="waiting-page">
                <div class="waiting-icon">&#x1F4E1;</div>
                <h2 class="waiting-title">{title}</h2>
                <p class="waiting-subtitle">{subtitle}</p>
                {error_html}

                <div class="waiting-tips">
                    <h4>Troubleshooting</h4>
                    <ul>
                        <li><b>USB</b>: Check that the USB drive is plugged in and recognized.</li>
                        <li><b>NFS</b>: Verify WiFi is connected and NFS server is reachable.</li>
                        <li><b>NVMe</b>: Check that the NVMe drive is installed correctly.</li>
                    </ul>
                </div>

                <p class="waiting-auto">This page auto-refreshes every 5 seconds.</p>
            </div>
        </main>
    </div>
</body>
</html>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{StorageStatus, build_waiting_page_test_state};

    #[tokio::test(flavor = "multi_thread")]
    async fn waiting_page_shows_reboot_action_on_storage_error() {
        let tmp = tempfile::tempdir().unwrap();
        let state = build_waiting_page_test_state(tmp.path());
        state.set_storage_status_for_test(StorageStatus::Error {
            message: "open <failed> & retry".into(),
        });

        let html = waiting_page_html(&state);

        assert!(
            html.contains("Storage was detected, but Replay Control could not open its database.")
        );
        assert!(html.contains("Replay Control will keep retrying automatically."));
        assert!(html.contains("open &lt;failed&gt; &amp; retry"));
        assert!(html.contains(r#"action="/waiting/reboot""#));
        assert!(html.contains("Reboot System"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn waiting_page_hides_reboot_action_while_waiting_for_mount() {
        let tmp = tempfile::tempdir().unwrap();
        let state = build_waiting_page_test_state(tmp.path());
        state.set_storage_status_for_test(StorageStatus::WaitingForMount);

        let html = waiting_page_html(&state);

        assert!(html.contains("The configured storage device is not available yet."));
        assert!(!html.contains("Reboot System"));
        assert!(!html.contains(r#"action="/waiting/reboot""#));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn waiting_page_shows_configured_storage_misconfiguration() {
        let tmp = tempfile::tempdir().unwrap();
        let state = build_waiting_page_test_state(tmp.path());
        state.set_storage_status_for_test(StorageStatus::Misconfigured {
            wanted: "nvme".into(),
            current_kind: None,
            reason: "path <missing> & not mounted".into(),
        });

        let html = waiting_page_html(&state);

        assert!(html.contains("The configured storage device is not available."));
        assert!(html.contains("Configured storage: NVMe"));
        assert!(html.contains("change the storage selection in RePlayOS settings"));
        assert!(html.contains("path &lt;missing&gt; &amp; not mounted"));
        assert!(!html.contains("Reboot System"));
    }
}
