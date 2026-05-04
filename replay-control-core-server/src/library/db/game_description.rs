//! Operations on the `game_description` table.
//!
//! Denormalized per-ROM cache of LaunchBox `description` + `publisher` —
//! the long-form fields the game-detail page needs. Lives in `library.db`
//! so the request path stays on a single pool; written by the enrichment
//! pass which already holds the matched `LaunchboxRow` for each ROM.

use rusqlite::{Connection, OptionalExtension, params};

use replay_control_core::error::{Error, Result};

use super::LibraryDb;

/// One row from `game_description`.
#[derive(Debug, Clone, Default)]
pub struct GameDescription {
    pub description: Option<String>,
    pub publisher: Option<String>,
}

impl LibraryDb {
    /// Look up the description + publisher for one ROM. `None` when the
    /// row doesn't exist (yet to be enriched, or no LaunchBox match).
    pub fn lookup_description(
        conn: &Connection,
        system: &str,
        rom_filename: &str,
    ) -> Result<Option<GameDescription>> {
        conn.query_row(
            "SELECT description, publisher FROM game_description
             WHERE system = ?1 AND rom_filename = ?2",
            params![system, rom_filename],
            |row| {
                Ok(GameDescription {
                    description: row.get(0)?,
                    publisher: row.get(1)?,
                })
            },
        )
        .optional()
        .map_err(|e| Error::Other(format!("lookup_description: {e}")))
    }

    /// Replace every `game_description` row for `system` with the supplied
    /// list. Run inside the enrichment write closure so the truncate +
    /// repopulate stays atomic from a reader's perspective. Rows whose
    /// description AND publisher are both `None` are dropped — empty rows
    /// would just waste space.
    pub fn replace_descriptions_for_system(
        conn: &mut Connection,
        system: &str,
        rows: &[(String, Option<String>, Option<String>)],
    ) -> Result<usize> {
        let tx = conn
            .transaction()
            .map_err(|e| Error::Other(format!("begin replace_descriptions: {e}")))?;
        tx.execute(
            "DELETE FROM game_description WHERE system = ?1",
            params![system],
        )
        .map_err(|e| Error::Other(format!("clear game_description for {system}: {e}")))?;
        let mut count = 0usize;
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO game_description
                       (system, rom_filename, description, publisher)
                     VALUES (?1, ?2, ?3, ?4)",
                )
                .map_err(|e| Error::Other(format!("prepare insert game_description: {e}")))?;
            for (rom_filename, description, publisher) in rows {
                if description.is_none() && publisher.is_none() {
                    continue;
                }
                stmt.execute(params![system, rom_filename, description, publisher])
                    .map_err(|e| Error::Other(format!("insert game_description: {e}")))?;
                count += 1;
            }
        }
        tx.commit()
            .map_err(|e| Error::Other(format!("commit replace_descriptions: {e}")))?;
        Ok(count)
    }
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
    fn lookup_returns_none_when_unset() {
        let (conn, _dir) = open_temp();
        assert!(
            LibraryDb::lookup_description(&conn, "snes", "Mario.sfc")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn replace_then_lookup_roundtrips() {
        let (mut conn, _dir) = open_temp();
        LibraryDb::replace_descriptions_for_system(
            &mut conn,
            "snes",
            &[
                (
                    "Mario.sfc".into(),
                    Some("Plumber jumps".into()),
                    Some("Nintendo".into()),
                ),
                ("Zelda.sfc".into(), Some("Boy saves princess".into()), None),
                ("Empty.sfc".into(), None, None),
            ],
        )
        .unwrap();

        let mario = LibraryDb::lookup_description(&conn, "snes", "Mario.sfc")
            .unwrap()
            .unwrap();
        assert_eq!(mario.description.as_deref(), Some("Plumber jumps"));
        assert_eq!(mario.publisher.as_deref(), Some("Nintendo"));

        let zelda = LibraryDb::lookup_description(&conn, "snes", "Zelda.sfc")
            .unwrap()
            .unwrap();
        assert_eq!(zelda.description.as_deref(), Some("Boy saves princess"));
        assert_eq!(zelda.publisher, None);

        // Empty row was dropped.
        assert!(
            LibraryDb::lookup_description(&conn, "snes", "Empty.sfc")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn replace_wipes_prior_rows_for_same_system() {
        let (mut conn, _dir) = open_temp();
        LibraryDb::replace_descriptions_for_system(
            &mut conn,
            "snes",
            &[("Mario.sfc".into(), Some("first".into()), None)],
        )
        .unwrap();
        // Second call replaces.
        LibraryDb::replace_descriptions_for_system(
            &mut conn,
            "snes",
            &[("Zelda.sfc".into(), Some("second".into()), None)],
        )
        .unwrap();

        assert!(
            LibraryDb::lookup_description(&conn, "snes", "Mario.sfc")
                .unwrap()
                .is_none()
        );
        assert_eq!(
            LibraryDb::lookup_description(&conn, "snes", "Zelda.sfc")
                .unwrap()
                .unwrap()
                .description
                .as_deref(),
            Some("second")
        );
    }

    #[test]
    fn replace_does_not_touch_other_systems() {
        let (mut conn, _dir) = open_temp();
        LibraryDb::replace_descriptions_for_system(
            &mut conn,
            "snes",
            &[("Mario.sfc".into(), Some("snes desc".into()), None)],
        )
        .unwrap();
        LibraryDb::replace_descriptions_for_system(
            &mut conn,
            "nintendo_nes",
            &[("Mario.nes".into(), Some("nes desc".into()), None)],
        )
        .unwrap();
        // Re-replacing snes only wipes snes, not nes.
        LibraryDb::replace_descriptions_for_system(&mut conn, "snes", &[]).unwrap();
        assert!(
            LibraryDb::lookup_description(&conn, "snes", "Mario.sfc")
                .unwrap()
                .is_none()
        );
        assert_eq!(
            LibraryDb::lookup_description(&conn, "nintendo_nes", "Mario.nes")
                .unwrap()
                .unwrap()
                .description
                .as_deref(),
            Some("nes desc")
        );
    }
}
