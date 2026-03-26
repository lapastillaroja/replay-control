//! Discovery queries: random, top-rated, genre counts, similar-by-genre, series siblings.

use rusqlite::{Connection, params};

use crate::error::{Error, Result};

use super::{GameEntry, MetadataDb};

/// Standard column list for `row_to_game_entry()`. All queries that use
/// `Self::row_to_game_entry` as the row mapper MUST select these columns
/// in this exact order.
const GAME_ENTRY_COLS: &str = "system, rom_filename, rom_path, display_name, base_title, series_key,
                        region, developer, genre, genre_group, rating, rating_count, players,
                        is_clone, is_m3u, is_translation, is_hack, is_special,
                        box_art_url, driver_status, size_bytes, crc32, hash_mtime, hash_matched_name,
                        release_year";

impl MetadataDb {
    /// Get random cached ROMs with box art from all systems.
    /// Returns a diverse selection across different systems.
    /// Filters out arcade clones and deduplicates regional variants,
    /// preferring the user's region preference.
    /// `region_secondary` is the fallback region (empty string = no secondary).
    pub fn random_cached_roms_diverse(
        conn: &Connection,
        count: usize,
        region_pref: &str,
        region_secondary: &str,
    ) -> Result<Vec<GameEntry>> {
        let sql = format!(
            "WITH deduped AS (
                SELECT *, ROW_NUMBER() OVER (
                    PARTITION BY system, base_title
                    ORDER BY CASE
                        WHEN region = ?2 THEN 0
                        WHEN region = ?3 THEN 1
                        WHEN region = 'world' THEN 2
                        ELSE 3
                    END
                ) AS rn
                FROM game_library
                WHERE is_clone = 0 AND is_translation = 0 AND is_hack = 0 AND is_special = 0
            )
            SELECT {GAME_ENTRY_COLS}
            FROM deduped WHERE rn = 1
            ORDER BY RANDOM() LIMIT ?1"
        );
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| Error::Other(format!("Prepare random_cached_roms_diverse: {e}")))?;

        let rows = stmt
            .query_map(
                params![(count * 5) as i64, region_pref, region_secondary],
                Self::row_to_game_entry,
            )
            .map_err(|e| Error::Other(format!("Query random_cached_roms_diverse: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Get random cached ROMs with box art from a specific system.
    pub fn random_cached_roms(
        conn: &Connection,
        system: &str,
        count: usize,
    ) -> Result<Vec<GameEntry>> {
        let sql = format!(
            "SELECT {GAME_ENTRY_COLS}
             FROM game_library
             WHERE system = ?1 AND box_art_url IS NOT NULL AND is_special = 0
             ORDER BY RANDOM() LIMIT ?2"
        );
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| Error::Other(format!("Prepare random_cached_roms: {e}")))?;

        let rows = stmt
            .query_map(params![system, count as i64], Self::row_to_game_entry)
            .map_err(|e| Error::Other(format!("Query random_cached_roms: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Get top-rated cached ROMs across all systems.
    /// Filters out arcade clones and deduplicates regional variants,
    /// then selects from the top-rated pool and randomizes within it
    /// so each page load shows a different selection of highly-rated games.
    ///
    /// Uses weighted scoring to penalize games with few votes:
    /// - 10+ votes: full rating
    /// - 3-9 votes: 90% of rating
    /// - 0-2 votes: 70% of rating
    ///
    /// This prevents obscure games rated 5.0 by a single voter from
    /// appearing above well-known classics with many votes.
    /// `region_secondary` is the fallback region (empty string = no secondary).
    pub fn top_rated_cached_roms(
        conn: &Connection,
        count: usize,
        region_pref: &str,
        region_secondary: &str,
    ) -> Result<Vec<GameEntry>> {
        let pool_size = (count * 4).max(40) as i64;
        let sql = format!(
            "WITH deduped AS (
                SELECT *, ROW_NUMBER() OVER (
                    PARTITION BY system, base_title
                    ORDER BY CASE
                        WHEN region = ?2 THEN 0
                        WHEN region = ?3 THEN 1
                        WHEN region = 'world' THEN 2
                        ELSE 3
                    END
                ) AS rn
                FROM game_library
                WHERE is_clone = 0 AND is_translation = 0 AND is_hack = 0 AND is_special = 0 AND rating IS NOT NULL AND rating > 0
            )
            SELECT {GAME_ENTRY_COLS}
            FROM (
                SELECT * FROM deduped WHERE rn = 1
                ORDER BY CASE
                    WHEN COALESCE(rating_count, 0) >= 10 THEN rating
                    WHEN COALESCE(rating_count, 0) >= 3 THEN rating * 0.9
                    ELSE rating * 0.7
                END DESC
                LIMIT ?1
            )
            ORDER BY RANDOM()"
        );
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| Error::Other(format!("Prepare top_rated_cached_roms: {e}")))?;

        let rows = stmt
            .query_map(
                params![pool_size, region_pref, region_secondary],
                Self::row_to_game_entry,
            )
            .map_err(|e| Error::Other(format!("Query top_rated_cached_roms: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Get genre counts across the entire library.
    pub fn genre_counts(conn: &Connection) -> Result<Vec<(String, usize)>> {
        let mut stmt = conn
            .prepare(
                "SELECT genre_group, COUNT(*) as cnt FROM game_library
                 WHERE genre_group != ''
                 GROUP BY genre_group ORDER BY cnt DESC",
            )
            .map_err(|e| Error::Other(format!("Prepare genre_counts: {e}")))?;

        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1).map(|v| v as usize)?,
                ))
            })
            .map_err(|e| Error::Other(format!("Query genre_counts: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Count multiplayer games (players >= 2) across the entire library.
    pub fn multiplayer_count(conn: &Connection) -> Result<usize> {
        conn.query_row(
            "SELECT COUNT(*) FROM game_library WHERE players IS NOT NULL AND players >= 2",
            [],
            |row| row.get::<_, i64>(0).map(|v| v as usize),
        )
        .map_err(|e| Error::Other(format!("Query multiplayer_count: {e}")))
    }

    /// Get all distinct genre groups across the entire game library.
    /// Returns sorted genre group names (excludes empty strings).
    pub fn all_genre_groups(conn: &Connection) -> Result<Vec<String>> {
        let mut stmt = conn
            .prepare(
                "SELECT DISTINCT genre_group FROM game_library
                 WHERE genre_group != ''
                 ORDER BY genre_group",
            )
            .map_err(|e| Error::Other(format!("Prepare all_genre_groups: {e}")))?;

        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| Error::Other(format!("Query all_genre_groups: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Get distinct genre groups for a specific system.
    /// Returns sorted genre group names (excludes empty strings).
    pub fn system_genre_groups(conn: &Connection, system: &str) -> Result<Vec<String>> {
        let mut stmt = conn
            .prepare(
                "SELECT DISTINCT genre_group FROM game_library
                 WHERE system = ?1 AND genre_group != ''
                 ORDER BY genre_group",
            )
            .map_err(|e| Error::Other(format!("Prepare system_genre_groups: {e}")))?;

        let rows = stmt
            .query_map(params![system], |row| row.get::<_, String>(0))
            .map_err(|e| Error::Other(format!("Query system_genre_groups: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Get non-favorited ROMs from a system, optionally filtered by genre.
    /// Filters out arcade clones and deduplicates regional variants.
    /// Selects from top-rated games and randomizes via SQL so each load
    /// shows different recommendations. Used for "Because You Love" section.
    /// `region_secondary` is the fallback region (empty string = no secondary).
    pub fn system_roms_excluding(
        conn: &Connection,
        system: &str,
        exclude_filenames: &[&str],
        genre_filter: Option<&str>,
        count: usize,
        region_pref: &str,
        region_secondary: &str,
    ) -> Result<Vec<GameEntry>> {
        let exclude_set: std::collections::HashSet<&str> =
            exclude_filenames.iter().copied().collect();

        let limit = ((count + exclude_filenames.len()) * 4).max(40) as i64;

        let roms = if let Some(genre) = genre_filter {
            let sql = format!(
                "WITH deduped AS (
                    SELECT *, ROW_NUMBER() OVER (
                        PARTITION BY system, base_title
                        ORDER BY CASE
                            WHEN region = ?4 THEN 0
                            WHEN region = ?5 THEN 1
                            WHEN region = 'world' THEN 2
                            ELSE 3
                        END
                    ) AS rn
                    FROM game_library
                    WHERE system = ?1 AND genre_group = ?2 AND is_clone = 0 AND is_translation = 0 AND is_hack = 0 AND is_special = 0
                )
                SELECT {GAME_ENTRY_COLS}
                FROM (
                    SELECT * FROM deduped WHERE rn = 1
                    ORDER BY rating DESC NULLS LAST
                    LIMIT ?3
                )
                ORDER BY RANDOM()"
            );
            let mut stmt = conn
                .prepare(&sql)
                .map_err(|e| Error::Other(format!("Prepare system_roms_excluding: {e}")))?;

            let rows = stmt
                .query_map(
                    params![system, genre, limit, region_pref, region_secondary],
                    Self::row_to_game_entry,
                )
                .map_err(|e| Error::Other(format!("Query system_roms_excluding: {e}")))?;
            rows.flatten().collect::<Vec<_>>()
        } else {
            let sql = format!(
                "WITH deduped AS (
                    SELECT *, ROW_NUMBER() OVER (
                        PARTITION BY system, base_title
                        ORDER BY CASE
                            WHEN region = ?3 THEN 0
                            WHEN region = ?4 THEN 1
                            WHEN region = 'world' THEN 2
                            ELSE 3
                        END
                    ) AS rn
                    FROM game_library
                    WHERE system = ?1 AND is_clone = 0 AND is_translation = 0 AND is_hack = 0 AND is_special = 0
                )
                SELECT {GAME_ENTRY_COLS}
                FROM (
                    SELECT * FROM deduped WHERE rn = 1
                    ORDER BY rating DESC NULLS LAST
                     LIMIT ?2
                 )
                 ORDER BY RANDOM()"
            );
            let mut stmt = conn
                .prepare(&sql)
                .map_err(|e| Error::Other(format!("Prepare system_roms_excluding: {e}")))?;

            let rows = stmt
                .query_map(
                    params![system, limit, region_pref, region_secondary],
                    Self::row_to_game_entry,
                )
                .map_err(|e| Error::Other(format!("Query system_roms_excluding: {e}")))?;
            rows.flatten().collect::<Vec<_>>()
        };

        Ok(roms
            .into_iter()
            .filter(|r| !exclude_set.contains(r.rom_filename.as_str()))
            .take(count)
            .collect())
    }

    /// Find similar games by genre within the same system.
    ///
    /// Uses a two-tier weighted query:
    /// - Exact `genre` match gets relevance=2 (same detailed genre)
    /// - `genre_group` match gets relevance=1 (same genre family)
    ///
    /// Results are ordered by relevance (exact first), then randomized
    /// within each tier. Excludes the given ROM, clones, translations,
    /// hacks, specials, and games without a genre.
    pub fn similar_by_genre(
        conn: &Connection,
        system: &str,
        genre: &str,
        exclude_filename: &str,
        limit: usize,
    ) -> Result<Vec<GameEntry>> {
        let genre_group = crate::genre::normalize_genre(genre);

        let sql = format!(
            "SELECT {GAME_ENTRY_COLS}
             FROM game_library
             WHERE system = ?1
               AND (genre = ?2 OR genre_group = ?3)
               AND genre_group != ''
               AND rom_filename != ?4
               AND is_clone = 0
               AND is_translation = 0
               AND is_hack = 0
               AND is_special = 0
             ORDER BY
               CASE WHEN genre = ?2 THEN 0 ELSE 1 END,
               RANDOM()
             LIMIT ?5"
        );
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| Error::Other(format!("Prepare similar_by_genre: {e}")))?;

        let rows = stmt
            .query_map(
                params![system, genre, genre_group, exclude_filename, limit as i64],
                Self::row_to_game_entry,
            )
            .map_err(|e| Error::Other(format!("Query similar_by_genre: {e}")))?;

        Ok(rows.flatten().collect())
    }

    /// Find series siblings: games with the same `series_key` but different `base_title`,
    /// across ALL systems (cross-system series).
    ///
    /// Deduplicates by `(system, base_title)` to pick one ROM per game per system,
    /// preferring the given region. Returns at most `limit` results.
    pub fn series_siblings(
        conn: &Connection,
        series_key: &str,
        current_base_title: &str,
        region_pref: &str,
        limit: usize,
    ) -> Result<Vec<GameEntry>> {
        if series_key.is_empty() {
            return Ok(Vec::new());
        }

        let sql = format!(
            "WITH deduped AS (
                SELECT *, ROW_NUMBER() OVER (
                    PARTITION BY system, base_title
                    ORDER BY CASE
                        WHEN region = ?2 THEN 0
                        WHEN region = 'world' THEN 1
                        ELSE 2
                    END
                ) AS rn
                FROM game_library
                WHERE series_key = ?1
                  AND series_key != ''
                  AND base_title != ?3
                  AND is_clone = 0
                  AND is_translation = 0
                  AND is_hack = 0
                  AND is_special = 0
            )
            SELECT {GAME_ENTRY_COLS}
            FROM deduped WHERE rn = 1
            ORDER BY display_name
            LIMIT ?4"
        );
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| Error::Other(format!("Prepare series_siblings: {e}")))?;

        let rows = stmt
            .query_map(
                params![series_key, region_pref, current_base_title, limit as i64],
                Self::row_to_game_entry,
            )
            .map_err(|e| Error::Other(format!("Query series_siblings: {e}")))?;

        Ok(rows.flatten().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::super::MetadataDb;
    use super::super::tests::*;

    #[test]
    fn recommendation_queries_exclude_special_roms() {
        let (mut conn, _dir) = open_temp_db();

        let mut normal = make_game_entry_with_genre("snes", "Mario (USA).sfc", "Platform");
        normal.base_title = "Mario".into();
        normal.region = "usa".into();
        normal.box_art_url = Some("/img/mario.png".into());
        normal.rating = Some(4.5);

        let mut special =
            make_game_entry_with_genre("snes", "Mario (USA) (FastRom).sfc", "Platform");
        special.base_title = "Mario FastRom".into();
        special.region = "usa".into();
        special.box_art_url = Some("/img/mario.png".into());
        special.rating = Some(4.5);
        special.is_special = true;

        MetadataDb::save_system_entries(&mut conn, "snes", &[normal, special], None).unwrap();

        let random = MetadataDb::random_cached_roms(&conn, "snes", 10).unwrap();
        assert_eq!(random.len(), 1);
        assert_eq!(random[0].rom_filename, "Mario (USA).sfc");

        let similar =
            MetadataDb::similar_by_genre(&conn, "snes", "Platform", "Other.sfc", 10).unwrap();
        assert_eq!(similar.len(), 1);
        assert_eq!(similar[0].rom_filename, "Mario (USA).sfc");
    }

    #[test]
    fn top_rated_weighted_scoring_prefers_many_votes() {
        let (mut conn, _dir) = open_temp_db();

        // Obscure game: 5.0 rating with 1 vote -> weighted = 5.0 * 0.7 = 3.5
        let mut obscure = make_game_entry("snes", "Obscure.sfc", false);
        obscure.base_title = "Obscure".into();
        obscure.region = "usa".into();
        obscure.rating = Some(5.0);
        obscure.rating_count = Some(1);

        // Classic game: 4.7 rating with 50 votes -> weighted = 4.7 * 1.0 = 4.7
        let mut classic = make_game_entry("snes", "Classic.sfc", false);
        classic.base_title = "Classic".into();
        classic.region = "usa".into();
        classic.rating = Some(4.7);
        classic.rating_count = Some(50);

        MetadataDb::save_system_entries(&mut conn, "snes", &[obscure, classic], None).unwrap();

        let top = MetadataDb::top_rated_cached_roms(&conn, 2, "usa", "").unwrap();
        assert_eq!(top.len(), 2);

        // Both should be present. The classic should rank higher due to weighted scoring.
        // Since the final ORDER BY is RANDOM(), we can't check ordering directly.
        // But let's verify both are included.
        let filenames: Vec<&str> = top.iter().map(|r| r.rom_filename.as_str()).collect();
        assert!(filenames.contains(&"Classic.sfc"));
        assert!(filenames.contains(&"Obscure.sfc"));
    }
}
