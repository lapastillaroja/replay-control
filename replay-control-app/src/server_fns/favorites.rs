use super::*;
#[cfg(feature = "ssr")]
use replay_control_core::metadata_db::MetadataDb;

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

/// Recommendation sections for the favorites page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FavoritesRecommendations {
    pub sections: Vec<super::GameSection>,
}

#[server(prefix = "/sfn", endpoint = "/get_favorites")]
pub async fn get_favorites() -> Result<Vec<FavoriteWithArt>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let favs = replay_control_core::favorites::list_favorites(&state.storage())
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    // Collect (system, rom_filename) keys for batch DB lookup.
    let keys: Vec<(String, String)> = favs
        .iter()
        .map(|f| (f.game.system.clone(), f.game.rom_filename.clone()))
        .collect();

    // Batch-load game entries (box_art_url + genre) and genre map in one DB read.
    let distinct_systems: std::collections::HashSet<String> =
        favs.iter().map(|f| f.game.system.clone()).collect();
    let systems_vec: Vec<String> = distinct_systems.into_iter().collect();
    let (db_entries, genre_map) = state
        .cache
        .db_read(move |conn| {
            let entries = MetadataDb::lookup_game_entries(conn, &keys).unwrap_or_default();
            let mut gmap = std::collections::HashMap::new();
            for sys in &systems_vec {
                if let Ok(genres) = MetadataDb::system_rom_genres(conn, sys) {
                    for (filename, genre) in genres {
                        gmap.insert((sys.clone(), filename), genre);
                    }
                }
            }
            (entries, gmap)
        })
        .await
        .unwrap_or_default();

    // Only build image indexes for systems that have entries missing box_art_url.
    // Note: FavoriteWithArt doesn't need is_favorite (they're all favorites) — skip
    // the shared enrich_box_art_and_favorites() to avoid redundant favorites loading.
    let needs_index: std::collections::HashSet<&str> = favs
        .iter()
        .filter(|f| {
            db_entries
                .get(&(f.game.system.clone(), f.game.rom_filename.clone()))
                .and_then(|e| e.box_art_url.as_ref())
                .is_none()
        })
        .map(|f| f.game.system.as_str())
        .collect();

    let mut image_indexes: std::collections::HashMap<
        String,
        std::sync::Arc<crate::api::cache::ImageIndex>,
    > = std::collections::HashMap::new();
    for sys in &needs_index {
        let index = state.cache.cached_image_index(&state, sys).await;
        image_indexes.insert(sys.to_string(), index);
    }

    let results: Vec<FavoriteWithArt> = favs
        .into_iter()
        .map(|fav| {
            let db_box_art = db_entries
                .get(&(fav.game.system.clone(), fav.game.rom_filename.clone()))
                .and_then(|e| e.box_art_url.clone());
            let box_art_url = db_box_art.or_else(|| {
                image_indexes.get(&fav.game.system).and_then(|index| {
                    state.cache.resolve_box_art(
                        &state,
                        index,
                        &fav.game.system,
                        &fav.game.rom_filename,
                    )
                })
            });
            let genre = genre_map
                .get(&(fav.game.system.clone(), fav.game.rom_filename.clone()))
                .filter(|g| !g.is_empty())
                .cloned();
            FavoriteWithArt {
                fav,
                box_art_url,
                genre,
            }
        })
        .collect();
    Ok(results)
}

#[server(prefix = "/sfn")]
pub async fn get_system_favorites(system: String) -> Result<Vec<FavoriteWithArt>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let favs = replay_control_core::favorites::list_favorites_for_system(&state.storage(), &system)
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    // Collect keys for batch DB lookup.
    let keys: Vec<(String, String)> = favs
        .iter()
        .map(|f| (f.game.system.clone(), f.game.rom_filename.clone()))
        .collect();

    // Batch-load game entries (box_art_url) and genre map in one DB read.
    let sys = system.clone();
    let (db_entries, genre_map) = state
        .cache
        .db_read(move |conn| {
            let entries = MetadataDb::lookup_game_entries(conn, &keys).unwrap_or_default();
            let genres: std::collections::HashMap<String, String> =
                MetadataDb::system_rom_genres(conn, &sys).unwrap_or_default();
            (entries, genres)
        })
        .await
        .unwrap_or_default();

    // Only build image index if some entries are missing box_art_url.
    // Note: FavoriteWithArt doesn't need is_favorite — skip the shared
    // enrich_box_art_and_favorites() to avoid redundant favorites loading.
    let needs_fallback = favs.iter().any(|f| {
        db_entries
            .get(&(f.game.system.clone(), f.game.rom_filename.clone()))
            .and_then(|e| e.box_art_url.as_ref())
            .is_none()
    });
    let image_index = if needs_fallback {
        Some(state.cache.cached_image_index(&state, &system).await)
    } else {
        None
    };

    let results: Vec<FavoriteWithArt> = favs
        .into_iter()
        .map(|fav| {
            let db_box_art = db_entries
                .get(&(fav.game.system.clone(), fav.game.rom_filename.clone()))
                .and_then(|e| e.box_art_url.clone());
            let box_art_url = db_box_art.or_else(|| {
                image_index.as_ref().and_then(|index| {
                    state.cache.resolve_box_art(
                        &state,
                        index,
                        &fav.game.system,
                        &fav.game.rom_filename,
                    )
                })
            });
            let genre = genre_map
                .get(&fav.game.rom_filename)
                .filter(|g| !g.is_empty())
                .cloned();
            FavoriteWithArt {
                fav,
                box_art_url,
                genre,
            }
        })
        .collect();
    Ok(results)
}

#[server(prefix = "/sfn")]
pub async fn add_favorite(
    system: String,
    rom_path: String,
    grouped: bool,
) -> Result<Favorite, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let result =
        replay_control_core::favorites::add_favorite(&state.storage(), &system, &rom_path, grouped)
            .map_err(|e| ServerFnError::new(e.to_string()))?;
    state.cache.invalidate_favorites();
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
            replay_control_core::favorites::remove_favorite(&storage, &filename, Some(s))
                .map_err(|e| ServerFnError::new(e.to_string()))?;
        }
        _ => {
            // Caller doesn't know the subfolder (e.g., game detail page).
            // Remove from all locations (root + all subfolders) since the
            // same .fav may exist in multiple places after reorganization.
            replay_control_core::favorites::remove_favorite_everywhere(&storage, &filename)
                .map_err(|e| ServerFnError::new(e.to_string()))?;
        }
    }

    state.cache.invalidate_favorites();
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
            .metadata_pool
            .read(|conn| replay_control_core::metadata_db::MetadataDb::all_ratings(conn).ok())
            .await
            .flatten()
    } else {
        None
    };
    let result = replay_control_core::favorites::organize_favorites(
        &state.storage(),
        primary,
        secondary,
        keep_originals,
        ratings.as_ref(),
    )
    .map_err(|e| ServerFnError::new(e.to_string()))?;
    state.cache.invalidate_favorites();
    Ok(OrganizeResult {
        organized: result.organized,
        skipped: result.skipped,
    })
}

#[server(prefix = "/sfn")]
pub async fn group_favorites() -> Result<usize, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let result = replay_control_core::favorites::group_by_system(&state.storage())
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    state.cache.invalidate_favorites();
    Ok(result)
}

#[server(prefix = "/sfn")]
pub async fn flatten_favorites() -> Result<usize, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let result = replay_control_core::favorites::flatten_favorites(&state.storage())
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    state.cache.invalidate_favorites();
    Ok(result)
}

/// Get personalized recommendation sections for the favorites page.
///
/// Returns up to 2 sections:
/// - "Because You Played [Game]": games similar to a random recent game by genre/developer
/// - "More from [Series]": series siblings of favorited games that aren't yet favorited
#[server(prefix = "/sfn")]
pub async fn get_favorites_recommendations() -> Result<FavoritesRecommendations, ServerFnError> {
    use super::{GameSection, RecommendedGame};

    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();
    let systems = state.cache.cached_systems(&storage).await;

    let region_pref = state.region_preference();
    let region_secondary = state.region_preference_secondary();
    let region_str = region_pref.as_str().to_string();
    let region_secondary_str = region_secondary
        .map(|r| r.as_str())
        .unwrap_or("")
        .to_string();

    // Collect recents and favorites from cache.
    let recents = state
        .cache
        .get_recents(&storage)
        .unwrap_or_default();
    let all_favorites = state
        .cache
        .get_all_favorited_systems(&storage)
        .unwrap_or_default();

    // Build a set of all favorite (system, rom_filename) for exclusion.
    let fav_keys: std::collections::HashSet<(String, String)> = all_favorites
        .iter()
        .flat_map(|(system, filenames)| {
            filenames
                .iter()
                .map(move |f| (system.clone(), f.clone()))
        })
        .collect();

    // Pick a seed game for "Because You Love..." section.
    // Prefer a favorite (user explicitly likes it, better recommendations).
    // Fall back to a recent game if no favorites.
    let seed_game: Option<(String, String)> = {
        use rand::Rng;
        let mut rng = rand::rng();
        if !fav_keys.is_empty() {
            let fav_list: Vec<_> = fav_keys.iter().collect();
            let idx = rng.random_range(0..fav_list.len());
            Some(fav_list[idx].clone())
        } else if !recents.is_empty() {
            let idx = rng.random_range(0..recents.len());
            Some((recents[idx].game.system.clone(), recents[idx].game.rom_filename.clone()))
        } else {
            None
        }
    };

    // Pick a random favorite for series lookup.
    let fav_keys_vec: Vec<(String, String)> = fav_keys.iter().cloned().collect();

    // DB closure: run all queries under one connection.
    let db_result = state
        .cache
        .db_read(move |conn| {
            let mut sections: Vec<(String, Vec<replay_control_core::metadata_db::GameEntry>, Option<String>)> = Vec::new();

            // --- "Because You Love [Game]" ---
            if let Some(ref seed) = seed_game {
                // Look up the seed game's metadata from game_library.
                let seed_keys = vec![seed.clone()];
                let seed_entries =
                    MetadataDb::lookup_game_entries(conn, &seed_keys).unwrap_or_default();
                if let Some(seed_entry) = seed_entries.get(seed) {
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
                        let by_genre = MetadataDb::top_rated_filtered(
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
                    if similar.len() < 6 {
                        if let Some(dev) = developer {
                            let by_dev = MetadataDb::top_rated_filtered(
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
                    }

                    if similar.len() >= 3 {
                        let raw_name = seed_entry
                            .display_name
                            .as_deref()
                            .unwrap_or(&seed_entry.rom_filename);
                        let display = replay_control_core::title_utils::strip_tags(raw_name);
                        let title = format!("Because You Love {display}");
                        sections.push((title, similar, None));
                    }
                }
            }

            // --- "More from [Series]" ---
            if !fav_keys_vec.is_empty() {
                // Look up series_key for all favorites.
                let fav_entries =
                    MetadataDb::lookup_game_entries(conn, &fav_keys_vec).unwrap_or_default();

                // Collect distinct series keys from favorites with proper display names.
                // series_key is a normalized key — look up the real series_name from game_series
                // via (system, base_title) join.
                let mut series_map: std::collections::HashMap<String, String> =
                    std::collections::HashMap::new();
                for entry in fav_entries.values() {
                    if !entry.series_key.is_empty() && !series_map.contains_key(&entry.series_key) {
                        let display = conn
                            .query_row(
                                "SELECT series_name FROM game_series WHERE system = ?1 AND base_title = ?2 LIMIT 1",
                                [&entry.system, &entry.base_title],
                                |row| row.get::<_, String>(0),
                            )
                            .ok()
                            .unwrap_or_else(|| title_case(&entry.series_key));
                        series_map.insert(entry.series_key.clone(), display);
                    }
                }

                // Pick a random series with non-favorited siblings.
                let series_keys: Vec<(String, String)> = series_map.into_iter().collect();
                if !series_keys.is_empty() {
                    use rand::Rng;
                    let mut indices: Vec<usize> = (0..series_keys.len()).collect();
                    let mut rng = rand::rng();
                    for i in (1..indices.len()).rev() {
                        let j = rng.random_range(0..=i);
                        indices.swap(i, j);
                    }

                    for &idx in &indices {
                        let (ref skey, ref stitle) = series_keys[idx];
                        let siblings = MetadataDb::series_siblings(
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
                            let title = format!("More from {stitle}");
                            sections.push((title, non_fav, None));
                            break;
                        }
                    }
                }
            }

            sections
        })
        .await;

    let raw_sections = db_result.unwrap_or_default();

    // Convert GameEntry to RecommendedGame and resolve box art.
    let mut result_sections = Vec::new();
    for (title, games, see_all_href) in raw_sections {
        let mut picks: Vec<RecommendedGame> = games
            .iter()
            .take(6)
            .filter_map(|rom| super::to_recommended(&rom.system, rom, &systems))
            .collect();
        if picks.is_empty() {
            continue;
        }
        super::resolve_box_art_for_picks(&state, &mut picks).await;
        result_sections.push(GameSection {
            title,
            games: picks,
            see_all_href,
        });
    }

    Ok(FavoritesRecommendations {
        sections: result_sections,
    })
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
