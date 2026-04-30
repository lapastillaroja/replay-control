use super::*;
#[cfg(feature = "ssr")]
use replay_control_core_server::library_db::LibraryDb;

/// Developer search result: a matched developer with their games.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeveloperSearchResult {
    pub developer_name: String,
    pub total_count: usize,
    pub games: Vec<RomListEntry>,
    pub other_developers: Vec<DeveloperMatch>,
}

/// An additional developer that matched the search query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeveloperMatch {
    pub name: String,
    pub game_count: usize,
}

/// A group of search results for a single system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemSearchGroup {
    pub system: String,
    pub system_display: String,
    pub total_matches: usize,
    pub top_results: Vec<RomListEntry>,
}

/// Aggregated global search results across all systems.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalSearchResults {
    pub groups: Vec<SystemSearchGroup>,
    pub total_results: usize,
    pub total_systems: usize,
}

// Search scoring logic lives in replay-control-core::search_scoring.
// Re-export for use within this crate.
#[cfg(feature = "ssr")]
pub(crate) use replay_control_core::search_scoring::{search_score, split_into_words};

/// Look up the normalized genre for a ROM on a given system.
#[cfg(feature = "ssr")]
pub(crate) async fn lookup_genre(system: &str, rom_filename: &str) -> String {
    use replay_control_core::systems;
    use replay_control_core_server::arcade_db;
    use replay_control_core_server::game_db;

    let is_arcade = systems::is_arcade_system(system);

    let stem = replay_control_core::title_utils::filename_stem(rom_filename);
    let baked_genre = if is_arcade {
        arcade_db::lookup_arcade_game(system, stem)
            .await
            .map(|info| info.normalized_genre.to_string())
            .unwrap_or_default()
    } else {
        let entry = game_db::lookup_game(system, stem).await;
        let game = match entry {
            Some(e) => Some(e.game),
            None => {
                let normalized = game_db::normalize_filename(stem);
                game_db::lookup_by_normalized_title(system, &normalized).await
            }
        };
        game.map(|g| g.normalized_genre.to_string())
            .unwrap_or_default()
    };

    if !baked_genre.is_empty() {
        return baked_genre;
    }

    // Fallback: check LaunchBox metadata for genre.
    let state = leptos::prelude::expect_context::<crate::api::AppState>();
    if let Some(genre) = state
        .library_pool
        .read({
            let system = system.to_string();
            let rom_filename = rom_filename.to_string();
            move |conn| {
                LibraryDb::lookup(conn, &system, &rom_filename)
                    .ok()
                    .flatten()
                    .and_then(|meta| meta.genre)
                    .filter(|g| !g.is_empty())
            }
        })
        .await
        .flatten()
    {
        return genre;
    }

    String::new()
}

// clippy::too_many_arguments — Leptos server functions require flat parameter lists
// for serialization; wrapping in a struct is not supported by the #[server] macro.
#[allow(clippy::too_many_arguments)]
#[server(prefix = "/sfn")]
pub async fn global_search(
    query: String,
    hide_hacks: bool,
    hide_translations: bool,
    hide_betas: bool,
    hide_clones: bool,
    #[server(default)] multiplayer_only: bool,
    #[server(default)] coop_only: bool,
    #[server(default)] min_rating: Option<f32>,
    genre: String,
    per_system_limit: usize,
    #[server(default)] min_year: Option<u16>,
    #[server(default)] max_year: Option<u16>,
) -> Result<GlobalSearchResults, ServerFnError> {
    use replay_control_core::systems::{self as sys_db};
    use replay_control_core_server::library_db::GameEntry;

    let state = expect_context::<crate::api::AppState>();
    let region_pref = state.region_preference();
    let region_secondary = state.region_preference_secondary();
    let q = query.to_lowercase();
    let per_system_limit = if per_system_limit == 0 {
        3
    } else {
        per_system_limit
    };

    // Empty query with no genre/multiplayer/coop/year filter: no results.
    if q.is_empty()
        && genre.is_empty()
        && !multiplayer_only
        && !coop_only
        && min_year.is_none()
        && max_year.is_none()
    {
        return Ok(GlobalSearchResults {
            groups: Vec::new(),
            total_results: 0,
            total_systems: 0,
        });
    }

    // Split query into words for search_text LIKE matching.
    let query_words: Vec<String> = split_into_words(&q)
        .into_iter()
        .map(|w| w.to_string())
        .collect();

    // Search aliases for cross-name expansion (e.g., "Bare Knuckle" -> "Streets of Rage").
    let alias_base_titles: std::collections::HashMap<String, std::collections::HashSet<String>> =
        if !q.is_empty() {
            let q_owned = q.clone();
            let alias_hits: std::collections::HashSet<(String, String)> = state
                .library_pool
                .read(move |conn| {
                    LibraryDb::search_aliases(conn, &q_owned)
                        .unwrap_or_default()
                        .into_iter()
                        .collect()
                })
                .await
                .unwrap_or_default();
            let mut map: std::collections::HashMap<String, std::collections::HashSet<String>> =
                std::collections::HashMap::new();
            for (sys, bt) in alias_hits {
                map.entry(sys).or_default().insert(bt);
            }
            map
        } else {
            std::collections::HashMap::new()
        };

    // Single DB query: SQL-level pre-filtering on search_text + content filters.
    let min_rating_f64 = min_rating.map(|r| r as f64);
    let genre_owned = genre.clone();
    let candidates: Vec<GameEntry> = state
        .library_pool
        .read(move |conn| {
            let filter = replay_control_core_server::library_db::SearchFilter {
                hide_hacks,
                hide_translations,
                hide_betas,
                hide_clones,
                genre: &genre_owned,
                multiplayer_only,
                coop_only,
                min_rating: min_rating_f64,
                min_year,
                max_year,
            };
            LibraryDb::search_game_library(conn, None, None, &query_words, &filter, 0, usize::MAX)
                .map(|(entries, _total)| entries)
                .unwrap_or_default()
        })
        .await
        .unwrap_or_default();

    // Score the pre-filtered candidates using the existing ranking logic.
    let mut scored: Vec<(u32, GameEntry)> = candidates
        .into_iter()
        .filter_map(|entry| {
            let display = entry.display_name.as_deref().unwrap_or(&entry.rom_filename);

            if q.is_empty() {
                // Filter-only mode (genre/multiplayer with no text query).
                let score = 1000u32.saturating_sub(display.len() as u32);
                return Some((score, entry));
            }

            let mut score = search_score(
                &q,
                display,
                &entry.rom_filename,
                region_pref,
                region_secondary,
            );

            // Alias expansion: if this ROM's base_title was found via alias search,
            // give it a minimum score so it appears in results.
            if score == 0
                && let Some(system_aliases) = alias_base_titles.get(&entry.system)
                && !entry.base_title.is_empty()
                && system_aliases.contains(&entry.base_title)
            {
                score = 350;
            }

            if score > 0 {
                Some((score, entry))
            } else {
                None
            }
        })
        .collect();

    scored.sort_by_key(|s| std::cmp::Reverse(s.0));

    // Group scored results by system.
    let mut system_groups: std::collections::HashMap<String, Vec<(u32, GameEntry)>> =
        std::collections::HashMap::new();
    for (score, entry) in scored {
        system_groups
            .entry(entry.system.clone())
            .or_default()
            .push((score, entry));
    }

    // Collect top entries per system and their metadata for batch enrichment.
    let mut system_meta: Vec<(String, String, usize)> = Vec::new(); // (system, system_display, match_count)
    let mut all_top_entries: Vec<GameEntry> = Vec::new();
    let mut total_results = 0usize;

    for (system, mut system_scored) in system_groups {
        system_scored.sort_by_key(|s| std::cmp::Reverse(s.0));
        let match_count = system_scored.len();
        total_results += match_count;

        let system_display = sys_db::find_system(&system)
            .map(|s| s.display_name.to_string())
            .unwrap_or_else(|| system.clone());

        // Take top N entries for this system.
        let top: Vec<GameEntry> = system_scored
            .into_iter()
            .take(per_system_limit)
            .map(|(_, entry)| entry)
            .collect();

        system_meta.push((system, system_display, match_count));
        all_top_entries.extend(top);
    }

    // Batch-enrich all top entries at once (shared box art + favorites resolution).
    let enriched = super::enrich_game_entries(&state, all_top_entries).await;

    // Re-group enriched entries by system.
    let mut enriched_by_system: std::collections::HashMap<String, Vec<RomListEntry>> =
        std::collections::HashMap::new();
    for entry in enriched {
        enriched_by_system
            .entry(entry.system.clone())
            .or_default()
            .push(entry);
    }

    // Build SystemSearchGroup results.
    let mut groups: Vec<SystemSearchGroup> = system_meta
        .into_iter()
        .map(|(system, system_display, match_count)| {
            let top_results = enriched_by_system.remove(&system).unwrap_or_default();
            SystemSearchGroup {
                system,
                system_display,
                total_matches: match_count,
                top_results,
            }
        })
        .collect();

    // Sort systems by match count descending.
    groups.sort_by_key(|g| std::cmp::Reverse(g.total_matches));
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

    // Use a single SQL query on game_library instead of iterating all ROMs.
    let genres = state
        .library_pool
        .read(move |conn| LibraryDb::all_genre_groups(conn).unwrap_or_default())
        .await
        .unwrap_or_default();

    Ok(genres)
}

/// Get genres available for a specific system.
#[server(prefix = "/sfn")]
pub async fn get_system_genres(system: String) -> Result<Vec<String>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();

    // Use a single SQL query on game_library instead of iterating all ROMs.
    let genres = state
        .library_pool
        .read(move |conn| LibraryDb::system_genre_groups(conn, &system).unwrap_or_default())
        .await
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
    let (region_str, region_secondary_str) = super::region_strings(&state);
    let limit = limit.clamp(1, 30);

    // Single DB access: find matching developers, then fetch games for the top match.
    let q_owned = q.clone();
    let db_result = state
        .library_pool
        .read(move |conn| {
            let matches = LibraryDb::find_developer_matches(conn, &q_owned).unwrap_or_default();
            if matches.is_empty() {
                return None;
            }

            let (top_dev, top_count) = &matches[0];
            let games = LibraryDb::games_by_developer(
                conn,
                top_dev,
                limit,
                &region_str,
                &region_secondary_str,
            )
            .unwrap_or_default();

            let other_developers: Vec<DeveloperMatch> = matches[1..]
                .iter()
                .map(|(name, count)| DeveloperMatch {
                    name: name.clone(),
                    game_count: *count,
                })
                .collect();

            Some((top_dev.clone(), *top_count, games, other_developers))
        })
        .await;

    let Some(Some((developer_name, total_count, game_entries, other_developers))) = db_result
    else {
        return Ok(None);
    };

    if game_entries.is_empty() {
        return Ok(None);
    }

    // Enrich entries: box art, favorites (shared enrichment function).
    let games = super::enrich_game_entries(&state, game_entries).await;

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

    let system_filter: Option<String> = if system.is_empty() {
        None
    } else {
        Some(system)
    };

    let genres = state
        .library_pool
        .read(move |conn| {
            LibraryDb::developer_genre_groups(conn, &developer, system_filter.as_deref())
                .unwrap_or_default()
        })
        .await
        .unwrap_or_default();

    Ok(genres)
}

/// Get paginated game list for a developer, with optional system and content filters.
// clippy::too_many_arguments — Leptos server functions require flat parameter lists
// for serialization; wrapping in a struct is not supported by the #[server] macro.
#[allow(clippy::too_many_arguments)]
#[server(prefix = "/sfn")]
pub async fn get_developer_games(
    developer: String,
    #[server(default)] system: String,
    offset: usize,
    limit: usize,
    #[server(default)] hide_hacks: bool,
    #[server(default)] hide_translations: bool,
    #[server(default)] hide_betas: bool,
    #[server(default)] hide_clones: bool,
    #[server(default)] multiplayer_only: bool,
    #[server(default)] genre: String,
    #[server(default)] min_rating: Option<f32>,
    #[server(default)] min_year: Option<u16>,
    #[server(default)] max_year: Option<u16>,
) -> Result<DeveloperPageData, ServerFnError> {
    use replay_control_core::systems as sys_db;
    use replay_control_core_server::library_db::SearchFilter;

    let state = expect_context::<crate::api::AppState>();
    let limit = limit.clamp(1, 200);

    let system_filter: Option<String> = if system.is_empty() {
        None
    } else {
        Some(system)
    };

    let min_rating_f64 = min_rating.map(|r| r as f64);

    // Fetch systems and paginated games in one DB session.
    let developer_owned = developer.clone();
    let fetch_limit = limit + 1; // fetch one extra to detect has_more
    let db_result = state
        .library_pool
        .read(move |conn| {
            let systems_raw =
                LibraryDb::developer_systems(conn, &developer_owned).unwrap_or_default();
            let filters = SearchFilter {
                hide_hacks,
                hide_translations,
                hide_betas,
                hide_clones,
                multiplayer_only,
                coop_only: false,
                genre: &genre,
                min_rating: min_rating_f64,
                min_year,
                max_year,
            };
            let (entries, total) = LibraryDb::search_game_library(
                conn,
                system_filter.as_deref(),
                Some(&developer_owned),
                &[],
                &filters,
                offset,
                fetch_limit,
            )
            .unwrap_or_default();
            (systems_raw, entries, total)
        })
        .await;

    let Some((systems_raw, mut entries, total)) = db_result else {
        return Ok(DeveloperPageData {
            roms: Vec::new(),
            total: 0,
            has_more: false,
            developer,
            systems: Vec::new(),
        });
    };

    let has_more = entries.len() > limit;
    entries.truncate(limit);

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

    // Enrich entries: box art, favorites, genre (shared enrichment function).
    let list_entries = super::enrich_game_entries(&state, entries).await;

    Ok(DeveloperPageData {
        roms: list_entries,
        total,
        has_more,
        developer,
        systems,
    })
}

// Tests for search scoring have been moved to replay-control-core/src/search_scoring.rs.

/// Pick a random game across all systems.
/// Weighted by system game count so larger collections get proportionally more picks.
/// Returns (system_folder_name, rom_filename).
#[server(prefix = "/sfn")]
pub async fn random_game() -> Result<(String, String), ServerFnError> {
    use rand::RngExt;

    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();
    let systems = state
        .cache
        .cached_systems(&storage, &state.library_pool)
        .await;

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

    // Pick system using a block-scoped RNG so it doesn't live across await points
    // (rand::rng() returns an Rc-based thread-local RNG that isn't Send).
    let chosen_system = {
        let mut rng = rand::rng();
        let pick = rng.random_range(0..total);

        let mut cumulative = 0;
        let mut chosen_system = weighted[0].0.clone();
        for (sys, count) in &weighted {
            cumulative += count;
            if pick < cumulative {
                chosen_system = sys.clone();
                break;
            }
        }
        chosen_system
    };

    // Pick a random ROM filename from L2 (SQLite).
    let sys = chosen_system.clone();
    let filename: Option<String> = state
        .library_pool
        .read(move |conn| {
            conn.query_row(
                "SELECT rom_filename FROM game_library
                 WHERE system = ?1 AND is_special = 0
                 ORDER BY RANDOM() LIMIT 1",
                [&sys],
                |row| row.get(0),
            )
            .ok()
        })
        .await
        .flatten();

    match filename {
        Some(f) => Ok((chosen_system, f)),
        None => Err(ServerFnError::new("No ROMs in selected system")),
    }
}
