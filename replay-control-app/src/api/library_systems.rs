use super::db_pools::LibraryReadPool;
use replay_control_core_server::library_db::{LibraryDb, SystemMeta};
use replay_control_core_server::roms::SystemSummary;

pub(crate) async fn system_summaries(db: &LibraryReadPool) -> Vec<SystemSummary> {
    match db.try_read(LibraryDb::load_all_system_meta).await {
        Ok(Ok(meta)) => derive_system_summaries(&meta),
        Ok(Err(e)) => {
            // TODO: return a typed error instead of making DB failures look like an empty list.
            tracing::warn!("system_summaries: system meta query failed: {e}");
            Vec::new()
        }
        Err(e) => {
            tracing::debug!("system_summaries: pool unavailable ({e}); returning empty summaries");
            Vec::new()
        }
    }
}

fn derive_system_summaries(meta: &[SystemMeta]) -> Vec<SystemSummary> {
    let meta_by_system: std::collections::HashMap<&str, &SystemMeta> =
        meta.iter().map(|row| (row.system.as_str(), row)).collect();

    let mut summaries: Vec<SystemSummary> = replay_control_core::systems::visible_systems()
        .map(|system| {
            let (game_count, total_size_bytes) = meta_by_system
                .get(system.folder_name)
                .map(|meta| (meta.rom_count, meta.total_size_bytes))
                .unwrap_or((0, 0));

            SystemSummary {
                folder_name: system.folder_name.to_string(),
                display_name: system.display_name.to_string(),
                manufacturer: system.manufacturer.to_string(),
                category: format!("{:?}", system.category).to_lowercase(),
                game_count,
                total_size_bytes,
            }
        })
        .collect();

    summaries.sort_by(|a, b| {
        let a_has = a.game_count > 0;
        let b_has = b.game_count > 0;
        b_has.cmp(&a_has).then(a.display_name.cmp(&b.display_name))
    });

    summaries
}

/// DB-backed systems with ROM rows. Unlike static `visible_systems()`, this
/// is for maintenance over the existing library, not discovery/rebuild scans.
pub(crate) async fn active_systems(db: &LibraryReadPool, phase: &'static str) -> Vec<String> {
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
