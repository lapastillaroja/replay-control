#![recursion_limit = "512"]

#[cfg(feature = "ssr")]
mod ssr {
    use axum::Router;
    use clap::Parser;
    use leptos::config::LeptosOptions;
    use leptos::prelude::*;
    use tower_http::cors::CorsLayer;
    use tower_http::services::ServeDir;

    use replay_control_app::Shell;
    use replay_control_app::api;

    #[derive(Parser)]
    #[command(
        name = "replay-control-app",
        about = "Replay Control — companion app for RePlayOS"
    )]
    struct Cli {
        /// Port to listen on
        #[arg(short, long, default_value = "8080")]
        port: u16,

        /// Storage root path override (auto-detected if not set)
        #[arg(long)]
        storage_path: Option<String>,

        /// Path to replay.cfg (auto-detected if not set)
        #[arg(long)]
        config_path: Option<String>,

        /// Path to the site root (where pkg/ and style.css live)
        #[arg(long, default_value = "target/site")]
        site_root: String,
    }

    pub async fn run() {
        tracing_subscriber::fmt()
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

        let app_state = match api::AppState::new(cli.storage_path, cli.config_path) {
            Ok(state) => state,
            Err(e) => {
                tracing::error!("Failed to initialize: {e}");
                tracing::info!("Hint: use --storage-path to point to a RePlayOS storage location");
                std::process::exit(1);
            }
        };

        // Spawn background storage re-detection task.
        app_state.clone().spawn_storage_watcher();

        // Auto-import metadata if Metadata.xml exists and DB is empty.
        app_state.spawn_auto_import();

        // Explicitly register all server functions (inventory auto-registration
        // doesn't work when the functions are in a library crate).
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetInfo>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetSystems>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetFavorites>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetRecents>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::AddFavorite>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::RemoveFavorite>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GroupFavorites>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::FlattenFavorites>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetRomsPage>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetSystemFavorites>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::DeleteRom>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::RenameRom>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::LaunchGame>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetRomDetail>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::RefreshStorage>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetWifiConfig>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::SaveWifiConfig>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetNfsConfig>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::SaveNfsConfig>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::RestartReplayUi>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::RebootSystem>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::OrganizeFavorites>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetSkins>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::SetSkin>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::SetSkinSync>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetHostname>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::SaveHostname>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetMetadataStats>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::ImportLaunchboxMetadata>(
        );
        server_fn::axum::register_explicit::<replay_control_app::server_fns::ClearMetadata>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::RegenerateMetadata>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::DownloadMetadata>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetImportProgress>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetSystemCoverage>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::ImportSystemImages>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::ImportAllImages>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetImageImportProgress>(
        );
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetImageCoverage>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetImageStats>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::ClearImages>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetSystemLogs>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::CancelImageImport>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::RematchAllImages>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetGameVideos>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::AddGameVideo>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::RemoveGameVideo>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::SearchGameVideos>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GlobalSearch>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetAllGenres>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetSystemGenres>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::RandomGame>();

        let leptos_options = LeptosOptions::builder()
            .output_name("replay_control_app")
            .site_root(cli.site_root.clone())
            .site_pkg_dir("pkg")
            .build();

        // REST API (kept for external access)
        let api_routes = Router::new()
            .merge(api::system_info::routes())
            .merge(api::roms::routes())
            .merge(api::favorites::routes())
            .merge(api::upload::routes())
            .merge(api::recents::routes());

        let state_for_ssr = app_state.clone();
        let opts_for_ssr = leptos_options.clone();

        let ssr_handler = leptos_axum::render_app_to_stream_with_context(
            move || {
                provide_context(state_for_ssr.clone());
            },
            move || {
                let opts = opts_for_ssr.clone();
                view! { <Shell options=opts /> }
            },
        );

        // Server function handler for client-side calls after hydration.
        let state_for_sfn = app_state.clone();

        let site_root = cli.site_root.clone();

        // Media handler: serves images from <storage>/.replay-control/media/<system>/<kind>/<file>
        let media_state = app_state.clone();
        let media_handler = axum::routing::get(
            move |axum::extract::Path(path): axum::extract::Path<String>| {
                let state = media_state.clone();
                async move {
                    use axum::http::StatusCode;
                    use axum::response::IntoResponse;

                    // Prevent path traversal.
                    if path.contains("..") {
                        return StatusCode::BAD_REQUEST.into_response();
                    }

                    let storage = state.storage();
                    let file_path = storage
                        .root
                        .join(replay_control_core::metadata_db::RC_DIR)
                        .join("media")
                        .join(&path);

                    match tokio::fs::read(&file_path).await {
                        Ok(data) => {
                            let content_type = if path.ends_with(".png") {
                                "image/png"
                            } else if path.ends_with(".jpg") || path.ends_with(".jpeg") {
                                "image/jpeg"
                            } else {
                                "application/octet-stream"
                            };
                            (
                                StatusCode::OK,
                                [
                                    ("content-type", content_type),
                                    ("cache-control", "public, max-age=86400"),
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

        // SSE endpoint for real-time image import progress.
        let sse_state = app_state.clone();
        let sse_handler = axum::routing::get(move || {
            let state = sse_state.clone();
            async move {
                use axum::response::sse::{Event, Sse};
                use std::convert::Infallible;
                use tokio_stream::StreamExt;

                let stream = tokio_stream::wrappers::IntervalStream::new(tokio::time::interval(
                    std::time::Duration::from_millis(200),
                ))
                .map(move |_| {
                    let guard = state.image_import_progress.read().expect("lock");
                    let json = match &*guard {
                        Some(p) => serde_json::to_string(p).unwrap_or_default(),
                        None => "null".to_string(),
                    };
                    Ok::<_, Infallible>(Event::default().data(json))
                });

                Sse::new(stream).keep_alive(
                    axum::response::sse::KeepAlive::new()
                        .interval(std::time::Duration::from_secs(15)),
                )
            }
        });

        // Captures handler: serves user screenshots from <storage>/captures/<system>/<file>
        let captures_state = app_state.clone();
        let captures_handler = axum::routing::get(
            move |axum::extract::Path(path): axum::extract::Path<String>| {
                let state = captures_state.clone();
                async move {
                    use axum::http::StatusCode;
                    use axum::response::IntoResponse;

                    if path.contains("..") {
                        return StatusCode::BAD_REQUEST.into_response();
                    }

                    let storage = state.storage();
                    let file_path = storage.captures_dir().join(&path);

                    match tokio::fs::read(&file_path).await {
                        Ok(data) => (
                            StatusCode::OK,
                            [
                                ("content-type", "image/png"),
                                (
                                    "cache-control",
                                    "public, max-age=31536000, immutable",
                                ),
                            ],
                            data,
                        )
                            .into_response(),
                        Err(_) => StatusCode::NOT_FOUND.into_response(),
                    }
                }
            },
        );

        let app = Router::new()
            .nest("/api", api_routes)
            .route("/sse/image-progress", sse_handler)
            .route("/captures/*path", captures_handler)
            .route("/media/*path", media_handler)
            .route(
                "/sfn/*fn_name",
                axum::routing::post(move |req: axum::http::Request<axum::body::Body>| {
                    let state = state_for_sfn.clone();
                    async move {
                        let ctx_state = state.clone();
                        leptos_axum::handle_server_fns_with_context(
                            move || provide_context(ctx_state.clone()),
                            req,
                        )
                        .await
                    }
                }),
            )
            .nest_service("/pkg", ServeDir::new(format!("{site_root}/pkg")))
            .nest_service("/icons", ServeDir::new(format!("{site_root}/icons")))
            .route(
                "/style.css",
                axum::routing::get(|| async {
                    (
                        [("content-type", "text/css")],
                        include_str!("../style/style.css"),
                    )
                }),
            )
            .route(
                "/manifest.json",
                axum::routing::get(|| async {
                    (
                        [("content-type", "application/manifest+json")],
                        include_str!("../static/manifest.json"),
                    )
                }),
            )
            .route(
                "/sw.js",
                axum::routing::get(|| async {
                    (
                        [("content-type", "application/javascript")],
                        include_str!("../static/sw.js"),
                    )
                }),
            )
            .fallback(ssr_handler)
            .with_state(app_state)
            .layer(CorsLayer::permissive());

        let addr = format!("0.0.0.0:{}", cli.port);
        tracing::info!("Starting server on {addr}");

        let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
        axum::serve(listener, app).await.unwrap();
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
