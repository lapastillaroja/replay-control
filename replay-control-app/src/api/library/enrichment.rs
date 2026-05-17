use std::collections::HashMap;
use std::time::Instant;

use replay_control_core::error::{Error, Result};
use replay_control_core_server::enrichment::{self, ArcadeInfoLookup, ImageIndex};
use replay_control_core_server::external_metadata::{
    self, LAUNCHBOX_PROVIDER, ProviderGameRow, ProviderResourceRow, ThumbnailManifestEntry,
};
use replay_control_core_server::library_db::{
    LibraryDb, PhaseState, ThumbnailDownloadJob, ThumbnailPhaseState,
};
use replay_control_core_server::thumbnail_manifest;
use replay_control_core_server::thumbnails::{ALL_THUMBNAIL_KINDS, ThumbnailKind};
use replay_control_core_server::user_data_db::UserDataDb;

use super::{LibraryService, ScanCancellation, ScanInputs};
use crate::api::db_pools::LIBRARY_MAINTENANCE_WRITE_TIMEOUT;

impl LibraryService {
    pub(crate) async fn resume_pending_thumbnail_downloads(&self, state: &crate::api::AppState) {
        const THUMBNAIL_RESUME_LIMIT: usize = 100_000;

        let jobs = state
            .library_reader
            .read(|conn| {
                LibraryDb::load_pending_thumbnail_jobs(conn, THUMBNAIL_RESUME_LIMIT)
                    .unwrap_or_default()
            })
            .await
            .unwrap_or_default();
        if jobs.is_empty() {
            return;
        }
        tracing::info!("Thumbnail queue: resuming {} pending job(s)", jobs.len());
        for job in jobs {
            submit_thumbnail_job(state, job).await;
        }
    }

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
    ) -> Result<()> {
        let enrichment_started = Instant::now();
        if let Some(cancellation) = cancellation {
            cancellation.ensure_current()?;
        }
        set_enrichment_state(state, &system, PhaseState::Running).await?;

        // Fetch the library entries once; scan-derived metadata needs the
        // row metadata, while image-index, launchbox load, and arcade lookup
        // consume just the filenames.
        // The network/CPU work below makes it impossible to hold the
        // writer connection open for the whole pass; this read goes
        // through the dedicated reader pool. Stale-row risk is mitigated
        // by INSERT OR REPLACE on the write side and by FK CASCADE on
        // ROM deletion.
        let visible_started = Instant::now();
        let sys = system.clone();
        let library_entries = state
            .library_reader
            .read(move |conn| LibraryDb::load_system_entries(conn, &sys).unwrap_or_default())
            .await
            .unwrap_or_default();
        let rom_filenames: Vec<String> = library_entries
            .iter()
            .map(|entry| entry.rom_filename.clone())
            .collect();
        let storage_root = state.storage().root.clone();
        let visible_ms = visible_started.elapsed().as_millis();

        if rom_filenames.is_empty() {
            let sys = system.clone();
            if let Some(cancellation) = cancellation {
                cancellation.ensure_current()?;
            }
            let cleanup_started = Instant::now();
            let description_rows = Vec::new();
            let resource_rows = Vec::new();
            let cleanup_result = state
                .library_writer
                .try_write_with_timeout(LIBRARY_MAINTENANCE_WRITE_TIMEOUT, move |conn| {
                    LibraryDb::replace_detail_metadata_and_resources_for_system(
                        conn,
                        &sys,
                        &description_rows,
                        &resource_rows,
                    )
                })
                .await;
            match cleanup_result {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => {
                    tracing::warn!("Empty enrichment cleanup SQL failed for {system}: {e}");
                    return Err(e);
                }
                Err(e) => {
                    tracing::warn!("Empty enrichment cleanup write failed for {system}: {e}");
                    return Err(Error::Other(e.to_string()));
                }
            }
            set_enrichment_state(state, &system, PhaseState::Complete).await?;
            set_thumbnail_state(state, &system, ThumbnailPhaseState::Complete).await?;
            tracing::info!(
                "L2 enrichment profile: {system}: roms=0 visible_ms={visible_ms} cleanup_write_ms={} total_ms={}",
                cleanup_started.elapsed().as_millis(),
                enrichment_started.elapsed().as_millis()
            );
            return Ok(());
        }

        let scan_metadata_started = Instant::now();
        self.populate_scan_derived_metadata(
            state,
            &system,
            &library_entries,
            &ScanInputs::new(
                Default::default(),
                Default::default(),
                cancellation.cloned(),
            ),
        )
        .await?;
        let scan_metadata_ms = scan_metadata_started.elapsed().as_millis();
        if let Some(cancellation) = cancellation {
            cancellation.ensure_current()?;
        }

        // Independent setup steps that all consume `system` / `rom_filenames`.
        // Two pools (library, external_metadata) plus the arcade lookup;
        // `join!` lets the slowest overlap with the others.
        let setup_started = Instant::now();
        let catalog_titles = catalog_resource_lookup_titles(&rom_filenames);
        let (
            index,
            launchbox_rows,
            alt_to_primary,
            provider_resources,
            catalog_resources,
            arcade_lookup,
        ) = tokio::join!(
            build_image_index(state, &system),
            load_launchbox_rows(&state.external_metadata_reader, &system),
            load_launchbox_alt_to_primary(&state.external_metadata_reader, &system),
            load_launchbox_resources(&state.external_metadata_reader, &system),
            load_catalog_manual_resources(&system, catalog_titles),
            ArcadeInfoLookup::build(&system, &rom_filenames),
        );
        let setup_ms = setup_started.elapsed().as_millis();

        let sys = system.clone();
        let arcade_lookup_for_match = arcade_lookup.clone();
        // Heavy enrichment pass (per-row matching, manifest cross-ref,
        // image index lookups). Routed through the reader pool so the
        // single writer slot stays free for downloads and other writes;
        // the write closure below batches the resulting per-row updates.
        let match_started = Instant::now();
        let result = state
            .library_reader
            .read(move |conn| {
                replay_control_core_server::enrichment::enrich_system(
                    enrichment::EnrichSystemInput {
                        conn,
                        system: &sys,
                        index: &index,
                        arcade_lookup: &arcade_lookup_for_match,
                        launchbox_rows: &launchbox_rows,
                        alt_to_primary: &alt_to_primary,
                        provider_resources: &provider_resources,
                        catalog_resources: &catalog_resources,
                    },
                )
            })
            .await;
        let match_ms = match_started.elapsed().as_millis();

        let Some(result) = result else {
            tracing::warn!(
                "L2 enrichment profile: {system}: enrichment read unavailable after {match_ms}ms"
            );
            return Ok(());
        };

        if let Some(cancellation) = cancellation {
            cancellation.ensure_current()?;
        }

        // Queue missing thumbnails for every libretro media kind. Box art
        // also updates `game_library.box_art_url` on completion; snaps and
        // titles are filesystem media only.
        let manifest_started = Instant::now();
        let mut thumbnail_jobs = result
            .manifest_downloads
            .iter()
            .map(|(rom_filename, manifest_match)| ThumbnailDownloadJob {
                system: system.clone(),
                kind: ThumbnailKind::Boxart,
                rom_filename: rom_filename.clone(),
                manifest: manifest_match.clone(),
            })
            .collect::<Vec<_>>();
        for kind in ALL_THUMBNAIL_KINDS
            .iter()
            .copied()
            .filter(|kind| *kind != ThumbnailKind::Boxart)
        {
            thumbnail_jobs.extend(
                plan_missing_thumbnail_jobs(state, &storage_root, &system, kind, &arcade_lookup)
                    .await,
            );
        }
        let valid_rom_filenames = rom_filenames
            .iter()
            .map(String::as_str)
            .collect::<std::collections::HashSet<_>>();
        let before_valid_filter = thumbnail_jobs.len();
        let invalid_thumbnail_jobs = thumbnail_jobs
            .iter()
            .filter(|job| !valid_rom_filenames.contains(job.rom_filename.as_str()))
            .take(5)
            .map(|job| format!("{}:{}", job.kind.media_dir(), job.rom_filename))
            .collect::<Vec<_>>();
        thumbnail_jobs.retain(|job| valid_rom_filenames.contains(job.rom_filename.as_str()));
        let invalid_jobs = before_valid_filter.saturating_sub(thumbnail_jobs.len());
        if invalid_jobs > 0 {
            tracing::warn!(
                "Thumbnail queue: skipped {invalid_jobs} non-library image job(s) for {system}; sample={}",
                invalid_thumbnail_jobs.join(", ")
            );
        }
        let queued_thumbnail_jobs =
            queue_scan_thumbnail_downloads(state, &system, thumbnail_jobs).await;
        let thumbnail_state = if queued_thumbnail_jobs == 0 {
            ThumbnailPhaseState::Complete
        } else {
            ThumbnailPhaseState::Queued
        };
        let manifest_ms = manifest_started.elapsed().as_millis();

        // Keep the enrichment writes grouped by dependency, but release the
        // writer between groups. Partial enrichment is better than no
        // enrichment, so each SQL step logs and lets the pass continue.
        let region_pref = state.region_preference();
        let region_secondary = state.region_preference_secondary();
        let sys = system.clone();
        let dev_count = result.developer_updates.len();
        let coop_count = result.cooperative_updates.len();
        let date_count = result.release_date_rows.len();
        let desc_count = result.description_rows.len();
        let resource_count = result.resource_rows.len();
        let enrich_count = result.enrichments.len();
        let developer_updates = result.developer_updates;
        let cooperative_updates = result.cooperative_updates;
        let release_date_rows = result.release_date_rows;
        let description_rows = result.description_rows;
        let resource_rows = result.resource_rows;
        let enrichments = result.enrichments;
        if let Some(cancellation) = cancellation {
            cancellation.ensure_current()?;
        }

        let metadata_sys = sys.clone();
        let metadata_started = Instant::now();
        let metadata_result = state
            .library_writer
            .try_write_with_timeout(
                LIBRARY_MAINTENANCE_WRITE_TIMEOUT,
                move |conn| -> Result<()> {
                    let mut first_error = None;
                    if !developer_updates.is_empty()
                        && let Err(e) =
                            LibraryDb::update_developers(conn, &metadata_sys, &developer_updates)
                    {
                        tracing::warn!("Developer enrichment failed for {metadata_sys}: {e}");
                        first_error.get_or_insert(e);
                    }
                    if !cooperative_updates.is_empty()
                        && let Err(e) =
                            LibraryDb::update_cooperative(conn, &metadata_sys, &cooperative_updates)
                    {
                        tracing::warn!("Cooperative enrichment failed for {metadata_sys}: {e}");
                        first_error.get_or_insert(e);
                    }
                    // Upsert LaunchBox-sourced rows into game_release_date BEFORE
                    // the resolver runs. The resolver rebuilds game_library's
                    // mirror columns from game_release_date; any rows we miss
                    // here would be cleared to NULL on the same write.
                    if !release_date_rows.is_empty()
                        && let Err(e) = LibraryDb::upsert_release_dates(conn, &release_date_rows)
                    {
                        tracing::warn!("Release-date upsert failed for {metadata_sys}: {e}");
                        first_error.get_or_insert(e);
                    }
                    // Release-date resolver: rewrites game_library mirror columns
                    // from `game_release_date` for this system's ROMs.
                    if let Err(e) = LibraryDb::resolve_release_date_for_system(
                        conn,
                        &metadata_sys,
                        region_pref,
                        region_secondary,
                    ) {
                        tracing::warn!("Release-date resolve failed for {metadata_sys}: {e}");
                        first_error.get_or_insert(e);
                    }
                    first_error.map_or(Ok(()), Err)
                },
            )
            .await;
        let metadata_write_ms = metadata_started.elapsed().as_millis();
        let mut failed = false;
        match metadata_result {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => {
                tracing::warn!("Metadata enrichment SQL failed for {sys}: {e}");
                failed = true;
            }
            Err(e) => {
                tracing::warn!("Metadata enrichment write failed for {sys}: {e}");
                failed = true;
            }
        }

        if let Some(cancellation) = cancellation {
            cancellation.ensure_current()?;
        }
        let detail_sys = sys.clone();
        let detail_description_rows = description_rows.clone();
        let detail_resource_rows = resource_rows.clone();
        let detail_started = Instant::now();
        let detail_result = state
            .library_writer
            .try_write_with_timeout(LIBRARY_MAINTENANCE_WRITE_TIMEOUT, move |conn| {
                LibraryDb::replace_detail_metadata_and_resources_for_system(
                    conn,
                    &detail_sys,
                    &detail_description_rows,
                    &detail_resource_rows,
                )
            })
            .await;
        let detail_write_ms = detail_started.elapsed().as_millis();
        match detail_result {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => {
                tracing::warn!("Detail/resource enrichment SQL failed for {sys}: {e}");
                failed = true;
            }
            Err(e) => {
                tracing::warn!("Detail/resource enrichment write failed for {sys}: {e}");
                failed = true;
            }
        }

        if let Some(cancellation) = cancellation {
            cancellation.ensure_current()?;
        }
        let box_art_sys = sys.clone();
        let box_art_started = Instant::now();
        let box_art_result = state
            .library_writer
            .try_write_with_timeout(
                LIBRARY_MAINTENANCE_WRITE_TIMEOUT,
                move |conn| -> Result<()> {
                    let mut first_error = None;
                    if !enrichments.is_empty()
                        && let Err(e) =
                            LibraryDb::update_box_art_genre_rating(conn, &box_art_sys, &enrichments)
                    {
                        tracing::warn!("Enrichment failed for {box_art_sys}: {e}");
                        first_error.get_or_insert(e);
                    }
                    first_error.map_or(Ok(()), Err)
                },
            )
            .await;
        let box_art_write_ms = box_art_started.elapsed().as_millis();
        match box_art_result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                tracing::warn!("Box art/genre/rating enrichment SQL failed for {sys}: {e}");
                failed = true;
            }
            Err(e) => {
                tracing::warn!("Box art/genre/rating enrichment write failed for {sys}: {e}");
                failed = true;
            }
        }

        tracing::debug!(
            "L2 enrichment: {system} — {dev_count} dev / {coop_count} coop / {date_count} dates / {desc_count} desc / {resource_count} resources / {enrich_count} box+genre+players+ratings"
        );
        tracing::info!(
            "L2 enrichment profile: {system}: roms={} visible_ms={visible_ms} scan_metadata_ms={scan_metadata_ms} setup_ms={setup_ms} match_ms={match_ms} manifest_ms={manifest_ms} metadata_write_ms={metadata_write_ms} detail_write_ms={detail_write_ms} box_art_write_ms={box_art_write_ms} total_ms={} dev={dev_count} coop={coop_count} dates={date_count} desc={desc_count} resources={resource_count} enrich={enrich_count}",
            rom_filenames.len(),
            enrichment_started.elapsed().as_millis()
        );
        if failed {
            let _ = set_enrichment_state(state, &system, PhaseState::Failed).await;
            Err(Error::Other(format!("enrichment failed for {system}")))
        } else {
            set_enrichment_state(state, &system, PhaseState::Complete).await?;
            set_thumbnail_state(state, &system, thumbnail_state).await?;
            Ok(())
        }
    }
}

async fn set_enrichment_state(
    state: &crate::api::AppState,
    system: &str,
    phase: PhaseState,
) -> Result<()> {
    let sys = system.to_string();
    let write = state
        .library_writer
        .try_write_with_timeout(LIBRARY_MAINTENANCE_WRITE_TIMEOUT, move |conn| {
            LibraryDb::set_enrichment_state(conn, &sys, phase)
        })
        .await;
    match write {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(e)) => Err(e),
        Err(e) => Err(Error::Other(e.to_string())),
    }
}

async fn set_thumbnail_state(
    state: &crate::api::AppState,
    system: &str,
    phase: ThumbnailPhaseState,
) -> Result<()> {
    let sys = system.to_string();
    let write = state
        .library_writer
        .try_write_with_timeout(LIBRARY_MAINTENANCE_WRITE_TIMEOUT, move |conn| {
            LibraryDb::set_thumbnail_state(conn, &sys, phase)
        })
        .await;
    match write {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(e)) => Err(e),
        Err(e) => Err(Error::Other(e.to_string())),
    }
}

/// Load the per-system LaunchBox provider rows from the host-global
/// `external_metadata.db`. Returns an empty map when the pool is unavailable
/// or the read fails (the design treats LaunchBox as optional — users
/// without it still get scan-time + catalog enrichment).
async fn load_launchbox_rows(
    em_reader: &crate::api::db_pools::ExternalMetadataReadPool,
    system: &str,
) -> HashMap<String, ProviderGameRow> {
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

async fn load_launchbox_resources(
    em_reader: &crate::api::db_pools::ExternalMetadataReadPool,
    system: &str,
) -> HashMap<String, Vec<ProviderResourceRow>> {
    let sys = system.to_string();
    em_reader
        .read(move |conn| {
            external_metadata::system_provider_resources(conn, LAUNCHBOX_PROVIDER, &sys, "video")
                .unwrap_or_default()
        })
        .await
        .unwrap_or_default()
}

fn catalog_resource_lookup_titles(rom_filenames: &[String]) -> Vec<String> {
    let mut titles = std::collections::HashSet::new();
    for filename in rom_filenames {
        let stem = replay_control_core::title_utils::filename_stem(filename);
        let normalized = replay_control_core::title_utils::normalize_title_for_metadata(stem);
        if !normalized.is_empty() {
            titles.insert(normalized);
        }
    }
    titles.into_iter().collect()
}

async fn load_catalog_manual_resources(
    system: &str,
    normalized_titles: Vec<String>,
) -> HashMap<String, Vec<replay_control_core_server::catalog_pool::CatalogGameResourceRow>> {
    replay_control_core_server::catalog_pool::lookup_catalog_game_resources(
        system,
        &normalized_titles,
        "manual",
    )
    .await
}

/// Load the per-system libretro repo manifest data from
/// `external_metadata.db`. Used by `build_image_index` to populate the
/// manifest fuzzy index. Empty list when the pool is unavailable or the
/// system has no libretro repos configured.
async fn load_libretro_repo_data(
    em_reader: &crate::api::db_pools::ExternalMetadataReadPool,
    system: &str,
    kind: ThumbnailKind,
) -> Vec<(String, String, Vec<ThumbnailManifestEntry>)> {
    let Some(repo_names) = replay_control_core_server::thumbnails::thumbnail_repo_names(system)
    else {
        return Vec::new();
    };
    let display_names: Vec<String> = repo_names.iter().map(|s| (*s).to_string()).collect();
    em_reader
        .read(move |conn| {
            let display_refs: Vec<&str> = display_names.iter().map(String::as_str).collect();
            thumbnail_manifest::load_repo_manifest_data(conn, &display_refs, kind.repo_dir())
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
    let libretro_fut = load_libretro_repo_data(
        &state.external_metadata_reader,
        system,
        ThumbnailKind::Boxart,
    );
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

async fn plan_missing_thumbnail_jobs(
    state: &crate::api::AppState,
    storage_root: &std::path::Path,
    system: &str,
    kind: ThumbnailKind,
    arcade_lookup: &ArcadeInfoLookup,
) -> Vec<ThumbnailDownloadJob> {
    let repo_data = load_libretro_repo_data(&state.external_metadata_reader, system, kind).await;
    if repo_data.is_empty() {
        return Vec::new();
    }
    match thumbnail_manifest::plan_system_thumbnails_from_repo_data(
        &repo_data,
        storage_root,
        system,
        kind,
        arcade_lookup,
    ) {
        Ok(plan) => plan
            .work_items
            .into_iter()
            .map(|(rom_filename, manifest)| ThumbnailDownloadJob {
                system: system.to_string(),
                rom_filename,
                kind,
                manifest,
            })
            .collect(),
        Err(e) => {
            let kind_name = kind.media_dir();
            tracing::warn!("{kind_name} thumbnail plan failed for {system}: {e}");
            Vec::new()
        }
    }
}

async fn queue_scan_thumbnail_downloads(
    state: &crate::api::AppState,
    system: &str,
    jobs: Vec<ThumbnailDownloadJob>,
) -> usize {
    if jobs.is_empty() {
        return 0;
    }
    let job_count = jobs.len();
    let persist_jobs = jobs.clone();
    let persist_result = state
        .library_writer
        .try_write_with_timeout(LIBRARY_MAINTENANCE_WRITE_TIMEOUT, move |conn| {
            LibraryDb::upsert_thumbnail_jobs(conn, &persist_jobs)
        })
        .await;
    match persist_result {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => {
            tracing::warn!("Thumbnail queue SQL failed for {system}: {e}");
            return 0;
        }
        Err(e) => {
            tracing::warn!("Thumbnail queue write failed for {system}: {e}");
            return 0;
        }
    }

    tracing::info!("Thumbnail queue: queued {job_count} missing image(s) for {system}");
    let state = state.clone();
    tokio::spawn(async move {
        for job in jobs {
            submit_thumbnail_job(&state, job).await;
        }
    });
    job_count
}

async fn submit_thumbnail_job(state: &crate::api::AppState, job: ThumbnailDownloadJob) {
    use crate::api::thumbnail_orchestrator::ThumbnailKey;

    let key = ThumbnailKey {
        system: job.system.clone(),
        kind: job.kind,
        filename: job.manifest.filename.clone(),
    };
    let on_complete = thumbnail_completion_hook(state, &key);
    let storage_root = state.storage().root.clone();
    state
        .thumbnail_orchestrator
        .submit_background(key, job.manifest, storage_root, Some(on_complete))
        .await;
}

fn thumbnail_completion_hook(
    state: &crate::api::AppState,
    key: &crate::api::thumbnail_orchestrator::ThumbnailKey,
) -> crate::api::thumbnail_orchestrator::OnCompleteHook {
    use crate::api::thumbnail_orchestrator::Outcome;

    let library_pool = state.library_writer.clone();
    let state_for_invalidate = state.clone();
    let system = key.system.clone();
    let kind = key.kind;
    let filename = key.filename.clone();

    Box::new(move |result| {
        Box::pin(async move {
            match result.outcome {
                Outcome::Saved => {
                    let png_name = format!("{filename}.png");
                    let url = replay_control_core_server::enrichment::format_box_art_url(
                        &system,
                        &format!("{}/{png_name}", kind.media_dir()),
                    );
                    let sys = system.clone();
                    let filename_for_db = filename.clone();
                    match library_pool
                        .try_write_with_timeout(LIBRARY_MAINTENANCE_WRITE_TIMEOUT, move |conn| {
                            LibraryDb::complete_thumbnail_jobs_for_key(
                                conn,
                                &sys,
                                kind,
                                &filename_for_db,
                                &url,
                            )
                        })
                        .await
                    {
                        Ok(Ok(updated)) => {
                            if updated > 0 {
                                state_for_invalidate.invalidate_user_caches().await;
                            }
                        }
                        Ok(Err(e)) => tracing::warn!("Thumbnail completion SQL failed: {e}"),
                        Err(e) => tracing::warn!("Thumbnail completion write failed: {e}"),
                    }
                }
                Outcome::DownloadFailed(e) | Outcome::SaveFailed(e) => {
                    let sys = system.clone();
                    let filename_for_db = filename.clone();
                    match library_pool
                        .try_write_with_timeout(LIBRARY_MAINTENANCE_WRITE_TIMEOUT, move |conn| {
                            LibraryDb::fail_thumbnail_jobs_for_key(
                                conn,
                                &sys,
                                kind,
                                &filename_for_db,
                            )
                        })
                        .await
                    {
                        Ok(Ok(_)) => {}
                        Ok(Err(err)) => tracing::warn!("Thumbnail failure SQL failed: {err}"),
                        Err(err) => tracing::warn!("Thumbnail failure write failed: {err}"),
                    }
                    tracing::debug!("Thumbnail download failed for {system}/{filename}: {e}");
                }
                Outcome::Skipped => {}
            }
        })
    })
}
