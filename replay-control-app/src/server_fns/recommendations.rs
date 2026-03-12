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

/// Get recommendation data purely from SQLite rom_cache.
/// One Mutex acquisition, a few fast SQL queries, no filesystem access.
/// Returns empty data gracefully if rom_cache is not yet populated.
#[server(prefix = "/sfn")]
pub async fn get_recommendations(count: usize) -> Result<RecommendationData, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();
    let systems = state.cache.get_systems(&storage);
    let count = count.clamp(1, 12);

    // Collect favorites from the in-memory cache (no filesystem or DB access).
    let favorites_info = collect_favorites_info(&state, &storage, &systems);

    // Single DB access: run all SQL queries under one Mutex lock.
    let db_data = state.cache.with_db_read(&storage, |db| {
        let random_pool = db.random_cached_roms_diverse(count).unwrap_or_default();
        let genre_counts = db.genre_counts().unwrap_or_default();
        let multiplayer = db.multiplayer_count().unwrap_or(0);
        let top_rated = db.top_rated_cached_roms(count * 3).unwrap_or_default();
        let fav_roms = favorites_info.as_ref().and_then(|fi| {
            let exclude: Vec<&str> = fi.fav_filenames.iter().map(|s| s.as_str()).collect();
            let top_genre = fi.top_genre.as_deref();
            let mut roms = db
                .system_roms_excluding(&fi.system, &exclude, top_genre, count)
                .unwrap_or_default();
            // Fill with any genre if not enough genre-matching.
            if roms.len() < count && top_genre.is_some() {
                let have: std::collections::HashSet<String> =
                    roms.iter().map(|r| r.rom_filename.clone()).collect();
                let more = db
                    .system_roms_excluding(&fi.system, &exclude, None, count)
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
            Some(roms)
        });
        (random_pool, genre_counts, multiplayer, top_rated, fav_roms)
    });

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
    let random_picks = diversify_picks(random_pool, count, &systems);

    // --- Genre/multiplayer ---
    let top_genres: Vec<GenreCount> = genre_counts
        .into_iter()
        .take(4)
        .map(|(genre, count)| GenreCount { genre, count })
        .collect();

    // --- Favorites picks ---
    let favorites_picks = favorites_info.and_then(|fi| {
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

    // --- Top rated: diversity across systems ---
    let top_rated = if top_rated_pool.is_empty() {
        None
    } else {
        let picks = diversify_picks(top_rated_pool, count, &systems);
        if picks.is_empty() { None } else { Some(picks) }
    };

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
struct FavoritesInfo {
    system: String,
    system_display: String,
    fav_filenames: Vec<String>,
    top_genre: Option<String>,
}

/// Collect favorites info from the in-memory cache — no filesystem access.
#[cfg(feature = "ssr")]
fn collect_favorites_info(
    state: &crate::api::AppState,
    storage: &replay_control_core::storage::StorageLocation,
    systems: &[SystemSummary],
) -> Option<FavoritesInfo> {
    let (top_system, fav_filenames) = state.cache.get_top_favorited_system(storage)?;

    let system_display = systems
        .iter()
        .find(|s| s.folder_name == top_system)
        .map(|s| s.display_name.clone())
        .unwrap_or_else(|| top_system.clone());

    // Determine top genre from favorites using baked-in game DB.
    let mut genre_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for filename in &fav_filenames {
        let genre = super::search::lookup_genre(&top_system, filename);
        if !genre.is_empty() {
            *genre_counts.entry(genre).or_default() += 1;
        }
    }
    let top_genre = genre_counts
        .into_iter()
        .max_by_key(|(_, c)| *c)
        .map(|(g, _)| g);

    Some(FavoritesInfo {
        system: top_system,
        system_display,
        fav_filenames,
        top_genre,
    })
}

/// Select diverse picks from a pool: prefer one per system, then fill.
#[cfg(feature = "ssr")]
fn diversify_picks(
    pool: Vec<replay_control_core::metadata_db::CachedRom>,
    count: usize,
    systems: &[SystemSummary],
) -> Vec<RecommendedGame> {
    use std::collections::HashSet;

    let mut picks = Vec::with_capacity(count);
    let mut used_systems: HashSet<String> = HashSet::new();

    // First pass: one per system.
    for rom in &pool {
        if picks.len() >= count {
            break;
        }
        if used_systems.contains(&rom.system) {
            continue;
        }
        if let Some(game) = to_recommended(&rom.system, rom, systems) {
            used_systems.insert(rom.system.clone());
            picks.push(game);
        }
    }

    // Second pass: fill remaining.
    for rom in &pool {
        if picks.len() >= count {
            break;
        }
        if picks
            .iter()
            .any(|p| p.system == rom.system && p.rom_filename == rom.rom_filename)
        {
            continue;
        }
        if let Some(game) = to_recommended(&rom.system, rom, systems) {
            picks.push(game);
        }
    }

    picks
}

/// Convert CachedRom to RecommendedGame. Uses cached box_art_url — no filesystem access.
#[cfg(feature = "ssr")]
fn to_recommended(
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
    })
}
