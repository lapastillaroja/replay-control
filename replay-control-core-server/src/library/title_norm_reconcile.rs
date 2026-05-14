//! Boot-time reconcile for stored normalized titles in `library.db`.
//!
//! `game_library.normalized_title{,_alt}` is populated at scan time using
//! `replay_control_core::title_utils::normalize_title_for_metadata`. When
//! that function changes in a future release, the stored values become
//! stale relative to fresh-install output and the enrichment matcher
//! silently degrades.
//!
//! `library_meta.title_norm_version` carries the version under which the
//! normalized columns were last computed. On boot, mismatch against the
//! current `TITLE_NORM_VERSION` triggers a per-system rebuild and stamp.
//!
//! The host-global `external_metadata.db` side is reconciled by
//! `phase_auto_import` — it already gates the LaunchBox XML re-parse on a
//! stamp, so it just reads `TITLE_NORM_VERSION` alongside the CRC32 and
//! re-imports if either differs. One XML parse per relevant boot, one
//! place to look.

use std::collections::HashMap;

use replay_control_core::error::Result;
use replay_control_core::title_utils::{TITLE_NORM_VERSION, normalize_title_for_metadata};
use replay_control_core::{systems, title_utils};

use crate::arcade_db;
use crate::db_pool::DbPool;
use crate::library_db::{LibraryDb, library_meta};

/// Outcome of a reconcile pass.
#[derive(Debug, Default, Clone)]
pub struct ReconcileStats {
    /// Per-system update counts in the order they were processed.
    pub per_system: Vec<(String, usize)>,
    pub total_updates: usize,
}

/// Check the per-storage `title_norm_version` stamp and rebuild on mismatch.
///
/// No-op when the stamp matches `TITLE_NORM_VERSION`. On mismatch, walks
/// every system in `game_library`, recomputes both normalized columns,
/// bulk-updates, and stamps the new version on success. The stamp write
/// is the LAST step so a partial failure leaves the stamp behind and the
/// next boot retries.
pub async fn reconcile_library_normalized_titles(pool: &DbPool) -> Result<ReconcileStats> {
    let stored = pool
        .read(|conn| library_meta::read_meta(conn, library_meta::keys::TITLE_NORM_VERSION))
        .await
        .flatten()
        .and_then(|s| s.parse::<u32>().ok());

    if stored == Some(TITLE_NORM_VERSION) {
        return Ok(ReconcileStats::default());
    }

    let systems = pool
        .read(|conn| LibraryDb::active_systems(conn).unwrap_or_default())
        .await
        .unwrap_or_default();

    let mut stats = ReconcileStats::default();
    for system in &systems {
        let count = rebuild_system(pool, system).await?;
        if count > 0 {
            stats.total_updates += count;
            stats.per_system.push((system.clone(), count));
        }
    }

    match pool
        .try_write(|conn| {
            library_meta::write_meta(
                conn,
                library_meta::keys::TITLE_NORM_VERSION,
                Some(&TITLE_NORM_VERSION.to_string()),
            )
        })
        .await
    {
        Ok(Ok(())) => {}
        Ok(Err(e)) => tracing::warn!("title_norm reconcile: stamp SQL failed: {e}"),
        Err(e) => tracing::warn!("title_norm reconcile: stamp write failed: {e}"),
    }

    tracing::info!(
        "title_norm reconcile: bumped {:?} → v{TITLE_NORM_VERSION}, {} systems / {} rows",
        stored,
        stats.per_system.len(),
        stats.total_updates,
    );
    Ok(stats)
}

/// Rebuild `normalized_title{,_alt}` for one system. Mirrors the scan-time
/// logic in `library/game_entry_builder.rs::populate_normalized_titles`:
/// console = filename stem; arcade = display_name with clone-parent fallback.
async fn rebuild_system(pool: &DbPool, system: &str) -> Result<usize> {
    let sys = system.to_string();
    let rom_filenames: Vec<String> = pool
        .read(move |conn| LibraryDb::visible_filenames(conn, &sys).unwrap_or_default())
        .await
        .unwrap_or_default();
    if rom_filenames.is_empty() {
        return Ok(0);
    }

    let is_arcade = systems::is_arcade_system(system);
    let arcade_lookup = if is_arcade {
        let stems: Vec<String> = rom_filenames
            .iter()
            .map(|f| title_utils::filename_stem(f).to_string())
            .collect();
        let stem_refs: Vec<&str> = stems.iter().map(|s| s.as_str()).collect();
        arcade_db::lookup_arcade_games_batch(system, &stem_refs).await
    } else {
        HashMap::new()
    };

    let updates: Vec<(String, String, String)> = rom_filenames
        .iter()
        .map(|filename| {
            let (norm, norm_alt) = recompute(filename, is_arcade, &arcade_lookup);
            (filename.clone(), norm, norm_alt)
        })
        .collect();

    let sys = system.to_string();
    let count = pool
        .try_write(move |conn| {
            LibraryDb::update_normalized_titles(conn, &sys, &updates).unwrap_or(0)
        })
        .await
        .unwrap_or(0);
    Ok(count)
}

fn recompute(
    rom_filename: &str,
    is_arcade: bool,
    arcade_lookup: &HashMap<String, arcade_db::ArcadeGameInfo>,
) -> (String, String) {
    let stem = title_utils::filename_stem(rom_filename);
    if is_arcade && let Some(info) = arcade_lookup.get(stem) {
        let primary = normalize_title_for_metadata(&info.display_name);
        let alt = if info.is_clone && !info.parent.is_empty() {
            arcade_lookup
                .get(&info.parent)
                .map(|p| normalize_title_for_metadata(&p.display_name))
                .filter(|p| p != &primary)
                .unwrap_or_default()
        } else {
            String::new()
        };
        return (primary, alt);
    }
    (normalize_title_for_metadata(stem), String::new())
}
