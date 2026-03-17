use replay_control_core::storage::StorageLocation;

use super::GameLibrary;

impl GameLibrary {
    /// Populate game_alias table with TGDB alternate names for a system.
    ///
    /// Matches canonical games in the embedded TGDB data to `game_library`
    /// entries via normalized title, then inserts their alternate names.
    pub(super) fn populate_tgdb_aliases(
        &self,
        storage: &StorageLocation,
        system: &str,
        roms: &[replay_control_core::metadata_db::GameEntry],
    ) {
        use replay_control_core::game_db;
        use replay_control_core::systems;

        let is_arcade = systems::is_arcade_system(system);

        // TGDB alternates are only available for non-arcade systems with game_db coverage.
        if is_arcade || !game_db::has_system(system) {
            return;
        }

        let alternates = game_db::system_alternates(system);
        if alternates.is_empty() {
            return;
        }

        let games = match game_db::system_games(system) {
            Some(g) => g,
            None => return,
        };

        // Build lookup maps for matching TGDB names to library base_titles.
        use replay_control_core::title_utils::{fuzzy_match_key, resolve_to_library_title};

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

        let mut aliases: Vec<(String, String, String, String, String)> = Vec::new();

        for &(game_id, alt_names) in alternates {
            if let Some(game) = games.get(game_id as usize) {
                let resolved =
                    resolve_to_library_title(game.display_name, &library_exact, &library_fuzzy);
                if !library_exact.contains(resolved.as_str())
                    && !library_fuzzy.contains_key(&fuzzy_match_key(&resolved))
                {
                    continue; // Game not in user's library
                }
                let library_bt = resolved;

                for alt in alt_names {
                    let alt_resolved =
                        resolve_to_library_title(alt, &library_exact, &library_fuzzy);
                    if alt_resolved != library_bt && !alt_resolved.is_empty() {
                        // Forward: library game -> alternate name
                        aliases.push((
                            system.to_string(),
                            library_bt.clone(),
                            alt_resolved.clone(),
                            String::new(),
                            "tgdb".to_string(),
                        ));
                        // Reverse: if the alternate is also in the library, link back
                        if library_exact.contains(alt_resolved.as_str())
                            || library_fuzzy.contains_key(&fuzzy_match_key(&alt_resolved))
                        {
                            aliases.push((
                                system.to_string(),
                                alt_resolved,
                                library_bt.clone(),
                                String::new(),
                                "tgdb".to_string(),
                            ));
                        }
                    }
                }
            }
        }

        if aliases.is_empty() {
            return;
        }

        let count = aliases.len();
        let result = self.with_db_mut(storage, |db| db.bulk_insert_aliases(&aliases));
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
    /// Matches embedded Wikidata entries to `game_library` rows by normalized
    /// title + system, then inserts series membership into `game_series`.
    pub(super) fn populate_wikidata_series(
        &self,
        storage: &StorageLocation,
        system: &str,
        roms: &[replay_control_core::metadata_db::GameEntry],
    ) {
        use replay_control_core::series_db;
        use replay_control_core::systems;

        // Wikidata series data is only available for non-arcade systems with game_db coverage.
        if systems::is_arcade_system(system) {
            return;
        }

        let wikidata_entries = series_db::system_series_entries(system);
        if wikidata_entries.is_empty() {
            return;
        }

        // Build a map of normalized_title -> base_title for games in the library.
        let mut norm_to_base: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for rom in roms {
            if rom.base_title.is_empty() {
                continue;
            }
            // Normalize the base_title the same way Wikidata titles are normalized:
            // lowercase, strip non-alphanumeric except spaces, collapse whitespace.
            let normalized =
                replay_control_core::title_utils::normalize_for_wikidata(&rom.base_title);
            if !normalized.is_empty() {
                norm_to_base
                    .entry(normalized)
                    .or_insert_with(|| rom.base_title.clone());
            }
            // Also try with display_name for better matching
            if let Some(ref dn) = rom.display_name {
                let norm_dn = replay_control_core::title_utils::normalize_for_wikidata(
                    &replay_control_core::title_utils::base_title(dn),
                );
                if !norm_dn.is_empty() {
                    norm_to_base
                        .entry(norm_dn)
                        .or_insert_with(|| rom.base_title.clone());
                }
            }
        }

        let mut series_entries: Vec<(String, String, String, Option<i32>, String)> = Vec::new();

        for entry in &wikidata_entries {
            if let Some(base_title) = norm_to_base.get(entry.normalized_title)
                && !entry.series_name.is_empty()
            {
                series_entries.push((
                    system.to_string(),
                    base_title.clone(),
                    entry.series_name.to_string(),
                    entry.series_order,
                    "wikidata".to_string(),
                ));
            }
        }

        if series_entries.is_empty() {
            return;
        }

        let count = series_entries.len();
        let result = self.with_db_mut(storage, |db| db.bulk_insert_series(&series_entries));
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
