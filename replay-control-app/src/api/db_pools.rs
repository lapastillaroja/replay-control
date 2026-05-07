//! Type-level read/write split for every DB pool the app touches.
//!
//! For each underlying [`DbPool`] (library, external_metadata, user_data)
//! there are two newtypes:
//!
//! - `…ReadPool` exposes only `read` / `try_read` (plus harmless metadata
//!   like `is_corrupt`). A handle of this type cannot mutate the DB.
//! - `…WritePool` exposes `write` / `try_write` / `transaction` (plus
//!   admin ops like `reopen` / `mark_corrupt`). It does **not** expose
//!   `read` — a writer-side read forces an explicit choice: do the read
//!   inside a `write` / `transaction` closure (atomic, same connection),
//!   or take a `…ReadPool` parameter for an intentional separate-
//!   connection read.
//!
//! Both newtypes for a given pool wrap the same underlying [`DbPool`]
//! (`Arc`-shaped — cloning is cheap). A `mark_corrupt` on one is
//! visible to all clones and to the parallel reader; a `reopen` swaps
//! the inner pool for everyone.

use replay_control_core_server::DbPool;
use replay_control_core_server::db_pool::{DbError, rusqlite};

// ── Library ──────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct LibraryReadPool {
    inner: DbPool,
}

impl LibraryReadPool {
    pub(crate) fn from_pool(inner: DbPool) -> Self {
        Self { inner }
    }

    pub async fn read<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&rusqlite::Connection) -> R + Send + 'static,
        R: Send + 'static,
    {
        self.inner.read(f).await
    }

    pub async fn try_read<F, R>(&self, f: F) -> Result<R, DbError>
    where
        F: FnOnce(&rusqlite::Connection) -> R + Send + 'static,
        R: Send + 'static,
    {
        self.inner.try_read(f).await
    }

    pub fn is_corrupt(&self) -> bool {
        self.inner.is_corrupt()
    }

    pub fn db_path(&self) -> std::path::PathBuf {
        self.inner.db_path()
    }
}

#[derive(Clone)]
pub struct LibraryWritePool {
    inner: DbPool,
}

impl LibraryWritePool {
    pub(crate) fn from_pool(inner: DbPool) -> Self {
        Self { inner }
    }

    pub async fn write<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&mut rusqlite::Connection) -> R + Send + 'static,
        R: Send + 'static,
    {
        self.inner.write(f).await
    }

    pub async fn try_write<F, R>(&self, f: F) -> Result<R, DbError>
    where
        F: FnOnce(&mut rusqlite::Connection) -> R + Send + 'static,
        R: Send + 'static,
    {
        self.inner.try_write(f).await
    }

    pub async fn transaction<F, R>(&self, f: F) -> Result<R, DbError>
    where
        F: FnOnce(&rusqlite::Transaction) -> rusqlite::Result<R> + Send + 'static,
        R: Send + 'static,
    {
        self.inner
            .try_write(move |conn| -> rusqlite::Result<R> {
                let tx = conn.transaction()?;
                let value = f(&tx)?;
                tx.commit()?;
                Ok(value)
            })
            .await?
            .map_err(DbError::Sql)
    }

    pub async fn reopen(&self, db_path: &std::path::Path) -> bool {
        self.inner.reopen(db_path).await
    }

    pub async fn reset_to_empty(&self) -> bool {
        self.inner.reset_to_empty().await
    }

    pub fn mark_corrupt(&self) {
        self.inner.mark_corrupt()
    }

    pub fn is_corrupt(&self) -> bool {
        self.inner.is_corrupt()
    }

    pub fn db_path(&self) -> std::path::PathBuf {
        self.inner.db_path()
    }

    /// Crate-internal escape hatch for `title_norm_reconcile`, which
    /// lives in `replay-control-core-server` and signs `&DbPool`.
    pub(crate) fn as_db_pool(&self) -> &DbPool {
        &self.inner
    }
}

// ── External metadata (host-global LaunchBox + libretro manifests) ──

#[derive(Clone)]
pub struct ExternalMetadataReadPool {
    inner: DbPool,
}

impl ExternalMetadataReadPool {
    pub(crate) fn from_pool(inner: DbPool) -> Self {
        Self { inner }
    }

    pub async fn read<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&rusqlite::Connection) -> R + Send + 'static,
        R: Send + 'static,
    {
        self.inner.read(f).await
    }

    pub async fn try_read<F, R>(&self, f: F) -> Result<R, DbError>
    where
        F: FnOnce(&rusqlite::Connection) -> R + Send + 'static,
        R: Send + 'static,
    {
        self.inner.try_read(f).await
    }

    pub fn db_path(&self) -> std::path::PathBuf {
        self.inner.db_path()
    }
}

#[derive(Clone)]
pub struct ExternalMetadataWritePool {
    inner: DbPool,
}

impl ExternalMetadataWritePool {
    pub(crate) fn from_pool(inner: DbPool) -> Self {
        Self { inner }
    }

    pub async fn write<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&mut rusqlite::Connection) -> R + Send + 'static,
        R: Send + 'static,
    {
        self.inner.write(f).await
    }

    pub async fn try_write<F, R>(&self, f: F) -> Result<R, DbError>
    where
        F: FnOnce(&mut rusqlite::Connection) -> R + Send + 'static,
        R: Send + 'static,
    {
        self.inner.try_write(f).await
    }

    pub async fn try_write_with_timeout<F, R>(
        &self,
        timeout: std::time::Duration,
        f: F,
    ) -> Result<R, DbError>
    where
        F: FnOnce(&mut rusqlite::Connection) -> R + Send + 'static,
        R: Send + 'static,
    {
        self.inner.try_write_with_timeout(timeout, f).await
    }

    pub async fn transaction<F, R>(&self, f: F) -> Result<R, DbError>
    where
        F: FnOnce(&rusqlite::Transaction) -> rusqlite::Result<R> + Send + 'static,
        R: Send + 'static,
    {
        self.inner
            .try_write(move |conn| -> rusqlite::Result<R> {
                let tx = conn.transaction()?;
                let value = f(&tx)?;
                tx.commit()?;
                Ok(value)
            })
            .await?
            .map_err(DbError::Sql)
    }

    pub fn db_path(&self) -> std::path::PathBuf {
        self.inner.db_path()
    }

    /// Crate-internal escape hatch for `thumbnail_manifest::import_all_manifests`,
    /// which lives in `replay-control-core-server` and signs `&DbPool`.
    pub(crate) fn as_db_pool(&self) -> &DbPool {
        &self.inner
    }
}

// ── User data (per-storage user state) ───────────────────────────────

#[derive(Clone)]
pub struct UserDataReadPool {
    inner: DbPool,
}

impl UserDataReadPool {
    pub(crate) fn from_pool(inner: DbPool) -> Self {
        Self { inner }
    }

    pub async fn read<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&rusqlite::Connection) -> R + Send + 'static,
        R: Send + 'static,
    {
        self.inner.read(f).await
    }

    pub async fn try_read<F, R>(&self, f: F) -> Result<R, DbError>
    where
        F: FnOnce(&rusqlite::Connection) -> R + Send + 'static,
        R: Send + 'static,
    {
        self.inner.try_read(f).await
    }

    pub fn is_corrupt(&self) -> bool {
        self.inner.is_corrupt()
    }

    pub fn db_path(&self) -> std::path::PathBuf {
        self.inner.db_path()
    }
}

#[derive(Clone)]
pub struct UserDataWritePool {
    inner: DbPool,
}

impl UserDataWritePool {
    pub(crate) fn from_pool(inner: DbPool) -> Self {
        Self { inner }
    }

    pub async fn write<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&mut rusqlite::Connection) -> R + Send + 'static,
        R: Send + 'static,
    {
        self.inner.write(f).await
    }

    pub async fn try_write<F, R>(&self, f: F) -> Result<R, DbError>
    where
        F: FnOnce(&mut rusqlite::Connection) -> R + Send + 'static,
        R: Send + 'static,
    {
        self.inner.try_write(f).await
    }

    pub async fn transaction<F, R>(&self, f: F) -> Result<R, DbError>
    where
        F: FnOnce(&rusqlite::Transaction) -> rusqlite::Result<R> + Send + 'static,
        R: Send + 'static,
    {
        self.inner
            .try_write(move |conn| -> rusqlite::Result<R> {
                let tx = conn.transaction()?;
                let value = f(&tx)?;
                tx.commit()?;
                Ok(value)
            })
            .await?
            .map_err(DbError::Sql)
    }

    pub async fn reopen(&self, db_path: &std::path::Path) -> bool {
        self.inner.reopen(db_path).await
    }

    pub async fn reset_to_empty(&self) -> bool {
        self.inner.reset_to_empty().await
    }

    pub async fn replace_with_file(&self, src: &std::path::Path) -> bool {
        self.inner.replace_with_file(src).await
    }

    pub fn mark_corrupt(&self) {
        self.inner.mark_corrupt()
    }

    pub fn is_corrupt(&self) -> bool {
        self.inner.is_corrupt()
    }

    pub fn db_path(&self) -> std::path::PathBuf {
        self.inner.db_path()
    }

    pub fn backup_path_exists(&self) -> bool {
        self.inner.backup_path_exists()
    }
}
