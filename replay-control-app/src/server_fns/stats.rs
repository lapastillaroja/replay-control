use super::*;

#[cfg(feature = "ssr")]
use replay_control_core_server::library_db::stats::compute_dashboard;

/// Fetch the complete stats dashboard for the library.
#[server(prefix = "/sfn")]
pub async fn get_stats_dashboard() -> Result<replay_control_core::stats::StatsDashboard, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();

    let dashboard_result = state
        .library_pool
        .read(compute_dashboard)
        .await;

    let mut dashboard = match dashboard_result {
        Some(Ok(d)) => d,
        Some(Err(e)) => return Err(ServerFnError::new(format!("Stats error: {e}"))),
        None => return Err(ServerFnError::new("Library pool unavailable")),
    };

    let mut systems = std::mem::take(&mut dashboard.systems);
    let mut total_favorites = 0usize;

    for sys_stat in &mut systems {
        let system = sys_stat.system.clone();
        let favs = state
            .cache
            .get_favorites_set(&storage, &system)
            .await;
        sys_stat.favorite_count = favs.len();
        total_favorites += favs.len();
    }

    dashboard.systems = systems;
    dashboard.summary.total_favorites = total_favorites;

    Ok(dashboard)
}
