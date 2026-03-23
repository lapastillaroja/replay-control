//! Operations on the `data_sources` and `thumbnail_index` tables.

use rusqlite::{Connection, OptionalExtension, params};

use crate::error::{Error, Result};

use super::{DataSourceInfo, DataSourceStats, MetadataDb, ThumbnailIndexEntry, unix_now};

impl MetadataDb {
    /// Insert or update a data source entry.
    pub fn upsert_data_source(
        conn: &Connection,
        source_name: &str,
        source_type: &str,
        version_hash: &str,
        branch: &str,
        entry_count: usize,
    ) -> Result<()> {
        let now = unix_now();
        conn.execute(
                "INSERT INTO data_sources (source_name, source_type, version_hash, imported_at, entry_count, branch)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(source_name) DO UPDATE SET
                    version_hash = excluded.version_hash,
                    imported_at = excluded.imported_at,
                    entry_count = excluded.entry_count,
                    branch = excluded.branch",
                params![source_name, source_type, version_hash, now, entry_count as i64, branch],
            )
            .map_err(|e| Error::Other(format!("Upsert data_source failed: {e}")))?;
        Ok(())
    }

    /// Look up a single data source.
    pub fn get_data_source(conn: &Connection, source_name: &str) -> Result<Option<DataSourceInfo>> {
        conn.query_row(
                "SELECT source_name, source_type, version_hash, imported_at, entry_count, branch
                 FROM data_sources WHERE source_name = ?1",
                params![source_name],
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
            .map_err(|e| Error::Other(format!("get_data_source failed: {e}")))
    }

    /// Get aggregate stats for a source type (e.g., "libretro-thumbnails").
    pub fn get_data_source_stats(conn: &Connection, source_type: &str) -> Result<DataSourceStats> {
        conn.query_row(
                "SELECT COUNT(*), COALESCE(SUM(entry_count), 0), MIN(imported_at)
                 FROM data_sources WHERE source_type = ?1",
                params![source_type],
                |row| {
                    Ok(DataSourceStats {
                        repo_count: row.get::<_, i64>(0)? as usize,
                        total_entries: row.get::<_, i64>(1)? as usize,
                        oldest_imported_at: row.get(2)?,
                    })
                },
            )
            .map_err(|e| Error::Other(format!("get_data_source_stats failed: {e:?}")))
    }

    /// Count total rows in the thumbnail_index table.
    pub fn thumbnail_index_count(conn: &Connection) -> Result<i64> {
        conn.query_row("SELECT COUNT(*) FROM thumbnail_index", [], |row| row.get(0))
            .map_err(|e| Error::Other(format!("thumbnail_index_count failed: {e}")))
    }

    // ── Thumbnail Index ─────────────────────────────────────────────

    /// Query thumbnail_index entries for a given repo and kind.
    pub fn query_thumbnail_index(
        conn: &Connection,
        repo_name: &str,
        kind: &str,
    ) -> Result<Vec<ThumbnailIndexEntry>> {
        let mut stmt = conn
            .prepare(
                "SELECT filename, symlink_target
                 FROM thumbnail_index
                 WHERE repo_name = ?1 AND kind = ?2",
            )
            .map_err(|e| Error::Other(format!("Prepare query_thumbnail_index: {e}")))?;

        let rows = stmt
            .query_map(params![repo_name, kind], |row| {
                Ok(ThumbnailIndexEntry {
                    filename: row.get(0)?,
                    symlink_target: row.get(1)?,
                })
            })
            .map_err(|e| Error::Other(format!("Query thumbnail_index: {e}")))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| Error::Other(format!("Row read failed: {e}")))?);
        }
        Ok(result)
    }

    /// Delete all thumbnail_index entries for a given repo.
    pub fn delete_thumbnail_index(conn: &Connection, repo_name: &str) -> Result<usize> {
        let count = conn
            .execute(
                "DELETE FROM thumbnail_index WHERE repo_name = ?1",
                params![repo_name],
            )
            .map_err(|e| Error::Other(format!("delete_thumbnail_index failed: {e}")))?;
        Ok(count)
    }

    /// Bulk insert thumbnail_index entries within a single transaction.
    /// Deletes existing entries for the repo first.
    pub fn bulk_insert_thumbnail_index(
        conn: &mut Connection,
        repo_name: &str,
        entries: &[(String, String, Option<String>)], // (kind, filename, symlink_target)
    ) -> Result<usize> {
        let tx = conn
            .transaction()
            .map_err(|e| Error::Other(format!("Transaction start failed: {e}")))?;

        // Delete existing entries for this repo.
        tx.execute(
            "DELETE FROM thumbnail_index WHERE repo_name = ?1",
            params![repo_name],
        )
        .map_err(|e| Error::Other(format!("Delete thumbnail_index failed: {e}")))?;

        let mut count = 0usize;
        {
            let mut stmt = tx
                .prepare(
                    "INSERT OR REPLACE INTO thumbnail_index
                     (repo_name, kind, filename, symlink_target)
                     VALUES (?1, ?2, ?3, ?4)",
                )
                .map_err(|e| Error::Other(format!("Prepare failed: {e}")))?;

            for (kind, filename, symlink_target) in entries {
                stmt.execute(params![repo_name, kind, filename, symlink_target])
                    .map_err(|e| Error::Other(format!("Insert thumbnail_index failed: {e}")))?;
                count += 1;
            }
        }

        tx.commit()
            .map_err(|e| Error::Other(format!("Transaction commit failed: {e}")))?;
        Ok(count)
    }

    /// Clear all thumbnail index entries and their data_sources rows.
    pub fn clear_thumbnail_index(conn: &Connection) -> Result<()> {
        conn.execute("DELETE FROM thumbnail_index", [])
            .map_err(|e| Error::Other(format!("Clear thumbnail_index failed: {e}")))?;
        conn.execute(
                "DELETE FROM data_sources WHERE source_type = 'libretro-thumbnails'",
                [],
            )
            .map_err(|e| Error::Other(format!("Clear libretro data_sources failed: {e}")))?;
        Ok(())
    }
}
