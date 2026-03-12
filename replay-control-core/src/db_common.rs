//! Shared SQLite helpers for nolock-based databases.
//!
//! Both `MetadataDb` and `UserDataDb` use the same open strategy:
//! 1. Recover stale WAL/SHM files (left by crash or WAL-mode fallback)
//! 2. Open with `nolock=1` + DELETE journal (fast, NFS-safe)
//! 3. Fall back to standard WAL mode if nolock fails
//! 4. Run table probes to detect corruption, auto-recreate if needed

use std::path::Path;

use rusqlite::{Connection, OpenFlags};

use crate::error::{Error, Result};

/// Open a SQLite connection using the nolock→WAL fallback strategy.
///
/// - Recovers stale WAL/SHM files first
/// - Tries `nolock=1` + DELETE journal mode (fast, NFS-safe)
/// - Falls back to standard WAL mode if nolock fails
pub fn open_connection(db_path: &Path, label: &str) -> Result<Connection> {
    recover_stale_wal(db_path);

    match open_nolock(db_path, label) {
        Ok(conn) => Ok(conn),
        Err(_) => {
            tracing::info!("{label}: nolock open failed, trying standard WAL mode");
            open_wal(db_path, label)
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
         PRAGMA cache_size = -8000;",
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
         PRAGMA cache_size = -8000;",
    )
    .map_err(|e| Error::Other(format!("{label}: failed to set pragmas: {e}")))?;
    Ok(conn)
}
