//! Shared SQLite helpers for database connections.
//!
//! Open strategy is auto-detected from the filesystem (via `/proc/mounts`):
//! - **WAL-capable** (ext4, btrfs, xfs, etc.): WAL + `synchronous=NORMAL`
//! - **Non-WAL** (NFS, exFAT, FAT32, etc.): `nolock=1` + DELETE journal
//!
//! The DB layer has zero knowledge of storage kind — no caller-supplied hints
//! are needed. After opening, table probes detect corruption and auto-recreate
//! if needed.

use std::path::Path;

use rusqlite::{Connection, OpenFlags};

use replay_control_core::error::{Error, Result};

/// Actual journal mode of an open SQLite database.
///
/// Determined once at pool creation by querying `PRAGMA journal_mode` on a
/// warmed connection. Used for WAL-specific PRAGMAs (autocheckpoint) and
/// pool sizing (WAL allows concurrent readers; DELETE does not).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum JournalMode {
    Wal,
    Delete,
}

/// Open a SQLite connection with strategy appropriate for the filesystem.
///
/// Filesystem detection is automatic via `/proc/mounts` (see `supports_wal()`):
/// - **WAL-capable** (ext4, btrfs, xfs, etc.): WAL + `synchronous=NORMAL`.
///   Falls back to nolock if WAL open fails.
/// - **Non-WAL** (NFS, exFAT, FAT32, etc.): `nolock=1` + DELETE journal.
///   Falls back to WAL if nolock open fails.
///
/// No caller-supplied hints are needed — the DB layer has zero knowledge of
/// storage kind.
pub fn open_connection(db_path: &Path, label: &str) -> Result<Connection> {
    recover_stale_wal(db_path);

    let db_dir = db_path.parent().unwrap_or(db_path);
    if crate::storage::supports_wal(db_dir) {
        match open_wal(db_path, label) {
            Ok(conn) => Ok(conn),
            Err(e) => {
                tracing::info!("{label}: WAL open failed ({e}), falling back to nolock mode");
                open_nolock(db_path, label)
            }
        }
    } else {
        tracing::info!("{label}: filesystem does not support WAL, using DELETE journal");
        match open_nolock(db_path, label) {
            Ok(conn) => Ok(conn),
            Err(_) => {
                tracing::info!("{label}: nolock open failed, trying WAL mode");
                open_wal(db_path, label)
            }
        }
    }
}

/// Probe tables for corruption. Returns `Err` if any table is unreadable.
///
/// `PRAGMA quick_check` misses corrupt data pages, so we touch every table
/// with `SELECT COUNT(*)` instead.
pub fn probe_tables(conn: &Connection, tables: &[&str]) -> std::result::Result<(), String> {
    for table in tables {
        if let Err(e) = conn.execute_batch(&format!("SELECT COUNT(*) FROM {table};")) {
            return Err(format!("table `{table}`: {e}"));
        }
    }
    Ok(())
}

/// Delete a DB and its WAL/SHM sidecar files.
pub fn delete_db_files(db_path: &Path) {
    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
}

/// If WAL or SHM files exist alongside a SQLite database, open with normal
/// locking to let SQLite run WAL recovery (checkpoint), then close.
///
/// Without this, `nolock` mode skips WAL recovery and the leftover WAL/SHM
/// files cause "database disk image is malformed" errors.
pub fn recover_stale_wal(db_path: &Path) {
    let wal_path = db_path.with_extension("db-wal");
    let shm_path = db_path.with_extension("db-shm");

    if !wal_path.exists() && !shm_path.exists() {
        return;
    }

    if !db_path.exists() {
        // No main DB file — just remove the orphaned WAL/SHM.
        let _ = std::fs::remove_file(&wal_path);
        let _ = std::fs::remove_file(&shm_path);
        tracing::info!("Removed orphaned WAL/SHM files (no main DB)");
        return;
    }

    tracing::info!(
        "Stale WAL/SHM files detected, running WAL recovery for {}",
        db_path.display()
    );

    // Open with normal locking so SQLite performs WAL recovery automatically.
    match Connection::open(db_path) {
        Ok(conn) => {
            let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
            let _ = conn.execute_batch("PRAGMA journal_mode = DELETE;");
            drop(conn);
            tracing::info!("WAL recovery complete");

            // Clean up if the pragma didn't remove them.
            if wal_path.exists() {
                let _ = std::fs::remove_file(&wal_path);
            }
            if shm_path.exists() {
                let _ = std::fs::remove_file(&shm_path);
            }
        }
        Err(e) => {
            tracing::warn!("WAL recovery open failed ({e}), removing stale WAL/SHM");
            let _ = std::fs::remove_file(&wal_path);
            let _ = std::fs::remove_file(&shm_path);
        }
    }
}

fn open_nolock(db_path: &Path, label: &str) -> Result<Connection> {
    let uri = format!("file:{}?nolock=1", db_path.display());
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_CREATE
        | OpenFlags::SQLITE_OPEN_URI;
    let conn = Connection::open_with_flags(uri, flags)
        .map_err(|e| Error::Other(format!("{label}: failed to open (nolock): {e}")))?;
    conn.execute_batch(
        "PRAGMA journal_mode = DELETE;
         PRAGMA synchronous = NORMAL;
         PRAGMA cache_size = -8000;
         PRAGMA busy_timeout = 5000;
         -- Enforce referential integrity (no-op today, ready for future use)
         PRAGMA foreign_keys = ON;",
        // mmap_size intentionally left at default (0) — not safe on NFS.
    )
    .map_err(|e| Error::Other(format!("{label}: failed to set pragmas: {e}")))?;
    Ok(conn)
}

fn open_wal(db_path: &Path, label: &str) -> Result<Connection> {
    let conn = Connection::open(db_path)
        .map_err(|e| Error::Other(format!("{label}: failed to open: {e}")))?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA cache_size = -8000;
         PRAGMA busy_timeout = 5000;
         -- Cap WAL file at 64 MB after checkpoint (prevents unbounded growth on USB)
         PRAGMA journal_size_limit = 67108864;
         -- Enforce referential integrity (no-op today, ready for future use)
         PRAGMA foreign_keys = ON;",
        // mmap_size intentionally omitted — causes stale reads when the same
        // process does heavy writes through a separate connection (e.g.,
        // thumbnail index rebuild writes 46K rows, read connections see
        // corrupted mmap'd pages and return SQLITE_IOERR).
    )
    .map_err(|e| Error::Other(format!("{label}: failed to set pragmas: {e}")))?;
    Ok(conn)
}
