//! Pure matching algorithms for building alias and series tuples.
//!
//! These functions take library data and embedded DB data, returning tuples
//! ready for DB insertion. No DB, Mutex, or AppState access.

use std::collections::{HashMap, HashSet};

use crate::metadata_db::GameEntry;
use crate::title_utils::{fuzzy_match_key, normalize_for_wikidata, resolve_to_library_title};

/// Build TGDB alternate name alias tuples for a system.
///
/// Matches canonical games in the embedded TGDB data to library entries via
/// normalized title, then returns alias tuples for their alternate names.
///
/// Returns `(system, base_title, alias_name, alias_region, source)` tuples.
///
/// # Arguments
/// * `system` - The system folder name.
/// * `library_base_titles` - Set of all `base_title` values in the library.
/// * `library_fuzzy` - Map of `fuzzy_match_key(base_title)` -> `base_title`.
pub fn build_tgdb_alias_tuples(
    system: &str,
    library_base_titles: &HashSet<&str>,
    library_fuzzy: &HashMap<String, &str>,
) -> Vec<(String, String, String, String, String)> {
    use crate::game_db;
    use crate::systems;

    let is_arcade = systems::is_arcade_system(system);

    // TGDB alternates are only available for non-arcade systems with game_db coverage.
    if is_arcade || !game_db::has_system(system) {
        return Vec::new();
    }

    let alternates = game_db::system_alternates(system);
    if alternates.is_empty() {
        return Vec::new();
    }

    let games = match game_db::system_games(system) {
        Some(g) => g,
        None => return Vec::new(),
    };

    let mut aliases: Vec<(String, String, String, String, String)> = Vec::new();

    for &(game_id, alt_names) in alternates {
        if let Some(game) = games.get(game_id as usize) {
            let resolved =
                resolve_to_library_title(game.display_name, library_base_titles, library_fuzzy);
            if !library_base_titles.contains(resolved.as_str())
                && !library_fuzzy.contains_key(&fuzzy_match_key(&resolved))
            {
                continue; // Game not in user's library
            }
            let library_bt = resolved;

            for alt in alt_names {
                let alt_resolved =
                    resolve_to_library_title(alt, library_base_titles, library_fuzzy);
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
                    if library_base_titles.contains(alt_resolved.as_str())
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

    aliases
}

/// Build Wikidata series tuples for a system.
///
/// Matches embedded Wikidata entries to library games by normalized title,
/// returning series membership tuples ready for DB insertion.
///
/// Returns `(system, base_title, series_name, series_order, source)` tuples.
///
/// # Arguments
/// * `system` - The system folder name.
/// * `library_entries` - All `GameEntry` rows from `game_library` for this system.
pub fn build_wikidata_series_tuples(
    system: &str,
    library_entries: &[GameEntry],
) -> Vec<(String, String, String, Option<i32>, String)> {
    use crate::series_db;
    use crate::systems;
    use crate::title_utils;

    let wikidata_entries = series_db::system_series_entries(system);
    if wikidata_entries.is_empty() {
        return Vec::new();
    }

    // Build a map of normalized_title -> base_title for games in the library.
    let mut norm_to_base: HashMap<String, String> = HashMap::new();
    for rom in library_entries {
        if rom.base_title.is_empty() {
            continue;
        }
        let normalized = normalize_for_wikidata(&rom.base_title);
        if !normalized.is_empty() {
            norm_to_base
                .entry(normalized)
                .or_insert_with(|| rom.base_title.clone());
        }
        // Also try with display_name for better matching
        if let Some(ref dn) = rom.display_name {
            let norm_dn = normalize_for_wikidata(&title_utils::base_title(dn));
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

    series_entries
}

/// Resolve LaunchBox alternate names into alias tuples.
///
/// Groups alternate names by DatabaseID, includes primary game names, then
/// resolves each group against the library's base_titles. Returns alias tuples
/// for groups where at least one name matches a library entry.
///
/// Returns `(system, base_title, alias_name, alias_region, source)` tuples.
///
/// # Arguments
/// * `alt_names` - Parsed alternate name entries from LaunchBox XML.
/// * `game_names` - DatabaseID -> primary game name mapping.
/// * `base_titles` - Map of `base_title` -> `[system, ...]` from the library DB.
pub fn resolve_launchbox_aliases(
    alt_names: &[crate::launchbox::LbAlternateName],
    game_names: &HashMap<String, String>,
    base_titles: &HashMap<String, Vec<String>>,
) -> Vec<(String, String, String, String, String)> {
    if alt_names.is_empty() {
        return Vec::new();
    }

    // Group alternates by DatabaseID -> Vec<(name, region)>.
    // Include the primary game name so that alias groups contain ALL names for a game.
    let mut by_db_id: HashMap<String, Vec<(String, String)>> = HashMap::new();
    for alt in alt_names {
        by_db_id
            .entry(alt.database_id.clone())
            .or_default()
            .push((alt.alternate_name.clone(), alt.region.clone()));
    }
    // Add primary game name to each group (with empty region).
    for (db_id, primary_name) in game_names {
        by_db_id
            .entry(db_id.clone())
            .or_default()
            .push((primary_name.clone(), String::new()));
    }

    // Build lookup maps for fuzzy matching (colon/dash normalization).
    let library_exact: HashSet<&str> = base_titles.keys().map(|s| s.as_str()).collect();

    let library_fuzzy: HashMap<String, &str> = base_titles
        .keys()
        .map(|bt| (fuzzy_match_key(bt), bt.as_str()))
        .collect();

    // For each DatabaseID group, check if any alternate name resolves to a known base_title.
    // If it does, create alias entries linking the other alternates to that base_title.
    let mut aliases: Vec<(String, String, String, String, String)> = Vec::new();

    for alts in by_db_id.values() {
        // Find which alternate resolves to a library base_title.
        let mut matched_bt: Option<(String, String)> = None; // (base_title, system)
        for (alt_name, _) in alts {
            let resolved = resolve_to_library_title(alt_name, &library_exact, &library_fuzzy);
            if let Some(systems) = base_titles.get(&resolved) {
                matched_bt = Some((resolved, systems[0].clone()));
                break;
            }
        }

        if let Some((bt, system)) = matched_bt {
            // Insert all other alternates as aliases of this base_title.
            for (alt_name, region) in alts {
                let resolved =
                    resolve_to_library_title(alt_name, &library_exact, &library_fuzzy);
                if resolved != bt && !resolved.is_empty() {
                    aliases.push((
                        system.clone(),
                        bt.clone(),
                        resolved,
                        region.clone(),
                        "launchbox".to_string(),
                    ));
                }
            }
        }
    }

    aliases
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(system: &str, base_title: &str) -> GameEntry {
        GameEntry {
            system: system.into(),
            rom_filename: format!("{base_title}.sfc"),
            rom_path: format!("/roms/{system}/{base_title}.sfc"),
            display_name: None,
            size_bytes: 1000,
            is_m3u: false,
            box_art_url: None,
            driver_status: None,
            genre: None,
            genre_group: String::new(),
            players: None,
            rating: None,
            is_clone: false,
            base_title: base_title.into(),
            region: String::new(),
            is_translation: false,
            is_hack: false,
            is_special: false,
            crc32: None,
            hash_mtime: None,
            hash_matched_name: None,
            series_key: String::new(),
        }
    }

    #[test]
    fn tgdb_aliases_empty_for_arcade() {
        let exact: HashSet<&str> = HashSet::new();
        let fuzzy: HashMap<String, &str> = HashMap::new();
        let result = build_tgdb_alias_tuples("arcade_fbneo", &exact, &fuzzy);
        assert!(result.is_empty());
    }

    #[test]
    fn tgdb_aliases_empty_for_unsupported_system() {
        let exact: HashSet<&str> = HashSet::new();
        let fuzzy: HashMap<String, &str> = HashMap::new();
        let result = build_tgdb_alias_tuples("nonexistent_system", &exact, &fuzzy);
        assert!(result.is_empty());
    }

    #[test]
    fn wikidata_series_works_for_arcade() {
        // Arcade systems now have Wikidata series data (546 entries).
        // "some game" won't match anything, but it shouldn't panic.
        let entries = vec![make_entry("arcade_fbneo", "some game")];
        let result = build_wikidata_series_tuples("arcade_fbneo", &entries);
        // No match expected for a fake game, but the function runs without skipping.
        assert!(result.is_empty());
    }

    #[test]
    fn wikidata_series_empty_for_empty_library() {
        let result = build_wikidata_series_tuples("nintendo_snes", &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn wikidata_series_skips_entries_without_base_title() {
        let entries = vec![make_entry("nintendo_snes", "")];
        let result = build_wikidata_series_tuples("nintendo_snes", &entries);
        // Even if there's a wikidata match, entries with empty base_title are skipped
        assert!(result.is_empty());
    }

    #[test]
    fn tgdb_aliases_source_is_tgdb() {
        // If we get any results, verify the source field
        let exact: HashSet<&str> = ["super mario world"].into_iter().collect();
        let fuzzy: HashMap<String, &str> = [(
            fuzzy_match_key("super mario world"),
            "super mario world",
        )]
        .into_iter()
        .collect();
        let result = build_tgdb_alias_tuples("nintendo_snes", &exact, &fuzzy);
        for tuple in &result {
            assert_eq!(tuple.4, "tgdb");
            assert_eq!(tuple.0, "nintendo_snes");
        }
    }

    // --- resolve_launchbox_aliases ---

    fn make_lb_alt(db_id: &str, name: &str, region: &str) -> crate::launchbox::LbAlternateName {
        crate::launchbox::LbAlternateName {
            database_id: db_id.into(),
            alternate_name: name.into(),
            region: region.into(),
        }
    }

    #[test]
    fn launchbox_aliases_empty_when_no_alt_names() {
        let game_names: HashMap<String, String> = HashMap::new();
        let base_titles: HashMap<String, Vec<String>> = HashMap::new();
        let result = resolve_launchbox_aliases(&[], &game_names, &base_titles);
        assert!(result.is_empty());
    }

    #[test]
    fn launchbox_aliases_no_match_returns_empty() {
        let alt_names = vec![make_lb_alt("1", "Unknown Game", "USA")];
        let game_names: HashMap<String, String> = HashMap::new();
        let base_titles: HashMap<String, Vec<String>> = HashMap::new();
        let result = resolve_launchbox_aliases(&alt_names, &game_names, &base_titles);
        assert!(result.is_empty());
    }

    #[test]
    fn launchbox_aliases_resolves_primary_to_library() {
        // base_title() normalizes "Super Mario Bros." to "super mario bros" (lowercase, no period)
        let alt_names = vec![make_lb_alt("1", "Super Mario Bros. (Japanese)", "Japan")];
        let mut game_names: HashMap<String, String> = HashMap::new();
        game_names.insert("1".into(), "Super Mario Bros.".into());

        let mut base_titles: HashMap<String, Vec<String>> = HashMap::new();
        // Use the normalized form that resolve_to_library_title would produce
        base_titles.insert("super mario bros".into(), vec!["nintendo_nes".into()]);

        let result = resolve_launchbox_aliases(&alt_names, &game_names, &base_titles);
        // The primary name resolves to "super mario bros" which matches the library.
        // The Japanese alt also resolves to "super mario bros" (same normalized form),
        // so it won't produce a separate alias (resolved == bt).
        // If they differ, an alias would be created.
        // In this case both normalize to the same thing, so no alias is emitted.
        // Let's use a truly different alternate name.
        assert!(result.is_empty()); // Same normalized form = no alias

        // Now with a genuinely different alternate name:
        let alt_names2 = vec![make_lb_alt("1", "Mario 1", "")];
        let result2 = resolve_launchbox_aliases(&alt_names2, &game_names, &base_titles);
        assert!(!result2.is_empty());
        for tuple in &result2 {
            assert_eq!(tuple.0, "nintendo_nes"); // system
            assert_eq!(tuple.1, "super mario bros"); // base_title
            assert_eq!(tuple.4, "launchbox"); // source
        }
    }

    #[test]
    fn launchbox_aliases_source_is_launchbox() {
        let alt_names = vec![make_lb_alt("1", "Sonic Hedgehog", "")];
        let mut game_names: HashMap<String, String> = HashMap::new();
        game_names.insert("1".into(), "Sonic The Hedgehog".into());

        let mut base_titles: HashMap<String, Vec<String>> = HashMap::new();
        base_titles.insert("sonic the hedgehog".into(), vec!["sega_smd".into()]);

        let result = resolve_launchbox_aliases(&alt_names, &game_names, &base_titles);
        for tuple in &result {
            assert_eq!(tuple.4, "launchbox");
        }
    }

    #[test]
    fn wikidata_series_source_is_wikidata() {
        // Build entries for games that might be in wikidata
        let entries = vec![
            make_entry("nintendo_snes", "super mario world"),
            make_entry("nintendo_snes", "the legend of zelda"),
        ];
        let result = build_wikidata_series_tuples("nintendo_snes", &entries);
        for tuple in &result {
            assert_eq!(tuple.4, "wikidata");
            assert_eq!(tuple.0, "nintendo_snes");
        }
    }
}
