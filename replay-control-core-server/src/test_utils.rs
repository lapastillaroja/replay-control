//! Shared fixtures for unit and integration tests.
//!
//! Workspace-internal crate, so these helpers are exposed unconditionally as a
//! plain `pub mod test_utils` — `cargo test` picks them up with no feature
//! flag, and unused symbols are dropped from release binaries by LTO. Switch
//! to a feature-gated module if this crate is ever published.

use rusqlite::params;
use tempfile::TempDir;

use crate::DbPool;
use crate::library_db::LibraryDb;

/// Build a real `DbPool` over a fresh library DB inside a tempdir.
///
/// Schema is initialised by `LibraryDb::open`, the same path used in
/// production. Drop the returned `TempDir` last; the pool retains a path
/// reference to it.
pub fn build_library_pool() -> (DbPool, TempDir) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (_, db_path) = LibraryDb::open(tmp.path()).expect("open library db");
    let pool = DbPool::new(db_path, "library_db", LibraryDb::open).expect("build pool");
    (pool, tmp)
}

/// Insert a minimal `game_library` row.
///
/// Default-only columns (genre, region, developer, etc.) are left at their
/// schema defaults. Pass `base_title = ""` if a test only cares about
/// `visible_filenames`/`active_systems` and not alias matching.
pub async fn insert_game_library_row(
    pool: &DbPool,
    system: &str,
    base_title: &str,
    rom_filename: &str,
) {
    let system = system.to_string();
    let base_title = base_title.to_string();
    let rom = rom_filename.to_string();
    pool.write(move |db| {
        db.execute(
            "INSERT INTO game_library
             (system, rom_filename, rom_path, display_name,
              base_title, series_key, region, developer, search_text)
             VALUES (?1, ?2, ?3, ?4, ?5, '', '', '', '')",
            params![
                system,
                rom,
                format!("/{system}/{rom}"),
                base_title,
                base_title,
            ],
        )
    })
    .await
    .expect("pool open")
    .expect("insert succeeds");
}
