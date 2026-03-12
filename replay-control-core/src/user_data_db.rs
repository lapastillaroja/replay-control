//! User data database — persistent user customizations that survive metadata clears.
//!
//! Stored at `<rom_storage>/.replay-control/user_data.db`.
//! Separate from `metadata.db` (which is a rebuildable cache) to ensure user
//! choices are never accidentally destroyed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};

use crate::error::{Error, Result};
use crate::storage::RC_DIR;

/// Filename for the SQLite user data database.
pub const USER_DATA_DB_FILE: &str = "user_data.db";

/// Handle to the user data SQLite database.
pub struct UserDataDb {
    conn: Connection,
    db_path: PathBuf,
}

impl UserDataDb {
    /// Tables to probe for corruption detection.
    /// NOTE: update this list when adding new tables.
    const TABLES: &[&str] = &["box_art_overrides"];

    /// Open (or create) the user data database at `<storage_root>/.replay-control/user_data.db`.
    ///
    /// Uses the shared nolock→WAL open strategy (see `db_common`), runs table
    /// init, then probes all tables for corruption — auto-recreates if corrupt.
    pub fn open(storage_root: &Path) -> Result<Self> {
        let dir = storage_root.join(RC_DIR);
        std::fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
        let db_path = dir.join(USER_DATA_DB_FILE);

        let conn = crate::db_common::open_connection(&db_path, "user_data.db")?;
        let db = Self {
            conn,
            db_path: db_path.clone(),
        };
        db.init()?;

        if let Err(detail) = crate::db_common::probe_tables(&db.conn, Self::TABLES) {
            tracing::warn!("User data DB corrupt ({detail}), deleting and recreating");
            drop(db);
            crate::db_common::delete_db_files(&db_path);
            let conn = crate::db_common::open_connection(&db_path, "user_data.db")?;
            let db = Self { conn, db_path };
            db.init()?;
            return Ok(db);
        }

        Ok(db)
    }

    fn init(&self) -> Result<()> {
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS box_art_overrides (
                    system TEXT NOT NULL,
                    rom_filename TEXT NOT NULL,
                    override_path TEXT NOT NULL,
                    set_at INTEGER NOT NULL,
                    PRIMARY KEY (system, rom_filename)
                );",
            )
            .map_err(|e| Error::Other(format!("Failed to init user_data DB: {e}")))?;
        Ok(())
    }

    /// Get the override path for a single ROM, if one exists.
    pub fn get_override(&self, system: &str, rom_filename: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
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
        &self,
        system: &str,
        rom_filename: &str,
        override_path: &str,
    ) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        self.conn
            .execute(
                "INSERT OR REPLACE INTO box_art_overrides (system, rom_filename, override_path, set_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![system, rom_filename, override_path, now],
            )
            .map_err(|e| Error::Other(format!("Failed to set box_art_override: {e}")))?;
        Ok(())
    }

    /// Remove a box art override (revert to default).
    pub fn remove_override(&self, system: &str, rom_filename: &str) -> Result<()> {
        self.conn
            .execute(
                "DELETE FROM box_art_overrides WHERE system = ?1 AND rom_filename = ?2",
                params![system, rom_filename],
            )
            .map_err(|e| Error::Other(format!("Failed to remove box_art_override: {e}")))?;
        Ok(())
    }

    /// Get all overrides for a system. Returns rom_filename -> override_path.
    pub fn get_system_overrides(&self, system: &str) -> Result<HashMap<String, String>> {
        let mut stmt = self
            .conn
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

    /// Path to the database file on disk.
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }
}
