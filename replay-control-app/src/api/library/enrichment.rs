use std::collections::HashMap;

use replay_control_core::enrichment::{self, ImageIndex};
use replay_control_core::metadata_db::MetadataDb;
use replay_control_core::user_data_db::UserDataDb;

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
        let db = &state.metadata_pool;

        // Build a temporary image index for this enrichment run (not cached).
        let index = build_image_index(state, &system).await;

        // Auto-match new ROMs against existing LaunchBox metadata.
        let auto_matched_ratings = self.auto_match_metadata(state, &system).await;

        // Run the pure enrichment pipeline in a single DB read.
        let sys = system.clone();
        let result = db
            .read(move |conn| {
                replay_control_core::enrichment::enrich_system(
                    conn,
                    &sys,
                    &index,
                    &auto_matched_ratings,
                )
            })
            .await;

        let Some(result) = result else {
            return;
        };

        // Queue on-demand manifest downloads (app-specific: needs AppState).
        for (rom_filename, manifest_match) in &result.manifest_downloads {
            queue_on_demand_download(state, &system, rom_filename, manifest_match);
        }

        // Write developer updates to DB.
        if !result.developer_updates.is_empty() {
            let dev_count = result.developer_updates.len();
            let sys = system.clone();
            let updates = result.developer_updates;
            db.write(move |conn| {
                if let Err(e) = MetadataDb::update_developers(conn, &sys, &updates) {
                    tracing::warn!("Developer enrichment failed for {sys}: {e}");
                }
            })
            .await;
            tracing::debug!("L2 enrichment: {system} — {dev_count} ROMs updated with developer");
        }

        // Write release year updates to DB.
        if !result.year_updates.is_empty() {
            let year_count = result.year_updates.len();
            let sys = system.clone();
            let updates = result.year_updates;
            db.write(move |conn| {
                if let Err(e) = MetadataDb::update_release_years(conn, &sys, &updates) {
                    tracing::warn!("Release year enrichment failed for {sys}: {e}");
                }
            })
            .await;
            tracing::debug!(
                "L2 enrichment: {system} — {year_count} ROMs updated with release_year"
            );
        }

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
            if let Err(e) = MetadataDb::update_box_art_genre_rating(conn, &system, &enrichments) {
                tracing::warn!("Enrichment failed for {system}: {e}");
            }
        })
        .await;
    }

    /// Auto-match new ROMs against existing LaunchBox metadata by normalized title.
    ///
    /// Delegates the pure matching logic to `replay_control_core::metadata_matching`,
    /// then persists results and returns a map of `rom_filename -> rating`.
    async fn auto_match_metadata(
        &self,
        state: &crate::api::AppState,
        system: &str,
    ) -> HashMap<String, f64> {
        use replay_control_core::metadata_matching;

        let db = &state.metadata_pool;

        // Gather inputs: existing metadata from DB.
        let sys = system.to_string();
        let all_metadata = state
            .metadata_pool
            .read(move |conn| MetadataDb::system_metadata_all(conn, &sys).ok())
            .await
            .flatten()
            .unwrap_or_default();

        if all_metadata.is_empty() {
            return HashMap::new();
        }

        // Gather inputs: ROM filenames from L2 (SQLite).
        let sys = system.to_string();
        let rom_filenames: Vec<String> = db
            .read(move |conn| MetadataDb::visible_filenames(conn, &sys).unwrap_or_default())
            .await
            .unwrap_or_default();

        if rom_filenames.is_empty() {
            return HashMap::new();
        }

        // Call pure core matching function.
        let matches =
            metadata_matching::match_roms_to_metadata(system, &rom_filenames, &all_metadata);

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
            if let Err(e) = MetadataDb::bulk_upsert(conn, &new_entries) {
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

    // Build the image index using the metadata pool connection.
    let sys = system.to_string();
    let storage_root = state.storage().root.clone();
    state
        .metadata_pool
        .read(move |conn| enrichment::build_image_index(conn, &sys, &storage_root, user_overrides))
        .await
        .unwrap_or_else(|| {
            // Pool unavailable — return an empty index.
            ImageIndex {
                dir_index: replay_control_core::image_matching::DirIndex {
                    exact: Default::default(),
                    exact_ci: Default::default(),
                    fuzzy: Default::default(),
                    version: Default::default(),
                    aggressive: Default::default(),
                },
                db_paths: Default::default(),
                manifest: None,
            }
        })
}

/// Queue a background download for a single thumbnail.
/// Deduplicates concurrent requests for the same image.
fn queue_on_demand_download(
    state: &crate::api::AppState,
    system: &str,
    rom_filename: &str,
    m: &replay_control_core::thumbnail_manifest::ManifestMatch,
) {
    use replay_control_core::thumbnail_manifest::{download_thumbnail, save_thumbnail};
    use replay_control_core::thumbnails::ThumbnailKind;

    let download_key = format!("{system}/{}", m.filename);

    // Check and insert atomically to prevent duplicate downloads.
    {
        let mut pending = state.pending_downloads.write().expect("pending lock");
        if !pending.insert(download_key.clone()) {
            return; // Already queued.
        }
    }

    let m = m.clone();
    let storage_root = state.storage().root.clone();
    let system = system.to_string();
    let rom_filename = rom_filename.to_string();
    let pending = state.pending_downloads.clone();
    let metadata_pool = state.metadata_pool.clone();
    let response_cache = state.response_cache.clone();
    let rt_handle = tokio::runtime::Handle::current();

    std::thread::spawn(move || {
        match download_thumbnail(&m, ThumbnailKind::Boxart.repo_dir()) {
            Ok(bytes) => {
                if let Err(e) = save_thumbnail(
                    &storage_root,
                    &system,
                    ThumbnailKind::Boxart,
                    &m.filename,
                    &bytes,
                ) {
                    tracing::debug!("On-demand save failed for {}: {e}", m.filename);
                } else {
                    // Update box_art_url in the DB so it's visible immediately.
                    let boxart_dir = ThumbnailKind::Boxart.media_dir();
                    let png_name = format!("{}.png", m.filename);
                    let url = replay_control_core::enrichment::format_box_art_url(
                        &system,
                        &format!("{boxart_dir}/{png_name}"),
                    );
                    let sys = system.clone();
                    let rom = rom_filename.clone();
                    let _ = rt_handle.block_on(
                        metadata_pool.write(move |conn| {
                            let _ = conn.execute(
                                "UPDATE game_library SET box_art_url = ?1 WHERE system = ?2 AND rom_filename = ?3",
                                [&url, &sys, &rom],
                            );
                        }),
                    );
                    // Clear response cache so next page load picks up the new art.
                    response_cache.invalidate_all();
                }
            }
            Err(e) => {
                tracing::debug!("On-demand download failed for {}: {e}", m.filename);
            }
        }

        // Remove from pending set.
        if let Ok(mut guard) = pending.write() {
            guard.remove(&download_key);
        }
    });
}
