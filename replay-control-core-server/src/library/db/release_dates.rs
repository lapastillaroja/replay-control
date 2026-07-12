//! Operations on the `game_release_date` table (multi-region, full-precision release dates).
//!
//! This table stores one row per `(system, base_title, region)` — across all
//! regions TGDB / LaunchBox / MAME / TOSEC found for that game. The resolver
//! picks the best date per the user's region preference and mirrors it into
//! `game_library` at scan time.

use std::collections::{HashMap, HashSet};

use rusqlite::{Connection, params};

use crate::{arcade_db, game_db};
use replay_control_core::error::{Error, Result};
use replay_control_core::rom_tags::RegionPreference;
use replay_control_core::{systems, title_utils};

use super::{DatePrecision, DpSql, LibraryDb};

// Rank every game_release_date row once via ROW_NUMBER(), then assign each
// game_library row the rank-1 match via a row-value UPDATE. If no match exists
// in `best`, the subquery yields (NULL, NULL, NULL), which clears stale mirror
// values without a separate NULL-out pass.
const GLOBAL_RESOLVE_RELEASE_DATE_SQL: &str = "\
    WITH best AS ( \
        SELECT system, base_title, release_date, precision, region FROM ( \
            SELECT grd.*, ROW_NUMBER() OVER ( \
                PARTITION BY grd.system, grd.base_title \
                ORDER BY \
                  CASE \
                    WHEN grd.region = ?1 THEN 1 \
                    WHEN ?2 IS NOT NULL AND grd.region = ?2 THEN 2 \
                    WHEN grd.region = 'world' THEN 3 \
                    WHEN grd.region = 'unknown' THEN 5 \
                    ELSE 4 \
                  END, \
                  CASE grd.precision WHEN 'day' THEN 1 WHEN 'month' THEN 2 ELSE 3 END \
            ) AS rn \
            FROM game_release_date grd \
        ) \
        WHERE rn = 1 \
    ) \
    UPDATE game_library AS gl \
    SET (release_date, release_precision, release_region_used) = ( \
        SELECT release_date, precision, region FROM best \
        WHERE best.system = gl.system AND best.base_title = gl.base_title \
    ) \
    WHERE gl.base_title != ''";

const SCOPED_RESOLVE_RELEASE_DATE_SQL: &str = "\
    WITH best AS ( \
        SELECT system, base_title, release_date, precision, region FROM ( \
            SELECT grd.*, ROW_NUMBER() OVER ( \
                PARTITION BY grd.system, grd.base_title \
                ORDER BY \
                  CASE \
                    WHEN grd.region = ?1 THEN 1 \
                    WHEN ?2 IS NOT NULL AND grd.region = ?2 THEN 2 \
                    WHEN grd.region = 'world' THEN 3 \
                    WHEN grd.region = 'unknown' THEN 5 \
                    ELSE 4 \
                  END, \
                  CASE grd.precision WHEN 'day' THEN 1 WHEN 'month' THEN 2 ELSE 3 END \
            ) AS rn \
            FROM game_release_date grd \
            WHERE grd.system = ?3 \
        ) \
        WHERE rn = 1 \
    ) \
    UPDATE game_library AS gl \
    SET (release_date, release_precision, release_region_used) = ( \
        SELECT release_date, precision, region FROM best \
        WHERE best.system = gl.system AND best.base_title = gl.base_title \
    ) \
    WHERE gl.system = ?3 AND gl.base_title != ''";

/// A row to insert into `game_release_date`.
#[derive(Debug, Clone)]
pub struct ReleaseDateRow {
    pub system: String,
    pub base_title: String,
    pub region: String,
    pub release_date: String,
    pub precision: DatePrecision,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseDateMirrorUpdate {
    pub rom_filename: String,
    pub release_date: Option<String>,
    pub precision: Option<DatePrecision>,
    pub region: Option<String>,
}

/// Map the user's `RegionPreference` to the corresponding `game_release_date.region` string.
pub fn region_pref_to_db_region(pref: RegionPreference) -> &'static str {
    match pref {
        RegionPreference::Usa => "usa",
        RegionPreference::Europe => "europe",
        RegionPreference::Japan => "japan",
        RegionPreference::World => "world",
    }
}

/// Pre-fetched catalog data for `seed_release_dates_from_static`.
///
/// Gathered asynchronously before entering the sync DB write closure.
#[derive(Default)]
pub struct StaticReleaseData {
    console_rows: Vec<(String, String, String, String, String, String)>,
    arcade_rows: Vec<(String, String, String)>,
    arcade_rom_to_display: HashMap<String, String>,
}

/// Fetch static release date data from the catalog asynchronously.
pub async fn fetch_static_release_data() -> StaticReleaseData {
    let console_rows = game_db::console_release_dates().await;
    let arcade_rows = arcade_db::arcade_release_dates().await;
    let rom_refs: Vec<&str> = arcade_rows.iter().map(|(r, _, _)| r.as_str()).collect();
    // System-agnostic context — empty system uses default priority order.
    let arcade_info = arcade_db::lookup_arcade_games_batch("", &rom_refs).await;
    let arcade_rom_to_display: HashMap<String, String> = arcade_info
        .into_iter()
        .map(|(rom, info)| (rom, info.display_name))
        .collect();
    StaticReleaseData {
        console_rows,
        arcade_rows,
        arcade_rom_to_display,
    }
}

impl LibraryDb {
    const RELEASE_DATE_BATCH_ROWS: usize = 5_000;

    /// Bulk insert release-date rows. Overwrites existing `(system, base_title, region)` entries
    /// with `source = <new>` iff the new precision is strictly higher than the existing one
    /// (year < month < day). Lower or equal precision is rejected.
    ///
    /// This is the "precision-upgrade" write path used by both build-time embedded data
    /// (year-precision from arcade DAT/TOSEC) and runtime LaunchBox enrichment (day-precision).
    /// Rows are committed in bounded chunks because the upsert is idempotent and
    /// repeatable by the next enrichment/import pass if interrupted.
    pub fn upsert_release_dates(conn: &mut Connection, rows: &[ReleaseDateRow]) -> Result<usize> {
        if rows.is_empty() {
            return Ok(0);
        }

        Self::upsert_release_dates_with_batch(conn, rows, Self::RELEASE_DATE_BATCH_ROWS)
    }

    fn upsert_release_dates_with_batch(
        conn: &mut Connection,
        rows: &[ReleaseDateRow],
        batch_rows: usize,
    ) -> Result<usize> {
        if rows.is_empty() {
            return Ok(0);
        }
        let batch_rows = batch_rows.max(1);
        let mut count = 0usize;
        for chunk in rows.chunks(batch_rows) {
            let tx = conn
                .transaction()
                .map_err(|e| Error::Other(format!("Transaction start: {e}")))?;
            {
                let sql = "INSERT INTO game_release_date (system, base_title, region, release_date, precision, source) \
                           VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
                           ON CONFLICT(system, base_title, region) DO UPDATE SET \
                               release_date = excluded.release_date, \
                               precision    = excluded.precision, \
                               source       = excluded.source \
                           WHERE \
                               CASE excluded.precision WHEN 'day' THEN 3 WHEN 'month' THEN 2 ELSE 1 END \
                             > CASE game_release_date.precision WHEN 'day' THEN 3 WHEN 'month' THEN 2 ELSE 1 END";
                let mut stmt = tx
                    .prepare(sql)
                    .map_err(|e| Error::Other(format!("Prepare upsert_release_dates: {e}")))?;
                for r in chunk {
                    stmt.execute(params![
                        r.system,
                        r.base_title,
                        r.region,
                        r.release_date,
                        DpSql(r.precision),
                        r.source,
                    ])
                    .map_err(|e| Error::Other(format!("Upsert release_date: {e}")))?;
                    count += 1;
                }
            }
            tx.commit()
                .map_err(|e| Error::Other(format!("Transaction commit: {e}")))?;
        }
        Ok(count)
    }

    /// Populate `game_release_date` from build-time embedded static data.
    ///
    /// Reads `game_db::console_release_dates()` (per-region TGDB-sourced rows)
    /// and `arcade_db::arcade_release_dates()` (arcade MAME/FBNeo/Naomi year
    /// rows), filtering to `(system, base_title)` pairs that actually appear
    /// in `game_library`. Upserts each matching row via the precision-upgrade
    /// write path (`upsert_release_dates`).
    ///
    /// Safe to call at every scan: `upsert_release_dates` rejects downgrades,
    /// so a subsequent LaunchBox day-precision row still wins over our
    /// year-precision TGDB entry for the same `(system, base_title, region)`.
    ///
    /// Returns the count of rows upserted (subject to the precision-upgrade
    /// filter — rows that couldn't upgrade are still counted here because
    /// SQLite's `execute` returns 1 even for no-op upserts).
    pub fn seed_release_dates_from_static(
        conn: &mut Connection,
        data: StaticReleaseData,
    ) -> Result<usize> {
        let library_titles = load_library_titles(conn, None)?;
        let rows = static_release_rows(data, &library_titles);
        Self::upsert_release_dates(conn, &rows)
    }

    /// Build static release-date rows that apply to one system's current library rows.
    pub fn static_release_date_rows_for_system(
        conn: &Connection,
        system: &str,
        data: StaticReleaseData,
    ) -> Result<Vec<ReleaseDateRow>> {
        let library_titles = load_library_titles(conn, Some(system))?;
        Ok(static_release_rows(data, &library_titles))
    }

    /// Populate `game_release_date` from `game_library` rows' current mirror columns.
    ///
    /// Reads `(system, base_title, region, release_date, release_precision)` tuples
    /// from `game_library` for ROMs with a non-null `release_date` and writes them
    /// into `game_release_date` with `source='builder'` (or `source` as provided).
    ///
    /// Used at scan-time to seed `game_release_date` with build-time data (embedded
    /// TGDB dates, arcade DAT years) that the builder wrote into `game_library`.
    /// Safe to call repeatedly — the upsert's precision-upgrade rule ensures we
    /// never downgrade an existing higher-precision entry.
    pub fn seed_release_dates_from_library(conn: &mut Connection, source: &str) -> Result<usize> {
        Self::seed_release_dates_from_library_scope(conn, None, source)
    }

    fn seed_release_dates_from_library_scope(
        conn: &mut Connection,
        system: Option<&str>,
        source: &str,
    ) -> Result<usize> {
        let rows = Self::library_release_date_rows_scope(conn, system, source)?;
        Self::upsert_release_dates(conn, &rows)
    }

    /// Build release-date rows from one system's current `game_library` mirror columns.
    pub fn library_release_date_rows_for_system(
        conn: &Connection,
        system: &str,
        source: &str,
    ) -> Result<Vec<ReleaseDateRow>> {
        Self::library_release_date_rows_scope(conn, Some(system), source)
    }

    fn library_release_date_rows_scope(
        conn: &Connection,
        system: Option<&str>,
        source: &str,
    ) -> Result<Vec<ReleaseDateRow>> {
        let mut rows = Vec::new();
        if let Some(system) = system {
            let mut stmt = conn
                .prepare(
                    "SELECT DISTINCT system, base_title,
                            CASE WHEN region = '' THEN 'unknown' ELSE region END,
                            release_date, release_precision
                     FROM game_library
                     WHERE system = ?1
                       AND base_title != ''
                       AND release_date IS NOT NULL",
                )
                .map_err(|e| {
                    Error::Other(format!("Prepare seed_release_dates_from_library: {e}"))
                })?;
            let mapped = stmt
                .query_map(params![system], release_date_row(source))
                .map_err(|e| Error::Other(format!("Query seed_release_dates_from_library: {e}")))?;
            for row in mapped {
                rows.push(row.map_err(|e| {
                    Error::Other(format!("Read seed_release_dates_from_library row: {e}"))
                })?);
            }
        } else {
            let mut stmt = conn
                .prepare(
                    "SELECT DISTINCT system, base_title,
                            CASE WHEN region = '' THEN 'unknown' ELSE region END,
                            release_date, release_precision
                     FROM game_library
                     WHERE base_title != ''
                       AND release_date IS NOT NULL",
                )
                .map_err(|e| {
                    Error::Other(format!("Prepare seed_release_dates_from_library: {e}"))
                })?;
            let mapped = stmt
                .query_map([], release_date_row(source))
                .map_err(|e| Error::Other(format!("Query seed_release_dates_from_library: {e}")))?;
            for row in mapped {
                rows.push(row.map_err(|e| {
                    Error::Other(format!("Read seed_release_dates_from_library row: {e}"))
                })?);
            }
        }

        Ok(rows)
    }

    /// Resolve each `game_library` row's best `release_date` from `game_release_date`
    /// according to the user's region preference (primary + optional secondary).
    ///
    /// For each `(system, base_title)` pair present in `game_library`, pick one row
    /// from `game_release_date`. Priority: user primary region → user secondary →
    /// 'world' → other regions → 'unknown'. Within the same region tier, higher
    /// precision wins.
    ///
    /// Writes `release_date`, `release_precision`, and `release_region_used` into
    /// `game_library`. Rows with no match get NULL in all three columns.
    pub fn resolve_release_date_for_library(
        conn: &mut Connection,
        primary: RegionPreference,
        secondary: Option<RegionPreference>,
    ) -> Result<usize> {
        Self::resolve_release_date(conn, None, primary, secondary)
    }

    /// Resolve one system's `game_library` release-date mirror columns from
    /// `game_release_date`.
    pub fn resolve_release_date_for_system(
        conn: &mut Connection,
        system: &str,
        primary: RegionPreference,
        secondary: Option<RegionPreference>,
    ) -> Result<usize> {
        Self::resolve_release_date(conn, Some(system), primary, secondary)
    }

    pub fn resolved_release_date_mirrors_for_system(
        conn: &Connection,
        system: &str,
        primary: RegionPreference,
        secondary: Option<RegionPreference>,
    ) -> Result<Vec<ReleaseDateMirrorUpdate>> {
        #[derive(Clone)]
        struct BestReleaseDate {
            release_date: String,
            precision: DatePrecision,
            region: String,
            rank: (u8, u8),
        }

        fn region_rank(region: &str, primary: &str, secondary: Option<&str>) -> u8 {
            if region == primary {
                1
            } else if secondary.is_some_and(|secondary| region == secondary) {
                2
            } else if region == "world" {
                3
            } else if region == "unknown" {
                5
            } else {
                4
            }
        }

        let primary_region = region_pref_to_db_region(primary);
        let secondary_region = secondary.map(region_pref_to_db_region);
        let mut best_by_title: HashMap<String, BestReleaseDate> = HashMap::new();
        let mut stmt = conn
            .prepare(
                "SELECT base_title, region, release_date, precision
                 FROM game_release_date
                 WHERE system = ?1",
            )
            .map_err(|e| Error::Other(format!("Prepare release-date mirror source: {e}")))?;
        let rows = stmt
            .query_map(params![system], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, DpSql>(3)?.0,
                ))
            })
            .map_err(|e| Error::Other(format!("Query release-date mirror source: {e}")))?;
        for row in rows {
            let (base_title, region, release_date, precision) =
                row.map_err(|e| Error::Other(format!("Read release-date mirror source: {e}")))?;
            let rank = (
                region_rank(&region, primary_region, secondary_region),
                4 - precision.rank(),
            );
            let candidate = BestReleaseDate {
                release_date,
                precision,
                region,
                rank,
            };
            match best_by_title.get(&base_title) {
                Some(current) if current.rank <= rank => {}
                _ => {
                    best_by_title.insert(base_title, candidate);
                }
            }
        }

        let mut stmt = conn
            .prepare(
                "SELECT rom_filename, base_title
                 FROM game_library
                 WHERE system = ?1 AND base_title != ''
                 ORDER BY rom_filename",
            )
            .map_err(|e| Error::Other(format!("Prepare release-date mirror targets: {e}")))?;
        let rows = stmt
            .query_map(params![system], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| Error::Other(format!("Query release-date mirror targets: {e}")))?;
        let mut updates = Vec::new();
        for row in rows {
            let (rom_filename, base_title) =
                row.map_err(|e| Error::Other(format!("Read release-date mirror target: {e}")))?;
            if let Some(best) = best_by_title.get(&base_title) {
                updates.push(ReleaseDateMirrorUpdate {
                    rom_filename,
                    release_date: Some(best.release_date.clone()),
                    precision: Some(best.precision),
                    region: Some(best.region.clone()),
                });
            } else {
                updates.push(ReleaseDateMirrorUpdate {
                    rom_filename,
                    release_date: None,
                    precision: None,
                    region: None,
                });
            }
        }
        Ok(updates)
    }

    pub fn update_release_date_mirrors(
        conn: &mut Connection,
        system: &str,
        updates: &[ReleaseDateMirrorUpdate],
    ) -> Result<usize> {
        if updates.is_empty() {
            return Ok(0);
        }
        let tx = conn
            .transaction()
            .map_err(|e| Error::Other(format!("Transaction start: {e}")))?;
        let mut count = 0usize;
        {
            let mut stmt = tx
                .prepare(
                    "UPDATE game_library
                     SET release_date = ?2,
                         release_precision = ?3,
                         release_region_used = ?4
                     WHERE system = ?5 AND rom_filename = ?1",
                )
                .map_err(|e| Error::Other(format!("Prepare release-date mirror update: {e}")))?;
            for update in updates {
                count += stmt
                    .execute(params![
                        update.rom_filename,
                        update.release_date,
                        update.precision.map(DpSql),
                        update.region,
                        system,
                    ])
                    .map_err(|e| Error::Other(format!("Update release-date mirror: {e}")))?;
            }
        }
        tx.commit()
            .map_err(|e| Error::Other(format!("Transaction commit: {e}")))?;
        Ok(count)
    }

    fn resolve_release_date(
        conn: &mut Connection,
        system: Option<&str>,
        primary: RegionPreference,
        secondary: Option<RegionPreference>,
    ) -> Result<usize> {
        let primary_region = region_pref_to_db_region(primary);
        let secondary_region = secondary.map(region_pref_to_db_region);

        let tx = conn
            .transaction()
            .map_err(|e| Error::Other(format!("Transaction start: {e}")))?;

        let affected = if let Some(system) = system {
            tx.execute(
                SCOPED_RESOLVE_RELEASE_DATE_SQL,
                params![primary_region, secondary_region, system],
            )
        } else {
            tx.execute(
                GLOBAL_RESOLVE_RELEASE_DATE_SQL,
                params![primary_region, secondary_region],
            )
        }
        .map_err(|e| Error::Other(format!("resolve_release_date: {e}")))?;

        tx.commit()
            .map_err(|e| Error::Other(format!("Transaction commit: {e}")))?;
        Ok(affected)
    }
}

fn release_date_row<'a>(
    source: &'a str,
) -> impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<ReleaseDateRow> + 'a {
    move |row| {
        Ok(ReleaseDateRow {
            system: row.get(0)?,
            base_title: row.get(1)?,
            region: row.get(2)?,
            release_date: row.get(3)?,
            precision: row
                .get::<_, Option<DpSql>>(4)?
                .map(|DpSql(d)| d)
                .unwrap_or(DatePrecision::Year),
            source: source.to_string(),
        })
    }
}

fn load_library_titles(
    conn: &Connection,
    system: Option<&str>,
) -> Result<HashMap<String, HashSet<String>>> {
    let mut titles_by_system: HashMap<String, HashSet<String>> = HashMap::new();
    if let Some(system) = system {
        let mut stmt = conn
            .prepare(
                "SELECT DISTINCT system, base_title
                 FROM game_library
                 WHERE system = ?1 AND base_title != ''",
            )
            .map_err(|e| Error::Other(format!("Prepare library release-date titles: {e}")))?;
        let rows = stmt
            .query_map(params![system], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| Error::Other(format!("Query library release-date titles: {e}")))?;
        for row in rows {
            let (system, base_title) =
                row.map_err(|e| Error::Other(format!("Read library release-date title: {e}")))?;
            titles_by_system
                .entry(system)
                .or_default()
                .insert(base_title);
        }
    } else {
        let mut stmt = conn
            .prepare(
                "SELECT DISTINCT system, base_title
                 FROM game_library
                 WHERE base_title != ''",
            )
            .map_err(|e| Error::Other(format!("Prepare library release-date titles: {e}")))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| Error::Other(format!("Query library release-date titles: {e}")))?;
        for row in rows {
            let (system, base_title) =
                row.map_err(|e| Error::Other(format!("Read library release-date title: {e}")))?;
            titles_by_system
                .entry(system)
                .or_default()
                .insert(base_title);
        }
    }
    Ok(titles_by_system)
}

fn static_release_rows(
    data: StaticReleaseData,
    library_titles: &HashMap<String, HashSet<String>>,
) -> Vec<ReleaseDateRow> {
    let has_title = |system: &str, base_title: &str| {
        library_titles
            .get(system)
            .is_some_and(|titles| titles.contains(base_title))
    };

    let StaticReleaseData {
        console_rows,
        arcade_rows,
        arcade_rom_to_display,
    } = data;

    let mut rows = Vec::new();
    for (system, base_title, region, date, precision, source) in console_rows {
        if !has_title(&system, &base_title) {
            continue;
        }
        let Some(precision) = DatePrecision::from_str(&precision) else {
            continue;
        };
        rows.push(ReleaseDateRow {
            system,
            base_title,
            region,
            release_date: date,
            precision,
            source,
        });
    }

    let arcade_systems: Vec<&str> = library_titles
        .keys()
        .filter(|system| systems::is_arcade_system(system))
        .map(String::as_str)
        .collect();
    if arcade_systems.is_empty() {
        return rows;
    }

    for (rom_name, year, source) in arcade_rows {
        let Some(display_name) = arcade_rom_to_display.get(&rom_name) else {
            continue;
        };
        let base_title = title_utils::base_title(display_name);
        if base_title.is_empty() {
            continue;
        }
        for system in &arcade_systems {
            if has_title(system, &base_title) {
                rows.push(ReleaseDateRow {
                    system: (*system).to_string(),
                    base_title: base_title.clone(),
                    region: "world".to_string(),
                    release_date: year.clone(),
                    precision: DatePrecision::Year,
                    source: source.clone(),
                });
            }
        }
    }

    rows
}

#[cfg(test)]
mod tests {
    use super::super::tests::open_temp_db;
    use super::*;

    fn row(
        system: &str,
        base_title: &str,
        region: &str,
        release_date: &str,
        precision: DatePrecision,
        source: &str,
    ) -> ReleaseDateRow {
        ReleaseDateRow {
            system: system.into(),
            base_title: base_title.into(),
            region: region.into(),
            release_date: release_date.into(),
            precision,
            source: source.into(),
        }
    }

    #[test]
    fn precision_upgrade_year_then_day() {
        let (mut conn, _d) = open_temp_db();

        // Seed with year precision from arcade DAT.
        LibraryDb::upsert_release_dates(
            &mut conn,
            &[row(
                "snes",
                "mario",
                "usa",
                "1991",
                DatePrecision::Year,
                "tgdb",
            )],
        )
        .unwrap();

        // Upgrade to day precision from LaunchBox.
        LibraryDb::upsert_release_dates(
            &mut conn,
            &[row(
                "snes",
                "mario",
                "usa",
                "1991-08-23",
                DatePrecision::Day,
                "launchbox",
            )],
        )
        .unwrap();

        let (date, prec, src): (String, String, String) = conn
            .query_row(
                "SELECT release_date, precision, source FROM game_release_date \
                 WHERE system = 'snes' AND base_title = 'mario' AND region = 'usa'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(date, "1991-08-23");
        assert_eq!(prec, "day");
        assert_eq!(src, "launchbox");
    }

    #[test]
    fn precision_upgrade_rejects_downgrade() {
        let (mut conn, _d) = open_temp_db();

        // Seed with day precision from LaunchBox.
        LibraryDb::upsert_release_dates(
            &mut conn,
            &[row(
                "snes",
                "mario",
                "usa",
                "1991-08-23",
                DatePrecision::Day,
                "launchbox",
            )],
        )
        .unwrap();

        // Attempt downgrade to year from TGDB — should NOT overwrite.
        LibraryDb::upsert_release_dates(
            &mut conn,
            &[row(
                "snes",
                "mario",
                "usa",
                "1991",
                DatePrecision::Year,
                "tgdb",
            )],
        )
        .unwrap();

        let (date, prec): (String, String) = conn
            .query_row(
                "SELECT release_date, precision FROM game_release_date \
                 WHERE system = 'snes' AND base_title = 'mario' AND region = 'usa'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(date, "1991-08-23");
        assert_eq!(prec, "day");
    }

    #[test]
    fn upsert_release_dates_spans_multiple_batches() {
        let (mut conn, _d) = open_temp_db();
        let row_count = LibraryDb::RELEASE_DATE_BATCH_ROWS + 3;
        let rows: Vec<_> = (0..row_count)
            .map(|i| {
                row(
                    "snes",
                    &format!("game {i}"),
                    "usa",
                    "1991",
                    DatePrecision::Year,
                    "test",
                )
            })
            .collect();

        let inserted = LibraryDb::upsert_release_dates(&mut conn, &rows).unwrap();
        assert_eq!(inserted, row_count);

        let count: usize = conn
            .query_row("SELECT COUNT(*) FROM game_release_date", [], |r| {
                r.get::<_, i64>(0).map(|v| v as usize)
            })
            .unwrap();
        assert_eq!(count, row_count);
    }

    fn make_entry(system: &str, filename: &str, base_title: &str) -> super::super::GameEntry {
        let mut e = super::super::tests::make_game_entry(system, filename, false);
        e.base_title = base_title.into();
        e
    }

    #[test]
    fn resolver_prefers_primary_region() {
        let (mut conn, _d) = open_temp_db();

        // Seed game_library row.
        LibraryDb::save_system_entries(
            &mut conn,
            "snes",
            &[make_entry("snes", "Mario (USA).sfc", "mario")],
            None,
        )
        .unwrap();

        // Multiple regions with different dates.
        LibraryDb::upsert_release_dates(
            &mut conn,
            &[
                row(
                    "snes",
                    "mario",
                    "japan",
                    "1990-11-21",
                    DatePrecision::Day,
                    "tgdb",
                ),
                row(
                    "snes",
                    "mario",
                    "usa",
                    "1991-08-23",
                    DatePrecision::Day,
                    "launchbox",
                ),
                row(
                    "snes",
                    "mario",
                    "europe",
                    "1991-08-13",
                    DatePrecision::Day,
                    "tgdb",
                ),
            ],
        )
        .unwrap();

        LibraryDb::resolve_release_date_for_library(&mut conn, RegionPreference::Usa, None)
            .unwrap();

        let (date, region_used): (Option<String>, Option<String>) = conn
            .query_row(
                "SELECT release_date, release_region_used FROM game_library WHERE system='snes' AND rom_filename='Mario (USA).sfc'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(date.as_deref(), Some("1991-08-23"));
        assert_eq!(region_used.as_deref(), Some("usa"));
    }

    #[test]
    fn mirror_updates_match_scoped_resolver() {
        let (mut conn, _d) = open_temp_db();

        LibraryDb::save_system_entries(
            &mut conn,
            "snes",
            &[
                make_entry("snes", "Mario (USA).sfc", "mario"),
                make_entry("snes", "Unmatched.sfc", "unmatched"),
            ],
            None,
        )
        .unwrap();
        LibraryDb::upsert_release_dates(
            &mut conn,
            &[
                row(
                    "snes",
                    "mario",
                    "world",
                    "1990",
                    DatePrecision::Year,
                    "tgdb",
                ),
                row(
                    "snes",
                    "mario",
                    "usa",
                    "1991-08-23",
                    DatePrecision::Day,
                    "launchbox",
                ),
            ],
        )
        .unwrap();

        let updates = LibraryDb::resolved_release_date_mirrors_for_system(
            &conn,
            "snes",
            RegionPreference::Usa,
            None,
        )
        .unwrap();
        assert_eq!(updates.len(), 2);
        LibraryDb::update_release_date_mirrors(&mut conn, "snes", &updates).unwrap();

        let rows: Vec<(String, Option<String>, Option<String>)> = conn
            .prepare(
                "SELECT rom_filename, release_date, release_region_used
                 FROM game_library
                 WHERE system = 'snes'
                 ORDER BY rom_filename",
            )
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();
        assert_eq!(
            rows,
            vec![
                (
                    "Mario (USA).sfc".to_string(),
                    Some("1991-08-23".to_string()),
                    Some("usa".to_string())
                ),
                ("Unmatched.sfc".to_string(), None, None)
            ]
        );
    }

    #[test]
    fn system_resolver_updates_only_target_system() {
        let (mut conn, _d) = open_temp_db();

        LibraryDb::save_system_entries(
            &mut conn,
            "snes",
            &[make_entry("snes", "Mario.sfc", "mario")],
            None,
        )
        .unwrap();
        LibraryDb::save_system_entries(
            &mut conn,
            "gba",
            &[make_entry("gba", "Mario Advance.gba", "mario")],
            None,
        )
        .unwrap();

        LibraryDb::upsert_release_dates(
            &mut conn,
            &[
                row("snes", "mario", "usa", "1991", DatePrecision::Year, "tgdb"),
                row("gba", "mario", "usa", "2001", DatePrecision::Year, "tgdb"),
            ],
        )
        .unwrap();

        LibraryDb::resolve_release_date_for_system(&mut conn, "snes", RegionPreference::Usa, None)
            .unwrap();

        let snes_date: Option<String> = conn
            .query_row(
                "SELECT release_date FROM game_library WHERE system='snes'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let gba_date: Option<String> = conn
            .query_row(
                "SELECT release_date FROM game_library WHERE system='gba'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(snes_date.as_deref(), Some("1991"));
        assert_eq!(gba_date, None);
    }

    #[tokio::test]
    async fn seed_from_static_only_inserts_library_members() {
        crate::catalog_pool::init_test_catalog().await;
        let (mut conn, _d) = open_temp_db();

        // Empty library: nothing should be inserted from static data.
        let data = super::fetch_static_release_data().await;
        let inserted = LibraryDb::seed_release_dates_from_static(&mut conn, data).unwrap();
        assert_eq!(
            inserted, 0,
            "with an empty library the static seeder should write 0 rows"
        );

        // Verify table is empty.
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM game_release_date", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn seed_from_static_populates_known_tgdb_game() {
        crate::catalog_pool::init_test_catalog().await;
        // This test requires real TGDB-backed static data (not stub fixtures),
        // so skip if static is empty.
        if crate::game_db::console_release_dates().await.is_empty() {
            return;
        }

        // Pick a well-known TGDB-covered SNES title.
        let (mut conn, _d) = open_temp_db();
        LibraryDb::save_system_entries(
            &mut conn,
            "nintendo_snes",
            &[make_entry(
                "nintendo_snes",
                "Super Mario World (USA).sfc",
                "super mario world",
            )],
            None,
        )
        .unwrap();

        let data = super::fetch_static_release_data().await;
        let inserted = LibraryDb::seed_release_dates_from_static(&mut conn, data).unwrap();

        // Should upsert at least one row for Super Mario World; TGDB knows
        // this game across multiple regions so we expect > 0 inserts.
        assert!(
            inserted > 0,
            "static seeder should have written at least one SMW row (got {inserted})"
        );

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM game_release_date \
                 WHERE system = 'nintendo_snes' AND base_title = 'super mario world'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            count > 0,
            "expected >= 1 row for SMW in game_release_date, got {count}"
        );
    }

    #[test]
    fn resolver_falls_back_to_world_then_unknown() {
        let (mut conn, _d) = open_temp_db();

        // game_library row.
        LibraryDb::save_system_entries(
            &mut conn,
            "snes",
            &[make_entry("snes", "Obscura.sfc", "obscura")],
            None,
        )
        .unwrap();

        // Only 'unknown' region available.
        LibraryDb::upsert_release_dates(
            &mut conn,
            &[row(
                "snes",
                "obscura",
                "unknown",
                "1989",
                DatePrecision::Year,
                "mame",
            )],
        )
        .unwrap();

        LibraryDb::resolve_release_date_for_library(&mut conn, RegionPreference::Usa, None)
            .unwrap();

        let (date, region_used): (Option<String>, Option<String>) = conn
            .query_row(
                "SELECT release_date, release_region_used FROM game_library WHERE rom_filename='Obscura.sfc'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(date.as_deref(), Some("1989"));
        assert_eq!(region_used.as_deref(), Some("unknown"));
    }
}
