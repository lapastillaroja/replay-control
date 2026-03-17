use std::collections::{HashMap, HashSet};

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
            .metadata_db()
            .and_then(|guard| guard.as_ref()?.system_ratings(system).ok())
            .unwrap_or_default();

        // Load genres from game_metadata table (from LaunchBox import).
        // Used to fill empty game_library.genre entries.
        let lb_genres: HashMap<String, String> = state
            .metadata_db()
            .and_then(|guard| guard.as_ref()?.system_metadata_genres(system).ok())
            .unwrap_or_default();

        // Load player counts from game_metadata table (from LaunchBox import).
        // Used to fill empty game_library.players entries as a fallback.
        let lb_players: HashMap<String, u8> = state
            .metadata_db()
            .and_then(|guard| guard.as_ref()?.system_metadata_players(system).ok())
            .unwrap_or_default();

        // Load current game_library genres from L2 to know which are already set.
        let existing_genres: HashSet<String> = self
            .with_db_read(&storage, |db| {
                db.system_rom_genres(system)
                    .map(|map| map.into_keys().collect())
                    .unwrap_or_default()
            })
            .unwrap_or_default();

        // Load current game_library players from L2 to know which already have player data.
        let existing_players: HashSet<String> = self
            .with_db_read(&storage, |db| {
                db.system_rom_players(system).unwrap_or_default()
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

        // Build enrichment tuples: (filename, box_art_url, genre, players, rating).
        // Genre and players are only filled from LaunchBox when game_library has no value.
        let enrichments: Vec<(String, Option<String>, Option<String>, Option<u8>, Option<f32>)> = rom_filenames
            .iter()
            .filter_map(|filename| {
                let art = self.resolve_box_art(state, &index, system, filename);
                let rating = all_ratings.get(filename).map(|&r| r as f32);
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
                if art.is_none() && rating.is_none() && genre.is_none() && players.is_none() {
                    return None;
                }
                Some((filename.clone(), art, genre, players, rating))
            })
            .collect();

        if enrichments.is_empty() {
            return;
        }

        let count = enrichments.len();
        // Use targeted SQL update for box_art_url, genre, and rating.
        self.with_db_mut(&storage, |db| {
            if let Err(e) = db.update_box_art_genre_rating(system, &enrichments) {
                tracing::warn!("Enrichment failed for {system}: {e}");
            }
        });

        // Also update L1 cache entries.
        // Build a HashMap for O(1) lookup instead of O(n*m) nested scan.
        let enrichment_map: HashMap<&str, &(String, Option<String>, Option<String>, Option<u8>, Option<f32>)> =
            enrichments.iter().map(|e| (e.0.as_str(), e)).collect();

        if let Ok(mut guard) = self.roms.write()
            && let Some(entry) = guard.get_mut(system)
        {
            let roms = std::sync::Arc::make_mut(&mut entry.data);
            for rom in roms {
                if let Some((_, art, _genre, players, rating)) = enrichment_map.get(rom.game.rom_filename.as_str()) {
                    if art.is_some() {
                        rom.box_art_url = art.clone();
                    }
                    // RomEntry doesn't carry genre — L1 genre is
                    // served via lookup_genre() which reads game_library.
                    if let Some(r) = rating {
                        rom.rating = Some(*r);
                    }
                    if rom.players.is_none() {
                        rom.players = *players;
                    }
                }
            }
        }

        tracing::debug!("L2 enrichment: {system} — {count} ROMs updated with box art/genre/players/ratings");
    }

    /// Auto-match new ROMs against existing LaunchBox metadata by normalized title.
    ///
    /// For ROMs that have no `game_metadata` entry (not in `existing_ratings`),
    /// normalizes the ROM filename and looks for existing entries with the same
    /// normalized title. When a match is found, a new `game_metadata` row is
    /// created for the new ROM so future lookups hit directly.
    ///
    /// Returns a map of `rom_filename -> rating` for newly matched ROMs.
    fn auto_match_metadata(
        &self,
        state: &crate::api::AppState,
        system: &str,
    ) -> HashMap<String, f64> {
        use replay_control_core::launchbox::normalize_title;
        use replay_control_core::metadata_db::GameMetadata;
        use replay_control_core::systems;

        let storage = state.storage();
        let mut matched_ratings: HashMap<String, f64> = HashMap::new();

        // Load all existing metadata entries for this system.
        let all_metadata: Vec<(String, GameMetadata)> = state
            .metadata_db()
            .and_then(|guard| guard.as_ref()?.system_metadata_all(system).ok())
            .unwrap_or_default();

        // Nothing to match against if there's no imported metadata.
        if all_metadata.is_empty() {
            return matched_ratings;
        }

        let is_arcade = systems::is_arcade_system(system);

        // Build a normalized-title -> metadata map from existing entries.
        let mut title_index: HashMap<String, &GameMetadata> = HashMap::new();
        for (rom_filename, meta) in &all_metadata {
            let stem = rom_filename
                .rfind('.')
                .map(|i| &rom_filename[..i])
                .unwrap_or(rom_filename);
            let normalized = if is_arcade {
                replay_control_core::arcade_db::lookup_arcade_game(stem)
                    .map(|info| normalize_title(info.display_name))
                    .unwrap_or_else(|| normalize_title(stem))
            } else {
                normalize_title(stem)
            };
            title_index.entry(normalized).or_insert(meta);
        }

        // Collect filenames of ROMs that already have metadata (by exact match).
        let has_metadata: HashSet<&str> = all_metadata
            .iter()
            .map(|(filename, _)| filename.as_str())
            .collect();

        // Read current ROMs from L1 cache.
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
            return matched_ratings;
        };

        // Find unmatched ROMs and try normalized-title lookup.
        let mut new_entries: Vec<(String, String, GameMetadata)> = Vec::new();
        for rom_filename in &rom_filenames {
            // Skip ROMs that already have a game_metadata entry.
            if has_metadata.contains(rom_filename.as_str()) {
                continue;
            }

            let stem = rom_filename
                .rfind('.')
                .map(|i| &rom_filename[..i])
                .unwrap_or(rom_filename);

            let normalized = if is_arcade {
                replay_control_core::arcade_db::lookup_arcade_game(stem)
                    .map(|info| normalize_title(info.display_name))
                    .unwrap_or_else(|| normalize_title(stem))
            } else {
                normalize_title(stem)
            };

            if let Some(donor_meta) = title_index.get(&normalized) {
                if let Some(rating) = donor_meta.rating {
                    matched_ratings.insert(rom_filename.clone(), rating);
                }
                // Persist the match so future lookups are direct.
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;
                new_entries.push((
                    system.to_string(),
                    rom_filename.clone(),
                    GameMetadata {
                        description: donor_meta.description.clone(),
                        rating: donor_meta.rating,
                        publisher: donor_meta.publisher.clone(),
                        developer: donor_meta.developer.clone(),
                        genre: donor_meta.genre.clone(),
                        players: donor_meta.players,
                        release_year: donor_meta.release_year,
                        cooperative: donor_meta.cooperative,
                        source: "launchbox-auto".to_string(),
                        fetched_at: now,
                        box_art_path: None,
                        screenshot_path: None,
                    },
                ));
            }
        }

        // Persist new matches to game_metadata.
        if !new_entries.is_empty() {
            let count = new_entries.len();
            self.with_db_mut(&storage, |db| {
                if let Err(e) = db.bulk_upsert(&new_entries) {
                    tracing::warn!("Auto-match metadata persist failed for {system}: {e}");
                }
            });
            tracing::info!("Auto-matched {count} new ROM(s) to existing metadata for {system}");
        }

        matched_ratings
    }
}
