use replay_control_core::title_utils::fuzzy_match_key;
use replay_control_core_server::library_db::LibraryDb;

use super::LibraryService;
use crate::api::DbPool;

impl LibraryService {
    /// Populate game_alias table with TGDB alternate names for a system.
    ///
    /// Builds lookup maps from library entries, delegates the matching to
    /// `replay_control_core_server::alias_matching`, then persists results.
    pub(super) async fn populate_tgdb_aliases(
        &self,
        system: &str,
        roms: &[replay_control_core_server::library_db::GameEntry],
        db: &DbPool,
    ) {
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
            return;
        }

        let count = aliases.len();
        let system = system.to_owned();
        let result = db
            .write(move |conn| LibraryDb::bulk_insert_aliases(conn, &aliases))
            .await;
        match result {
            Some(Ok(n)) => {
                tracing::debug!("TGDB aliases for {system}: {n}/{count} inserted")
            }
            Some(Err(e)) => tracing::warn!("TGDB aliases for {system}: insert failed: {e}"),
            None => {}
        }
    }

    /// Populate game_series table with Wikidata series data for a system.
    ///
    /// Delegates the matching to `replay_control_core_server::alias_matching`,
    /// then persists results.
    pub(super) async fn populate_wikidata_series(
        &self,
        system: &str,
        roms: &[replay_control_core_server::library_db::GameEntry],
        db: &DbPool,
    ) {
        // Call pure core matching function.
        let series_entries =
            replay_control_core_server::alias_matching::build_wikidata_series_tuples(system, roms)
                .await;

        if series_entries.is_empty() {
            return;
        }

        let count = series_entries.len();
        let system = system.to_owned();
        let result = db
            .write(move |conn| LibraryDb::bulk_insert_series(conn, &series_entries))
            .await;
        match result {
            Some(Ok(n)) => {
                tracing::debug!("Wikidata series for {system}: {n}/{count} inserted")
            }
            Some(Err(e)) => {
                tracing::warn!("Wikidata series for {system}: insert failed: {e}")
            }
            None => {}
        }
    }
}
