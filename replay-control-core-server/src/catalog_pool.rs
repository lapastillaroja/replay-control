//! Async read-only connection pool for the bundled `catalog.sqlite`.
//!
//! The catalog is read-only and lives on local storage bundled with the binary
//! (not USB/NFS), so WAL concerns don't apply — concurrent readers are safe.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use deadpool::managed::{self, Metrics, Pool, PoolConfig, RecycleResult};
use deadpool_sqlite::Runtime;
use deadpool_sync::SyncWrapper;
use replay_control_core::library::resource_kind;

static CATALOG_POOL: OnceLock<Pool<CatalogManager>> = OnceLock::new();

/// Set to `true` at startup if the bundled catalog's `arcade_game` schema
/// doesn't match what the running binary expects. When set, `with_catalog`
/// short-circuits arcade queries to `None` instead of spamming WARN-per-row
/// SQL errors; non-arcade systems are unaffected.
static CATALOG_SCHEMA_OUTDATED: OnceLock<bool> = OnceLock::new();

/// Returns true if startup detected a catalog schema mismatch. Test-only
/// helper for forcing the flag is in `#[cfg(test)]` below.
pub fn schema_outdated() -> bool {
    CATALOG_SCHEMA_OUTDATED.get().copied().unwrap_or(false)
}

const DEFAULT_POOL_SIZE: usize = 2;
const DEFAULT_CACHE_KIB: i64 = 2048;
const DEFAULT_MMAP_MB: u64 = 64;

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

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(default)
}

#[derive(Debug, thiserror::Error)]
pub enum CatalogInitError {
    #[error("pool build failed: {0}")]
    Build(String),
    #[error("connection failed: {0}")]
    Connection(String),
    #[error(transparent)]
    Db(#[from] rusqlite::Error),
}

struct CatalogManager {
    path: PathBuf,
    cache_kib: i64,
    mmap_bytes: u64,
}

impl managed::Manager for CatalogManager {
    type Type = SyncWrapper<rusqlite::Connection>;
    type Error = rusqlite::Error;

    async fn create(&self) -> Result<Self::Type, Self::Error> {
        let path = self.path.clone();
        let cache_kib = self.cache_kib;
        let mmap_bytes = self.mmap_bytes;
        SyncWrapper::new(Runtime::Tokio1, move || {
            let flags = rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
                | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX
                | rusqlite::OpenFlags::SQLITE_OPEN_URI;
            let conn = rusqlite::Connection::open_with_flags(&path, flags)?;
            conn.execute_batch(&format!(
                "PRAGMA mmap_size = {mmap_bytes}; PRAGMA cache_size = -{cache_kib}; PRAGMA temp_store = MEMORY;"
            ))?;
            Ok(conn)
        })
        .await
    }

    async fn recycle(
        &self,
        _conn: &mut Self::Type,
        _metrics: &Metrics,
    ) -> RecycleResult<Self::Error> {
        Ok(())
    }
}

pub async fn init_catalog(path: impl AsRef<std::path::Path>) -> Result<(), CatalogInitError> {
    let pool_size = env_usize("REPLAY_CATALOG_POOL_SIZE", DEFAULT_POOL_SIZE);
    let cache_kib = env_i64("REPLAY_CATALOG_CACHE_KB", DEFAULT_CACHE_KIB);
    let mmap_bytes = env_u64("REPLAY_CATALOG_MMAP_MB", DEFAULT_MMAP_MB) * 1024 * 1024;
    tracing::info!(
        "catalog pool: {pool_size} read connection(s), {cache_kib} KiB cache, {mmap_bytes} mmap bytes"
    );
    let pool = Pool::builder(CatalogManager {
        path: path.as_ref().to_path_buf(),
        cache_kib,
        mmap_bytes,
    })
    .config(PoolConfig::new(pool_size))
    .runtime(Runtime::Tokio1)
    .build()
    .map_err(|e| CatalogInitError::Build(e.to_string()))?;

    let conn: managed::Object<CatalogManager> = pool
        .get()
        .await
        .map_err(|e| CatalogInitError::Connection(e.to_string()))?;

    // Two checks under one connection: arcade_game exists, and its column
    // set matches what the runtime expects. The second guards against a
    // partial upgrade where the binary was replaced but the catalog wasn't.
    let outdated = conn
        .interact(|c: &mut rusqlite::Connection| {
            c.query_row(
                "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'arcade_game'",
                [],
                |_| Ok(()),
            )?;
            Ok(crate::sqlite::table_columns_diverge(
                c,
                "arcade_game",
                crate::game::arcade_db::ARCADE_COL_NAMES,
            ))
        })
        .await
        .map_err(|e| CatalogInitError::Connection(e.to_string()))?
        .map_err(|e: rusqlite::Error| {
            CatalogInitError::Connection(format!("catalog schema missing: {e}"))
        })?;
    drop(conn);

    if outdated {
        let _ = CATALOG_SCHEMA_OUTDATED.set(true);
        tracing::error!(
            target: "telemetry",
            event = "catalog_outdated",
            "Catalog out of date: arcade_game column set does not match runtime expectation. \
             Reinstall Replay Control to refresh /usr/local/bin/catalog.sqlite."
        );
    }

    let _ = CATALOG_POOL.set(pool);
    Ok(())
}

pub async fn with_catalog<F, T>(f: F) -> Option<T>
where
    F: FnOnce(&rusqlite::Connection) -> rusqlite::Result<T> + Send + 'static,
    T: Send + 'static,
{
    if schema_outdated() {
        return None;
    }
    let pool = CATALOG_POOL.get()?;
    let conn = match pool.get().await {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("catalog pool.get failed: {e}");
            return None;
        }
    };
    match conn.interact(move |c| f(c)).await {
        Ok(Ok(v)) => Some(v),
        Ok(Err(e)) => {
            tracing::warn!("catalog query failed: {e}");
            None
        }
        Err(e) => {
            tracing::warn!("catalog interact failed: {e}");
            None
        }
    }
}

#[derive(Debug, Clone)]
pub struct CatalogGameResourceRow {
    pub source: String,
    pub resource_type: String,
    pub resource_id: String,
    pub url: String,
    pub title: String,
    pub languages: String,
    pub mime_type: String,
}

#[derive(Debug, Clone, Default)]
pub struct CatalogResourceStats {
    pub total_resources: usize,
    pub manual_resources: usize,
    pub mister_manual_resources: usize,
    pub retrokit_manual_resources: usize,
    pub shmups_wiki_resources: usize,
    pub shmups_wiki_strategy_guides: usize,
    pub shmups_wiki_video_indexes: usize,
}

pub async fn catalog_resource_stats() -> CatalogResourceStats {
    with_catalog(|conn| {
        let mut stmt = conn.prepare_cached(
            "SELECT source, resource_type, COUNT(*)
             FROM catalog_game_resource
             GROUP BY source, resource_type",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        let mut stats = CatalogResourceStats::default();
        for row in rows {
            let (source, resource_type, count) = row?;
            let count = count.max(0) as usize;
            stats.total_resources += count;
            if resource_type == resource_kind::MANUAL {
                stats.manual_resources += count;
                match source.as_str() {
                    resource_kind::MISTER_MANUALS_SOURCE => stats.mister_manual_resources = count,
                    resource_kind::RETROKIT_SOURCE => stats.retrokit_manual_resources = count,
                    _ => {}
                }
            }
            if source == resource_kind::SHMUPS_WIKI_SOURCE {
                stats.shmups_wiki_resources += count;
                match resource_type.as_str() {
                    resource_kind::STRATEGY_GUIDE => stats.shmups_wiki_strategy_guides = count,
                    resource_kind::VIDEO_INDEX => stats.shmups_wiki_video_indexes = count,
                    _ => {}
                }
            }
        }
        Ok(stats)
    })
    .await
    .unwrap_or_default()
}

pub async fn catalog_resource_version() -> Option<String> {
    with_catalog(|conn| {
        conn.query_row(
            "SELECT value FROM db_meta WHERE key = 'catalog_resource_version'",
            [],
            |row| row.get::<_, String>(0),
        )
    })
    .await
}

pub async fn lookup_catalog_game_resources(
    system: &str,
) -> HashMap<String, Vec<CatalogGameResourceRow>> {
    let system = system.to_string();
    with_catalog(move |conn| {
        let mut stmt = conn.prepare_cached(
            "SELECT normalized_title, source, resource_type, resource_id, url, title, languages, mime_type
             FROM catalog_game_resource
             WHERE system = ?1 OR system = ?2
             ORDER BY normalized_title,
                      CASE WHEN system = ?1 THEN 0 ELSE 1 END,
                      resource_type, source, title, url",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![system, resource_kind::GLOBAL_SYSTEM],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    CatalogGameResourceRow {
                        source: row.get(1)?,
                        resource_type: row.get(2)?,
                        resource_id: row.get(3)?,
                        url: row.get(4)?,
                        title: row.get(5)?,
                        languages: row.get(6)?,
                        mime_type: row.get(7)?,
                    },
                ))
            },
        )?;
        let mut out: HashMap<String, Vec<CatalogGameResourceRow>> = HashMap::new();
        for row in rows {
            let (key, value) = row?;
            out.entry(key).or_default().push(value);
        }
        Ok(out)
    })
    .await
    .unwrap_or_default()
}

#[cfg(test)]
pub async fn init_test_catalog() {
    use tokio::sync::OnceCell;
    static ONCE: OnceCell<()> = OnceCell::const_new();
    ONCE.get_or_init(|| async {
        let path = std::env::var("REPLAY_CATALOG_PATH")
            .unwrap_or_else(|_| format!("{}/fixtures/catalog.sqlite", env!("CARGO_MANIFEST_DIR")));
        init_catalog(&path).await.unwrap_or_else(|e| {
            panic!(
                "Failed to open catalog at {path}: {e}\n\
                 Run: cargo run -p build-catalog -- --stub \
                 --output replay-control-core-server/fixtures/catalog.sqlite"
            )
        });
    })
    .await;
}

#[cfg(test)]
pub fn using_stub_data() -> bool {
    true
}
