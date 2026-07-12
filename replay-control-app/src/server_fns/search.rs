// Server functions need flat parameter lists (`#[server(default)]` per field), so
// several here exceed clippy's arg-count threshold. server_fn 0.8's `#[server]`
// macro no longer propagates a per-fn `#[allow]` to the generated client stub, so
// the allow is module-scoped to cover the macro-generated code.
#![allow(clippy::too_many_arguments)]

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
                only_mature: false,
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
pub async fn get_system_genres(
    system: String,
    #[server(default)] only_mature: bool,
) -> Result<Vec<String>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();

    // Use a single SQL query on game_library instead of iterating all ROMs.
    let genres = state
        .library_reader
        .read(move |conn| {
            LibraryDb::system_genre_groups(conn, &system, only_mature).unwrap_or_default()
        })
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

/// A system that a facet (developer or arcade board) has games on, with game
/// count and display name — the per-system filter chips on a facet page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FacetSystem {
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
    pub systems: Vec<FacetSystem>,
}

/// Shared body for the developer/board genre-filter dropdowns: the distinct
/// genre groups a facet's games fall into, optionally scoped to one system.
#[cfg(feature = "ssr")]
async fn facet_genres(state: &crate::api::AppState, facet: Facet, system: String) -> Vec<String> {
    let system_filter: Option<String> = (!system.is_empty()).then_some(system);
    let (key, is_developer) = match facet {
        Facet::Developer(name) => (name, true),
        Facet::Board(board) => (board.as_tag().to_string(), false),
    };
    state
        .library_reader
        .read(move |conn| {
            if is_developer {
                LibraryDb::developer_genre_groups(conn, &key, system_filter.as_deref())
            } else {
                LibraryDb::board_genre_groups(conn, &key, system_filter.as_deref())
            }
            .unwrap_or_default()
        })
        .await
        .unwrap_or_default()
}

/// Get genres available for a developer's games, optionally filtered by system.
#[server(prefix = "/sfn")]
pub async fn get_developer_genres(
    developer: String,
    #[server(default)] system: String,
) -> Result<Vec<String>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    Ok(facet_genres(&state, Facet::Developer(developer), system).await)
}

/// Which facet a game-list page is scoped to. The developer name and the
/// arcade board differ in how they filter (`search_game_library`'s developer
/// argument vs the `board` field on `SearchFilter`) and in which per-system
/// count query they use — this enum centralizes that dispatch.
#[cfg(feature = "ssr")]
enum Facet {
    Developer(String),
    Board(replay_control_core::arcade_board::ArcadeBoard),
}

/// The content filters shared by every facet page, bundled so the helper
/// doesn't need a flat parameter list (the `#[server]` entry points still do).
#[cfg(feature = "ssr")]
struct FacetFilters {
    hide_hacks: bool,
    hide_translations: bool,
    hide_betas: bool,
    hide_clones: bool,
    multiplayer_only: bool,
    genre: String,
    min_rating: Option<f32>,
    min_year: Option<u16>,
    max_year: Option<u16>,
    has_achievements: bool,
}

/// The facet-agnostic part of a facet page's response.
#[cfg(feature = "ssr")]
struct FacetPage {
    roms: Vec<RomListEntry>,
    total: usize,
    has_more: bool,
    systems: Vec<FacetSystem>,
}

/// Shared body for the developer/board facet game-list pages. The two
/// `#[server]` entry points differ only in their flat parameter list and their
/// response envelope (the macro forces both to stay separate); everything in
/// between — filter assembly, the systems + paginated-games DB session,
/// `has_more`, and enrichment — is identical, so it lives here. Returns `None`
/// only if the DB session itself failed (mirrors the old empty-page fallback).
#[cfg(feature = "ssr")]
async fn facet_games_page(
    state: &crate::api::AppState,
    facet: Facet,
    system: String,
    offset: usize,
    limit: usize,
    filters: FacetFilters,
) -> Option<FacetPage> {
    use replay_control_core::systems::system_display_name;
    use replay_control_core_server::library_db::SearchFilter;

    let limit = limit.clamp(1, 200);
    let system_filter: Option<String> = (!system.is_empty()).then_some(system);
    let fetch_limit = limit + 1; // fetch one extra to detect has_more

    // Resolve the facet-specific bits before the DB closure: the developer
    // search argument, the board filter, and the per-system count key.
    let (developer_arg, board_filter, systems_key) = match facet {
        Facet::Developer(name) => (Some(name.clone()), None, name),
        Facet::Board(board) => (None, Some(board), board.as_tag().to_string()),
    };
    let is_developer = developer_arg.is_some();

    let (systems_raw, mut entries, total) = state
        .library_reader
        .read(move |conn| {
            let systems_raw = if is_developer {
                LibraryDb::developer_systems(conn, &systems_key)
            } else {
                LibraryDb::board_systems(conn, &systems_key)
            }
            .unwrap_or_default();
            let search_filter = SearchFilter {
                hide_hacks: filters.hide_hacks,
                hide_translations: filters.hide_translations,
                hide_betas: filters.hide_betas,
                hide_clones: filters.hide_clones,
                multiplayer_only: filters.multiplayer_only,
                coop_only: false,
                genre: &filters.genre,
                min_rating: filters.min_rating.map(|r| r as f64),
                min_year: filters.min_year,
                max_year: filters.max_year,
                board: board_filter,
                has_achievements: filters.has_achievements,
                only_mature: false,
            };
            let (entries, total) = LibraryDb::search_game_library(
                conn,
                system_filter.as_deref(),
                developer_arg.as_deref(),
                &[],
                &search_filter,
                offset,
                fetch_limit,
            )
            .unwrap_or_default();
            (systems_raw, entries, total)
        })
        .await?;

    let has_more = entries.len() > limit;
    entries.truncate(limit);

    let systems = systems_raw
        .into_iter()
        .map(|(sys, count)| FacetSystem {
            system_display: system_display_name(&sys),
            system: sys,
            game_count: count,
        })
        .collect();

    let roms = super::enrich_game_entries(state, entries).await;
    Some(FacetPage {
        roms,
        total,
        has_more,
        systems,
    })
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
    let state = expect_context::<crate::api::AppState>();
    let filters = FacetFilters {
        hide_hacks,
        hide_translations,
        hide_betas,
        hide_clones,
        multiplayer_only,
        genre,
        min_rating,
        min_year,
        max_year,
        has_achievements,
    };
    let page = facet_games_page(
        &state,
        Facet::Developer(developer.clone()),
        system,
        offset,
        limit,
        filters,
    )
    .await;

    Ok(match page {
        Some(p) => DeveloperPageData {
            roms: p.roms,
            total: p.total,
            has_more: p.has_more,
            developer,
            systems: p.systems,
        },
        None => DeveloperPageData {
            roms: Vec::new(),
            total: 0,
            has_more: false,
            developer,
            systems: Vec::new(),
        },
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
    pub systems: Vec<FacetSystem>,
}

/// Get genres available for a board's games, optionally filtered by system.
#[server(prefix = "/sfn")]
pub async fn get_board_genres(
    board_tag: String,
    #[server(default)] system: String,
) -> Result<Vec<String>, ServerFnError> {
    use replay_control_core::arcade_board::ArcadeBoard;
    let state = expect_context::<crate::api::AppState>();
    // Unknown tag → no genres (an unresolvable board has no games either).
    match ArcadeBoard::from_tag(&board_tag) {
        Some(board) => Ok(facet_genres(&state, Facet::Board(board), system).await),
        None => Ok(Vec::new()),
    }
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

    let state = expect_context::<crate::api::AppState>();

    // Resolve tag → enum once, up front. Unknown tag → empty page (the UI
    // shows the "no games" empty state with the raw tag as the title).
    let board_enum = ArcadeBoard::from_tag(&board_tag);
    let board_display_name = board_enum
        .map(|b| b.display_label())
        .unwrap_or_else(|| board_tag.clone());

    let Some(board_enum) = board_enum else {
        return Ok(BoardPageData {
            roms: Vec::new(),
            total: 0,
            has_more: false,
            board_tag,
            board_display_name,
            systems: Vec::new(),
        });
    };

    let filters = FacetFilters {
        hide_hacks,
        hide_translations,
        hide_betas,
        hide_clones,
        multiplayer_only,
        genre,
        min_rating,
        min_year,
        max_year,
        has_achievements,
    };
    let page = facet_games_page(
        &state,
        Facet::Board(board_enum),
        system,
        offset,
        limit,
        filters,
    )
    .await;

    Ok(match page {
        Some(p) => BoardPageData {
            roms: p.roms,
            total: p.total,
            has_more: p.has_more,
            board_tag,
            board_display_name,
            systems: p.systems,
        },
        None => BoardPageData {
            roms: Vec::new(),
            total: 0,
            has_more: false,
            board_tag,
            board_display_name,
            systems: Vec::new(),
        },
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
