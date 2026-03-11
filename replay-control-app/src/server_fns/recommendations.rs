use super::*;

/// A recommended game card with display info and navigation link.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecommendedGame {
    pub system: String,
    pub system_display: String,
    pub rom_filename: String,
    pub display_name: String,
    pub box_art_url: Option<String>,
    pub href: String,
}

/// A genre with its game count across the library.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenreCount {
    pub genre: String,
    pub count: usize,
}

/// Favorites-based recommendation: games from the user's most-favorited system(s).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FavoritesPicks {
    /// Display name of the system (e.g., "Mega Drive / Genesis").
    pub system_display: String,
    /// System folder name for building "See all" links.
    pub system: String,
    /// Recommended games from that system (non-favorited).
    pub picks: Vec<RecommendedGame>,
}

/// All recommendation data in a single response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecommendationData {
    pub random_picks: Vec<RecommendedGame>,
    pub top_genres: Vec<GenreCount>,
    pub multiplayer_count: usize,
    /// Favorites-based: games from the user's most-favorited system (Phase 2).
    /// None if the user has no favorites.
    pub favorites_picks: Option<FavoritesPicks>,
    /// Top-rated games from the user's library (Phase 3).
    /// None if metadata DB has no rating data.
    pub top_rated: Option<Vec<RecommendedGame>>,
}

/// Get recommendation data: random picks, discover links, favorites-based, and top-rated.
/// Returns everything in a single call to minimize server round-trips on the Pi.
///
/// Uses SQL queries on the rom_cache for genre/multiplayer aggregation (fast)
/// instead of iterating all ROMs in memory (slow on NFS cold start).
#[server(prefix = "/sfn")]
pub async fn get_recommendations(count: usize) -> Result<RecommendationData, ServerFnError> {
    use rand::seq::SliceRandom;
    use rand::Rng;

    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();
    let region_pref = state.region_preference();
    let systems = state.cache.get_systems(&storage);

    let count = count.clamp(1, 12);
    let mut rng = rand::rng();

    // --- Phase A: Random picks with box art ---
    let random_picks = build_random_picks(
        &state, &storage, region_pref, &systems, count, &mut rng,
    );

    // --- Phase B: Genre/multiplayer aggregation via SQL ---
    let (top_genres, multiplayer_count) = build_discover_stats(&state, &storage, &systems, region_pref);

    // --- Phase C: Favorites-based recommendations ---
    let favorites_picks = build_favorites_picks(
        &state, &storage, region_pref, &systems, count, &mut rng,
    );

    // --- Phase D: Top-rated recommendations ---
    let top_rated = build_top_rated_picks(&state, &storage, &systems, count);

    Ok(RecommendationData {
        random_picks,
        top_genres,
        multiplayer_count,
        favorites_picks,
        top_rated,
    })
}

/// Build random game picks, preferring diversity across systems and games with box art.
#[cfg(feature = "ssr")]
fn build_random_picks(
    state: &crate::api::AppState,
    storage: &replay_control_core::storage::StorageLocation,
    region_pref: replay_control_core::rom_tags::RegionPreference,
    systems: &[SystemSummary],
    count: usize,
    rng: &mut impl rand::Rng,
) -> Vec<RecommendedGame> {
    use rand::seq::SliceRandom;

    let weighted: Vec<(&str, usize)> = systems
        .iter()
        .filter(|s| s.game_count > 0)
        .map(|s| (s.folder_name.as_str(), s.game_count))
        .collect();

    let total_games: usize = weighted.iter().map(|(_, c)| c).sum();
    if total_games == 0 {
        return Vec::new();
    }

    let mut picks = Vec::with_capacity(count);
    let mut used_systems: Vec<String> = Vec::new();
    let mut attempts = 0;

    while picks.len() < count && attempts < count * 4 {
        attempts += 1;

        // Weighted random system selection.
        let pick = rng.random_range(0..total_games);
        let mut cumulative = 0;
        let mut chosen_system = weighted[0].0;
        for &(sys, cnt) in &weighted {
            cumulative += cnt;
            if pick < cumulative {
                chosen_system = sys;
                break;
            }
        }

        // Prefer systems we haven't picked from yet.
        if used_systems.len() < weighted.len()
            && used_systems.contains(&chosen_system.to_string())
        {
            continue;
        }

        let roms = match state.cache.get_roms(storage, chosen_system, region_pref) {
            Ok(roms) => roms,
            Err(_) => continue,
        };

        if roms.is_empty() {
            continue;
        }

        let idx = rng.random_range(0..roms.len());
        let rom = &roms[idx];

        if let Some(game) = rom_to_recommended(state, chosen_system, rom, systems) {
            picks.push(game);
            used_systems.push(chosen_system.to_string());
        }
    }

    // Fill remaining slots if needed (allow repeats).
    while picks.len() < count {
        let pick = rng.random_range(0..total_games);
        let mut cumulative = 0;
        let mut chosen_system = weighted[0].0;
        for &(sys, cnt) in &weighted {
            cumulative += cnt;
            if pick < cumulative {
                chosen_system = sys;
                break;
            }
        }

        let roms = match state.cache.get_roms(storage, chosen_system, region_pref) {
            Ok(roms) => roms,
            Err(_) => break,
        };
        if roms.is_empty() {
            break;
        }

        let idx = rng.random_range(0..roms.len());
        let rom = &roms[idx];

        // Skip duplicates.
        if picks
            .iter()
            .any(|p| p.rom_filename == rom.game.rom_filename && p.system == chosen_system)
        {
            continue;
        }

        if let Some(game) = rom_to_recommended(state, chosen_system, rom, systems) {
            picks.push(game);
        }
    }

    picks.shuffle(rng);
    picks
}

/// Build genre counts and multiplayer count using SQL queries on rom_cache.
/// Falls back to iterating ROM lists if rom_cache is empty.
#[cfg(feature = "ssr")]
fn build_discover_stats(
    state: &crate::api::AppState,
    storage: &replay_control_core::storage::StorageLocation,
    systems: &[SystemSummary],
    region_pref: replay_control_core::rom_tags::RegionPreference,
) -> (Vec<GenreCount>, usize) {
    // Try SQL path first (fast, uses rom_cache).
    if let Some(guard) = state.metadata_db() {
        if let Some(db) = guard.as_ref() {
            let genre_counts = db.genre_counts().unwrap_or_default();
            let multiplayer = db.multiplayer_count().unwrap_or(0);

            // Only use SQL results if the rom_cache has data.
            if !genre_counts.is_empty() || multiplayer > 0 {
                let mut top_genres: Vec<GenreCount> = genre_counts
                    .into_iter()
                    .map(|(genre, count)| GenreCount { genre, count })
                    .collect();
                top_genres.truncate(4);
                return (top_genres, multiplayer);
            }
        }
    }

    // Fallback: iterate ROMs from cache (only happens before any L3 scan populates rom_cache).
    let mut genre_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let mut multiplayer_count: usize = 0;

    for sys in systems {
        if sys.game_count == 0 {
            continue;
        }
        let roms = match state.cache.get_roms(storage, &sys.folder_name, region_pref) {
            Ok(roms) => roms,
            Err(_) => continue,
        };
        for rom in &roms {
            let genre =
                super::search::lookup_genre(&sys.folder_name, &rom.game.rom_filename);
            if !genre.is_empty() {
                *genre_counts.entry(genre).or_insert(0) += 1;
            }
            let players =
                super::search::lookup_players(&sys.folder_name, &rom.game.rom_filename);
            if players >= 2 {
                multiplayer_count += 1;
            }
        }
    }

    let mut top_genres: Vec<GenreCount> = genre_counts
        .into_iter()
        .map(|(genre, count)| GenreCount { genre, count })
        .collect();
    top_genres.sort_by(|a, b| b.count.cmp(&a.count));
    top_genres.truncate(4);

    (top_genres, multiplayer_count)
}

/// Build favorites-based recommendations: find the user's most-favorited system,
/// then suggest non-favorited games from that system. Prefers games from the same
/// genres as the user's favorites, sorted by rating when available.
#[cfg(feature = "ssr")]
fn build_favorites_picks(
    state: &crate::api::AppState,
    storage: &replay_control_core::storage::StorageLocation,
    region_pref: replay_control_core::rom_tags::RegionPreference,
    systems: &[SystemSummary],
    count: usize,
    rng: &mut impl rand::Rng,
) -> Option<FavoritesPicks> {
    use rand::seq::SliceRandom;
    use std::collections::{HashMap, HashSet};

    let favorites = replay_control_core::favorites::list_favorites(storage).ok()?;
    if favorites.is_empty() {
        return None;
    }

    // Count favorites per system to find the top system.
    let mut fav_per_system: HashMap<&str, usize> = HashMap::new();
    for fav in &favorites {
        *fav_per_system.entry(&fav.game.system).or_default() += 1;
    }

    let (top_system, _) = fav_per_system.iter().max_by_key(|(_, count)| *count)?;
    let top_system = top_system.to_string();

    let system_display = systems
        .iter()
        .find(|s| s.folder_name == top_system)
        .map(|s| s.display_name.clone())
        .unwrap_or_else(|| top_system.clone());

    let fav_filenames: HashSet<String> = favorites
        .iter()
        .filter(|f| f.game.system == top_system)
        .map(|f| f.game.rom_filename.clone())
        .collect();

    // Determine the genres the user likes.
    let mut fav_genres: HashMap<String, usize> = HashMap::new();
    for fav in &favorites {
        if fav.game.system == top_system {
            let genre = super::search::lookup_genre(&top_system, &fav.game.rom_filename);
            if !genre.is_empty() {
                *fav_genres.entry(genre).or_default() += 1;
            }
        }
    }
    let fav_genre_set: HashSet<String> = fav_genres.keys().cloned().collect();

    let roms = state
        .cache
        .get_roms(storage, &top_system, region_pref)
        .ok()?;
    let mut candidates: Vec<_> = roms
        .into_iter()
        .filter(|r| !fav_filenames.contains(&r.game.rom_filename))
        .collect();

    if candidates.is_empty() {
        return None;
    }

    // Load ratings for sorting.
    let ratings: HashMap<String, f64> = if let Some(guard) = state.metadata_db() {
        if let Some(db) = guard.as_ref() {
            db.system_ratings(&top_system).unwrap_or_default()
        } else {
            HashMap::new()
        }
    } else {
        HashMap::new()
    };

    // Score: prefer same genre, then by rating.
    candidates.sort_by(|a, b| {
        let a_genre = super::search::lookup_genre(&top_system, &a.game.rom_filename);
        let b_genre = super::search::lookup_genre(&top_system, &b.game.rom_filename);
        let a_genre_match = fav_genre_set.contains(&a_genre);
        let b_genre_match = fav_genre_set.contains(&b_genre);

        let a_rating = ratings.get(&a.game.rom_filename).copied().unwrap_or(0.0);
        let b_rating = ratings.get(&b.game.rom_filename).copied().unwrap_or(0.0);

        b_genre_match
            .cmp(&a_genre_match)
            .then(b_rating.partial_cmp(&a_rating).unwrap_or(std::cmp::Ordering::Equal))
    });

    // Add randomness within genre-matching tier.
    let genre_matching_count = candidates
        .iter()
        .take_while(|r| {
            let g = super::search::lookup_genre(&top_system, &r.game.rom_filename);
            fav_genre_set.contains(&g)
        })
        .count();

    if genre_matching_count > count {
        candidates[..genre_matching_count].shuffle(rng);
    }

    let picks: Vec<RecommendedGame> = candidates
        .iter()
        .take(count)
        .filter_map(|rom| rom_to_recommended(state, &top_system, rom, systems))
        .collect();

    if picks.is_empty() {
        return None;
    }

    Some(FavoritesPicks {
        system_display,
        system: top_system,
        picks,
    })
}

/// Build top-rated recommendations from the metadata DB.
/// Returns None if metadata is not available or has no ratings.
#[cfg(feature = "ssr")]
fn build_top_rated_picks(
    state: &crate::api::AppState,
    storage: &replay_control_core::storage::StorageLocation,
    systems: &[SystemSummary],
    count: usize,
) -> Option<Vec<RecommendedGame>> {
    use std::collections::HashSet;

    // Try SQL path: top_rated_cached_roms from rom_cache.
    if let Some(guard) = state.metadata_db() {
        if let Some(db) = guard.as_ref() {
            let top = db.top_rated_cached_roms(count * 3).unwrap_or_default();
            if !top.is_empty() {
                // Prefer diversity across systems.
                let mut picks = Vec::with_capacity(count);
                let mut used_systems: HashSet<String> = HashSet::new();

                for rom in &top {
                    if picks.len() >= count {
                        break;
                    }
                    if used_systems.len() < systems.len()
                        && used_systems.contains(&rom.system)
                    {
                        continue;
                    }
                    if let Some(game) =
                        cached_rom_to_recommended(state, &rom.system, rom, systems)
                    {
                        used_systems.insert(rom.system.clone());
                        picks.push(game);
                    }
                }

                // Fill remaining from top-rated without diversity constraint.
                for rom in &top {
                    if picks.len() >= count {
                        break;
                    }
                    if picks
                        .iter()
                        .any(|p| p.system == rom.system && p.rom_filename == rom.rom_filename)
                    {
                        continue;
                    }
                    if let Some(game) =
                        cached_rom_to_recommended(state, &rom.system, rom, systems)
                    {
                        picks.push(game);
                    }
                }

                if !picks.is_empty() {
                    return Some(picks);
                }
            }
        }
    }

    // Fallback: use all_ratings from game_metadata table.
    let guard = state.metadata_db()?;
    let db = guard.as_ref()?;

    let all_ratings = db.all_ratings().ok()?;
    if all_ratings.is_empty() {
        return None;
    }

    let mut rated: Vec<((String, String), f64)> = all_ratings
        .into_iter()
        .filter(|(_, rating)| *rating > 0.0)
        .collect();
    rated.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let active_systems: HashSet<&str> = systems
        .iter()
        .filter(|s| s.game_count > 0)
        .map(|s| s.folder_name.as_str())
        .collect();

    let region_pref = state.region_preference();

    let mut picks = Vec::with_capacity(count);
    let mut used_systems: HashSet<String> = HashSet::new();

    for ((system, rom_filename), _) in &rated {
        if picks.len() >= count {
            break;
        }
        if !active_systems.contains(system.as_str()) {
            continue;
        }
        if used_systems.len() < active_systems.len() && used_systems.contains(system) {
            continue;
        }

        let roms = match state.cache.get_roms(storage, system, region_pref) {
            Ok(roms) => roms,
            Err(_) => continue,
        };

        if let Some(rom) = roms.iter().find(|r| r.game.rom_filename == *rom_filename) {
            if let Some(game) = rom_to_recommended(state, system, rom, systems) {
                used_systems.insert(system.clone());
                picks.push(game);
            }
        }
    }

    if picks.is_empty() { None } else { Some(picks) }
}

/// Convert a RomEntry to a RecommendedGame.
#[cfg(feature = "ssr")]
fn rom_to_recommended(
    state: &crate::api::AppState,
    system: &str,
    rom: &replay_control_core::roms::RomEntry,
    systems: &[SystemSummary],
) -> Option<RecommendedGame> {
    let rom_filename = &rom.game.rom_filename;
    let display_name = rom
        .game
        .display_name
        .as_deref()
        .unwrap_or(rom_filename)
        .to_string();
    let system_display = systems
        .iter()
        .find(|s| s.folder_name == system)
        .map(|s| s.display_name.clone())
        .unwrap_or_else(|| system.to_string());
    let box_art_url = super::resolve_box_art_url(state, system, rom_filename);
    let href = format!(
        "/games/{}/{}",
        system,
        urlencoding::encode(rom_filename)
    );
    Some(RecommendedGame {
        system: system.to_string(),
        system_display,
        rom_filename: rom_filename.clone(),
        display_name,
        box_art_url,
        href,
    })
}

/// Convert a CachedRom to a RecommendedGame.
#[cfg(feature = "ssr")]
fn cached_rom_to_recommended(
    state: &crate::api::AppState,
    system: &str,
    rom: &replay_control_core::metadata_db::CachedRom,
    systems: &[SystemSummary],
) -> Option<RecommendedGame> {
    let display_name = rom
        .display_name
        .as_deref()
        .unwrap_or(&rom.rom_filename)
        .to_string();
    let system_display = systems
        .iter()
        .find(|s| s.folder_name == system)
        .map(|s| s.display_name.clone())
        .unwrap_or_else(|| system.to_string());
    let box_art_url = super::resolve_box_art_url(state, system, &rom.rom_filename);
    let href = format!(
        "/games/{}/{}",
        system,
        urlencoding::encode(&rom.rom_filename)
    );
    Some(RecommendedGame {
        system: system.to_string(),
        system_display,
        rom_filename: rom.rom_filename.clone(),
        display_name,
        box_art_url,
        href,
    })
}
