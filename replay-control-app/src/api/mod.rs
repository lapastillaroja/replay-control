pub mod activity;
pub mod analytics;
pub mod background;
pub(crate) mod core_api;
pub mod favorites;
pub mod import;
pub(crate) mod library;
pub mod recents;
pub mod response_cache;
pub mod roms;
pub mod system_info;
pub mod thumbnail_pipeline;
pub mod upload;

pub use activity::{Activity, ActivityGuard, MaintenanceKind, StartupPhase};
pub use background::BackgroundManager;
pub use import::ImportPipeline;
pub use library::LibraryService;
pub use replay_control_core_server::db_pool::{DbPool, WriteGate, rusqlite};
pub use thumbnail_pipeline::ThumbnailPipeline;

/// Cache-control header values for static asset responses.
pub const CACHE_1H: &str = "public, max-age=3600";
pub const CACHE_1D: &str = "public, max-age=86400";
pub const CACHE_IMMUTABLE: &str = "public, max-age=31536000, immutable";

use std::path::PathBuf;
use std::sync::Arc;

use replay_control_core_server::config::SystemConfig;
use replay_control_core_server::storage::{StorageKind, StorageLocation};

/// Config change events pushed to clients via the `/sse/config` broadcast channel.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(tag = "type")]
pub enum ConfigEvent {
    SkinChanged {
        skin_index: u32,
        skin_css: Option<String>,
    },
    StorageChanged {
        storage_kind: String,
    },
    UpdateAvailable {
        update: replay_control_core::update::AvailableUpdate,
    },
    CorruptionChanged {
        library_corrupt: bool,
        user_data_corrupt: bool,
        user_data_backup_exists: bool,
    },
}

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub storage: Arc<std::sync::RwLock<Option<StorageLocation>>>,
    pub config: Arc<std::sync::RwLock<SystemConfig>>,
    pub config_path: Option<PathBuf>,
    pub cache: Arc<LibraryService>,
    /// Response-level cache for assembled recommendation payloads.
    pub response_cache: Arc<response_cache::ResponseCache>,
    /// When set, --storage-path was given on the CLI and auto-detection is skipped.
    pub storage_path_override: Option<PathBuf>,
    /// Resolved settings store (owns the directory path for settings.cfg).
    pub settings: replay_control_core_server::settings::SettingsStore,
    /// Cached user preferences (skin, locale, region, font size).
    /// Loaded once at startup; updated in-memory on every settings change.
    pub prefs: Arc<std::sync::RwLock<replay_control_core_server::settings::UserPreferences>>,
    /// Library DB pool (deadpool-backed, concurrent reads).
    pub library_pool: DbPool,
    /// User data DB pool (deadpool-backed, concurrent reads).
    pub user_data_pool: DbPool,
    /// Import pipeline (metadata import operations).
    pub import: Arc<ImportPipeline>,
    /// Thumbnail pipeline (index + download operations).
    pub thumbnails: Arc<ThumbnailPipeline>,
    /// Track in-flight on-demand thumbnail downloads to avoid duplicates.
    pub pending_downloads: Arc<std::sync::RwLock<std::collections::HashSet<String>>>,
    /// Unified activity state: at most one activity at a time.
    /// Replaces `busy`, `busy_label`, `scanning`, and `rebuild_progress`.
    pub(crate) activity: Arc<std::sync::RwLock<Activity>>,
    /// Broadcast channel for config change notifications (skin, storage).
    pub config_tx: tokio::sync::broadcast::Sender<ConfigEvent>,
    /// Broadcast channel for activity state changes (import, thumbnail, rebuild).
    pub activity_tx: tokio::sync::broadcast::Sender<Activity>,
}

/// Register a corruption-change callback on each pool.
///
/// Both pools share a closure that reads the latest combined corruption state
/// and broadcasts `ConfigEvent::CorruptionChanged` on `config_tx`. The closure
/// captures atomic flag handles and the user-data path handle (no `DbPool`
/// clones), so there is no Pool ↔ callback reference cycle.
fn register_corruption_callbacks(
    library_pool: &DbPool,
    user_data_pool: &DbPool,
    config_tx: tokio::sync::broadcast::Sender<ConfigEvent>,
) {
    use std::sync::atomic::Ordering;

    let lib_flag = library_pool.corrupt_flag();
    let ud_flag = user_data_pool.corrupt_flag();
    let ud_path = user_data_pool.db_path_handle();

    let make_cb = || {
        let lib_flag = lib_flag.clone();
        let ud_flag = ud_flag.clone();
        let ud_path = ud_path.clone();
        let tx = config_tx.clone();
        move || {
            let _ = tx.send(ConfigEvent::CorruptionChanged {
                library_corrupt: lib_flag.load(Ordering::Relaxed),
                user_data_corrupt: ud_flag.load(Ordering::Relaxed),
                user_data_backup_exists: ud_path
                    .read()
                    .ok()
                    .map(|p| p.with_extension("db.bak").exists())
                    .unwrap_or(false),
            });
        }
    };

    library_pool.set_corruption_callback(make_cb());
    user_data_pool.set_corruption_callback(make_cb());
}

/// Opener for library DB.
fn open_library_db(
    storage_root: &std::path::Path,
) -> replay_control_core::error::Result<(rusqlite::Connection, PathBuf)> {
    replay_control_core_server::library_db::LibraryDb::open(storage_root)
}

/// Opener for user data DB.
fn open_user_data_db(
    storage_root: &std::path::Path,
) -> replay_control_core::error::Result<(rusqlite::Connection, PathBuf)> {
    let (conn, path, _corrupt) =
        replay_control_core_server::user_data_db::UserDataDb::open(storage_root)?;
    Ok((conn, path))
}

/// Resolve the settings directory from CLI arguments.
///
/// Priority:
/// 1. `--settings-path` explicit override
/// 2. `--storage-path` given -> `<storage>/.replay-control` (local dev backwards compat)
/// 3. Pi production fallback -> `/etc/replay-control`
fn resolve_settings_dir(
    settings_path: Option<&str>,
    storage_path: Option<&str>,
) -> replay_control_core_server::settings::SettingsStore {
    use replay_control_core_server::settings::SettingsStore;
    use replay_control_core_server::storage::RC_DIR;

    if let Some(p) = settings_path {
        return SettingsStore::new(p);
    }
    if let Some(s) = storage_path {
        return SettingsStore::new(PathBuf::from(s).join(RC_DIR));
    }
    SettingsStore::new("/etc/replay-control")
}

impl AppState {
    pub fn new(
        storage_path: Option<String>,
        config_path: Option<String>,
        settings_path: Option<String>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let config_path = config_path.map(PathBuf::from);
        let storage_path_override = storage_path.as_ref().map(PathBuf::from);

        let (storage, config) = if let Some(path) = storage_path {
            let storage_root = PathBuf::from(&path);
            if !storage_root.exists() {
                return Err(format!("Storage path does not exist: {path}").into());
            }

            let config = config_path
                .as_ref()
                .and_then(|p| SystemConfig::from_file(p).ok())
                .or_else(|| SystemConfig::from_file(&storage_root.join("config/replay.cfg")).ok())
                .unwrap_or_else(|| SystemConfig::parse("").unwrap());

            let kind = match config.storage_mode() {
                "usb" => StorageKind::Usb,
                "nvme" => StorageKind::Nvme,
                "nfs" => StorageKind::Nfs,
                _ => StorageKind::Sd,
            };

            (Some(StorageLocation::from_path(storage_root, kind)), config)
        } else {
            // Auto-detect: try to read config from default location (SD card, always available)
            let default_config = PathBuf::from("/media/sd/config/replay.cfg");
            let config = if default_config.exists() {
                SystemConfig::from_file(&default_config)?
            } else {
                SystemConfig::parse("")?
            };

            match StorageLocation::detect(&config) {
                Ok(storage) => (Some(storage), config),
                Err(e) => {
                    tracing::warn!("Storage unavailable at startup: {e}");
                    (None, config)
                }
            }
        };

        // Channels are constructed before the pools so the corruption
        // callbacks registered below can capture `config_tx`.
        let (config_tx, _) = tokio::sync::broadcast::channel::<ConfigEvent>(16);
        let (activity_tx, _) = tokio::sync::broadcast::channel::<Activity>(32);

        let (library_pool, user_data_pool) = if let Some(ref storage) = storage {
            tracing::info!("Storage: {:?} at {}", storage.kind, storage.root.display());

            // Open DBs eagerly at startup so they're ready for the first request.
            let (_meta_conn, meta_path) =
                replay_control_core_server::library_db::LibraryDb::open(&storage.root)
                    .map_err(|e| format!("Failed to open library DB: {e}"))?;
            tracing::info!("Library DB ready at {}", meta_path.display());
            let library_pool = DbPool::new(meta_path.clone(), "library_db", open_library_db)?;

            // Pre-flight: a clobbered SQLite header makes `UserDataDb::open`
            // return Err and crash-loop the service via systemd. Detect that
            // case and start with a `new_corrupt` pool so the user sees the
            // recovery banner via the SSE init payload and can pick Restore
            // or Reset — both of which call `pool.reopen()` against the path
            // we wire up here.
            let ud_path =
                replay_control_core_server::user_data_db::UserDataDb::db_path(&storage.root);
            let user_data_pool = if replay_control_core_server::sqlite::has_invalid_sqlite_header(
                &ud_path,
            ) {
                tracing::error!(
                    "User data DB at {} has invalid SQLite header — starting in corrupt state; user can recover via Restore from backup or Reset",
                    ud_path.display()
                );
                DbPool::new_corrupt(ud_path, "user_data_db", open_user_data_db)
            } else {
                let (_ud_conn, ud_path, ud_corrupt) =
                    replay_control_core_server::user_data_db::UserDataDb::open(&storage.root)
                        .map_err(|e| format!("Failed to open user data DB: {e}"))?;
                tracing::info!("User data DB ready at {}", ud_path.display());
                let pool = DbPool::new(ud_path.clone(), "user_data_db", open_user_data_db)?;
                if ud_corrupt {
                    tracing::warn!("User data DB is corrupt — marking pool, awaiting user action");
                    pool.mark_corrupt();
                } else {
                    let backup_path = ud_path.with_extension("db.bak");
                    match std::fs::copy(&ud_path, &backup_path) {
                        Ok(_) => {
                            tracing::info!("User data backup saved to {}", backup_path.display())
                        }
                        Err(e) => tracing::debug!("Could not back up user_data.db: {e}"),
                    }
                }
                pool
            };

            register_corruption_callbacks(&library_pool, &user_data_pool, config_tx.clone());

            (library_pool, user_data_pool)
        } else {
            tracing::warn!(
                "Starting without storage — all requests will redirect to /waiting until storage appears"
            );
            let library_pool = DbPool::new_closed("library_db");
            let user_data_pool = DbPool::new_closed("user_data_db");
            register_corruption_callbacks(&library_pool, &user_data_pool, config_tx.clone());
            (library_pool, user_data_pool)
        };

        let activity = Arc::new(std::sync::RwLock::new(Activity::Idle));

        let import = Arc::new(ImportPipeline::new());
        let thumbnails = Arc::new(ThumbnailPipeline::new());

        // Resolve settings directory from CLI args.
        let settings = resolve_settings_dir(
            settings_path.as_deref(),
            storage_path_override
                .as_ref()
                .map(|p| p.to_str().unwrap_or_default()),
        );

        // On Pi (no --storage-path): migrate old per-storage settings if needed.
        if storage_path_override.is_none()
            && let Some(ref s) = storage
        {
            let _ = settings.migrate_from_storage(&s.root);
        }

        // Load all user preferences from settings.cfg once at startup.
        let prefs = replay_control_core_server::settings::UserPreferences::load(&settings);

        let state = Self {
            storage: Arc::new(std::sync::RwLock::new(storage)),
            config: Arc::new(std::sync::RwLock::new(config)),
            config_path,
            cache: Arc::new(LibraryService::new()),
            response_cache: Arc::new(response_cache::ResponseCache::new()),
            storage_path_override,
            settings,
            prefs: Arc::new(std::sync::RwLock::new(prefs)),
            library_pool,
            user_data_pool,
            import,
            thumbnails,
            pending_downloads: Arc::new(std::sync::RwLock::new(std::collections::HashSet::new())),
            activity,
            config_tx,
            activity_tx,
        };

        // Surface custom-skin fallback in the log; without this it's invisible
        // that the user's configured palette isn't being honoured.
        let effective_skin = state.effective_skin();
        if replay_control_core::skins::is_custom(effective_skin) {
            tracing::info!(
                "system_skin={effective_skin} is a ReplayOS custom user skin; rendering with default palette until PNG-based color extraction is added"
            );
        }

        Ok(state)
    }

    /// Check whether storage is available.
    pub fn has_storage(&self) -> bool {
        self.storage
            .read()
            .expect("storage lock poisoned")
            .is_some()
    }

    /// Read-lock storage and clone the current StorageLocation.
    /// Panics if storage is None — the middleware redirects ALL requests to
    /// `/waiting` when storage is unavailable, so no handler should ever
    /// reach this when storage is None.
    pub fn storage(&self) -> StorageLocation {
        self.storage
            .read()
            .expect("storage lock poisoned")
            .clone()
            .expect(
                "storage() called without storage — middleware should have redirected to /waiting",
            )
    }

    /// Check if either database has been flagged as corrupt.
    /// Returns `(library_corrupt, user_data_corrupt)`.
    pub fn is_db_corrupt(&self) -> (bool, bool) {
        (
            self.library_pool.is_corrupt(),
            self.user_data_pool.is_corrupt(),
        )
    }

    /// Returns `(library_corrupt, user_data_corrupt, user_data_backup_exists)`.
    /// Used by `sse_config_stream` to seed the `init` payload.
    pub fn corruption_status(&self) -> (bool, bool, bool) {
        let (library_corrupt, user_data_corrupt) = self.is_db_corrupt();
        (
            library_corrupt,
            user_data_corrupt,
            self.user_data_pool.backup_path_exists(),
        )
    }

    /// Get the user's region preference from cached preferences.
    pub fn region_preference(&self) -> replay_control_core::rom_tags::RegionPreference {
        self.prefs.read().expect("prefs lock poisoned").region
    }

    /// Get the user's secondary (fallback) region preference from cached preferences.
    pub fn region_preference_secondary(
        &self,
    ) -> Option<replay_control_core::rom_tags::RegionPreference> {
        self.prefs
            .read()
            .expect("prefs lock poisoned")
            .region_secondary
    }

    /// Get the effective skin index: app preference if set,
    /// otherwise fall back to `replay.cfg`'s `system_skin` (sync mode).
    pub fn effective_skin(&self) -> u32 {
        if let Some(index) = self.prefs.read().expect("prefs lock poisoned").skin {
            index
        } else {
            self.config
                .read()
                .expect("config lock poisoned")
                .system_skin()
        }
    }

    /// Update wifi settings in `replay.cfg` and write back to disk.
    pub fn update_wifi(
        &self,
        ssid: &str,
        password: &str,
        country: &str,
        mode: &str,
        hidden: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let config_path = self.config_file_path();
        let mut config = self.config.write().expect("config lock poisoned");
        config.set_wifi(ssid, password, country, mode, hidden);
        config.write_to_file(&config_path, &config_path)?;
        Ok(())
    }

    /// Update NFS settings in `replay.cfg` and write back to disk.
    pub fn update_nfs(
        &self,
        server: &str,
        share: &str,
        version: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let config_path = self.config_file_path();
        let mut config = self.config.write().expect("config lock poisoned");
        config.set_nfs(server, share, version);
        config.write_to_file(&config_path, &config_path)?;
        Ok(())
    }

    /// Re-detect storage from config (unless a CLI override was given).
    /// Returns `true` if the storage location actually changed.
    /// Handles None->Some transitions (storage appearing after startup).
    pub async fn refresh_storage(&self) -> Result<bool, Box<dyn std::error::Error>> {
        // Re-read config from disk so system-level settings (wifi, NFS,
        // system_skin for sync mode, etc.) are picked up on next SSR render.
        let config_path = self.config_file_path();
        let config = if config_path.exists() {
            SystemConfig::from_file(&config_path)?
        } else {
            SystemConfig::parse("")?
        };

        // Check if the effective skin changed after config re-read.
        let old_skin = self.effective_skin();
        {
            let mut guard = self.config.write().expect("config lock poisoned");
            *guard = config.clone();
        }
        let new_skin = self.effective_skin();
        if old_skin != new_skin {
            let skin_css = replay_control_core::skins::theme_css(new_skin);
            let _ = self.config_tx.send(ConfigEvent::SkinChanged {
                skin_index: new_skin,
                skin_css,
            });
        }

        // Skip storage re-detection when an explicit path was given.
        if self.storage_path_override.is_some() {
            return Ok(false);
        }

        let new_storage = StorageLocation::detect(&config)?;
        let had_storage = self.has_storage();

        let changed = {
            let current = self.storage.read().expect("storage lock poisoned");
            match current.as_ref() {
                Some(s) => s.root != new_storage.root || s.kind != new_storage.kind,
                None => true, // None -> Some is always a change
            }
        };

        if changed {
            tracing::info!(
                "Storage changed: {:?} at {}",
                new_storage.kind,
                new_storage.root.display()
            );

            {
                let mut guard = self.storage.write().expect("storage lock poisoned");
                *guard = Some(new_storage);
            }

            // Close old DB connections so they re-open at the new storage root.
            self.library_pool.close();
            self.user_data_pool.close();
            // Re-open at the new storage root.
            let new_storage_ref = self.storage();
            self.library_pool.reopen(&new_storage_ref.root);
            self.user_data_pool.reopen(&new_storage_ref.root);

            // Back up user_data.db after opening at the new location.
            if !had_storage {
                let ud_path = self.user_data_pool.db_path();
                let backup_path = ud_path.with_extension("db.bak");
                match std::fs::copy(&ud_path, &backup_path) {
                    Ok(_) => tracing::info!("User data backup saved to {}", backup_path.display()),
                    Err(e) => tracing::debug!("Could not back up user_data.db: {e}"),
                }
            }

            self.cache.invalidate(&self.library_pool).await;
            self.response_cache.invalidate_all();

            // Reload user preferences from the settings store.
            let new_prefs =
                replay_control_core_server::settings::UserPreferences::load(&self.settings);
            *self.prefs.write().expect("prefs lock poisoned") = new_prefs;

            let kind = format!("{:?}", new_storage_ref.kind).to_lowercase();
            let _ = self
                .config_tx
                .send(ConfigEvent::StorageChanged { storage_kind: kind });

            // None->Some transition: start background pipeline and ROM watcher.
            if !had_storage {
                tracing::info!("Storage appeared — starting background pipeline and ROM watcher");
                BackgroundManager::start(self.clone());
            }
        }

        Ok(changed)
    }

    /// Resolve the path to `replay.cfg` that `refresh_storage()` will read.
    pub(crate) fn config_file_path(&self) -> PathBuf {
        if let Some(ref p) = self.config_path {
            p.clone()
        } else if let Some(ref p) = self.storage_path_override {
            p.join("config/replay.cfg")
        } else {
            PathBuf::from("/media/sd/config/replay.cfg")
        }
    }
}

/// Parse the `Accept-Language` header and return the best-matching supported locale.
/// Returns `Locale::En` as fallback.
fn resolve_locale_from_accept_language(
    headers: &axum::http::HeaderMap,
) -> replay_control_core::locale::Locale {
    use replay_control_core::locale::Locale;

    let Some(accept) = headers.get(axum::http::header::ACCEPT_LANGUAGE) else {
        return Locale::En;
    };
    let Ok(value) = accept.to_str() else {
        return Locale::En;
    };
    // Parse "es-ES,es;q=0.9,en;q=0.8,ja;q=0.7" style values
    let mut langs: Vec<(&str, f32)> = value
        .split(',')
        .filter_map(|part| {
            let mut parts = part.trim().splitn(2, ';');
            let tag = parts.next()?.trim();
            let q = parts
                .next()
                .and_then(|p| p.trim().strip_prefix("q="))
                .and_then(|q| q.parse::<f32>().ok())
                .unwrap_or(1.0);
            Some((tag, q))
        })
        .collect();
    langs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    for (tag, _) in &langs {
        let primary = tag.split('-').next().unwrap_or(tag);
        match primary {
            "en" => return Locale::En,
            "es" => return Locale::Es,
            "ja" => return Locale::Ja,
            _ => continue,
        }
    }
    Locale::En
}

/// Build the application router with API routes, server function handler,
/// and SSR fallback. Extracted from main.rs so integration tests can reuse
/// the same router construction.
pub fn build_router(
    app_state: AppState,
    leptos_options: leptos::config::LeptosOptions,
) -> axum::Router {
    use axum::Router;
    use leptos::prelude::*;

    let api_routes = Router::new()
        .merge(system_info::routes())
        .merge(roms::routes())
        .merge(favorites::routes())
        .merge(upload::routes())
        .merge(recents::routes())
        .nest("/core", core_api::routes());

    let state_for_ssr = app_state.clone();
    let opts_for_ssr = leptos_options.clone();

    let ssr_handler = leptos_axum::render_app_to_stream_with_context(
        move || {
            use crate::i18n::InitialLocale;
            use replay_control_core::locale::Locale;

            let state = state_for_ssr.clone();

            // Resolve locale: cached prefs → Accept-Language header → En
            let locale = state.prefs.read().expect("prefs lock poisoned").locale;

            let locale = locale.unwrap_or_else(|| {
                // Fall back to Accept-Language header
                use_context::<axum::http::request::Parts>()
                    .map(|parts| resolve_locale_from_accept_language(&parts.headers))
                    .unwrap_or(Locale::En)
            });

            provide_context(InitialLocale(locale));
            provide_context(state);
        },
        move || {
            let opts = opts_for_ssr.clone();
            view! { <crate::Shell options=opts /> }
        },
    );

    let state_for_sfn = app_state.clone();

    Router::new()
        .nest("/api", api_routes)
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
        .route(
            "/static/style.css",
            axum::routing::get(|| async {
                (
                    [("content-type", "text/css"), ("cache-control", CACHE_1H)],
                    include_str!(concat!(env!("OUT_DIR"), "/style.css")),
                )
            }),
        )
        .fallback(ssr_handler)
        .with_state(app_state)
}
