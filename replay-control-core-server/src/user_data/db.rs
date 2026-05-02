//! User data database — persistent user customizations that survive metadata clears.
//!
//! Stored at `<rom_storage>/.replay-control/user_data.db`.
//! Separate from `library.db` (which is a rebuildable cache) to ensure user
//! choices are never accidentally destroyed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};

use crate::storage::RC_DIR;
use replay_control_core::error::{Error, Result};

/// Filename for the SQLite user data database.
pub const USER_DATA_DB_FILE: &str = "user_data.db";

pub use replay_control_core::user_data_db::VideoEntry;
pub use replay_control_core::want_to_play::{HltbData, WantToPlayEntry};

/// Stateless query namespace for the user data SQLite database.
///
/// All methods are associated functions that take `conn: &Connection` as their
/// first parameter. No connection ownership — the pool manages lifecycle.
pub struct UserDataDb;

impl UserDataDb {
    /// Tables to probe for corruption detection.
    /// NOTE: update this list when adding new tables.
    pub const TABLES: &[&str] = &["box_art_overrides", "game_videos", "want_to_play", "hltb_cache"];

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

                CREATE TABLE IF NOT EXISTS want_to_play (
                    system TEXT NOT NULL,
                    rom_filename TEXT NOT NULL,
                    base_title TEXT NOT NULL DEFAULT '',
                    added_at INTEGER NOT NULL,
                    PRIMARY KEY (system, rom_filename)
                );

                CREATE TABLE IF NOT EXISTS hltb_cache (
                    base_title TEXT NOT NULL PRIMARY KEY,
                    game_id INTEGER,
                    main_secs INTEGER,
                    main_extra_secs INTEGER,
                    completionist_secs INTEGER,
                    fetched_at INTEGER NOT NULL
                );",
        )
        .map_err(|e| Error::Other(format!("Failed to init user_data DB: {e}")))?;
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

    /// Delete all user data entries for a ROM (box art overrides, videos, backlog).
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
            "DELETE FROM want_to_play WHERE system = ?1 AND rom_filename = ?2",
            params![system, rom_filename],
        );
    }

    /// Rename a ROM across all user data tables (box art overrides + videos).
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
            "UPDATE want_to_play SET rom_filename = ?3 WHERE system = ?1 AND rom_filename = ?2",
            params![system, old_filename, new_filename],
        ) {
            tracing::warn!("Failed to update want_to_play: {e}");
        }
    }

    // --- Want To Play (Backlog) ---

    /// Add a game to the backlog. Returns true if it was newly added, false if already present.
    pub fn add_want_to_play(
        conn: &Connection,
        system: &str,
        rom_filename: &str,
        base_title: &str,
    ) -> Result<bool> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let affected = conn
            .execute(
                "INSERT OR IGNORE INTO want_to_play (system, rom_filename, base_title, added_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![system, rom_filename, base_title, now],
            )
            .map_err(|e| Error::Other(format!("Failed to add want_to_play: {e}")))?;

        Ok(affected > 0)
    }

    /// Remove a game from the backlog.
    pub fn remove_want_to_play(
        conn: &Connection,
        system: &str,
        rom_filename: &str,
    ) -> Result<()> {
        conn.execute(
            "DELETE FROM want_to_play WHERE system = ?1 AND rom_filename = ?2",
            params![system, rom_filename],
        )
        .map_err(|e| Error::Other(format!("Failed to remove want_to_play: {e}")))?;
        Ok(())
    }

    /// Check whether a specific ROM is in the backlog.
    pub fn is_want_to_play(
        conn: &Connection,
        system: &str,
        rom_filename: &str,
    ) -> Result<bool> {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM want_to_play WHERE system = ?1 AND rom_filename = ?2",
                params![system, rom_filename],
                |row| row.get(0),
            )
            .map_err(|e| Error::Other(format!("Failed to query want_to_play: {e}")))?;
        Ok(count > 0)
    }

    /// List all backlog entries ordered newest-first.
    pub fn list_want_to_play(conn: &Connection) -> Result<Vec<WantToPlayEntry>> {
        let mut stmt = conn
            .prepare(
                "SELECT system, rom_filename, base_title, added_at
                 FROM want_to_play
                 ORDER BY added_at DESC",
            )
            .map_err(|e| Error::Other(format!("Failed to prepare list_want_to_play: {e}")))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(WantToPlayEntry {
                    system: row.get(0)?,
                    rom_filename: row.get(1)?,
                    base_title: row.get(2)?,
                    added_at: row.get::<_, i64>(3)? as u64,
                })
            })
            .map_err(|e| Error::Other(format!("Failed to query want_to_play: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Delete want_to_play entry for a deleted ROM.
    pub fn delete_want_to_play_for_rom(conn: &Connection, system: &str, rom_filename: &str) {
        let _ = conn.execute(
            "DELETE FROM want_to_play WHERE system = ?1 AND rom_filename = ?2",
            params![system, rom_filename],
        );
    }

    // --- HLTB Cache ---

    const HLTB_CACHE_TTL_SECS: i64 = 7 * 24 * 3600;

    /// Look up a cached HLTB result. Returns `None` if absent or older than 7 days.
    pub fn get_hltb_cache(conn: &Connection, base_title: &str) -> Result<Option<HltbData>> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let row = conn
            .query_row(
                "SELECT game_id, main_secs, main_extra_secs, completionist_secs, fetched_at
                 FROM hltb_cache WHERE base_title = ?1",
                params![base_title],
                |row| {
                    Ok((
                        row.get::<_, Option<i64>>(0)?,
                        row.get::<_, Option<i64>>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                },
            )
            .optional()
            .map_err(|e| Error::Other(format!("Failed to query hltb_cache: {e}")))?;

        let Some((game_id, main, plus, comp100, fetched_at)) = row else {
            return Ok(None);
        };

        if now - fetched_at > Self::HLTB_CACHE_TTL_SECS {
            return Ok(None);
        }

        Ok(Some(HltbData {
            game_id: game_id.unwrap_or(0) as u64,
            main_secs: main.map(|v| v as u64),
            main_extra_secs: plus.map(|v| v as u64),
            completionist_secs: comp100.map(|v| v as u64),
        }))
    }

    /// Store a HLTB result in the cache, overwriting any previous entry.
    pub fn set_hltb_cache(
        conn: &Connection,
        base_title: &str,
        data: &HltbData,
    ) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        conn.execute(
            "INSERT OR REPLACE INTO hltb_cache
                 (base_title, game_id, main_secs, main_extra_secs, completionist_secs, fetched_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                base_title,
                data.game_id as i64,
                data.main_secs.map(|v| v as i64),
                data.main_extra_secs.map(|v| v as i64),
                data.completionist_secs.map(|v| v as i64),
                now,
            ],
        )
        .map_err(|e| Error::Other(format!("Failed to set hltb_cache: {e}")))?;
        Ok(())
    }

    /// Store a negative HLTB result (no data found) to prevent re-fetching for 7 days.
    pub fn set_hltb_cache_empty(conn: &Connection, base_title: &str) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        conn.execute(
            "INSERT OR REPLACE INTO hltb_cache
                 (base_title, game_id, main_secs, main_extra_secs, completionist_secs, fetched_at)
             VALUES (?1, NULL, NULL, NULL, NULL, ?2)",
            params![base_title, now],
        )
        .map_err(|e| Error::Other(format!("Failed to set empty hltb_cache: {e}")))?;
        Ok(())
    }
}
