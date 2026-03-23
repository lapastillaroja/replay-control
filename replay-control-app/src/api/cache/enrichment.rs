use std::collections::{HashMap, HashSet};

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
    pub fn enrich_system_cache(&self, state: &crate::api::AppState, system: &str) {
        let storage = state.storage();
        let index = self.get_image_index(state, system);

        // Load ratings from game_metadata table (from LaunchBox import).
        let ratings: HashMap<String, f64> = state
            .metadata_pool
            .read(|conn| MetadataDb::system_ratings(conn, system).ok())
            .flatten()
            .unwrap_or_default();

        // Load genres from game_metadata table (from LaunchBox import).
        // Used to fill empty game_library.genre entries.
        let lb_genres: HashMap<String, String> = state
            .metadata_pool
            .read(|conn| MetadataDb::system_metadata_genres(conn, system).ok())
            .flatten()
            .unwrap_or_default();

        // Load player counts from game_metadata table (from LaunchBox import).
        // Used to fill empty game_library.players entries as a fallback.
        let lb_players: HashMap<String, u8> = state
            .metadata_pool
            .read(|conn| MetadataDb::system_metadata_players(conn, system).ok())
            .flatten()
            .unwrap_or_default();

        // Load rating counts from game_metadata table (from LaunchBox import).
        // Used to propagate vote counts to game_library for weighted scoring.
        let lb_rating_counts: HashMap<String, u32> = state
            .metadata_pool
            .read(|conn| MetadataDb::system_metadata_rating_counts(conn, system).ok())
            .flatten()
            .unwrap_or_default();

        // Load developers from game_metadata table (from LaunchBox import).
        // Used to fill empty game_library.developer entries as a fallback.
        let lb_developers: HashMap<String, String> = state
            .metadata_pool
            .read(|conn| MetadataDb::system_metadata_developers(conn, system).ok())
            .flatten()
            .unwrap_or_default();

        // Load current game_library genres from L2 to know which are already set.
        let existing_genres: HashSet<String> = self
            .with_db_read(&storage, |conn| {
                MetadataDb::system_rom_genres(conn, system)
                    .map(|map| map.into_keys().collect())
                    .unwrap_or_default()
            })
            .unwrap_or_default();

        // Load current game_library players from L2 to know which already have player data.
        let existing_players: HashSet<String> = self
            .with_db_read(&storage, |conn| {
                MetadataDb::system_rom_players(conn, system).unwrap_or_default()
            })
            .unwrap_or_default();

        // Load current game_library developers from L2 to know which already have developer data.
        let existing_developers: HashSet<String> = self
            .with_db_read(&storage, |conn| {
                MetadataDb::system_rom_developers(conn, system).unwrap_or_default()
            })
            .unwrap_or_default();

        // Auto-match new ROMs: build a normalized-title index from existing
        // game_metadata entries so ROMs added after the last import can inherit
        // metadata from entries that share the same normalized title.
        let auto_matched_ratings = self.auto_match_metadata(state, system);

        // Merge auto-matched ratings into the main ratings map.
        let mut all_ratings = ratings;
        for (filename, rating) in &auto_matched_ratings {
            all_ratings.entry(filename.clone()).or_insert(*rating);
        }

        // Read current ROMs from L1 cache to get filenames.
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
                let art = self.resolve_box_art(state, &index, system, filename);
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
                    let normalized =
                        replay_control_core::developer::normalize_developer(dev);
                    (f.clone(), normalized)
                })
            })
            .filter(|(_, dev)| !dev.is_empty())
            .collect();

        if !developer_updates.is_empty() {
            let dev_count = developer_updates.len();
            self.with_db_mut(&storage, |conn| {
                if let Err(e) = MetadataDb::update_developers(conn, system, &developer_updates) {
                    tracing::warn!("Developer enrichment failed for {system}: {e}");
                }
            });
            tracing::debug!(
                "L2 enrichment: {system} — {dev_count} ROMs updated with developer"
            );
        }

        if enrichments.is_empty() {
            return;
        }

        let count = enrichments.len();
        // Use targeted SQL update for box_art_url, genre, and rating.
        self.with_db_mut(&storage, |conn| {
            if let Err(e) = MetadataDb::update_box_art_genre_rating(conn, system, &enrichments) {
                tracing::warn!("Enrichment failed for {system}: {e}");
            }
        });

        // Also update L1 cache entries.
        // Build a HashMap for O(1) lookup instead of O(n*m) nested scan.
        let enrichment_map: HashMap<&str, &replay_control_core::metadata_db::BoxArtGenreRating> =
            enrichments
                .iter()
                .map(|e| (e.rom_filename.as_str(), e))
                .collect();

        if let Ok(mut guard) = self.roms.write()
            && let Some(entry) = guard.get_mut(system)
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
    }

    /// Auto-match new ROMs against existing LaunchBox metadata by normalized title.
    ///
    /// Delegates the pure matching logic to `replay_control_core::metadata_matching`,
    /// then persists results and returns a map of `rom_filename -> rating`.
    fn auto_match_metadata(
        &self,
        state: &crate::api::AppState,
        system: &str,
    ) -> HashMap<String, f64> {
        use replay_control_core::metadata_matching;

        let storage = state.storage();

        // Gather inputs: existing metadata from DB.
        let all_metadata = state
            .metadata_pool
            .read(|conn| MetadataDb::system_metadata_all(conn, system).ok())
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
        self.with_db_mut(&storage, |conn| {
            if let Err(e) = MetadataDb::bulk_upsert(conn, &new_entries) {
                tracing::warn!("Auto-match metadata persist failed for {system}: {e}");
            }
        });
        tracing::info!("Auto-matched {count} new ROM(s) to existing metadata for {system}");

        matched_ratings
    }
}
