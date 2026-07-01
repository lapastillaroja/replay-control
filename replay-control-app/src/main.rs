#![recursion_limit = "512"]

#[cfg(feature = "ssr")]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

#[cfg(feature = "ssr")]
mod ssr {
    use axum::extract::connect_info::ConnectInfo;
    use axum::http::StatusCode;
    use axum::http::header::HOST;
    use axum::http::{HeaderMap, Request};
    use axum::response::Response;
    use axum::response::{Html, IntoResponse};
    use axum::routing::MethodRouter;
    use axum_server::tls_rustls::RustlsConfig;
    use clap::Parser;
    use leptos::config::LeptosOptions;
    use replay_control_core::skins;
    use replay_control_core_server::security::tls::{
        ensure_self_signed_certificate, install_default_crypto_provider,
        regenerate_self_signed_certificate,
    };
    use replay_control_core_server::update::read_available_update;
    use serde::Serialize;
    use std::net::SocketAddr;
    use tower_http::compression::CompressionLayer;
    use tower_http::services::ServeDir;
    use tower_http::set_header::SetResponseHeaderLayer;

    use replay_control_app::api;
    use replay_control_app::server_fns::{
        AddGameResourceLink, CompleteFirstSetup, DeleteUserCapture, DowngradeAdminToUser,
        EnableReplayApiAssisted, GetAdminSessionTimeout, GetAuthStatus, GetGamePlaytime,
        GetGameResourceLinks, GetLibraryPlaytime, GetLiveStats, GetReplayApiStatus,
        GetReplayosSettings, GetSaveStateSlots, GetTlsCertificateInfo, GetUserCaptures, LoginAdmin,
        LoginWithReplayCode, Logout, LogoutAllBrowsers, PowerOffReplayosDevice,
        RegenerateTlsCertificateInfo, RemoveGameResourceLink, ReprobeReplayApi,
        RestartReplayosGame, SaveReplayosKioskMode, SendReplayPlayerCommand, SendReplayosMessage,
        SetAdminSessionTimeout, StartSetupMetadataDownloads, VerifyReplayApiToken,
    };

    #[derive(Parser)]
    #[command(
        name = "replay-control-app",
        about = "Replay Control — companion app for RePlayOS"
    )]
    struct Cli {
        /// HTTP port. With HTTPS enabled, this serves the HTTPS guidance page.
        #[arg(short, long, default_value = "8080")]
        port: u16,

        /// HTTPS port for the main Replay Control app.
        #[arg(long, default_value = "8443")]
        https_port: u16,

        /// Enable HTTPS in standalone mode. Device mode enables HTTPS by default.
        #[arg(long)]
        enable_https: bool,

        /// Disable HTTPS and serve the app over plain HTTP. Dangerous: for local debugging only.
        #[arg(long)]
        dangerous_disable_https: bool,

        /// Allow session cookies over plain HTTP when HTTPS is disabled. Dangerous: credentials and sessions cross the LAN unencrypted.
        #[arg(long)]
        dangerous_allow_insecure_auth_over_http: bool,

        /// Storage root path override (auto-detected if not set)
        #[arg(long)]
        storage_path: Option<String>,

        /// Override the settings directory (default: /etc/replay-control on Pi,
        /// <storage>/.replay-control with --storage-path)
        #[arg(long)]
        settings_path: Option<String>,

        /// Override the data directory used for per-storage library DBs
        /// (default: /var/lib/replay-control on Pi). With --storage-path,
        /// defaults to <storage>/.replay-control-data so dev runs are
        /// self-contained.
        #[arg(long)]
        data_dir: Option<String>,

        /// Path to the game catalog SQLite file
        #[arg(long, default_value = "catalog.sqlite")]
        catalog_path: String,

        /// Path to the site root (where pkg/ and style.css live)
        #[arg(long, default_value = "target/site")]
        site_root: String,
    }

    fn https_enabled(enable_https: bool, dangerous_disable_https: bool, is_device: bool) -> bool {
        !dangerous_disable_https && (is_device || enable_https)
    }

    fn http_guidance_router(
        https_port: u16,
        state: api::AppState,
        media_handler: MethodRouter,
        captures_handler: MethodRouter,
        manuals_handler: MethodRouter,
        owned_manuals_handler: MethodRouter,
        rom_docs_handler: MethodRouter,
    ) -> axum::Router {
        let guidance_skin_css = skins::theme_css(state.effective_skin()).unwrap_or_default();
        let loopback_compat_routes = axum::Router::new()
            .nest("/api/core", api::core_routes().with_state(state))
            .route("/captures/*path", captures_handler)
            .route("/manuals/*path", manuals_handler)
            .route("/owned-manuals/*path", owned_manuals_handler)
            .route("/rom-docs/*path", rom_docs_handler)
            .route("/media/*path", media_handler)
            .route_layer(axum::middleware::from_fn(require_loopback));

        axum::Router::new()
            .merge(loopback_compat_routes)
            .route(
                "/api/version",
                axum::routing::get(|| async {
                    axum::Json(serde_json::json!({
                        "version": replay_control_app::VERSION,
                        "git_hash": replay_control_app::GIT_HASH,
                    }))
                }),
            )
            .route(
                "/static/branding/logo-oneline-transparent.png",
                axum::routing::get(|| async {
                    (
                        [
                            ("content-type", "image/png"),
                            ("cache-control", api::CACHE_IMMUTABLE),
                        ],
                        include_bytes!("../static/branding/logo-oneline-transparent.png")
                            .as_slice(),
                    )
                }),
            )
            .fallback(axum::routing::any(move |headers: HeaderMap| {
                let guidance_skin_css = guidance_skin_css.clone();
                async move { https_guidance_response(&headers, https_port, &guidance_skin_css) }
            }))
    }

    async fn require_loopback(
        request: Request<axum::body::Body>,
        next: axum::middleware::Next,
    ) -> Response {
        if request_peer_is_loopback(&request) {
            next.run(request).await
        } else {
            StatusCode::FORBIDDEN.into_response()
        }
    }

    fn request_peer_is_loopback(request: &Request<axum::body::Body>) -> bool {
        request
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .is_some_and(|ConnectInfo(addr)| addr.ip().is_loopback())
    }

    fn https_guidance_response(
        headers: &HeaderMap,
        https_port: u16,
        skin_css: &str,
    ) -> axum::response::Response {
        let https_url = https_url_for_request(headers, https_port);
        let https_url_html = escape_html(&https_url);
        Html(format!(
            r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Use HTTPS - Replay Control</title>
  <style>
    :root {{
      color-scheme: light dark;
      --bg: #0f1115;
      --surface: #1a1d23;
      --surface-shell-bg: var(--surface);
      --text: #f9fafb;
      --text-secondary: #d1d5db;
      --accent: #7dd3fc;
      --accent-hover: #bae6fd;
      --text-on-accent: #111827;
      --border: rgba(255,255,255,0.16);
    }}
    {skin_css}
    * {{ box-sizing: border-box; }}
    body {{
      margin: 0;
      min-height: 100vh;
      min-height: 100dvh;
      display: grid;
      align-items: start;
      justify-items: center;
      overflow-y: auto;
      -webkit-overflow-scrolling: touch;
      padding: max(clamp(1.125rem, 7dvh, 2rem), env(safe-area-inset-top)) max(1rem, env(safe-area-inset-right)) max(clamp(1.125rem, 7dvh, 2rem), env(safe-area-inset-bottom)) max(1rem, env(safe-area-inset-left));
      font-family: system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
      background: var(--surface-shell-bg);
      color: var(--text);
      line-height: 1.5;
    }}
    main {{
      width: min(34rem, 100%);
    }}
    .logo {{
      display: block;
      width: min(18rem, 72vw);
      height: auto;
      margin: 0 0 1.5rem;
    }}
    h1 {{ margin: 0 0 0.75rem; font-size: 1.75rem; letter-spacing: 0; }}
    p {{ margin: 0 0 1rem; line-height: 1.55; color: var(--text-secondary); }}
    a {{
      display: inline-flex;
      align-items: center;
      justify-content: center;
      width: min(100%, 22rem);
      min-height: 2.75rem;
      padding: 0 1rem;
      border-radius: 0.375rem;
      background: var(--accent);
      color: var(--text-on-accent);
      font-weight: 650;
      text-decoration: none;
    }}
    a:focus-visible,
    select:focus {{
      outline: 2px solid var(--accent);
      outline-offset: 2px;
    }}
    code {{ overflow-wrap: anywhere; }}
    .note {{ margin-top: 1rem; font-size: 0.95rem; color: var(--text-secondary); }}
    .language {{
      margin-top: 2rem;
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: 0.625rem;
      color: var(--text-secondary);
      font-size: 0.9rem;
    }}
    select {{
      min-height: 2rem;
      border: 1px solid color-mix(in srgb, var(--accent), transparent 45%);
      border-radius: 0.375rem;
      padding: 0 2rem 0 0.625rem;
      appearance: none;
      background-color: #f9fafb;
      background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 12 12'%3E%3Cpath fill='%23888' d='M6 8L1 3h10z'/%3E%3C/svg%3E");
      background-position: right 0.625rem center;
      background-repeat: no-repeat;
      background-size: 12px 12px;
      color: #111827;
      accent-color: var(--accent);
    }}
    option {{
      background: #f9fafb;
      color: #111827;
    }}
    @media (min-width: 700px) and (min-height: 760px) {{
      body {{ align-items: center; }}
    }}
    @media (max-width: 520px) {{
      body {{
        padding-right: max(0.875rem, env(safe-area-inset-right));
        padding-left: max(0.875rem, env(safe-area-inset-left));
      }}
      .logo {{
        width: min(14rem, 72vw);
        margin-bottom: 1.125rem;
      }}
      h1 {{ font-size: 1.35rem; }}
      a {{ width: 100%; }}
    }}
  </style>
</head>
<body>
  <main>
    <img class="logo" src="/static/branding/logo-oneline-transparent.png" alt="Replay Control">
    <h1 data-i18n="title">Use HTTPS</h1>
    <p data-i18n="body">Replay Control is available over an encrypted local HTTPS connection.</p>
    <p><a href="{https_url_html}" data-i18n="button">Open Replay Control over HTTPS</a></p>
    <p class="note"><span data-i18n="note_before">This device uses a local self-signed certificate. Your browser will show a security warning the first time you open</span> <code>{https_url_html}</code>. <span data-i18n="note_after">Choose the advanced or continue option to approve the security exception for this device.</span></p>
    <label class="language">
      <span data-i18n="language">Language</span>
      <select id="language-select" aria-label="Language">
        <option value="en">English</option>
        <option value="es">Español</option>
        <option value="ja">日本語</option>
      </select>
    </label>
  </main>
  <script>
    const messages = {{
      en: {{
        title: "Use HTTPS",
        body: "Replay Control is available over an encrypted local HTTPS connection.",
        button: "Open Replay Control over HTTPS",
        note_before: "This device uses a local self-signed certificate. Your browser will show a security warning the first time you open",
        note_after: "Choose the advanced or continue option to approve the security exception for this device.",
        language: "Language",
      }},
      es: {{
        title: "Usa HTTPS",
        body: "Replay Control está disponible mediante una conexión HTTPS local cifrada.",
        button: "Abrir Replay Control con HTTPS",
        note_before: "Este dispositivo usa un certificado local autofirmado. El navegador mostrará una advertencia de seguridad la primera vez que abras",
        note_after: "Elige la opción avanzada o continuar para aprobar la excepción de seguridad de este dispositivo.",
        language: "Idioma",
      }},
      ja: {{
        title: "HTTPSを使用",
        body: "Replay Controlは暗号化されたローカルHTTPS接続で利用できます。",
        button: "HTTPSでReplay Controlを開く",
        note_before: "このデバイスはローカルの自己署名証明書を使用しています。初めて開くと、ブラウザーにセキュリティ警告が表示されます:",
        note_after: "詳細または続行のオプションを選び、このデバイスのセキュリティ例外を承認してください。",
        language: "言語",
      }},
    }};

    const supported = Object.keys(messages);
    const select = document.getElementById("language-select");

    function normalizeLanguage(language) {{
      const base = String(language || "").toLowerCase().split("-")[0];
      return supported.includes(base) ? base : null;
    }}

    function browserLanguage() {{
      const languages = navigator.languages && navigator.languages.length
        ? navigator.languages
        : [navigator.language];
      return languages.map(normalizeLanguage).find(Boolean) || "en";
    }}

    function applyLanguage(language) {{
      const selected = messages[language] ? language : "en";
      document.documentElement.lang = selected;
      document.title = `${{messages[selected].title}} - Replay Control`;
      document.querySelectorAll("[data-i18n]").forEach((node) => {{
        node.textContent = messages[selected][node.dataset.i18n];
      }});
      select.value = selected;
    }}

    select.addEventListener("change", (event) => applyLanguage(event.target.value));
    applyLanguage(browserLanguage());
  </script>
</body>
</html>"#
        ))
        .into_response()
    }

    fn https_url_for_request(headers: &HeaderMap, https_port: u16) -> String {
        let host = headers
            .get(HOST)
            .and_then(|value| value.to_str().ok())
            .and_then(validated_host_without_port)
            .unwrap_or_else(|| "replay.local".to_string());
        format!("https://{host}:{https_port}/")
    }

    fn validated_host_without_port(host: &str) -> Option<String> {
        let host = host.trim();
        if host.is_empty() {
            return None;
        }
        if host.starts_with('[') {
            let end = host.find(']')?;
            let address = &host[1..end];
            let remainder = &host[end + 1..];
            if !(remainder.is_empty()
                || remainder.strip_prefix(':').is_some_and(|port| {
                    !port.is_empty() && port.chars().all(|ch| ch.is_ascii_digit())
                }))
            {
                return None;
            }
            let ip: std::net::IpAddr = address.parse().ok()?;
            return ip.is_ipv6().then(|| format!("[{ip}]"));
        }
        let name = host.split_once(':').map_or(host, |(name, port)| {
            if port.chars().all(|ch| ch.is_ascii_digit()) {
                name
            } else {
                ""
            }
        });
        if name.is_empty() {
            return None;
        }
        if let Ok(ip) = name.parse::<std::net::IpAddr>() {
            return ip.is_ipv4().then(|| ip.to_string());
        }
        is_valid_dns_name(name).then(|| name.to_ascii_lowercase())
    }

    fn is_valid_dns_name(name: &str) -> bool {
        name.len() <= 253
            && name.split('.').all(|label| {
                !label.is_empty()
                    && label.len() <= 63
                    && !label.starts_with('-')
                    && !label.ends_with('-')
                    && label
                        .chars()
                        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
            })
    }

    fn escape_html(value: &str) -> String {
        let mut escaped = String::with_capacity(value.len());
        for ch in value.chars() {
            match ch {
                '&' => escaped.push_str("&amp;"),
                '<' => escaped.push_str("&lt;"),
                '>' => escaped.push_str("&gt;"),
                '"' => escaped.push_str("&quot;"),
                '\'' => escaped.push_str("&#39;"),
                _ => escaped.push(ch),
            }
        }
        escaped
    }

    /// Broadcast-based SSE stream for activity state changes.
    ///
    /// Sends an initial state snapshot, then waits on the activity broadcast
    /// channel for updates. No polling loop — events are pushed whenever the
    /// activity state changes (import progress, thumbnail download, etc.).
    fn sse_activity_stream(
        state: api::AppState,
    ) -> axum::response::sse::Sse<
        impl tokio_stream::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>,
    > {
        use axum::response::sse::{Event, KeepAlive, Sse};
        use std::convert::Infallible;

        let mut rx = state.activity_tx.subscribe();
        let stream = async_stream::stream! {
            // Send initial state so the client has current values on connect.
            let activity = state.activity();
            let json = serde_json::to_string(&activity).unwrap_or_default();
            yield Ok::<_, Infallible>(Event::default().data(json));

            // Wait for broadcast events (no polling).
            loop {
                match rx.recv().await {
                    Ok(activity) => {
                        let json = serde_json::to_string(&activity).unwrap_or_default();
                        yield Ok::<_, Infallible>(Event::default().data(json));
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        // Missed some events — send current state to catch up.
                        let activity = state.activity();
                        let json = serde_json::to_string(&activity).unwrap_or_default();
                        yield Ok::<_, Infallible>(Event::default().data(json));
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        };

        Sse::new(stream).keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)))
    }

    fn config_init_payload(state: &api::AppState) -> serde_json::Value {
        let skin = state.effective_skin();
        let skin_css = skins::theme_css(skin);
        let storage_kind = if state.has_storage() {
            state.storage().kind.as_str().to_string()
        } else {
            "none".to_string()
        };
        let storage_status = state.storage_status();
        let rom_watcher_status = state.rom_watcher_status();
        let available_update = read_available_update();
        let version = replay_control_app::VERSION;
        let (library_corrupt, user_data_corrupt, user_data_backup_exists) =
            state.corruption_status();
        let asset_health = state.asset_health_snapshot();
        let replay_api_status = state
            .replay_api
            .as_ref()
            .map(|api| api.status())
            .unwrap_or_default();

        serde_json::json!({
            "type": "init",
            "skin_index": skin,
            "skin_css": skin_css,
            "storage_kind": storage_kind,
            "storage_status": storage_status,
            "rom_watcher_status": rom_watcher_status,
            "available_update": available_update,
            "version": version,
            "library_corrupt": library_corrupt,
            "user_data_corrupt": user_data_corrupt,
            "user_data_backup_exists": user_data_backup_exists,
            "asset_health": asset_health,
            "replay_api_status": replay_api_status,
        })
    }

    fn multiplexed_sse_payload<T: Serialize>(stream: &str, payload: T) -> String {
        serde_json::json!({
            "stream": stream,
            "payload": payload,
        })
        .to_string()
    }

    /// Broadcast-based SSE stream for config change notifications.
    ///
    /// Unlike the polling-based `/sse/activity`, this stream is event-driven:
    /// it sends an initial state snapshot, then waits on a broadcast channel
    /// for skin or storage change events. No polling loop.
    fn sse_config_stream(
        state: api::AppState,
    ) -> axum::response::sse::Sse<
        impl tokio_stream::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>,
    > {
        use axum::response::sse::{Event, KeepAlive, Sse};
        use std::convert::Infallible;

        let mut rx = state.events_tx.subscribe();
        let stream = async_stream::stream! {
            // Send initial state so the client has current values on connect.
            yield Ok::<_, Infallible>(Event::default().data(config_init_payload(&state).to_string()));

            // Then wait for broadcast events (no polling).
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        if let Ok(json) = serde_json::to_string(&event) {
                            yield Ok::<_, Infallible>(Event::default().data(json));
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        };

        Sse::new(stream).keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(30)))
    }

    /// Broadcast-based SSE stream for now-playing state changes.
    fn sse_now_playing_stream(
        state: api::AppState,
    ) -> axum::response::sse::Sse<
        impl tokio_stream::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>,
    > {
        use axum::response::sse::{Event, KeepAlive, Sse};
        use std::convert::Infallible;

        let mut rx = state.now_playing_tx.subscribe();
        let stream = async_stream::stream! {
            let now_playing = state.now_playing();
            let json = serde_json::to_string(&now_playing).unwrap_or_default();
            yield Ok::<_, Infallible>(Event::default().data(json));

            loop {
                match rx.recv().await {
                    Ok(now_playing) => {
                        let json = serde_json::to_string(&now_playing).unwrap_or_default();
                        yield Ok::<_, Infallible>(Event::default().data(json));
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        let now_playing = state.now_playing();
                        let json = serde_json::to_string(&now_playing).unwrap_or_default();
                        yield Ok::<_, Infallible>(Event::default().data(json));
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        };

        Sse::new(stream).keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)))
    }

    /// Single browser-facing SSE stream for app-wide live state.
    ///
    /// The legacy per-topic endpoints remain available, but the hydrated UI uses
    /// this endpoint so each tab holds one HTTP/1.1 connection instead of three.
    fn sse_events_stream(
        state: api::AppState,
    ) -> axum::response::sse::Sse<
        impl tokio_stream::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>,
    > {
        use axum::response::sse::{Event, KeepAlive, Sse};
        use std::convert::Infallible;

        let mut config_rx = state.events_tx.subscribe();
        let mut activity_rx = state.activity_tx.subscribe();
        let mut now_playing_rx = state.now_playing_tx.subscribe();
        let stream = async_stream::stream! {
            yield Ok::<_, Infallible>(Event::default().data(
                multiplexed_sse_payload("config", config_init_payload(&state)),
            ));
            yield Ok::<_, Infallible>(Event::default().data(
                multiplexed_sse_payload("activity", state.activity()),
            ));
            yield Ok::<_, Infallible>(Event::default().data(
                multiplexed_sse_payload("now_playing", state.now_playing()),
            ));

            loop {
                tokio::select! {
                    event = config_rx.recv() => {
                        match event {
                            Ok(event) => {
                                yield Ok::<_, Infallible>(Event::default().data(
                                    multiplexed_sse_payload("config", event),
                                ));
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                                yield Ok::<_, Infallible>(Event::default().data(
                                    multiplexed_sse_payload("config", config_init_payload(&state)),
                                ));
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        }
                    }
                    activity = activity_rx.recv() => {
                        match activity {
                            Ok(activity) => {
                                yield Ok::<_, Infallible>(Event::default().data(
                                    multiplexed_sse_payload("activity", activity),
                                ));
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                                yield Ok::<_, Infallible>(Event::default().data(
                                    multiplexed_sse_payload("activity", state.activity()),
                                ));
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        }
                    }
                    now_playing = now_playing_rx.recv() => {
                        match now_playing {
                            Ok(now_playing) => {
                                yield Ok::<_, Infallible>(Event::default().data(
                                    multiplexed_sse_payload("now_playing", now_playing),
                                ));
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                                yield Ok::<_, Infallible>(Event::default().data(
                                    multiplexed_sse_payload("now_playing", state.now_playing()),
                                ));
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        }
                    }
                }
            }
        };

        Sse::new(stream).keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)))
    }

    /// Strong ETag from file metadata (mtime + size). `None` means the file
    /// is missing or its metadata is unreadable — callers map both to 404.
    async fn file_etag(path: &std::path::Path) -> Option<String> {
        let meta = tokio::fs::metadata(path).await.ok()?;
        let mtime = meta.modified().ok()?;
        let nanos = mtime.duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos();
        Some(format!("\"{nanos}-{}\"", meta.len()))
    }

    /// True if the request's `If-None-Match` matches `etag` (or is `*`).
    fn etag_matches(headers: &axum::http::HeaderMap, etag: &str) -> bool {
        headers
            .get(axum::http::header::IF_NONE_MATCH)
            .and_then(|v| v.to_str().ok())
            .map(|inm| inm.split(',').map(str::trim).any(|t| t == etag || t == "*"))
            .unwrap_or(false)
    }

    /// Serve a file with ETag-based revalidation. Returns 304 when the client's
    /// `If-None-Match` matches the file's mtime+size tag, 200 with body otherwise,
    /// and 404 if the file is missing or unreadable.
    async fn serve_file_etagged(
        file_path: &std::path::Path,
        content_type: &str,
        headers: &axum::http::HeaderMap,
        cache_control: &'static str,
    ) -> axum::response::Response {
        use axum::http::StatusCode;
        use axum::response::IntoResponse;

        let Some(etag) = file_etag(file_path).await else {
            return StatusCode::NOT_FOUND.into_response();
        };

        if etag_matches(headers, &etag) {
            return (
                StatusCode::NOT_MODIFIED,
                [("cache-control", cache_control), ("etag", etag.as_str())],
            )
                .into_response();
        }

        match tokio::fs::read(file_path).await {
            Ok(data) => (
                StatusCode::OK,
                [
                    ("content-type", content_type),
                    ("cache-control", cache_control),
                    ("etag", etag.as_str()),
                ],
                data,
            )
                .into_response(),
            Err(_) => StatusCode::NOT_FOUND.into_response(),
        }
    }

    /// Serve in-folder documents (PDFs, text files, images) from a game's ROM directory.
    ///
    /// URL format: `/rom-docs/<system>/<base64_rom_filename>/<relative_doc_path>`
    ///
    /// Handles special ROM types:
    /// - `.svm` files: reads the file to find the ScummVM game directory
    /// - `.m3u` playlists: looks for a sibling directory or follows .svm references
    /// - Directories: serves directly from the ROM path
    async fn serve_rom_doc(
        state: api::AppState,
        path: String,
        headers: axum::http::HeaderMap,
    ) -> axum::response::Response {
        use axum::http::StatusCode;
        use axum::response::IntoResponse;

        // Parse: system/base64_rom/relative_path
        let parts: Vec<&str> = path.splitn(3, '/').collect();
        if parts.len() < 3 {
            return StatusCode::BAD_REQUEST.into_response();
        }

        let system = parts[0];
        let rom_b64 = parts[1];
        let doc_relative = urlencoding::decode(parts[2])
            .map(|s| s.into_owned())
            .unwrap_or_else(|_| parts[2].to_string());

        // Path traversal protection
        if doc_relative.split('/').any(|s| s == "..") || doc_relative.contains('\\') {
            return StatusCode::BAD_REQUEST.into_response();
        }

        // Decode ROM filename
        let rom_filename = match replay_control_app::util::base64_decode(rom_b64) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(s) => s,
                Err(_) => return StatusCode::BAD_REQUEST.into_response(),
            },
            Err(_) => return StatusCode::BAD_REQUEST.into_response(),
        };

        // Resolve game directory
        let roms_dir = state.storage().roms_dir().join(system);
        let rom_path = roms_dir.join(&rom_filename);

        let game_dir = if rom_filename.ends_with(".svm") {
            // ScummVM: read .svm to find game directory
            match tokio::fs::read_to_string(&rom_path).await {
                Ok(content) => {
                    let svm_path = content.trim().to_string();
                    let candidate = std::path::PathBuf::from(&svm_path);
                    if candidate.is_dir() {
                        candidate
                    } else {
                        let rel = roms_dir.join(&svm_path);
                        if rel.is_dir() {
                            rel
                        } else {
                            return StatusCode::NOT_FOUND.into_response();
                        }
                    }
                }
                Err(_) => return StatusCode::NOT_FOUND.into_response(),
            }
        } else if rom_filename.ends_with(".m3u") {
            // M3U playlist: check for sibling directory with same name
            let stem = rom_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            let sibling = rom_path.parent().unwrap_or(&roms_dir).join(&stem);
            if sibling.is_dir() {
                sibling
            } else if let Ok(content) = tokio::fs::read_to_string(&rom_path).await {
                // Follow .svm reference from .m3u
                let mut resolved = None;
                for line in content.lines() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    if line.to_lowercase().ends_with(".svm") {
                        let svm = std::path::PathBuf::from(line);
                        if let Some(p) = svm.parent() {
                            let dir = if p.is_absolute() {
                                p.to_path_buf()
                            } else {
                                roms_dir.join(p)
                            };
                            if dir.is_dir() {
                                resolved = Some(dir);
                                break;
                            }
                        }
                    }
                }
                match resolved {
                    Some(d) => d,
                    None => return StatusCode::NOT_FOUND.into_response(),
                }
            } else {
                return StatusCode::NOT_FOUND.into_response();
            }
        } else if rom_path.is_dir() {
            rom_path
        } else {
            return StatusCode::NOT_FOUND.into_response();
        };

        let file_path = game_dir.join(&doc_relative);

        // Verify the resolved path is within the game directory
        match file_path.canonicalize() {
            Ok(canonical) => {
                if let Ok(game_canonical) = game_dir.canonicalize()
                    && !canonical.starts_with(&game_canonical)
                {
                    return StatusCode::BAD_REQUEST.into_response();
                }
            }
            Err(_) => return StatusCode::NOT_FOUND.into_response(),
        }

        let content_type = match doc_relative
            .rsplit('.')
            .next()
            .unwrap_or("")
            .to_lowercase()
            .as_str()
        {
            "pdf" => "application/pdf",
            "txt" => "text/plain; charset=utf-8",
            "html" | "htm" => "text/html; charset=utf-8",
            "jpg" | "jpeg" => "image/jpeg",
            "png" => "image/png",
            "gif" => "image/gif",
            "doc" => "application/msword",
            _ => "application/octet-stream",
        };

        serve_file_etagged(&file_path, content_type, &headers, api::CACHE_PRIVATE_1D).await
    }

    /// Resolve the catalog SQLite path. If the supplied path is relative and
    /// doesn't exist at the current working directory, fall back to the same
    /// filename next to the executable (systemd units without `WorkingDirectory`
    /// default to `/`, where `catalog.sqlite` won't exist).
    fn resolve_catalog_path(configured: &str) -> std::path::PathBuf {
        let as_given = std::path::PathBuf::from(configured);
        if as_given.is_absolute() || as_given.exists() {
            return as_given;
        }
        if let Ok(exe) = std::env::current_exe()
            && let Some(dir) = exe.parent()
        {
            let candidate = dir.join(&as_given);
            if candidate.exists() {
                return candidate;
            }
        }
        as_given
    }

    pub async fn run() {
        install_default_crypto_provider();

        use std::io::IsTerminal;
        tracing_subscriber::fmt()
            .with_ansi(std::io::stderr().is_terminal())
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                    "replay_control_app=info,replay_control_core=info"
                        .parse()
                        .unwrap()
                }),
            )
            .init();

        // Initialize the async task executor for Leptos reactive system.
        // Without this, Resource async tasks won't run during SSR.
        let _ = any_spawner::Executor::init_tokio();

        let cli = Cli::parse();
        tracing::info!(
            "Replay Control version: v{} ({})",
            replay_control_app::VERSION,
            replay_control_app::GIT_HASH
        );

        let catalog_path = resolve_catalog_path(&cli.catalog_path);
        if let Err(e) = replay_control_core_server::init_catalog(&catalog_path).await {
            tracing::error!(
                "catalog not loaded from {} ({e}) — catalog.sqlite is required. \
                Place it next to the executable or pass --catalog-path.",
                catalog_path.display()
            );
            std::process::exit(1);
        }
        tracing::info!("catalog loaded from {}", catalog_path.display());

        let mut app_state =
            match api::AppState::new(cli.storage_path, cli.settings_path, cli.data_dir) {
                Ok(state) => state,
                Err(e) => {
                    tracing::error!("Failed to initialize: {e}");
                    tracing::info!(
                        "Hint: use --storage-path to point to a RePlayOS storage location"
                    );
                    std::process::exit(1);
                }
            };
        let device_mode = app_state.mode.is_device();
        let serve_https = https_enabled(cli.enable_https, cli.dangerous_disable_https, device_mode);

        // Browsers drop a Secure cookie on a non-HTTPS origin, so when we are not
        // serving HTTPS the session cookie must drop Secure or login never
        // persists (infinite redirect to /login). Secure iff serving HTTPS.
        if !serve_https {
            tracing::warn!("serving without HTTPS; session cookies will be sent over plain HTTP");
            app_state.auth.cookie_policy.allow_insecure_transport();
        }
        if cli.dangerous_disable_https && cli.enable_https {
            tracing::warn!("dangerous_disable_https overrides enable_https; serving without HTTPS");
        }

        // The external_metadata DB pool is constructed inside AppState::new
        // (alongside library_pool / user_data_pool); no extra wiring here.

        // Start background pipeline only if storage is available.
        // When no storage, the mount/config watchers (and the fallback poll)
        // start the pipeline on the None->Some transition via
        // reload_config_and_redetect_storage().
        if app_state.has_storage() {
            api::BackgroundManager::start(app_state.clone());
        } else {
            app_state.clone().spawn_storage_watcher();
        }

        // Explicitly register all server functions (inventory auto-registration
        // doesn't work when the functions are in a library crate).
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetInfo>();
        server_fn::axum::register_explicit::<GetLiveStats>();
        server_fn::axum::register_explicit::<GetAuthStatus>();
        server_fn::axum::register_explicit::<LoginWithReplayCode>();
        server_fn::axum::register_explicit::<LoginAdmin>();
        server_fn::axum::register_explicit::<CompleteFirstSetup>();
        server_fn::axum::register_explicit::<GetAdminSessionTimeout>();
        server_fn::axum::register_explicit::<SetAdminSessionTimeout>();
        server_fn::axum::register_explicit::<DowngradeAdminToUser>();
        server_fn::axum::register_explicit::<Logout>();
        server_fn::axum::register_explicit::<LogoutAllBrowsers>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetMode>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetSystems>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetFavorites>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetRecents>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::DeleteRecent>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::AddFavorite>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::RemoveFavorite>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GroupFavorites>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::FlattenFavorites>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetRomsPage>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetSystemFavorites>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::DeleteRom>();
        server_fn::axum::register_explicit::<DeleteUserCapture>();
        server_fn::axum::register_explicit::<GetUserCaptures>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::RenameRom>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetRomFileGroup>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::LaunchGame>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetRomDetail>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::RefreshStorage>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetWifiConfig>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::SaveWifiConfig>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetNfsConfig>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::SaveNfsConfig>();
        server_fn::axum::register_explicit::<
            replay_control_app::server_fns::GetRetroachievementsConfig,
        >();
        server_fn::axum::register_explicit::<
            replay_control_app::server_fns::SaveRetroachievementsConfigAndRestart,
        >();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::RebootSystem>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::OrganizeFavorites>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetSkins>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::SetSkin>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::SetSkinSync>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetHostname>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::SaveHostname>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::ChangeRootPassword>();
        server_fn::axum::register_explicit::<GetTlsCertificateInfo>();
        server_fn::axum::register_explicit::<RegenerateTlsCertificateInfo>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::ClearMetadata>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::RegenerateMetadata>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::DownloadMetadata>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::ClearImages>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::CleanupOrphanedImages>(
        );
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetSystemLogs>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetLogLevelConfig>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::SaveLogLevelConfig>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetGameVideos>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetProviderGameVideos>(
        );
        server_fn::axum::register_explicit::<replay_control_app::server_fns::AddGameVideo>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::RemoveGameVideo>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::SearchGameVideos>();
        server_fn::axum::register_explicit::<GetGameResourceLinks>();
        server_fn::axum::register_explicit::<AddGameResourceLink>();
        server_fn::axum::register_explicit::<RemoveGameResourceLink>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GlobalSearch>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetAllGenres>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetSystemGenres>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::RandomGame>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::RandomGameForSystem>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::SearchByDeveloper>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetDeveloperGames>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetDeveloperGenres>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetBoardGames>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetBoardGenres>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::SearchByBoard>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetRegionPreference>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::SaveRegionPreference>(
        );
        server_fn::axum::register_explicit::<
            replay_control_app::server_fns::GetRegionPreferenceSecondary,
        >();
        server_fn::axum::register_explicit::<
            replay_control_app::server_fns::SaveRegionPreferenceSecondary,
        >();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetFontSize>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::SaveFontSize>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetRecommendations>();
        server_fn::axum::register_explicit::<
            replay_control_app::server_fns::GetFavoritesRecommendations,
        >();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::UpdateThumbnails>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::CancelThumbnailUpdate>(
        );
        server_fn::axum::register_explicit::<replay_control_app::server_fns::ClearThumbnailIndex>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetGithubApiKey>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::SaveGithubApiKey>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetBoxartVariants>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::SetBoxartOverride>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::ResetBoxartOverride>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetRelatedGames>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::RescanGameLibrary>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::RebuildGameLibrary>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::RebuildCorruptLibrary>(
        );
        server_fn::axum::register_explicit::<replay_control_app::server_fns::RepairCorruptUserData>(
        );
        server_fn::axum::register_explicit::<replay_control_app::server_fns::RestoreUserDataBackup>(
        );
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetGameDocuments>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetLocalManuals>();
        server_fn::axum::register_explicit::<
            replay_control_app::server_fns::GetGameManualSuggestions,
        >();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::DownloadManual>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::DeleteManual>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetLanguagePreference>(
        );
        server_fn::axum::register_explicit::<replay_control_app::server_fns::SaveLanguagePreference>(
        );
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetPreferredLanguages>(
        );
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetLocale>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::SaveLocale>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::CheckForUpdates>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetUpdateChannel>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::SaveUpdateChannel>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::SkipVersion>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::StartUpdate>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetAnalyticsPreference>(
        );
        server_fn::axum::register_explicit::<replay_control_app::server_fns::SaveAnalyticsPreference>(
        );
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetSetupStatus>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::DismissSetup>();
        server_fn::axum::register_explicit::<StartSetupMetadataDownloads>();
        server_fn::axum::register_explicit::<EnableReplayApiAssisted>();
        server_fn::axum::register_explicit::<GetReplayApiStatus>();
        server_fn::axum::register_explicit::<GetLibraryPlaytime>();
        server_fn::axum::register_explicit::<GetGamePlaytime>();
        server_fn::axum::register_explicit::<GetReplayosSettings>();
        server_fn::axum::register_explicit::<PowerOffReplayosDevice>();
        server_fn::axum::register_explicit::<ReprobeReplayApi>();
        server_fn::axum::register_explicit::<RestartReplayosGame>();
        server_fn::axum::register_explicit::<SaveReplayosKioskMode>();
        server_fn::axum::register_explicit::<GetSaveStateSlots>();
        server_fn::axum::register_explicit::<SendReplayPlayerCommand>();
        server_fn::axum::register_explicit::<SendReplayosMessage>();
        server_fn::axum::register_explicit::<VerifyReplayApiToken>();
        server_fn::axum::register_explicit::<
            replay_control_app::server_fns::GetMetadataLibraryOverview,
        >();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetMetadataPageSnapshot>(
        );
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetGameStatus>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::SetGameStatus>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::ClearGameStatus>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetGamesByStatus>();
        let site_root_abs = std::fs::canonicalize(&cli.site_root).unwrap_or_else(|e| {
            panic!("site root '{}' not found: {e}", cli.site_root);
        });
        let hash_file_path = site_root_abs.join("hash.txt");
        let leptos_options = LeptosOptions::builder()
            .output_name("replay_control_app")
            .site_root(cli.site_root.clone())
            .site_pkg_dir("static/pkg")
            .hash_files(true)
            .hash_file(std::sync::Arc::from(
                hash_file_path.to_string_lossy().as_ref(),
            ))
            .build();

        let site_root = cli.site_root.clone();

        // Media handler: serves images from <storage>/.replay-control/media/<system>/<kind>/<file>
        let media_state = app_state.clone();
        let media_handler = axum::routing::get(
            move |axum::extract::Path(path): axum::extract::Path<String>,
                  headers: axum::http::HeaderMap| {
                let state = media_state.clone();
                async move {
                    use axum::http::StatusCode;
                    use axum::response::IntoResponse;

                    // Prevent path traversal (check segments, not substrings —
                    // filenames like "MASTER VER..png" contain ".." legitimately).
                    if path.split('/').any(|s| s == "..") {
                        return StatusCode::BAD_REQUEST.into_response();
                    }

                    let file_path = state.storage().rc_dir().join("media").join(&path);

                    let content_type = if path.ends_with(".png") {
                        "image/png"
                    } else if path.ends_with(".jpg") || path.ends_with(".jpeg") {
                        "image/jpeg"
                    } else {
                        "application/octet-stream"
                    };

                    serve_file_etagged(&file_path, content_type, &headers, api::CACHE_PRIVATE_1D)
                        .await
                }
            },
        );

        // Captures handler: serves user screenshots from <storage>/captures/<system>/<file>
        let captures_state = app_state.clone();
        let captures_handler = axum::routing::get(
            move |axum::extract::Path(path): axum::extract::Path<String>| {
                let state = captures_state.clone();
                async move {
                    use axum::http::StatusCode;
                    use axum::response::IntoResponse;

                    if path.split('/').any(|s| s == "..") {
                        return StatusCode::BAD_REQUEST.into_response();
                    }

                    let storage = state.storage();
                    let file_path = storage.captures_dir().join(&path);

                    match tokio::fs::read(&file_path).await {
                        Ok(data) => (
                            StatusCode::OK,
                            [
                                ("content-type", "image/png"),
                                ("cache-control", api::CACHE_PRIVATE_IMMUTABLE),
                            ],
                            data,
                        )
                            .into_response(),
                        Err(_) => StatusCode::NOT_FOUND.into_response(),
                    }
                }
            },
        );

        // Manuals handler: serves downloaded PDFs from <storage>/manuals/<system>/<file>
        let manuals_state = app_state.clone();
        let manuals_handler = axum::routing::get(
            move |axum::extract::Path(path): axum::extract::Path<String>| {
                let state = manuals_state.clone();
                async move {
                    use axum::http::StatusCode;
                    use axum::response::IntoResponse;

                    if path.split('/').any(|s| s == "..") {
                        return StatusCode::BAD_REQUEST.into_response();
                    }

                    let file_path = state.storage().manuals_dir().join(&path);

                    match tokio::fs::read(&file_path).await {
                        Ok(data) => {
                            let content_type = if path.ends_with(".pdf") {
                                "application/pdf"
                            } else if path.ends_with(".txt") {
                                "text/plain; charset=utf-8"
                            } else {
                                "application/octet-stream"
                            };
                            (
                                StatusCode::OK,
                                [
                                    ("content-type", content_type),
                                    ("cache-control", api::CACHE_PRIVATE_1D),
                                ],
                                data,
                            )
                                .into_response()
                        }
                        Err(_) => StatusCode::NOT_FOUND.into_response(),
                    }
                }
            },
        );

        // Owned manuals handler: serves saved/uploaded manuals from
        // <storage>/.replay-control/manuals/<system>/<file>.
        let owned_manuals_state = app_state.clone();
        let owned_manuals_handler = axum::routing::get(
            move |axum::extract::Path(path): axum::extract::Path<String>| {
                let state = owned_manuals_state.clone();
                async move {
                    use axum::http::StatusCode;
                    use axum::response::IntoResponse;

                    if path.split('/').any(|s| s == "..") {
                        return StatusCode::BAD_REQUEST.into_response();
                    }

                    let file_path = state.storage().rc_dir().join("manuals").join(&path);
                    match tokio::fs::read(&file_path).await {
                        Ok(data) => {
                            let content_type = if path.ends_with(".pdf") {
                                "application/pdf"
                            } else if path.ends_with(".txt") {
                                "text/plain; charset=utf-8"
                            } else {
                                "application/octet-stream"
                            };
                            (
                                StatusCode::OK,
                                [
                                    ("content-type", content_type),
                                    ("cache-control", api::CACHE_PRIVATE_1D),
                                ],
                                data,
                            )
                                .into_response()
                        }
                        Err(_) => StatusCode::NOT_FOUND.into_response(),
                    }
                }
            },
        );

        let rom_docs_state = app_state.clone();
        let rom_docs_handler = axum::routing::get(
            move |axum::extract::Path(path): axum::extract::Path<String>,
                  headers: axum::http::HeaderMap| {
                let state = rom_docs_state.clone();
                async move { serve_rom_doc(state, path, headers).await }
            },
        );

        // SSE endpoint for activity progress (broadcast-based, metadata page only).
        let sse_state = app_state.clone();
        let activity_sse_handler = axum::routing::get(move || {
            let state = sse_state.clone();
            async move { sse_activity_stream(state) }
        });

        // SSE endpoint for config changes (broadcast-based, always open from app shell).
        let config_sse_state = app_state.clone();
        let config_sse_handler = axum::routing::get(move || {
            let state = config_sse_state.clone();
            async move { sse_config_stream(state) }
        });
        let now_playing_sse_state = app_state.clone();
        let now_playing_sse_handler = axum::routing::get(move || {
            let state = now_playing_sse_state.clone();
            async move { sse_now_playing_stream(state) }
        });
        let events_sse_state = app_state.clone();
        let events_sse_handler = axum::routing::get(move || {
            let state = events_sse_state.clone();
            async move { sse_events_stream(state) }
        });

        // Clone state for the storage guard middleware before build_router consumes it.
        let guard_state = app_state.clone();
        let http_media_handler = media_handler.clone();
        let http_captures_handler = captures_handler.clone();
        let http_manuals_handler = manuals_handler.clone();
        let http_owned_manuals_handler = owned_manuals_handler.clone();
        let http_rom_docs_handler = rom_docs_handler.clone();

        let app = api::build_router(app_state, leptos_options)
            // DEPRECATED: Remove /more redirects in next-next beta release
            // Redirect legacy /more/* routes to /settings/*
            .route(
                "/more",
                axum::routing::get(|| async { axum::response::Redirect::permanent("/settings") }),
            )
            .route(
                "/more/*rest",
                axum::routing::get(
                    |axum::extract::Path(rest): axum::extract::Path<String>| async move {
                        axum::response::Redirect::permanent(&format!("/settings/{rest}"))
                    },
                ),
            )
            .route(
                "/api/version",
                axum::routing::get(|| async {
                    axum::Json(serde_json::json!({
                        "version": replay_control_app::VERSION,
                        "git_hash": replay_control_app::GIT_HASH,
                    }))
                }),
            )
            .route("/sse/activity", activity_sse_handler)
            .route("/sse/config", config_sse_handler)
            .route("/sse/now-playing", now_playing_sse_handler)
            .route("/sse/events", events_sse_handler)
            .route("/captures/*path", captures_handler)
            .route("/manuals/*path", manuals_handler)
            .route("/owned-manuals/*path", owned_manuals_handler)
            .route("/rom-docs/*path", rom_docs_handler)
            .route("/media/*path", media_handler)
            .nest_service(
                "/static/pkg/snippets",
                tower::ServiceBuilder::new()
                    .layer(SetResponseHeaderLayer::overriding(
                        http::header::CACHE_CONTROL,
                        http::HeaderValue::from_static(api::CACHE_REVALIDATE),
                    ))
                    .service(
                        ServeDir::new(format!("{site_root}/pkg/snippets"))
                            .precompressed_br()
                            .precompressed_gzip(),
                    ),
            )
            .nest_service(
                "/static/pkg",
                tower::ServiceBuilder::new()
                    .layer(SetResponseHeaderLayer::overriding(
                        http::header::CACHE_CONTROL,
                        http::HeaderValue::from_static(api::CACHE_IMMUTABLE),
                    ))
                    .service(
                        ServeDir::new(format!("{site_root}/pkg"))
                            .precompressed_br()
                            .precompressed_gzip(),
                    ),
            )
            .nest_service(
                "/static/icons",
                tower::ServiceBuilder::new()
                    .layer(SetResponseHeaderLayer::overriding(
                        http::header::CACHE_CONTROL,
                        http::HeaderValue::from_static(api::CACHE_1D),
                    ))
                    .service(ServeDir::new(format!("{site_root}/icons"))),
            )
            .nest_service(
                "/static/branding",
                tower::ServiceBuilder::new()
                    .layer(SetResponseHeaderLayer::overriding(
                        http::header::CACHE_CONTROL,
                        http::HeaderValue::from_static(api::CACHE_1D),
                    ))
                    .service(ServeDir::new(format!("{site_root}/branding"))),
            )
            .route(
                "/static/manifest.json",
                axum::routing::get(|| async {
                    (
                        [
                            ("content-type", "application/manifest+json"),
                            ("cache-control", api::CACHE_1H),
                        ],
                        include_str!("../static/manifest.json"),
                    )
                }),
            )
            .route(
                "/static/ptr-init.js",
                axum::routing::get(|| async {
                    (
                        [
                            ("content-type", "application/javascript"),
                            ("cache-control", api::CACHE_1H),
                        ],
                        include_str!("../static/ptr-init.js"),
                    )
                }),
            )
            .route(
                "/static/pulltorefresh.min.js",
                axum::routing::get(|| async {
                    (
                        [
                            ("content-type", "application/javascript"),
                            ("cache-control", api::CACHE_1H),
                        ],
                        include_str!("../static/pulltorefresh.min.js"),
                    )
                }),
            )
            .layer(CompressionLayer::new().gzip(true));

        let app = api::with_storage_guard(app, guard_state.clone());
        let app = api::with_auth_guard(app, guard_state.clone());

        if !serve_https {
            let addr = SocketAddr::from(([0, 0, 0, 0], cli.port));
            if device_mode {
                tracing::warn!(
                    "dangerous_disable_https is set; serving Replay Control over plain HTTP on {addr}"
                );
            } else {
                tracing::info!("Starting standalone Replay Control on http://{addr}");
            }
            let listener = tokio::net::TcpListener::bind(addr)
                .await
                .unwrap_or_else(|error| {
                    tracing::error!("failed to bind HTTP listener on {addr}: {error}");
                    std::process::exit(1);
                });
            if let Err(error) = axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            {
                tracing::error!("HTTP server stopped with error: {error}");
                std::process::exit(1);
            }
            return;
        }

        let certificate_paths = ensure_self_signed_certificate(&guard_state.data_dir)
            .unwrap_or_else(|error| {
                tracing::error!("failed to prepare HTTPS certificate: {error}");
                std::process::exit(1);
            });
        let tls_config = match RustlsConfig::from_pem_file(
            &certificate_paths.cert,
            &certificate_paths.key,
        )
        .await
        {
            Ok(config) => config,
            Err(error) => {
                tracing::warn!(
                    "failed to load HTTPS certificate, regenerating self-signed certificate: {error}"
                );
                let regenerated_paths = regenerate_self_signed_certificate(&guard_state.data_dir)
                    .unwrap_or_else(|error| {
                        tracing::error!("failed to regenerate HTTPS certificate: {error}");
                        std::process::exit(1);
                    });
                RustlsConfig::from_pem_file(&regenerated_paths.cert, &regenerated_paths.key)
                    .await
                    .unwrap_or_else(|error| {
                        tracing::error!("failed to load regenerated HTTPS certificate: {error}");
                        std::process::exit(1);
                    })
            }
        };

        let http_addr = SocketAddr::from(([0, 0, 0, 0], cli.port));
        let https_addr = SocketAddr::from(([0, 0, 0, 0], cli.https_port));
        let http_app = http_guidance_router(
            cli.https_port,
            guard_state.clone(),
            http_media_handler,
            http_captures_handler,
            http_manuals_handler,
            http_owned_manuals_handler,
            http_rom_docs_handler,
        );

        tracing::info!("Starting HTTPS app on https://0.0.0.0:{}", cli.https_port);
        tracing::info!(
            "HTTP guidance page available on http://0.0.0.0:{}",
            cli.port
        );
        if device_mode {
            tracing::info!(
                "Try https://replay.local:{} or use this device's LAN IP address",
                cli.https_port
            );
        } else {
            tracing::info!(
                "Standalone HTTPS was enabled explicitly; open https://localhost:{}",
                cli.https_port
            );
        }

        let http_listener = tokio::net::TcpListener::bind(http_addr)
            .await
            .unwrap_or_else(|error| {
                tracing::error!("failed to bind HTTP guidance listener on {http_addr}: {error}");
                std::process::exit(1);
            });
        let http_server = axum::serve(
            http_listener,
            http_app.into_make_service_with_connect_info::<SocketAddr>(),
        );
        let https_server = axum_server::bind_rustls(https_addr, tls_config)
            .serve(app.into_make_service_with_connect_info::<SocketAddr>());

        tokio::select! {
            result = http_server => {
                if let Err(error) = result {
                    tracing::error!("HTTP guidance server stopped with error: {error}");
                    std::process::exit(1);
                }
            }
            result = https_server => {
                if let Err(error) = result {
                    tracing::error!("HTTPS server stopped with error: {error}");
                    std::process::exit(1);
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{
            https_enabled, https_url_for_request, request_peer_is_loopback,
            validated_host_without_port,
        };
        use axum::body::Body;
        use axum::extract::connect_info::ConnectInfo;
        use axum::http::{HeaderMap, HeaderValue, Request, header};
        use std::net::SocketAddr;

        #[test]
        fn device_mode_enables_https_by_default() {
            assert!(https_enabled(false, false, true));
        }

        #[test]
        fn standalone_mode_uses_plain_http_by_default() {
            assert!(!https_enabled(false, false, false));
        }

        #[test]
        fn standalone_mode_can_opt_into_https() {
            assert!(https_enabled(true, false, false));
        }

        #[test]
        fn dangerous_disable_https_overrides_device_default_and_standalone_opt_in() {
            assert!(!https_enabled(false, true, true));
            assert!(!https_enabled(true, true, false));
        }

        #[test]
        fn builds_https_url_from_http_host_header() {
            let mut headers = HeaderMap::new();
            headers.insert(header::HOST, HeaderValue::from_static("replay.local:8080"));

            assert_eq!(
                https_url_for_request(&headers, 8443),
                "https://replay.local:8443/"
            );
        }

        #[test]
        fn validates_hosts_before_using_them_in_guidance_page() {
            assert_eq!(
                validated_host_without_port("192.168.1.30:8080").as_deref(),
                Some("192.168.1.30")
            );
            assert_eq!(
                validated_host_without_port("[fd00::1]:8080").as_deref(),
                Some("[fd00::1]")
            );
            assert_eq!(
                validated_host_without_port("replay.local").as_deref(),
                Some("replay.local")
            );
            assert_eq!(validated_host_without_port("[fd00::1]:bad"), None);
            assert_eq!(validated_host_without_port("bad\"><script>"), None);
        }

        #[test]
        fn hostile_host_header_falls_back_to_replay_local() {
            let mut headers = HeaderMap::new();
            headers.insert(
                header::HOST,
                HeaderValue::from_str("bad\"><script>").unwrap(),
            );

            assert_eq!(
                https_url_for_request(&headers, 8443),
                "https://replay.local:8443/"
            );
        }

        #[test]
        fn loopback_compatibility_uses_socket_peer_not_headers() {
            let mut loopback = Request::builder()
                .uri("/api/core/recents")
                .body(Body::empty())
                .unwrap();
            loopback
                .extensions_mut()
                .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 8080))));
            assert!(request_peer_is_loopback(&loopback));

            let mut lan = Request::builder()
                .uri("/api/core/recents")
                .body(Body::empty())
                .unwrap();
            lan.extensions_mut()
                .insert(ConnectInfo(SocketAddr::from(([192, 168, 1, 40], 8080))));
            assert!(!request_peer_is_loopback(&lan));

            let spoofed = Request::builder()
                .uri("/api/core/recents")
                .header(header::HOST, "127.0.0.1:8080")
                .header("x-forwarded-for", "127.0.0.1")
                .body(Body::empty())
                .unwrap();
            assert!(!request_peer_is_loopback(&spoofed));
        }
    }
}

#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() {
    ssr::run().await;
}

#[cfg(not(feature = "ssr"))]
fn main() {
    // WASM entry point is the hydrate() function in lib.rs
}
