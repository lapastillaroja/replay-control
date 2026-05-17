use replay_control_core::title_utils::fuzzy_match_key;
use replay_control_core_server::library_db::LibraryDb;
use std::time::Instant;

use super::{LibraryService, ScanInputs};
use crate::api::AppState;
use crate::api::db_pools::{LIBRARY_MAINTENANCE_WRITE_TIMEOUT, LibraryWritePool};

impl LibraryService {
    pub(super) async fn populate_scan_derived_metadata(
        &self,
        state: &AppState,
        system: &str,
        roms: &[replay_control_core_server::library_db::GameEntry],
        scan_inputs: &ScanInputs,
    ) -> replay_control_core::error::Result<()> {
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

        let system_owned = system.to_string();
        let release_write_started = Instant::now();
        let region_pref = state.region_preference();
        let region_secondary = state.region_preference_secondary();
        let release_result = state
            .library_writer
            .try_write_with_timeout(LIBRARY_MAINTENANCE_WRITE_TIMEOUT, move |conn| {
                if let Err(e) = LibraryDb::seed_release_dates_from_static_for_system(
                    conn,
                    &system_owned,
                    static_data,
                ) {
                    tracing::warn!("Static release-date seed failed for {system_owned}: {e}");
                }
                if let Err(e) = LibraryDb::seed_release_dates_from_library_for_system(
                    conn,
                    &system_owned,
                    "builder",
                ) {
                    tracing::warn!("Library release-date seed failed for {system_owned}: {e}");
                }
                if let Err(e) = LibraryDb::resolve_release_date_for_system(
                    conn,
                    &system_owned,
                    region_pref,
                    region_secondary,
                ) {
                    tracing::warn!("Release-date resolve failed for {system_owned}: {e}");
                }
            })
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
        roms: &[replay_control_core_server::library_db::GameEntry],
        db: &LibraryWritePool,
        scan_inputs: &ScanInputs,
    ) -> replay_control_core::error::Result<()> {
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
        let aliases = replay_control_core_server::alias_matching::build_tgdb_alias_tuples(
            system,
            &library_exact,
            &library_fuzzy,
        )
        .await;

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
        roms: &[replay_control_core_server::library_db::GameEntry],
        db: &LibraryWritePool,
        scan_inputs: &ScanInputs,
    ) -> replay_control_core::error::Result<()> {
        // Call pure core matching function.
        let series_entry =
            replay_control_core_server::alias_matching::build_wikidata_series_tuples(system, roms)
                .await;

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
