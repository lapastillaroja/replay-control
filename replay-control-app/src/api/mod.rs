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

    /// Axum-handler variant of
    /// [`Self::require_configured_storage_ready_for_mutation`]: not-ready
    /// storage maps to `409 Conflict` — the mutation is retryable once
    /// storage settles, not a client error.
    pub async fn require_storage_ready_or_conflict(
        &self,
        action: &str,
    ) -> Result<(), axum::http::StatusCode> {
        self.require_configured_storage_ready_for_mutation(action)
            .await
            .map_err(|_| axum::http::StatusCode::CONFLICT)
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

mod auth_gate;
mod waiting;

pub use auth_gate::{is_public_without_auth, with_auth_guard};
pub use waiting::{
    is_allowed_without_storage, waiting_page_html, waiting_page_response, waiting_reboot_response,
    with_storage_guard,
};

/// Shared test fixture: an `AppState` rooted at a temp storage dir with a
/// minimal NFS replay.cfg — used by the waiting-page tests (api::waiting)
/// and the watcher/guard tests below.
#[cfg(test)]
pub(crate) fn build_waiting_page_test_state(storage_root: &std::path::Path) -> AppState {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
