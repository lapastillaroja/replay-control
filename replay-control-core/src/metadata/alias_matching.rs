//! Pure matching algorithms for building alias and series tuples.
//!
//! These functions take library data and embedded DB data, returning tuples
//! ready for DB insertion. No DB, Mutex, or AppState access.

use std::collections::{HashMap, HashSet};

use crate::metadata_db::{AliasInsert, GameEntry, SeriesInsert};
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
) -> Vec<AliasInsert> {
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

    let mut aliases: Vec<AliasInsert> = Vec::new();

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
                    aliases.push(AliasInsert {
                        system: system.to_string(),
                        base_title: library_bt.clone(),
                        alias_name: alt_resolved.clone(),
                        alias_region: String::new(),
                        source: "tgdb".to_string(),
                    });
                    // Reverse: if the alternate is also in the library, link back
                    if library_base_titles.contains(alt_resolved.as_str())
                        || library_fuzzy.contains_key(&fuzzy_match_key(&alt_resolved))
                    {
                        aliases.push(AliasInsert {
                            system: system.to_string(),
                            base_title: alt_resolved,
                            alias_name: library_bt.clone(),
                            alias_region: String::new(),
                            source: "tgdb".to_string(),
                        });
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
/// Returns `(system, base_title, series_name, series_order, source, follows_base_title, followed_by_base_title)` tuples.
/// The follows/followed_by values are resolved to library base_titles when possible,
/// or stored as raw Wikidata titles when the linked game isn't in the library.
///
/// # Arguments
/// * `system` - The system folder name.
/// * `library_entries` - All `GameEntry` rows from `game_library` for this system.
pub fn build_wikidata_series_tuples(
    system: &str,
    library_entries: &[GameEntry],
) -> Vec<SeriesInsert> {
    use crate::series_db;
    use crate::title_utils;

    // Match against ALL Wikidata entries regardless of system.
    // A game's Wikidata entry may list a different platform than the ROM's actual system
    // (e.g., Metal Slug X tagged as sony_psx but ROM is on arcade_fbneo).
    // Series data is platform-independent, so cross-system matching is correct.
    let wikidata_entries = series_db::all_entries();
    if wikidata_entries.is_empty() {
        return Vec::new();
    }

    // Build a map of normalized_title -> set of base_titles for games in the library.
    // Multiple normalized forms are added per ROM to maximize match chances:
    // 1. base_title as-is (e.g., "dodonpachi ii - bee storm")
    // 2. display_name derived base_title
    // 3. Subtitle-stripped form (e.g., "dodonpachi ii") — catches cases where
    //    Wikidata uses the short name and the ROM has a subtitle after " - " or " / "
    //
    // We collect ALL base_titles per normalized key (not just the first) because
    // multiple ROMs can share the same normalized form but have different base_titles.
    // E.g., ddp2j ("dodonpachi ii") and ddp2 ("dodonpachi ii - bee storm") both
    // normalize to "dodonpachi ii" — we need game_series entries for BOTH.
    let mut norm_to_bases: HashMap<String, Vec<String>> = HashMap::new();
    let mut seen: HashSet<(String, String)> = HashSet::new(); // (norm_key, base_title) dedup

    for rom in library_entries {
        if rom.base_title.is_empty() {
            continue;
        }
        let normalized = normalize_for_wikidata(&rom.base_title);
        if !normalized.is_empty() && seen.insert((normalized.clone(), rom.base_title.clone())) {
            norm_to_bases
                .entry(normalized.clone())
                .or_default()
                .push(rom.base_title.clone());
        }
        // Subtitle-stripped: "dodonpachi ii - bee storm" -> "dodonpachi ii"
        for sep in [" - ", " / ", ": "] {
            if let Some(prefix) = rom.base_title.split(sep).next() {
                let norm_prefix = normalize_for_wikidata(prefix);
                if norm_prefix.len() >= 4
                    && norm_prefix != normalized
                    && seen.insert((norm_prefix.clone(), rom.base_title.clone()))
                {
                    norm_to_bases
                        .entry(norm_prefix)
                        .or_default()
                        .push(rom.base_title.clone());
                }
            }
        }
        // Also try with display_name for better matching
        if let Some(ref dn) = rom.display_name {
            let norm_dn = normalize_for_wikidata(&title_utils::base_title(dn));
            if !norm_dn.is_empty() && seen.insert((norm_dn.clone(), rom.base_title.clone())) {
                norm_to_bases
                    .entry(norm_dn)
                    .or_default()
                    .push(rom.base_title.clone());
            }
        }
    }

    tracing::debug!(
        "Wikidata series matching for {}: {} norm entries from {} library ROMs, {} wikidata entries",
        system,
        norm_to_bases.len(),
        library_entries.len(),
        wikidata_entries.len()
    );

    let mut series_entries: Vec<SeriesInsert> = Vec::new();

    for entry in wikidata_entries {
        if let Some(base_titles) = norm_to_bases.get(entry.normalized_title)
            && !entry.series_name.is_empty()
        {
            // Resolve follows/followed_by Wikidata titles to library base_titles.
            // If the linked game is in the library, store its base_title for direct join.
            // If not, store the raw Wikidata title for display purposes.
            // Filter out unresolved QID references (e.g., "Q88759").
            let follows = resolve_sequel_link(entry.follows, &norm_to_bases);
            let followed_by = resolve_sequel_link(entry.followed_by, &norm_to_bases);

            for base_title in base_titles {
                series_entries.push(SeriesInsert {
                    system: system.to_string(),
                    base_title: base_title.clone(),
                    series_name: entry.series_name.to_string(),
                    series_order: entry.series_order,
                    source: "wikidata".to_string(),
                    follows_base_title: follows.clone(),
                    followed_by_base_title: followed_by.clone(),
                });
            }
        }
    }

    series_entries
}

/// Resolve a Wikidata sequel/prequel title to a library base_title.
///
/// Returns `Some(base_title)` if the title resolves to a library game,
/// `Some(wikidata_title)` if it's a valid title but not in library,
/// or `None` if the field is empty or an unresolved QID.
fn resolve_sequel_link(
    wikidata_title: &str,
    norm_to_bases: &HashMap<String, Vec<String>>,
) -> Option<String> {
    if wikidata_title.is_empty() {
        return None;
    }
    // Filter out unresolved Wikidata QID references (e.g., "Q88759").
    if wikidata_title.starts_with('Q') && wikidata_title[1..].chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let normalized = normalize_for_wikidata(wikidata_title);
    // Try to resolve to a library base_title.
    if let Some(base_titles) = norm_to_bases.get(&normalized) {
        Some(base_titles[0].clone())
    } else {
        // Not in library — store raw Wikidata title for display.
        Some(wikidata_title.to_string())
    }
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
) -> Vec<AliasInsert> {
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
    let mut aliases: Vec<AliasInsert> = Vec::new();

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
                let resolved = resolve_to_library_title(alt_name, &library_exact, &library_fuzzy);
                if resolved != bt && !resolved.is_empty() {
                    aliases.push(AliasInsert {
                        system: system.clone(),
                        base_title: bt.clone(),
                        alias_name: resolved,
                        alias_region: region.clone(),
                        source: "launchbox".to_string(),
                    });
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
    fn wikidata_series_subtitle_stripped_matching() {
        // ROM has "dodonpachi ii - bee storm" as base_title.
        // Wikidata has "DoDonPachi II" (no subtitle).
        // The subtitle-stripped fallback should match via "dodonpachi ii".
        let mut entry = make_entry("arcade_fbneo", "dodonpachi ii - bee storm");
        entry.display_name = Some("DoDonPachi II - Bee Storm (World, ver. 102)".to_string());
        let result = build_wikidata_series_tuples("arcade_fbneo", &[entry]);
        let has_donpachi = result.iter().any(|s| s.series_name == "DonPachi");
        assert!(
            has_donpachi,
            "Should match DonPachi series via subtitle stripping, got: {:?}",
            result
        );
        // The series entry should use the ROM's actual base_title, not the stripped form.
        let donpachi_entry = result.iter().find(|s| s.series_name == "DonPachi").unwrap();
        assert_eq!(
            donpachi_entry.base_title, "dodonpachi ii - bee storm",
            "Series entry should use the full base_title"
        );
    }

    #[test]
    fn wikidata_series_multi_base_title_per_norm_key() {
        // Bug fix: when multiple ROMs normalize to the same Wikidata key,
        // ALL of them should get game_series entries (not just the first one).
        //
        // ddp2j has base_title "dodonpachi ii" (direct match to Wikidata "dodonpachi ii").
        // ddp2 has base_title "dodonpachi ii - bee storm" (subtitle-stripped to "dodonpachi ii").
        // Both should appear in the series.
        let entries = vec![
            make_entry("arcade_fbneo", "dodonpachi ii"),
            make_entry("arcade_fbneo", "dodonpachi ii - bee storm"),
        ];
        let result = build_wikidata_series_tuples("arcade_fbneo", &entries);
        let donpachi_bts: Vec<&str> = result
            .iter()
            .filter(|s| s.series_name == "DonPachi")
            .map(|s| s.base_title.as_str())
            .collect();
        assert!(
            donpachi_bts.contains(&"dodonpachi ii"),
            "Should have series entry for 'dodonpachi ii', got: {:?}",
            donpachi_bts
        );
        assert!(
            donpachi_bts.contains(&"dodonpachi ii - bee storm"),
            "Should have series entry for 'dodonpachi ii - bee storm', got: {:?}",
            donpachi_bts
        );
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
        let fuzzy: HashMap<String, &str> =
            [(fuzzy_match_key("super mario world"), "super mario world")]
                .into_iter()
                .collect();
        let result = build_tgdb_alias_tuples("nintendo_snes", &exact, &fuzzy);
        for a in &result {
            assert_eq!(a.source, "tgdb");
            assert_eq!(a.system, "nintendo_snes");
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
        for a in &result2 {
            assert_eq!(a.system, "nintendo_nes");
            assert_eq!(a.base_title, "super mario bros");
            assert_eq!(a.source, "launchbox");
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
        for a in &result {
            assert_eq!(a.source, "launchbox");
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
        for s in &result {
            assert_eq!(s.source, "wikidata");
            assert_eq!(s.system, "nintendo_snes");
        }
    }

    #[test]
    fn wikidata_series_donpachi_direct_match() {
        // These base_titles should match Wikidata entries directly (no subtitle stripping needed).
        let entries = vec![
            make_entry("arcade_fbneo", "donpachi"),
            make_entry("arcade_fbneo", "dodonpachi"),
            make_entry("arcade_fbneo", "dodonpachi dai-ou-jou"),
            make_entry("arcade_fbneo", "dodonpachi saidaioujou"),
        ];
        let result = build_wikidata_series_tuples("arcade_fbneo", &entries);
        // All 4 should match the DonPachi series
        let donpachi_count = result
            .iter()
            .filter(|s| s.series_name == "DonPachi")
            .count();
        assert!(
            donpachi_count >= 4,
            "Expected at least 4 DonPachi series matches, got {donpachi_count}. Full results: {:?}",
            result
        );
    }

    #[test]
    fn wikidata_series_donpachi_sequel_links() {
        // Verify that sequel chain data is resolved for DonPachi entries.
        let entries = vec![
            make_entry("arcade_fbneo", "donpachi"),
            make_entry("arcade_fbneo", "dodonpachi"),
            make_entry("arcade_fbneo", "dodonpachi ii - bee storm"),
        ];
        let result = build_wikidata_series_tuples("arcade_fbneo", &entries);

        // DonPachi should have followed_by pointing to "dodonpachi" (resolved library base_title).
        let donpachi = result
            .iter()
            .find(|s| s.base_title == "donpachi")
            .expect("Should have donpachi entry");
        assert_eq!(
            donpachi.followed_by_base_title.as_deref(),
            Some("dodonpachi"),
            "DonPachi's followed_by should resolve to library base_title 'dodonpachi'"
        );

        // DoDonPachi should have follows = "donpachi" and followed_by = "dodonpachi ii - bee storm"
        // (or "DoDonPachi II" if not resolved to library).
        let dodonpachi = result
            .iter()
            .find(|s| s.base_title == "dodonpachi")
            .expect("Should have dodonpachi entry");
        assert_eq!(
            dodonpachi.follows_base_title.as_deref(),
            Some("donpachi"),
            "DoDonPachi's follows should resolve to 'donpachi'"
        );
        // followed_by should be either the library base_title or the raw Wikidata title
        assert!(
            dodonpachi.followed_by_base_title.is_some(),
            "DoDonPachi should have a followed_by link"
        );
    }

    #[test]
    fn wikidata_series_cross_system_matching() {
        // Metal Slug 6 is on Atomiswave (arcade_dc) but Wikidata maps it to arcade_fbneo.
        let entries = vec![make_entry("arcade_dc", "metal slug 6")];
        let result = build_wikidata_series_tuples("arcade_dc", &entries);
        let ms6 = result.iter().find(|s| s.base_title == "metal slug 6");
        assert!(
            ms6.is_some(),
            "Metal Slug 6 on arcade_dc should match Wikidata entry from another system. Got: {:?}",
            result
        );
        assert_eq!(ms6.unwrap().series_name, "Metal Slug");
        assert_eq!(ms6.unwrap().system, "arcade_dc");

        // Metal Slug X is on arcade but Wikidata only maps it to sony_psx.
        let entries = vec![make_entry("arcade_fbneo", "metal slug x - super vehicle-001")];
        let result = build_wikidata_series_tuples("arcade_fbneo", &entries);
        let msx = result.iter().find(|s| s.base_title == "metal slug x - super vehicle-001");
        assert!(
            msx.is_some(),
            "Metal Slug X on arcade should match sony_psx Wikidata entry via subtitle stripping. Got: {:?}",
            result
        );
        assert_eq!(msx.unwrap().series_name, "Metal Slug");
    }
}
