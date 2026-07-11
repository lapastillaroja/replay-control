//! User data database — persistent user customizations that survive metadata clears.
//!
//! Stored at `<rom_storage>/.replay-control/user_data.db`.
//! Separate from `library.db` (which is a rebuildable library index) to ensure user
//! choices are never accidentally destroyed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};

use crate::storage::RC_DIR;
use replay_control_core::error::{Error, Result};

/// Filename for the SQLite user data database.
pub const USER_DATA_DB_FILE: &str = "user_data.db";

pub use replay_control_core::user_data_db::{ResourceEntry, VideoEntry};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManualOrigin {
    Downloaded,
    Upload,
}

impl ManualOrigin {
    fn from_db(value: String) -> Self {
        match value.trim() {
            "upload" => Self::Upload,
            _ => Self::Downloaded,
        }
    }

    fn to_db_value(&self) -> &str {
        match self {
            Self::Downloaded => "downloaded",
            Self::Upload => "upload",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ManualEntry {
    pub manual_id: String,
    pub resource_key: String,
    pub title: Option<String>,
    pub origin: ManualOrigin,
    pub provider: Option<String>,
    pub url: Option<String>,
    pub storage_path: Option<String>,
    pub original_filename: Option<String>,
    pub languages: String,
    pub mime_type: String,
    pub size_bytes: Option<u64>,
    pub added_at: u64,
}

/// Stateless query namespace for the user data SQLite database.
///
/// All methods are associated functions that take `conn: &Connection` as their
/// first parameter. No connection ownership — the pool manages lifecycle.
pub struct UserDataDb;

impl UserDataDb {
    /// Tables to probe for corruption detection.
    /// NOTE: update this list when adding new tables.
    pub const TABLES: &[&str] = &[
        "box_art_overrides",
        "game_videos",
        "game_manual_resource",
        "game_resource_link",
    ];

    /// Resolve the user_data.db path under `<storage_root>/.replay-control/`
    /// without touching the filesystem. Useful for callers that need the path
    /// to pre-flight-check it (e.g. `has_invalid_sqlite_header`) before
    /// invoking `open`, which would crash on a clobbered header.
    pub fn db_path(storage_root: &Path) -> PathBuf {
        storage_root.join(RC_DIR).join(USER_DATA_DB_FILE)
    }

    /// Open (or create) the user data DB at `db_path`. Returns the
    /// connection plus a corruption flag — the connection is always
    /// usable on `Ok`; `is_corrupt = true` means a follow-up
    /// `probe_tables` failed and the caller should surface the recovery
    /// banner. User data is never silently destroyed.
    pub fn open_at(db_path: &Path) -> Result<(Connection, bool)> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }
        let conn = crate::sqlite::open_connection(db_path, "user_data.db")?;
        Self::init_tables(&conn)?;
        let is_corrupt = match crate::sqlite::probe_tables(&conn, Self::TABLES) {
            Ok(()) => false,
            Err(detail) => {
                tracing::warn!("User data DB corrupt ({detail})");
                true
            }
        };
        Ok((conn, is_corrupt))
    }

    /// Create all tables if they don't exist.
    pub fn init_tables(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS box_art_overrides (
                    system TEXT NOT NULL,
                    rom_filename TEXT NOT NULL,
                    override_path TEXT NOT NULL,
                    set_at INTEGER NOT NULL,
                    PRIMARY KEY (system, rom_filename)
                );

                CREATE TABLE IF NOT EXISTS game_videos (
                    -- Identity
                    system TEXT NOT NULL,
                    base_title TEXT NOT NULL DEFAULT '',
                    rom_filename TEXT NOT NULL,
                    -- Content
                    video_id TEXT NOT NULL,
                    url TEXT NOT NULL,
                    platform TEXT NOT NULL,
                    platform_video_id TEXT NOT NULL,
                    title TEXT,
                    -- Metadata
                    added_at INTEGER NOT NULL,
                    from_recommendation INTEGER NOT NULL DEFAULT 0,
                    tag TEXT,
                    PRIMARY KEY (system, rom_filename, video_id)
                );

                CREATE INDEX IF NOT EXISTS idx_game_videos_base_title
                    ON game_videos (system, base_title);

                CREATE TABLE IF NOT EXISTS game_manual_resource (
                    system TEXT NOT NULL,
                    base_title TEXT NOT NULL DEFAULT '',
                    rom_filename TEXT NOT NULL,
                    manual_id TEXT NOT NULL,
                    resource_key TEXT NOT NULL,
                    title TEXT,
                    origin TEXT NOT NULL,
                    provider TEXT,
                    url TEXT,
                    storage_path TEXT,
                    original_filename TEXT,
                    languages TEXT NOT NULL DEFAULT '',
                    mime_type TEXT NOT NULL DEFAULT '',
                    size_bytes INTEGER,
                    added_at INTEGER NOT NULL,
                    CHECK (origin IN ('downloaded', 'upload')),
                    CHECK (origin != 'downloaded' OR (url IS NOT NULL AND storage_path IS NOT NULL)),
                    CHECK (origin != 'upload' OR (url IS NULL AND storage_path IS NOT NULL)),
                    PRIMARY KEY (system, rom_filename, manual_id)
                );

                CREATE INDEX IF NOT EXISTS game_manual_resource_idx_base_title
                    ON game_manual_resource(system, base_title);
                CREATE INDEX IF NOT EXISTS game_manual_resource_idx_resource_key
                    ON game_manual_resource(system, rom_filename, resource_key);

                CREATE TABLE IF NOT EXISTS game_resource_link (
                    system TEXT NOT NULL,
                    base_title TEXT NOT NULL DEFAULT '',
                    rom_filename TEXT NOT NULL,
                    link_id TEXT NOT NULL,
                    url TEXT NOT NULL,
                    title TEXT NOT NULL,
                    source TEXT,
                    resource_type TEXT NOT NULL,
                    added_at INTEGER NOT NULL,
                    PRIMARY KEY (system, rom_filename, link_id)
                );

                CREATE INDEX IF NOT EXISTS game_resource_link_idx_base_title
                    ON game_resource_link(system, base_title);
                CREATE INDEX IF NOT EXISTS game_resource_link_idx_url
                    ON game_resource_link(system, rom_filename, url);",
        )
        .map_err(|e| Error::Other(format!("Failed to init user_data DB: {e}")))?;
        Self::migrate_tables(conn)?;
        Ok(())
    }

    fn migrate_tables(conn: &Connection) -> Result<()> {
        if !table_has_column(conn, "game_manual_resource", "provider") {
            conn.execute(
                "ALTER TABLE game_manual_resource ADD COLUMN provider TEXT",
                [],
            )
            .map_err(|e| Error::Other(format!("Failed to add manual provider column: {e}")))?;
        }
        Ok(())
    }

    /// Get the override path for a single ROM, if one exists.
    pub fn get_override(
        conn: &Connection,
        system: &str,
        rom_filename: &str,
    ) -> Result<Option<String>> {
        conn.query_row(
            "SELECT override_path FROM box_art_overrides
                 WHERE system = ?1 AND rom_filename = ?2",
            params![system, rom_filename],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| Error::Other(format!("Failed to query box_art_overrides: {e}")))
    }

    /// Set (insert or replace) a box art override.
    pub fn set_override(
        conn: &Connection,
        system: &str,
        rom_filename: &str,
        override_path: &str,
    ) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        conn.execute(
            "INSERT OR REPLACE INTO box_art_overrides (system, rom_filename, override_path, set_at)
                 VALUES (?1, ?2, ?3, ?4)",
            params![system, rom_filename, override_path, now],
        )
        .map_err(|e| Error::Other(format!("Failed to set box_art_override: {e}")))?;
        Ok(())
    }

    /// Remove a box art override (revert to default).
    pub fn remove_override(conn: &Connection, system: &str, rom_filename: &str) -> Result<()> {
        conn.execute(
            "DELETE FROM box_art_overrides WHERE system = ?1 AND rom_filename = ?2",
            params![system, rom_filename],
        )
        .map_err(|e| Error::Other(format!("Failed to remove box_art_override: {e}")))?;
        Ok(())
    }

    /// Get all overrides for a system. Returns rom_filename -> override_path.
    pub fn get_system_overrides(
        conn: &Connection,
        system: &str,
    ) -> Result<HashMap<String, String>> {
        let mut stmt = conn
            .prepare("SELECT rom_filename, override_path FROM box_art_overrides WHERE system = ?1")
            .map_err(|e| Error::Other(format!("Failed to prepare system overrides query: {e}")))?;

        let rows = stmt
            .query_map(params![system], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| Error::Other(format!("Failed to query system overrides: {e}")))?;

        let mut map = HashMap::new();
        for row in rows.flatten() {
            map.insert(row.0, row.1);
        }
        Ok(map)
    }

    // --- Game Videos ---

    /// Get all saved videos for a game by base_title, ordered newest first.
    ///
    /// Queries by `(system, base_title)` so regional variants share videos.
    /// `base_titles` should include the primary base_title plus any alias
    /// base_titles resolved from `game_alias` in `library.db`.
    pub fn get_game_videos(
        conn: &Connection,
        system: &str,
        base_titles: &[&str],
    ) -> Result<Vec<VideoEntry>> {
        if base_titles.is_empty() {
            return Ok(Vec::new());
        }

        // Build a WHERE clause with placeholders for IN (...)
        let placeholders: Vec<String> = (0..base_titles.len())
            .map(|i| format!("?{}", i + 2))
            .collect();
        let sql = format!(
            "SELECT video_id, url, platform, platform_video_id, title,
                    added_at, from_recommendation, tag, rom_filename
             FROM game_videos
             WHERE system = ?1 AND base_title IN ({})
             ORDER BY added_at DESC",
            placeholders.join(", ")
        );

        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| Error::Other(format!("Failed to prepare get_game_videos: {e}")))?;

        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> =
            Vec::with_capacity(1 + base_titles.len());
        param_values.push(Box::new(system.to_string()));
        for bt in base_titles {
            param_values.push(Box::new(bt.to_string()));
        }
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();

        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                let platform: String = row.get(2)?;
                let platform_video_id: String = row.get(3)?;
                Ok(VideoEntry {
                    id: format!("{platform}-{platform_video_id}"),
                    url: row.get(1)?,
                    platform,
                    video_id: platform_video_id,
                    title: row.get(4)?,
                    added_at: row.get::<_, i64>(5)? as u64,
                    from_recommendation: row.get::<_, i64>(6)? != 0,
                    tag: row.get(7)?,
                    rom_filename: row.get(8)?,
                })
            })
            .map_err(|e| Error::Other(format!("Failed to query game_videos: {e}")))?;

        // Deduplicate by video_id (same video saved from different ROMs).
        let mut seen = std::collections::HashSet::new();
        let mut videos = Vec::new();
        for row in rows.flatten() {
            if seen.insert(row.id.clone()) {
                videos.push(row);
            }
        }
        Ok(videos)
    }

    /// Add a video to a game's list. Returns an error if a duplicate exists.
    pub fn add_game_video(
        conn: &Connection,
        system: &str,
        rom_filename: &str,
        base_title: &str,
        entry: &VideoEntry,
    ) -> Result<()> {
        let exists = conn
            .query_row(
                "SELECT 1 FROM game_videos
                 WHERE system = ?1 AND base_title = ?2 AND video_id = ?3
                 LIMIT 1",
                params![system, base_title, &entry.id],
                |_| Ok(()),
            )
            .optional()
            .map_err(|e| Error::Other(format!("Failed to check game_video duplicate: {e}")))?;
        if exists.is_some() {
            return Err(Error::Other("This video is already saved.".to_string()));
        }

        let affected = conn
            .execute(
                "INSERT INTO game_videos
                    (system, base_title, rom_filename, video_id, url, platform,
                     platform_video_id, title, added_at, from_recommendation, tag)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    system,
                    base_title,
                    rom_filename,
                    &entry.id,
                    &entry.url,
                    &entry.platform,
                    &entry.video_id,
                    &entry.title,
                    entry.added_at as i64,
                    entry.from_recommendation as i64,
                    &entry.tag,
                ],
            )
            .map_err(|e| Error::Other(format!("Failed to add game_video: {e}")))?;

        if affected == 0 {
            return Err(Error::Other("This video is already saved.".to_string()));
        }
        Ok(())
    }

    /// Remove a saved video by its ID from a game's list.
    pub fn remove_game_video(
        conn: &Connection,
        system: &str,
        rom_filename: &str,
        video_id: &str,
    ) -> Result<()> {
        let base_title = conn
            .query_row(
                "SELECT base_title FROM game_videos
                 WHERE system = ?1 AND rom_filename = ?2 AND video_id = ?3",
                params![system, rom_filename, video_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|e| Error::Other(format!("Failed to query game_video: {e}")))?;
        if let Some(base_title) = base_title {
            conn.execute(
                "DELETE FROM game_videos
                 WHERE system = ?1 AND base_title = ?2 AND video_id = ?3",
                params![system, base_title, video_id],
            )
            .map_err(|e| Error::Other(format!("Failed to remove game_video: {e}")))?;
        }
        Ok(())
    }

    pub fn get_game_manuals(
        conn: &Connection,
        system: &str,
        base_titles: &[&str],
    ) -> Result<Vec<ManualEntry>> {
        if base_titles.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders: Vec<String> = (0..base_titles.len())
            .map(|i| format!("?{}", i + 2))
            .collect();
        let sql = format!(
            "SELECT manual_id, resource_key, title, origin, provider, url, storage_path,
                    original_filename, languages, mime_type, size_bytes, added_at
             FROM game_manual_resource
             WHERE system = ?1 AND base_title IN ({})
             ORDER BY added_at DESC",
            placeholders.join(", ")
        );
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| Error::Other(format!("Failed to prepare get_game_manuals: {e}")))?;
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> =
            Vec::with_capacity(1 + base_titles.len());
        param_values.push(Box::new(system.to_string()));
        for bt in base_titles {
            param_values.push(Box::new(bt.to_string()));
        }
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();
        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                let size_bytes: Option<i64> = row.get(10)?;
                Ok(ManualEntry {
                    manual_id: row.get(0)?,
                    resource_key: row.get(1)?,
                    title: row.get(2)?,
                    origin: ManualOrigin::from_db(row.get(3)?),
                    provider: row.get(4)?,
                    url: row.get(5)?,
                    storage_path: row.get(6)?,
                    original_filename: row.get(7)?,
                    languages: row.get(8)?,
                    mime_type: row.get(9)?,
                    size_bytes: size_bytes.map(|v| v.max(0) as u64),
                    added_at: row.get::<_, i64>(11)? as u64,
                })
            })
            .map_err(|e| Error::Other(format!("Failed to query game_manual_resource: {e}")))?;
        let mut manuals = Vec::new();
        for row in rows.flatten() {
            manuals.push(row);
        }
        Ok(manuals)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_game_manual(
        conn: &Connection,
        system: &str,
        rom_filename: &str,
        base_title: &str,
        entry: &ManualEntry,
    ) -> Result<()> {
        conn.execute(
            "INSERT OR REPLACE INTO game_manual_resource
                (system, base_title, rom_filename, manual_id, resource_key, title, origin,
                 provider, url, storage_path, original_filename, languages, mime_type, size_bytes,
                 added_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                system,
                base_title,
                rom_filename,
                &entry.manual_id,
                &entry.resource_key,
                &entry.title,
                entry.origin.to_db_value(),
                &entry.provider,
                &entry.url,
                &entry.storage_path,
                &entry.original_filename,
                &entry.languages,
                &entry.mime_type,
                entry.size_bytes.map(|v| v as i64),
                entry.added_at as i64,
            ],
        )
        .map_err(|e| Error::Other(format!("Failed to add game_manual_resource: {e}")))?;
        Ok(())
    }

    pub fn remove_game_manual(
        conn: &Connection,
        system: &str,
        manual_id: &str,
    ) -> Result<Option<ManualEntry>> {
        let entry = conn
            .query_row(
                "SELECT manual_id, resource_key, title, origin, provider, url, storage_path,
                        original_filename, languages, mime_type, size_bytes, added_at
                 FROM game_manual_resource
                 WHERE system = ?1 AND manual_id = ?2",
                params![system, manual_id],
                |row| {
                    let size_bytes: Option<i64> = row.get(10)?;
                    Ok(ManualEntry {
                        manual_id: row.get(0)?,
                        resource_key: row.get(1)?,
                        title: row.get(2)?,
                        origin: ManualOrigin::from_db(row.get(3)?),
                        provider: row.get(4)?,
                        url: row.get(5)?,
                        storage_path: row.get(6)?,
                        original_filename: row.get(7)?,
                        languages: row.get(8)?,
                        mime_type: row.get(9)?,
                        size_bytes: size_bytes.map(|v| v.max(0) as u64),
                        added_at: row.get::<_, i64>(11)? as u64,
                    })
                },
            )
            .optional()
            .map_err(|e| Error::Other(format!("Failed to query game_manual_resource: {e}")))?;
        if entry.is_some() {
            conn.execute(
                "DELETE FROM game_manual_resource WHERE system = ?1 AND manual_id = ?2",
                params![system, manual_id],
            )
            .map_err(|e| Error::Other(format!("Failed to remove game_manual_resource: {e}")))?;
        }
        Ok(entry)
    }

    pub fn get_game_resource_links(
        conn: &Connection,
        system: &str,
        base_titles: &[&str],
    ) -> Result<Vec<ResourceEntry>> {
        if base_titles.is_empty() {
            return Ok(Vec::new());
        }

        let placeholders: Vec<String> = (0..base_titles.len())
            .map(|i| format!("?{}", i + 2))
            .collect();
        let sql = format!(
            "SELECT link_id, url, title, source, resource_type, added_at, rom_filename
             FROM game_resource_link
             WHERE system = ?1 AND base_title IN ({})
             ORDER BY added_at DESC",
            placeholders.join(", ")
        );
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| Error::Other(format!("Failed to prepare get_game_resource_links: {e}")))?;

        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> =
            Vec::with_capacity(1 + base_titles.len());
        param_values.push(Box::new(system.to_string()));
        for bt in base_titles {
            param_values.push(Box::new(bt.to_string()));
        }
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();

        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                Ok(ResourceEntry {
                    id: row.get(0)?,
                    url: row.get(1)?,
                    title: row.get(2)?,
                    source: row.get(3)?,
                    resource_type: row.get(4)?,
                    added_at: row.get::<_, i64>(5)? as u64,
                    rom_filename: row.get(6)?,
                })
            })
            .map_err(|e| Error::Other(format!("Failed to query game_resource_link: {e}")))?;

        let mut seen = std::collections::HashSet::new();
        let mut links = Vec::new();
        for row in rows.flatten() {
            if seen.insert(normalized_resource_url_key(&row.url)) {
                links.push(row);
            }
        }
        Ok(links)
    }

    pub fn add_game_resource_link(
        conn: &Connection,
        system: &str,
        rom_filename: &str,
        base_title: &str,
        entry: &ResourceEntry,
    ) -> Result<()> {
        let normalized_url = normalized_resource_url_key(&entry.url);
        let exists = conn
            .query_row(
                "SELECT 1 FROM game_resource_link
                 WHERE system = ?1
                   AND base_title = ?2
                   AND (link_id = ?3 OR lower(rtrim(url, '/')) = ?4)
                 LIMIT 1",
                params![system, base_title, &entry.id, normalized_url],
                |_| Ok(()),
            )
            .optional()
            .map_err(|e| {
                Error::Other(format!("Failed to check game_resource_link duplicate: {e}"))
            })?;
        if exists.is_some() {
            return Err(Error::Other("This link is already saved.".to_string()));
        }

        let affected = conn
            .execute(
                "INSERT INTO game_resource_link
                    (system, base_title, rom_filename, link_id, url, title, source,
                     resource_type, added_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    system,
                    base_title,
                    rom_filename,
                    &entry.id,
                    &entry.url,
                    &entry.title,
                    &entry.source,
                    &entry.resource_type,
                    entry.added_at as i64,
                ],
            )
            .map_err(|e| Error::Other(format!("Failed to add game_resource_link: {e}")))?;

        if affected == 0 {
            return Err(Error::Other("This link is already saved.".to_string()));
        }
        Ok(())
    }

    pub fn remove_game_resource_link(
        conn: &Connection,
        system: &str,
        rom_filename: &str,
        link_id: &str,
    ) -> Result<()> {
        let row = conn
            .query_row(
                "SELECT base_title, url FROM game_resource_link
                 WHERE system = ?1 AND rom_filename = ?2 AND link_id = ?3",
                params![system, rom_filename, link_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(|e| Error::Other(format!("Failed to query game_resource_link: {e}")))?;
        if let Some((base_title, url)) = row {
            let normalized_url = normalized_resource_url_key(&url);
            conn.execute(
                "DELETE FROM game_resource_link
                 WHERE system = ?1
                   AND base_title = ?2
                   AND (link_id = ?3 OR lower(rtrim(url, '/')) = ?4)",
                params![system, base_title, link_id, normalized_url],
            )
            .map_err(|e| Error::Other(format!("Failed to remove game_resource_link: {e}")))?;
        }
        Ok(())
    }

    /// Delete all user data entries for a ROM (box art overrides, videos, manuals, links).
    pub fn delete_for_rom(conn: &Connection, system: &str, rom_filename: &str) {
        let _ = conn.execute(
            "DELETE FROM box_art_overrides WHERE system = ?1 AND rom_filename = ?2",
            params![system, rom_filename],
        );
        let _ = conn.execute(
            "DELETE FROM game_videos WHERE system = ?1 AND rom_filename = ?2",
            params![system, rom_filename],
        );
        let _ = conn.execute(
            "DELETE FROM game_manual_resource WHERE system = ?1 AND rom_filename = ?2",
            params![system, rom_filename],
        );
        let _ = conn.execute(
            "DELETE FROM game_resource_link WHERE system = ?1 AND rom_filename = ?2",
            params![system, rom_filename],
        );
    }

    /// Rename a ROM across all user data tables.
    pub fn rename_for_rom(conn: &Connection, system: &str, old_filename: &str, new_filename: &str) {
        if let Err(e) = conn.execute(
            "UPDATE box_art_overrides SET rom_filename = ?3 WHERE system = ?1 AND rom_filename = ?2",
            params![system, old_filename, new_filename],
        ) {
            tracing::warn!("Failed to update box_art_overrides: {e}");
        }
        if let Err(e) = conn.execute(
            "UPDATE game_videos SET rom_filename = ?3 WHERE system = ?1 AND rom_filename = ?2",
            params![system, old_filename, new_filename],
        ) {
            tracing::warn!("Failed to update game_videos: {e}");
        }
        if let Err(e) = conn.execute(
            "UPDATE game_manual_resource SET rom_filename = ?3 WHERE system = ?1 AND rom_filename = ?2",
            params![system, old_filename, new_filename],
        ) {
            tracing::warn!("Failed to update game_manual_resource: {e}");
        }
        if let Err(e) = conn.execute(
            "UPDATE game_resource_link SET rom_filename = ?3 WHERE system = ?1 AND rom_filename = ?2",
            params![system, old_filename, new_filename],
        ) {
            tracing::warn!("Failed to update game_resource_link: {e}");
        }
    }
}

fn table_has_column(conn: &Connection, table: &str, column: &str) -> bool {
    let Ok(mut stmt) = conn.prepare(&format!("PRAGMA table_info({table})")) else {
        return false;
    };
    let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(1)) else {
        return false;
    };
    rows.filter_map(std::result::Result::ok)
        .any(|name| name == column)
}

fn normalized_resource_url_key(url: &str) -> String {
    url.trim_end_matches('/').to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::StorageKind;

    fn open_temp() -> (Connection, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let storage =
            crate::storage::StorageLocation::from_path(tmp.path().to_path_buf(), StorageKind::Sd);
        let (conn, corrupt) = UserDataDb::open_at(&UserDataDb::db_path(&storage.root)).unwrap();
        assert!(!corrupt);
        (conn, tmp)
    }

    #[test]
    fn manual_resource_round_trips_and_deletes() {
        let (conn, _tmp) = open_temp();
        let entry = ManualEntry {
            manual_id: "urlhash:abc".to_string(),
            resource_key: "url:https://example.com/manual.pdf".to_string(),
            title: Some("Manual".to_string()),
            origin: ManualOrigin::Downloaded,
            provider: Some("retrokit".to_string()),
            url: Some("https://example.com/manual.pdf".to_string()),
            storage_path: Some("nintendo_snes/urlhash_abc.pdf".to_string()),
            original_filename: Some("urlhash_abc.pdf".to_string()),
            languages: "en,es".to_string(),
            mime_type: "application/pdf".to_string(),
            size_bytes: Some(123),
            added_at: 42,
        };
        UserDataDb::add_game_manual(
            &conn,
            "nintendo_snes",
            "Super Mario World.sfc",
            "super mario world",
            &entry,
        )
        .unwrap();

        let rows =
            UserDataDb::get_game_manuals(&conn, "nintendo_snes", &["super mario world"]).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].resource_key, entry.resource_key);
        assert_eq!(rows[0].origin, ManualOrigin::Downloaded);
        assert_eq!(rows[0].provider.as_deref(), Some("retrokit"));
        assert_eq!(rows[0].languages, "en,es");

        let removed =
            UserDataDb::remove_game_manual(&conn, "nintendo_snes", "urlhash:abc").unwrap();
        assert!(removed.is_some());
        let rows =
            UserDataDb::get_game_manuals(&conn, "nintendo_snes", &["super mario world"]).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn resource_link_round_trips_and_deletes() {
        let (conn, _tmp) = open_temp();
        let entry = ResourceEntry {
            id: "urlhash:abc".to_string(),
            url: "https://example.com/guide".to_string(),
            title: "Strategy guide".to_string(),
            source: Some("shmups_wiki".to_string()),
            resource_type: "strategy_guide".to_string(),
            added_at: 42,
            rom_filename: "DoDonPachi DaiOuJou.zip".to_string(),
        };
        UserDataDb::add_game_resource_link(
            &conn,
            "arcade",
            "DoDonPachi DaiOuJou.zip",
            "dodonpachi daioujou",
            &entry,
        )
        .unwrap();

        let rows =
            UserDataDb::get_game_resource_links(&conn, "arcade", &["dodonpachi daioujou"]).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].url, entry.url);
        assert_eq!(rows[0].source.as_deref(), Some("shmups_wiki"));

        UserDataDb::remove_game_resource_link(
            &conn,
            "arcade",
            "DoDonPachi DaiOuJou.zip",
            "urlhash:abc",
        )
        .unwrap();
        let rows =
            UserDataDb::get_game_resource_links(&conn, "arcade", &["dodonpachi daioujou"]).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn video_duplicate_detection_and_removal_use_base_title() {
        let (conn, _tmp) = open_temp();
        let entry = VideoEntry {
            id: "youtube-abc123".to_string(),
            url: "https://www.youtube.com/watch?v=abc123".to_string(),
            platform: "youtube".to_string(),
            video_id: "abc123".to_string(),
            title: Some("Gameplay".to_string()),
            added_at: 42,
            from_recommendation: true,
            tag: Some("gameplay".to_string()),
            rom_filename: "Sonic (USA).md".to_string(),
        };
        UserDataDb::add_game_video(&conn, "sega_smd", "Sonic (USA).md", "sonic", &entry).unwrap();

        let mut duplicate = entry.clone();
        duplicate.rom_filename = "Sonic (Europe).md".to_string();
        assert!(
            UserDataDb::add_game_video(
                &conn,
                "sega_smd",
                "Sonic (Europe).md",
                "sonic",
                &duplicate,
            )
            .is_err()
        );

        conn.execute(
            "INSERT INTO game_videos
                (system, base_title, rom_filename, video_id, url, platform,
                 platform_video_id, title, added_at, from_recommendation, tag)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                "sega_smd",
                "sonic",
                "Sonic (Europe).md",
                &duplicate.id,
                &duplicate.url,
                &duplicate.platform,
                &duplicate.video_id,
                &duplicate.title,
                duplicate.added_at as i64,
                duplicate.from_recommendation as i64,
                &duplicate.tag,
            ],
        )
        .unwrap();

        UserDataDb::remove_game_video(&conn, "sega_smd", "Sonic (USA).md", "youtube-abc123")
            .unwrap();
        let rows = UserDataDb::get_game_videos(&conn, "sega_smd", &["sonic"]).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn resource_link_duplicate_detection_and_removal_use_base_title() {
        let (conn, _tmp) = open_temp();
        let entry = ResourceEntry {
            id: "urlhash:guide".to_string(),
            url: "https://example.com/guide".to_string(),
            title: "Guide".to_string(),
            source: Some("shmups_wiki".to_string()),
            resource_type: "strategy_guide".to_string(),
            added_at: 42,
            rom_filename: "Sonic (USA).md".to_string(),
        };
        UserDataDb::add_game_resource_link(&conn, "sega_smd", "Sonic (USA).md", "sonic", &entry)
            .unwrap();

        let mut duplicate = entry.clone();
        duplicate.rom_filename = "Sonic (Europe).md".to_string();
        assert!(
            UserDataDb::add_game_resource_link(
                &conn,
                "sega_smd",
                "Sonic (Europe).md",
                "sonic",
                &duplicate,
            )
            .is_err()
        );

        conn.execute(
            "INSERT INTO game_resource_link
                (system, base_title, rom_filename, link_id, url, title, source,
                 resource_type, added_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                "sega_smd",
                "sonic",
                "Sonic (Europe).md",
                &duplicate.id,
                &duplicate.url,
                &duplicate.title,
                &duplicate.source,
                &duplicate.resource_type,
                duplicate.added_at as i64,
            ],
        )
        .unwrap();

        UserDataDb::remove_game_resource_link(&conn, "sega_smd", "Sonic (USA).md", "urlhash:guide")
            .unwrap();
        let rows = UserDataDb::get_game_resource_links(&conn, "sega_smd", &["sonic"]).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn resource_link_duplicate_detection_and_removal_use_normalized_url() {
        let (conn, _tmp) = open_temp();
        let entry = ResourceEntry {
            id: "urlhash:guide-normalized".to_string(),
            url: "https://example.com/Guide".to_string(),
            title: "Guide".to_string(),
            source: Some("saved".to_string()),
            resource_type: "link".to_string(),
            added_at: 42,
            rom_filename: "Sonic (USA).md".to_string(),
        };
        UserDataDb::add_game_resource_link(&conn, "sega_smd", "Sonic (USA).md", "sonic", &entry)
            .unwrap();

        let duplicate = ResourceEntry {
            id: "urlhash:guide-trailing-slash".to_string(),
            url: "https://example.com/guide/".to_string(),
            title: "Guide duplicate".to_string(),
            source: Some("saved".to_string()),
            resource_type: "link".to_string(),
            added_at: 43,
            rom_filename: "Sonic (Europe).md".to_string(),
        };
        assert!(
            UserDataDb::add_game_resource_link(
                &conn,
                "sega_smd",
                "Sonic (Europe).md",
                "sonic",
                &duplicate,
            )
            .is_err()
        );

        conn.execute(
            "INSERT INTO game_resource_link
                (system, base_title, rom_filename, link_id, url, title, source,
                 resource_type, added_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                "sega_smd",
                "sonic",
                "Sonic (Europe).md",
                &duplicate.id,
                &duplicate.url,
                &duplicate.title,
                &duplicate.source,
                &duplicate.resource_type,
                duplicate.added_at as i64,
            ],
        )
        .unwrap();

        UserDataDb::remove_game_resource_link(
            &conn,
            "sega_smd",
            "Sonic (USA).md",
            "urlhash:guide-normalized",
        )
        .unwrap();
        let rows = UserDataDb::get_game_resource_links(&conn, "sega_smd", &["sonic"]).unwrap();
        assert!(rows.is_empty());
    }
}
