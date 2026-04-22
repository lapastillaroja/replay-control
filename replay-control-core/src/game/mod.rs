pub mod arcade_db;
pub mod date_precision;
pub mod developer;
pub mod game_db;
pub mod game_ref;
pub mod genre;
pub mod rom_tags;
pub mod series_db;
pub mod title_utils;

#[cfg(not(target_arch = "wasm32"))]
mod catalog_pool {
    use std::path::PathBuf;
    use std::sync::OnceLock;

    use deadpool::managed::{self, Metrics, Pool, PoolConfig, RecycleResult};
    use deadpool_sqlite::Runtime;
    use deadpool_sync::SyncWrapper;

    static CATALOG_POOL: OnceLock<Pool<CatalogManager>> = OnceLock::new();

    /// Catalog is read-only and lives on local storage bundled with the binary
    /// (not USB/NFS), so WAL concerns don't apply — concurrent readers are safe.
    /// Sized for the ~6 parallel Suspense resources the metadata page fires
    /// plus headroom for background enrichment / batch lookups.
    const POOL_SIZE: usize = 8;

    const CATALOG_PRAGMAS: &str = "\
        PRAGMA mmap_size = 67108864;\
        PRAGMA cache_size = -8192;\
        PRAGMA temp_store = MEMORY;";

    #[derive(Debug, thiserror::Error)]
    pub enum CatalogInitError {
        #[error("pool build failed: {0}")]
        Build(String),
        #[error("connection failed: {0}")]
        Connection(String),
        #[error(transparent)]
        Db(#[from] rusqlite::Error),
    }

    /// Open catalog connections read-only.
    ///
    /// The catalog is shipped alongside the binary and is never mutated at
    /// runtime. Opening with `SQLITE_OPEN_READ_ONLY` (and *without*
    /// `SQLITE_OPEN_CREATE`) forces a resolvable-but-empty or missing path to
    /// surface as an error at init time, instead of SQLite silently creating
    /// a new schemaless file. A previous incident left a 0-byte
    /// `/catalog.sqlite` at the systemd CWD; with the default flags, the app
    /// opened that file, every query failed with "no such table", and the UI
    /// silently showed ROM filenames without names/metadata.
    struct CatalogManager {
        path: PathBuf,
    }

    impl managed::Manager for CatalogManager {
        type Type = SyncWrapper<rusqlite::Connection>;
        type Error = rusqlite::Error;

        async fn create(&self) -> Result<Self::Type, Self::Error> {
            let path = self.path.clone();
            SyncWrapper::new(Runtime::Tokio1, move || {
                let flags = rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
                    | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX
                    | rusqlite::OpenFlags::SQLITE_OPEN_URI;
                let conn = rusqlite::Connection::open_with_flags(&path, flags)?;
                conn.execute_batch(CATALOG_PRAGMAS)?;
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
        let pool = Pool::builder(CatalogManager {
            path: path.as_ref().to_path_buf(),
        })
        .config(PoolConfig::new(POOL_SIZE))
        .runtime(Runtime::Tokio1)
        .build()
        .map_err(|e| CatalogInitError::Build(e.to_string()))?;

        // Warm the pool and confirm the schema is actually present. A bare
        // `SELECT 1` is not enough: it succeeds even against an empty DB, so
        // a misresolved path wouldn't surface until the first real query.
        let conn: managed::Object<CatalogManager> = pool
            .get()
            .await
            .map_err(|e| CatalogInitError::Connection(e.to_string()))?;
        conn.interact(|c: &mut rusqlite::Connection| {
            c.query_row(
                "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'arcade_games'",
                [],
                |_| Ok(()),
            )
        })
        .await
        .map_err(|e| CatalogInitError::Connection(e.to_string()))?
        .map_err(|e| CatalogInitError::Connection(format!("catalog schema missing: {e}")))?;
        drop(conn);

        let _ = CATALOG_POOL.set(pool);
        Ok(())
    }

    pub(crate) async fn with_catalog<F, T>(f: F) -> Option<T>
    where
        F: FnOnce(&rusqlite::Connection) -> rusqlite::Result<T> + Send + 'static,
        T: Send + 'static,
    {
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
}

#[cfg(not(target_arch = "wasm32"))]
pub use catalog_pool::init_catalog;
#[cfg(not(target_arch = "wasm32"))]
#[allow(unused_imports)]
pub use catalog_pool::CatalogInitError;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use catalog_pool::with_catalog;

#[cfg(test)]
pub(crate) async fn init_test_catalog() {
    use tokio::sync::OnceCell;
    static ONCE: OnceCell<()> = OnceCell::const_new();
    ONCE.get_or_init(|| async {
        // Allow override via env var (used in test-full.yml with real data).
        let path = std::env::var("REPLAY_CATALOG_PATH").unwrap_or_else(|_| {
            format!(
                "{}/fixtures/catalog.sqlite",
                env!("CARGO_MANIFEST_DIR")
            )
        });
        init_catalog(&path).await.unwrap_or_else(|e| {
            panic!(
                "Failed to open catalog at {path}: {e}\n\
                 Run: cargo run -p build-catalog -- --stub \
                 --output replay-control-core/fixtures/catalog.sqlite"
            )
        });
    })
    .await;
}

/// In tests, always use the fixture catalog (stub data).
#[cfg(test)]
pub(crate) fn using_stub_data() -> bool {
    true
}
