//! Single-flight cached snapshot of the `/settings/metadata` page payload.
//!
//! Implements Tier 1 of the pool-design plan
//! (`investigations/2026-04-29-pool-design-findings.md`):
//!   - one in-memory snapshot replaces six per-stat server fns
//!   - invalidation hooks attach to the same write-completion sites that
//!     already invalidate the other caches
//!   - stale-on-`None` keeps the page interactive while a write is in flight
//!     or the pool is briefly unavailable
//!
//! The result is that hot SSR navigation never queues on the DB pool. The
//! pool is touched only at boot, on cache miss, and once after each write
//! batch completes.
//!
//! Each query runs in its own short `pool.read()` closure rather than one
//! bundled closure: bundling was a premature "single pool acquisition"
//! optimisation that became a problem at scale (the whole rebuild ran as
//! a single 80–170 s closure on 141 k-ROM libraries, holding a read slot
//! and tripping the 15 s `INTERACT_TIMEOUT` tripwire). Splitting lets the
//! pool cycle: SSR readers slot in between queries, no closure exceeds
//! the cap, and total wallclock is ~the same on cold cache (or faster
//! under contention because requests don't wait for the slowest one to
//! release the connection).
//!
//! See `investigations/2026-04-29-ssr-cache-snapshot-vs-pool-starvation.md`
//! for the design rationale.

use replay_control_core::library_db::{DriverStatusCounts, SystemCoverage};
use replay_control_core_server::library_db::{DataSourceStats, LibraryDb, SystemCoverageStats};

use crate::api::AppState;
pub use crate::server_fns::MetadataPageSnapshot;
use crate::server_fns::{BuiltinDbStats, DataSourceSummary};

/// Build the snapshot. Returns `None` only when the DB pool was unavailable
/// for the duration of the call — caller should keep the previous (stale)
/// snapshot rather than caching `None`.
pub(super) async fn compute(state: &AppState) -> Option<MetadataPageSnapshot> {
    let storage = state.storage();
    let db_path = state.library_pool.db_path();

    // 8 independent reads. Each closure does one query and returns
    // immediately, releasing the pool slot for SSR / other background
    // work to slot in. `unwrap_or_default()` keeps the snapshot
    // best-effort: a single transient pool failure degrades that one
    // section instead of failing the whole rebuild.
    let pool = &state.library_pool;
    let stats = pool
        .read(move |c| LibraryDb::stats(c, &db_path).unwrap_or_default())
        .await?;
    let library_summary = pool
        .read(|c| LibraryDb::library_summary(c).unwrap_or_default())
        .await?;
    let entries_per_system = pool
        .read(|c| LibraryDb::entries_per_system(c).unwrap_or_default())
        .await?;
    let thumbnails_per_system = pool
        .read(|c| LibraryDb::thumbnails_per_system(c).unwrap_or_default())
        .await?;
    let coverage_stats = pool
        .read(|c| LibraryDb::system_coverage_stats(c).unwrap_or_default())
        .await?;
    let driver_status = pool
        .read(|c| LibraryDb::driver_status_per_system(c).unwrap_or_default())
        .await?;
    let data_source_stats = pool
        .read(|c| LibraryDb::get_data_source_stats(c, "libretro-thumbnails").ok())
        .await?;
    let image_count_pair = pool
        .read(|c| LibraryDb::image_stats(c).unwrap_or((0, 0)))
        .await?;

    // Off-pool work: the L1 systems cache, the on-disk media size, and the
    // bundled-catalog read-only stats.
    let systems = state
        .cache
        .cached_systems(&storage, &state.library_pool)
        .await;
    let media_size = replay_control_core_server::thumbnails::media_dir_size(&storage.root);
    let image_stats = (image_count_pair.0, image_count_pair.1, media_size);

    let coverage = build_coverage(
        systems,
        entries_per_system,
        thumbnails_per_system,
        coverage_stats,
        driver_status,
    );

    let data_source = build_data_source_summary(data_source_stats);
    let builtin_stats = build_builtin_stats().await;

    Some(MetadataPageSnapshot {
        stats,
        library_summary,
        coverage,
        data_source,
        image_stats,
        builtin_stats,
        storage_kind: format!("{:?}", storage.kind).to_lowercase(),
        storage_root: storage.root.display().to_string(),
    })
}

fn build_coverage(
    systems: Vec<replay_control_core_server::roms::SystemSummary>,
    entries_per_system: Vec<(String, usize)>,
    thumbnails_per_system: Vec<(String, usize)>,
    coverage_stats: Vec<SystemCoverageStats>,
    driver_status: std::collections::HashMap<String, DriverStatusCounts>,
) -> Vec<SystemCoverage> {
    let mut meta_map: std::collections::HashMap<String, usize> =
        entries_per_system.into_iter().collect();
    let mut thumb_map: std::collections::HashMap<String, usize> =
        thumbnails_per_system.into_iter().collect();
    let mut stats_map: std::collections::HashMap<String, SystemCoverageStats> = coverage_stats
        .into_iter()
        .map(|s| (s.system.clone(), s))
        .collect();
    let mut driver_map = driver_status;

    let mut coverage: Vec<SystemCoverage> = systems
        .into_iter()
        .filter(|s| s.game_count > 0)
        .map(|s| {
            let with_metadata = meta_map.remove(&s.folder_name).unwrap_or(0);
            let with_thumbnail = thumb_map.remove(&s.folder_name).unwrap_or(0);
            let stats = stats_map.remove(&s.folder_name).unwrap_or_default();
            let driver_status = driver_map.remove(&s.folder_name);
            SystemCoverage {
                system: s.folder_name,
                display_name: s.display_name,
                total_games: s.game_count,
                with_thumbnail: with_thumbnail.min(s.game_count),
                with_genre: stats.with_genre,
                with_developer: stats.with_developer,
                with_rating: stats.with_rating,
                size_bytes: stats.size_bytes,
                with_description: with_metadata.min(s.game_count),
                clone_count: stats.clone_count,
                hack_count: stats.hack_count,
                translation_count: stats.translation_count,
                special_count: stats.special_count,
                coop_count: stats.coop_count,
                verified_count: stats.verified_count,
                min_year: stats.min_year,
                max_year: stats.max_year,
                driver_status,
            }
        })
        .collect();
    coverage.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    coverage
}

fn build_data_source_summary(stats: Option<DataSourceStats>) -> DataSourceSummary {
    let Some(stats) = stats else {
        return DataSourceSummary {
            entry_count: 0,
            repo_count: 0,
            oldest_imported_at: None,
            last_updated_text: String::new(),
        };
    };
    let last_updated_text = stats
        .oldest_imported_at
        .map(|ts| {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            let diff = now - ts;
            if diff < 60 {
                "just now".to_string()
            } else if diff < 3600 {
                format!("{}m ago", diff / 60)
            } else if diff < 86400 {
                format!("{}h ago", diff / 3600)
            } else {
                format!("{}d ago", diff / 86400)
            }
        })
        .unwrap_or_default();
    DataSourceSummary {
        entry_count: stats.total_entries,
        repo_count: stats.repo_count,
        oldest_imported_at: stats.oldest_imported_at,
        last_updated_text,
    }
}

async fn build_builtin_stats() -> BuiltinDbStats {
    use replay_control_core_server::{arcade_db, game_db, series_db};
    BuiltinDbStats {
        arcade_entries: arcade_db::entry_count().await,
        arcade_mame_version: arcade_db::MAME_VERSION.to_string(),
        game_rom_entries: game_db::total_rom_entries().await,
        game_system_count: game_db::system_count().await,
        wikidata_series_entries: series_db::entry_count().await,
        wikidata_series_count: series_db::all_series_names().await.len(),
    }
}
