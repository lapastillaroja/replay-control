//! Operations on the `library_game_resource` table.

use rusqlite::{Connection, params};

use replay_control_core::error::{Error, Result};

use super::{LibraryDb, LibraryGameResource};

impl LibraryDb {
    /// Read derived resources for one ROM and type, ordered by provenance/title.
    pub fn game_resources(
        conn: &Connection,
        system: &str,
        rom_filename: &str,
        resource_type: &str,
    ) -> Result<Vec<LibraryGameResource>> {
        let mut stmt = conn
            .prepare(
                "SELECT source, resource_type, resource_id, url, title, languages, platform, mime_type
                 FROM library_game_resource
                 WHERE system = ?1 AND rom_filename = ?2 AND resource_type = ?3
                 ORDER BY source, COALESCE(title, ''), url",
            )
            .map_err(|e| Error::Other(format!("prepare game_resources: {e}")))?;
        let rows = stmt
            .query_map(params![system, rom_filename, resource_type], |row| {
                Ok(LibraryGameResource {
                    rom_filename: rom_filename.to_string(),
                    source: row.get(0)?,
                    resource_type: row.get(1)?,
                    resource_id: row.get(2)?,
                    url: row.get(3)?,
                    title: row.get(4)?,
                    languages: row.get(5)?,
                    platform: row.get(6)?,
                    mime_type: row.get(7)?,
                })
            })
            .map_err(|e| Error::Other(format!("query game_resources: {e}")))?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| Error::Other(format!("game_resource row: {e}")))?);
        }
        Ok(out)
    }

    /// Replace derived resources for `system`. Caller owns surrounding transaction
    /// when this must be atomic with other per-system enrichment writes.
    pub fn replace_resources_for_system_in_tx(
        conn: &Connection,
        system: &str,
        rows: &[LibraryGameResource],
    ) -> Result<usize> {
        conn.execute(
            "DELETE FROM library_game_resource WHERE system = ?1",
            params![system],
        )
        .map_err(|e| Error::Other(format!("clear library_game_resource for {system}: {e}")))?;
        let mut stmt = conn
            .prepare(
                "INSERT OR REPLACE INTO library_game_resource
                   (system, rom_filename, source, resource_type, resource_id,
                    url, title, languages, platform, mime_type)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )
            .map_err(|e| Error::Other(format!("prepare insert library_game_resource: {e}")))?;
        let mut count = 0usize;
        for row in rows {
            stmt.execute(params![
                system,
                row.rom_filename,
                row.source,
                row.resource_type,
                row.resource_id,
                row.url,
                row.title,
                row.languages,
                row.platform,
                row.mime_type,
            ])
            .map_err(|e| Error::Other(format!("insert library_game_resource: {e}")))?;
            count += 1;
        }
        Ok(count)
    }

    /// Replace detail metadata and derived resources for `system` atomically.
    pub fn replace_detail_metadata_and_resources_for_system(
        conn: &mut Connection,
        system: &str,
        description_rows: &[(String, Option<String>, Option<String>)],
        resource_rows: &[LibraryGameResource],
    ) -> Result<(usize, usize)> {
        let tx = conn
            .transaction()
            .map_err(|e| Error::Other(format!("begin replace detail/resources: {e}")))?;
        tx.execute(
            "DELETE FROM game_detail_metadata WHERE system = ?1",
            params![system],
        )
        .map_err(|e| Error::Other(format!("clear game_detail_metadata for {system}: {e}")))?;
        let desc_count = {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO game_detail_metadata
                       (system, rom_filename, description, publisher)
                     VALUES (?1, ?2, ?3, ?4)",
                )
                .map_err(|e| Error::Other(format!("prepare insert game_detail_metadata: {e}")))?;
            let mut count = 0usize;
            for (rom_filename, description, publisher) in description_rows {
                if description.is_none() && publisher.is_none() {
                    continue;
                }
                stmt.execute(params![system, rom_filename, description, publisher])
                    .map_err(|e| Error::Other(format!("insert game_detail_metadata: {e}")))?;
                count += 1;
            }
            count
        };
        let resource_count = Self::replace_resources_for_system_in_tx(&tx, system, resource_rows)?;
        tx.commit()
            .map_err(|e| Error::Other(format!("commit replace detail/resources: {e}")))?;
        Ok((desc_count, resource_count))
    }
}
