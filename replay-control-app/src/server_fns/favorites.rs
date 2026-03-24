use super::*;

/// Build a `FavoriteWithArt` from a favorite and its resolved box art URL.
#[cfg(feature = "ssr")]
async fn enrich_favorite(fav: Favorite, box_art_url: Option<String>) -> FavoriteWithArt {
    let genre_str = super::search::lookup_genre(&fav.game.system, &fav.game.rom_filename).await;
    let genre = if genre_str.is_empty() {
        None
    } else {
        Some(genre_str)
    };
    FavoriteWithArt {
        fav,
        box_art_url,
        genre,
    }
}

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
    // Pre-load image indexes for each distinct system (get_image_index is async).
    let distinct_systems: std::collections::HashSet<String> =
        favs.iter().map(|f| f.game.system.clone()).collect();
    let mut image_indexes: std::collections::HashMap<
        String,
        std::sync::Arc<crate::api::cache::ImageIndex>,
    > = std::collections::HashMap::new();
    for sys in &distinct_systems {
        let index = state.cache.get_image_index(&state, sys).await;
        image_indexes.insert(sys.clone(), index);
    }
    let mut results = Vec::with_capacity(favs.len());
    for fav in favs {
        let index = &image_indexes[&fav.game.system];
        let box_art_url =
            state
                .cache
                .resolve_box_art(&state, index, &fav.game.system, &fav.game.rom_filename);
        results.push(enrich_favorite(fav, box_art_url).await);
    }
    Ok(results)
}

#[server(prefix = "/sfn")]
pub async fn get_system_favorites(system: String) -> Result<Vec<FavoriteWithArt>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let favs = replay_control_core::favorites::list_favorites_for_system(&state.storage(), &system)
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    let image_index = state.cache.get_image_index(&state, &system).await;
    let mut results = Vec::with_capacity(favs.len());
    for fav in favs {
        let box_art_url = state.cache.resolve_box_art(
            &state,
            &image_index,
            &fav.game.system,
            &fav.game.rom_filename,
        );
        results.push(enrich_favorite(fav, box_art_url).await);
    }
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
    replay_control_core::favorites::remove_favorite(
        &state.storage(),
        &filename,
        subfolder.as_deref(),
    )
    .map_err(|e| ServerFnError::new(e.to_string()))?;
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
