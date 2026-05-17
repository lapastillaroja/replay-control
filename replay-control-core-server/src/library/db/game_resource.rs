//! Operations on the `library_game_resource` table.

use rusqlite::{Connection, params};

use replay_control_core::error::{Error, Result};

use super::{LibraryDb, LibraryGameResource};

impl LibraryDb {
    pub fn begin_detail_resource_stage(conn: &mut Connection, system: &str) -> Result<i64> {
        let tx = conn
            .transaction()
            .map_err(|e| Error::Other(format!("begin detail/resource stage: {e}")))?;
        tx.execute(
            "INSERT OR IGNORE INTO library_build_sequence (name, next_value)
             VALUES ('detail_resource_stage_token', 1)",
            [],
        )
        .map_err(|e| Error::Other(format!("initialize detail/resource stage token: {e}")))?;
        let token = tx
            .query_row(
                "SELECT next_value
                 FROM library_build_sequence
                 WHERE name = 'detail_resource_stage_token'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|e| Error::Other(format!("read detail/resource stage token: {e}")))?;
        tx.execute(
            "UPDATE library_build_sequence
             SET next_value = next_value + 1
             WHERE name = 'detail_resource_stage_token'",
            [],
        )
        .map_err(|e| Error::Other(format!("advance detail/resource stage token: {e}")))?;
        tx.execute(
            "DELETE FROM game_detail_metadata_stage WHERE system = ?1",
            params![system],
        )
        .map_err(|e| Error::Other(format!("clear old detail stage for {system}: {e}")))?;
        tx.execute(
            "DELETE FROM library_game_resource_stage WHERE system = ?1",
            params![system],
        )
        .map_err(|e| Error::Other(format!("clear old resource stage for {system}: {e}")))?;
        tx.commit()
            .map_err(|e| Error::Other(format!("commit begin detail/resource stage: {e}")))?;
        Ok(token)
    }

    pub fn insert_detail_metadata_stage_chunk(
        conn: &mut Connection,
        system: &str,
        stage_token: i64,
        rows: &[(String, Option<String>, Option<String>)],
    ) -> Result<usize> {
        if rows.is_empty() {
            return Ok(0);
        }
        let tx = conn
            .transaction()
            .map_err(|e| Error::Other(format!("begin detail stage chunk: {e}")))?;
        let mut count = 0usize;
        {
            let mut stmt = tx
                .prepare(
                    "INSERT OR REPLACE INTO game_detail_metadata_stage
                       (system, stage_token, rom_filename, description, publisher)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                )
                .map_err(|e| Error::Other(format!("prepare insert detail stage: {e}")))?;
            for (rom_filename, description, publisher) in rows {
                if description.is_none() && publisher.is_none() {
                    continue;
                }
                stmt.execute(params![
                    system,
                    stage_token,
                    rom_filename,
                    description,
                    publisher
                ])
                .map_err(|e| Error::Other(format!("insert detail stage: {e}")))?;
                count += 1;
            }
        }
        tx.commit()
            .map_err(|e| Error::Other(format!("commit detail stage chunk: {e}")))?;
        Ok(count)
    }

    pub fn insert_library_game_resource_stage_chunk(
        conn: &mut Connection,
        system: &str,
        stage_token: i64,
        rows: &[LibraryGameResource],
    ) -> Result<usize> {
        if rows.is_empty() {
            return Ok(0);
        }
        let tx = conn
            .transaction()
            .map_err(|e| Error::Other(format!("begin resource stage chunk: {e}")))?;
        let mut count = 0usize;
        {
            let mut stmt = tx
                .prepare(
                    "INSERT OR REPLACE INTO library_game_resource_stage
                       (system, stage_token, rom_filename, source, resource_type,
                        resource_id, url, title, languages, platform, mime_type)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                )
                .map_err(|e| Error::Other(format!("prepare insert resource stage: {e}")))?;
            for row in rows {
                stmt.execute(params![
                    system,
                    stage_token,
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
                .map_err(|e| Error::Other(format!("insert resource stage: {e}")))?;
                count += 1;
            }
        }
        tx.commit()
            .map_err(|e| Error::Other(format!("commit resource stage chunk: {e}")))?;
        Ok(count)
    }

    pub fn finalize_detail_resource_stage(
        conn: &mut Connection,
        system: &str,
        stage_token: i64,
    ) -> Result<(usize, usize)> {
        let tx = conn
            .transaction()
            .map_err(|e| Error::Other(format!("begin finalize detail/resource stage: {e}")))?;
        tx.execute(
            "DELETE FROM game_detail_metadata WHERE system = ?1",
            params![system],
        )
        .map_err(|e| Error::Other(format!("clear live detail metadata for {system}: {e}")))?;
        let desc_count = tx
            .execute(
                "INSERT INTO game_detail_metadata
                   (system, rom_filename, description, publisher)
                 SELECT system, rom_filename, description, publisher
                 FROM game_detail_metadata_stage
                 WHERE system = ?1 AND stage_token = ?2",
                params![system, stage_token],
            )
            .map_err(|e| Error::Other(format!("publish staged detail metadata: {e}")))?;
        tx.execute(
            "DELETE FROM library_game_resource WHERE system = ?1",
            params![system],
        )
        .map_err(|e| Error::Other(format!("clear live resources for {system}: {e}")))?;
        let resource_count = tx
            .execute(
                "INSERT OR REPLACE INTO library_game_resource
                   (system, rom_filename, source, resource_type, resource_id,
                    url, title, languages, platform, mime_type)
                 SELECT system, rom_filename, source, resource_type, resource_id,
                        url, title, languages, platform, mime_type
                 FROM library_game_resource_stage
                 WHERE system = ?1 AND stage_token = ?2",
                params![system, stage_token],
            )
            .map_err(|e| Error::Other(format!("publish staged resources: {e}")))?;
        tx.execute(
            "DELETE FROM game_detail_metadata_stage WHERE system = ?1",
            params![system],
        )
        .map_err(|e| Error::Other(format!("clear detail stage for {system}: {e}")))?;
        tx.execute(
            "DELETE FROM library_game_resource_stage WHERE system = ?1",
            params![system],
        )
        .map_err(|e| Error::Other(format!("clear resource stage for {system}: {e}")))?;
        tx.commit()
            .map_err(|e| Error::Other(format!("commit finalize detail/resource stage: {e}")))?;
        Ok((desc_count, resource_count))
    }

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

#[cfg(test)]
mod tests {
    use super::super::tests::{make_game_entry, open_temp_db};
    use super::*;

    fn manual_resource(rom_filename: &str, url: &str) -> LibraryGameResource {
        LibraryGameResource {
            rom_filename: rom_filename.to_string(),
            source: "test".to_string(),
            resource_type: "manual".to_string(),
            resource_id: url.to_string(),
            url: url.to_string(),
            title: Some("Manual".to_string()),
            languages: Some("en".to_string()),
            platform: None,
            mime_type: Some("application/pdf".to_string()),
        }
    }

    #[test]
    fn staged_detail_resource_publish_preserves_live_rows_until_finalize() {
        let (mut conn, _dir) = open_temp_db();
        LibraryDb::save_system_entries(
            &mut conn,
            "snes",
            &[make_game_entry("snes", "Mario.sfc", false)],
            None,
        )
        .unwrap();
        LibraryDb::replace_detail_metadata_and_resources_for_system(
            &mut conn,
            "snes",
            &[(
                "Mario.sfc".to_string(),
                Some("old description".to_string()),
                Some("old publisher".to_string()),
            )],
            &[manual_resource("Mario.sfc", "https://example.test/old.pdf")],
        )
        .unwrap();

        let token = LibraryDb::begin_detail_resource_stage(&mut conn, "snes").unwrap();
        LibraryDb::insert_detail_metadata_stage_chunk(
            &mut conn,
            "snes",
            token,
            &[(
                "Mario.sfc".to_string(),
                Some("new description".to_string()),
                Some("new publisher".to_string()),
            )],
        )
        .unwrap();
        LibraryDb::insert_library_game_resource_stage_chunk(
            &mut conn,
            "snes",
            token,
            &[manual_resource("Mario.sfc", "https://example.test/new.pdf")],
        )
        .unwrap();

        let description = LibraryDb::lookup_description(&conn, "snes", "Mario.sfc")
            .unwrap()
            .unwrap();
        assert_eq!(description.description.as_deref(), Some("old description"));
        let resources = LibraryDb::game_resources(&conn, "snes", "Mario.sfc", "manual").unwrap();
        assert_eq!(resources[0].url, "https://example.test/old.pdf");

        let (detail_count, resource_count) =
            LibraryDb::finalize_detail_resource_stage(&mut conn, "snes", token).unwrap();
        assert_eq!(detail_count, 1);
        assert_eq!(resource_count, 1);

        let description = LibraryDb::lookup_description(&conn, "snes", "Mario.sfc")
            .unwrap()
            .unwrap();
        assert_eq!(description.description.as_deref(), Some("new description"));
        let resources = LibraryDb::game_resources(&conn, "snes", "Mario.sfc", "manual").unwrap();
        assert_eq!(resources[0].url, "https://example.test/new.pdf");
    }
}
