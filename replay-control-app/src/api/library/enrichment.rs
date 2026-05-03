use std::collections::HashMap;

use replay_control_core_server::enrichment::{self, ArcadeInfoLookup, ImageIndex};
use replay_control_core_server::library_db::LibraryDb;
use replay_control_core_server::user_data_db::UserDataDb;

use super::LibraryService;

impl LibraryService {
    /// Enrich box_art_url (and rating) for all entries in a system's game library.
    /// Uses the image index for box art and game_metadata for ratings.
    /// Called after L2 write-through to populate fields that `list_roms()` doesn't set.
    ///
    /// Also auto-matches new ROMs (those without metadata) against existing
    /// LaunchBox entries by normalized title. Matched metadata is persisted
    /// so future lookups hit directly without re-matching.
    pub async fn enrich_system_cache(&self, state: &crate::api::AppState, system: String) {
        let db = &state.library_pool;

        // Fetch the visible-filename list once; image-index, auto-match,
        // and arcade lookup all consume it. Previously each path read
        // it independently (an N+1 against the same query).
        let sys = system.clone();
        let rom_filenames: Vec<String> = db
            .read(move |conn| LibraryDb::visible_filenames(conn, &sys).unwrap_or_default())
            .await
            .unwrap_or_default();

        if rom_filenames.is_empty() {
            return;
        }

        // Three independent setup steps. The pool's read slots serialize
        // as needed; `join!` lets the slowest overlap with the others.
        let (index, auto_matched_ratings, arcade_lookup) = tokio::join!(
            build_image_index(state, &system),
            self.auto_match_metadata(state, &system, &rom_filenames),
            ArcadeInfoLookup::build(&system, &rom_filenames),
        );

        let sys = system.clone();
        // Heavy enrichment pass (per-row matching, manifest cross-ref, image
        // index lookups). Library pool has 3 read slots, so SSR keeps the
        // other 2 free while this runs.
        let result = db
            .read(move |conn| {
                replay_control_core_server::enrichment::enrich_system(
                    conn,
                    &sys,
                    &index,
                    &arcade_lookup,
                    &auto_matched_ratings,
                )
            })
            .await;

        let Some(result) = result else {
            return;
        };

        // Queue on-demand manifest downloads. Each await throttles to
        // the orchestrator's visible queue capacity so a large fan-out
        // (e.g. 4k+ thumbnails after a fresh rescan) backpressures
        // instead of dropping work.
        for (rom_filename, manifest_match) in &result.manifest_downloads {
            queue_on_demand_download(state, &system, rom_filename, manifest_match).await;
        }

        // Write developer updates to DB.
        if !result.developer_updates.is_empty() {
            let dev_count = result.developer_updates.len();
            let sys = system.clone();
            let updates = result.developer_updates;
            db.write(move |conn| {
                if let Err(e) = LibraryDb::update_developers(conn, &sys, &updates) {
                    tracing::warn!("Developer enrichment failed for {sys}: {e}");
                }
            })
            .await;
            tracing::debug!("L2 enrichment: {system} — {dev_count} ROMs updated with developer");
        }

        // Write cooperative updates to DB.
        if !result.cooperative_updates.is_empty() {
            let coop_count = result.cooperative_updates.len();
            let sys = system.clone();
            let updates = result.cooperative_updates;
            db.write(move |conn| {
                if let Err(e) = LibraryDb::update_cooperative(conn, &sys, &updates) {
                    tracing::warn!("Cooperative enrichment failed for {sys}: {e}");
                }
            })
            .await;
            tracing::debug!("L2 enrichment: {system} — {coop_count} ROMs updated with cooperative");
        }

        // Write release year updates to DB.
        if !result.year_updates.is_empty() {
            let year_count = result.year_updates.len();
            let sys = system.clone();
            let updates = result.year_updates;
            db.write(move |conn| {
                if let Err(e) = LibraryDb::update_release_years(conn, &sys, &updates) {
                    tracing::warn!("Release year enrichment failed for {sys}: {e}");
                }
            })
            .await;
            tracing::debug!(
                "L2 enrichment: {system} — {year_count} ROMs updated with release_year"
            );
        }

        // Seed game_release_date from game_metadata (LaunchBox day-precision dates),
        // then re-run the resolver so game_library mirror columns reflect the new info.
        // The precision-upgrade rule ensures day > month > year.
        let region_pref = state.region_preference();
        let region_secondary = state.region_preference_secondary();
        db.write(move |conn| {
            let _ = LibraryDb::seed_release_dates_from_metadata(conn);
            let _ =
                LibraryDb::resolve_release_date_for_library(conn, region_pref, region_secondary);
        })
        .await;

        if result.enrichments.is_empty() {
            return;
        }

        let count = result.enrichments.len();

        tracing::debug!(
            "L2 enrichment: {system} — {count} ROMs updated with box art/genre/players/ratings"
        );

        // Write enrichments to L2 (SQLite).
        let enrichments = result.enrichments;
        db.write(move |conn| {
            if let Err(e) = LibraryDb::update_box_art_genre_rating(conn, &system, &enrichments) {
                tracing::warn!("Enrichment failed for {system}: {e}");
            }
        })
        .await;
    }

    /// Auto-match new ROMs against existing LaunchBox metadata by normalized title.
    ///
    /// Delegates the pure matching logic to `replay_control_core_server::metadata_matching`,
    /// then persists results and returns a map of `rom_filename -> rating`.
    async fn auto_match_metadata(
        &self,
        state: &crate::api::AppState,
        system: &str,
        rom_filenames: &[String],
    ) -> HashMap<String, f64> {
        use replay_control_core_server::metadata_matching;

        let db = &state.library_pool;

        let sys = system.to_string();
        let all_metadata = state
            .library_pool
            .read(move |conn| LibraryDb::system_metadata_all(conn, &sys).ok())
            .await
            .flatten()
            .unwrap_or_default();

        if all_metadata.is_empty() {
            return HashMap::new();
        }

        let matches =
            metadata_matching::match_roms_to_metadata(system, rom_filenames, &all_metadata).await;

        if matches.is_empty() {
            return HashMap::new();
        }

        // Build ratings map and persistence entries from results.
        let mut matched_ratings: HashMap<String, f64> = HashMap::new();
        let new_entries: Vec<(String, String, _)> = matches
            .into_iter()
            .map(|m| {
                if let Some(rating) = m.metadata.rating {
                    matched_ratings.insert(m.rom_filename.clone(), rating);
                }
                (system.to_string(), m.rom_filename, m.metadata)
            })
            .collect();

        // Persist new matches to game_metadata.
        let count = new_entries.len();
        let sys = system.to_string();
        db.write(move |conn| {
            if let Err(e) = LibraryDb::bulk_upsert(conn, &new_entries) {
                tracing::warn!("Auto-match metadata persist failed for {sys}: {e}");
            }
        })
        .await;
        tracing::info!("Auto-matched {count} new ROM(s) to existing metadata for {system}");

        matched_ratings
    }
}

// ── Image index + on-demand downloads ──────────────────────────────

/// Build an image index for a system.
///
/// Orchestrates pool access (user_data + metadata) and delegates to
/// the core `enrichment::build_image_index` which does pure DB + filesystem work.
async fn build_image_index(state: &crate::api::AppState, system: &str) -> ImageIndex {
    // Load user box art overrides first (separate pool, no contention with metadata).
    let system_owned = system.to_string();
    let user_overrides = state
        .user_data_pool
        .read(move |conn| UserDataDb::get_system_overrides(conn, &system_owned).ok())
        .await
        .flatten()
        .unwrap_or_default();

    // Build the image index. Walks every visible filename for the system
    // and resolves matched images — long enough that it'd serialise SSR
    // on a single-slot pool. Library pool has 3 slots, so SSR keeps the
    // others free while this runs.
    let sys = system.to_string();
    let storage_root = state.storage().root.clone();
    state
        .library_pool
        .read(move |conn| enrichment::build_image_index(conn, &sys, &storage_root, user_overrides))
        .await
        .unwrap_or_else(|| {
            // Pool unavailable — return an empty index.
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
        })
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
    let library_pool = state.library_pool.clone();
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
