use std::collections::HashMap;

use replay_control_core_server::enrichment::{self, ArcadeInfoLookup, ImageIndex};
use replay_control_core_server::external_metadata::{self, LaunchboxRow, ThumbnailManifestEntry};
use replay_control_core_server::library_db::LibraryDb;
use replay_control_core_server::thumbnail_manifest;
use replay_control_core_server::thumbnails::ThumbnailKind;
use replay_control_core_server::user_data_db::UserDataDb;

use super::{LibraryService, ScanCancellation};

impl LibraryService {
    /// Enrich box_art_url (and rating) for all entries in a system's game library.
    /// Reads the host-global `external_metadata.db` for LaunchBox metadata and
    /// libretro thumbnail manifests, and the per-storage `library.db` for
    /// filesystem state. Called after L2 write-through to populate fields
    /// that `list_roms()` doesn't set.
    pub async fn enrich_system_cache(&self, state: &crate::api::AppState, system: String) {
        if let Err(e) = self
            .enrich_system_cache_with_cancellation(state, system, None)
            .await
        {
            tracing::warn!("L2 enrichment cancelled or failed: {e}");
        }
    }

    pub(crate) async fn enrich_system_cache_with_cancellation(
        &self,
        state: &crate::api::AppState,
        system: String,
        cancellation: Option<&ScanCancellation>,
    ) -> replay_control_core::error::Result<()> {
        if let Some(cancellation) = cancellation {
            cancellation.ensure_current()?;
        }

        // Fetch the visible-filename list once; image-index, launchbox load,
        // and arcade lookup all consume it. Previously each path read it
        // independently (an N+1 against the same query).
        // The network/CPU work below makes it impossible to hold the
        // writer connection open for the whole pass; this read goes
        // through the dedicated reader pool. Stale-row risk is mitigated
        // by INSERT OR REPLACE on the write side and by FK CASCADE on
        // ROM deletion.
        let sys = system.clone();
        let rom_filenames: Vec<String> = state
            .library_reader
            .read(move |conn| LibraryDb::visible_filenames(conn, &sys).unwrap_or_default())
            .await
            .unwrap_or_default();

        if rom_filenames.is_empty() {
            let sys = system.clone();
            if let Some(cancellation) = cancellation {
                cancellation.ensure_current()?;
            }
            let _ = state
                .library_writer
                .write(move |conn| {
                    let _ = LibraryDb::replace_descriptions_for_system(conn, &sys, &[]);
                })
                .await;
            return Ok(());
        }

        // Independent setup steps that all consume `system` / `rom_filenames`.
        // Two pools (library, external_metadata) plus the arcade lookup;
        // `join!` lets the slowest overlap with the others.
        let (index, launchbox_rows, alt_to_primary, arcade_lookup) = tokio::join!(
            build_image_index(state, &system),
            load_launchbox_rows(&state.external_metadata_reader, &system),
            load_launchbox_alt_to_primary(&state.external_metadata_reader, &system),
            ArcadeInfoLookup::build(&system, &rom_filenames),
        );

        let sys = system.clone();
        // Heavy enrichment pass (per-row matching, manifest cross-ref,
        // image index lookups). Routed through the reader pool so the
        // single writer slot stays free for downloads and other writes;
        // the write closure below batches the resulting per-row updates.
        let result = state
            .library_reader
            .read(move |conn| {
                replay_control_core_server::enrichment::enrich_system(
                    conn,
                    &sys,
                    &index,
                    &arcade_lookup,
                    &launchbox_rows,
                    &alt_to_primary,
                )
            })
            .await;

        let Some(result) = result else {
            return Ok(());
        };

        if let Some(cancellation) = cancellation {
            cancellation.ensure_current()?;
        }

        // Queue on-demand manifest downloads. Each await throttles to
        // the orchestrator's visible queue capacity so a large fan-out
        // (e.g. 4k+ thumbnails after a fresh rescan) backpressures
        // instead of dropping work.
        for (rom_filename, manifest_match) in &result.manifest_downloads {
            queue_on_demand_download(state, &system, rom_filename, manifest_match).await;
        }

        // Bundle every per-system write into a single `db.write` so a Pi's
        // synchronous-NORMAL fsync-per-commit only happens once per
        // enrichment pass instead of six times. Each step inside the
        // closure logs its own failure and continues — partial enrichment
        // is better than no enrichment.
        let region_pref = state.region_preference();
        let region_secondary = state.region_preference_secondary();
        let sys = system.clone();
        let dev_count = result.developer_updates.len();
        let coop_count = result.cooperative_updates.len();
        let date_count = result.release_date_rows.len();
        let desc_count = result.description_rows.len();
        let enrich_count = result.enrichments.len();
        let developer_updates = result.developer_updates;
        let cooperative_updates = result.cooperative_updates;
        let release_date_rows = result.release_date_rows;
        let description_rows = result.description_rows;
        let enrichments = result.enrichments;
        if let Some(cancellation) = cancellation {
            cancellation.ensure_current()?;
        }
        state
            .library_writer
            .write(move |conn| {
                if !developer_updates.is_empty()
                    && let Err(e) = LibraryDb::update_developers(conn, &sys, &developer_updates)
                {
                    tracing::warn!("Developer enrichment failed for {sys}: {e}");
                }
                if !cooperative_updates.is_empty()
                    && let Err(e) = LibraryDb::update_cooperative(conn, &sys, &cooperative_updates)
                {
                    tracing::warn!("Cooperative enrichment failed for {sys}: {e}");
                }
                // Upsert LaunchBox-sourced rows into game_release_date BEFORE
                // the resolver runs. The resolver rebuilds game_library's
                // mirror columns from game_release_date; any rows we miss
                // here would be cleared to NULL on the same write.
                if !release_date_rows.is_empty()
                    && let Err(e) = LibraryDb::upsert_release_dates(conn, &release_date_rows)
                {
                    tracing::warn!("Release-date upsert failed for {sys}: {e}");
                }
                // Release-date resolver: rewrites game_library mirror columns
                // from `game_release_date` for every ROM.
                let _ = LibraryDb::resolve_release_date_for_library(
                    conn,
                    region_pref,
                    region_secondary,
                );
                // Always rebuilt (truncate + repopulate) so removed ROMs lose
                // their description on the next pass.
                if !description_rows.is_empty()
                    && let Err(e) =
                        LibraryDb::replace_descriptions_for_system(conn, &sys, &description_rows)
                {
                    tracing::warn!("game_description rebuild failed for {sys}: {e}");
                }
                if !enrichments.is_empty()
                    && let Err(e) = LibraryDb::update_box_art_genre_rating(conn, &sys, &enrichments)
                {
                    tracing::warn!("Enrichment failed for {sys}: {e}");
                }
            })
            .await;

        tracing::debug!(
            "L2 enrichment: {system} — {dev_count} dev / {coop_count} coop / {date_count} dates / {desc_count} desc / {enrich_count} box+genre+players+ratings"
        );
        Ok(())
    }
}

/// Load the per-system `launchbox_game` rows from the host-global
/// `external_metadata.db`. Returns an empty map when the pool is unavailable
/// or the read fails (the design treats LaunchBox as optional — users
/// without it still get scan-time + catalog enrichment).
async fn load_launchbox_rows(
    em_reader: &crate::api::db_pools::ExternalMetadataReadPool,
    system: &str,
) -> HashMap<String, LaunchboxRow> {
    let sys = system.to_string();
    em_reader
        .read(move |conn| external_metadata::system_launchbox_rows(conn, &sys).unwrap_or_default())
        .await
        .unwrap_or_default()
}

/// Load the alt-name → primary normalized_title map for a system. Empty
/// `normalized_alternate` rows (legacy data pre-dating Phase 1 import) are
/// dropped so the matcher's lookup is dense.
async fn load_launchbox_alt_to_primary(
    em_reader: &crate::api::db_pools::ExternalMetadataReadPool,
    system: &str,
) -> HashMap<String, String> {
    let sys = system.to_string();
    em_reader
        .read(move |conn| {
            external_metadata::system_launchbox_alternates(conn, &sys)
                .unwrap_or_default()
                .into_iter()
                .filter(|(_, _, na)| !na.is_empty())
                .map(|(prim, _alt_raw, na)| (na, prim))
                .collect::<HashMap<_, _>>()
        })
        .await
        .unwrap_or_default()
}

/// Load the per-system libretro repo manifest data from
/// `external_metadata.db`. Used by `build_image_index` to populate the
/// manifest fuzzy index. Empty list when the pool is unavailable or the
/// system has no libretro repos configured.
async fn load_libretro_repo_data(
    em_reader: &crate::api::db_pools::ExternalMetadataReadPool,
    system: &str,
) -> Vec<(String, String, Vec<ThumbnailManifestEntry>)> {
    let Some(repo_names) = replay_control_core_server::thumbnails::thumbnail_repo_names(system)
    else {
        return Vec::new();
    };
    let display_names: Vec<String> = repo_names.iter().map(|s| (*s).to_string()).collect();
    em_reader
        .read(move |conn| {
            let display_refs: Vec<&str> = display_names.iter().map(String::as_str).collect();
            thumbnail_manifest::load_repo_manifest_data(
                conn,
                &display_refs,
                ThumbnailKind::Boxart.repo_dir(),
            )
        })
        .await
        .unwrap_or_default()
}

// ── Image index + on-demand downloads ──────────────────────────────

/// Build an image index for a system.
///
/// Pre-loads libretro repo manifest data from the host-global
/// `external_metadata.db`, then delegates to the core
/// `enrichment::build_image_index` which does the filesystem walk + library
/// DB read + manifest fuzzy-index construction.
async fn build_image_index(state: &crate::api::AppState, system: &str) -> ImageIndex {
    // Load user overrides + libretro repo data in parallel — both are
    // small, both come from independent pools.
    let system_owned = system.to_string();
    let user_overrides_fut = state
        .user_data_reader
        .read(move |conn| UserDataDb::get_system_overrides(conn, &system_owned).ok());
    let libretro_fut = load_libretro_repo_data(&state.external_metadata_reader, system);
    let (user_overrides, libretro_repo_data) = tokio::join!(user_overrides_fut, libretro_fut);
    let user_overrides = user_overrides.flatten().unwrap_or_default();

    // Build the image index off any pool — it's a filesystem walk plus
    // pure data reduction over the libretro repo data already loaded above.
    let sys = system.to_string();
    let storage_root = state.storage().root.clone();
    tokio::task::spawn_blocking(move || {
        enrichment::build_image_index(&sys, &storage_root, user_overrides, libretro_repo_data)
    })
    .await
    .unwrap_or_else(|e| {
        tracing::warn!("build_image_index task panicked: {e}");
        empty_image_index()
    })
}

fn empty_image_index() -> ImageIndex {
    ImageIndex {
        dir_index: replay_control_core_server::image_matching::DirIndex {
            exact: Default::default(),
            exact_ci: Default::default(),
            fuzzy: Default::default(),
            version: Default::default(),
            aggressive: Default::default(),
            aggressive_compact: Default::default(),
        },
        db_paths: Default::default(),
        manifest: None,
    }
}

/// Queue an on-demand box-art download via the thumbnail orchestrator.
/// The on-complete hook persists `box_art_url` and invalidates user caches.
async fn queue_on_demand_download(
    state: &crate::api::AppState,
    system: &str,
    rom_filename: &str,
    m: &replay_control_core_server::thumbnail_manifest::ManifestMatch,
) {
    use replay_control_core_server::thumbnails::ThumbnailKind;

    use crate::api::thumbnail_orchestrator::{Outcome, ThumbnailKey};

    let key = ThumbnailKey {
        system: system.to_string(),
        kind: ThumbnailKind::Boxart,
        filename: m.filename.clone(),
    };

    // Capture the per-job state the on-complete hook needs. Cheap clones
    // (Arc + small strings); the orchestrator's dedup ensures this hook
    // only runs once per (system, filename).
    let library_pool = state.library_writer.clone();
    let state_for_invalidate = state.clone();
    let system_for_hook = system.to_string();
    let rom_filename_for_hook = rom_filename.to_string();
    let filename_for_hook = m.filename.clone();

    let on_complete: crate::api::thumbnail_orchestrator::OnCompleteHook = Box::new(move |result| {
        Box::pin(async move {
            match result.outcome {
                Outcome::Saved => {
                    let boxart_dir = ThumbnailKind::Boxart.media_dir();
                    let png_name = format!("{filename_for_hook}.png");
                    let url = replay_control_core_server::enrichment::format_box_art_url(
                        &system_for_hook,
                        &format!("{boxart_dir}/{png_name}"),
                    );
                    let sys = system_for_hook.clone();
                    let rom = rom_filename_for_hook.clone();
                    let _ = library_pool
                        .write(move |conn| {
                            if let Err(e) =
                                LibraryDb::update_box_art_url(conn, &sys, &rom, Some(&url))
                            {
                                tracing::error!("Failed to save box art URL for {sys}/{rom}: {e}");
                            }
                        })
                        .await;
                    state_for_invalidate.invalidate_user_caches().await;
                }
                Outcome::DownloadFailed(e) | Outcome::SaveFailed(e) => {
                    tracing::debug!("On-demand thumbnail failed for {filename_for_hook}: {e}");
                }
                Outcome::Skipped => {}
            }
        })
    });

    state
        .thumbnail_orchestrator
        .submit_visible(
            key,
            m.clone(),
            state.storage().root.clone(),
            Some(on_complete),
        )
        .await;
}
