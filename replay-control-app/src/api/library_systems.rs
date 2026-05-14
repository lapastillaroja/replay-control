use super::db_pools::LibraryReadPool;
use replay_control_core_server::library_db::LibraryDb;

pub(crate) async fn active_library_systems_with_roms(
    db: &LibraryReadPool,
    phase: &'static str,
) -> Vec<String> {
    match db.try_read(LibraryDb::active_systems).await {
        Ok(Ok(systems)) => order_known_systems(systems),
        Ok(Err(e)) => {
            tracing::warn!("{phase}: active systems query failed: {e}");
            Vec::new()
        }
        Err(e) => {
            tracing::debug!("{phase}: pool unavailable ({e}); skipping");
            Vec::new()
        }
    }
}

fn order_known_systems(systems: Vec<String>) -> Vec<String> {
    let mut remaining: std::collections::BTreeSet<String> = systems.into_iter().collect();
    let mut ordered = Vec::with_capacity(remaining.len());

    for system in replay_control_core::systems::visible_systems() {
        if remaining.remove(system.folder_name) {
            ordered.push(system.folder_name.to_string());
        }
    }

    ordered.extend(remaining);
    ordered
}
