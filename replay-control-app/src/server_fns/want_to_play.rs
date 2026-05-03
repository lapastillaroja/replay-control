use super::*;
#[cfg(feature = "ssr")]
use replay_control_core_server::library_db::LibraryDb;
#[cfg(feature = "ssr")]
use replay_control_core_server::user_data_db::UserDataDb;

pub use replay_control_core::want_to_play::{HltbData, WantToPlayEntry};

/// A backlog entry enriched with box art and HLTB data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacklogEntry {
    #[serde(flatten)]
    pub entry: WantToPlayEntry,
    pub display_name: Option<String>,
    pub box_art_url: Option<String>,
    pub hltb: Option<HltbData>,
}

/// List all backlog games, enriched with box art and cached HLTB data.
#[server(prefix = "/sfn", endpoint = "/get_backlog")]
pub async fn get_backlog() -> Result<Vec<BacklogEntry>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();

    let entries = state
        .user_data_pool
        .read(|conn| UserDataDb::list_want_to_play(conn))
        .await
        .ok_or_else(|| ServerFnError::new("Cannot open user data DB"))?
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    if entries.is_empty() {
        return Ok(Vec::new());
    }

    let keys: Vec<(String, String)> = entries
        .iter()
        .map(|e| (e.system.clone(), e.rom_filename.clone()))
        .collect();

    let art_names: Vec<(Option<String>, Option<String>)> = state
        .library_pool
        .read({
            let keys = keys.clone();
            move |conn| {
                let map = LibraryDb::lookup_game_entries(conn, &keys).unwrap_or_default();
                keys.iter()
                    .map(|k| {
                        let e = map.get(k);
                        let art = e.and_then(|g| g.box_art_url.clone());
                        let name = e
                            .and_then(|g| g.display_name.clone())
                            .filter(|n| !n.is_empty());
                        (art, name)
                    })
                    .collect()
            }
        })
        .await
        .unwrap_or_else(|| vec![(None, None); entries.len()]);

    let base_titles: Vec<String> = entries.iter().map(|e| e.base_title.clone()).collect();
    let hltb_results: Vec<Option<HltbData>> = state
        .user_data_pool
        .read(move |conn| {
            base_titles
                .iter()
                .map(|bt| UserDataDb::get_hltb_cache(conn, bt).ok().flatten())
                .collect()
        })
        .await
        .unwrap_or_else(|| vec![None; entries.len()]);

    let result = entries
        .into_iter()
        .zip(art_names)
        .zip(hltb_results)
        .map(|((entry, (box_art_url, display_name)), hltb)| BacklogEntry {
            entry,
            display_name,
            box_art_url,
            hltb,
        })
        .collect();

    Ok(result)
}

/// Add a game to the backlog. Returns true if newly added.
#[server(prefix = "/sfn")]
pub async fn add_to_backlog(
    system: String,
    rom_filename: String,
    base_title: String,
) -> Result<bool, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    state
        .user_data_pool
        .write(move |conn| UserDataDb::add_want_to_play(conn, &system, &rom_filename, &base_title))
        .await
        .ok_or_else(|| ServerFnError::new("Cannot open user data DB"))?
        .map_err(|e| ServerFnError::new(e.to_string()))
}

/// Remove a game from the backlog.
#[server(prefix = "/sfn")]
pub async fn remove_from_backlog(
    system: String,
    rom_filename: String,
) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    state
        .user_data_pool
        .write(move |conn| UserDataDb::remove_want_to_play(conn, &system, &rom_filename))
        .await
        .ok_or_else(|| ServerFnError::new("Cannot open user data DB"))?
        .map_err(|e| ServerFnError::new(e.to_string()))
}

/// Check whether a specific ROM is in the backlog.
#[server(prefix = "/sfn")]
pub async fn is_in_backlog(
    system: String,
    rom_filename: String,
) -> Result<bool, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    state
        .user_data_pool
        .read(move |conn| UserDataDb::is_want_to_play(conn, &system, &rom_filename))
        .await
        .ok_or_else(|| ServerFnError::new("Cannot open user data DB"))?
        .map_err(|e| ServerFnError::new(e.to_string()))
}

/// Fetch HLTB completion data for a game, using the 7-day DB cache.
///
/// Returns `None` if HLTB has no data for the title or is unreachable —
/// callers should treat this as "data unavailable" rather than an error.
#[server(prefix = "/sfn")]
pub async fn get_hltb_data(
    base_title: String,
    display_name: String,
) -> Result<Option<HltbData>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();

    // Check the cache first.
    let cached = state
        .user_data_pool
        .read({
            let base_title = base_title.clone();
            move |conn| UserDataDb::get_hltb_cache(conn, &base_title)
        })
        .await
        .ok_or_else(|| ServerFnError::new("Cannot open user data DB"))?
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    if let Some(data) = cached {
        // game_id == 0 means a cached "no result" sentinel.
        return Ok((data.game_id > 0).then_some(data));
    }

    // Cache miss — fetch from HLTB.
    let fetched = replay_control_core_server::hltb::fetch_hltb(&display_name).await;

    // Persist result (or negative sentinel) to avoid re-fetching.
    let base_title_clone = base_title.clone();
    let fetched_clone = fetched.clone();
    let _ = state
        .user_data_pool
        .write(move |conn| {
            if let Some(ref data) = fetched_clone {
                UserDataDb::set_hltb_cache(conn, &base_title_clone, data)
            } else {
                UserDataDb::set_hltb_cache_empty(conn, &base_title_clone)
            }
        })
        .await;

    Ok(fetched)
}
