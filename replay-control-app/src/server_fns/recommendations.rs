use super::*;
#[cfg(feature = "ssr")]
use replay_control_core::metadata_db::MetadataDb;

/// A recommended game card with display info and navigation link.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecommendedGame {
    pub system: String,
    pub system_display: String,
    pub rom_filename: String,
    pub display_name: String,
    pub box_art_url: Option<String>,
    pub href: String,
    /// Optional short label (e.g., region tags). When set, UI can show this
    /// instead of `display_name` for compact display.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
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
    pub system_display: String,
    pub system: String,
    pub picks: Vec<RecommendedGame>,
}

/// All recommendation data in a single response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecommendationData {
    pub random_picks: Vec<RecommendedGame>,
    pub top_genres: Vec<GenreCount>,
    pub multiplayer_count: usize,
    pub favorites_picks: Option<FavoritesPicks>,
    pub top_rated: Option<Vec<RecommendedGame>>,
}

/// Get recommendation data from SQLite game_library + filesystem image resolution.
/// Returns empty data gracefully if game_library is not yet populated.
#[server(prefix = "/sfn")]
pub async fn get_recommendations(count: usize) -> Result<RecommendationData, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();
    let systems = state.cache.get_systems(&storage).await;
    let count = count.clamp(1, 12);

    // Collect favorites from the in-memory cache (no filesystem or DB access).
    let favorites_info = collect_favorites_info(&state, &storage, &systems).await;

    // Clone favorites_info for use after the DB closure (which consumes it via move).
    let favorites_info_for_picks = favorites_info.clone();

    let region_pref = state.region_preference();
    let region_secondary = state.region_preference_secondary();
    let region_str = region_pref.as_str().to_string();
    let region_secondary_str = region_secondary
        .map(|r| r.as_str())
        .unwrap_or("")
        .to_string();

    // Single DB access: run all SQL queries under one Mutex lock.
    let db_data = state
        .cache
        .db_read(move |conn| {
            let random_pool = MetadataDb::random_cached_roms_diverse(
                conn,
                count,
                &region_str,
                &region_secondary_str,
            )
            .unwrap_or_default();
            let genre_counts = MetadataDb::genre_counts(conn).unwrap_or_default();
            let multiplayer = MetadataDb::multiplayer_count(conn).unwrap_or(0);
            let top_rated = MetadataDb::top_rated_cached_roms(
                conn,
                count * 3,
                &region_str,
                &region_secondary_str,
            )
            .unwrap_or_default();
            let fav_roms = favorites_info.as_ref().map(|fi| {
                let exclude: Vec<&str> = fi.fav_filenames.iter().map(|s| s.as_str()).collect();
                let top_genre = fi.top_genre.as_deref();
                let mut roms = MetadataDb::system_roms_excluding(
                    conn,
                    &fi.system,
                    &exclude,
                    top_genre,
                    count,
                    &region_str,
                    &region_secondary_str,
                )
                .unwrap_or_default();
                // Fill with any genre if not enough genre-matching.
                if roms.len() < count && top_genre.is_some() {
                    let have: std::collections::HashSet<String> =
                        roms.iter().map(|r| r.rom_filename.clone()).collect();
                    let more = MetadataDb::system_roms_excluding(
                        conn,
                        &fi.system,
                        &exclude,
                        None,
                        count,
                        &region_str,
                        &region_secondary_str,
                    )
                    .unwrap_or_default();
                    for r in more {
                        if roms.len() >= count {
                            break;
                        }
                        if !have.contains(&r.rom_filename) {
                            roms.push(r);
                        }
                    }
                }
                roms
            });
            (random_pool, genre_counts, multiplayer, top_rated, fav_roms)
        })
        .await;

    let Some((random_pool, genre_counts, multiplayer_count, top_rated_pool, fav_roms)) = db_data
    else {
        return Ok(RecommendationData {
            random_picks: Vec::new(),
            top_genres: Vec::new(),
            multiplayer_count: 0,
            favorites_picks: None,
            top_rated: None,
        });
    };

    // --- Post-process random picks: ensure system diversity ---
    let mut random_picks = diversify_picks(random_pool, count, &systems);

    // --- Genre/multiplayer ---
    let top_genres: Vec<GenreCount> = genre_counts
        .into_iter()
        .take(4)
        .map(|(genre, count)| GenreCount { genre, count })
        .collect();

    // --- Favorites picks (pool already randomized by SQL) ---
    let mut favorites_picks = favorites_info_for_picks.and_then(|fi| {
        let roms = fav_roms?;
        if roms.is_empty() {
            return None;
        }
        let picks: Vec<RecommendedGame> = roms
            .iter()
            .take(count)
            .filter_map(|rom| to_recommended(&rom.system, rom, &systems))
            .collect();
        if picks.is_empty() {
            return None;
        }
        Some(FavoritesPicks {
            system_display: fi.system_display,
            system: fi.system,
            picks,
        })
    });

    // --- Top rated: pool already randomized by SQL, diversify across systems ---
    let mut top_rated = if top_rated_pool.is_empty() {
        None
    } else {
        let picks = diversify_picks(top_rated_pool, count, &systems);
        if picks.is_empty() { None } else { Some(picks) }
    };

    // Resolve box art from filesystem (same approach as recents/favorites/games).
    // The pre-cached game_library.box_art_url may be stale or NULL.
    resolve_box_art_for_picks(&state, &mut random_picks).await;
    if let Some(ref mut fp) = favorites_picks {
        resolve_box_art_for_picks(&state, &mut fp.picks).await;
    }
    if let Some(ref mut tr) = top_rated {
        resolve_box_art_for_picks(&state, tr).await;
    }

    Ok(RecommendationData {
        random_picks,
        top_genres,
        multiplayer_count,
        favorites_picks,
        top_rated,
    })
}

/// Info about the user's favorites needed for building recommendations.
#[cfg(feature = "ssr")]
#[derive(Clone)]
struct FavoritesInfo {
    system: String,
    system_display: String,
    fav_filenames: Vec<String>,
    top_genre: Option<String>,
}

/// Collect favorites info from the in-memory cache — no filesystem access.
/// Randomly picks among systems that have favorites (weighted by sqrt of count)
/// so the section rotates across systems on each page load.
#[cfg(feature = "ssr")]
async fn collect_favorites_info(
    state: &crate::api::AppState,
    storage: &replay_control_core::storage::StorageLocation,
    systems: &[SystemSummary],
) -> Option<FavoritesInfo> {
    let all_favorites = state.cache.get_all_favorited_systems(storage)?;
    if all_favorites.is_empty() {
        return None;
    }

    // Build a weighted pool: systems with more favorites appear more often.
    // Weight = sqrt(count) to avoid overwhelming dominance by large collections.
    let mut weighted: Vec<(&str, &Vec<String>)> = Vec::new();
    for (system, filenames) in &all_favorites {
        if !filenames.is_empty() {
            let weight = (filenames.len() as f64).sqrt().ceil() as usize;
            for _ in 0..weight {
                weighted.push((system.as_str(), filenames));
            }
        }
    }

    if weighted.is_empty() {
        return None;
    }

    // Pick a random entry from the weighted pool.
    use rand::Rng;
    let idx = rand::rng().random_range(0..weighted.len());
    let (chosen_system, fav_filenames) = weighted[idx];

    let system_display = systems
        .iter()
        .find(|s| s.folder_name == chosen_system)
        .map(|s| s.display_name.clone())
        .unwrap_or_else(|| chosen_system.to_string());

    // Determine top genre_group from favorites using game_library DB.
    let genre_map: std::collections::HashMap<String, String> = state
        .cache
        .db_read({
            let chosen_system = chosen_system.to_string();
            move |conn| {
                MetadataDb::load_system_entries(conn, &chosen_system)
                    .map(|entries| {
                        entries
                            .into_iter()
                            .filter(|e| !e.genre_group.is_empty())
                            .map(|e| (e.rom_filename, e.genre_group))
                            .collect()
                    })
                    .unwrap_or_default()
            }
        })
        .await
        .unwrap_or_default();

    let mut genre_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for filename in fav_filenames {
        if let Some(gg) = genre_map.get(filename) {
            *genre_counts.entry(gg.clone()).or_default() += 1;
        }
    }
    let top_genre = genre_counts
        .into_iter()
        .max_by_key(|(_, c)| *c)
        .map(|(g, _)| g);

    Some(FavoritesInfo {
        system: chosen_system.to_string(),
        system_display,
        fav_filenames: fav_filenames.clone(),
        top_genre,
    })
}

/// Select diverse picks from a pool: prefer one per system, then fill with a cap.
#[cfg(feature = "ssr")]
fn diversify_picks(
    pool: Vec<replay_control_core::metadata_db::GameEntry>,
    count: usize,
    systems: &[SystemSummary],
) -> Vec<RecommendedGame> {
    use std::collections::HashMap;

    let mut picks = Vec::with_capacity(count);
    let mut system_counts: HashMap<String, usize> = HashMap::new();

    // First pass: one per system.
    for rom in &pool {
        if picks.len() >= count {
            break;
        }
        if system_counts.contains_key(&rom.system) {
            continue;
        }
        if let Some(game) = to_recommended(&rom.system, rom, systems) {
            *system_counts.entry(rom.system.clone()).or_default() += 1;
            picks.push(game);
        }
    }

    // Second pass: fill remaining, but cap each system to ensure diversity.
    // With count=6, max_per_system=2 guarantees at least 3 different systems.
    let max_per_system = (count / 3).max(2);
    for rom in &pool {
        if picks.len() >= count {
            break;
        }
        let sys_count = system_counts.get(&rom.system).copied().unwrap_or(0);
        if sys_count >= max_per_system {
            continue;
        }
        if picks
            .iter()
            .any(|p| p.system == rom.system && p.rom_filename == rom.rom_filename)
        {
            continue;
        }
        if let Some(game) = to_recommended(&rom.system, rom, systems) {
            *system_counts.entry(rom.system.clone()).or_default() += 1;
            picks.push(game);
        }
    }

    picks
}

/// Resolve box art URLs from the filesystem for a batch of picks.
/// Uses the same ImageIndex approach as recents/favorites/games pages.
#[cfg(feature = "ssr")]
pub(super) async fn resolve_box_art_for_picks(
    state: &crate::api::AppState,
    picks: &mut [RecommendedGame],
) {
    let mut image_indexes: std::collections::HashMap<
        String,
        std::sync::Arc<crate::api::cache::ImageIndex>,
    > = std::collections::HashMap::new();
    for game in picks.iter_mut() {
        if !image_indexes.contains_key(&game.system) {
            let index = state.cache.get_image_index(state, &game.system).await;
            image_indexes.insert(game.system.clone(), index);
        }
        let index = &image_indexes[&game.system];
        if let Some(url) =
            state
                .cache
                .resolve_box_art(state, index, &game.system, &game.rom_filename)
        {
            game.box_art_url = Some(url);
        }
    }
}

/// Convert GameEntry to RecommendedGame. box_art_url is resolved later by the caller.
#[cfg(feature = "ssr")]
pub(super) fn to_recommended(
    system: &str,
    rom: &replay_control_core::metadata_db::GameEntry,
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
        box_art_url: rom.box_art_url.clone(),
        href,
        label: None,
    })
}
