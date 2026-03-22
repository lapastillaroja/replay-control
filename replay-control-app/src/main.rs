#![recursion_limit = "512"]

#[cfg(feature = "ssr")]
mod ssr {
    use clap::Parser;
    use leptos::config::LeptosOptions;
    use tower_http::compression::CompressionLayer;
    use tower_http::cors::CorsLayer;
    use tower_http::services::ServeDir;

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

        match tokio::fs::read(&file_path).await {
            Ok(data) => {
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

    pub async fn run() {
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

        let app_state = match api::AppState::new(cli.storage_path, cli.config_path) {
            Ok(state) => state,
            Err(e) => {
                tracing::error!("Failed to initialize: {e}");
                tracing::info!("Hint: use --storage-path to point to a RePlayOS storage location");
                std::process::exit(1);
            }
        };

        // Start the ordered background pipeline (auto-import → cache verify →
        // enrichment) and filesystem watchers.
        api::BackgroundManager::start(app_state.clone());

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
        server_fn::axum::register_explicit::<replay_control_app::server_fns::IsMetadataBusy>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetBusyLabel>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetMetadataStats>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::ImportLaunchboxMetadata>(
        );
        server_fn::axum::register_explicit::<replay_control_app::server_fns::ClearMetadata>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::RegenerateMetadata>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::DownloadMetadata>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetImportProgress>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetSystemCoverage>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetImageStats>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::ClearImages>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::CleanupOrphanedImages>(
        );
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetSystemLogs>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetGameVideos>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::AddGameVideo>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::RemoveGameVideo>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::SearchGameVideos>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GlobalSearch>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetAllGenres>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetSystemGenres>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::RandomGame>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::SearchByDeveloper>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetDeveloperGames>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetDeveloperGenres>();
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
        server_fn::axum::register_explicit::<replay_control_app::server_fns::UpdateThumbnails>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::CancelThumbnailUpdate>(
        );
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetThumbnailProgress>(
        );
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetThumbnailDataSource>(
        );
        server_fn::axum::register_explicit::<replay_control_app::server_fns::ClearThumbnailIndex>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetGithubApiKey>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::SaveGithubApiKey>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetBoxartVariants>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::SetBoxartOverride>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::ResetBoxartOverride>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetRelatedGames>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::RebuildGameLibrary>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetBuiltinDbStats>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::IsScanning>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetGameDocuments>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetLocalManuals>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::SearchGameManuals>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::DownloadManual>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::DeleteManual>();
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetLanguagePreference>(
        );
        server_fn::axum::register_explicit::<replay_control_app::server_fns::SaveLanguagePreference>(
        );
        server_fn::axum::register_explicit::<replay_control_app::server_fns::GetPreferredLanguages>(
        );

        let leptos_options = LeptosOptions::builder()
            .output_name("replay_control_app")
            .site_root(cli.site_root.clone())
            .site_pkg_dir("pkg")
            .build();

        let site_root = cli.site_root.clone();

        // Media handler: serves images from <storage>/.replay-control/media/<system>/<kind>/<file>
        let media_state = app_state.clone();
        let media_handler = axum::routing::get(
            move |axum::extract::Path(path): axum::extract::Path<String>| {
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
                                ("cache-control", "public, max-age=31536000, immutable"),
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

        let rom_docs_state = app_state.clone();
        let rom_docs_handler = axum::routing::get(
            move |axum::extract::Path(path): axum::extract::Path<String>| {
                let state = rom_docs_state.clone();
                async move { serve_rom_doc(state, path).await }
            },
        );

        // SSE endpoint for real-time metadata import progress.
        // Same pattern as image progress: 200ms interval, idle counter, auto-close.
        let metadata_sse_state = app_state.clone();
        let metadata_sse_handler = axum::routing::get(move || {
            let state = metadata_sse_state.clone();
            async move {
                use axum::response::sse::{Event, Sse};
                use std::convert::Infallible;
                use tokio_stream::StreamExt;

                let import = state.import.clone();
                let idle_count = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));

                let stream = tokio_stream::wrappers::IntervalStream::new(tokio::time::interval(
                    std::time::Duration::from_millis(200),
                ))
                .map({
                    let idle_count = idle_count.clone();
                    move |_| {
                        let progress = import.progress();
                        let is_active = progress.as_ref().is_some_and(|p| {
                            use replay_control_app::server_fns::ImportState;
                            matches!(
                                p.state,
                                ImportState::Downloading
                                    | ImportState::BuildingIndex
                                    | ImportState::Parsing
                            )
                        });
                        let json = match &progress {
                            Some(p) => serde_json::to_string(p).unwrap_or_default(),
                            None => "null".to_string(),
                        };

                        if is_active {
                            idle_count.store(0, std::sync::atomic::Ordering::Relaxed);
                        } else {
                            idle_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        }
                        Ok::<_, Infallible>(Event::default().data(json))
                    }
                })
                // Close stream after 5 consecutive idle ticks (1s of no active operation).
                .take_while({
                    let idle_count = idle_count.clone();
                    move |_| idle_count.load(std::sync::atomic::Ordering::Relaxed) <= 5
                });

                Sse::new(stream).keep_alive(
                    axum::response::sse::KeepAlive::new()
                        .interval(std::time::Duration::from_secs(15)),
                )
            }
        });

        // SSE endpoint for real-time thumbnail update progress.
        let thumbnail_sse_state = app_state.clone();
        let thumbnail_sse_handler = axum::routing::get(move || {
            let state = thumbnail_sse_state.clone();
            async move {
                use axum::response::sse::{Event, Sse};
                use std::convert::Infallible;
                use tokio_stream::StreamExt;

                let thumbnails = state.thumbnails.clone();
                let idle_count = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));

                let stream = tokio_stream::wrappers::IntervalStream::new(tokio::time::interval(
                    std::time::Duration::from_millis(200),
                ))
                .map({
                    let idle_count = idle_count.clone();
                    move |_| {
                        let progress = thumbnails.progress();
                        let is_active = progress.as_ref().is_some_and(|p| {
                            use replay_control_app::server_fns::ThumbnailPhase;
                            matches!(
                                p.phase,
                                ThumbnailPhase::Indexing | ThumbnailPhase::Downloading
                            )
                        });
                        let json = match &progress {
                            Some(p) => serde_json::to_string(p).unwrap_or_default(),
                            None => "null".to_string(),
                        };

                        if is_active {
                            idle_count.store(0, std::sync::atomic::Ordering::Relaxed);
                        } else {
                            idle_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        }
                        Ok::<_, Infallible>(Event::default().data(json))
                    }
                })
                .take_while({
                    let idle_count = idle_count.clone();
                    move |_| idle_count.load(std::sync::atomic::Ordering::Relaxed) <= 5
                });

                Sse::new(stream).keep_alive(
                    axum::response::sse::KeepAlive::new()
                        .interval(std::time::Duration::from_secs(15)),
                )
            }
        });

        let app = api::build_router(app_state, leptos_options)
            .route("/sse/metadata-progress", metadata_sse_handler)
            .route("/sse/thumbnail-progress", thumbnail_sse_handler)
            .route("/captures/*path", captures_handler)
            .route("/manuals/*path", manuals_handler)
            .route("/rom-docs/*path", rom_docs_handler)
            .route("/media/*path", media_handler)
            .nest_service(
                "/pkg",
                ServeDir::new(format!("{site_root}/pkg")).precompressed_gzip(),
            )
            .nest_service("/icons", ServeDir::new(format!("{site_root}/icons")))
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
            .route(
                "/ptr-init.js",
                axum::routing::get(|| async {
                    (
                        [("content-type", "application/javascript")],
                        include_str!("../static/ptr-init.js"),
                    )
                }),
            )
            .route(
                "/pulltorefresh.min.js",
                axum::routing::get(|| async {
                    (
                        [("content-type", "application/javascript")],
                        include_str!("../static/pulltorefresh.min.js"),
                    )
                }),
            )
            .layer(CompressionLayer::new().gzip(true))
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
