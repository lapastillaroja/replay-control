use super::*;

/// Developer search result: a matched developer with their games.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeveloperSearchResult {
    pub developer_name: String,
    pub total_count: usize,
    pub games: Vec<GlobalSearchResult>,
    pub other_developers: Vec<DeveloperMatch>,
}

/// An additional developer that matched the search query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeveloperMatch {
    pub name: String,
    pub game_count: usize,
}

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

/// Split a string into words on whitespace and hyphens, stripping trailing punctuation.
#[cfg(feature = "ssr")]
fn split_into_words(s: &str) -> Vec<&str> {
    s.split(|c: char| c.is_whitespace() || c == '-')
        .map(|w| w.trim_end_matches(['.', ',', ':', '!', '?', ';']))
        .filter(|w| !w.is_empty())
        .collect()
}

/// Check whether query words appear in the same relative order in target words.
#[cfg(feature = "ssr")]
fn words_in_order(query_words: &[&str], title_words: &[&str]) -> bool {
    let mut last_pos: Option<usize> = None;
    for qw in query_words {
        let found = title_words
            .iter()
            .enumerate()
            .position(|(i, tw)| tw.starts_with(qw) && last_pos.is_none_or(|lp| i > lp));
        match found {
            Some(pos) => last_pos = Some(pos),
            None => return false,
        }
    }
    true
}

/// Count pairs of consecutive query words that are also adjacent in the title.
#[cfg(feature = "ssr")]
fn count_adjacent_pairs(query_words: &[&str], title_words: &[&str]) -> u32 {
    if query_words.len() < 2 {
        return 0;
    }
    let mut pairs = 0u32;
    // Find positions of each query word in title
    let positions: Vec<Option<usize>> = query_words
        .iter()
        .map(|qw| title_words.iter().position(|tw| tw.starts_with(qw)))
        .collect();
    for i in 0..positions.len() - 1 {
        if let (Some(p1), Some(p2)) = (positions[i], positions[i + 1])
            && p2 == p1 + 1
        {
            pairs += 1;
        }
    }
    pairs
}

/// Count query words that match a title word exactly (not just starts_with).
#[cfg(feature = "ssr")]
fn count_exact_word_matches(query_words: &[&str], title_words: &[&str]) -> u32 {
    query_words
        .iter()
        .filter(|qw| title_words.iter().any(|tw| tw == *qw))
        .count() as u32
}

/// Try word-level matching: all query words must appear (via starts_with) in the target words.
/// Returns (base_score, is_filename_only) or None if not all words match.
#[cfg(feature = "ssr")]
fn try_word_match(
    query_words: &[&str],
    display_words: &[&str],
    filename_words: &[&str],
) -> Option<(u32, bool)> {
    let display_matched = query_words
        .iter()
        .filter(|qw| display_words.iter().any(|tw| tw.starts_with(**qw)))
        .count();

    if display_matched == query_words.len() {
        // All words found in display name
        let order_bonus: u32 = if words_in_order(query_words, display_words) {
            50
        } else {
            0
        };
        let coverage = display_matched as f32 / display_words.len().max(1) as f32;
        let coverage_bonus = (coverage * 50.0) as u32;
        let adjacency_bonus = count_adjacent_pairs(query_words, display_words) * 20;
        let exact_bonus = count_exact_word_matches(query_words, display_words) * 30;
        return Some((
            400 + order_bonus + coverage_bonus + adjacency_bonus + exact_bonus,
            false,
        ));
    }

    // Try filename
    let filename_matched = query_words
        .iter()
        .filter(|qw| filename_words.iter().any(|tw| tw.starts_with(**qw)))
        .count();

    if filename_matched == query_words.len() {
        let order_bonus: u32 = if words_in_order(query_words, filename_words) {
            50
        } else {
            0
        };
        let coverage = filename_matched as f32 / filename_words.len().max(1) as f32;
        let coverage_bonus = (coverage * 50.0) as u32;
        let adjacency_bonus = count_adjacent_pairs(query_words, filename_words) * 20;
        let exact_bonus = count_exact_word_matches(query_words, filename_words) * 30;
        return Some((
            300 + order_bonus + coverage_bonus + adjacency_bonus + exact_bonus,
            true,
        ));
    }

    None
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
    region_secondary: Option<replay_control_core::rom_tags::RegionPreference>,
) -> u32 {
    let display_lower = display_name.to_lowercase();
    let filename_lower = filename.to_lowercase();

    // Check whether the match ends at a word boundary (next char is non-alphanumeric or end-of-string).
    let is_word_boundary_at = |s: &str, offset: usize| -> bool {
        s[offset..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric())
    };

    // Check whether `needle` appears in `haystack` at a clean trailing word boundary
    // (i.e., at least one occurrence where the character after the match is non-alphanumeric or end-of-string).
    let contains_at_word_boundary = |haystack: &str, needle: &str| -> bool {
        let mut start = 0;
        while let Some(pos) = haystack[start..].find(needle) {
            let abs_pos = start + pos;
            if is_word_boundary_at(haystack, abs_pos + needle.len()) {
                return true;
            }
            start = abs_pos + 1;
        }
        false
    };

    // Base score from match type
    let base = if display_lower == *query {
        10_000 // exact match on display name
    } else if display_lower.starts_with(query) && is_word_boundary_at(&display_lower, query.len()) {
        5_000 // display name starts with query (clean word boundary)
    } else if display_lower
        .split_whitespace()
        .any(|w| w.starts_with(query) && is_word_boundary_at(w, query.len()))
    {
        2_000 // a word in display name starts with query (clean word boundary)
    } else if display_lower.contains(query) {
        // For multi-word queries, demote mid-word matches (e.g. "sonic 3" in "sonic 3d blast").
        if query.contains(' ') && !contains_at_word_boundary(&display_lower, query) {
            500 // multi-word query mid-word match (e.g. "sonic 3" in "sonic 3d blast")
        } else {
            1_000
        }
    } else if filename_lower.contains(query) {
        if query.contains(' ') && !contains_at_word_boundary(&filename_lower, query) {
            250 // multi-word query mid-word match in filename
        } else {
            500
        }
    } else {
        // Fallback: word-level matching for multi-word queries
        let word_score = if !query.contains(' ') {
            None
        } else {
            let query_words: Vec<&str> = split_into_words(query);
            if query_words.len() < 2 {
                None
            } else {
                let display_words: Vec<&str> = split_into_words(&display_lower);
                let filename_words: Vec<&str> = split_into_words(&filename_lower);
                try_word_match(&query_words, &display_words, &filename_words)
            }
        };

        match word_score {
            Some((word_base, _)) => {
                // Apply the same bonuses/penalties as other tiers
                let length_bonus: u32 = if display_name.len() < 40 { 100 } else { 0 };
                let (tier, region, _) = replay_control_core::rom_tags::classify(filename);
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
                let region_bonus: u32 = match region.sort_key(region_pref, region_secondary) {
                    0 => 20,
                    1 => 15,
                    2 => 10,
                    3 => 5,
                    _ => 0,
                };
                return (word_base + length_bonus + region_bonus).saturating_sub(tier_penalty);
            }
            None => return 0,
        }
    };

    // Shorter names are more likely the original game
    let length_bonus: u32 = if display_name.len() < 40 { 100 } else { 0 };

    // Tier penalty: deprioritize non-original ROMs
    let (tier, region, _) = replay_control_core::rom_tags::classify(filename);
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
    let region_bonus: u32 = match region.sort_key(region_pref, region_secondary) {
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

    let is_arcade =
        systems::find_system(system).is_some_and(|s| s.category == SystemCategory::Arcade);

    let baked_genre = if is_arcade {
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
    };

    if !baked_genre.is_empty() {
        return baked_genre;
    }

    // Fallback: check LaunchBox metadata for genre.
    let state = leptos::prelude::expect_context::<crate::api::AppState>();
    if let Some(guard) = state.metadata_db()
        && let Some(db) = guard.as_ref()
        && let Ok(Some(meta)) = db.lookup(system, rom_filename)
        && let Some(genre) = meta.genre
        && !genre.is_empty()
    {
        return genre;
    }

    String::new()
}

/// Look up the max player count for a ROM on a given system.
/// Returns 0 if unknown.
#[cfg(feature = "ssr")]
pub(crate) fn lookup_players(system: &str, rom_filename: &str) -> u8 {
    use replay_control_core::arcade_db;
    use replay_control_core::game_db;
    use replay_control_core::systems::{self, SystemCategory};

    let is_arcade =
        systems::find_system(system).is_some_and(|s| s.category == SystemCategory::Arcade);

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

#[allow(clippy::too_many_arguments)]
#[server(prefix = "/sfn")]
pub async fn global_search(
    query: String,
    hide_hacks: bool,
    hide_translations: bool,
    hide_betas: bool,
    hide_clones: bool,
    #[server(default)] multiplayer_only: bool,
    #[server(default)] min_rating: Option<f32>,
    genre: String,
    per_system_limit: usize,
) -> Result<GlobalSearchResults, ServerFnError> {
    use replay_control_core::rom_tags;
    use replay_control_core::systems::{self as sys_db, SystemCategory};

    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();
    let region_pref = state.region_preference();
    let region_secondary = state.region_preference_secondary();
    let systems = state.cache.get_systems(&storage);
    let q = query.to_lowercase();
    let per_system_limit = if per_system_limit == 0 {
        3
    } else {
        per_system_limit
    };

    // Search aliases for cross-name expansion (e.g., "Bare Knuckle" -> "Streets of Rage").
    let alias_hits: std::collections::HashSet<(String, String)> = if !q.is_empty() {
        state
            .cache
            .with_db_read(&storage, |db| {
                db.search_aliases(&q)
                    .unwrap_or_default()
                    .into_iter()
                    .collect()
            })
            .unwrap_or_default()
    } else {
        std::collections::HashSet::new()
    };

    // Build a set of base_titles per system that matched via alias.
    let alias_base_titles: std::collections::HashMap<String, std::collections::HashSet<String>> = {
        let mut map: std::collections::HashMap<String, std::collections::HashSet<String>> =
            std::collections::HashMap::new();
        for (sys, bt) in &alias_hits {
            map.entry(sys.clone()).or_default().insert(bt.clone());
        }
        map
    };

    let mut groups: Vec<SystemSearchGroup> = Vec::new();
    let mut total_results = 0usize;

    for sys in &systems {
        if sys.game_count == 0 {
            continue;
        }

        let is_arcade = sys_db::find_system(&sys.folder_name)
            .is_some_and(|s| s.category == SystemCategory::Arcade);

        let all_roms =
            match state
                .cache
                .get_roms(&storage, &sys.folder_name, region_pref, region_secondary)
            {
                Ok(roms) => roms,
                Err(_) => continue,
            };

        // Batch-load genre groups and base_titles for this system.
        // Developer matching is handled separately by `search_by_developer`.
        let (system_genre_groups, system_base_titles): (
            std::collections::HashMap<String, String>,
            std::collections::HashMap<String, String>,
        ) = state
            .cache
            .with_db_read(&storage, |db| {
                db.load_system_entries(&sys.folder_name)
                    .map(|entries| {
                        let genres: std::collections::HashMap<String, String> = entries
                            .iter()
                            .filter(|e| !e.genre_group.is_empty())
                            .map(|e| (e.rom_filename.clone(), e.genre_group.clone()))
                            .collect();
                        let base_titles: std::collections::HashMap<String, String> = entries
                            .iter()
                            .filter(|e| !e.base_title.is_empty())
                            .map(|e| (e.rom_filename.clone(), e.base_title.clone()))
                            .collect();
                        (genres, base_titles)
                    })
                    .unwrap_or_default()
            })
            .unwrap_or_default();

        // Batch-load ratings for this system when a minimum rating filter is active.
        let system_ratings = if min_rating.is_some() {
            if let Some(guard) = state.metadata_db() {
                if let Some(db) = guard.as_ref() {
                    db.system_ratings(&sys.folder_name).unwrap_or_default()
                } else {
                    std::collections::HashMap::new()
                }
            } else {
                std::collections::HashMap::new()
            }
        } else {
            std::collections::HashMap::new()
        };

        let mut scored: Vec<(u32, &RomEntry)> = all_roms
            .iter()
            .filter(|r| {
                // Apply tier-based filters (hacks, translations, betas/protos).
                if hide_hacks || hide_translations || hide_betas {
                    let (tier, _, _) = rom_tags::classify(&r.game.rom_filename);
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
                    let stem = r
                        .game
                        .rom_filename
                        .strip_suffix(".zip")
                        .unwrap_or(&r.game.rom_filename);
                    if let Some(info) = arcade_db::lookup_arcade_game(stem)
                        && info.is_clone
                    {
                        return false;
                    }
                }
                true
            })
            .filter(|r| {
                // Apply genre filter using genre_group from game_library.
                if genre.is_empty() {
                    return true;
                }
                system_genre_groups
                    .get(&r.game.rom_filename)
                    .is_some_and(|gg| gg.eq_ignore_ascii_case(&genre))
            })
            .filter(|r| {
                if !multiplayer_only {
                    return true;
                }
                lookup_players(&sys.folder_name, &r.game.rom_filename) >= 2
            })
            .filter(|r| {
                if let Some(threshold) = min_rating {
                    system_ratings
                        .get(&r.game.rom_filename)
                        .is_some_and(|&rating| rating >= threshold as f64)
                } else {
                    true
                }
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
                    let mut score = search_score(
                        &q,
                        display,
                        &r.game.rom_filename,
                        region_pref,
                        region_secondary,
                    );

                    // Alias expansion: if this ROM's base_title was found via alias search,
                    // give it a minimum score so it appears in results.
                    if score == 0
                        && let Some(system_aliases) = alias_base_titles.get(&sys.folder_name)
                        && let Some(bt) = system_base_titles.get(&r.game.rom_filename)
                        && system_aliases.contains(bt)
                    {
                        // Score it like a word-level match (below substring tier).
                        score = 350;
                    }

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
            .map(|(_, r)| r.clone())
            .collect();

        replay_control_core::roms::mark_favorites(&storage, &sys.folder_name, &mut top_roms);

        // Batch lookup ratings from metadata DB.
        let ratings_map = if let Some(guard) = state.metadata_db() {
            if let Some(db) = guard.as_ref() {
                let filenames: Vec<&str> = top_roms
                    .iter()
                    .map(|r| r.game.rom_filename.as_str())
                    .collect();
                db.lookup_ratings(&sys.folder_name, &filenames)
                    .unwrap_or_default()
            } else {
                std::collections::HashMap::new()
            }
        } else {
            std::collections::HashMap::new()
        };

        let image_index = state.cache.get_image_index(&state, &sys.folder_name);
        let top_results: Vec<GlobalSearchResult> = top_roms
            .into_iter()
            .map(|rom| {
                let box_art_url = state.cache.resolve_box_art(
                    &state,
                    &image_index,
                    &sys.folder_name,
                    &rom.game.rom_filename,
                );
                // Use genre_group from batch-loaded map; fall back to lookup_genre.
                let genre_str = system_genre_groups
                    .get(&rom.game.rom_filename)
                    .cloned()
                    .unwrap_or_else(|| lookup_genre(&sys.folder_name, &rom.game.rom_filename));
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
                    box_art_url,
                    rating,
                    players: if players_val > 0 {
                        Some(players_val)
                    } else {
                        None
                    },
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
    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();

    // Use a single SQL query on game_library instead of iterating all ROMs.
    let genres = state
        .cache
        .with_db_read(&storage, |db| db.all_genre_groups().unwrap_or_default())
        .unwrap_or_default();

    Ok(genres)
}

/// Get genres available for a specific system.
#[server(prefix = "/sfn")]
pub async fn get_system_genres(system: String) -> Result<Vec<String>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();

    // Use a single SQL query on game_library instead of iterating all ROMs.
    let genres = state
        .cache
        .with_db_read(&storage, |db| {
            db.system_genre_groups(&system).unwrap_or_default()
        })
        .unwrap_or_default();

    Ok(genres)
}

/// Search for a developer matching the query. If found, returns the developer name,
/// total game count, and up to `limit` games with box art resolved.
#[server(prefix = "/sfn")]
pub async fn search_by_developer(
    query: String,
    limit: usize,
) -> Result<Option<DeveloperSearchResult>, ServerFnError> {
    let q = query.trim().to_lowercase();
    if q.len() < 2 {
        return Ok(None);
    }

    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();
    let region_pref = state.region_preference();
    let region_secondary = state.region_preference_secondary();
    let region_str = region_pref.as_str();
    let region_secondary_str = region_secondary.map(|r| r.as_str()).unwrap_or("");
    let limit = limit.clamp(1, 30);

    // Single DB access: find matching developers, then fetch games for the top match.
    let db_result = state.cache.with_db_read(&storage, |db| {
        let matches = db.find_developer_matches(&q).unwrap_or_default();
        if matches.is_empty() {
            return None;
        }

        let (top_dev, top_count) = &matches[0];
        let games = db
            .games_by_developer(top_dev, limit, region_str, region_secondary_str)
            .unwrap_or_default();

        let other_developers: Vec<DeveloperMatch> = matches[1..]
            .iter()
            .map(|(name, count)| DeveloperMatch {
                name: name.clone(),
                game_count: *count,
            })
            .collect();

        Some((top_dev.clone(), *top_count, games, other_developers))
    });

    let Some(Some((developer_name, total_count, game_entries, other_developers))) = db_result
    else {
        return Ok(None);
    };

    if game_entries.is_empty() {
        return Ok(None);
    }

    let mut image_indexes: std::collections::HashMap<
        String,
        std::sync::Arc<crate::api::cache::ImageIndex>,
    > = std::collections::HashMap::new();
    let games: Vec<GlobalSearchResult> = game_entries
        .into_iter()
        .map(|entry| {
            let index = image_indexes
                .entry(entry.system.clone())
                .or_insert_with(|| state.cache.get_image_index(&state, &entry.system));
            let box_art_url =
                state
                    .cache
                    .resolve_box_art(&state, index, &entry.system, &entry.rom_filename);
            GlobalSearchResult {
                display_name: entry.display_name.unwrap_or_else(|| entry.rom_filename.clone()),
                system: entry.system,
                rom_filename: entry.rom_filename,
                rom_path: entry.rom_path,
                genre: entry.genre.unwrap_or_default(),
                is_favorite: false,
                box_art_url,
                rating: entry.rating,
                players: entry.players,
            }
        })
        .collect();

    Ok(Some(DeveloperSearchResult {
        developer_name,
        total_count,
        games,
        other_developers,
    }))
}

/// A system that a developer has games on, with game count and display name.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeveloperSystem {
    pub system: String,
    pub system_display: String,
    pub game_count: usize,
}

/// Response for the developer game list page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeveloperPageData {
    pub roms: Vec<RomListEntry>,
    pub total: usize,
    pub has_more: bool,
    pub developer: String,
    pub systems: Vec<DeveloperSystem>,
}

/// Get genres available for a developer's games, optionally filtered by system.
#[server(prefix = "/sfn")]
pub async fn get_developer_genres(
    developer: String,
    #[server(default)] system: String,
) -> Result<Vec<String>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();

    let system_filter = if system.is_empty() {
        None
    } else {
        Some(system.as_str())
    };

    let genres = state
        .cache
        .with_db_read(&storage, |db| {
            db.developer_genre_groups(&developer, system_filter)
                .unwrap_or_default()
        })
        .unwrap_or_default();

    Ok(genres)
}

/// Get paginated game list for a developer, with optional system and content filters.
#[allow(clippy::too_many_arguments)]
#[server(prefix = "/sfn")]
pub async fn get_developer_games(
    developer: String,
    #[server(default)] system: String,
    offset: usize,
    limit: usize,
    #[server(default)] hide_hacks: bool,
    #[server(default)] hide_translations: bool,
    #[server(default)] hide_clones: bool,
    #[server(default)] multiplayer_only: bool,
    #[server(default)] genre: String,
    #[server(default)] min_rating: Option<f32>,
) -> Result<DeveloperPageData, ServerFnError> {
    use replay_control_core::metadata_db::DeveloperGamesFilter;
    use replay_control_core::systems as sys_db;

    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();
    let region_pref = state.region_preference();
    let region_secondary = state.region_preference_secondary();
    let region_str = region_pref.as_str();
    let region_secondary_str = region_secondary.map(|r| r.as_str()).unwrap_or("");
    let limit = limit.clamp(1, 200);

    let system_filter = if system.is_empty() {
        None
    } else {
        Some(system.as_str())
    };

    let filters = DeveloperGamesFilter {
        hide_hacks,
        hide_translations,
        hide_clones,
        multiplayer_only,
        genre: &genre,
        min_rating: min_rating.map(|r| r as f64),
    };

    // Fetch systems and paginated games in one DB session.
    let db_result = state.cache.with_db_read(&storage, |db| {
        let systems_raw = db.developer_systems(&developer).unwrap_or_default();
        let (entries, total) = db
            .developer_games_paginated(
                &developer,
                system_filter,
                offset,
                limit,
                region_str,
                region_secondary_str,
                &filters,
            )
            .unwrap_or_default();
        (systems_raw, entries, total)
    });

    let Some((systems_raw, entries, total)) = db_result else {
        return Ok(DeveloperPageData {
            roms: Vec::new(),
            total: 0,
            has_more: false,
            developer,
            systems: Vec::new(),
        });
    };

    // Convert system folder names to display names.
    let systems: Vec<DeveloperSystem> = systems_raw
        .into_iter()
        .map(|(sys, count)| {
            let display = sys_db::find_system(&sys)
                .map(|s| s.display_name.to_string())
                .unwrap_or_else(|| sys.clone());
            DeveloperSystem {
                system: sys,
                system_display: display,
                game_count: count,
            }
        })
        .collect();

    let has_more = offset + entries.len() < total;

    // Collect the set of distinct systems in this page to batch-load favorites.
    let page_systems: std::collections::HashSet<&str> =
        entries.iter().map(|e| e.system.as_str()).collect();
    let fav_sets: std::collections::HashMap<String, std::collections::HashSet<String>> =
        page_systems
            .into_iter()
            .map(|sys| {
                let set = state.cache.get_favorites_set(&storage, sys);
                (sys.to_string(), set)
            })
            .collect();

    // Convert GameEntry -> RomListEntry with box art resolution and favorites.
    let mut image_indexes: std::collections::HashMap<
        String,
        std::sync::Arc<crate::api::cache::ImageIndex>,
    > = std::collections::HashMap::new();
    let list_entries: Vec<RomListEntry> = entries
        .into_iter()
        .map(|entry| {
            let index = image_indexes
                .entry(entry.system.clone())
                .or_insert_with(|| state.cache.get_image_index(&state, &entry.system));
            let box_art_url =
                state
                    .cache
                    .resolve_box_art(&state, index, &entry.system, &entry.rom_filename);
            let is_favorite = fav_sets
                .get(&entry.system)
                .is_some_and(|set| set.contains(&entry.rom_filename));
            RomListEntry {
                display_name: entry
                    .display_name
                    .unwrap_or_else(|| entry.rom_filename.clone()),
                system: entry.system,
                rom_filename: entry.rom_filename,
                rom_path: entry.rom_path,
                size_bytes: entry.size_bytes,
                is_m3u: entry.is_m3u,
                is_favorite,
                box_art_url,
                driver_status: entry.driver_status,
                rating: entry.rating,
                players: entry.players,
                genre: entry.genre.unwrap_or_default(),
            }
        })
        .collect();

    Ok(DeveloperPageData {
        roms: list_entries,
        total,
        has_more,
        developer,
        systems,
    })
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use super::search_score;
    use replay_control_core::rom_tags::RegionPreference;

    /// Default preference for tests (matches pre-preference behavior).
    const PREF: RegionPreference = RegionPreference::Usa;
    /// No secondary preference for most tests.
    const SEC: Option<RegionPreference> = None;

    // --- Exact match (10_000 base) ---

    #[test]
    fn exact_match_display_name() {
        let score = search_score("tetris", "Tetris", "Tetris (USA).nes", PREF, SEC);
        assert!(
            score >= 10_000,
            "Exact match should score >= 10000, got {score}"
        );
    }

    #[test]
    fn exact_match_is_case_insensitive() {
        let score = search_score("tetris", "Tetris", "Tetris (USA).nes", PREF, SEC);
        let score2 = search_score("tetris", "TETRIS", "TETRIS.nes", PREF, SEC);
        assert!(score >= 10_000);
        assert!(score2 >= 10_000);
    }

    // --- Prefix match (5_000 base) ---

    #[test]
    fn prefix_match() {
        let score = search_score(
            "super",
            "Super Mario World",
            "Super Mario World (USA).sfc",
            PREF,
            SEC,
        );
        assert!(
            (5_000..10_000).contains(&score),
            "Prefix match should score in 5000..10000, got {score}"
        );
    }

    // --- Word boundary match (2_000 base) ---

    #[test]
    fn word_boundary_match() {
        let score = search_score(
            "mario",
            "Super Mario World",
            "Super Mario World (USA).sfc",
            PREF,
            SEC,
        );
        assert!(
            (2_000..5_000).contains(&score),
            "Word boundary match should score in 2000..5000, got {score}"
        );
    }

    // --- Substring match (1_000 base) ---

    #[test]
    fn substring_match() {
        // "ari" is inside "Mario" but doesn't start a word
        let score = search_score(
            "ari",
            "Super Mario World",
            "Super Mario World (USA).sfc",
            PREF,
            SEC,
        );
        assert!(
            (1_000..2_000).contains(&score),
            "Substring match should score in 1000..2000, got {score}"
        );
    }

    // --- Filename-only match (500 base) ---

    #[test]
    fn filename_only_match() {
        // Query matches filename but not display name
        let score = search_score("usa", "Tetris", "Tetris (USA).nes", PREF, SEC);
        assert!(
            (500..1_000).contains(&score),
            "Filename-only match should score in 500..1000, got {score}"
        );
    }

    // --- No match ---

    #[test]
    fn no_match_returns_zero() {
        let score = search_score("zzzznotfound", "Tetris", "Tetris (USA).nes", PREF, SEC);
        assert_eq!(score, 0);
    }

    #[test]
    fn empty_query_no_match() {
        let score = search_score("", "Tetris", "Tetris (USA).nes", PREF, SEC);
        // Empty query matches everything via contains(""), so it should score > 0
        // (exact match since "tetris".contains("") is true, but actually "" == display_lower is false)
        // Let's verify the actual behavior
        assert!(score > 0, "Empty string is contained in all strings");
    }

    // --- Tier ordering ---

    #[test]
    fn exact_beats_prefix() {
        let exact = search_score("tetris", "Tetris", "Tetris (USA).nes", PREF, SEC);
        let prefix = search_score("tetris", "Tetris Plus", "Tetris Plus (USA).nes", PREF, SEC);
        assert!(
            exact > prefix,
            "Exact ({exact}) should beat prefix ({prefix})"
        );
    }

    #[test]
    fn prefix_beats_word_boundary() {
        let prefix = search_score(
            "super",
            "Super Mario World",
            "Super Mario World (USA).sfc",
            PREF,
            SEC,
        );
        let word = search_score(
            "mario",
            "Super Mario World",
            "Super Mario World (USA).sfc",
            PREF,
            SEC,
        );
        assert!(
            prefix > word,
            "Prefix ({prefix}) should beat word boundary ({word})"
        );
    }

    #[test]
    fn word_boundary_beats_substring() {
        let word = search_score(
            "mario",
            "Super Mario World",
            "Super Mario World (USA).sfc",
            PREF,
            SEC,
        );
        let substr = search_score(
            "ari",
            "Super Mario World",
            "Super Mario World (USA).sfc",
            PREF,
            SEC,
        );
        assert!(
            word > substr,
            "Word boundary ({word}) should beat substring ({substr})"
        );
    }

    #[test]
    fn substring_beats_filename_only() {
        let substr = search_score(
            "ari",
            "Super Mario World",
            "Super Mario World (USA).sfc",
            PREF,
            SEC,
        );
        let filename = search_score("usa", "Tetris", "Tetris (USA).nes", PREF, SEC);
        assert!(
            substr > filename,
            "Substring ({substr}) should beat filename-only ({filename})"
        );
    }

    // --- Length bonus ---

    #[test]
    fn short_name_gets_length_bonus() {
        let short = search_score(
            "mario",
            "Super Mario World",
            "Super Mario World (USA).sfc",
            PREF,
            SEC,
        );
        let long = search_score(
            "mario",
            "Super Mario World - Long Subtitle That Makes It Over 40 Characters",
            "Super Mario World - Long Subtitle (USA).sfc",
            PREF,
            SEC,
        );
        assert!(
            short > long,
            "Short name ({short}) should beat long name ({long}) due to length bonus"
        );
    }

    // --- Tier penalties ---

    #[test]
    fn hack_is_penalized() {
        let original = search_score(
            "mario",
            "Super Mario World",
            "Super Mario World (USA).sfc",
            PREF,
            SEC,
        );
        let hack = search_score(
            "mario",
            "Super Mario World",
            "Super Mario World (Hack).sfc",
            PREF,
            SEC,
        );
        assert!(
            original > hack,
            "Original ({original}) should beat hack ({hack})"
        );
    }

    #[test]
    fn translation_is_penalized() {
        let original = search_score(
            "mario",
            "Super Mario World",
            "Super Mario World (USA).sfc",
            PREF,
            SEC,
        );
        let translated = search_score(
            "mario",
            "Super Mario World",
            "Super Mario World (Traducido Es).sfc",
            PREF,
            SEC,
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
            SEC,
        );
        assert!(
            score >= 10_000,
            "Exact match with special chars should work, got {score}"
        );
    }

    #[test]
    fn query_with_dash() {
        let score = search_score(
            "x-men",
            "X-Men - Mutant Apocalypse",
            "X-Men - Mutant Apocalypse (USA).sfc",
            PREF,
            SEC,
        );
        assert!(score > 0, "Query with dash should match");
    }

    // --- Region preference affects scoring ---

    #[test]
    fn japan_preference_boosts_japan_roms() {
        let usa_with_usa_pref = search_score(
            "mario",
            "Super Mario World",
            "Super Mario World (USA).sfc",
            RegionPreference::Usa,
            SEC,
        );
        let japan_with_japan_pref = search_score(
            "mario",
            "Super Mario World",
            "Super Mario World (Japan).sfc",
            RegionPreference::Japan,
            SEC,
        );
        // Both should get sort_key=0 (preferred region = primary), so equal region bonus.
        assert_eq!(usa_with_usa_pref, japan_with_japan_pref);
    }

    #[test]
    fn preferred_region_beats_non_preferred() {
        let japan_pref = RegionPreference::Japan;
        let japan_score = search_score(
            "mario",
            "Super Mario World",
            "Super Mario World (Japan).sfc",
            japan_pref,
            SEC,
        );
        let usa_score = search_score(
            "mario",
            "Super Mario World",
            "Super Mario World (USA).sfc",
            japan_pref,
            SEC,
        );
        assert!(
            japan_score > usa_score,
            "Japan ({japan_score}) should beat USA ({usa_score}) with Japan preference"
        );
    }

    #[test]
    fn europe_preference_puts_europe_before_usa() {
        let pref = RegionPreference::Europe;
        let europe_score = search_score(
            "mario",
            "Super Mario World",
            "Super Mario World (Europe).sfc",
            pref,
            SEC,
        );
        let usa_score = search_score(
            "mario",
            "Super Mario World",
            "Super Mario World (USA).sfc",
            pref,
            SEC,
        );
        assert!(
            europe_score > usa_score,
            "Europe ({europe_score}) should beat USA ({usa_score}) with Europe preference"
        );
    }

    // --- Word-level fuzzy matching ---

    #[test]
    fn word_match_sonic_3() {
        let score = search_score(
            "sonic 3",
            "Sonic The Hedgehog 3",
            "Sonic The Hedgehog 3 (USA).md",
            PREF,
            SEC,
        );
        assert!(
            score > 0,
            "\"sonic 3\" should match \"Sonic The Hedgehog 3\", got {score}"
        );
        assert!(
            score < 1000,
            "Word match ({score}) should be below substring tier (1000)"
        );
    }

    #[test]
    fn word_match_zelda_link() {
        let score = search_score(
            "zelda link",
            "The Legend of Zelda - A Link to the Past",
            "Legend of Zelda, The - A Link to the Past (USA).sfc",
            PREF,
            SEC,
        );
        assert!(
            score > 0,
            "\"zelda link\" should match Zelda ALTTP, got {score}"
        );
    }

    #[test]
    fn word_match_ranks_exact_word_above_prefix() {
        // Both titles fail substring tiers for "sonic 3".
        // "Sonic The Hedgehog 3" has exact word "3" — should rank higher than
        // "Sonic Adventures 3D Edition" where "3" only prefix-matches "3D".
        let hedgehog = search_score(
            "sonic 3",
            "Sonic The Hedgehog 3",
            "Sonic The Hedgehog 3 (USA).md",
            PREF,
            SEC,
        );
        let adventures = search_score(
            "sonic 3",
            "Sonic Adventures 3D Edition",
            "Sonic Adventures 3D Edition (USA).md",
            PREF,
            SEC,
        );
        assert!(
            hedgehog > adventures,
            "Hedgehog 3 ({hedgehog}) should beat Adventures 3D ({adventures})"
        );
    }

    #[test]
    fn word_match_does_not_activate_for_single_word() {
        let score = search_score(
            "zzzznotfound",
            "Sonic The Hedgehog 3",
            "Sonic The Hedgehog 3 (USA).md",
            PREF,
            SEC,
        );
        assert_eq!(score, 0);
    }

    #[test]
    fn word_match_requires_all_words() {
        // "sonic mario" — "mario" is not in "Sonic The Hedgehog 3"
        let score = search_score(
            "sonic mario",
            "Sonic The Hedgehog 3",
            "Sonic The Hedgehog 3 (USA).md",
            PREF,
            SEC,
        );
        assert_eq!(score, 0, "Not all query words present, should return 0");
    }

    #[test]
    fn word_match_below_substring() {
        // A substring match should always score higher than a word match
        let substring = search_score(
            "sonic",
            "Sonic The Hedgehog 3",
            "Sonic The Hedgehog 3 (USA).md",
            PREF,
            SEC,
        );
        let word = search_score(
            "sonic 3",
            "Sonic The Hedgehog 3",
            "Sonic The Hedgehog 3 (USA).md",
            PREF,
            SEC,
        );
        assert!(
            substring > word,
            "Substring ({substring}) should beat word match ({word})"
        );
    }

    #[test]
    fn word_match_x_men_hyphen() {
        let score = search_score(
            "x men",
            "X-Men - Mutant Apocalypse",
            "X-Men - Mutant Apocalypse (USA).sfc",
            PREF,
            SEC,
        );
        assert!(
            score > 0,
            "\"x men\" should match \"X-Men\" via hyphen splitting, got {score}"
        );
    }

    #[test]
    fn word_match_preserves_existing_substring_match() {
        // "mega man x" is a contiguous substring — should match at prefix tier, not word tier
        let score = search_score(
            "mega man x",
            "Mega Man X",
            "Mega Man X (USA).sfc",
            PREF,
            SEC,
        );
        assert!(
            score >= 5000,
            "Contiguous match should hit prefix tier, got {score}"
        );
    }

    #[test]
    fn word_match_mario_kart() {
        let score = search_score(
            "mario kart",
            "Super Mario Kart",
            "Super Mario Kart (USA).sfc",
            PREF,
            SEC,
        );
        // "mario kart" IS a contiguous substring of "Super Mario Kart"
        assert!(
            score >= 1000,
            "\"mario kart\" is a substring, should score >= 1000, got {score}"
        );
    }

    // --- Prefix word-boundary tests ---

    #[test]
    fn prefix_match_word_boundary() {
        // "sonic 3" should score "Sonic 3 (Europe)" higher than "Sonic 3D Blast"
        // because "sonic 3" in "sonic 3d blast" breaks mid-word ("3" → "3d").
        let clean = search_score(
            "sonic 3",
            "Sonic 3 (Europe)",
            "Sonic 3 (Europe).md",
            PREF,
            SEC,
        );
        let midword = search_score(
            "sonic 3",
            "Sonic 3D Blast",
            "Sonic 3D Blast (USA).md",
            PREF,
            SEC,
        );
        assert!(
            clean > midword,
            "Clean prefix 'Sonic 3 (Europe)' ({clean}) should beat mid-word 'Sonic 3D Blast' ({midword})"
        );
    }

    #[test]
    fn sonic_3_hedgehog_above_3d_blast() {
        // "sonic 3" should rank "Sonic The Hedgehog 3 (USA)" above "Sonic 3D Blast (USA)"
        let hedgehog = search_score(
            "sonic 3",
            "Sonic The Hedgehog 3",
            "Sonic The Hedgehog 3 (USA).md",
            PREF,
            SEC,
        );
        let blast = search_score(
            "sonic 3",
            "Sonic 3D Blast",
            "Sonic 3D Blast (USA).md",
            PREF,
            SEC,
        );
        assert!(
            hedgehog > blast,
            "Hedgehog 3 ({hedgehog}) should beat 3D Blast ({blast})"
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
        .get_roms(
            &storage,
            chosen_system,
            state.region_preference(),
            state.region_preference_secondary(),
        )
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    if roms.is_empty() {
        return Err(ServerFnError::new("No ROMs in selected system"));
    }

    let idx = rng.random_range(0..roms.len());
    let rom = &roms[idx];
    Ok((chosen_system.clone(), rom.game.rom_filename.clone()))
}
