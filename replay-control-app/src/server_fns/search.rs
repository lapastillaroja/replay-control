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

/// A structured filter the search recognizer extracted from the user's free-text
/// query (e.g. typing "CPS-2" auto-routes to a board filter). Surfaces to the UI
/// for the per-system pill on `RomPage`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RecognizedFilter {
    /// `ArcadeBoard::display_label()` of the matched board, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub board: Option<String>,
    /// The query text after the recognizer consumed its tokens. Powers the
    /// pill's "remove filter" button: clicking ✕ navigates the page to the
    /// same route with `q = remaining_query`, dropping the structured term.
    #[serde(default)]
    pub remaining_query: String,
}

impl RecognizedFilter {
    pub fn is_empty(&self) -> bool {
        self.board.is_none()
    }
}

/// A board match surfaced on `/search` — name + library game count. Drives
/// both the top "Games on …" preview block and the "Other arcade boards
/// matching" list below it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardMatch {
    /// `ArcadeBoard::as_tag()` — stable URL/storage slug.
    pub tag: String,
    /// `ArcadeBoard::display_label()` — what the user reads on the card.
    pub display_name: String,
    /// Distinct base-title count on this board in the user's library
    /// (clones / translations / hacks / specials excluded, matching the
    /// developer-count semantics on the same page).
    pub game_count: usize,
}

/// Top board search result on `/search`. Mirrors `DeveloperSearchResult`:
/// the top match renders a `BoardBlock` preview (3 thumbnails + see-all
/// link), and additional matches show up in `other_boards`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardSearchResult {
    pub board_tag: String,
    pub board_display_name: String,
    pub total_count: usize,
    pub games: Vec<RomListEntry>,
    pub other_boards: Vec<BoardMatch>,
}

/// Aggregated global search results across all systems.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalSearchResults {
    pub groups: Vec<SystemSearchGroup>,
    pub total_results: usize,
    pub total_systems: usize,
}

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

    // Fallback: read the genre that enrichment already wrote to
    // `game_library.genre` (populated from LaunchBox when the catalog had
    // nothing). Single library-pool acquire — no cross-pool launchbox
    // lookup needed.
    let state = leptos::prelude::expect_context::<crate::api::AppState>();
    let system = system.to_string();
    let rom_filename = rom_filename.to_string();
    state
        .library_reader
        .read(move |conn| {
            replay_control_core_server::library_db::LibraryDb::rom_genre(
                conn,
                &system,
                &rom_filename,
            )
            .unwrap_or_default()
        })
        .await
        .unwrap_or_default()
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
    #[server(default)] has_achievements: bool,
    #[server(default)] min_rating: Option<f32>,
    genre: String,
    per_system_limit: usize,
    #[server(default)] min_year: Option<u16>,
    #[server(default)] max_year: Option<u16>,
) -> Result<GlobalSearchResults, ServerFnError> {
    use replay_control_core::systems::{self as sys_db};
    use replay_control_core_server::library_db::{GameEntry, LibraryDb};

    let state = expect_context::<crate::api::AppState>();
    let region_pref = state.region_preference();
    let region_secondary = state.region_preference_secondary();

    let q = query.trim().to_lowercase();

    let per_system_limit = if per_system_limit == 0 {
        3
    } else {
        per_system_limit
    };

    // Empty query with no positive content filter: no results.
    if q.is_empty()
        && genre.is_empty()
        && !multiplayer_only
        && !coop_only
        && min_rating.is_none()
        && min_year.is_none()
        && max_year.is_none()
    {
        return Ok(GlobalSearchResults {
            groups: Vec::new(),
            total_results: 0,
            total_systems: 0,
        });
    }

    let min_rating_f64 = min_rating.map(|r| r as f64);
    let genre_owned = genre.clone();
    let candidates = state
        .library_reader
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
                board: None,
                has_achievements,
            };
            LibraryDb::search_game_library_ranked(
                conn,
                None,
                &q,
                &filter,
                0,
                usize::MAX,
                region_pref,
                region_secondary,
            )
            .map(|(entries, _total)| entries)
            .unwrap_or_default()
        })
        .await
        .unwrap_or_default();

    // Group ranked results by system while preserving ranked order within each group.
    let mut system_groups: std::collections::HashMap<String, Vec<_>> =
        std::collections::HashMap::new();
    for entry in candidates {
        system_groups
            .entry(entry.system.clone())
            .or_default()
            .push(entry);
    }

    // Collect top entries per system and their metadata for batch enrichment.
    let mut system_meta: Vec<(String, String, usize)> = Vec::new(); // (system, system_display, match_count)
    let mut all_top_entries: Vec<GameEntry> = Vec::new();
    let mut total_results = 0usize;

    for (system, system_entries) in system_groups {
        let match_count = system_entries.len();
        total_results += match_count;

        let system_display = sys_db::system_display_name(&system);

        // Take top N entries for this system.
        let top = system_entries
            .into_iter()
            .take(per_system_limit)
            .collect::<Vec<_>>();

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
        .library_reader
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
        .library_reader
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
        .library_reader
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
        .library_reader
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
    #[server(default)] has_achievements: bool,
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
        .library_reader
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
                board: None,
                has_achievements,
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
            let display = sys_db::system_display_name(&sys);
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

/// Find arcade boards matching the query and surface the top one with a
/// 3-game preview + an "Other arcade boards matching" list. Mirrors
/// `search_by_developer`.
///
/// The recognizer is broader than for implicit filtering: any board whose
/// display-name / tag / synonym contains the query string scores in. Only
/// boards with at least one game in the user's library are returned, so the
/// click-through `/board/<tag>` page always has something to show.
#[server(prefix = "/sfn")]
pub async fn search_by_board(
    query: String,
    limit: usize,
) -> Result<Option<BoardSearchResult>, ServerFnError> {
    use replay_control_core_server::library::search_recognizer::find_board_matches;

    let q = query.trim();
    if q.len() < 2 {
        return Ok(None);
    }

    let state = expect_context::<crate::api::AppState>();
    let (region_str, region_secondary_str) = super::region_strings(&state);
    let limit = limit.clamp(1, 30);

    // Rank-only step. No DB hit yet.
    let ranked_boards = find_board_matches(q);
    if ranked_boards.is_empty() {
        return Ok(None);
    }

    // Single DB acquire: count every candidate board in one grouped query,
    // keep those with hits (preserving the recognizer's ranking order), then
    // fetch the top-N games for the winner.
    let db_result = state
        .library_reader
        .read(move |conn| {
            let tags: Vec<&str> = ranked_boards.iter().map(|b| b.as_tag()).collect();
            let counts = LibraryDb::board_game_counts(conn, &tags).unwrap_or_default();
            let mut with_counts: Vec<(replay_control_core::arcade_board::ArcadeBoard, usize)> =
                ranked_boards
                    .into_iter()
                    .filter_map(|board| {
                        counts
                            .get(board.as_tag())
                            .copied()
                            .filter(|&c| c > 0)
                            .map(|c| (board, c))
                    })
                    .collect();
            if with_counts.is_empty() {
                return None;
            }
            let (top_board, top_count) = with_counts.remove(0);
            let games = LibraryDb::games_by_board(
                conn,
                top_board.as_tag(),
                limit,
                &region_str,
                &region_secondary_str,
            )
            .unwrap_or_default();
            let other_boards: Vec<BoardMatch> = with_counts
                .into_iter()
                .map(|(b, c)| BoardMatch {
                    tag: b.as_tag().to_string(),
                    display_name: b.display_label(),
                    game_count: c,
                })
                .collect();
            Some((
                top_board.as_tag().to_string(),
                top_board.display_label(),
                top_count,
                games,
                other_boards,
            ))
        })
        .await;

    let Some(Some((board_tag, board_display_name, total_count, game_entries, other_boards))) =
        db_result
    else {
        return Ok(None);
    };

    if game_entries.is_empty() {
        return Ok(None);
    }

    let games = super::enrich_game_entries(&state, game_entries).await;

    Ok(Some(BoardSearchResult {
        board_tag,
        board_display_name,
        total_count,
        games,
        other_boards,
    }))
}

/// A system that has games on a given arcade board, with game count and
/// display name. Mirrors `DeveloperSystem`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardSystem {
    pub system: String,
    pub system_display: String,
    pub game_count: usize,
}

/// Response for the board game list page. Mirrors `DeveloperPageData`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardPageData {
    pub roms: Vec<RomListEntry>,
    pub total: usize,
    pub has_more: bool,
    /// `ArcadeBoard::as_tag()` slug — stable URL value.
    pub board_tag: String,
    /// `ArcadeBoard::display_label()` — what the title bar shows.
    pub board_display_name: String,
    pub systems: Vec<BoardSystem>,
}

/// Get genres available for a board's games, optionally filtered by system.
#[server(prefix = "/sfn")]
pub async fn get_board_genres(
    board_tag: String,
    #[server(default)] system: String,
) -> Result<Vec<String>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();

    let system_filter: Option<String> = if system.is_empty() {
        None
    } else {
        Some(system)
    };

    let genres = state
        .library_reader
        .read(move |conn| {
            LibraryDb::board_genre_groups(conn, &board_tag, system_filter.as_deref())
                .unwrap_or_default()
        })
        .await
        .unwrap_or_default();

    Ok(genres)
}

/// Get paginated game list for a board, with optional system and content filters.
// clippy::too_many_arguments — Leptos server functions require flat parameter lists
// for serialization; wrapping in a struct is not supported by the #[server] macro.
#[allow(clippy::too_many_arguments)]
#[server(prefix = "/sfn")]
pub async fn get_board_games(
    board_tag: String,
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
    #[server(default)] has_achievements: bool,
) -> Result<BoardPageData, ServerFnError> {
    use replay_control_core::arcade_board::ArcadeBoard;
    use replay_control_core::systems as sys_db;
    use replay_control_core_server::library_db::SearchFilter;

    let state = expect_context::<crate::api::AppState>();
    let limit = limit.clamp(1, 200);

    // Resolve tag → enum once, up front. Unknown tag → empty page (the UI
    // shows the "no games" empty state with the raw tag as the title).
    let board_enum = ArcadeBoard::from_tag(&board_tag);
    let board_display_name = board_enum
        .map(|b| b.display_label())
        .unwrap_or_else(|| board_tag.clone());

    if board_enum.is_none() {
        return Ok(BoardPageData {
            roms: Vec::new(),
            total: 0,
            has_more: false,
            board_tag,
            board_display_name,
            systems: Vec::new(),
        });
    }

    let system_filter: Option<String> = if system.is_empty() {
        None
    } else {
        Some(system)
    };

    let min_rating_f64 = min_rating.map(|r| r as f64);

    let tag_for_db = board_tag.clone();
    let fetch_limit = limit + 1;
    let db_result = state
        .library_reader
        .read(move |conn| {
            let systems_raw = LibraryDb::board_systems(conn, &tag_for_db).unwrap_or_default();
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
                board: board_enum,
                has_achievements,
            };
            let (entries, total) = LibraryDb::search_game_library(
                conn,
                system_filter.as_deref(),
                None,
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
        return Ok(BoardPageData {
            roms: Vec::new(),
            total: 0,
            has_more: false,
            board_tag,
            board_display_name,
            systems: Vec::new(),
        });
    };

    let has_more = entries.len() > limit;
    entries.truncate(limit);

    let systems: Vec<BoardSystem> = systems_raw
        .into_iter()
        .map(|(sys, count)| {
            let display = sys_db::system_display_name(&sys);
            BoardSystem {
                system: sys,
                system_display: display,
                game_count: count,
            }
        })
        .collect();

    let list_entries = super::enrich_game_entries(&state, entries).await;

    Ok(BoardPageData {
        roms: list_entries,
        total,
        has_more,
        board_tag,
        board_display_name,
        systems,
    })
}

// Tests for search scoring have been moved to replay-control-core/src/search_scoring.rs.

/// Pick a random game across all systems.
/// Weighted by system game count so larger collections get proportionally more picks.
/// Returns (system_folder_name, rom_filename).
#[server(prefix = "/sfn")]
pub async fn random_game() -> Result<(String, String), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let random = state
        .library_reader
        .read(LibraryDb::random_library_rom)
        .await
        .transpose()
        .map_err(|e| ServerFnError::new(e.to_string()))?
        .flatten();

    match random {
        Some((system, filename)) => Ok((system, filename)),
        None => Err(ServerFnError::new("No games available")),
    }
}

/// Pick a random game from one system.
#[server(prefix = "/sfn")]
pub async fn random_game_for_system(system: String) -> Result<(String, String), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let random = state
        .library_reader
        .read(move |conn| LibraryDb::random_library_rom_for_system(conn, &system))
        .await
        .transpose()
        .map_err(|e| ServerFnError::new(e.to_string()))?
        .flatten();

    match random {
        Some((system, filename)) => Ok((system, filename)),
        None => Err(ServerFnError::new("No games available for this system")),
    }
}
