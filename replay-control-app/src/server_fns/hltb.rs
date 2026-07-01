use super::*;
#[cfg(feature = "ssr")]
use replay_control_core_server::user_data_db::UserDataDb;

pub use replay_control_core::hltb::HltbData;

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
        .user_data_reader
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
        .user_data_writer
        .try_write(move |conn| {
            if let Some(ref data) = fetched_clone {
                UserDataDb::set_hltb_cache(conn, &base_title_clone, data)
            } else {
                UserDataDb::set_hltb_cache_empty(conn, &base_title_clone)
            }
        })
        .await;

    Ok(fetched)
}
