use super::*;
#[cfg(feature = "ssr")]
use replay_control_core_server::library_db::LibraryDb;

/// A favorite enriched with box art URL and genre.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FavoriteWithArt {
    #[serde(flatten)]
    pub fav: Favorite,
    pub box_art_url: Option<String>,
    /// Genre string for display (e.g., "Platform", "Beat 'em Up").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
}

/// Result of organizing favorites into subfolders.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct OrganizeResult {
    pub organized: usize,
    pub skipped: usize,
}

#[server(prefix = "/sfn", endpoint = "/get_favorites")]
pub async fn get_favorites() -> Result<Vec<FavoriteWithArt>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let favs = replay_control_core_server::favorites::list_favorites(&state.storage())
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    Ok(enrich_favorites(&state, favs).await)
}

#[server(prefix = "/sfn")]
pub async fn get_system_favorites(system: String) -> Result<Vec<FavoriteWithArt>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let favs =
        replay_control_core_server::favorites::list_favorites_for_system(&state.storage(), &system)
            .await
            .map_err(|e| ServerFnError::new(e.to_string()))?;
    Ok(enrich_favorites(&state, favs).await)
}

/// Pair each favorite with its DB-backed `box_art_url` and `genre_group`.
///
/// One batch DB read keyed by `(system, rom_filename)`. Keys are built once
/// and consumed inside the read closure so the post-read zip doesn't need a
/// second `.clone()` per favorite.
#[cfg(feature = "ssr")]
async fn enrich_favorites(
    state: &crate::api::AppState,
    favs: Vec<Favorite>,
) -> Vec<FavoriteWithArt> {
    let keys: Vec<(String, String)> = favs
        .iter()
        .map(|f| (f.game.system.clone(), f.game.rom_filename.clone()))
        .collect();
    let favs_count = favs.len();

    let art_genre: Vec<(Option<String>, Option<String>)> = state
        .library_pool
        .read(move |conn| {
            let entries = LibraryDb::lookup_game_entries(conn, &keys).unwrap_or_default();
            keys.iter()
                .map(|k| {
                    let entry = entries.get(k);
                    let box_art_url = entry.and_then(|e| e.box_art_url.clone());
                    let genre = entry
                        .map(|e| &e.genre_group)
                        .filter(|g| !g.is_empty())
                        .cloned();
                    (box_art_url, genre)
                })
                .collect()
        })
        .await
        .unwrap_or_else(|| vec![(None, None); favs_count]);

    favs.into_iter()
        .zip(art_genre)
        .map(|(fav, (box_art_url, genre))| FavoriteWithArt {
            fav,
            box_art_url,
            genre,
        })
        .collect()
}

#[server(prefix = "/sfn")]
pub async fn add_favorite(
    system: String,
    rom_path: String,
    grouped: bool,
) -> Result<Favorite, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let result = replay_control_core_server::favorites::add_favorite(
        &state.storage(),
        &system,
        &rom_path,
        grouped,
    )
    .await
    .map_err(|e| ServerFnError::new(e.to_string()))?;
    state.cache.invalidate_favorites().await;
    state.response_cache.invalidate_all();
    Ok(result)
}

#[server(prefix = "/sfn")]
pub async fn remove_favorite(
    filename: String,
    subfolder: Option<String>,
) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();

    match &subfolder {
        Some(s) if !s.is_empty() => {
            // Caller knows the subfolder — remove from that specific location.
            replay_control_core_server::favorites::remove_favorite(&storage, &filename, Some(s))
                .map_err(|e| ServerFnError::new(e.to_string()))?;
        }
        _ => {
            // Caller doesn't know the subfolder (e.g., game detail page).
            // Remove from all locations (root + all subfolders) since the
            // same .fav may exist in multiple places after reorganization.
            replay_control_core_server::favorites::remove_favorite_everywhere(&storage, &filename)
                .map_err(|e| ServerFnError::new(e.to_string()))?;
        }
    }

    state.cache.invalidate_favorites().await;
    state.response_cache.invalidate_all();
    Ok(())
}

#[server(prefix = "/sfn")]
pub async fn organize_favorites(
    primary: OrganizeCriteria,
    secondary: Option<OrganizeCriteria>,
    keep_originals: bool,
) -> Result<OrganizeResult, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let needs_ratings =
        primary == OrganizeCriteria::Rating || secondary == Some(OrganizeCriteria::Rating);
    let ratings = if needs_ratings {
        state
            .library_pool
            .read(|conn| replay_control_core_server::library_db::LibraryDb::all_ratings(conn).ok())
            .await
            .flatten()
    } else {
        None
    };
    let result = replay_control_core_server::favorites::organize_favorites(
        &state.storage(),
        primary,
        secondary,
        keep_originals,
        ratings.as_ref(),
    )
    .await
    .map_err(|e| ServerFnError::new(e.to_string()))?;
    state.cache.invalidate_favorites().await;
    state.response_cache.invalidate_all();
    Ok(OrganizeResult {
        organized: result.organized,
        skipped: result.skipped,
    })
}

#[server(prefix = "/sfn")]
pub async fn group_favorites() -> Result<usize, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let result = replay_control_core_server::favorites::group_by_system(&state.storage())
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    state.cache.invalidate_favorites().await;
    state.response_cache.invalidate_all();
    Ok(result)
}

#[server(prefix = "/sfn")]
pub async fn flatten_favorites() -> Result<usize, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let result = replay_control_core_server::favorites::flatten_favorites(&state.storage())
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    state.cache.invalidate_favorites().await;
    state.response_cache.invalidate_all();
    Ok(result)
}

/// Get personalized recommendation sections for the favorites page.
///
/// Returns up to 2 sections:
/// - "Because You Played [Game]": games similar to a random recent game by genre/developer
/// - "More from [Series]": series siblings of favorited games that aren't yet favorited
#[server(prefix = "/sfn")]
pub async fn get_favorites_recommendations() -> Result<Vec<super::GameSection>, ServerFnError> {
    #[cfg(feature = "ssr")]
    let fn_start = std::time::Instant::now();
    use super::{GameSection, RecommendedGame};

    let state = expect_context::<crate::api::AppState>();

    if let Some(cached) = state.response_cache.favorites_recommendations.get() {
        #[cfg(feature = "ssr")]
        tracing::debug!(
            elapsed_ms = fn_start.elapsed().as_millis(),
            "get_favorites_recommendations cache hit"
        );
        return Ok(cached);
    }

    let storage = state.storage();
    let systems = state
        .cache
        .cached_systems(&storage, &state.library_pool)
        .await;

    let (region_str, region_secondary_str) = super::region_strings(&state);

    let recents = state.cache.get_recents(&storage).await.unwrap_or_default();
    let all_favorites = state
        .cache
        .get_all_favorited_systems(&storage)
        .await
        .unwrap_or_default();

    // Build Vec of all favorite keys, then derive HashSet for O(1) exclusion checks.
    let fav_keys_vec: Vec<(String, String)> = all_favorites
        .iter()
        .flat_map(|(system, filenames)| filenames.iter().map(move |f| (system.clone(), f.clone())))
        .collect();
    let fav_keys: std::collections::HashSet<(String, String)> =
        fav_keys_vec.iter().cloned().collect();

    // Pick a seed game for "Because You Love..." section.
    // Prefer a favorite (user explicitly likes it, better recommendations).
    // Fall back to a recent game if no favorites.
    let seed_game: Option<(String, String)> = {
        use rand::seq::IndexedRandom;
        let mut rng = rand::rng();
        if !fav_keys_vec.is_empty() {
            fav_keys_vec.choose(&mut rng).cloned()
        } else if !recents.is_empty() {
            recents
                .choose(&mut rng)
                .map(|r| (r.game.system.clone(), r.game.rom_filename.clone()))
        } else {
            None
        }
    };

    // DB closure: run all queries under one connection.
    let db_result = state
        .library_pool
        .read(move |conn| {
            #[allow(clippy::type_complexity)]
            let mut sections: Vec<(
                String,
                Vec<String>,
                Vec<replay_control_core_server::library_db::GameEntry>,
                Option<String>,
            )> = Vec::new();

            // Batch lookup: all favorites + seed game (if from recents) in one query.
            let mut all_keys = fav_keys_vec;
            if let Some(ref seed) = seed_game
                && !fav_keys.contains(seed)
            {
                all_keys.push(seed.clone());
            }
            let all_entries = LibraryDb::lookup_game_entries(conn, &all_keys).unwrap_or_default();

            // --- "Because You Love [Game]" ---
            if let Some(ref seed) = seed_game
                && let Some(seed_entry) = all_entries.get(seed)
            {
                let genre = if seed_entry.genre_group.is_empty() {
                    None
                } else {
                    Some(seed_entry.genre_group.as_str())
                };
                let developer = if seed_entry.developer.is_empty() {
                    None
                } else {
                    Some(seed_entry.developer.as_str())
                };

                // Find similar games by genre (cross-system) excluding favorites and seed.
                let mut similar = Vec::new();
                if let Some(genre) = genre {
                    let by_genre = LibraryDb::top_rated_filtered(
                        conn,
                        None,
                        Some(genre),
                        None,
                        30,
                        &region_str,
                        &region_secondary_str,
                    )
                    .unwrap_or_default();
                    for g in by_genre {
                        if !fav_keys.contains(&(g.system.clone(), g.rom_filename.clone()))
                            && g.rom_filename != seed.1
                        {
                            similar.push(g);
                        }
                    }
                }
                // Fill with developer matches if not enough genre matches.
                if similar.len() < 6
                    && let Some(dev) = developer
                {
                    let by_dev = LibraryDb::top_rated_filtered(
                        conn,
                        None,
                        None,
                        Some(dev),
                        20,
                        &region_str,
                        &region_secondary_str,
                    )
                    .unwrap_or_default();
                    let have: std::collections::HashSet<String> =
                        similar.iter().map(|g| g.rom_filename.clone()).collect();
                    for g in by_dev {
                        if similar.len() >= 12 {
                            break;
                        }
                        if !fav_keys.contains(&(g.system.clone(), g.rom_filename.clone()))
                            && g.rom_filename != seed.1
                            && !have.contains(&g.rom_filename)
                        {
                            similar.push(g);
                        }
                    }
                }

                if similar.len() >= 3 {
                    let raw_name = seed_entry
                        .display_name
                        .as_deref()
                        .unwrap_or(&seed_entry.rom_filename);
                    let display = replay_control_core::title_utils::strip_tags(raw_name);
                    sections.push((
                        "SpotlightBecauseYouLove".to_string(),
                        vec![display.to_string()],
                        similar,
                        None,
                    ));
                }
            }

            // --- "More from [Series]" ---
            if !all_entries.is_empty() {
                let mut series_map: std::collections::HashMap<String, String> =
                    std::collections::HashMap::new();
                for entry in all_entries.values() {
                    if !entry.series_key.is_empty() && !series_map.contains_key(&entry.series_key) {
                        let display =
                            LibraryDb::lookup_series_name(conn, &entry.system, &entry.base_title)
                                .unwrap_or_else(|| title_case(&entry.series_key));
                        series_map.insert(entry.series_key.clone(), display);
                    }
                }

                // Pick a random series with non-favorited siblings.
                let mut series_keys: Vec<(String, String)> = series_map.into_iter().collect();
                if !series_keys.is_empty() {
                    use rand::seq::SliceRandom;
                    series_keys.shuffle(&mut rand::rng());

                    for (skey, stitle) in &series_keys {
                        let siblings = LibraryDb::series_siblings(
                            conn,
                            skey,
                            "", // empty base_title so all series members are returned
                            &region_str,
                            30,
                        )
                        .unwrap_or_default();

                        let non_fav: Vec<_> = siblings
                            .into_iter()
                            .filter(|g| {
                                !fav_keys.contains(&(g.system.clone(), g.rom_filename.clone()))
                            })
                            .collect();

                        if non_fav.len() >= 2 {
                            sections.push((
                                "SpotlightMoreFrom".to_string(),
                                vec![stitle.clone()],
                                non_fav,
                                None,
                            ));
                            break;
                        }
                    }
                }
            }

            sections
        })
        .await;
    #[cfg(feature = "ssr")]
    tracing::debug!(
        elapsed_ms = fn_start.elapsed().as_millis(),
        "get_favorites_recommendations db_read complete"
    );

    let raw_sections = db_result.unwrap_or_default();

    // Convert GameEntry to RecommendedGame.
    // Box art comes from the DB `box_art_url` field (set by enrichment pipeline).
    let mut result_sections = Vec::new();
    for (title_key, title_args, games, see_all_href) in raw_sections {
        let picks: Vec<RecommendedGame> = games
            .iter()
            .take(6)
            .filter_map(|rom| super::to_recommended(&rom.system, rom, &systems))
            .collect();
        if picks.is_empty() {
            continue;
        }
        result_sections.push(GameSection {
            title_key,
            title_args,
            games: picks,
            see_all_href,
        });
    }

    state
        .response_cache
        .favorites_recommendations
        .set(result_sections.clone());

    #[cfg(feature = "ssr")]
    tracing::info!(
        elapsed_ms = fn_start.elapsed().as_millis(),
        "get_favorites_recommendations complete"
    );
    Ok(result_sections)
}

/// Title-case a string: capitalize the first letter of each word.
#[cfg(feature = "ssr")]
fn title_case(s: &str) -> String {
    s.split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => {
                    let upper: String = c.to_uppercase().collect();
                    upper + chars.as_str()
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
