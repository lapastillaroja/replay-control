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
