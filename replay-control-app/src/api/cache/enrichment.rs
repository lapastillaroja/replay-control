use std::collections::HashMap;

use replay_control_core::metadata_db::MetadataDb;

use super::GameLibrary;

impl GameLibrary {
    /// Enrich box_art_url (and rating) for all entries in a system's game library.
    /// Uses the image index for box art and game_metadata for ratings.
    /// Called after L2 write-through to populate fields that `list_roms()` doesn't set.
    ///
    /// Also auto-matches new ROMs (those without metadata) against existing
    /// LaunchBox entries by normalized title. Matched metadata is persisted
    /// so future lookups hit directly without re-matching.
    pub async fn enrich_system_cache(&self, state: &crate::api::AppState, system: String) {
        // Build a temporary image index for this enrichment run (not cached).
        let index = super::images::build_image_index(state, &system).await;

        // Auto-match new ROMs against existing LaunchBox metadata.
        let auto_matched_ratings = self.auto_match_metadata(state, &system).await;

        // Run the pure enrichment pipeline in a single DB read.
        let sys = system.clone();
        let result = self
            .db
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
            super::images::queue_on_demand_download(state, &system, rom_filename, manifest_match);
        }

        // Write developer updates to DB.
        if !result.developer_updates.is_empty() {
            let dev_count = result.developer_updates.len();
            let sys = system.clone();
            let updates = result.developer_updates;
            self.db
                .write(move |conn| {
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
            self.db
                .write(move |conn| {
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
        self.db
            .write(move |conn| {
                if let Err(e) =
                    MetadataDb::update_box_art_genre_rating(conn, &system, &enrichments)
                {
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
        let rom_filenames: Vec<String> = self
            .db
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
        self.db
            .write(move |conn| {
                if let Err(e) = MetadataDb::bulk_upsert(conn, &new_entries) {
                    tracing::warn!("Auto-match metadata persist failed for {sys}: {e}");
                }
            })
            .await;
        tracing::info!("Auto-matched {count} new ROM(s) to existing metadata for {system}");

        matched_ratings
    }
}
