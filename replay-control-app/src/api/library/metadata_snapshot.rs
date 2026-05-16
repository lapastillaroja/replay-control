//! Single-flight cached snapshot of the `/settings/metadata` page payload.
//!
//! Each query runs in its own short `pool.read()` closure (fanned out
//! with `tokio::join!`) so SSR readers can slot in between them and no
//! single closure exceeds the pool's `INTERACT_TIMEOUT` cap. See
//! `investigations/2026-04-29-ssr-cache-snapshot-vs-pool-starvation.md`
//! for the design rationale.

use replay_control_core_server::external_metadata::{self, DataSourceStats};
use replay_control_core_server::library_db::{self, LibraryDb};

use crate::api::AppState;
pub use crate::server_fns::MetadataPageSnapshot;
use crate::server_fns::{BuiltinDbStats, DataSourceSummary};

/// Build the snapshot. Returns `None` only when the DB pool was unavailable
/// for the duration of the call — caller should keep the previous (stale)
/// snapshot rather than caching `None`.
pub(super) async fn compute(state: &AppState) -> Option<MetadataPageSnapshot> {
    let storage = state.storage();
    let em_db_path = state.external_metadata_reader.db_path();

    // 9 independent reads across two pools. The pools serialize them on their
    // slot counts; fanning them out lets the slowest queries overlap with the
    // others instead of running back-to-back. `unwrap_or_default()` keeps the
    // snapshot best-effort: a single transient pool failure degrades that one
    // section instead of failing the whole rebuild.
    let lib_pool = &state.library_reader;
    let em_pool = &state.external_metadata_reader;
    let (
        stats,
        library_summary,
        system_meta,
        entries_per_system,
        thumbnails_per_system,
        coverage_stats,
        driver_status,
        data_source_stats,
        image_count_pair,
    ) = tokio::join!(
        em_pool
            .read(move |c| external_metadata::launchbox_stats(c, &em_db_path).unwrap_or_default()),
        lib_pool.read(|c| LibraryDb::library_summary(c).unwrap_or_default()),
        lib_pool.read(|c| LibraryDb::load_all_system_meta(c).unwrap_or_default()),
        em_pool.read(|c| external_metadata::launchbox_entries_per_system(c).unwrap_or_default()),
        lib_pool.read(|c| LibraryDb::thumbnails_per_system(c).unwrap_or_default()),
        lib_pool.read(|c| LibraryDb::system_coverage_stats(c).unwrap_or_default()),
        lib_pool.read(|c| LibraryDb::driver_status_per_system(c).unwrap_or_default()),
        em_pool.read(|c| external_metadata::get_data_source_stats(c, "libretro-thumbnails").ok()),
        lib_pool.read(|c| LibraryDb::image_stats(c).unwrap_or((0, 0))),
    );
    let stats = stats?;
    let library_summary = library_summary?;
    let system_meta = system_meta?;
    let entries_per_system = entries_per_system?;
    let thumbnails_per_system = thumbnails_per_system?;
    let coverage_stats = coverage_stats?;
    let driver_status = driver_status?;
    let data_source_stats = data_source_stats?;
    let image_count_pair: (usize, usize) = image_count_pair?;

    // Off-pool work: on-disk media size and bundled-catalog read-only stats.
    let storage_root = storage.root.clone();
    let media_size = tokio::task::spawn_blocking(move || {
        replay_control_core_server::thumbnails::media_dir_size(&storage_root)
    })
    .await
    .unwrap_or(0);
    let image_stats = (image_count_pair.0, image_count_pair.1, media_size);

    let coverage = library_db::build_system_coverage(
        system_meta,
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
    use replay_control_core_server::{arcade_db, catalog_pool, game_db, series_db};
    let (
        arcade_entries,
        game_rom_entries,
        game_system_count,
        wikidata_series_entries,
        wikidata_series_count,
        catalog_resources,
    ) = tokio::join!(
        arcade_db::entry_count(),
        game_db::total_rom_entries(),
        game_db::system_count(),
        series_db::entry_count(),
        async { series_db::all_series_names().await.len() },
        catalog_pool::catalog_resource_stats(),
    );
    BuiltinDbStats {
        arcade_entries,
        arcade_mame_version: arcade_db::MAME_VERSION.to_string(),
        game_rom_entries,
        game_system_count,
        wikidata_series_entries,
        wikidata_series_count,
        manual_resource_entries: catalog_resources.manual_resources,
        mister_manual_resource_entries: catalog_resources.mister_manual_resources,
        retrokit_manual_resource_entries: catalog_resources.retrokit_manual_resources,
    }
}
