#[cfg(feature = "ssr")]
mod ssr {
    use axum::Router;
    use clap::Parser;
    use leptos::config::LeptosOptions;
    use leptos::prelude::*;
    use tower_http::cors::CorsLayer;
    use tower_http::services::ServeDir;

    use replay_app::api;
    use replay_app::Shell;

    #[derive(Parser)]
    #[command(name = "replay-app", about = "Replay Control — companion app for RePlayOS")]
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
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "replay_app=info,replay_core=info".parse().unwrap()),
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
                tracing::info!(
                    "Hint: use --storage-path to point to a RePlayOS storage location"
                );
                std::process::exit(1);
            }
        };

        // Spawn background storage re-detection task.
        app_state.clone().spawn_storage_watcher();

        // Explicitly register all server functions (inventory auto-registration
        // doesn't work when the functions are in a library crate).
        server_fn::axum::register_explicit::<replay_app::server_fns::GetInfo>();
        server_fn::axum::register_explicit::<replay_app::server_fns::GetSystems>();
        server_fn::axum::register_explicit::<replay_app::server_fns::GetFavorites>();
        server_fn::axum::register_explicit::<replay_app::server_fns::GetRecents>();
        server_fn::axum::register_explicit::<replay_app::server_fns::AddFavorite>();
        server_fn::axum::register_explicit::<replay_app::server_fns::RemoveFavorite>();
        server_fn::axum::register_explicit::<replay_app::server_fns::GroupFavorites>();
        server_fn::axum::register_explicit::<replay_app::server_fns::FlattenFavorites>();
        server_fn::axum::register_explicit::<replay_app::server_fns::GetRomsPage>();
        server_fn::axum::register_explicit::<replay_app::server_fns::GetSystemFavorites>();
        server_fn::axum::register_explicit::<replay_app::server_fns::DeleteRom>();
        server_fn::axum::register_explicit::<replay_app::server_fns::RenameRom>();
        server_fn::axum::register_explicit::<replay_app::server_fns::GetRomDetail>();
        server_fn::axum::register_explicit::<replay_app::server_fns::RefreshStorage>();
        server_fn::axum::register_explicit::<replay_app::server_fns::GetWifiConfig>();
        server_fn::axum::register_explicit::<replay_app::server_fns::SaveWifiConfig>();
        server_fn::axum::register_explicit::<replay_app::server_fns::GetNfsConfig>();
        server_fn::axum::register_explicit::<replay_app::server_fns::SaveNfsConfig>();
        server_fn::axum::register_explicit::<replay_app::server_fns::RestartReplayUi>();
        server_fn::axum::register_explicit::<replay_app::server_fns::RebootSystem>();
        server_fn::axum::register_explicit::<replay_app::server_fns::OrganizeFavorites>();

        let leptos_options = LeptosOptions::builder()
            .output_name("replay_app")
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

        let app = Router::new()
            .nest("/api", api_routes)
            .route(
                "/sfn/*fn_name",
                axum::routing::post(
                    move |req: axum::http::Request<axum::body::Body>| {
                        let state = state_for_sfn.clone();
                        async move {
                            let ctx_state = state.clone();
                            leptos_axum::handle_server_fns_with_context(
                                move || provide_context(ctx_state.clone()),
                                req,
                            )
                            .await
                        }
                    },
                ),
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
