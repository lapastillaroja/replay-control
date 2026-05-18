//! Per-storage k/v table for library-scoped metadata that doesn't deserve
//! its own column.
//!
//! First inhabitant: `title_norm_version`. Stamped after a scan or reconcile
//! has populated `game_library.normalized_title{,_alt}` against the current
//! `replay_control_core::title_utils::TITLE_NORM_VERSION`. Mismatch on boot
//! triggers per-system rebuild of the normalized columns so the matcher
//! benefits from normalizer improvements without user action.
//!
//! Sibling of `external_metadata::external_meta` (host-global k/v) — same
//! shape, different scope. The library-side stamp is per-storage because a
//! user can have multiple `library.db` files (NFS, USB, SD); each must be
//! independently reconciled when it becomes active.
//!
//! See `plans/2026-05-06-metadata-enrichment-fixes-plan.md` Phase 1.5.

use rusqlite::{Connection, OptionalExtension, params};

use replay_control_core::error::{Error, Result};

/// Well-known keys for `library_meta`.
pub mod keys {
    /// `replay_control_core::title_utils::TITLE_NORM_VERSION` at the time
    /// `game_library.normalized_title{,_alt}` was last (re)populated.
    pub const TITLE_NORM_VERSION: &str = "title_norm_version";

    /// `catalog.sqlite.db_meta.catalog_resource_version` last copied into
    /// `library_game_resource` by enrichment for this storage.
    pub const CATALOG_RESOURCE_VERSION: &str = "catalog_resource_version";

    /// Per-system recursive ROM scan fingerprint used by startup verification
    /// to skip unchanged systems without trusting directory mtimes.
    pub fn discovery_fingerprint(system: &str) -> String {
        format!("discovery_fingerprint:{system}")
    }

    /// Storage-scoped mtime probe verdict used by startup verification.
    pub const MTIME_PROBE_TRUSTWORTHY: &str = "mtime_probe_trustworthy";

    /// Storage signature the mtime probe verdict applies to.
    pub const MTIME_PROBE_SIGNATURE: &str = "mtime_probe_signature";

    /// Informational filesystem type observed during the mtime probe.
    pub const MTIME_PROBE_FSTYPE: &str = "mtime_probe_fstype";

    /// Probe implementation version. Bump when the probe policy changes.
    pub const MTIME_PROBE_VERSION: &str = "mtime_probe_version";
}

/// Read a value from `library_meta`. Returns `None` for missing keys.
pub fn read_meta(conn: &Connection, key: &str) -> Option<String> {
    read_meta_result(conn, key).ok().flatten()
}

/// Read a value from `library_meta`, preserving SQL errors.
pub fn read_meta_result(conn: &Connection, key: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT value FROM library_meta WHERE key = ?1",
        params![key],
        |row| row.get::<_, Option<String>>(0),
    )
    .optional()
    .map(|value| value.flatten())
    .map_err(|e| Error::Other(format!("read library_meta {key}: {e}")))
}

/// Write (or clear) a key in `library_meta`. Pass `value = None` to set
/// the value column to NULL while keeping the key row.
pub fn write_meta(conn: &Connection, key: &str, value: Option<&str>) -> Result<()> {
    conn.execute(
        "INSERT INTO library_meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )
    .map_err(|e| Error::Other(format!("write library_meta {key}: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library_db::LibraryDb;

    fn open_temp() -> (Connection, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let conn = LibraryDb::open(dir.path()).unwrap();
        (conn, dir)
    }

    #[test]
    fn round_trip() {
        let (conn, _tmp) = open_temp();
        assert_eq!(read_meta(&conn, keys::TITLE_NORM_VERSION), None);

        write_meta(&conn, keys::TITLE_NORM_VERSION, Some("1")).unwrap();
        assert_eq!(
            read_meta(&conn, keys::TITLE_NORM_VERSION),
            Some("1".to_string())
        );

        write_meta(&conn, keys::TITLE_NORM_VERSION, Some("2")).unwrap();
        assert_eq!(
            read_meta(&conn, keys::TITLE_NORM_VERSION),
            Some("2".to_string())
        );
    }

    #[test]
    fn null_value_is_none() {
        let (conn, _tmp) = open_temp();
        write_meta(&conn, keys::TITLE_NORM_VERSION, None).unwrap();
        assert_eq!(read_meta(&conn, keys::TITLE_NORM_VERSION), None);
    }
}
