use replay_control_core::error::{Error, Result};
use replay_control_core::rom_tags::RegionPreference;
use replay_control_core::title_utils::fuzzy_match_key;
use replay_control_core_server::alias_matching::{
    build_tgdb_alias_tuples, build_wikidata_series_tuples,
};
use replay_control_core_server::library_db::GameEntry;
use replay_control_core_server::library_db::{LibraryDb, ReleaseDateMirrorUpdate, ReleaseDateRow};
use std::time::Instant;

use super::{LibraryService, ScanInputs};
use crate::api::AppState;
use crate::api::db_pools::{LIBRARY_MAINTENANCE_WRITE_TIMEOUT, LibraryWritePool};

const SCAN_RELEASE_DATE_WRITE_CHUNK_ROWS: usize = 1_000;

impl LibraryService {
    pub(super) async fn populate_scan_derived_metadata(
        &self,
        state: &AppState,
        system: &str,
        roms: &[GameEntry],
        scan_inputs: &ScanInputs,
    ) -> Result<()> {
        let started = Instant::now();

        let aliases_started = Instant::now();
        self.populate_tgdb_aliases(system, roms, &state.library_writer, scan_inputs)
            .await?;
        let aliases_ms = aliases_started.elapsed().as_millis();

        let series_started = Instant::now();
        self.populate_wikidata_series(system, roms, &state.library_writer, scan_inputs)
            .await?;
        let series_ms = series_started.elapsed().as_millis();
        scan_inputs.ensure_current()?;

        let static_release_started = Instant::now();
        let static_data = replay_control_core_server::library_db::fetch_static_release_data().await;
        let static_release_ms = static_release_started.elapsed().as_millis();
        scan_inputs.ensure_current()?;

        let static_system = system.to_string();
        let static_rows = match state
            .library_reader
            .try_read(move |conn| {
                LibraryDb::static_release_date_rows_for_system(conn, &static_system, static_data)
            })
            .await
        {
            Ok(Ok(rows)) => rows,
            Ok(Err(e)) => {
                tracing::warn!("Static release-date row build failed for {system}: {e}");
                Vec::new()
            }
            Err(e) => {
                tracing::warn!("Static release-date row read failed for {system}: {e}");
                Vec::new()
            }
        };
        let library_system = system.to_string();
        let library_rows = match state
            .library_reader
            .try_read(move |conn| {
                LibraryDb::library_release_date_rows_for_system(conn, &library_system, "builder")
            })
            .await
        {
            Ok(Ok(rows)) => rows,
            Ok(Err(e)) => {
                tracing::warn!("Library release-date row build failed for {system}: {e}");
                Vec::new()
            }
            Err(e) => {
                tracing::warn!("Library release-date row read failed for {system}: {e}");
                Vec::new()
            }
        };
        let release_write_started = Instant::now();
        let region_pref = state.region_preference();
        let region_secondary = state.region_preference_secondary();
        let release_result = write_scan_release_dates(
            state,
            system,
            static_rows,
            library_rows,
            region_pref,
            region_secondary,
            scan_inputs,
        )
        .await;
        let release_write_ms = release_write_started.elapsed().as_millis();
        if let Err(e) = release_result {
            tracing::warn!("Release-date write failed for {system}: {e}");
        }

        tracing::info!(
            "Scan-derived enrichment profile: {system}: roms={} aliases_ms={aliases_ms} series_ms={series_ms} static_release_ms={static_release_ms} release_write_ms={release_write_ms} total_ms={}",
            roms.len(),
            started.elapsed().as_millis()
        );
        Ok(())
    }

    /// Populate game_alias table with TGDB alternate names for a system.
    ///
    /// Builds lookup maps from library entries, delegates the matching to
    /// `replay_control_core_server::alias_matching`, then persists results.
    pub(super) async fn populate_tgdb_aliases(
        &self,
        system: &str,
        roms: &[GameEntry],
        db: &LibraryWritePool,
        scan_inputs: &ScanInputs,
    ) -> Result<()> {
        // Build lookup maps for matching TGDB names to library base_titles.
        let library_exact: std::collections::HashSet<&str> = roms
            .iter()
            .filter(|r| !r.base_title.is_empty())
            .map(|r| r.base_title.as_str())
            .collect();

        let library_fuzzy: std::collections::HashMap<String, &str> = roms
            .iter()
            .filter(|r| !r.base_title.is_empty())
            .map(|r| (fuzzy_match_key(&r.base_title), r.base_title.as_str()))
            .collect();

        // Call pure core matching function.
        let aliases = build_tgdb_alias_tuples(system, &library_exact, &library_fuzzy).await;

        if aliases.is_empty() {
            return Ok(());
        }

        let count = aliases.len();
        let system = system.to_owned();
        scan_inputs.ensure_current()?;
        let result = db
            .try_write_with_timeout(LIBRARY_MAINTENANCE_WRITE_TIMEOUT, move |conn| {
                LibraryDb::bulk_insert_aliases(conn, &aliases)
            })
            .await;
        match result {
            Ok(Ok(n)) => {
                tracing::debug!("TGDB aliases for {system}: {n}/{count} inserted")
            }
            Ok(Err(e)) => tracing::warn!("TGDB aliases for {system}: insert failed: {e}"),
            Err(e) => tracing::warn!("TGDB aliases for {system}: write failed: {e}"),
        }
        Ok(())
    }

    /// Populate game_series table with Wikidata series data for a system.
    ///
    /// Delegates the matching to `replay_control_core_server::alias_matching`,
    /// then persists results.
    pub(super) async fn populate_wikidata_series(
        &self,
        system: &str,
        roms: &[GameEntry],
        db: &LibraryWritePool,
        scan_inputs: &ScanInputs,
    ) -> Result<()> {
        // Call pure core matching function.
        let series_entry = build_wikidata_series_tuples(system, roms).await;

        if series_entry.is_empty() {
            return Ok(());
        }

        let count = series_entry.len();
        let system = system.to_owned();
        scan_inputs.ensure_current()?;
        let result = db
            .try_write_with_timeout(LIBRARY_MAINTENANCE_WRITE_TIMEOUT, move |conn| {
                LibraryDb::bulk_insert_series(conn, &series_entry)
            })
            .await;
        match result {
            Ok(Ok(n)) => {
                tracing::debug!("Wikidata series for {system}: {n}/{count} inserted")
            }
            Ok(Err(e)) => {
                tracing::warn!("Wikidata series for {system}: insert failed: {e}")
            }
            Err(e) => tracing::warn!("Wikidata series for {system}: write failed: {e}"),
        }
        Ok(())
    }
}

async fn write_scan_release_dates(
    state: &AppState,
    system: &str,
    mut static_rows: Vec<ReleaseDateRow>,
    library_rows: Vec<ReleaseDateRow>,
    primary: RegionPreference,
    secondary: Option<RegionPreference>,
    scan_inputs: &ScanInputs,
) -> Result<()> {
    static_rows.extend(library_rows);
    for (index, chunk) in static_rows
        .chunks(SCAN_RELEASE_DATE_WRITE_CHUNK_ROWS)
        .enumerate()
    {
        scan_inputs.ensure_current()?;
        let rows = chunk.to_vec();
        let rows_len = rows.len();
        let result = state
            .library_writer
            .try_write_with_timeout(LIBRARY_MAINTENANCE_WRITE_TIMEOUT, move |conn| {
                LibraryDb::upsert_release_dates(conn, &rows)
            })
            .await;
        match result {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => {
                return Err(Error::Other(format!(
                    "release-date chunk {} ({} rows) SQL failed: {e}",
                    index + 1,
                    rows_len
                )));
            }
            Err(e) => {
                return Err(Error::Other(format!(
                    "release-date chunk {} ({} rows) write failed: {e}",
                    index + 1,
                    rows_len
                )));
            }
        }
    }

    scan_inputs.ensure_current()?;
    let mirror_system = system.to_string();
    let mirror_updates = match state
        .library_reader
        .try_read(move |conn| {
            LibraryDb::resolved_release_date_mirrors_for_system(
                conn,
                &mirror_system,
                primary,
                secondary,
            )
        })
        .await
    {
        Ok(Ok(updates)) => updates,
        Ok(Err(e)) => return Err(e),
        Err(e) => return Err(Error::Other(e.to_string())),
    };
    write_release_date_mirror_chunks(&state.library_writer, system, mirror_updates, scan_inputs)
        .await
}

async fn write_release_date_mirror_chunks(
    db: &LibraryWritePool,
    system: &str,
    updates: Vec<ReleaseDateMirrorUpdate>,
    scan_inputs: &ScanInputs,
) -> Result<()> {
    for (index, chunk) in updates
        .chunks(SCAN_RELEASE_DATE_WRITE_CHUNK_ROWS)
        .enumerate()
    {
        scan_inputs.ensure_current()?;
        let system = system.to_string();
        let rows = chunk.to_vec();
        let rows_len = rows.len();
        let result = db
            .try_write_with_timeout(LIBRARY_MAINTENANCE_WRITE_TIMEOUT, move |conn| {
                LibraryDb::update_release_date_mirrors(conn, &system, &rows)
            })
            .await;
        match result {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => {
                return Err(Error::Other(format!(
                    "release-date mirror chunk {} ({} rows) SQL failed: {e}",
                    index + 1,
                    rows_len
                )));
            }
            Err(e) => {
                return Err(Error::Other(format!(
                    "release-date mirror chunk {} ({} rows) write failed: {e}",
                    index + 1,
                    rows_len
                )));
            }
        }
    }
    Ok(())
}
