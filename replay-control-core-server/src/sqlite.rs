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
#[repr(u8)]
pub enum JournalMode {
    Wal = 0,
    Delete = 1,
}

impl JournalMode {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => JournalMode::Wal,
            _ => JournalMode::Delete,
        }
    }
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

/// Open a SQLite connection with strategy appropriate for the filesystem.
///
/// Filesystem detection is automatic via `/proc/mounts` (see `supports_wal()`):
/// - **WAL-capable** (ext4, btrfs, xfs, etc.): WAL + `synchronous=NORMAL`.
///   Falls back to nolock if WAL open fails.
/// - **Non-WAL** (NFS, exFAT, FAT32, etc.): `nolock=1` + DELETE journal.
///   Falls back to WAL if nolock open fails.
///
/// **Does not run WAL recovery.** Recovery from a previous process's
/// unclean shutdown is `recover_after_unclean_shutdown` and must be called
/// *before any connection exists* for this path in this process — the pool
/// owns that ordering. Calling it from per-connection open used to unlink
/// `-wal`/`-shm` while sibling connections held them, returning empty
/// reads to live callers; see
/// `investigations/2026-05-01-library-wal-unlink-under-live-connections.md`.
pub fn open_connection(db_path: &Path, label: &str) -> Result<Connection> {
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
        // debug! not info! — fires per connection on every open against an
        // exFAT / NFS DB. The "this pool is in DELETE mode" fact only needs
        // to surface once, and the higher-level "User data DB ready at …"
        // INFO already tells the operator the DB is up.
        tracing::debug!("{label}: filesystem does not support WAL, using DELETE journal");
        match open_nolock(db_path, label) {
            Ok(conn) => Ok(conn),
            Err(_) => {
                tracing::info!("{label}: nolock open failed, trying WAL mode");
                open_wal(db_path, label)
            }
        }
    }
}

/// Cheap sanity check for the SQLite magic header.
///
/// Returns `true` when the file exists with content that SQLite would refuse
/// to recognize as a database — i.e. either:
/// - it's at least 16 bytes long and the first 16 don't match `b"SQLite format 3\0"`, or
/// - it's between 1 and 15 bytes long (too short to even hold a header).
///
/// Returns `false` when the file is missing or zero-bytes — SQLite treats
/// both as a fresh DB and `open_connection` handles them correctly.
///
/// Used at startup to detect a `user_data.db` whose header has been clobbered
/// (torn write on power loss, manual corruption testing, etc.) so the host
/// crate can degrade gracefully instead of letting `open_connection` crash
/// the service in a restart loop.
pub fn has_invalid_sqlite_header(path: &Path) -> bool {
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    let mut buf = [0u8; 16];
    match f.read_exact(&mut buf) {
        Ok(()) => &buf != b"SQLite format 3\0",
        // Short read: 0 bytes means a fresh-DB placeholder SQLite handles
        // fine; 1-15 bytes is a torn write and SQLite will reject it.
        // Use the same handle to disambiguate (no path-level TOCTOU).
        Err(_) => f.metadata().map(|m| m.len() > 0).unwrap_or(false),
    }
}

/// Returns `true` if the table's column set differs from `expected`
/// (missing column, extra column, or count mismatch). Returns `false`
/// when the table doesn't exist or `PRAGMA table_info` fails — caller
/// decides what those mean in context.
///
/// Pure detection: no logging. Library callers (drop-and-rebuild) and
/// catalog callers (refuse-to-query) wrap this with their own
/// response. Reads are sub-millisecond on schemas of any practical size.
pub fn table_columns_diverge(conn: &Connection, table: &str, expected: &[&str]) -> bool {
    let actual: std::collections::HashSet<String> =
        match conn.prepare(&format!("PRAGMA table_info({table})")) {
            Ok(mut stmt) => match stmt
                .query_map([], |row| row.get::<_, String>(1))
                .and_then(|rows| rows.collect::<std::result::Result<_, _>>())
            {
                Ok(cols) => cols,
                Err(_) => return false,
            },
            Err(_) => return false,
        };
    if actual.is_empty() {
        return false;
    }
    if actual.len() != expected.len() {
        return true;
    }
    expected.iter().any(|col| !actual.contains(*col))
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

/// Delete a DB and its WAL/SHM/journal sidecar files.
///
/// **Destructive — must only be called when no connection to `db_path` is
/// open in this process.** `DbPool::reset_to_empty` and
/// `DbPool::replace_with_file` are the supported entry points; they drain
/// the pool first. The free function is `pub(crate)` to keep new callers
/// from re-introducing the unlink-under-live-fds bug.
///
/// Covers both journal modes: `.db-wal` / `.db-shm` for WAL (ext4/btrfs) and
/// `.db-journal` for DELETE rollback mode (exFAT/NFS). Any of them can linger
/// depending on which journal mode was active when the DB was last open.
pub(crate) fn delete_db_files(db_path: &Path) {
    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-journal"));
}

/// One-shot WAL recovery from a *previous process's* unclean shutdown.
///
/// **Must be called before any connection to `db_path` exists in this
/// process.** Owned by `DbPool::new` / `DbPool::reopen`; do not call from
/// `open_connection`, from a per-connection deadpool `Manager::create`, or
/// from any code path that runs while sibling connections may be live —
/// running it then unlinks `-wal`/`-shm` while live fds reference them and
/// silently strands the writes those connections committed but didn't
/// checkpoint.
///
/// Steady-state SQLite handles its own WAL recovery on first open in WAL
/// mode. This helper is only needed for the cross-mode case (a DB whose
/// previous process opened in WAL but whose current process opens with
/// `nolock=1`, e.g. an exFAT/NFS migration), where `nolock` skips WAL
/// recovery and the leftover sidecars cause
/// "database disk image is malformed" errors.
pub fn recover_after_unclean_shutdown(db_path: &Path) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn tmp_path(tag: &str) -> std::path::PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        std::env::temp_dir().join(format!(
            "rc-sqlite-header-{}-{}-{}",
            tag,
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed),
        ))
    }

    #[test]
    fn header_check_missing_file_is_not_invalid() {
        let p = tmp_path("missing");
        assert!(!has_invalid_sqlite_header(&p));
    }

    #[test]
    fn header_check_zero_byte_file_is_not_invalid() {
        let p = tmp_path("empty");
        std::fs::write(&p, b"").unwrap();
        assert!(!has_invalid_sqlite_header(&p));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn header_check_partial_header_is_invalid() {
        // 5-byte file: too short to hold the 16-byte magic. Pre-fix this slipped
        // through and crash-looped the service. Lock in the catch.
        let p = tmp_path("partial");
        std::fs::write(&p, b"SQLit").unwrap();
        assert!(has_invalid_sqlite_header(&p));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn header_check_clobbered_header_is_invalid() {
        let p = tmp_path("clobbered");
        std::fs::write(&p, [0xDEu8; 4096]).unwrap();
        assert!(has_invalid_sqlite_header(&p));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn header_check_valid_header_is_not_invalid() {
        let p = tmp_path("valid");
        let mut bytes = b"SQLite format 3\0".to_vec();
        bytes.extend(std::iter::repeat_n(0u8, 4080));
        std::fs::write(&p, &bytes).unwrap();
        assert!(!has_invalid_sqlite_header(&p));
        let _ = std::fs::remove_file(&p);
    }

    fn open_in_memory_with(create_sql: &str) -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(create_sql).unwrap();
        conn
    }

    #[test]
    fn columns_match_returns_false_for_exact_schema() {
        let conn = open_in_memory_with("CREATE TABLE t (a INTEGER, b TEXT, c BLOB);");
        assert!(!table_columns_diverge(&conn, "t", &["a", "b", "c"]));
    }

    #[test]
    fn columns_match_ignores_declaration_order() {
        let conn = open_in_memory_with("CREATE TABLE t (a INTEGER, b TEXT, c BLOB);");
        // Column order in the expected list is irrelevant — set comparison.
        assert!(!table_columns_diverge(&conn, "t", &["c", "a", "b"]));
    }

    #[test]
    fn columns_diverge_when_expected_column_is_missing() {
        // The beta.5 arcade trap, in miniature: runtime expects `source` but
        // the on-disk schema (the user's stale catalog) doesn't have it.
        let conn =
            open_in_memory_with("CREATE TABLE arcade_games (rom_name TEXT, display_name TEXT);");
        assert!(table_columns_diverge(
            &conn,
            "arcade_games",
            &["rom_name", "source", "display_name"],
        ));
    }

    #[test]
    fn columns_diverge_when_table_has_extra_column() {
        let conn = open_in_memory_with("CREATE TABLE t (a INTEGER, b TEXT, c BLOB);");
        assert!(table_columns_diverge(&conn, "t", &["a", "b"]));
    }

    #[test]
    fn columns_match_returns_false_for_missing_table() {
        // A missing table means PRAGMA returns no rows. Caller decides what
        // that means in context (existence is a separate check upstream); the
        // helper itself reports "no divergence" rather than guessing.
        let conn = Connection::open_in_memory().unwrap();
        assert!(!table_columns_diverge(&conn, "nope", &["a"]));
    }
}
