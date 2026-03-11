use super::*;

/// A single result in global search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalSearchResult {
    pub rom_filename: String,
    pub display_name: String,
    pub system: String,
    pub rom_path: String,
    pub genre: String,
    pub is_favorite: bool,
    pub box_art_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub players: Option<u8>,
}

/// A group of search results for a single system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemSearchGroup {
    pub system: String,
    pub system_display: String,
    pub total_matches: usize,
    pub top_results: Vec<GlobalSearchResult>,
}

/// Aggregated global search results across all systems.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalSearchResults {
    pub groups: Vec<SystemSearchGroup>,
    pub total_results: usize,
    pub total_systems: usize,
}

/// Compute a relevance score for a ROM against a search query.
/// Higher = more relevant. Returns 0 for no match.
/// The `region_pref` parameter controls which region gets the highest bonus.
#[cfg(feature = "ssr")]
pub(crate) fn search_score(
    query: &str,
    display_name: &str,
    filename: &str,
    region_pref: replay_control_core::rom_tags::RegionPreference,
) -> u32 {
    let display_lower = display_name.to_lowercase();
    let filename_lower = filename.to_lowercase();

    // Base score from match type
    let base = if display_lower == *query {
        10_000 // exact match on display name
    } else if display_lower.starts_with(query) {
        5_000 // display name starts with query
    } else if display_lower
        .split_whitespace()
        .any(|w| w.starts_with(query))
    {
        2_000 // a word in display name starts with query
    } else if display_lower.contains(query) {
        1_000 // display name contains query
    } else if filename_lower.contains(query) {
        500 // only filename contains query
    } else {
        return 0;
    };

    // Shorter names are more likely the original game
    let length_bonus: u32 = if display_name.len() < 40 { 100 } else { 0 };

    // Tier penalty: deprioritize non-original ROMs
    let (tier, region) = replay_control_core::rom_tags::classify(filename);
    let tier_penalty = match tier {
        replay_control_core::rom_tags::RomTier::Original => 0,
        replay_control_core::rom_tags::RomTier::Revision => 5,
        replay_control_core::rom_tags::RomTier::RegionVariant => 10,
        replay_control_core::rom_tags::RomTier::Translation => 50,
        replay_control_core::rom_tags::RomTier::Unlicensed => 60,
        replay_control_core::rom_tags::RomTier::Homebrew => 100,
        replay_control_core::rom_tags::RomTier::Hack => 200,
        replay_control_core::rom_tags::RomTier::PreRelease => 250,
        replay_control_core::rom_tags::RomTier::Pirate => 300,
    };

    // Region bonus: based on sort_key from user's preference.
    // Lower sort_key = higher bonus.
    let region_bonus: u32 = match region.sort_key(region_pref) {
        0 => 20, // World (or preferred when World is the preference)
        1 => 15, // User's preferred region
        2 => 10, // Second-best major region
        3 => 5,  // Third major region
        _ => 0,  // Other / Unknown
    };

    (base + length_bonus + region_bonus).saturating_sub(tier_penalty)
}

/// Look up the normalized genre for a ROM on a given system.
#[cfg(feature = "ssr")]
pub(crate) fn lookup_genre(system: &str, rom_filename: &str) -> String {
    use replay_control_core::arcade_db;
    use replay_control_core::game_db;
    use replay_control_core::systems::{self, SystemCategory};

    let is_arcade = systems::find_system(system)
        .is_some_and(|s| s.category == SystemCategory::Arcade);

    if is_arcade {
        let stem = rom_filename.strip_suffix(".zip").unwrap_or(rom_filename);
        arcade_db::lookup_arcade_game(stem)
            .map(|info| info.normalized_genre.to_string())
            .unwrap_or_default()
    } else {
        let stem = rom_filename
            .rfind('.')
            .map(|i| &rom_filename[..i])
            .unwrap_or(rom_filename);
        let entry = game_db::lookup_game(system, stem);
        let game = entry.map(|e| e.game).or_else(|| {
            let normalized = game_db::normalize_filename(stem);
            game_db::lookup_by_normalized_title(system, &normalized)
        });
        game.map(|g| g.normalized_genre.to_string())
            .unwrap_or_default()
    }
}

/// Look up the max player count for a ROM on a given system.
/// Returns 0 if unknown.
#[cfg(feature = "ssr")]
pub(crate) fn lookup_players(system: &str, rom_filename: &str) -> u8 {
    use replay_control_core::arcade_db;
    use replay_control_core::game_db;
    use replay_control_core::systems::{self, SystemCategory};

    let is_arcade = systems::find_system(system)
        .is_some_and(|s| s.category == SystemCategory::Arcade);

    if is_arcade {
        let stem = rom_filename.strip_suffix(".zip").unwrap_or(rom_filename);
        arcade_db::lookup_arcade_game(stem)
            .map(|info| info.players)
            .unwrap_or(0)
    } else {
        let stem = rom_filename
            .rfind('.')
            .map(|i| &rom_filename[..i])
            .unwrap_or(rom_filename);
        let entry = game_db::lookup_game(system, stem);
        let game = entry.map(|e| e.game).or_else(|| {
            let normalized = game_db::normalize_filename(stem);
            game_db::lookup_by_normalized_title(system, &normalized)
        });
        game.map(|g| g.players).unwrap_or(0)
    }
}

#[server(prefix = "/sfn")]
pub async fn global_search(
    query: String,
    hide_hacks: bool,
    hide_translations: bool,
    hide_betas: bool,
    hide_clones: bool,
    #[server(default)]
    multiplayer_only: bool,
    genre: String,
    per_system_limit: usize,
) -> Result<GlobalSearchResults, ServerFnError> {
    use replay_control_core::rom_tags;
    use replay_control_core::systems::{self as sys_db, SystemCategory};

    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();
    let region_pref = state.region_preference();
    let systems = state.cache.get_systems(&storage);
    let q = query.to_lowercase();
    let per_system_limit = if per_system_limit == 0 { 3 } else { per_system_limit };

    let mut groups: Vec<SystemSearchGroup> = Vec::new();
    let mut total_results = 0usize;

    for sys in &systems {
        if sys.game_count == 0 {
            continue;
        }

        let is_arcade = sys_db::find_system(&sys.folder_name)
            .is_some_and(|s| s.category == SystemCategory::Arcade);

        let all_roms = match state.cache.get_roms(&storage, &sys.folder_name, region_pref) {
            Ok(roms) => roms,
            Err(_) => continue,
        };

        let mut scored: Vec<(u32, RomEntry)> = all_roms
            .into_iter()
            .filter(|r| {
                // Apply tier-based filters (hacks, translations, betas/protos).
                if hide_hacks || hide_translations || hide_betas {
                    let (tier, _) = rom_tags::classify(&r.game.rom_filename);
                    if hide_hacks && tier == rom_tags::RomTier::Hack {
                        return false;
                    }
                    if hide_translations && tier == rom_tags::RomTier::Translation {
                        return false;
                    }
                    if hide_betas && tier == rom_tags::RomTier::PreRelease {
                        return false;
                    }
                }
                // Apply clone filter (arcade only).
                if hide_clones && is_arcade {
                    use replay_control_core::arcade_db;
                    let stem = r.game.rom_filename.strip_suffix(".zip")
                        .unwrap_or(&r.game.rom_filename);
                    if let Some(info) = arcade_db::lookup_arcade_game(stem) {
                        if info.is_clone {
                            return false;
                        }
                    }
                }
                true
            })
            .filter(|r| {
                // Apply genre filter.
                if genre.is_empty() {
                    return true;
                }
                let rom_genre = lookup_genre(&sys.folder_name, &r.game.rom_filename);
                rom_genre.eq_ignore_ascii_case(&genre)
            })
            .filter(|r| {
                if !multiplayer_only {
                    return true;
                }
                lookup_players(&sys.folder_name, &r.game.rom_filename) >= 2
            })
            .filter_map(|r| {
                if q.is_empty() {
                    // No query: if genre is set or multiplayer filter active, include all matching; otherwise skip.
                    if !genre.is_empty() || multiplayer_only {
                        // Assign a default score based on display name length.
                        let display = r
                            .game
                            .display_name
                            .as_deref()
                            .unwrap_or(&r.game.rom_filename);
                        let score = 1000u32.saturating_sub(display.len() as u32);
                        Some((score, r))
                    } else {
                        None
                    }
                } else {
                    let display = r
                        .game
                        .display_name
                        .as_deref()
                        .unwrap_or(&r.game.rom_filename);
                    let score = search_score(&q, display, &r.game.rom_filename, region_pref);
                    if score > 0 { Some((score, r)) } else { None }
                }
            })
            .collect();

        if scored.is_empty() {
            continue;
        }

        scored.sort_by(|a, b| b.0.cmp(&a.0));
        let match_count = scored.len();
        total_results += match_count;

        // Mark favorites for the top results.
        let mut top_roms: Vec<RomEntry> = scored
            .into_iter()
            .take(per_system_limit)
            .map(|(_, r)| r)
            .collect();

        replay_control_core::roms::mark_favorites(&storage, &sys.folder_name, &mut top_roms);

        // Batch lookup ratings from metadata DB.
        let ratings_map = if let Some(guard) = state.metadata_db() {
            if let Some(db) = guard.as_ref() {
                let filenames: Vec<&str> =
                    top_roms.iter().map(|r| r.game.rom_filename.as_str()).collect();
                db.lookup_ratings(&sys.folder_name, &filenames).unwrap_or_default()
            } else {
                std::collections::HashMap::new()
            }
        } else {
            std::collections::HashMap::new()
        };

        let top_results: Vec<GlobalSearchResult> = top_roms
            .into_iter()
            .map(|mut rom| {
                rom.box_art_url =
                    resolve_box_art_url(&state, &sys.folder_name, &rom.game.rom_filename);
                let genre_str = lookup_genre(&sys.folder_name, &rom.game.rom_filename);
                let players_val = lookup_players(&sys.folder_name, &rom.game.rom_filename);
                let rating = ratings_map
                    .get(&rom.game.rom_filename)
                    .filter(|&&r| r > 0.0)
                    .map(|&r| r as f32);
                GlobalSearchResult {
                    display_name: rom
                        .game
                        .display_name
                        .unwrap_or_else(|| rom.game.rom_filename.clone()),
                    rom_filename: rom.game.rom_filename,
                    system: sys.folder_name.clone(),
                    rom_path: rom.game.rom_path,
                    genre: genre_str,
                    is_favorite: rom.is_favorite,
                    box_art_url: rom.box_art_url,
                    rating,
                    players: if players_val > 0 { Some(players_val) } else { None },
                }
            })
            .collect();

        groups.push(SystemSearchGroup {
            system: sys.folder_name.clone(),
            system_display: sys.display_name.clone(),
            total_matches: match_count,
            top_results,
        });
    }

    // Sort systems by match count descending.
    groups.sort_by(|a, b| b.total_matches.cmp(&a.total_matches));
    let total_systems = groups.len();

    Ok(GlobalSearchResults {
        groups,
        total_results,
        total_systems,
    })
}

#[server(prefix = "/sfn")]
pub async fn get_all_genres() -> Result<Vec<String>, ServerFnError> {
    use std::collections::BTreeSet;

    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();
    let systems = state.cache.get_systems(&storage);
    let mut genres = BTreeSet::new();

    for sys in &systems {
        if sys.game_count == 0 {
            continue;
        }
        let roms = match state.cache.get_roms(&storage, &sys.folder_name, state.region_preference()) {
            Ok(roms) => roms,
            Err(_) => continue,
        };
        for rom in &roms {
            let g = lookup_genre(&sys.folder_name, &rom.game.rom_filename);
            if !g.is_empty() {
                genres.insert(g);
            }
        }
    }

    Ok(genres.into_iter().collect())
}

/// Get genres available for a specific system.
#[server(prefix = "/sfn")]
pub async fn get_system_genres(system: String) -> Result<Vec<String>, ServerFnError> {
    use std::collections::BTreeSet;

    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();
    let roms = state
        .cache
        .get_roms(&storage, &system, state.region_preference())
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    let mut genres = BTreeSet::new();
    for rom in &roms {
        let g = lookup_genre(&system, &rom.game.rom_filename);
        if !g.is_empty() {
            genres.insert(g);
        }
    }

    Ok(genres.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::search_score;
    use replay_control_core::rom_tags::RegionPreference;

    /// Default preference for tests (matches pre-preference behavior).
    const PREF: RegionPreference = RegionPreference::Usa;

    // --- Exact match (10_000 base) ---

    #[test]
    fn exact_match_display_name() {
        let score = search_score("tetris", "Tetris", "Tetris (USA).nes", PREF);
        assert!(score >= 10_000, "Exact match should score >= 10000, got {score}");
    }

    #[test]
    fn exact_match_is_case_insensitive() {
        let score = search_score("tetris", "Tetris", "Tetris (USA).nes", PREF);
        let score2 = search_score("tetris", "TETRIS", "TETRIS.nes", PREF);
        assert!(score >= 10_000);
        assert!(score2 >= 10_000);
    }

    // --- Prefix match (5_000 base) ---

    #[test]
    fn prefix_match() {
        let score = search_score("super", "Super Mario World", "Super Mario World (USA).sfc", PREF);
        assert!(
            (5_000..10_000).contains(&score),
            "Prefix match should score in 5000..10000, got {score}"
        );
    }

    // --- Word boundary match (2_000 base) ---

    #[test]
    fn word_boundary_match() {
        let score = search_score("mario", "Super Mario World", "Super Mario World (USA).sfc", PREF);
        assert!(
            (2_000..5_000).contains(&score),
            "Word boundary match should score in 2000..5000, got {score}"
        );
    }

    // --- Substring match (1_000 base) ---

    #[test]
    fn substring_match() {
        // "ari" is inside "Mario" but doesn't start a word
        let score = search_score("ari", "Super Mario World", "Super Mario World (USA).sfc", PREF);
        assert!(
            (1_000..2_000).contains(&score),
            "Substring match should score in 1000..2000, got {score}"
        );
    }

    // --- Filename-only match (500 base) ---

    #[test]
    fn filename_only_match() {
        // Query matches filename but not display name
        let score = search_score("usa", "Tetris", "Tetris (USA).nes", PREF);
        assert!(
            (500..1_000).contains(&score),
            "Filename-only match should score in 500..1000, got {score}"
        );
    }

    // --- No match ---

    #[test]
    fn no_match_returns_zero() {
        let score = search_score("zzzznotfound", "Tetris", "Tetris (USA).nes", PREF);
        assert_eq!(score, 0);
    }

    #[test]
    fn empty_query_no_match() {
        let score = search_score("", "Tetris", "Tetris (USA).nes", PREF);
        // Empty query matches everything via contains(""), so it should score > 0
        // (exact match since "tetris".contains("") is true, but actually "" == display_lower is false)
        // Let's verify the actual behavior
        assert!(score > 0, "Empty string is contained in all strings");
    }

    // --- Tier ordering ---

    #[test]
    fn exact_beats_prefix() {
        let exact = search_score("tetris", "Tetris", "Tetris (USA).nes", PREF);
        let prefix = search_score("tetris", "Tetris Plus", "Tetris Plus (USA).nes", PREF);
        assert!(exact > prefix, "Exact ({exact}) should beat prefix ({prefix})");
    }

    #[test]
    fn prefix_beats_word_boundary() {
        let prefix = search_score("super", "Super Mario World", "Super Mario World (USA).sfc", PREF);
        let word = search_score("mario", "Super Mario World", "Super Mario World (USA).sfc", PREF);
        assert!(
            prefix > word,
            "Prefix ({prefix}) should beat word boundary ({word})"
        );
    }

    #[test]
    fn word_boundary_beats_substring() {
        let word = search_score("mario", "Super Mario World", "Super Mario World (USA).sfc", PREF);
        let substr = search_score("ari", "Super Mario World", "Super Mario World (USA).sfc", PREF);
        assert!(
            word > substr,
            "Word boundary ({word}) should beat substring ({substr})"
        );
    }

    #[test]
    fn substring_beats_filename_only() {
        let substr = search_score("ari", "Super Mario World", "Super Mario World (USA).sfc", PREF);
        let filename = search_score("usa", "Tetris", "Tetris (USA).nes", PREF);
        assert!(
            substr > filename,
            "Substring ({substr}) should beat filename-only ({filename})"
        );
    }

    // --- Length bonus ---

    #[test]
    fn short_name_gets_length_bonus() {
        let short = search_score("mario", "Super Mario World", "Super Mario World (USA).sfc", PREF);
        let long = search_score(
            "mario",
            "Super Mario World - Long Subtitle That Makes It Over 40 Characters",
            "Super Mario World - Long Subtitle (USA).sfc",
            PREF,
        );
        assert!(
            short > long,
            "Short name ({short}) should beat long name ({long}) due to length bonus"
        );
    }

    // --- Tier penalties ---

    #[test]
    fn hack_is_penalized() {
        let original = search_score("mario", "Super Mario World", "Super Mario World (USA).sfc", PREF);
        let hack =
            search_score("mario", "Super Mario World", "Super Mario World (Hack).sfc", PREF);
        assert!(
            original > hack,
            "Original ({original}) should beat hack ({hack})"
        );
    }

    #[test]
    fn translation_is_penalized() {
        let original = search_score("mario", "Super Mario World", "Super Mario World (USA).sfc", PREF);
        let translated = search_score(
            "mario",
            "Super Mario World",
            "Super Mario World (Traducido Es).sfc",
            PREF,
        );
        assert!(
            original > translated,
            "Original ({original}) should beat translation ({translated})"
        );
    }

    // --- Special characters ---

    #[test]
    fn special_characters_in_query() {
        let score = search_score(
            "asterix & obelix",
            "Asterix & Obelix",
            "Asterix & Obelix (Europe).sfc",
            PREF,
        );
        assert!(score >= 10_000, "Exact match with special chars should work, got {score}");
    }

    #[test]
    fn query_with_dash() {
        let score = search_score(
            "x-men",
            "X-Men - Mutant Apocalypse",
            "X-Men - Mutant Apocalypse (USA).sfc",
            PREF,
        );
        assert!(score > 0, "Query with dash should match");
    }

    // --- Region preference affects scoring ---

    #[test]
    fn japan_preference_boosts_japan_roms() {
        let usa_with_usa_pref =
            search_score("mario", "Super Mario World", "Super Mario World (USA).sfc", RegionPreference::Usa);
        let japan_with_japan_pref =
            search_score("mario", "Super Mario World", "Super Mario World (Japan).sfc", RegionPreference::Japan);
        // Both should get sort_key=1 (preferred region), so equal region bonus.
        assert_eq!(usa_with_usa_pref, japan_with_japan_pref);
    }

    #[test]
    fn preferred_region_beats_non_preferred() {
        let japan_pref = RegionPreference::Japan;
        let japan_score =
            search_score("mario", "Super Mario World", "Super Mario World (Japan).sfc", japan_pref);
        let usa_score =
            search_score("mario", "Super Mario World", "Super Mario World (USA).sfc", japan_pref);
        assert!(
            japan_score > usa_score,
            "Japan ({japan_score}) should beat USA ({usa_score}) with Japan preference"
        );
    }

    #[test]
    fn europe_preference_puts_europe_before_usa() {
        let pref = RegionPreference::Europe;
        let europe_score =
            search_score("mario", "Super Mario World", "Super Mario World (Europe).sfc", pref);
        let usa_score =
            search_score("mario", "Super Mario World", "Super Mario World (USA).sfc", pref);
        assert!(
            europe_score > usa_score,
            "Europe ({europe_score}) should beat USA ({usa_score}) with Europe preference"
        );
    }
}

/// Pick a random game across all systems.
/// Weighted by system game count so larger collections get proportionally more picks.
/// Returns (system_folder_name, rom_filename).
#[server(prefix = "/sfn")]
pub async fn random_game() -> Result<(String, String), ServerFnError> {
    use rand::Rng;

    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();
    let systems = state.cache.get_systems(&storage);

    // Build a weighted list: (system_folder, game_count).
    let weighted: Vec<(String, usize)> = systems
        .iter()
        .filter(|s| s.game_count > 0)
        .map(|s| (s.folder_name.clone(), s.game_count))
        .collect();

    if weighted.is_empty() {
        return Err(ServerFnError::new("No games available"));
    }

    let total: usize = weighted.iter().map(|(_, c)| c).sum();
    let mut rng = rand::rng();
    let pick = rng.random_range(0..total);

    let mut cumulative = 0;
    let mut chosen_system = &weighted[0].0;
    for (sys, count) in &weighted {
        cumulative += count;
        if pick < cumulative {
            chosen_system = sys;
            break;
        }
    }

    let roms = state
        .cache
        .get_roms(&storage, chosen_system, state.region_preference())
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    if roms.is_empty() {
        return Err(ServerFnError::new("No ROMs in selected system"));
    }

    let idx = rng.random_range(0..roms.len());
    let rom = &roms[idx];
    Ok((chosen_system.clone(), rom.game.rom_filename.clone()))
}
