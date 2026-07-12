pub mod activity;
pub mod analytics;
pub mod background;
pub(crate) mod core_api;
pub mod db_pools;
pub mod export;
pub mod favorites;
pub(crate) mod library;
pub(crate) mod library_systems;
mod mountinfo_watcher;
pub mod now_playing;
pub mod recents;
pub mod replay_api;
pub mod response_cache;
pub mod roms;
pub mod system_info;
pub mod thumbnail_orchestrator;
pub mod thumbnail_pipeline;
pub mod updates;
pub mod upload;

pub use activity::{Activity, ActivityGuard, MaintenanceKind, StartupPhase};
pub use library::LibraryService;
use replay_control_core::auth::{AuthRole, valid_session_cookie_value};
pub use replay_control_core_server::db_pool::{DbError, DbPool, rusqlite};
pub use thumbnail_pipeline::ThumbnailPipeline;

/// Cache-control header values for static asset responses.
pub const CACHE_1H: &str = "public, max-age=3600";
pub const CACHE_1D: &str = "public, max-age=86400";
pub const CACHE_IMMUTABLE: &str = "public, max-age=31536000, immutable";
pub const CACHE_REVALIDATE: &str = "public, max-age=0, must-revalidate";
pub const CACHE_PRIVATE_1D: &str = "private, max-age=86400";
pub const CACHE_PRIVATE_IMMUTABLE: &str = "private, max-age=31536000, immutable";

/// Read pool size for the library DB. WAL on ext4 SD lets concurrent reads
/// actually parallelise. Two readers with 2 MiB caches were the best
/// memory/runtime tradeoff measured on the large NFS test library.
const LIBRARY_READ_POOL_SIZE: usize = 2;
const LIBRARY_READ_CACHE_KIB: i64 = 2048;
const LIBRARY_WRITE_CACHE_KIB: i64 = 2048;

/// Read pool size for the user_data DB. The DB lives on the ROM storage,
/// which can be DELETE-mode (exFAT/NFS); extra readers don't help there
/// and the WriteGate path still serializes against writers.
const USER_DATA_READ_POOL_SIZE: usize = 1;

/// Read pool size for the host-global external_metadata DB. Metadata
/// snapshots, thumbnail planning, enrichment, box art variants, and metadata
/// server functions all read from here. Two readers keep short UI reads moving
/// while one longer background read is active.
const EXTERNAL_METADATA_READ_POOL_SIZE: usize = 2;
const EXTERNAL_METADATA_READ_CACHE_KIB: i64 = 2048;
const EXTERNAL_METADATA_WRITE_CACHE_KIB: i64 = 2048;

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(default)
}

fn env_i64(name: &str, default: i64) -> i64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(default)
}

fn library_read_pool_size() -> usize {
    env_usize("REPLAY_LIBRARY_READ_POOL_SIZE", LIBRARY_READ_POOL_SIZE)
}

fn library_read_cache_kib() -> i64 {
    env_i64("REPLAY_LIBRARY_READ_CACHE_KIB", LIBRARY_READ_CACHE_KIB)
}

fn library_write_cache_kib() -> i64 {
    env_i64("REPLAY_LIBRARY_WRITE_CACHE_KIB", LIBRARY_WRITE_CACHE_KIB)
}

fn external_metadata_read_pool_size() -> usize {
    env_usize(
        "REPLAY_EXTERNAL_METADATA_READ_POOL_SIZE",
        EXTERNAL_METADATA_READ_POOL_SIZE,
    )
}

fn external_metadata_read_cache_kib() -> i64 {
    env_i64(
        "REPLAY_EXTERNAL_METADATA_READ_CACHE_KIB",
        EXTERNAL_METADATA_READ_CACHE_KIB,
    )
}

fn external_metadata_write_cache_kib() -> i64 {
    env_i64(
        "REPLAY_EXTERNAL_METADATA_WRITE_CACHE_KIB",
        EXTERNAL_METADATA_WRITE_CACHE_KIB,
    )
}

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use replay_control_core::error::Result as CoreResult;
use replay_control_core::runtime_env::Mode;
use replay_control_core_server::auth::{AuthStore, LoginRateLimiter};
use replay_control_core_server::config::{ReplayConfig, replay_config_path};
use replay_control_core_server::data_dir::DataDir;
use replay_control_core_server::replay_service::detect_mode;
use replay_control_core_server::roms::{StorageProbe, probe_storage_ready};
use replay_control_core_server::storage::{StorageKind, StorageLocation};

pub use crate::types::{RomWatcherStatus, StorageStatus, storage_kind_label};

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
    StorageStatusChanged {
        status: StorageStatus,
    },
    RomWatcherStatusChanged {
        status: RomWatcherStatus,
    },
    ReplayApiStatusChanged {
        status: replay_control_core::replay_api::ReplayApiStatus,
    },
}

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub storage: Arc<std::sync::RwLock<Option<StorageLocation>>>,
    pub storage_status: Arc<std::sync::RwLock<StorageStatus>>,
    pub rom_watcher_status: Arc<std::sync::RwLock<RomWatcherStatus>>,
    /// Last-known-good parsed `replay.cfg`. `None` means no readable config:
    /// on the device that is the `ConfigUnavailable` state; off-device it is
    /// normal (config-dependent features are disabled). Never an empty
    /// fabrication.
    pub replay_config: Arc<std::sync::RwLock<Option<ReplayConfig>>>,
    pub library: Arc<LibraryService>,
    /// Response-level cache for assembled recommendation payloads.
    pub response_cache: Arc<response_cache::ResponseCache>,
    /// How the app is deployed (`Device` on RePlayOS, `Standalone` off-device).
    /// Fixed at startup. The single source of truth for system-mutation gating,
    /// storage auto-detection, and where `replay.cfg` lives. `Standalone`
    /// carries the storage root as part of the variant, so the "where is
    /// replay.cfg?" question is answered by pattern-matching on this field —
    /// no parallel `Option<PathBuf>` to keep in sync, no panicking invariant.
    pub mode: Mode,
    /// Resolved settings store (owns the directory path for settings.cfg).
    pub settings: replay_control_core_server::settings::SettingsStore,
    /// Resolved data directory (per-host root for storage-id-keyed library DBs).
    pub data_dir: DataDir,
    /// Host-local authentication runtime. Sessions never live on ROM storage.
    pub auth: AuthRuntime,
    /// Cached user preferences (skin, locale, region, font size).
    /// Loaded once at startup; updated in-memory on every settings change.
    pub prefs: Arc<std::sync::RwLock<replay_control_core_server::settings::UserPreferences>>,
    /// Library DB read handle. Type-fenced — readers cannot write.
    /// See `api/db_pools.rs`.
    pub library_reader: db_pools::LibraryReadPool,
    /// Library DB write handle. Holds the same underlying pool as
    /// `library_reader`; only background, scan, watcher, and explicit
    /// user-action paths should hold a writer.
    pub library_writer: db_pools::LibraryWritePool,
    /// User data DB read handle.
    pub user_data_reader: db_pools::UserDataReadPool,
    /// User data DB write handle. Server-fn user actions (favorites,
    /// box-art override, video add/remove, recents append) and
    /// destructive recovery paths (`repair_corrupt_user_data`,
    /// `restore_user_data_backup`) are the only legitimate writers.
    pub user_data_writer: db_pools::UserDataWritePool,
    /// Host-global external metadata DB read handle.
    pub external_metadata_reader: db_pools::ExternalMetadataReadPool,
    /// Host-global external metadata DB write handle. Only the
    /// LaunchBox refresh path and the libretro manifest refresh path
    /// should hold this; SSR/HTTP read handlers must use the reader.
    pub external_metadata_writer: db_pools::ExternalMetadataWritePool,
    /// Thumbnail pipeline (index + download operations).
    pub thumbnails: Arc<ThumbnailPipeline>,
    /// Single coordinator for all thumbnail-download work (bulk
    /// pre-fetch + on-demand): concurrency cap, dedup, and priority.
    /// See `api/thumbnail_orchestrator.rs`.
    pub thumbnail_orchestrator: Arc<thumbnail_orchestrator::ThumbnailDownloadOrchestrator>,
    /// Unified activity state: at most one activity at a time.
    /// Replaces `busy`, `busy_label`, `scanning`, and `rebuild_progress`.
    pub(crate) activity: Arc<std::sync::RwLock<Activity>>,
    /// Set once the boot library populate has finished at least once. Lets
    /// `/api/core/status` distinguish a genuinely-empty library (`ready`) from
    /// one that simply hasn't been scanned yet (an empty per-system map appears
    /// in both cases, and `activity` can't disambiguate — see the ~2s pre-Startup
    /// idle window). Never reset; a later rescan only touches per-system state.
    pub(crate) initial_populate_done: Arc<std::sync::atomic::AtomicBool>,
    /// Broadcast channel for config change notifications (skin, storage).
    pub events_tx: tokio::sync::broadcast::Sender<ConfigEvent>,
    /// RePlayOS API integration (client + status machine). `None` in
    /// standalone mode — the absence is structural: off-device code cannot
    /// reach the API at all. See `api/replay_api.rs`.
    pub replay_api: Option<Arc<replay_api::ReplayApi>>,
    /// Broadcast channel for activity state changes (import, thumbnail, rebuild).
    pub activity_tx: tokio::sync::broadcast::Sender<Activity>,
    /// Current "Now Playing" session state.
    pub now_playing: Arc<std::sync::RwLock<crate::types::NowPlayingState>>,
    /// Broadcast channel for now-playing updates.
    pub now_playing_tx: tokio::sync::broadcast::Sender<crate::types::NowPlayingState>,
    /// Generation token for the local ROM filesystem watcher. Incrementing it
    /// asks any existing watcher task to stop before a storage swap starts a
    /// watcher for the new `roms/` path.
    pub(crate) rom_watcher_generation: Arc<AtomicU64>,
    /// Generation token for storage-bound scans. Incrementing it asks long
    /// rebuild/rescan/startup tasks to stop before writing into a DB that no
    /// longer belongs to the active storage.
    pub(crate) storage_generation: Arc<AtomicU64>,
    /// Serializes deferred identity hashing batches. Hashing is intentionally
    /// background work, but multiple rebuild/rescan/startup batches must not
    /// compete with each other.
    pub(crate) identity_phase: Arc<tokio::sync::Mutex<()>>,
    /// Reportable health issues with shipped data assets (catalog schema
    /// mismatch today; future asset types via the release-asset-manifest plan).
    /// Populated at startup; consumed by the SSE init payload + the
    /// `<AssetHealthBanner>` UI component.
    pub asset_health:
        Arc<std::sync::RwLock<Vec<replay_control_core::asset_health::AssetHealthIssue>>>,
}

/// Runtime authentication services and cookie policy.
#[derive(Debug, Clone)]
pub struct AuthRuntime {
    pub store: AuthStore,
    /// User (Net Control code) login attempts.
    pub login_rate_limiter: LoginRateLimiter,
    /// Admin (device password) login attempts — separate counter so a flood of
    /// bad user codes can't lock out the admin recovery path, and vice versa.
    pub admin_login_rate_limiter: LoginRateLimiter,
    pub cookie_policy: SessionCookiePolicy,
}

impl AuthRuntime {
    fn open(data_dir: &DataDir) -> CoreResult<Self> {
        Ok(Self {
            store: AuthStore::open(data_dir)?,
            login_rate_limiter: LoginRateLimiter::default(),
            admin_login_rate_limiter: LoginRateLimiter::default(),
            cookie_policy: SessionCookiePolicy::secure(),
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SessionCookiePolicy {
    secure: bool,
}

impl SessionCookiePolicy {
    fn secure() -> Self {
        Self { secure: true }
    }

    pub fn allow_insecure_transport(&mut self) {
        self.secure = false;
    }

    pub fn secure_attribute(self) -> bool {
        self.secure
    }
}

/// Register a corruption-change callback on each pool.
///
/// Both pools share a closure that reads the latest combined corruption state
/// and broadcasts `ConfigEvent::CorruptionChanged` on `events_tx`. The closure
/// captures atomic flag handles and the user-data path handle (no `DbPool`
/// clones), so there is no Pool ↔ callback reference cycle.
fn register_corruption_callbacks(
    library_pool: &DbPool,
    user_data_pool: &DbPool,
    events_tx: tokio::sync::broadcast::Sender<ConfigEvent>,
) {
    use std::sync::atomic::Ordering;

    let lib_flag = library_pool.corrupt_flag();
    let ud_flag = user_data_pool.corrupt_flag();
    let ud_path = user_data_pool.db_path_handle();

    let make_cb = || {
        let lib_flag = lib_flag.clone();
        let ud_flag = ud_flag.clone();
        let ud_path = ud_path.clone();
        let tx = events_tx.clone();
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
/// `redetect_storage`: wait for the FS to surface (production only),
/// assign/read the storage id, migrate any per-storage `library.db`
/// into the central data dir, and resolve both DB paths.
///
/// Centralising this is what kept §A2 from drifting again — adding a
/// new step (migration, marker rewrite, validation) only happens here,
/// so init and refresh stay in lockstep by construction.
fn prepare_storage_dbs(
    storage: &replay_control_core_server::storage::StorageLocation,
    data_dir: &replay_control_core_server::data_dir::DataDir,
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
    {
        let conn = replay_control_core_server::library_db::LibraryDb::open_at(&library)
            .map_err(|e| format!("Failed to open library DB for mtime probe metadata: {e}"))?;
        ensure_mtime_probe_metadata(&conn, storage)
            .map_err(|e| format!("Failed to record storage mtime probe: {e}"))?;
    }

    let user_data = replay_control_core_server::user_data_db::UserDataDb::db_path(&storage.root);

    Ok(ResolvedDbPaths { library, user_data })
}

fn ensure_mtime_probe_metadata(
    conn: &rusqlite::Connection,
    storage: &replay_control_core_server::storage::StorageLocation,
) -> replay_control_core::error::Result<()> {
    use replay_control_core_server::library_db::library_meta;

    const PROBE_VERSION: &str = "1";

    let signature = storage.mtime_probe_signature();
    let existing_signature =
        library_meta::read_meta_result(conn, library_meta::keys::MTIME_PROBE_SIGNATURE)?;
    let existing_version =
        library_meta::read_meta_result(conn, library_meta::keys::MTIME_PROBE_VERSION)?;
    if existing_signature.as_deref() == Some(signature.as_str())
        && existing_version.as_deref() == Some(PROBE_VERSION)
    {
        let existing_trustworthy =
            library_meta::read_meta_result(conn, library_meta::keys::MTIME_PROBE_TRUSTWORTHY)?
                .as_deref()
                == Some("true");
        tracing::info!(
            "storage mtime probe: reused signature={} trustworthy={existing_trustworthy}",
            signature
        );
        return Ok(());
    }

    let probe = storage.probe_mtime_reliability();
    tracing::info!(
        "storage mtime probe: root={} kind={} fs={} advanced={} trustworthy={} grain_ns={:?} signature={}",
        storage.root.display(),
        storage.kind.as_str(),
        probe.fs_type.as_deref().unwrap_or("unknown"),
        probe.advanced,
        probe.trustworthy,
        probe.grain_ns,
        probe.signature
    );
    library_meta::write_meta(
        conn,
        library_meta::keys::MTIME_PROBE_TRUSTWORTHY,
        Some(if probe.trustworthy { "true" } else { "false" }),
    )?;
    library_meta::write_meta(
        conn,
        library_meta::keys::MTIME_PROBE_SIGNATURE,
        Some(&probe.signature),
    )?;
    library_meta::write_meta(
        conn,
        library_meta::keys::MTIME_PROBE_FSTYPE,
        probe.fs_type.as_deref(),
    )?;
    library_meta::write_meta(
        conn,
        library_meta::keys::MTIME_PROBE_VERSION,
        Some(PROBE_VERSION),
    )?;
    Ok(())
}

/// Reopen `pool` at `db_path`, but flag corrupt without opening when the
/// SQLite magic header is invalid. Used by both initial open and storage
/// swap so the corruption banner fires on either path. Library DBs don't
/// need this — `LibraryDb::open_at` deletes-and-recreates on bad header
/// because the file is a rebuildable library index; user_data is not rebuildable.
async fn reopen_user_data_or_mark_corrupt(
    pool: &db_pools::UserDataWritePool,
    db_path: &std::path::Path,
) -> bool {
    if replay_control_core_server::sqlite::has_invalid_sqlite_header(db_path) {
        tracing::error!(
            "user_data.db at {} has invalid SQLite header — flagging pool corrupt",
            db_path.display()
        );
        pool.mark_corrupt();
        true
    } else {
        pool.reopen(db_path).await
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
        settings_path: Option<String>,
        data_dir: Option<String>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let storage_path_override = storage_path.as_ref().map(PathBuf::from);

        // Decide the deployment mode once, from the marker + CLI override.
        // Off-device with no `--storage-path` is an unrecoverable startup error
        // (the app has no ROM library to point at).
        let mode = detect_mode(storage_path_override.clone())?;

        let data_dir = resolve_data_dir(data_dir.as_deref(), storage_path.as_deref());
        tracing::info!("Data dir: {}", data_dir.root().display());
        let auth = AuthRuntime::open(&data_dir)
            .map_err(|e| format!("Failed to open auth signing key store: {e}"))?;

        let (storage, config, initial_storage_status): (
            Option<StorageLocation>,
            Option<ReplayConfig>,
            StorageStatus,
        ) = if let Some(path) = storage_path {
            // Standalone mode: the user pointed us at a folder. `storage_mode`
            // in replay.cfg is a RePlayOS-only concept (sd/usb/nvme/nfs control
            // device-side mount detection); off-device the folder is just a
            // folder. A replay.cfg may still exist alongside it (typical for
            // test fixtures), so we read it for general settings but ignore
            // its storage_mode.
            let storage_root = PathBuf::from(&path);
            if !storage_root.exists() {
                return Err(format!("Storage path does not exist: {path}").into());
            }

            let config_file = replay_config_path(Some(&storage_root));
            let config: Option<ReplayConfig> = ReplayConfig::from_file(&config_file).ok();

            (
                Some(StorageLocation::from_path(
                    storage_root,
                    StorageKind::Folder,
                )),
                config,
                StorageStatus::Ready,
            )
        } else {
            // On the device: RePlayOS owns replay.cfg. A missing or unreadable
            // file is an error — we never fabricate an empty config and never
            // guess storage from a default. Surface the waiting page via
            // ConfigUnavailable; the watcher adopts the file once it appears.
            let config_file = replay_config_path(None);
            let config_result: Result<ReplayConfig, String> = if config_file.exists() {
                ReplayConfig::from_file(&config_file).map_err(|e| e.to_string())
            } else {
                Err(format!("{} not found", config_file.display()))
            };

            match config_result {
                Err(reason) => {
                    tracing::error!("replay.cfg unavailable at startup: {reason}");
                    (None, None, StorageStatus::ConfigUnavailable { reason })
                }
                Ok(config) => {
                    let wanted = config.storage_mode().to_string();
                    match StorageLocation::detect(&config) {
                        Ok(storage) if storage.is_ready() => {
                            (Some(storage), Some(config), StorageStatus::Ready)
                        }
                        Ok(storage) => {
                            // Path exists but the kernel hasn't finished mounting
                            // on top of the rootfs stub yet (slow NFS first-mount,
                            // etc). Route to the no-storage path; background
                            // re-detection picks the mount up when it appears.
                            tracing::warn!(
                                "Storage path {} not yet a mount point — starting in no-storage mode, will retry",
                                storage.root.display()
                            );
                            (
                                None,
                                Some(config),
                                StorageStatus::Misconfigured {
                                    wanted,
                                    current_kind: None,
                                    reason: format!(
                                        "{} exists but is not a mounted storage device yet",
                                        storage.root.display()
                                    ),
                                },
                            )
                        }
                        Err(e) => {
                            tracing::warn!("Storage unavailable at startup: {e}");
                            (
                                None,
                                Some(config),
                                StorageStatus::Misconfigured {
                                    wanted,
                                    current_kind: None,
                                    reason: e.to_string(),
                                },
                            )
                        }
                    }
                }
            }
        };

        // Channels are constructed before the pools so the corruption
        // callbacks registered below can capture `events_tx`.
        let (events_tx, _) = tokio::sync::broadcast::channel::<ConfigEvent>(16);
        let (activity_tx, _) = tokio::sync::broadcast::channel::<Activity>(32);
        let (now_playing_tx, _) =
            tokio::sync::broadcast::channel::<crate::types::NowPlayingState>(32);

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
        let em_read_pool_size = external_metadata_read_pool_size();
        let em_read_cache_kib = external_metadata_read_cache_kib();
        let em_write_cache_kib = external_metadata_write_cache_kib();
        tracing::info!(
            "external_metadata pool: {em_read_pool_size} read connection(s), {em_read_cache_kib} KiB read cache, {em_write_cache_kib} KiB write cache"
        );
        let external_metadata_pool = DbPool::new_with_cache(
            em_path.clone(),
            "external_metadata_db",
            replay_control_core_server::external_metadata::open_at,
            em_read_pool_size,
            em_read_cache_kib,
            em_write_cache_kib,
        )?;
        tracing::info!("external_metadata DB ready at {}", em_path.display());

        let (library_pool, user_data_pool) = if let Some(ref storage) = storage {
            tracing::info!("Storage: {:?} at {}", storage.kind, storage.root.display());

            let paths = prepare_storage_dbs(storage, &data_dir)?;

            replay_control_core_server::library_db::LibraryDb::open_at(&paths.library)
                .map_err(|e| format!("Failed to open library DB: {e}"))?;
            tracing::info!("Library DB ready at {}", paths.library.display());
            let lib_read_pool_size = library_read_pool_size();
            let lib_read_cache_kib = library_read_cache_kib();
            let lib_write_cache_kib = library_write_cache_kib();
            tracing::info!(
                "library pool: {lib_read_pool_size} read connection(s), {lib_read_cache_kib} KiB read cache, {lib_write_cache_kib} KiB write cache"
            );
            let library_pool = DbPool::new_with_cache(
                paths.library.clone(),
                "library_db",
                replay_control_core_server::library_db::LibraryDb::open_at,
                lib_read_pool_size,
                lib_read_cache_kib,
                lib_write_cache_kib,
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

            register_corruption_callbacks(&library_pool, &user_data_pool, events_tx.clone());

            (library_pool, user_data_pool)
        } else {
            tracing::warn!(
                "Starting without storage — all requests will redirect to /waiting until storage appears"
            );
            let library_pool = DbPool::new_deferred(
                "library_db",
                replay_control_core_server::library_db::LibraryDb::open_at,
                library_read_pool_size(),
                library_read_cache_kib(),
                library_write_cache_kib(),
            );
            let user_data_pool = DbPool::new_deferred(
                "user_data_db",
                open_user_data_db,
                USER_DATA_READ_POOL_SIZE,
                4096,
                2048,
            );
            register_corruption_callbacks(&library_pool, &user_data_pool, events_tx.clone());
            (library_pool, user_data_pool)
        };

        let storage_status = Arc::new(std::sync::RwLock::new(initial_storage_status));

        let rom_watcher_status = Arc::new(std::sync::RwLock::new(RomWatcherStatus::default()));

        let activity = Arc::new(std::sync::RwLock::new(Activity::Idle));
        let initial_populate_done = Arc::new(std::sync::atomic::AtomicBool::new(false));

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

        // RePlayOS API integration: device-only by construction. The stored
        // Net Control code (if onboarding happened) seeds the client; the
        // startup probe in `spawn_boot_tasks` resolves the status.
        let replay_api = mode.is_device().then(|| {
            Arc::new(replay_api::ReplayApi::new(
                replay_control_core_server::settings::read_replay_api_token(&settings),
                events_tx.clone(),
            ))
        });

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

        let library_reader = db_pools::LibraryReadPool::from_pool(library_pool.clone());
        let library_writer = db_pools::LibraryWritePool::from_pool(library_pool);
        let user_data_reader = db_pools::UserDataReadPool::from_pool(user_data_pool.clone());
        let user_data_writer = db_pools::UserDataWritePool::from_pool(user_data_pool);
        let external_metadata_reader =
            db_pools::ExternalMetadataReadPool::from_pool(external_metadata_pool.clone());
        let external_metadata_writer =
            db_pools::ExternalMetadataWritePool::from_pool(external_metadata_pool);

        let state = Self {
            mode,
            storage: Arc::new(std::sync::RwLock::new(storage)),
            storage_status,
            rom_watcher_status,
            replay_config: Arc::new(std::sync::RwLock::new(config)),
            library: Arc::new(LibraryService::new()),
            response_cache: Arc::new(response_cache::ResponseCache::new()),
            settings,
            data_dir,
            auth,
            prefs: Arc::new(std::sync::RwLock::new(prefs)),
            library_reader,
            library_writer,
            user_data_reader,
            user_data_writer,
            external_metadata_reader,
            external_metadata_writer,
            thumbnails,
            thumbnail_orchestrator: Arc::new(
                thumbnail_orchestrator::ThumbnailDownloadOrchestrator::spawn(
                    thumbnail_orchestrator::Config::default(),
                ),
            ),
            activity,
            initial_populate_done,
            events_tx,
            replay_api,
            activity_tx,
            now_playing: Arc::new(std::sync::RwLock::new(
                crate::types::NowPlayingState::NotRunning,
            )),
            now_playing_tx,
            rom_watcher_generation: Arc::new(AtomicU64::new(0)),
            storage_generation: Arc::new(AtomicU64::new(0)),
            identity_phase: Arc::new(tokio::sync::Mutex::new(())),
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

    /// Whether the app can serve its normal UI, or must show the waiting page.
    /// Requires storage AND a readable config: on the device, `replay.cfg` can
    /// go missing while the mount is still present (`has_storage()` true), and
    /// in that `ConfigUnavailable` state we must route to `/waiting` too.
    pub fn is_serviceable(&self) -> bool {
        self.has_storage()
            && !matches!(
                self.storage_status(),
                StorageStatus::ConfigUnavailable { .. }
            )
    }

    pub fn storage_status(&self) -> StorageStatus {
        self.storage_status
            .read()
            .expect("storage status lock poisoned")
            .clone()
    }

    fn set_storage_status(&self, status: StorageStatus) {
        let mut guard = self
            .storage_status
            .write()
            .expect("storage status lock poisoned");
        if *guard == status {
            return;
        }
        *guard = status.clone();
        drop(guard);
        let _ = self
            .events_tx
            .send(ConfigEvent::StorageStatusChanged { status });
    }

    #[cfg(test)]
    pub(crate) fn set_storage_status_for_test(&self, status: StorageStatus) {
        self.set_storage_status(status);
    }

    pub fn rom_watcher_status(&self) -> RomWatcherStatus {
        self.rom_watcher_status
            .read()
            .expect("rom watcher status lock poisoned")
            .clone()
    }

    pub(crate) fn set_rom_watcher_status(&self, status: RomWatcherStatus) {
        let mut guard = self
            .rom_watcher_status
            .write()
            .expect("rom watcher status lock poisoned");
        if *guard == status {
            return;
        }
        *guard = status.clone();
        drop(guard);
        let _ = self
            .events_tx
            .send(ConfigEvent::RomWatcherStatusChanged { status });
    }

    #[cfg(test)]
    pub(crate) fn set_rom_watcher_status_for_test(&self, status: RomWatcherStatus) {
        self.set_rom_watcher_status(status);
    }

    pub(crate) fn storage_generation(&self) -> u64 {
        self.storage_generation
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub(crate) fn has_replay_config(&self) -> bool {
        self.replay_config
            .read()
            .expect("replay_config lock poisoned")
            .is_some()
    }

    fn bump_storage_generation(&self) {
        self.storage_generation
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    fn cancel_storage_scans_if_ready(&self) {
        if matches!(
            self.storage_status(),
            StorageStatus::Ready | StorageStatus::Activating
        ) {
            self.bump_storage_generation();
        }
    }

    pub(crate) fn ensure_storage_generation(
        &self,
        expected: u64,
    ) -> replay_control_core::error::Result<()> {
        if self.storage_generation() != expected {
            return Err(replay_control_core::error::Error::StorageChanged);
        }
        Ok(())
    }

    /// User-initiated mutations must not write to fallback storage while
    /// replay.cfg points at an unavailable target. Refresh first so a stale
    /// Ready status cannot race ahead of the config/mount watcher.
    ///
    /// The extra work is bounded to user-triggered mutation paths: one
    /// replay.cfg read plus storage detection syscalls per click/action, not
    /// per request or polling loop.
    pub async fn require_configured_storage_ready_for_mutation(
        &self,
        action: &str,
    ) -> Result<(), String> {
        self.reload_config_and_redetect_storage()
            .await
            .map_err(|e| format!("Cannot {action}: failed to refresh storage status: {e}"))?;

        match self.storage_status() {
            StorageStatus::Ready => Ok(()),
            StorageStatus::Misconfigured {
                wanted,
                current_kind,
                reason,
            } => {
                let fallback = current_kind
                    .map(|kind| format!(" Replay Control is currently using {kind} as fallback."))
                    .unwrap_or_default();
                Err(format!(
                    "Cannot {action}: configured storage {wanted} is unavailable.{fallback} \
                     Restore the configured storage or change the storage selection in RePlayOS settings. \
                     ({reason})"
                ))
            }
            StorageStatus::Activating => {
                Err(format!("Cannot {action}: storage is still activating."))
            }
            StorageStatus::Error { message } => {
                Err(format!("Cannot {action}: storage error: {message}"))
            }
            StorageStatus::WaitingForMount => {
                Err(format!("Cannot {action}: storage is not mounted yet."))
            }
            StorageStatus::ConfigUnavailable { reason } => Err(format!(
                "Cannot {action}: the system configuration is unavailable. ({reason})"
            )),
        }
    }

    fn active_storage_kind(&self) -> Option<String> {
        self.storage
            .read()
            .expect("storage lock poisoned")
            .as_ref()
            .map(|storage| storage.kind.as_str().to_string())
    }

    async fn suspected_storage_fallback_reason(&self, next: &StorageLocation) -> Option<String> {
        let current = self
            .storage
            .read()
            .expect("storage lock poisoned")
            .clone()?;

        if current.root == next.root
            || current.kind == next.kind
            || !storage_kind_is_downgrade(current.kind, next.kind)
            || !storage_kind_is_safe_for_fallback_probe(current.kind)
            || !current.is_ready()
        {
            return None;
        }

        match probe_storage_ready(&current).await {
            StorageProbe::HasVisibleEntries | StorageProbe::StableEmpty => Some(format!(
                "RePlayOS reported {} storage at {}, but current {} storage at {} is still readable; keeping the current storage online to avoid adopting an OS fallback.",
                next.kind.as_str(),
                next.root.display(),
                current.kind.as_str(),
                current.root.display()
            )),
            StorageProbe::NotReady => None,
        }
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
            self.library_reader.is_corrupt(),
            self.user_data_reader.is_corrupt(),
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
            .events_tx
            .send(ConfigEvent::AssetHealthChanged { issues: snapshot });
    }

    /// Invalidate user-facing caches that depend on library state:
    /// the `ResponseCache` TTL slots and the `recommendations` snapshot.
    /// The metadata page reads DB-backed system stats directly.
    pub async fn invalidate_user_caches(&self) {
        self.response_cache.invalidate_all();
        self.library.invalidate_recommendations().await;
    }

    /// Returns `(library_corrupt, user_data_corrupt, user_data_backup_exists)`.
    /// Used by `sse_config_stream` to seed the `init` payload.
    pub fn corruption_status(&self) -> (bool, bool, bool) {
        let (library_corrupt, user_data_corrupt) = self.is_db_corrupt();
        (
            library_corrupt,
            user_data_corrupt,
            self.user_data_writer.backup_path_exists(),
        )
    }

    pub fn now_playing(&self) -> crate::types::NowPlayingState {
        self.now_playing
            .read()
            .expect("now_playing lock poisoned")
            .clone()
    }

    pub fn set_now_playing(&self, next: crate::types::NowPlayingState) {
        let changed = {
            let mut guard = self.now_playing.write().expect("now_playing lock poisoned");
            if *guard == next {
                false
            } else {
                *guard = next.clone();
                true
            }
        };
        if changed {
            let _ = self.now_playing_tx.send(next);
        }
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

    /// Whether first setup has been completed. Standalone mode bypasses
    /// first-setup enforcement; this cached flag is used only in device mode.
    pub fn first_setup_done(&self) -> bool {
        self.prefs
            .read()
            .expect("prefs lock poisoned")
            .first_setup_done
    }

    /// Get the effective skin index: app preference if set,
    /// otherwise fall back to `replay.cfg`'s `system_skin` (sync mode).
    pub fn effective_skin(&self) -> u32 {
        if let Some(index) = self.prefs.read().expect("prefs lock poisoned").skin {
            index
        } else {
            self.replay_config
                .read()
                .expect("replay_config lock poisoned")
                .as_ref()
                .map(|c| c.system_skin())
                .unwrap_or(0)
        }
    }

    /// Enable RePlayOS Net Control in `replay.cfg` and write back to disk.
    /// RePlayOS generates `replay_http_token`; this only flips the feature flag.
    pub fn enable_replayos_net_control(&self) -> Result<(), Box<dyn std::error::Error>> {
        let config_path = self.config_file_path();
        let mut config = ReplayConfig::from_file(&config_path)?;
        config.set_system_net_control(true);
        config.write_to_file(&config_path, &config_path)?;
        *self
            .replay_config
            .write()
            .expect("replay_config lock poisoned") = Some(config);
        Ok(())
    }

    /// Re-read `replay.cfg` into the in-memory config (the source SSR and the
    /// UI read). RePlayOS owns the file and rewrites it atomically; the app
    /// only mirrors it. A freshly read config is adopted ONLY when the file is
    /// present, non-empty, and parses:
    ///
    ///   - missing / empty → expected transient (mid atomic-rename, or storage
    ///     briefly unmounted during the frontend restart a wifi/NFS save
    ///     triggers); logged at debug,
    ///   - unparseable → unexpected (corruption, or a non-atomic external
    ///     rewrite caught mid-write); logged at warn.
    ///
    /// In every failure case we keep the last-known-good config rather than
    /// blanking live settings — the mirror of the write path, which likewise
    /// refuses to act on a missing/empty `replay.cfg`. Returns whether a fresh
    /// config was adopted.
    pub fn reload_replay_config(&self) -> bool {
        let config_path = self.config_file_path();
        // `ReplayConfig::from_file` itself rejects empty/whitespace-only files,
        // so we don't need a separate stat-then-read window — any failure here
        // (missing, empty, corrupt, mid-rewrite) is treated the same: keep the
        // last-known-good config in memory and let the next tick try again.
        let fresh_config = match ReplayConfig::from_file(&config_path) {
            Ok(config) => config,
            Err(e) => {
                tracing::debug!(
                    path = %config_path.display(),
                    "replay.cfg unreadable; keeping last-known-good config: {e}"
                );
                return false;
            }
        };

        let old_skin = self.effective_skin();
        {
            let mut guard = self
                .replay_config
                .write()
                .expect("replay_config lock poisoned");
            *guard = Some(fresh_config);
        }
        let new_skin = self.effective_skin();
        if old_skin != new_skin {
            let skin_css = replay_control_core::skins::theme_css(new_skin);
            let _ = self.events_tx.send(ConfigEvent::SkinChanged {
                skin_index: new_skin,
                skin_css,
            });
        }
        true
    }

    /// Spawn the ordered startup library pipeline and, once it finishes the boot
    /// scan, mark the populate done — which gates `ready` in `/api/core/status`.
    /// Used at boot and after a storage swap. The pipeline runs on its own task;
    /// tracking that the boot populate completed is this startup orchestration's
    /// concern, not something the pipeline body reaches back into.
    pub(crate) fn spawn_startup_pipeline(&self) {
        let state = self.clone();
        tokio::spawn(async move {
            if background::run_pipeline(&state).await {
                state.mark_initial_populate_done();
            }
        });
    }

    /// Reload `replay.cfg` and then re-detect storage from it. Used by the
    /// config-file watcher, the fallback poll, and user-triggered refreshes —
    /// any path where the on-disk config may have changed. Returns whether the
    /// active storage location changed.
    pub async fn reload_config_and_redetect_storage(
        &self,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let config_adopted = self.reload_replay_config();
        let storage_changed = self.redetect_storage().await?;
        // Report change when either the in-memory config rolled forward or the
        // storage location moved. Without the OR, a watcher tick that adopted a
        // fresh config but kept the same mount would return Ok(false), and any
        // user-triggered refresh would falsely report 'no change' even though
        // the wifi/RA/NFS values just changed under it.
        Ok(config_adopted || storage_changed)
    }

    /// Re-detect the active storage location from the current in-memory config
    /// (unless a CLI override was given). Does NOT re-read `replay.cfg` — call
    /// [`Self::reload_replay_config`] first if the file may have changed. Used
    /// directly by the mount-table watcher, where the mounts changed but the
    /// config did not. Returns `true` if the storage location actually changed;
    /// handles None->Some transitions (storage appearing after startup).
    pub async fn redetect_storage(&self) -> Result<bool, Box<dyn std::error::Error>> {
        if let Some(root) = self.mode.standalone_root() {
            // Standalone: the storage root is fixed at startup; no auto-detection
            // to run, but we still owe the user a liveness check — an external
            // mount (USB, NFS, SMB) can disappear out-of-band. Surface a missing
            // root as `Misconfigured` so the existing waiting-page / banner flow
            // takes over, instead of letting downstream ROM reads fail with raw
            // IO errors. `Folder` is the only kind in Standalone (mod.rs:480).
            let alive = tokio::task::spawn_blocking({
                let root = root.to_path_buf();
                move || root.is_dir()
            })
            .await
            .unwrap_or(false);
            // `wanted = "folder"` is the marker for "Standalone liveness check
            // wrote this" — we only clear that exact shape on recovery, so we
            // never overwrite a Misconfigured set by tests or some future
            // Standalone-specific path.
            const LIVENESS_WANTED: &str = "folder";
            if !alive {
                self.cancel_storage_scans_if_ready();
                self.set_storage_status(StorageStatus::Misconfigured {
                    wanted: LIVENESS_WANTED.to_string(),
                    current_kind: Some(LIVENESS_WANTED.to_string()),
                    reason: format!("Storage path {} is not accessible", root.display()),
                });
            } else {
                let owned_by_liveness = matches!(
                    &*self
                        .storage_status
                        .read()
                        .expect("storage_status lock poisoned"),
                    StorageStatus::Misconfigured { wanted, .. } if wanted == LIVENESS_WANTED
                );
                if owned_by_liveness {
                    self.set_storage_status(StorageStatus::Ready);
                }
            }
            return Ok(false);
        }

        // No readable config ⇒ we can't determine the storage mode. Refuse to
        // detect against a default (that would silently pick "sd"); the device
        // is in the ConfigUnavailable state until the file is restored.
        let Some(config) = self
            .replay_config
            .read()
            .expect("replay_config lock poisoned")
            .clone()
        else {
            return Ok(false);
        };
        let wanted = config.storage_mode().to_string();
        let current_kind = self.active_storage_kind();
        let new_storage = match StorageLocation::detect(&config) {
            Ok(storage) => storage,
            Err(e) => {
                tracing::warn!("redetect_storage: configured storage unavailable: {e}");
                self.cancel_storage_scans_if_ready();
                self.set_storage_status(StorageStatus::Misconfigured {
                    wanted,
                    current_kind,
                    reason: e.to_string(),
                });
                return Ok(false);
            }
        };
        if !new_storage.is_ready() {
            // Path exists but mount hasn't completed — same rootfs-stub
            // race the startup detect site handles. Caller's next tick
            // will retry; treating as no-change avoids tearing down a
            // working storage state for a transient mount-not-ready blip.
            tracing::debug!(
                "redetect_storage: {} not yet a mount point; deferring",
                new_storage.root.display()
            );
            self.cancel_storage_scans_if_ready();
            self.set_storage_status(StorageStatus::Misconfigured {
                wanted,
                current_kind,
                reason: format!(
                    "{} exists but is not a mounted storage device yet",
                    new_storage.root.display()
                ),
            });
            return Ok(false);
        }
        if let Some(reason) = self.suspected_storage_fallback_reason(&new_storage).await {
            tracing::warn!("redetect_storage: {reason}");
            self.cancel_storage_scans_if_ready();
            self.set_storage_status(StorageStatus::Misconfigured {
                wanted,
                current_kind,
                reason,
            });
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

        if !changed {
            self.set_storage_status(StorageStatus::Ready);
            return Ok(false);
        }

        {
            // Confirm the storage is fully usable before flipping the
            // middleware gate. A mount point can show up in
            // /proc/self/mountinfo while subdir dirent caches are still
            // cold (NFS partial-mount window) — opening the gate then
            // lets request handlers see zero ROMs and poison library.db
            // with all-zero rows. The probe walks roms_dir + system
            // subdirs with a bounded retry so the gate only opens once
            // the dirent cache stabilizes.
            match probe_storage_ready(&new_storage).await {
                StorageProbe::HasVisibleEntries | StorageProbe::StableEmpty => {}
                StorageProbe::NotReady => {
                    tracing::debug!(
                        "redetect_storage: probe at {} not yet ready; deferring",
                        new_storage.root.display()
                    );
                    self.cancel_storage_scans_if_ready();
                    self.set_storage_status(StorageStatus::Misconfigured {
                        wanted,
                        current_kind,
                        reason: format!(
                            "{} is mounted but its roms directory is not readable yet",
                            new_storage.root.display()
                        ),
                    });
                    return Ok(false);
                }
            }

            tracing::info!(
                "Storage changed: {:?} at {}",
                new_storage.kind,
                new_storage.root.display()
            );
            self.bump_storage_generation();
            self.set_storage_status(StorageStatus::Activating);

            let paths = match prepare_storage_dbs(&new_storage, &self.data_dir) {
                Ok(paths) => paths,
                Err(e) => {
                    let message = format!("Could not prepare storage databases: {e}");
                    tracing::warn!("{message}");
                    self.set_storage_status(StorageStatus::Error { message });
                    return Ok(false);
                }
            };

            if !self.library_writer.reopen(&paths.library).await {
                let message = format!(
                    "Could not open library database at {}",
                    paths.library.display()
                );
                tracing::warn!("{message}");
                self.set_storage_status(StorageStatus::Error { message });
                return Ok(false);
            }

            if !reopen_user_data_or_mark_corrupt(&self.user_data_writer, &paths.user_data).await {
                let message = format!(
                    "Could not open user data database at {}",
                    paths.user_data.display()
                );
                tracing::warn!("{message}");
                self.set_storage_status(StorageStatus::Error { message });
                return Ok(false);
            }

            {
                let mut guard = self.storage.write().expect("storage lock poisoned");
                *guard = Some(new_storage);
            }
            self.set_storage_status(StorageStatus::Ready);

            // Back up user_data.db after opening at the new location.
            if !had_storage {
                let ud_path = self.user_data_writer.db_path();
                let backup_path = ud_path.with_extension("db.bak");
                match std::fs::copy(&ud_path, &backup_path) {
                    Ok(_) => tracing::info!("User data backup saved to {}", backup_path.display()),
                    Err(e) => tracing::debug!("Could not back up user_data.db: {e}"),
                }
            }

            // The writer now points at the newly selected storage's central
            // library DB. Do not call the destructive `clear_library_and_invalidate_caches()` here:
            // it clears that DB and a Some -> Some storage swap would leave
            // the metadata page/library empty until a manual rebuild. Drop
            // only in-memory snapshots; the pipeline below verifies or
            // populates stored library rows using the strict reconcile rules.
            self.library.invalidate_in_memory_views().await;
            self.invalidate_user_caches().await;

            // Reload user preferences from the settings store.
            let new_prefs =
                replay_control_core_server::settings::UserPreferences::load(&self.settings);
            *self.prefs.write().expect("prefs lock poisoned") = new_prefs;

            let kind = self.storage().kind.as_str().to_string();
            let _ = self
                .events_tx
                .send(ConfigEvent::StorageChanged { storage_kind: kind });

            // None->Some transition: start background pipeline and ROM watcher.
            if !had_storage {
                tracing::info!("Storage appeared — starting background pipeline and ROM watcher");
                background::spawn_boot_tasks(self);
            } else {
                tracing::info!("Storage swapped — starting background verification pipeline");
                self.spawn_startup_pipeline();
                background::restart_rom_watcher(self);
            }
        }
        Ok(true)
    }

    /// Resolve the path to `replay.cfg`. Device mode → the SD card's fixed
    /// location (`DEFAULT_REPLAY_CFG`). Standalone mode → `<storage_root>/
    /// config/replay.cfg`, where `storage_root` is owned by `Mode::Standalone`
    /// itself (captured from `--storage-path` at startup, immutable). No
    /// optional fields, no panicking invariant — `Mode::standalone_root()` is
    /// `Some` iff we're in Standalone, by construction.
    pub(crate) fn config_file_path(&self) -> PathBuf {
        replay_config_path(self.mode.standalone_root())
    }
}

fn storage_kind_rank(kind: StorageKind) -> u8 {
    match kind {
        StorageKind::Sd => 0,
        StorageKind::Usb => 1,
        StorageKind::Nvme => 2,
        StorageKind::Nfs => 3,
        StorageKind::Folder => 4,
    }
}

fn storage_kind_is_downgrade(current: StorageKind, next: StorageKind) -> bool {
    storage_kind_rank(next) < storage_kind_rank(current)
}

fn storage_kind_is_safe_for_fallback_probe(kind: StorageKind) -> bool {
    kind.is_local()
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
        .merge(export::routes())
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
            "/sfn/{*fn_name}",
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
            })
            // Server functions read the request body directly (bypassing axum's
            // extractor-level DefaultBodyLimit), so cap the raw body here. Their
            // payloads are small serialized args; large binary transfers use the
            // multipart upload routes, which enforce their own streaming caps.
            .layer(tower_http::limit::RequestBodyLimitLayer::new(
                4 * 1024 * 1024,
            )),
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

/// Stable REST routes consumed by the in-repo libretro core.
pub fn core_routes() -> axum::Router<AppState> {
    core_api::routes()
}

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

pub fn is_public_without_auth(path: &str) -> bool {
    path == "/login"
        || path == "/waiting"
        || path == "/api/version"
        // Read-only library-state signal for the libretro core + health checks +
        // the E2E harness.
        || path == "/api/core/status"
        || path.starts_with("/static/")
}

pub fn with_auth_guard(app: axum::Router, app_state: AppState) -> axum::Router {
    use axum::http::{Request, StatusCode};
    use axum::middleware::Next;
    use axum::response::{IntoResponse, Redirect};

    app.layer(axum::middleware::from_fn(
        move |request: Request<axum::body::Body>, next: Next| {
            let state = app_state.clone();
            async move {
                if !state.mode.is_device() {
                    return next.run(request).await;
                }
                let path = request.uri().path().to_string();
                // Static assets and health/version need no session. Short-circuit
                // them BEFORE resolving the session, because resolving an admin
                // cookie reads /etc/shadow (admin credential fingerprint) + the
                // signing key from disk — doing that per static asset on every
                // page load is needless blocking I/O on the hot path. These are
                // all GET, so the CSRF check below doesn't apply to them.
                if path.starts_with("/static/") || path == "/api/version" || path == "/waiting" {
                    return next.run(request).await;
                }
                let role = request_auth_role(&state, &request);
                if is_unsafe_method(request.method())
                    && role != AuthRole::Anonymous
                    && !passes_csrf_origin_check(request.headers())
                {
                    return StatusCode::FORBIDDEN.into_response();
                }

                if !state.first_setup_done() {
                    if is_public_during_first_setup(&path) {
                        return next.run(request).await;
                    }
                    if request.method() == axum::http::Method::GET
                        && wants_html_response(request.headers())
                    {
                        return Redirect::temporary("/first-setup").into_response();
                    }
                    return StatusCode::UNAUTHORIZED.into_response();
                }

                if path == "/login"
                    && request.method() == axum::http::Method::GET
                    && role != AuthRole::Anonymous
                {
                    return Redirect::temporary("/").into_response();
                }

                if is_public_without_auth(&path) {
                    return next.run(request).await;
                }

                if let Some(required_role) = server_function_required_role(&path) {
                    if role.allows(required_role) {
                        return next.run(request).await;
                    }
                    return if role == AuthRole::Anonymous {
                        StatusCode::UNAUTHORIZED.into_response()
                    } else {
                        StatusCode::FORBIDDEN.into_response()
                    };
                }

                let required_role = route_required_role(request.method(), &path);
                if role.allows(required_role) {
                    return next.run(request).await;
                }

                let wants_html = wants_html_response(request.headers());
                if request.method() == axum::http::Method::GET && wants_html {
                    let target = if role == AuthRole::Anonymous {
                        login_redirect_target(request.uri())
                    } else {
                        access_redirect_target(request.uri())
                    };
                    return Redirect::temporary(&target).into_response();
                }

                if role != AuthRole::Anonymous {
                    StatusCode::FORBIDDEN.into_response()
                } else {
                    StatusCode::UNAUTHORIZED.into_response()
                }
            }
        },
    ))
}

fn wants_html_response(headers: &axum::http::HeaderMap) -> bool {
    use axum::http::header::ACCEPT;

    headers
        .get(ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_none_or(|value| value.contains("text/html"))
}

fn login_redirect_target(uri: &axum::http::Uri) -> String {
    let next = uri
        .path_and_query()
        .map(|path| path.as_str())
        .unwrap_or("/");
    let next = if next.starts_with("/login") {
        "/"
    } else {
        next
    };
    format!("/login?next={}", urlencoding::encode(next))
}

fn access_redirect_target(uri: &axum::http::Uri) -> String {
    let next = uri
        .path_and_query()
        .map(|path| path.as_str())
        .unwrap_or("/");
    let next = if next.starts_with("/settings/access") {
        "/settings"
    } else {
        next
    };
    format!("/settings/access?next={}", urlencoding::encode(next))
}

fn is_unsafe_method(method: &axum::http::Method) -> bool {
    use axum::http::Method;

    !matches!(
        *method,
        Method::GET | Method::HEAD | Method::OPTIONS | Method::TRACE
    )
}

fn passes_csrf_origin_check(headers: &axum::http::HeaderMap) -> bool {
    use axum::http::header::{HOST, ORIGIN, REFERER};

    if let Some(fetch_site) = headers
        .get("sec-fetch-site")
        .and_then(|value| value.to_str().ok())
    {
        match fetch_site {
            "same-origin" => return true,
            "same-site" | "cross-site" => return false,
            _ => {}
        }
    }

    let Some(host) = headers
        .get(HOST)
        .and_then(|value| value.to_str().ok())
        .and_then(normalize_authority)
    else {
        return false;
    };

    if let Some(origin_matches) = headers
        .get(ORIGIN)
        .and_then(|value| value.to_str().ok())
        .map(|value| origin_authority(value).is_some_and(|origin| origin == host))
    {
        return origin_matches;
    }

    if let Some(referer_matches) = headers
        .get(REFERER)
        .and_then(|value| value.to_str().ok())
        .map(|value| origin_authority(value).is_some_and(|referer| referer == host))
    {
        return referer_matches;
    }

    false
}

fn origin_authority(value: &str) -> Option<String> {
    value
        .parse::<axum::http::Uri>()
        .ok()?
        .authority()
        .and_then(|authority| normalize_authority(authority.as_str()))
}

fn normalize_authority(value: &str) -> Option<String> {
    let value = value.trim().trim_end_matches('.');
    if value.is_empty()
        || value.contains('@')
        || value.contains('/')
        || value.contains('\\')
        || value.chars().any(char::is_whitespace)
    {
        return None;
    }
    Some(value.to_ascii_lowercase())
}

fn route_required_role(method: &axum::http::Method, path: &str) -> AuthRole {
    use axum::http::Method;

    if (method == Method::POST && path.starts_with("/api/upload/"))
        || (method == Method::GET && path == "/api/upload/targets")
    {
        return AuthRole::Admin;
    }
    if method == Method::PUT && path == "/api/roms/rename" {
        return AuthRole::Admin;
    }
    // The library CSV export is surfaced only in the admin Metadata page's
    // Advanced section, so the download route is admin-gated to match.
    if method == Method::GET && path == "/api/export/library.csv" {
        return AuthRole::Admin;
    }
    if method == Method::GET && is_admin_page_route(path) {
        return AuthRole::Admin;
    }
    if is_user_route(method, path) {
        return AuthRole::User;
    }
    if is_unsafe_method(method) {
        return AuthRole::Admin;
    }
    AuthRole::User
}

fn request_auth_role(
    state: &AppState,
    request: &axum::http::Request<axum::body::Body>,
) -> AuthRole {
    use axum::http::header::COOKIE;

    session_token_from_cookie(request.headers().get(COOKIE))
        .and_then(|token| {
            state
                .auth
                .store
                .resolve_session(&token, &state.settings)
                .ok()
                .flatten()
        })
        .unwrap_or(AuthRole::Anonymous)
}

fn server_function_required_role(path: &str) -> Option<AuthRole> {
    let function = normalized_server_function_path(path)?;
    let function = function.as_str();
    if is_public_auth_server_function(function) {
        return Some(AuthRole::Anonymous);
    }
    if is_admin_server_function(function) {
        Some(AuthRole::Admin)
    } else if is_user_server_function(function) {
        Some(AuthRole::User)
    } else {
        Some(AuthRole::Admin)
    }
}

fn normalized_server_function_path(path: &str) -> Option<String> {
    let function = path.strip_prefix("/sfn/")?.trim_matches('/');
    let function = function.split('/').next().unwrap_or(function);
    let function = function.trim_end_matches(|ch: char| ch.is_ascii_digit());
    Some(normalize_server_function_name(function))
}

fn is_public_auth_server_function(function: &str) -> bool {
    matches!(
        function,
        "get_auth_status"
            | "login_with_replay_code"
            | "login_admin"
            | "complete_first_setup"
            | "logout"
    )
}

fn is_public_during_first_setup(path: &str) -> bool {
    if path == "/first-setup"
        || path == "/waiting"
        || path == "/api/version"
        || path.starts_with("/static/")
    {
        return true;
    }
    normalized_server_function_path(path).is_some_and(|function| {
        matches!(
            function.as_str(),
            "get_auth_status" | "complete_first_setup" | "logout"
        )
    })
}

#[cfg(test)]
fn is_explicitly_classified_server_function(function: &str) -> bool {
    let function = normalize_server_function_name(function);
    let function = function.as_str();
    is_public_auth_server_function(function)
        || is_admin_server_function(function)
        || is_user_read_server_function(function)
        || is_user_server_function(function)
}

fn is_admin_page_route(path: &str) -> bool {
    matches!(
        path,
        "/settings/wifi"
            | "/settings/nfs"
            | "/settings/hostname"
            | "/settings/retroachievements"
            | "/settings/replayos"
            | "/settings/replay-net-control"
            | "/settings/game-library"
            | "/settings/metadata"
            | "/settings/logs"
            | "/settings/github"
            | "/updating"
    )
}

fn is_user_route(method: &axum::http::Method, path: &str) -> bool {
    use axum::http::Method;

    matches!(
        (method, path),
        (&Method::POST, "/api/favorites")
            | (&Method::DELETE, "/api/favorites")
            | (&Method::PUT, "/api/favorites/group")
            | (&Method::PUT, "/api/favorites/flatten")
    ) || (method == Method::POST && path.starts_with("/api/manuals/upload/"))
}

fn is_user_read_server_function(function: &str) -> bool {
    matches!(
        function,
        "get_systems"
            | "get_recents"
            | "get_roms_page"
            | "get_rom_detail"
            | "get_rom_file_group"
            | "global_search"
            | "get_all_genres"
            | "get_system_genres"
            | "search_by_developer"
            | "get_developer_genres"
            | "get_developer_games"
            | "search_by_board"
            | "get_board_genres"
            | "get_board_games"
            | "random_game"
            | "random_game_for_system"
            | "get_related_games"
            | "get_recommendations"
            | "get_game_documents"
            | "get_local_manuals"
            | "get_game_manual_suggestions"
            | "get_game_resource_links"
            | "get_game_videos"
            | "get_provider_game_videos"
            | "search_game_videos"
            | "get_boxart_variants"
            // Read-only: the update banner (shown to every user, not just
            // admins) renders the "what's new" changelog from this.
            | "get_update_changelog"
    )
}

fn normalize_server_function_name(function: &str) -> String {
    if function.contains('_') {
        return function.to_ascii_lowercase();
    }

    let chars = function.chars().collect::<Vec<_>>();
    let mut normalized = String::with_capacity(function.len() + 8);
    for (index, ch) in chars.iter().copied().enumerate() {
        if ch.is_ascii_uppercase() {
            let prev = index.checked_sub(1).and_then(|i| chars.get(i)).copied();
            let next = chars.get(index + 1).copied();
            if index > 0
                && (prev.is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
                    || next.is_some_and(|c| c.is_ascii_lowercase()))
            {
                normalized.push('_');
            }
            normalized.push(ch.to_ascii_lowercase());
        } else if ch == '-' {
            normalized.push('_');
        } else {
            normalized.push(ch);
        }
    }
    normalized
}

fn is_admin_server_function(function: &str) -> bool {
    matches!(
        function,
        "get_wifi_config"
            | "save_wifi_config"
            | "get_nfs_config"
            | "save_nfs_config"
            | "get_retroachievements_config"
            | "save_retroachievements_config_and_restart"
            | "reboot_system"
            | "power_off_replayos_device"
            | "downgrade_admin_to_user"
            | "logout_all_browsers"
            | "get_admin_session_timeout"
            | "set_admin_session_timeout"
            | "get_hostname"
            | "save_hostname"
            | "change_root_password"
            | "clear_metadata"
            | "regenerate_metadata"
            | "download_metadata"
            | "clear_images"
            | "cleanup_orphaned_images"
            | "get_metadata_library_overview"
            | "get_metadata_page_snapshot"
            | "get_system_logs"
            | "get_log_level_config"
            | "save_log_level_config"
            | "get_replayos_log_level"
            | "get_github_api_key"
            | "save_github_api_key"
            | "save_region_preference"
            | "save_region_preference_secondary"
            | "save_language_preference"
            | "refresh_storage"
            | "regenerate_tls_certificate_info"
            | "get_tls_certificate_info"
            | "get_analytics_preference"
            | "delete_rom"
            | "rename_rom"
            | "get_replayos_settings"
            | "enable_replay_api_assisted"
            | "verify_replay_api_token"
            | "save_replayos_kiosk_mode"
            | "start_setup_metadata_downloads"
            | "update_thumbnails"
            | "cancel_thumbnail_update"
            | "clear_thumbnail_index"
            | "rescan_game_library"
            | "rebuild_game_library"
            | "rebuild_corrupt_library"
            | "repair_corrupt_user_data"
            | "restore_user_data_backup"
            | "check_for_updates"
            | "get_update_channel"
            | "save_update_channel"
            | "skip_version"
            | "start_update"
            | "save_analytics_preference"
    )
}

fn is_user_server_function(function: &str) -> bool {
    is_user_read_server_function(function)
        || matches!(
            function,
            "get_info"
                | "get_live_stats"
                | "get_mode"
                | "get_favorites"
                | "get_system_favorites"
                | "add_favorite"
                | "remove_favorite"
                | "organize_favorites"
                | "group_favorites"
                | "flatten_favorites"
                | "get_favorites_recommendations"
                | "delete_recent"
                | "get_user_captures"
                | "delete_user_capture"
                | "launch_game"
                | "get_replay_api_status"
                | "get_library_playtime"
                | "get_game_playtime"
                | "reprobe_replay_api"
                | "send_replay_player_command"
                | "send_replayos_message"
                | "restart_replayos_game"
                | "get_save_state_slots"
                | "add_game_resource_link"
                | "remove_game_resource_link"
                | "add_game_video"
                | "remove_game_video"
                | "download_manual"
                | "delete_manual"
                | "set_boxart_override"
                | "reset_boxart_override"
                | "get_setup_status"
                | "dismiss_setup"
                | "get_skins"
                | "set_skin"
                | "set_skin_sync"
                | "get_font_size"
                | "save_font_size"
                | "get_region_preference"
                | "get_region_preference_secondary"
                | "get_language_preference"
                | "get_locale"
                | "save_locale"
                | "get_preferred_languages"
        )
}

fn session_token_from_cookie(value: Option<&axum::http::HeaderValue>) -> Option<String> {
    value?
        .to_str()
        .ok()?
        .split(';')
        .filter_map(|part| part.trim().split_once('='))
        .find_map(|(name, value)| {
            let value = value.trim();
            (name == "ReplayControlSession" && valid_session_cookie_value(value))
                .then(|| value.to_string())
        })
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
    let skin_css = replay_control_core::skins::theme_css(skin_index).unwrap_or_default();
    let theme_color = replay_control_core::skins::theme_color(skin_index);
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

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_waiting_page_test_state(storage_root: &std::path::Path) -> AppState {
        std::fs::create_dir_all(storage_root.join("roms")).unwrap();
        std::fs::create_dir_all(storage_root.join("config")).unwrap();
        std::fs::write(
            storage_root.join("config/replay.cfg"),
            "system_storage=\"nfs\"\nsystem_skin=0\n",
        )
        .unwrap();

        AppState::new(
            Some(storage_root.to_string_lossy().into_owned()),
            None,
            None,
        )
        .unwrap()
    }

    /// `redetect_storage`'s symmetric pre-flight: when the re-attached
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

        let raw = DbPool::new(
            valid_path,
            "user_data_db",
            open_user_data_db,
            USER_DATA_READ_POOL_SIZE,
        )
        .unwrap();
        let pool = db_pools::UserDataWritePool::from_pool(raw);
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

        let raw = DbPool::new(
            initial_path,
            "user_data_db",
            open_user_data_db,
            USER_DATA_READ_POOL_SIZE,
        )
        .unwrap();
        let pool = db_pools::UserDataWritePool::from_pool(raw);

        reopen_user_data_or_mark_corrupt(&pool, &new_path).await;

        assert!(!pool.is_corrupt(), "healthy header must not flag corrupt");
        assert_eq!(pool.db_path(), new_path, "pool must reopen at new path");
    }

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

    #[tokio::test(flavor = "multi_thread")]
    async fn rom_watcher_status_broadcasts_only_on_transition() {
        let tmp = tempfile::tempdir().unwrap();
        let state = build_waiting_page_test_state(tmp.path());
        let mut rx = state.events_tx.subscribe();
        let failed = RomWatcherStatus::Failed {
            reason: "inotify max_user_watches exceeded".into(),
        };

        state.set_rom_watcher_status_for_test(failed.clone());
        state.set_rom_watcher_status_for_test(failed);

        let event = rx.try_recv().expect("first transition should broadcast");
        assert!(matches!(
            event,
            ConfigEvent::RomWatcherStatusChanged {
                status: RomWatcherStatus::Failed { .. }
            }
        ));
        assert!(
            rx.try_recv().is_err(),
            "duplicate status should not broadcast"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rom_watcher_status_skipped_does_not_block_active_transition() {
        let tmp = tempfile::tempdir().unwrap();
        let state = build_waiting_page_test_state(tmp.path());
        let mut rx = state.events_tx.subscribe();

        state.set_rom_watcher_status_for_test(RomWatcherStatus::Skipped {
            reason: "NFS".into(),
        });
        state.set_rom_watcher_status_for_test(RomWatcherStatus::Active);

        let _ = rx.try_recv().expect("Skipped should broadcast");
        let event = rx.try_recv().expect("Active transition should broadcast");
        assert!(matches!(
            event,
            ConfigEvent::RomWatcherStatusChanged {
                status: RomWatcherStatus::Active
            }
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn storage_status_broadcasts_only_on_transition() {
        let tmp = tempfile::tempdir().unwrap();
        let state = build_waiting_page_test_state(tmp.path());
        let mut rx = state.events_tx.subscribe();
        let status = StorageStatus::Misconfigured {
            wanted: "nvme".into(),
            current_kind: Some("usb".into()),
            reason: "not mounted".into(),
        };

        state.set_storage_status_for_test(status.clone());
        state.set_storage_status_for_test(status);

        let event = rx.try_recv().expect("first transition should broadcast");
        assert!(matches!(
            event,
            ConfigEvent::StorageStatusChanged {
                status: StorageStatus::Misconfigured { .. }
            }
        ));
        assert!(
            rx.try_recv().is_err(),
            "duplicate status should not broadcast"
        );
    }

    #[test]
    fn fallback_probe_skips_nfs_storage() {
        assert!(storage_kind_is_safe_for_fallback_probe(StorageKind::Sd));
        assert!(storage_kind_is_safe_for_fallback_probe(StorageKind::Usb));
        assert!(storage_kind_is_safe_for_fallback_probe(StorageKind::Nvme));
        assert!(storage_kind_is_safe_for_fallback_probe(StorageKind::Folder));
        assert!(!storage_kind_is_safe_for_fallback_probe(StorageKind::Nfs));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn mutation_guard_blocks_misconfigured_fallback_storage() {
        let tmp = tempfile::tempdir().unwrap();
        let state = build_waiting_page_test_state(tmp.path());
        state.set_storage_status_for_test(StorageStatus::Misconfigured {
            wanted: "nvme".into(),
            current_kind: Some("usb".into()),
            reason: "not mounted".into(),
        });

        let err = state
            .require_configured_storage_ready_for_mutation("launch games")
            .await
            .expect_err("misconfigured fallback must block mutations");

        assert!(err.contains("Cannot launch games"));
        assert!(err.contains("configured storage nvme is unavailable"));
        assert!(err.contains("using usb as fallback"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn mutation_guard_allows_ready_storage() {
        let tmp = tempfile::tempdir().unwrap();
        let state = build_waiting_page_test_state(tmp.path());
        state.set_storage_status_for_test(StorageStatus::Ready);

        state
            .require_configured_storage_ready_for_mutation("launch games")
            .await
            .expect("ready storage should allow mutations");
    }

    #[test]
    fn auth_guard_keeps_bootstrap_server_functions_public() {
        assert_eq!(
            server_function_required_role("/sfn/get_auth_status"),
            Some(AuthRole::Anonymous)
        );
        assert_eq!(
            server_function_required_role("/sfn/get_auth_status10507523110576576594"),
            Some(AuthRole::Anonymous)
        );
        assert_eq!(
            server_function_required_role("/sfn/GetAuthStatus"),
            Some(AuthRole::Anonymous)
        );
        assert_eq!(
            server_function_required_role("/sfn/login_with_replay_code"),
            Some(AuthRole::Anonymous)
        );
        assert_eq!(
            server_function_required_role("/sfn/LoginWithReplayCode"),
            Some(AuthRole::Anonymous)
        );
        assert_eq!(
            server_function_required_role("/sfn/complete_first_setup"),
            Some(AuthRole::Anonymous)
        );
        assert_eq!(
            server_function_required_role("/sfn/CompleteFirstSetup"),
            Some(AuthRole::Anonymous)
        );
    }

    #[test]
    fn first_setup_public_paths_are_narrow() {
        assert!(is_public_during_first_setup("/first-setup"));
        assert!(is_public_during_first_setup("/static/style.css"));
        assert!(is_public_during_first_setup("/api/version"));
        assert!(is_public_during_first_setup("/sfn/get_auth_status"));
        assert!(is_public_during_first_setup("/sfn/complete_first_setup"));

        assert!(!is_public_during_first_setup("/login"));
        assert!(!is_public_during_first_setup("/settings"));
        assert!(!is_public_during_first_setup("/games/nintendo_nes"));
        assert!(!is_public_during_first_setup("/sfn/login_admin"));
        assert!(!is_public_during_first_setup("/sfn/login_with_replay_code"));
    }

    #[test]
    fn auth_guard_classifies_admin_and_user_server_functions() {
        assert_eq!(
            server_function_required_role("/sfn/save_wifi_config"),
            Some(AuthRole::Admin)
        );
        assert_eq!(
            server_function_required_role("/sfn/SaveWifiConfig"),
            Some(AuthRole::Admin)
        );
        assert_eq!(
            server_function_required_role("/sfn/get_wifi_config"),
            Some(AuthRole::Admin)
        );
        assert_eq!(
            server_function_required_role("/sfn/get_hostname"),
            Some(AuthRole::Admin)
        );
        assert_eq!(
            server_function_required_role("/sfn/delete_rom"),
            Some(AuthRole::Admin)
        );
        assert_eq!(
            server_function_required_role("/sfn/DeleteRom"),
            Some(AuthRole::Admin)
        );
        assert_eq!(
            server_function_required_role("/sfn/rename_rom"),
            Some(AuthRole::Admin)
        );
        assert_eq!(
            server_function_required_role("/sfn/refresh_storage"),
            Some(AuthRole::Admin)
        );
        assert_eq!(
            server_function_required_role("/sfn/save_region_preference"),
            Some(AuthRole::Admin)
        );
        assert_eq!(
            server_function_required_role("/sfn/save_region_preference_secondary"),
            Some(AuthRole::Admin)
        );
        assert_eq!(
            server_function_required_role("/sfn/save_language_preference"),
            Some(AuthRole::Admin)
        );
        assert_eq!(
            server_function_required_role("/sfn/downgrade_admin_to_user"),
            Some(AuthRole::Admin)
        );
        assert_eq!(
            server_function_required_role("/sfn/logout_all_browsers"),
            Some(AuthRole::Admin)
        );
        assert_eq!(
            server_function_required_role("/sfn/get_admin_session_timeout"),
            Some(AuthRole::Admin)
        );
        assert_eq!(
            server_function_required_role("/sfn/set_admin_session_timeout"),
            Some(AuthRole::Admin)
        );
        assert_eq!(
            server_function_required_role("/sfn/get_tls_certificate_info"),
            Some(AuthRole::Admin)
        );
        assert_eq!(
            server_function_required_role("/sfn/regenerate_tls_certificate_info"),
            Some(AuthRole::Admin)
        );
        assert_eq!(
            server_function_required_role("/sfn/get_metadata_page_snapshot"),
            Some(AuthRole::Admin)
        );
        assert_eq!(
            server_function_required_role("/sfn/get_metadata_library_overview"),
            Some(AuthRole::Admin)
        );
        assert_eq!(
            server_function_required_role("/sfn/completely_new_server_function"),
            Some(AuthRole::Admin)
        );
        assert_eq!(
            server_function_required_role("/sfn/add_favorite"),
            Some(AuthRole::User)
        );
        assert_eq!(
            server_function_required_role("/sfn/launch_game"),
            Some(AuthRole::User)
        );
        assert_eq!(
            server_function_required_role("/sfn/send_replay_player_command"),
            Some(AuthRole::User)
        );
        assert_eq!(
            server_function_required_role("/sfn/save_locale"),
            Some(AuthRole::User)
        );
        assert_eq!(
            server_function_required_role("/sfn/save_font_size"),
            Some(AuthRole::User)
        );
    }

    #[test]
    fn auth_guard_classifies_every_server_function_intentionally() {
        let names = discovered_server_function_names();
        assert!(
            names.len() > 100,
            "server-function inventory unexpectedly small: {}",
            names.len()
        );

        let missing = names
            .iter()
            .filter(|name| !is_explicitly_classified_server_function(name))
            .cloned()
            .collect::<Vec<_>>();

        assert!(
            missing.is_empty(),
            "server functions need explicit auth classification: {missing:?}"
        );
    }

    /// Every discovered `#[server]` function must be registered via
    /// `register_explicit` in `main.rs`. An unregistered function resolves on
    /// the initial SSR render (direct call) but 404s when a client-side
    /// navigation re-runs its resource as an HTTP POST — a silent break on the
    /// SPA path only. This closes that drift class (two functions had slipped
    /// through: `get_update_changelog`, `get_replayos_log_level`).
    #[test]
    fn every_server_function_is_registered_in_main() {
        let main_src = include_str!("../main.rs");
        // Collapse whitespace so registrations split across lines
        // (`register_explicit::<...,\n>();`) match the same way single-line
        // ones do, then pull the last `::`-segment from each type argument.
        let joined: String = main_src.split_whitespace().collect();
        let needle = "register_explicit::<";
        let mut registered = std::collections::HashSet::new();
        let mut rest = joined.as_str();
        while let Some(pos) = rest.find(needle) {
            rest = &rest[pos + needle.len()..];
            if let Some(end) = rest.find('>') {
                let ty = rest[..end].trim_end_matches(',');
                let name = ty.rsplit("::").next().unwrap_or(ty);
                registered.insert(name.to_string());
            }
        }

        let to_pascal = |snake: &str| -> String {
            snake
                .split('_')
                .map(|seg| {
                    let mut chars = seg.chars();
                    match chars.next() {
                        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                        None => String::new(),
                    }
                })
                .collect()
        };

        let missing = discovered_server_function_names()
            .iter()
            .map(|name| to_pascal(name))
            .filter(|struct_name| !registered.contains(struct_name))
            .collect::<Vec<_>>();

        assert!(
            missing.is_empty(),
            "server functions defined but not registered in main.rs (they 404 on client-side nav): {missing:?}"
        );
    }

    #[test]
    fn auth_guard_inventory_covers_every_server_function_file() {
        let server_fns_dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/server_fns");
        let mut files = std::fs::read_dir(&server_fns_dir)
            .unwrap()
            .filter_map(|entry| {
                let name = entry.ok()?.file_name().into_string().ok()?;
                (name.ends_with(".rs") && name != "mod.rs").then_some(name)
            })
            .collect::<Vec<_>>();
        files.sort();

        let mut inventoried = SERVER_FUNCTION_SOURCES
            .iter()
            .map(|(name, _)| (*name).to_string())
            .collect::<Vec<_>>();
        inventoried.sort();

        assert_eq!(
            inventoried, files,
            "server function source files must be added to the auth inventory"
        );
    }

    #[test]
    fn auth_guard_classifies_rest_mutations() {
        assert_eq!(
            route_required_role(&axum::http::Method::POST, "/api/upload/snes"),
            AuthRole::Admin
        );
        assert_eq!(
            route_required_role(&axum::http::Method::GET, "/api/upload/targets"),
            AuthRole::Admin
        );
        assert_eq!(
            route_required_role(&axum::http::Method::POST, "/api/manuals/upload/snes"),
            AuthRole::User
        );
        assert_eq!(
            route_required_role(&axum::http::Method::PUT, "/api/roms/rename"),
            AuthRole::Admin
        );
        assert_eq!(
            route_required_role(&axum::http::Method::DELETE, "/api/roms"),
            AuthRole::Admin
        );
        assert_eq!(
            route_required_role(&axum::http::Method::POST, "/api/favorites"),
            AuthRole::User
        );
        assert_eq!(
            route_required_role(&axum::http::Method::DELETE, "/api/favorites"),
            AuthRole::User
        );
        assert_eq!(
            route_required_role(&axum::http::Method::POST, "/api/new-mutation"),
            AuthRole::Admin
        );
        assert_eq!(
            route_required_role(&axum::http::Method::GET, "/settings/wifi"),
            AuthRole::Admin
        );
    }

    #[test]
    fn unauthenticated_browse_routes_require_user_access() {
        assert_eq!(
            route_required_role(&axum::http::Method::GET, "/games/nes"),
            AuthRole::User
        );
        assert_eq!(
            route_required_role(&axum::http::Method::GET, "/api/systems"),
            AuthRole::User
        );
        assert_eq!(
            route_required_role(&axum::http::Method::GET, "/media/nes/Mario.png"),
            AuthRole::User
        );
        assert_eq!(
            route_required_role(&axum::http::Method::GET, "/api/favorites"),
            AuthRole::User
        );
        assert_eq!(
            route_required_role(&axum::http::Method::GET, "/api/info"),
            AuthRole::User
        );
        assert_eq!(
            route_required_role(&axum::http::Method::GET, "/settings"),
            AuthRole::User
        );
        assert_eq!(
            server_function_required_role("/sfn/get_systems"),
            Some(AuthRole::User)
        );
        assert_eq!(
            server_function_required_role("/sfn/add_favorite"),
            Some(AuthRole::User)
        );
    }

    #[test]
    fn login_redirect_preserves_local_next_path() {
        assert_eq!(
            login_redirect_target(&"/settings/wifi?mode=manual".parse().unwrap()),
            "/login?next=%2Fsettings%2Fwifi%3Fmode%3Dmanual"
        );
        assert_eq!(
            login_redirect_target(&"/login".parse().unwrap()),
            "/login?next=%2F"
        );
    }

    #[test]
    fn access_redirect_preserves_local_next_path_without_looping() {
        assert_eq!(
            access_redirect_target(&"/settings/wifi?mode=manual".parse().unwrap()),
            "/settings/access?next=%2Fsettings%2Fwifi%3Fmode%3Dmanual"
        );
        assert_eq!(
            access_redirect_target(&"/settings/access".parse().unwrap()),
            "/settings/access?next=%2Fsettings"
        );
    }

    #[test]
    fn route_auth_cookie_parser_trims_and_rejects_malformed_values() {
        let valid = axum::http::HeaderValue::from_static(
            "theme=dark; ReplayControlSession= abc.def ; other=value",
        );
        assert_eq!(
            session_token_from_cookie(Some(&valid)).as_deref(),
            Some("abc.def")
        );

        let empty = axum::http::HeaderValue::from_static("ReplayControlSession= ");
        assert_eq!(session_token_from_cookie(Some(&empty)), None);

        let unsigned = axum::http::HeaderValue::from_static("ReplayControlSession=abc");
        assert_eq!(session_token_from_cookie(Some(&unsigned)), None);

        let too_many_segments =
            axum::http::HeaderValue::from_static("ReplayControlSession=abc.def.ghi");
        assert_eq!(session_token_from_cookie(Some(&too_many_segments)), None);

        let control = axum::http::HeaderValue::from_static("ReplayControlSession=abc\tdef");
        assert_eq!(session_token_from_cookie(Some(&control)), None);
    }

    #[test]
    fn csrf_check_accepts_same_origin() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::HOST,
            axum::http::HeaderValue::from_static("replay.local:8443"),
        );
        headers.insert(
            axum::http::header::ORIGIN,
            axum::http::HeaderValue::from_static("https://replay.local:8443"),
        );

        assert!(passes_csrf_origin_check(&headers));
    }

    #[test]
    fn csrf_check_accepts_same_origin_referer() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::HOST,
            axum::http::HeaderValue::from_static("192.168.1.20:8443"),
        );
        headers.insert(
            axum::http::header::REFERER,
            axum::http::HeaderValue::from_static("https://192.168.1.20:8443/settings"),
        );

        assert!(passes_csrf_origin_check(&headers));
    }

    #[test]
    fn csrf_check_accepts_same_origin_fetch_metadata() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            "sec-fetch-site",
            axum::http::HeaderValue::from_static("same-origin"),
        );
        assert!(passes_csrf_origin_check(&headers));

        headers.insert(
            "sec-fetch-site",
            axum::http::HeaderValue::from_static("none"),
        );
        assert!(!passes_csrf_origin_check(&headers));

        headers.insert(
            axum::http::header::HOST,
            axum::http::HeaderValue::from_static("replay.local:8443"),
        );
        headers.insert(
            axum::http::header::ORIGIN,
            axum::http::HeaderValue::from_static("https://replay.local:8443"),
        );
        assert!(passes_csrf_origin_check(&headers));

        headers.insert(
            "sec-fetch-site",
            axum::http::HeaderValue::from_static("cross-site"),
        );
        assert!(!passes_csrf_origin_check(&headers));
    }

    #[test]
    fn csrf_check_rejects_cross_origin_or_missing_headers() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::HOST,
            axum::http::HeaderValue::from_static("replay.local:8443"),
        );
        headers.insert(
            axum::http::header::ORIGIN,
            axum::http::HeaderValue::from_static("https://example.com"),
        );
        assert!(!passes_csrf_origin_check(&headers));

        headers.remove(axum::http::header::ORIGIN);
        assert!(!passes_csrf_origin_check(&headers));
    }

    const SERVER_FUNCTION_SOURCES: &[(&str, &str)] = &[
        ("auth.rs", include_str!("../server_fns/auth.rs")),
        ("boxart.rs", include_str!("../server_fns/boxart.rs")),
        ("favorites.rs", include_str!("../server_fns/favorites.rs")),
        ("images.rs", include_str!("../server_fns/images.rs")),
        ("manuals.rs", include_str!("../server_fns/manuals.rs")),
        ("metadata.rs", include_str!("../server_fns/metadata.rs")),
        (
            "recommendations.rs",
            include_str!("../server_fns/recommendations.rs"),
        ),
        ("related.rs", include_str!("../server_fns/related.rs")),
        ("replay_api.rs", include_str!("../server_fns/replay_api.rs")),
        ("resources.rs", include_str!("../server_fns/resources.rs")),
        ("roms.rs", include_str!("../server_fns/roms.rs")),
        (
            "save_states.rs",
            include_str!("../server_fns/save_states.rs"),
        ),
        ("search.rs", include_str!("../server_fns/search.rs")),
        ("settings.rs", include_str!("../server_fns/settings.rs")),
        ("system.rs", include_str!("../server_fns/system.rs")),
        ("thumbnails.rs", include_str!("../server_fns/thumbnails.rs")),
        ("videos.rs", include_str!("../server_fns/videos.rs")),
    ];

    fn discovered_server_function_names() -> Vec<String> {
        let mut names = SERVER_FUNCTION_SOURCES
            .iter()
            .flat_map(|(_, source)| source.lines())
            .filter_map(server_function_name_from_line)
            .collect::<Vec<_>>();
        names.sort();
        names.dedup();
        names
    }

    fn server_function_name_from_line(line: &str) -> Option<String> {
        let rest = line.trim_start().strip_prefix("pub async fn ")?;
        let name = rest
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
            .collect::<String>();
        (!name.is_empty()).then_some(name)
    }
}
