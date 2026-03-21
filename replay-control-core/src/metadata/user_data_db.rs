//! User data database — persistent user customizations that survive metadata clears.
//!
//! Stored at `<rom_storage>/.replay-control/user_data.db`.
//! Separate from `metadata.db` (which is a rebuildable cache) to ensure user
//! choices are never accidentally destroyed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::storage::RC_DIR;

/// Filename for the SQLite user data database.
pub const USER_DATA_DB_FILE: &str = "user_data.db";

/// A single saved video entry for a game.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoEntry {
    /// Unique ID: "{platform}-{video_id}"
    pub id: String,
    /// Sanitized canonical URL
    pub url: String,
    /// Platform name (e.g., "youtube")
    pub platform: String,
    /// Platform-specific video ID
    pub video_id: String,
    /// Human-readable title (from user or search results)
    pub title: Option<String>,
    /// Unix timestamp when the video was added
    pub added_at: u64,
    /// Whether this was pinned from a recommendation search
    pub from_recommendation: bool,
    /// Tag: "trailer", "gameplay", "1cc", or None for manual
    pub tag: Option<String>,
    /// The ROM filename this video was originally saved from (for delete path).
    pub rom_filename: String,
}

/// Stateless query namespace for the user data SQLite database.
///
/// All methods are associated functions that take `conn: &Connection` as their
/// first parameter. No connection ownership — the pool manages lifecycle.
pub struct UserDataDb;

impl UserDataDb {
    /// Tables to probe for corruption detection.
    /// NOTE: update this list when adding new tables.
    pub const TABLES: &[&str] = &["box_art_overrides", "game_videos"];

    /// Open (or create) the user data database at `<storage_root>/.replay-control/user_data.db`.
    ///
    /// Uses the shared nolock->WAL open strategy (see `db_common`), runs table
    /// init, then probes all tables for corruption — auto-recreates if corrupt.
    /// Returns a raw `Connection` — the caller (or pool manager) owns it.
    pub fn open(storage_root: &Path, is_local: bool) -> Result<(Connection, PathBuf)> {
        let dir = storage_root.join(RC_DIR);
        std::fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
        let db_path = dir.join(USER_DATA_DB_FILE);

        let conn = crate::db_common::open_connection(&db_path, "user_data.db", is_local)?;
        Self::init_tables(&conn)?;

        if let Err(detail) = crate::db_common::probe_tables(&conn, Self::TABLES) {
            tracing::warn!("User data DB corrupt ({detail}), deleting and recreating");
            drop(conn);
            crate::db_common::delete_db_files(&db_path);
            let conn = crate::db_common::open_connection(&db_path, "user_data.db", is_local)?;
            Self::init_tables(&conn)?;
            return Ok((conn, db_path));
        }

        Ok((conn, db_path))
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
                    ON game_videos (system, base_title);",
            )
            .map_err(|e| Error::Other(format!("Failed to init user_data DB: {e}")))?;
        Ok(())
    }

    /// Get the override path for a single ROM, if one exists.
    pub fn get_override(conn: &Connection, system: &str, rom_filename: &str) -> Result<Option<String>> {
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
    pub fn get_system_overrides(conn: &Connection, system: &str) -> Result<HashMap<String, String>> {
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
    /// base_titles resolved from `game_alias` in `metadata.db`.
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
        let affected = conn
            .execute(
                "INSERT OR IGNORE INTO game_videos
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
        conn.execute(
                "DELETE FROM game_videos
                 WHERE system = ?1 AND rom_filename = ?2 AND video_id = ?3",
                params![system, rom_filename, video_id],
            )
            .map_err(|e| Error::Other(format!("Failed to remove game_video: {e}")))?;
        Ok(())
    }
}
