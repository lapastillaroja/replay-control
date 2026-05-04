//! Host-global SQLite for external metadata (LaunchBox text + libretro thumbnail manifests).
//!
//! Lives at `<data_dir>/external_metadata.db` (default `/var/lib/replay-control/`).
//! Read-only at request time — only background enrichment ever queries it.
//! Writes happen via in-place transactions on the `DbPool::write` connection;
//! the host pool is constructed in the app layer (`AppState::external_metadata_pool`)
//! and passed down via `&Connection` like every other pool in the codebase.

use std::path::Path;

use replay_control_core::error::{Error, Result};

/// Filename for the host-global external-metadata SQLite.
pub const EXTERNAL_METADATA_DB_FILE: &str = "external_metadata.db";

// ── Schema ────────────────────────────────────────────────────────────────

const CREATE_LAUNCHBOX_GAME_SQL: &str = "
    CREATE TABLE IF NOT EXISTS launchbox_game (
        system TEXT NOT NULL,
        normalized_title TEXT NOT NULL,
        description TEXT,
        genre TEXT,
        developer TEXT,
        publisher TEXT,
        release_date TEXT,
        release_precision TEXT,
        rating REAL,
        rating_count INTEGER,
        cooperative INTEGER NOT NULL DEFAULT 0,
        players INTEGER,
        PRIMARY KEY (system, normalized_title)
    );
";

const LAUNCHBOX_GAME_COLUMNS: &[&str] = &[
    "system",
    "normalized_title",
    "description",
    "genre",
    "developer",
    "publisher",
    "release_date",
    "release_precision",
    "rating",
    "rating_count",
    "cooperative",
    "players",
];

const CREATE_LAUNCHBOX_ALTERNATE_SQL: &str = "
    CREATE TABLE IF NOT EXISTS launchbox_alternate (
        system TEXT NOT NULL,
        normalized_title TEXT NOT NULL,
        alternate_name TEXT NOT NULL,
        PRIMARY KEY (system, normalized_title, alternate_name)
    );
";

const LAUNCHBOX_ALTERNATE_COLUMNS: &[&str] = &["system", "normalized_title", "alternate_name"];

const CREATE_THUMBNAIL_MANIFEST_SQL: &str = "
    CREATE TABLE IF NOT EXISTS thumbnail_manifest (
        repo_name TEXT NOT NULL,
        kind TEXT NOT NULL,
        filename TEXT NOT NULL,
        symlink_target TEXT,
        PRIMARY KEY (repo_name, kind, filename)
    );
";

const THUMBNAIL_MANIFEST_COLUMNS: &[&str] = &["repo_name", "kind", "filename", "symlink_target"];

const CREATE_DATA_SOURCE_SQL: &str = "
    CREATE TABLE IF NOT EXISTS data_source (
        source_name TEXT PRIMARY KEY,
        source_type TEXT NOT NULL,
        version_hash TEXT,
        imported_at INTEGER NOT NULL,
        entry_count INTEGER NOT NULL DEFAULT 0,
        branch TEXT
    );
    CREATE INDEX IF NOT EXISTS idx_data_source_type ON data_source (source_type);
";

const DATA_SOURCE_COLUMNS: &[&str] = &[
    "source_name",
    "source_type",
    "version_hash",
    "imported_at",
    "entry_count",
    "branch",
];

const CREATE_EXTERNAL_META_SQL: &str = "
    CREATE TABLE IF NOT EXISTS external_meta (
        key TEXT PRIMARY KEY,
        value TEXT
    );
";

const EXTERNAL_META_COLUMNS: &[&str] = &["key", "value"];

/// Tables probed for corruption detection on open.
const TABLES: &[&str] = &[
    "launchbox_game",
    "launchbox_alternate",
    "thumbnail_manifest",
    "data_source",
    "external_meta",
];

/// Well-known keys for `external_meta`.
pub mod meta_keys {
    /// CRC32 of the LaunchBox XML file last parsed into `launchbox_game`.
    /// Comparison is always current-vs-stored (n=2), so CRC32's collision
    /// profile is plenty — no need for a cryptographic hash.
    pub const LAUNCHBOX_XML_CRC32: &str = "launchbox_xml_crc32";

    /// ETag of the upstream LaunchBox `Metadata.zip` at the time of the last
    /// successful download. Compared against the server's current ETag via a
    /// HEAD request before starting a new download — if they match the file
    /// hasn't changed and the download is skipped.
    pub const LAUNCHBOX_UPSTREAM_ETAG: &str = "launchbox_upstream_etag";

    /// Unix seconds at which the libretro thumbnail manifest was last
    /// successfully fetched from GitHub. Used as a short TTL to skip
    /// per-repo SHA checks when the user clicks "Update Thumbnails" twice
    /// within a few minutes.
    pub const THUMBNAIL_MANIFEST_FETCHED_AT: &str = "thumbnail_manifest_fetched_at";
}

/// Create or rebuild the schema. Drops divergent tables before recreating
/// so a column-set drift is recovered transparently — same pattern as
/// `LibraryDb::init_tables`.
pub fn init_tables(conn: &rusqlite::Connection) -> Result<()> {
    let pairs: &[(&str, &[&str], &str)] = &[
        (
            "launchbox_game",
            LAUNCHBOX_GAME_COLUMNS,
            CREATE_LAUNCHBOX_GAME_SQL,
        ),
        (
            "launchbox_alternate",
            LAUNCHBOX_ALTERNATE_COLUMNS,
            CREATE_LAUNCHBOX_ALTERNATE_SQL,
        ),
        (
            "thumbnail_manifest",
            THUMBNAIL_MANIFEST_COLUMNS,
            CREATE_THUMBNAIL_MANIFEST_SQL,
        ),
        ("data_source", DATA_SOURCE_COLUMNS, CREATE_DATA_SOURCE_SQL),
        (
            "external_meta",
            EXTERNAL_META_COLUMNS,
            CREATE_EXTERNAL_META_SQL,
        ),
    ];
    for (name, cols, ddl) in pairs {
        if crate::sqlite::table_columns_diverge(conn, name, cols) {
            tracing::warn!("external_metadata: {name} schema differs, rebuilding");
            let _ = conn.execute_batch(&format!("DROP TABLE IF EXISTS {name};"));
        }
        conn.execute_batch(ddl)
            .map_err(|e| Error::Other(format!("create {name}: {e}")))?;
    }
    Ok(())
}

/// Open (creating if missing) the external-metadata DB at the given file path.
/// Self-heals header-clobbered files (delete + recreate) and table-level
/// corruption (probe-on-open). Schema drift is rebuilt by `init_tables`.
pub fn open_at(db_path: &Path) -> Result<rusqlite::Connection> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    }
    if crate::sqlite::has_invalid_sqlite_header(db_path) {
        tracing::warn!(
            "external_metadata DB at {} has invalid SQLite header — recreating",
            db_path.display()
        );
        crate::sqlite::delete_db_files(db_path);
    }
    let conn = crate::sqlite::open_connection(db_path, EXTERNAL_METADATA_DB_FILE)?;
    init_tables(&conn)?;
    if let Err(detail) = crate::sqlite::probe_tables(&conn, TABLES) {
        tracing::warn!("external_metadata DB corrupt ({detail}), recreating");
        drop(conn);
        crate::sqlite::delete_db_files(db_path);
        let conn = crate::sqlite::open_connection(db_path, EXTERNAL_METADATA_DB_FILE)?;
        init_tables(&conn)?;
        return Ok(conn);
    }
    Ok(conn)
}

/// Stream-hash a file with CRC32 and return the hex digest.
/// Used for content-derived freshness on the LaunchBox XML — mtime is
/// unreliable across copies / rsync / clock skew. CRC32 is enough because
/// the comparison is always current-vs-stored (n=2), well below the
/// birthday-paradox bound for a 32-bit hash.
pub fn hash_file_crc32(path: &Path) -> Result<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|e| Error::io(path, e))?;
    let mut hasher = crc32fast::Hasher::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|e| Error::io(path, e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:08x}", hasher.finalize()))
}

/// Read a key from `external_meta`. `None` if missing or DB unavailable.
pub fn read_meta(conn: &rusqlite::Connection, key: &str) -> Option<String> {
    use rusqlite::OptionalExtension;
    conn.query_row(
        "SELECT value FROM external_meta WHERE key = ?1",
        rusqlite::params![key],
        |row| row.get::<_, Option<String>>(0),
    )
    .optional()
    .ok()
    .flatten()
    .flatten()
}

// ── data_source / thumbnail_manifest types ────────────────────────────────

/// One libretro source-version row.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DataSourceInfo {
    pub source_name: String,
    pub source_type: String,
    pub version_hash: Option<String>,
    pub imported_at: i64,
    pub entry_count: usize,
    pub branch: Option<String>,
}

/// Aggregate stats for a `source_type` (e.g. `"libretro-thumbnails"`).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct DataSourceStats {
    pub repo_count: usize,
    pub total_entries: usize,
    pub oldest_imported_at: Option<i64>,
}

/// One entry from the `thumbnail_manifest` table.
#[derive(Debug, Clone)]
pub struct ThumbnailManifestEntry {
    pub filename: String,
    pub symlink_target: Option<String>,
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

// ── data_source operations ────────────────────────────────────────────────

/// Insert or update a `data_source` row. Caller manages the transaction
/// when bundling with thumbnail-manifest writes.
pub fn upsert_data_source(
    conn: &rusqlite::Connection,
    source_name: &str,
    source_type: &str,
    version_hash: &str,
    branch: &str,
    entry_count: usize,
) -> Result<()> {
    let now = unix_now();
    conn.execute(
        "INSERT INTO data_source (source_name, source_type, version_hash, imported_at, entry_count, branch)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(source_name) DO UPDATE SET
            version_hash = excluded.version_hash,
            imported_at = excluded.imported_at,
            entry_count = excluded.entry_count,
            branch = excluded.branch",
        rusqlite::params![source_name, source_type, version_hash, now, entry_count as i64, branch],
    )
    .map_err(|e| Error::Other(format!("upsert_data_source: {e}")))?;
    Ok(())
}

/// Look up a single `data_source` row.
pub fn get_data_source(
    conn: &rusqlite::Connection,
    source_name: &str,
) -> Result<Option<DataSourceInfo>> {
    use rusqlite::OptionalExtension;
    conn.query_row(
        "SELECT source_name, source_type, version_hash, imported_at, entry_count, branch
         FROM data_source WHERE source_name = ?1",
        rusqlite::params![source_name],
        |row| {
            Ok(DataSourceInfo {
                source_name: row.get(0)?,
                source_type: row.get(1)?,
                version_hash: row.get(2)?,
                imported_at: row.get(3)?,
                entry_count: row.get::<_, i64>(4)? as usize,
                branch: row.get(5)?,
            })
        },
    )
    .optional()
    .map_err(|e| Error::Other(format!("get_data_source: {e}")))
}

/// Aggregate stats over a single `source_type`.
pub fn get_data_source_stats(
    conn: &rusqlite::Connection,
    source_type: &str,
) -> Result<DataSourceStats> {
    conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(entry_count), 0), MIN(imported_at)
         FROM data_source WHERE source_type = ?1",
        rusqlite::params![source_type],
        |row| {
            Ok(DataSourceStats {
                repo_count: row.get::<_, i64>(0)? as usize,
                total_entries: row.get::<_, i64>(1)? as usize,
                oldest_imported_at: row.get(2)?,
            })
        },
    )
    .map_err(|e| Error::Other(format!("get_data_source_stats: {e:?}")))
}

// ── thumbnail_manifest operations ─────────────────────────────────────────

/// Total row count across all repos.
pub fn thumbnail_manifest_count(conn: &rusqlite::Connection) -> Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM thumbnail_manifest", [], |row| {
        row.get(0)
    })
    .map_err(|e| Error::Other(format!("thumbnail_manifest_count: {e}")))
}

/// Look up all manifest entries for a `(repo_name, kind)` pair.
pub fn query_thumbnail_manifest(
    conn: &rusqlite::Connection,
    repo_name: &str,
    kind: &str,
) -> Result<Vec<ThumbnailManifestEntry>> {
    let mut stmt = conn
        .prepare(
            "SELECT filename, symlink_target
             FROM thumbnail_manifest
             WHERE repo_name = ?1 AND kind = ?2",
        )
        .map_err(|e| Error::Other(format!("prepare query_thumbnail_manifest: {e}")))?;
    let rows = stmt
        .query_map(rusqlite::params![repo_name, kind], |row| {
            Ok(ThumbnailManifestEntry {
                filename: row.get(0)?,
                symlink_target: row.get(1)?,
            })
        })
        .map_err(|e| Error::Other(format!("query thumbnail_manifest: {e}")))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| Error::Other(format!("thumbnail_manifest row: {e}")))?);
    }
    Ok(out)
}

/// Delete every manifest row for `repo_name`. Returns the number of rows removed.
pub fn delete_thumbnail_manifest(conn: &rusqlite::Connection, repo_name: &str) -> Result<usize> {
    let count = conn
        .execute(
            "DELETE FROM thumbnail_manifest WHERE repo_name = ?1",
            rusqlite::params![repo_name],
        )
        .map_err(|e| Error::Other(format!("delete_thumbnail_manifest: {e}")))?;
    Ok(count)
}

/// Insert manifest entries for one repo. The caller is expected to have
/// already deleted the prior rows for `repo_name` (typically inside the
/// same transaction as the surrounding `upsert_data_source` call).
pub fn insert_thumbnail_manifest_rows(
    conn: &rusqlite::Connection,
    repo_name: &str,
    entries: &[(String, String, Option<String>)],
) -> Result<usize> {
    let mut stmt = conn
        .prepare(
            "INSERT OR REPLACE INTO thumbnail_manifest
             (repo_name, kind, filename, symlink_target)
             VALUES (?1, ?2, ?3, ?4)",
        )
        .map_err(|e| Error::Other(format!("prepare insert thumbnail_manifest: {e}")))?;
    let mut count = 0usize;
    for (kind, filename, symlink_target) in entries {
        stmt.execute(rusqlite::params![repo_name, kind, filename, symlink_target])
            .map_err(|e| Error::Other(format!("insert thumbnail_manifest: {e}")))?;
        count += 1;
    }
    Ok(count)
}

/// Drop every libretro-thumbnails row in both `thumbnail_manifest` and the
/// matching `data_source` rows. Used by user-triggered "clear thumbnails".
pub fn clear_libretro_thumbnail_manifest(conn: &rusqlite::Connection) -> Result<()> {
    conn.execute("DELETE FROM thumbnail_manifest", [])
        .map_err(|e| Error::Other(format!("clear thumbnail_manifest: {e}")))?;
    conn.execute(
        "DELETE FROM data_source WHERE source_type = 'libretro-thumbnails'",
        [],
    )
    .map_err(|e| Error::Other(format!("clear libretro data_source rows: {e}")))?;
    Ok(())
}

// ── Per-system batched readers ────────────────────────────────────────────

/// One LaunchBox metadata row, keyed by `(system, normalized_title)`.
/// Fields mirror the `launchbox_game` schema; `release_year` is derived in SQL.
#[derive(Debug, Clone, Default)]
pub struct LaunchboxRow {
    pub description: Option<String>,
    pub genre: Option<String>,
    pub developer: Option<String>,
    pub publisher: Option<String>,
    pub release_year: Option<u16>,
    pub rating: Option<f64>,
    pub rating_count: Option<u32>,
    pub cooperative: bool,
    pub players: Option<u8>,
}

/// Load every `launchbox_game` row for a system into a normalized-title-keyed map.
/// Single SELECT — replaces the legacy "one query per field" pattern.
pub fn system_launchbox_rows(
    conn: &rusqlite::Connection,
    system: &str,
) -> Result<std::collections::HashMap<String, LaunchboxRow>> {
    let mut stmt = conn
        .prepare(
            "SELECT normalized_title, description, genre, developer, publisher,
                    CAST(substr(release_date, 1, 4) AS INTEGER),
                    rating, rating_count, cooperative, players
             FROM launchbox_game
             WHERE system = ?1",
        )
        .map_err(|e| Error::Other(format!("prepare system_launchbox_rows: {e}")))?;

    let rows = stmt
        .query_map(rusqlite::params![system], |row| {
            let norm: String = row.get(0)?;
            let release_year = row.get::<_, Option<i64>>(5)?.and_then(|y| {
                if (1900..=9999).contains(&y) {
                    Some(y as u16)
                } else {
                    None
                }
            });
            let r = LaunchboxRow {
                description: row.get(1)?,
                genre: row.get(2)?,
                developer: row.get(3)?,
                publisher: row.get(4)?,
                release_year,
                rating: row.get(6)?,
                rating_count: row.get::<_, Option<i64>>(7)?.map(|c| c as u32),
                cooperative: row.get::<_, i64>(8).unwrap_or(0) != 0,
                players: row.get::<_, Option<i64>>(9)?.map(|p| p as u8),
            };
            Ok((norm, r))
        })
        .map_err(|e| Error::Other(format!("query system_launchbox_rows: {e}")))?;

    let mut map = std::collections::HashMap::new();
    for r in rows.flatten() {
        map.insert(r.0, r.1);
    }
    Ok(map)
}

/// Total `launchbox_game` row count — used by the setup checklist's
/// "metadata imported?" check and by metadata coverage stats.
pub fn launchbox_game_count(conn: &rusqlite::Connection) -> Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM launchbox_game", [], |row| row.get(0))
        .map_err(|e| Error::Other(format!("launchbox_game_count: {e}")))
}

/// Aggregate stats over `launchbox_game` for the metadata-coverage UI.
pub fn launchbox_stats(
    conn: &rusqlite::Connection,
    db_path: &Path,
) -> Result<replay_control_core::library_db::MetadataStats> {
    let total_entries: usize = conn
        .query_row("SELECT COUNT(*) FROM launchbox_game", [], |row| {
            row.get::<_, i64>(0).map(|v| v as usize)
        })
        .map_err(|e| Error::Other(format!("launchbox_stats total: {e}")))?;
    let with_description: usize = conn
        .query_row(
            "SELECT COUNT(*) FROM launchbox_game WHERE description IS NOT NULL AND description != ''",
            [],
            |row| row.get::<_, i64>(0).map(|v| v as usize),
        )
        .map_err(|e| Error::Other(format!("launchbox_stats with_description: {e}")))?;
    let with_rating: usize = conn
        .query_row(
            "SELECT COUNT(*) FROM launchbox_game WHERE rating IS NOT NULL",
            [],
            |row| row.get::<_, i64>(0).map(|v| v as usize),
        )
        .map_err(|e| Error::Other(format!("launchbox_stats with_rating: {e}")))?;
    let db_size_bytes = std::fs::metadata(db_path).map(|m| m.len()).unwrap_or(0);
    Ok(replay_control_core::library_db::MetadataStats {
        total_entries,
        with_description,
        with_rating,
        db_size_bytes,
        last_updated_text: String::new(),
    })
}

/// Per-system count of `launchbox_game` rows with a non-empty description —
/// the "metadata coverage" number on the per-system list. Coverage is by
/// `(system, normalized_title)`, which is necessarily ≤ the on-disk ROM count
/// since multiple ROM filenames can share one normalized title.
pub fn launchbox_entries_per_system(conn: &rusqlite::Connection) -> Result<Vec<(String, usize)>> {
    let mut stmt = conn
        .prepare(
            "SELECT system, COUNT(*) FROM launchbox_game
             WHERE description IS NOT NULL AND description != ''
             GROUP BY system ORDER BY 2 DESC",
        )
        .map_err(|e| Error::Other(format!("launchbox_entries_per_system prepare: {e}")))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1).map(|v| v as usize)?,
            ))
        })
        .map_err(|e| Error::Other(format!("launchbox_entries_per_system query: {e}")))?;
    let mut out = Vec::new();
    for r in rows.flatten() {
        out.push(r);
    }
    Ok(out)
}

/// Drop every `launchbox_game` + `launchbox_alternate` row. Used by the
/// "Clear metadata" UI button.
pub fn clear_launchbox(conn: &rusqlite::Connection) -> Result<()> {
    conn.execute("DELETE FROM launchbox_game", [])
        .map_err(|e| Error::Other(format!("clear launchbox_game: {e}")))?;
    conn.execute("DELETE FROM launchbox_alternate", [])
        .map_err(|e| Error::Other(format!("clear launchbox_alternate: {e}")))?;
    write_meta(conn, meta_keys::LAUNCHBOX_XML_CRC32, None)?;
    write_meta(conn, meta_keys::LAUNCHBOX_UPSTREAM_ETAG, None)?;
    Ok(())
}

/// Load every `launchbox_alternate` row for a system. Returns
/// `(normalized_title, alternate_name)` pairs for repopulating `game_alias`.
pub fn system_launchbox_alternates(
    conn: &rusqlite::Connection,
    system: &str,
) -> Result<Vec<(String, String)>> {
    let mut stmt = conn
        .prepare(
            "SELECT normalized_title, alternate_name
             FROM launchbox_alternate
             WHERE system = ?1",
        )
        .map_err(|e| Error::Other(format!("prepare system_launchbox_alternates: {e}")))?;

    let rows = stmt
        .query_map(rusqlite::params![system], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| Error::Other(format!("query system_launchbox_alternates: {e}")))?;

    let mut out = Vec::new();
    for r in rows.flatten() {
        out.push(r);
    }
    Ok(out)
}

/// Write a key/value pair to `external_meta`. Used during refresh.
pub fn write_meta(conn: &rusqlite::Connection, key: &str, value: Option<&str>) -> Result<()> {
    conn.execute(
        "INSERT INTO external_meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, value],
    )
    .map_err(|e| Error::Other(format!("write external_meta {key}: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_temp() -> (rusqlite::Connection, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let conn = open_at(&tmp.path().join(EXTERNAL_METADATA_DB_FILE)).unwrap();
        (conn, tmp)
    }

    #[test]
    fn open_creates_all_tables() {
        let (conn, _dir) = open_temp();
        for table in TABLES {
            let cnt: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    [table],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(cnt, 1, "{table} should exist");
        }
    }

    #[test]
    fn divergent_table_is_rebuilt() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(EXTERNAL_METADATA_DB_FILE);
        // Pre-create an old-shape launchbox_game with a row.
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE launchbox_game (
                    system TEXT NOT NULL,
                    rom_filename TEXT NOT NULL,
                    PRIMARY KEY (system, rom_filename)
                );
                INSERT INTO launchbox_game VALUES ('snes', 'mario.sfc');",
            )
            .unwrap();
        }
        let conn = open_at(&path).unwrap();
        // Old row gone; expected columns present.
        let cnt: i64 = conn
            .query_row("SELECT COUNT(*) FROM launchbox_game", [], |r| r.get(0))
            .unwrap();
        assert_eq!(cnt, 0);
        let has_normalized: bool = conn
            .prepare("PRAGMA table_info(launchbox_game)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .flatten()
            .any(|c| c == "normalized_title");
        assert!(has_normalized);
    }

    #[test]
    fn header_clobbered_recovers() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(EXTERNAL_METADATA_DB_FILE);
        std::fs::write(&path, [0xDEu8; 4096]).unwrap();
        let conn = open_at(&path).unwrap();
        // Tables exist after recreate.
        let cnt: i64 = conn
            .query_row("SELECT COUNT(*) FROM launchbox_game", [], |r| r.get(0))
            .unwrap();
        assert_eq!(cnt, 0);
    }

    #[test]
    fn hash_file_crc32_matches_known_value() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("file.txt");
        std::fs::write(&path, b"hello world").unwrap();
        let h = hash_file_crc32(&path).unwrap();
        // Known CRC32 of "hello world".
        assert_eq!(h, "0d4a1185");
    }

    #[test]
    fn meta_read_write_roundtrip() {
        let (conn, _dir) = open_temp();
        assert_eq!(read_meta(&conn, meta_keys::LAUNCHBOX_XML_CRC32), None);
        write_meta(&conn, meta_keys::LAUNCHBOX_XML_CRC32, Some("0d4a1185")).unwrap();
        assert_eq!(
            read_meta(&conn, meta_keys::LAUNCHBOX_XML_CRC32),
            Some("0d4a1185".into())
        );
        write_meta(&conn, meta_keys::LAUNCHBOX_XML_CRC32, Some("deadbeef")).unwrap();
        assert_eq!(
            read_meta(&conn, meta_keys::LAUNCHBOX_XML_CRC32),
            Some("deadbeef".into())
        );
    }
}
