use super::*;
#[cfg(feature = "ssr")]
use replay_control_core_server::library_db::LibraryDb;

/// A recommended game card with display info and navigation link.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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

/// A pill in the Discover section: translation key + interpolation args + link.
/// The client resolves `label_key` via `key_from_str` and calls `tf(locale, key, &args)`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DiscoverPill {
    pub label_key: String,
    pub label_args: Vec<String>,
    pub href: String,
}

/// A titled row of game recommendations (favorites-based, curated spotlight, etc.).
/// `title_key` is a `Key` variant name; `title_args` are interpolation arguments.
/// The client resolves and translates these via `key_from_str` + `tf`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GameSection {
    pub title_key: String,
    pub title_args: Vec<String>,
    pub games: Vec<RecommendedGame>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub see_all_href: Option<String>,
}

/// All recommendation data in a single response.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecommendationData {
    pub random_picks: GameSection,
    pub discover_pills: Vec<DiscoverPill>,
    pub favorites_picks: Option<GameSection>,
    pub curated_spotlight: Option<GameSection>,
}

/// Get recommendation data from SQLite game_library + filesystem image resolution.
/// Returns empty data gracefully if game_library is not yet populated.
#[server(prefix = "/sfn")]
pub async fn get_recommendations() -> Result<RecommendationData, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    Ok(state.cache.recommendations_snapshot(&state).await)
}

/// Compute the recommendation payload from scratch. Called by
/// `LibraryService::recommendations_snapshot` on cache miss; the
/// SsrSnapshot layer handles single-flight + stale-on-`None` caching.
///
/// Returns `None` only when the DB pool is unavailable — caller keeps
/// the previous (stale) snapshot rather than caching `None`.
#[cfg(feature = "ssr")]
pub(crate) async fn compute_recommendations(
    state: &crate::api::AppState,
    count: usize,
) -> Option<RecommendationData> {
    let storage = state.storage();
    let systems = state
        .cache
        .cached_systems(&storage, &state.library_pool)
        .await;
    let count = count.clamp(1, 12);

    let favorites_info = collect_favorites_info_sync(&state, &storage, &systems).await;
    let favorites_info_for_picks = favorites_info.clone();

    // Pre-roll spotlight type so we can lazily collect Hidden Gems exclusion data.
    let spotlight_type = {
        use rand::RngExt;
        rand::rng().random_range(0u8..6)
    };

    // Only collect recent + favorite keys when Hidden Gems is selected (type 4).
    // These are (system, rom_filename) pairs used as an exclusion set.
    let hidden_gems_exclude: Vec<(String, String)> = if spotlight_type == 4 {
        let recents = state.cache.get_recents(&storage).await.unwrap_or_default();
        let recent_keys = recents
            .iter()
            .map(|r| (r.game.system.clone(), r.game.rom_filename.clone()));
        let fav_keys = state
            .cache
            .get_all_favorited_systems(&storage)
            .await
            .unwrap_or_default()
            .into_iter()
            .flat_map(|(system, filenames)| {
                filenames.into_iter().map(move |f| (system.clone(), f))
            });
        recent_keys.chain(fav_keys).collect()
    } else {
        Vec::new()
    };

    let (region_str, region_secondary_str) = super::region_strings(&state);

    let systems_for_spotlight: Vec<(String, String)> = systems
        .iter()
        .map(|s| (s.folder_name.clone(), s.display_name.clone()))
        .collect();

    let cached_genres = state.cache.query_cache.get_top_genres();
    let cached_developers = state.cache.query_cache.get_top_developers();
    let cached_decades = state.cache.query_cache.get_decades();
    let cached_active_systems = state.cache.query_cache.get_active_systems();
    // Single DB access: run all SQL queries under one connection.
    // This includes the favorites genre lookup that previously required a
    // separate DB read round-trip.
    let db_data = state
        .library_pool
        .read(move |conn| {
            let random_pool = LibraryDb::random_cached_roms_diverse(
                conn,
                count,
                &region_str,
                &region_secondary_str,
            )
            .unwrap_or_default();
            let top_genres = cached_genres
                .unwrap_or_else(|| LibraryDb::top_genre_names(conn, 6).unwrap_or_default());
            let top_developers = cached_developers
                .unwrap_or_else(|| LibraryDb::top_developers(conn, 10).unwrap_or_default());
            let decades =
                cached_decades.unwrap_or_else(|| LibraryDb::decade_list(conn).unwrap_or_default());
            let active_systems = cached_active_systems
                .unwrap_or_else(|| LibraryDb::active_systems(conn).unwrap_or_default());
            // --- Spotlight: type was pre-rolled above ---

            // Exclude the favorites system from system spotlight candidates.
            let fav_system: Option<&str> = favorites_info.as_ref().map(|fi| fi.system.as_str());

            // Minimum games a spotlight must return to be shown.
            // Fewer than this looks sparse — fall back to global Top Rated.
            let spotlight_min = count;

            #[allow(clippy::type_complexity)]
            let spotlight_result: Option<(
                Vec<replay_control_core_server::library_db::GameEntry>,
                String,
                Vec<String>,
                Option<String>,
            )> = match spotlight_type {
                1 if !top_genres.is_empty() => {
                    // Best by Genre
                    use rand::RngExt;
                    let idx = rand::rng().random_range(0..top_genres.len());
                    let genre = &top_genres[idx];
                    let games = LibraryDb::top_rated_filtered(
                        conn,
                        None,
                        Some(genre),
                        None,
                        count * 3,
                        &region_str,
                        &region_secondary_str,
                    )
                    .unwrap_or_default();
                    if games.len() < spotlight_min {
                        None
                    } else {
                        let href = Some(format!(
                            "/search?genre={}&min_rating=3.5",
                            urlencoding::encode(genre)
                        ));
                        Some((
                            games,
                            "SpotlightBestGenre".to_string(),
                            vec![genre.clone()],
                            href,
                        ))
                    }
                }
                2 if !active_systems.is_empty() => {
                    // Best of System — pick from systems excluding favorites system
                    use rand::RngExt;
                    let candidates: Vec<&String> = active_systems
                        .iter()
                        .filter(|s| fav_system != Some(s.as_str()))
                        .collect();
                    if candidates.is_empty() {
                        None
                    } else {
                        let idx = rand::rng().random_range(0..candidates.len());
                        let sys = candidates[idx];
                        let games = LibraryDb::top_rated_filtered(
                            conn,
                            Some(sys),
                            None,
                            None,
                            count * 3,
                            &region_str,
                            &region_secondary_str,
                        )
                        .unwrap_or_default();
                        if games.len() < spotlight_min {
                            None
                        } else {
                            let display = systems_for_spotlight
                                .iter()
                                .find(|s| s.0 == *sys)
                                .map(|s| s.1.clone())
                                .unwrap_or_else(|| sys.clone());
                            let href = Some(format!("/games/{sys}?min_rating=3.5"));
                            Some((games, "SpotlightBestOf".to_string(), vec![display], href))
                        }
                    }
                }
                3 if !top_developers.is_empty() => {
                    // Games by Developer
                    use rand::RngExt;
                    let idx = rand::rng().random_range(0..top_developers.len());
                    let dev = &top_developers[idx];
                    let games = LibraryDb::top_rated_filtered(
                        conn,
                        None,
                        None,
                        Some(dev),
                        count * 3,
                        &region_str,
                        &region_secondary_str,
                    )
                    .unwrap_or_default();
                    if games.len() < spotlight_min {
                        None
                    } else {
                        let href = Some(format!("/developer/{}", urlencoding::encode(dev)));
                        Some((
                            games,
                            "SpotlightGamesBy".to_string(),
                            vec![dev.clone()],
                            href,
                        ))
                    }
                }
                4 => {
                    // Hidden Gems — high-rated games the user hasn't played recently or favorited.
                    // Prefer games with fewer ratings to surface less-known titles.
                    let games = LibraryDb::top_rated_filtered(
                        conn,
                        None,
                        None,
                        None,
                        count * 6,
                        &region_str,
                        &region_secondary_str,
                    )
                    .unwrap_or_default();
                    // Build exclude set from recents + favorites (pre-collected above).
                    let exclude: std::collections::HashSet<(&str, &str)> = hidden_gems_exclude
                        .iter()
                        .map(|(s, f)| (s.as_str(), f.as_str()))
                        .collect();
                    // Filter out known games and prefer low rating_count.
                    let mut filtered: Vec<_> = games
                        .into_iter()
                        .filter(|g| {
                            !exclude.contains(&(g.system.as_str(), g.rom_filename.as_str()))
                        })
                        .collect();
                    // Sort by rating_count ascending so lesser-known gems come first,
                    // then take a random subset from the top candidates.
                    filtered.sort_by_key(|g| g.rating_count.unwrap_or(0));
                    let pool: Vec<_> = filtered.into_iter().take(count * 3).collect();
                    if pool.len() < spotlight_min {
                        None
                    } else {
                        Some((pool, "SpotlightHiddenGems".to_string(), vec![], None))
                    }
                }
                5 => {
                    // Co-op Games — best rated cooperative games
                    let games = LibraryDb::random_coop_games(
                        conn,
                        count * 3,
                        &region_str,
                        &region_secondary_str,
                    )
                    .unwrap_or_default();
                    if games.len() < spotlight_min {
                        None
                    } else {
                        let href = Some("/search?coop=true".to_string());
                        Some((games, "SpotlightCoOp".to_string(), vec![], href))
                    }
                }
                _ => None, // Falls through to global top rated below
            };

            // Fall back to global top rated if the selected type returned empty or was type 0.
            let (spotlight_pool, spotlight_title_key, spotlight_title_args, spotlight_href) =
                spotlight_result.unwrap_or_else(|| {
                    let games = LibraryDb::top_rated_filtered(
                        conn,
                        None,
                        None,
                        None,
                        count * 3,
                        &region_str,
                        &region_secondary_str,
                    )
                    .unwrap_or_default();
                    (games, "SpotlightTopRated".to_string(), vec![], None)
                });
            let fav_roms = favorites_info.as_ref().map(|fi| {
                // Compute top genre inside this closure instead of a separate DB read.
                let fav_refs: Vec<&str> = fi.fav_filenames.iter().map(|s| s.as_str()).collect();
                let top_genre = LibraryDb::top_genre_for_filenames(conn, &fi.system, &fav_refs)
                    .ok()
                    .flatten();
                let exclude: Vec<&str> = fi.fav_filenames.iter().map(|s| s.as_str()).collect();
                let mut roms = LibraryDb::system_roms_excluding(
                    conn,
                    &fi.system,
                    &exclude,
                    top_genre.as_deref(),
                    count,
                    &region_str,
                    &region_secondary_str,
                )
                .unwrap_or_default();
                // Fill with any genre if not enough genre-matching.
                if roms.len() < count && top_genre.is_some() {
                    let have: std::collections::HashSet<String> =
                        roms.iter().map(|r| r.rom_filename.clone()).collect();
                    let more = LibraryDb::system_roms_excluding(
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
            (
                random_pool,
                top_genres,
                top_developers,
                decades,
                active_systems,
                spotlight_pool,
                spotlight_title_key,
                spotlight_title_args,
                spotlight_href,
                fav_roms,
            )
        })
        .await;

    let Some((
        random_pool,
        top_genres,
        top_developers,
        decades,
        active_systems,
        spotlight_pool,
        spotlight_title_key,
        spotlight_title_args,
        spotlight_href,
        fav_roms,
    )) = db_data
    else {
        return Some(RecommendationData::default());
    };

    state.cache.query_cache.set_top_genres(&top_genres);
    state.cache.query_cache.set_top_developers(&top_developers);
    state.cache.query_cache.set_decades(&decades);
    state.cache.query_cache.set_active_systems(&active_systems);

    // --- Post-process random picks: ensure system diversity ---
    let random_picks = diversify_picks(random_pool, count, &systems);

    // --- Discover pills: build pool and pick 5 ---
    let discover_pills = build_discover_pills(
        &top_genres,
        &top_developers,
        &decades,
        &active_systems,
        &systems,
    );

    // --- Favorites picks (pool already randomized by SQL) ---
    let favorites_picks = favorites_info_for_picks.and_then(|fi| {
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
        Some(GameSection {
            title_key: "SpotlightBecauseYouLove".to_string(),
            title_args: vec![fi.system_display.clone()],
            games: picks,
            see_all_href: Some(format!("/games/{}", fi.system)),
        })
    });

    // --- Curated spotlight: pool already randomized by SQL ---
    // For single-system spotlights (e.g., "Best of SNES"), skip diversity capping
    // since all games share one system and the cap would limit output to 2 games.
    let curated_spotlight = if spotlight_pool.is_empty() {
        None
    } else {
        let single_system = spotlight_pool
            .iter()
            .all(|g| g.system == spotlight_pool[0].system);
        let games = if single_system {
            spotlight_pool
                .iter()
                .take(count)
                .filter_map(|rom| to_recommended(&rom.system, rom, &systems))
                .collect()
        } else {
            diversify_picks(spotlight_pool, count, &systems)
        };
        if games.is_empty() {
            None
        } else {
            Some(GameSection {
                title_key: spotlight_title_key,
                title_args: spotlight_title_args,
                games,
                see_all_href: spotlight_href,
            })
        }
    };

    let data = RecommendationData {
        random_picks: GameSection {
            title_key: "SpotlightRediscover".to_string(),
            title_args: vec![],
            games: random_picks,
            see_all_href: None,
        },
        discover_pills,
        favorites_picks,
        curated_spotlight,
    };

    Some(data)
}

/// Info about the user's favorites needed for building recommendations.
#[cfg(feature = "ssr")]
#[derive(Clone)]
struct FavoritesInfo {
    system: String,
    system_display: String,
    fav_filenames: Vec<String>,
}

/// Collect favorites info from the in-memory cache — no DB access.
/// Randomly picks among systems that have favorites (weighted by sqrt of count)
/// so the section rotates across systems on each page load.
///
/// `top_genre` is left as `None` — the caller computes it inside the main
/// DB read closure using `LibraryDb::top_genre_for_filenames` to avoid
/// a separate round-trip.
#[cfg(feature = "ssr")]
async fn collect_favorites_info_sync(
    state: &crate::api::AppState,
    storage: &replay_control_core_server::storage::StorageLocation,
    systems: &[SystemSummary],
) -> Option<FavoritesInfo> {
    let all_favorites = state.cache.get_all_favorited_systems(storage).await?;
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
    use rand::RngExt;
    let idx = rand::rng().random_range(0..weighted.len());
    let (chosen_system, fav_filenames) = weighted[idx];

    let system_display = systems
        .iter()
        .find(|s| s.folder_name == chosen_system)
        .map(|s| s.display_name.clone())
        .unwrap_or_else(|| chosen_system.to_string());

    Some(FavoritesInfo {
        system: chosen_system.to_string(),
        system_display,
        fav_filenames: fav_filenames.clone(),
    })
}

/// Build the Discover pills: pick 5 from a pool of genre, system, developer,
/// decade, and multiplayer pills.
///
/// Selection: always 1 genre + 1 multiplayer, then 3 more random (no type repeats).
#[cfg(feature = "ssr")]
fn build_discover_pills(
    top_genres: &[String],
    top_developers: &[String],
    decades: &[u16],
    active_systems: &[String],
    systems: &[SystemSummary],
) -> Vec<DiscoverPill> {
    use rand::RngExt;

    if top_genres.is_empty() && active_systems.is_empty() {
        return Vec::new();
    }

    let mut rng = rand::rng();
    let mut pills: Vec<DiscoverPill> = Vec::with_capacity(5);

    // Track which pill types we've used to avoid repeats.
    // Types: "genre", "system", "developer", "decade"
    let mut used_types: Vec<&str> = Vec::new();

    // 1. Always include 1 genre pill (random from top genres).
    if !top_genres.is_empty() {
        let idx = rng.random_range(0..top_genres.len());
        let genre = &top_genres[idx];
        pills.push(DiscoverPill {
            label_key: "SpotlightBestGenre".to_string(),
            label_args: vec![genre.clone()],
            href: format!("/search?genre={}", urlencoding::encode(genre)),
        });
        used_types.push("genre");
    }

    // 2. Always include the multiplayer pill.
    pills.push(DiscoverPill {
        label_key: "PillMultiplayer".to_string(),
        label_args: vec![],
        href: "/search?multiplayer=true".to_string(),
    });
    used_types.push("multiplayer");

    // 3. Build a pool of candidate pills for the remaining slots.
    // Co-op pill as a candidate (not always shown — competes with other types).
    let mut candidates: Vec<(&str, DiscoverPill)> = Vec::new();

    candidates.push((
        "coop",
        DiscoverPill {
            label_key: "PillCoOp".to_string(),
            label_args: vec![],
            href: "/search?coop=true".to_string(),
        },
    ));

    // Another genre (different from the one already picked).
    for genre in top_genres {
        if pills
            .iter()
            .any(|p| p.label_args.first().map(|a| a == genre).unwrap_or(false))
        {
            continue;
        }
        candidates.push((
            "genre",
            DiscoverPill {
                label_key: "SpotlightBestGenre".to_string(),
                label_args: vec![genre.clone()],
                href: format!("/search?genre={}", urlencoding::encode(genre)),
            },
        ));
        break; // Only add one extra genre candidate
    }

    // System spotlight: link to the system's own page.
    if !active_systems.is_empty() {
        let idx = rng.random_range(0..active_systems.len());
        let sys = &active_systems[idx];
        let display = systems
            .iter()
            .find(|s| s.folder_name == *sys)
            .map(|s| s.display_name.clone())
            .unwrap_or_else(|| sys.clone());
        candidates.push((
            "system",
            DiscoverPill {
                label_key: "PillBestOf".to_string(),
                label_args: vec![display],
                href: format!("/games/{sys}?min_rating=3.5"),
            },
        ));
    }

    // Developer pill: pick a random developer from top list.
    if !top_developers.is_empty() {
        let idx = rng.random_range(0..top_developers.len());
        let dev = &top_developers[idx];
        candidates.push((
            "developer",
            DiscoverPill {
                label_key: "PillGamesBy".to_string(),
                label_args: vec![dev.clone()],
                href: format!("/developer/{}", urlencoding::encode(dev)),
            },
        ));
    }

    // Decade pill: pick a random decade.
    if !decades.is_empty() {
        let idx = rng.random_range(0..decades.len());
        let decade = decades[idx];
        let end = decade + 9;
        candidates.push((
            "decade",
            DiscoverPill {
                label_key: "PillClassics".to_string(),
                label_args: vec![decade.to_string()],
                href: format!("/search?min_year={decade}&max_year={end}"),
            },
        ));
    }

    // NOTE: 4-Player pill deferred to Phase 3 — needs `min_players` search filter.

    // Shuffle candidates and pick up to 3 more, no type repeats.
    {
        use rand::seq::SliceRandom;
        candidates.shuffle(&mut rng);
    }

    for (pill_type, pill) in candidates {
        if pills.len() >= 5 {
            break;
        }
        if used_types.contains(&pill_type) {
            continue;
        }
        used_types.push(pill_type);
        pills.push(pill);
    }

    pills
}

/// Select diverse picks from a pool: prefer one per system, then fill with a cap.
#[cfg(feature = "ssr")]
fn diversify_picks(
    pool: Vec<replay_control_core_server::library_db::GameEntry>,
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

// NOTE: resolve_box_art_for_picks was removed — box art at request time uses
// the DB `box_art_url` field only (set by enrichment pipeline). If NULL, no
// art is available and the UI shows a placeholder.

/// Convert GameEntry to RecommendedGame. box_art_url is resolved later by the caller.
#[cfg(feature = "ssr")]
pub(super) fn to_recommended(
    system: &str,
    rom: &replay_control_core_server::library_db::GameEntry,
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
