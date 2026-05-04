pub mod activity;
pub mod analytics;
pub mod background;
pub(crate) mod core_api;
pub mod favorites;
pub(crate) mod library;
pub mod recents;
pub mod response_cache;
pub mod roms;
pub mod system_info;
pub mod thumbnail_orchestrator;
pub mod thumbnail_pipeline;
pub mod upload;

pub use activity::{Activity, ActivityGuard, MaintenanceKind, StartupPhase};
pub use background::BackgroundManager;
pub use library::LibraryService;
pub use replay_control_core_server::db_pool::{DbError, DbPool, rusqlite};
pub use thumbnail_pipeline::ThumbnailPipeline;

/// Cache-control header values for static asset responses.
pub const CACHE_1H: &str = "public, max-age=3600";
pub const CACHE_1D: &str = "public, max-age=86400";
pub const CACHE_IMMUTABLE: &str = "public, max-age=31536000, immutable";

/// Read pool size for the library DB. WAL on ext4 SD lets concurrent reads
/// actually parallelise; 3 covers typical SSR fan-out (recommendations +
/// recents + favorites + system info) overlapping with one long enrichment
/// or thumbnail-planning pass.
const LIBRARY_READ_POOL_SIZE: usize = 3;

/// Read pool size for the user_data DB. The DB lives on the ROM storage,
/// which can be DELETE-mode (exFAT/NFS); extra readers don't help there
/// and the WriteGate path still serializes against writers.
const USER_DATA_READ_POOL_SIZE: usize = 1;

/// Read pool size for the host-global external_metadata DB. Metadata
/// snapshots, thumbnail planning, enrichment, box art variants, and metadata
/// server functions all read from here. Two readers keep short UI reads moving
/// while one longer background read is active.
const EXTERNAL_METADATA_READ_POOL_SIZE: usize = 2;

use std::path::PathBuf;
use std::sync::Arc;

use replay_control_core_server::config::SystemConfig;
use replay_control_core_server::data_dir::DataDir;
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
    AssetHealthChanged {
        issues: Vec<replay_control_core::asset_health::AssetHealthIssue>,
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
    /// Resolved data directory (per-host root for storage-id-keyed library DBs).
    pub data_dir: DataDir,
    /// Cached user preferences (skin, locale, region, font size).
    /// Loaded once at startup; updated in-memory on every settings change.
    pub prefs: Arc<std::sync::RwLock<replay_control_core_server::settings::UserPreferences>>,
    /// Library DB pool (deadpool-backed, concurrent reads).
    pub library_pool: DbPool,
    /// User data DB pool (deadpool-backed, concurrent reads).
    pub user_data_pool: DbPool,
    /// Host-global external metadata DB pool — LaunchBox text + libretro
    /// thumbnail manifests. Read by enrichment + thumbnail UI lookups;
    /// written by the LaunchBox + libretro refresh paths.
    pub external_metadata_pool: DbPool,
    /// Thumbnail pipeline (index + download operations).
    pub thumbnails: Arc<ThumbnailPipeline>,
    /// Single coordinator for all thumbnail-download work (bulk
    /// pre-fetch + on-demand): concurrency cap, dedup, and priority.
    /// See `api/thumbnail_orchestrator.rs`.
    pub thumbnail_orchestrator: Arc<thumbnail_orchestrator::ThumbnailDownloadOrchestrator>,
    /// Unified activity state: at most one activity at a time.
    /// Replaces `busy`, `busy_label`, `scanning`, and `rebuild_progress`.
    pub(crate) activity: Arc<std::sync::RwLock<Activity>>,
    /// Broadcast channel for config change notifications (skin, storage).
    pub config_tx: tokio::sync::broadcast::Sender<ConfigEvent>,
    /// Broadcast channel for activity state changes (import, thumbnail, rebuild).
    pub activity_tx: tokio::sync::broadcast::Sender<Activity>,
    /// Reportable health issues with shipped data assets (catalog schema
    /// mismatch today; future asset types via the release-asset-manifest plan).
    /// Populated at startup; consumed by the SSE init payload + the
    /// `<AssetHealthBanner>` UI component.
    pub asset_health:
        Arc<std::sync::RwLock<Vec<replay_control_core::asset_health::AssetHealthIssue>>>,
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

/// User-data opener: drops the corruption probe flag, since `DbPool::reopen`
/// just needs the connection. The caller running the *initial* open in
/// `AppState::new` calls `open_at` directly so it can act on the flag.
fn open_user_data_db(
    db_path: &std::path::Path,
) -> replay_control_core::error::Result<rusqlite::Connection> {
    replay_control_core_server::user_data_db::UserDataDb::open_at(db_path).map(|(c, _)| c)
}

/// Resolved per-storage DB paths after one-time pre-attach steps.
struct ResolvedDbPaths {
    library: PathBuf,
    user_data: PathBuf,
}

/// Run the pre-attach pipeline shared by `AppState::new` and
/// `refresh_storage`: wait for the FS to surface (production only),
/// assign/read the storage id, migrate any per-storage `library.db`
/// into the central data dir, and resolve both DB paths.
///
/// Centralising this is what kept §A2 from drifting again — adding a
/// new step (migration, marker rewrite, validation) only happens here,
/// so init and refresh stay in lockstep by construction.
fn prepare_storage_dbs(
    storage: &replay_control_core_server::storage::StorageLocation,
    data_dir: &replay_control_core_server::data_dir::DataDir,
    _is_production: bool,
) -> Result<ResolvedDbPaths, String> {
    // Caller is responsible for the readiness gate (`StorageLocation::is_ready`)
    // — when the FS isn't a real mount yet (rootfs-stub race on slow NFS),
    // this function should never be called. Routing the not-ready case
    // through the existing no-storage path lets the background re-detection
    // loop pick the mount up later, instead of failing startup with a
    // bounded-deadline timeout that could never be tuned right.
    let storage_id = storage
        .ensure_storage_id()
        .map_err(|e| format!("Failed to assign storage id: {e}"))?;
    tracing::info!("Storage id: {storage_id}");

    let library = data_dir.library_db_path(&storage_id);
    replay_control_core_server::library_db::LibraryDb::migrate_from_storage(
        &storage.root,
        &library,
    )
    .map_err(|e| format!("Failed to migrate library DB: {e}"))?;

    let user_data = replay_control_core_server::user_data_db::UserDataDb::db_path(&storage.root);

    Ok(ResolvedDbPaths { library, user_data })
}

/// Reopen `pool` at `db_path`, but flag corrupt without opening when the
/// SQLite magic header is invalid. Used by both initial open and storage
/// swap so the corruption banner fires on either path. Library DBs don't
/// need this — `LibraryDb::open_at` deletes-and-recreates on bad header
/// (the file is rebuildable cache); user_data is not rebuildable.
async fn reopen_user_data_or_mark_corrupt(pool: &DbPool, db_path: &std::path::Path) {
    if replay_control_core_server::sqlite::has_invalid_sqlite_header(db_path) {
        tracing::error!(
            "user_data.db at {} has invalid SQLite header — flagging pool corrupt",
            db_path.display()
        );
        pool.mark_corrupt();
    } else {
        pool.reopen(db_path).await;
    }
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

/// Resolve the data directory (per-host root for storage-id-keyed DBs).
///
/// Priority:
/// 1. `--data-dir` explicit override
/// 2. `--storage-path` given -> `<storage>/.replay-control-data` (local dev,
///    keeps the dev tree self-contained — does not collide with the
///    in-storage `.replay-control/` folder used by user_data + thumbnails)
/// 3. Pi production fallback -> `/var/lib/replay-control`
fn resolve_data_dir(data_dir: Option<&str>, storage_path: Option<&str>) -> DataDir {
    if let Some(p) = data_dir {
        return DataDir::new(p);
    }
    if let Some(s) = storage_path {
        return DataDir::new(PathBuf::from(s).join(".replay-control-data"));
    }
    DataDir::default_root()
}

impl AppState {
    pub fn new(
        storage_path: Option<String>,
        config_path: Option<String>,
        settings_path: Option<String>,
        data_dir: Option<String>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let config_path = config_path.map(PathBuf::from);
        let storage_path_override = storage_path.as_ref().map(PathBuf::from);

        let data_dir = resolve_data_dir(data_dir.as_deref(), storage_path.as_deref());
        tracing::info!("Data dir: {}", data_dir.root().display());

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
                Ok(storage) if storage.is_ready() => (Some(storage), config),
                Ok(storage) => {
                    // Path exists but the kernel hasn't finished mounting on
                    // top of the rootfs stub yet (slow NFS first-mount, etc).
                    // Route to the no-storage path; background re-detection
                    // will pick the mount up when it appears.
                    tracing::warn!(
                        "Storage path {} not yet a mount point — starting in no-storage mode, will retry",
                        storage.root.display()
                    );
                    (None, config)
                }
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

        // Open / create the host-global external_metadata.db before any
        // pool that might write to it. Same model as `init_catalog` —
        // single file directly under the data root, not per-storage.
        let em_path = data_dir.external_metadata_db_path();
        if let Some(parent) = em_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create {}: {e}", parent.display()))?;
        }
        replay_control_core_server::external_metadata::open_at(&em_path)
            .map_err(|e| format!("Failed to open external_metadata DB: {e}"))?;
        let external_metadata_pool = DbPool::new(
            em_path.clone(),
            "external_metadata_db",
            replay_control_core_server::external_metadata::open_at,
            EXTERNAL_METADATA_READ_POOL_SIZE,
        )?;
        tracing::info!("external_metadata DB ready at {}", em_path.display());

        let (library_pool, user_data_pool) = if let Some(ref storage) = storage {
            tracing::info!("Storage: {:?} at {}", storage.kind, storage.root.display());

            let paths = prepare_storage_dbs(storage, &data_dir, storage_path_override.is_none())?;

            replay_control_core_server::library_db::LibraryDb::open_at(&paths.library)
                .map_err(|e| format!("Failed to open library DB: {e}"))?;
            tracing::info!("Library DB ready at {}", paths.library.display());
            let library_pool = DbPool::new(
                paths.library.clone(),
                "library_db",
                replay_control_core_server::library_db::LibraryDb::open_at,
                LIBRARY_READ_POOL_SIZE,
            )?;

            // user_data isn't rebuildable, so a clobbered header → start in
            // corrupt state with the recovery banner instead of crashing.
            // The probe-failed-but-loadable path is handled separately
            // (open + mark_corrupt below).
            let user_data_pool = if replay_control_core_server::sqlite::has_invalid_sqlite_header(
                &paths.user_data,
            ) {
                tracing::error!(
                    "User data DB at {} has invalid SQLite header — starting in corrupt state; user can recover via Restore from backup or Reset",
                    paths.user_data.display()
                );
                DbPool::new_corrupt(
                    paths.user_data,
                    "user_data_db",
                    open_user_data_db,
                    USER_DATA_READ_POOL_SIZE,
                )
            } else {
                let (_ud_conn, ud_corrupt) =
                    replay_control_core_server::user_data_db::UserDataDb::open_at(&paths.user_data)
                        .map_err(|e| format!("Failed to open user data DB: {e}"))?;
                tracing::info!("User data DB ready at {}", paths.user_data.display());
                let pool = DbPool::new(
                    paths.user_data.clone(),
                    "user_data_db",
                    open_user_data_db,
                    USER_DATA_READ_POOL_SIZE,
                )?;
                if ud_corrupt {
                    tracing::warn!("User data DB is corrupt — marking pool, awaiting user action");
                    pool.mark_corrupt();
                } else {
                    let backup_path = paths.user_data.with_extension("db.bak");
                    match std::fs::copy(&paths.user_data, &backup_path) {
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

        // Seed the asset-health registry from startup probes. Today's only
        // reporter is the catalog schema check (set in init_catalog before
        // AppState construction); future asset types plug in here when the
        // release-asset-manifest work lands.
        let mut initial_issues: Vec<replay_control_core::asset_health::AssetHealthIssue> =
            Vec::new();
        if replay_control_core_server::catalog_pool::schema_outdated() {
            initial_issues.push(replay_control_core::asset_health::AssetHealthIssue {
                asset: "catalog.sqlite".into(),
                kind: "schema_too_old".into(),
                message: "Catalog out of date. Reinstall Replay Control to refresh.".into(),
            });
        }

        let state = Self {
            storage: Arc::new(std::sync::RwLock::new(storage)),
            config: Arc::new(std::sync::RwLock::new(config)),
            config_path,
            cache: Arc::new(LibraryService::new()),
            response_cache: Arc::new(response_cache::ResponseCache::new()),
            storage_path_override,
            settings,
            data_dir,
            prefs: Arc::new(std::sync::RwLock::new(prefs)),
            library_pool,
            user_data_pool,
            external_metadata_pool,
            thumbnails,
            thumbnail_orchestrator: Arc::new(
                thumbnail_orchestrator::ThumbnailDownloadOrchestrator::spawn(
                    thumbnail_orchestrator::Config::default(),
                ),
            ),
            activity,
            config_tx,
            activity_tx,
            asset_health: Arc::new(std::sync::RwLock::new(initial_issues)),
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

    /// Snapshot of currently-reported asset health issues. Used by
    /// `sse_config_stream` to seed the `init` payload.
    pub fn asset_health_snapshot(
        &self,
    ) -> Vec<replay_control_core::asset_health::AssetHealthIssue> {
        self.asset_health
            .read()
            .expect("asset_health lock poisoned")
            .clone()
    }

    /// Append an asset health issue to the registry and broadcast on the
    /// config channel. Idempotent on `(asset, kind)` — if the same issue is
    /// already reported, no-op (avoids duplicate banners on retry paths).
    pub fn report_asset_issue(&self, issue: replay_control_core::asset_health::AssetHealthIssue) {
        let snapshot = {
            let mut guard = self
                .asset_health
                .write()
                .expect("asset_health lock poisoned");
            if guard
                .iter()
                .any(|existing| existing.asset == issue.asset && existing.kind == issue.kind)
            {
                return;
            }
            guard.push(issue);
            guard.clone()
        };
        let _ = self
            .config_tx
            .send(ConfigEvent::AssetHealthChanged { issues: snapshot });
    }

    /// Invalidate user-facing caches that depend on library state:
    /// the `ResponseCache` TTL slots and the `recommendations` snapshot.
    /// `metadata_page` is invalidated separately by the few sites that
    /// affect system-stats display.
    pub async fn invalidate_user_caches(&self) {
        self.response_cache.invalidate_all();
        self.cache.invalidate_recommendations().await;
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
        if !new_storage.is_ready() {
            // Path exists but mount hasn't completed — same rootfs-stub
            // race the startup detect site handles. Caller's next tick
            // will retry; treating as no-change avoids tearing down a
            // working storage state for a transient mount-not-ready blip.
            tracing::debug!(
                "refresh_storage: {} not yet a mount point; deferring",
                new_storage.root.display()
            );
            return Ok(false);
        }
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

            let paths = prepare_storage_dbs(
                &new_storage,
                &self.data_dir,
                self.storage_path_override.is_none(),
            )?;

            {
                let mut guard = self.storage.write().expect("storage lock poisoned");
                *guard = Some(new_storage);
            }

            self.library_pool.reopen(&paths.library).await;
            reopen_user_data_or_mark_corrupt(&self.user_data_pool, &paths.user_data).await;

            // Back up user_data.db after opening at the new location.
            if !had_storage {
                let ud_path = self.user_data_pool.db_path();
                let backup_path = ud_path.with_extension("db.bak");
                match std::fs::copy(&ud_path, &backup_path) {
                    Ok(_) => tracing::info!("User data backup saved to {}", backup_path.display()),
                    Err(e) => tracing::debug!("Could not back up user_data.db: {e}"),
                }
            }

            if let Err(e) = self.cache.invalidate(&self.library_pool).await {
                tracing::debug!("storage-change cache.invalidate skipped: {e}");
            }
            self.invalidate_user_caches().await;

            // Reload user preferences from the settings store.
            let new_prefs =
                replay_control_core_server::settings::UserPreferences::load(&self.settings);
            *self.prefs.write().expect("prefs lock poisoned") = new_prefs;

            let kind = format!("{:?}", self.storage().kind).to_lowercase();
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `refresh_storage`'s symmetric pre-flight: when the re-attached
    /// storage has a clobbered user_data.db magic header, the helper
    /// flags the pool corrupt instead of leaving `reopen` to fail
    /// silently. This is the §A2 fix from the WAL-unlink investigation;
    /// without the test a future refactor that drops the header check
    /// would silently break the corruption banner on storage swap.
    #[tokio::test(flavor = "multi_thread")]
    async fn reopen_user_data_or_mark_corrupt_flags_bad_header() {
        let tmp = tempfile::tempdir().unwrap();
        let valid_path = tmp.path().join("good.db");
        let bad_path = tmp.path().join("bad.db");

        replay_control_core_server::user_data_db::UserDataDb::open_at(&valid_path).unwrap();

        let pool = DbPool::new(
            valid_path,
            "user_data_db",
            open_user_data_db,
            USER_DATA_READ_POOL_SIZE,
        )
        .unwrap();
        assert!(!pool.is_corrupt());

        // 4 KiB of garbage = clobbered SQLite magic header.
        std::fs::write(&bad_path, [0xDEu8; 4096]).unwrap();

        reopen_user_data_or_mark_corrupt(&pool, &bad_path).await;

        assert!(
            pool.is_corrupt(),
            "bad-header path must flag the pool corrupt so the banner fires"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn reopen_user_data_or_mark_corrupt_proceeds_on_healthy_header() {
        let tmp = tempfile::tempdir().unwrap();
        let initial_path = tmp.path().join("a.db");
        let new_path = tmp.path().join("b.db");

        replay_control_core_server::user_data_db::UserDataDb::open_at(&initial_path).unwrap();
        replay_control_core_server::user_data_db::UserDataDb::open_at(&new_path).unwrap();

        let pool = DbPool::new(
            initial_path,
            "user_data_db",
            open_user_data_db,
            USER_DATA_READ_POOL_SIZE,
        )
        .unwrap();

        reopen_user_data_or_mark_corrupt(&pool, &new_path).await;

        assert!(!pool.is_corrupt(), "healthy header must not flag corrupt");
        assert_eq!(pool.db_path(), new_path, "pool must reopen at new path");
    }
}
