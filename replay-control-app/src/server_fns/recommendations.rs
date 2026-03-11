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

/// All Phase 1 recommendation data in a single response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecommendationData {
    pub random_picks: Vec<RecommendedGame>,
    pub top_genres: Vec<GenreCount>,
    pub multiplayer_count: usize,
}

/// Get Phase 1 recommendation data: random game picks, top genres, and multiplayer count.
/// Returns everything in a single call to minimize server round-trips on the Pi.
#[server(prefix = "/sfn")]
pub async fn get_recommendations(count: usize) -> Result<RecommendationData, ServerFnError> {
    use rand::seq::SliceRandom;
    use rand::Rng;
    use std::collections::HashMap;

    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();
    let region_pref = state.region_preference();
    let systems = state.cache.get_systems(&storage);

    // Build weighted system list for random selection.
    let weighted: Vec<(&str, usize)> = systems
        .iter()
        .filter(|s| s.game_count > 0)
        .map(|s| (s.folder_name.as_str(), s.game_count))
        .collect();

    let total_games: usize = weighted.iter().map(|(_, c)| c).sum();

    let count = count.clamp(1, 12);
    let mut random_picks = Vec::with_capacity(count);
    let mut genre_counts: HashMap<String, usize> = HashMap::new();
    let mut multiplayer_count: usize = 0;

    // Track which systems we've already picked from to spread recommendations.
    let mut used_systems: Vec<String> = Vec::new();
    let mut rng = rand::rng();

    if total_games > 0 {
        // Pick random games, preferring diversity across systems.
        let mut attempts = 0;
        while random_picks.len() < count && attempts < count * 4 {
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

            // Prefer systems we haven't picked from yet (but allow repeats after exhaustion).
            if used_systems.len() < weighted.len() && used_systems.contains(&chosen_system.to_string()) {
                continue;
            }

            let roms = match state.cache.get_roms(&storage, chosen_system, region_pref) {
                Ok(roms) => roms,
                Err(_) => continue,
            };

            if roms.is_empty() {
                continue;
            }

            let idx = rng.random_range(0..roms.len());
            let rom = &roms[idx];
            let rom_filename = &rom.game.rom_filename;

            // Get display name.
            let display_name = rom
                .game
                .display_name
                .as_deref()
                .unwrap_or(rom_filename)
                .to_string();

            // Get system display name.
            let system_display = systems
                .iter()
                .find(|s| s.folder_name == chosen_system)
                .map(|s| s.display_name.clone())
                .unwrap_or_else(|| chosen_system.to_string());

            // Resolve box art.
            let box_art_url = super::resolve_box_art_url(&state, chosen_system, rom_filename);

            let href = format!(
                "/games/{}/{}",
                chosen_system,
                urlencoding::encode(rom_filename)
            );

            random_picks.push(RecommendedGame {
                system: chosen_system.to_string(),
                system_display,
                rom_filename: rom_filename.clone(),
                display_name,
                box_art_url,
                href,
            });

            used_systems.push(chosen_system.to_string());
        }

        // If we still need more picks (all systems used), allow repeats.
        while random_picks.len() < count {
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

            let roms = match state.cache.get_roms(&storage, chosen_system, region_pref) {
                Ok(roms) => roms,
                Err(_) => break,
            };
            if roms.is_empty() {
                break;
            }

            let idx = rng.random_range(0..roms.len());
            let rom = &roms[idx];
            let rom_filename = &rom.game.rom_filename;

            // Skip duplicates.
            if random_picks.iter().any(|p| p.rom_filename == *rom_filename && p.system == chosen_system) {
                continue;
            }

            let display_name = rom
                .game
                .display_name
                .as_deref()
                .unwrap_or(rom_filename)
                .to_string();

            let system_display = systems
                .iter()
                .find(|s| s.folder_name == chosen_system)
                .map(|s| s.display_name.clone())
                .unwrap_or_else(|| chosen_system.to_string());

            let box_art_url = super::resolve_box_art_url(&state, chosen_system, rom_filename);

            let href = format!(
                "/games/{}/{}",
                chosen_system,
                urlencoding::encode(rom_filename)
            );

            random_picks.push(RecommendedGame {
                system: chosen_system.to_string(),
                system_display,
                rom_filename: rom_filename.clone(),
                display_name,
                box_art_url,
                href,
            });
        }

        // Shuffle the final picks so system ordering isn't predictable.
        random_picks.shuffle(&mut rng);
    }

    // Compute genre counts and multiplayer count across all systems.
    for sys in &systems {
        if sys.game_count == 0 {
            continue;
        }
        let roms = match state.cache.get_roms(&storage, &sys.folder_name, region_pref) {
            Ok(roms) => roms,
            Err(_) => continue,
        };
        for rom in &roms {
            let genre = super::search::lookup_genre(&sys.folder_name, &rom.game.rom_filename);
            if !genre.is_empty() {
                *genre_counts.entry(genre).or_insert(0) += 1;
            }
            let players = super::search::lookup_players(&sys.folder_name, &rom.game.rom_filename);
            if players >= 2 {
                multiplayer_count += 1;
            }
        }
    }

    // Sort genres by count descending, take the top ones.
    let mut top_genres: Vec<GenreCount> = genre_counts
        .into_iter()
        .map(|(genre, count)| GenreCount { genre, count })
        .collect();
    top_genres.sort_by(|a, b| b.count.cmp(&a.count));
    top_genres.truncate(4);

    Ok(RecommendationData {
        random_picks,
        top_genres,
        multiplayer_count,
    })
}
