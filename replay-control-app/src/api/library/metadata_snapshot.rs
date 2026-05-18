//! DB-backed `/settings/metadata` page payload builder.

use replay_control_core_server::external_metadata::{self, DataSourceStats};
use replay_control_core_server::library_db::LibraryDb;

use crate::api::AppState;
pub use crate::server_fns::MetadataPageSnapshot;
use crate::server_fns::{BuiltinDbStats, DataSourceSummary};

/// Build the slower data-source payload for `/settings/metadata`. The top
/// library overview is loaded separately from `game_library_system_stats` so
/// it does not wait for filesystem media sizing or bundled catalog stats.
pub(crate) async fn compute(state: &AppState) -> MetadataPageSnapshot {
    let em_db_path = state.external_metadata_reader.db_path();

    // Independent reads across two pools. The pools serialize them on their
    // slot counts; fanning them out lets the slowest queries overlap with the
    // others instead of running back-to-back. Defaults keep the page best-effort
    // if a pool is briefly unavailable.
    let lib_pool = &state.library_reader;
    let em_pool = &state.external_metadata_reader;
    let (stats, data_source_stats, image_stats) = tokio::join!(
        em_pool
            .read(move |c| external_metadata::launchbox_stats(c, &em_db_path).unwrap_or_default()),
        em_pool.read(|c| external_metadata::get_data_source_stats(c, "libretro-thumbnails").ok()),
        lib_pool
            .read(|c| LibraryDb::thumbnail_media_totals_from_system_stats(c).unwrap_or_default()),
    );
    let stats = stats.unwrap_or_default();
    let data_source_stats = data_source_stats.unwrap_or_default();
    let image_stats = image_stats.unwrap_or_default();

    let data_source = build_data_source_summary(data_source_stats);
    let builtin_stats = build_builtin_stats().await;

    MetadataPageSnapshot {
        stats,
        data_source,
        image_stats,
        builtin_stats,
    }
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
