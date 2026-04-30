//! Operations on the `game_release_date` table (multi-region, full-precision release dates).
//!
//! This table stores one row per `(system, base_title, region)` — across all
//! regions TGDB / LaunchBox / MAME / TOSEC found for that game. The resolver
//! picks the best date per the user's region preference and mirrors it into
//! `game_library` at scan time.

use std::collections::HashMap;

use rusqlite::{Connection, params};

use crate::{arcade_db, game_db};
use replay_control_core::error::{Error, Result};
use replay_control_core::rom_tags::RegionPreference;
use replay_control_core::{systems, title_utils};

use super::{DatePrecision, DpSql, LibraryDb};

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
    /// Bulk insert release-date rows. Overwrites existing `(system, base_title, region)` entries
    /// with `source = <new>` iff the new precision is strictly higher than the existing one
    /// (year < month < day). Lower or equal precision is rejected.
    ///
    /// This is the "precision-upgrade" write path used by both build-time embedded data
    /// (year-precision from arcade DAT/TOSEC) and runtime LaunchBox enrichment (day-precision).
    pub fn upsert_release_dates(conn: &mut Connection, rows: &[ReleaseDateRow]) -> Result<usize> {
        if rows.is_empty() {
            return Ok(0);
        }
        let tx = conn
            .transaction()
            .map_err(|e| Error::Other(format!("Transaction start: {e}")))?;

        let mut count = 0usize;
        {
            // Precision rank: day=3, month=2, year=1.
            // SQLite COALESCE + CASE lets us compare existing vs new.
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
            for r in rows {
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
        Ok(count)
    }

    /// Populate `game_release_date` from `game_metadata` rows (LaunchBox-origin dates).
    ///
    /// Cross-joins against `game_library` to get the `(base_title, region)` for each
    /// `(system, rom_filename)`. Uses `source = 'launchbox'`.
    pub fn seed_release_dates_from_metadata(conn: &mut Connection) -> Result<usize> {
        let mut stmt = conn
            .prepare(
                "SELECT DISTINCT gm.system, gl.base_title, \
                        CASE WHEN gl.region = '' THEN 'unknown' ELSE gl.region END, \
                        gm.release_date, gm.release_precision \
                 FROM game_metadata gm \
                 JOIN game_library gl \
                   ON gl.system = gm.system AND gl.rom_filename = gm.rom_filename \
                 WHERE gl.base_title != '' AND gm.release_date IS NOT NULL",
            )
            .map_err(|e| Error::Other(format!("Prepare seed_release_dates_from_metadata: {e}")))?;

        let rows: Vec<ReleaseDateRow> = stmt
            .query_map([], |row| {
                Ok(ReleaseDateRow {
                    system: row.get(0)?,
                    base_title: row.get(1)?,
                    region: row.get(2)?,
                    release_date: row.get(3)?,
                    precision: row
                        .get::<_, Option<DpSql>>(4)?
                        .map(|DpSql(d)| d)
                        .unwrap_or(DatePrecision::Year),
                    source: "launchbox".to_string(),
                })
            })
            .map_err(|e| Error::Other(format!("Query seed_release_dates_from_metadata: {e}")))?
            .flatten()
            .collect();

        drop(stmt);
        Self::upsert_release_dates(conn, &rows)
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
        // Step 1: Collect the `(system, base_title)` set that actually exists
        // in the user's library. Grouped by system so we can look up with
        // `&str` keys and skip cloning on every filter check.
        let mut library_pairs: HashMap<String, std::collections::HashSet<String>> = HashMap::new();
        {
            let mut stmt = conn
                .prepare(
                    "SELECT DISTINCT system, base_title FROM game_library \
                     WHERE base_title != ''",
                )
                .map_err(|e| Error::Other(format!("Prepare library pairs: {e}")))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|e| Error::Other(format!("Query library pairs: {e}")))?;
            for (system, base_title) in rows.flatten() {
                library_pairs.entry(system).or_default().insert(base_title);
            }
        }

        let has_pair = |system: &str, base_title: &str| {
            library_pairs
                .get(system)
                .is_some_and(|titles| titles.contains(base_title))
        };

        let StaticReleaseData {
            console_rows,
            arcade_rows,
            arcade_rom_to_display,
        } = data;

        // Step 2: Filter console TGDB rows against the library set.
        let mut rows: Vec<ReleaseDateRow> = Vec::new();
        for (system, base_title, region, date, precision, source) in console_rows {
            if !has_pair(&system, &base_title) {
                continue;
            }
            let Some(prec) = DatePrecision::from_str(&precision) else {
                continue;
            };
            rows.push(ReleaseDateRow {
                system,
                base_title,
                region,
                release_date: date,
                precision: prec,
                source,
            });
        }

        // Step 3: Arcade rows. Each row's `rom_name` is the arcade ROM stem;
        // we need to look up the arcade_db entry to get the display_name,
        // then compute `base_title`. Then emit one row per arcade system
        // folder that the user has in `game_library`.
        let arcade_systems_in_library: Vec<&str> = library_pairs
            .keys()
            .filter(|s| systems::is_arcade_system(s))
            .map(String::as_str)
            .collect();

        if !arcade_systems_in_library.is_empty() {
            for (rom_name, year, source) in &arcade_rows {
                let Some(display_name) = arcade_rom_to_display.get(rom_name) else {
                    continue;
                };
                let base_title = title_utils::base_title(display_name);
                if base_title.is_empty() {
                    continue;
                }
                for sys in &arcade_systems_in_library {
                    if has_pair(sys, &base_title) {
                        rows.push(ReleaseDateRow {
                            system: (*sys).to_string(),
                            base_title: base_title.clone(),
                            region: "world".to_string(),
                            release_date: year.clone(),
                            precision: DatePrecision::Year,
                            source: source.clone(),
                        });
                    }
                }
            }
        }

        Self::upsert_release_dates(conn, &rows)
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
        // Collect distinct (system, base_title, region, release_date, precision) tuples.
        let mut stmt = conn
            .prepare(
                "SELECT DISTINCT system, base_title, \
                        CASE WHEN region = '' THEN 'unknown' ELSE region END, \
                        release_date, release_precision \
                 FROM game_library \
                 WHERE base_title != '' AND release_date IS NOT NULL",
            )
            .map_err(|e| Error::Other(format!("Prepare seed_release_dates_from_library: {e}")))?;

        let rows: Vec<ReleaseDateRow> = stmt
            .query_map([], |row| {
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
            })
            .map_err(|e| Error::Other(format!("Query seed_release_dates_from_library: {e}")))?
            .flatten()
            .collect();

        drop(stmt);
        Self::upsert_release_dates(conn, &rows)
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
        let primary_region = region_pref_to_db_region(primary);
        let secondary_region = secondary.map(region_pref_to_db_region);

        let tx = conn
            .transaction()
            .map_err(|e| Error::Other(format!("Transaction start: {e}")))?;

        // Rank every game_release_date row once via ROW_NUMBER(), then assign
        // each game_library row the rank-1 match via a row-value UPDATE.
        // If no match exists in `best`, the subquery yields (NULL, NULL, NULL),
        // which clears any stale mirror values — no separate NULL-out pass needed.
        let sql = "\
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
            WHERE base_title != ''";

        let affected = tx
            .execute(sql, params![primary_region, secondary_region])
            .map_err(|e| Error::Other(format!("resolve_release_date_for_library: {e}")))?;

        tx.commit()
            .map_err(|e| Error::Other(format!("Transaction commit: {e}")))?;
        Ok(affected)
    }
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
