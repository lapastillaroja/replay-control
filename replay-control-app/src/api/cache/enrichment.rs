use std::collections::{HashMap, HashSet};

use replay_control_core::metadata_db::MetadataDb;

use super::GameLibrary;

/// Batched metadata from LaunchBox import (ratings, genres, players, rating counts, developers, release years).
type LaunchBoxMetadata = (
    HashMap<String, f64>,
    HashMap<String, String>,
    HashMap<String, u8>,
    HashMap<String, u32>,
    HashMap<String, String>,
    HashMap<String, u16>,
);

impl GameLibrary {
    /// Enrich box_art_url (and rating) for all entries in a system's game library.
    /// Uses the image index for box art and game_metadata for ratings.
    /// Called after L2 write-through to populate fields that `list_roms()` doesn't set.
    ///
    /// Also auto-matches new ROMs (those without metadata) against existing
    /// LaunchBox entries by normalized title. Matched metadata is persisted
    /// so future lookups hit directly without re-matching.
    pub async fn enrich_system_cache(&self, state: &crate::api::AppState, system: String) {
        let index = self.get_image_index(state, &system).await;

        // Load ratings, genres, players, rating counts, and developers from
        // game_metadata table (from LaunchBox import) in a single read call.
        let sys = system.clone();
        let (ratings, lb_genres, lb_players, lb_rating_counts, lb_developers, lb_release_years): LaunchBoxMetadata =
            state
            .metadata_pool
            .read(move |conn| {
                let ratings = MetadataDb::system_ratings(conn, &sys)
                    .ok()
                    .unwrap_or_default();
                let genres = MetadataDb::system_metadata_genres(conn, &sys)
                    .ok()
                    .unwrap_or_default();
                let players = MetadataDb::system_metadata_players(conn, &sys)
                    .ok()
                    .unwrap_or_default();
                let rating_counts = MetadataDb::system_metadata_rating_counts(conn, &sys)
                    .ok()
                    .unwrap_or_default();
                let developers = MetadataDb::system_metadata_developers(conn, &sys)
                    .ok()
                    .unwrap_or_default();
                let release_years = MetadataDb::system_metadata_release_years(conn, &sys)
                    .ok()
                    .unwrap_or_default();
                (ratings, genres, players, rating_counts, developers, release_years)
            })
            .await
            .unwrap_or_default();

        // Load current game_library genres, players, developers, and release_years from L2
        // to know which are already set, in a single read call.
        let sys = system.clone();
        let (existing_genres, existing_players, existing_developers, existing_years): (
            HashSet<String>,
            HashSet<String>,
            HashSet<String>,
            HashSet<String>,
        ) = self
            .db
            .read(move |conn| {
                let genres = MetadataDb::system_rom_genres(conn, &sys)
                    .map(|map| map.into_keys().collect())
                    .unwrap_or_default();
                let players = MetadataDb::system_rom_players(conn, &sys).unwrap_or_default();
                let developers = MetadataDb::system_rom_developers(conn, &sys).unwrap_or_default();
                let years = MetadataDb::system_rom_release_years(conn, &sys).unwrap_or_default();
                (genres, players, developers, years)
            })
            .await
            .unwrap_or_default();

        // Auto-match new ROMs: build a normalized-title index from existing
        // game_metadata entries so ROMs added after the last import can inherit
        // metadata from entries that share the same normalized title.
        let auto_matched_ratings = self.auto_match_metadata(state, &system).await;

        // Merge auto-matched ratings into the main ratings map.
        let mut all_ratings = ratings;
        for (filename, rating) in &auto_matched_ratings {
            all_ratings.entry(filename.clone()).or_insert(*rating);
        }

        // Read current ROMs from L1 cache to get filenames.
        let rom_filenames: Vec<String> = if let Ok(guard) = self.roms.read() {
            guard
                .get(&system)
                .map(|entry| {
                    entry
                        .data
                        .iter()
                        .map(|r| r.game.rom_filename.clone())
                        .collect()
                })
                .unwrap_or_default()
        } else {
            return;
        };

        if rom_filenames.is_empty() {
            return;
        }

        // Build enrichment entries: box_art_url, genre, players, rating, rating_count per ROM.
        // Genre and players are only filled from LaunchBox when game_library has no value.
        let enrichments: Vec<replay_control_core::metadata_db::BoxArtGenreRating> = rom_filenames
            .iter()
            .filter_map(|filename| {
                let art = self.resolve_box_art(state, &index, &system, filename);
                let rating = all_ratings.get(filename).map(|&r| r as f32);
                let rating_count = lb_rating_counts.get(filename).copied();
                let genre = if !existing_genres.contains(filename) {
                    lb_genres.get(filename).cloned()
                } else {
                    None
                };
                let players = if !existing_players.contains(filename) {
                    lb_players.get(filename).copied()
                } else {
                    None
                };
                if art.is_none()
                    && rating.is_none()
                    && rating_count.is_none()
                    && genre.is_none()
                    && players.is_none()
                {
                    return None;
                }
                Some(replay_control_core::metadata_db::BoxArtGenreRating {
                    rom_filename: filename.clone(),
                    box_art_url: art,
                    genre,
                    players,
                    rating,
                    rating_count,
                })
            })
            .collect();

        // Enrich developer from LaunchBox metadata for ROMs that don't already have one.
        // This runs as a separate update because developer uses a different SQL method
        // and doesn't need to be bundled with the box_art/genre/rating enrichment.
        let developer_updates: Vec<(String, String)> = rom_filenames
            .iter()
            .filter(|f| !existing_developers.contains(*f))
            .filter_map(|f| {
                lb_developers.get(f).map(|dev| {
                    let normalized = replay_control_core::developer::normalize_developer(dev);
                    (f.clone(), normalized)
                })
            })
            .filter(|(_, dev)| !dev.is_empty())
            .collect();

        if !developer_updates.is_empty() {
            let dev_count = developer_updates.len();
            let sys = system.clone();
            self.db
                .write(move |conn| {
                    if let Err(e) = MetadataDb::update_developers(conn, &sys, &developer_updates) {
                        tracing::warn!("Developer enrichment failed for {sys}: {e}");
                    }
                })
                .await;
            tracing::debug!("L2 enrichment: {system} — {dev_count} ROMs updated with developer");
        }

        // Enrich release_year from LaunchBox metadata for ROMs that don't already have one.
        let year_updates: Vec<(String, u16)> = rom_filenames
            .iter()
            .filter(|f| !existing_years.contains(*f))
            .filter_map(|f| lb_release_years.get(f).map(|&year| (f.clone(), year)))
            .collect();

        if !year_updates.is_empty() {
            let year_count = year_updates.len();
            let sys = system.clone();
            self.db
                .write(move |conn| {
                    if let Err(e) = MetadataDb::update_release_years(conn, &sys, &year_updates) {
                        tracing::warn!("Release year enrichment failed for {sys}: {e}");
                    }
                })
                .await;
            tracing::debug!(
                "L2 enrichment: {system} — {year_count} ROMs updated with release_year"
            );
        }

        if enrichments.is_empty() {
            return;
        }

        let count = enrichments.len();

        // Update L1 cache entries before the final DB write so `system` can be
        // moved into the last closure without an extra clone.
        // Build a HashMap for O(1) lookup instead of O(n*m) nested scan.
        let enrichment_map: HashMap<&str, &replay_control_core::metadata_db::BoxArtGenreRating> =
            enrichments
                .iter()
                .map(|e| (e.rom_filename.as_str(), e))
                .collect();

        if let Ok(mut guard) = self.roms.write()
            && let Some(entry) = guard.get_mut(&*system)
        {
            let roms = std::sync::Arc::make_mut(&mut entry.data);
            for rom in roms {
                if let Some(e) = enrichment_map.get(rom.game.rom_filename.as_str()) {
                    if e.box_art_url.is_some() {
                        rom.box_art_url = e.box_art_url.clone();
                    }
                    // RomEntry doesn't carry genre — L1 genre is
                    // served via lookup_genre() which reads game_library.
                    if let Some(r) = e.rating {
                        rom.rating = Some(r);
                    }
                    if rom.players.is_none() {
                        rom.players = e.players;
                    }
                }
            }
        }

        tracing::debug!(
            "L2 enrichment: {system} — {count} ROMs updated with box art/genre/players/ratings"
        );

        // Use targeted SQL update for box_art_url, genre, and rating.
        // This is the last use of `system` — move it into the closure.
        let enrichments_for_db = enrichments.clone();
        self.db
            .write(move |conn| {
                if let Err(e) =
                    MetadataDb::update_box_art_genre_rating(conn, &system, &enrichments_for_db)
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

        // Gather inputs: ROM filenames from L1 cache.
        let rom_filenames: Vec<String> = if let Ok(guard) = self.roms.read() {
            guard
                .get(system)
                .map(|entry| {
                    entry
                        .data
                        .iter()
                        .map(|r| r.game.rom_filename.clone())
                        .collect()
                })
                .unwrap_or_default()
        } else {
            return HashMap::new();
        };

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
