use std::collections::HashMap;
use std::path::Path;

use replay_control_core::roms::RomEntry;
use replay_control_core::storage::StorageLocation;

use replay_control_core::metadata_db::MetadataDb;

use super::{GameLibrary, dir_mtime};

impl GameLibrary {
    /// Hash ROM files for a hash-eligible system and apply identification results.
    ///
    /// For eligible systems (cartridge-based with No-Intro CRC data), this:
    /// 1. Loads cached hashes from the database
    /// 2. Computes CRC32 for new/modified files
    /// 3. Looks up CRC32 in the No-Intro index
    /// 4. Overrides display names for matched ROMs (via `GameRef::new()` with the
    ///    canonical No-Intro name)
    ///
    /// Returns a map of rom_filename -> HashResult for use by save_roms_to_db.
    pub(super) async fn hash_roms_for_system(
        &self,
        storage: &StorageLocation,
        system: &str,
        roms: &mut [RomEntry],
    ) -> HashMap<String, replay_control_core::rom_hash::HashResult> {
        use replay_control_core::rom_hash::{self, HashResult};

        if !rom_hash::is_hash_eligible(system) {
            return HashMap::new();
        }

        // Load cached hashes from L2 (database).
        let system_owned = system.to_string();
        let cached_hashes = self
            .db
            .read(move |conn| MetadataDb::load_cached_hashes(conn, &system_owned))
            .await
            .and_then(|r| r.ok())
            .unwrap_or_default();

        // Build input list: (rom_filename, rom_path, size_bytes).
        let rom_files: Vec<(String, String, u64)> = roms
            .iter()
            .filter(|r| !r.is_m3u) // Skip M3U playlists
            .map(|r| {
                (
                    r.game.rom_filename.clone(),
                    r.game.rom_path.clone(),
                    r.size_bytes,
                )
            })
            .collect();

        let results =
            rom_hash::hash_and_identify(system, &rom_files, &cached_hashes, &storage.root);

        // Build a lookup map for applying results.
        let mut result_map: HashMap<String, HashResult> = HashMap::new();
        for result in results {
            result_map.insert(result.rom_filename.clone(), result);
        }

        // Apply hash-matched display names to RomEntries.
        // When a CRC match gives us a canonical No-Intro name (e.g.,
        // "Super Mario World (USA)"), re-resolve the display name through
        // GameRef::new() using that canonical name as the filename stem.
        // This gives us the proper display name with tags.
        for rom in roms.iter_mut() {
            if let Some(hash_result) = result_map.get(&rom.game.rom_filename)
                && let Some(ref matched_name) = hash_result.matched_name
            {
                // The matched_name is the No-Intro canonical filename stem
                // (e.g., "Super Mario World (USA)"). Use game_display_name()
                // to get the clean display title, then apply tags from the
                // original filename.
                let canonical_filename = format!("{matched_name}.rom");
                if let Some(display) =
                    replay_control_core::game_db::game_display_name(system, &canonical_filename)
                {
                    let with_tags = replay_control_core::rom_tags::display_name_with_tags(
                        display,
                        &rom.game.rom_filename,
                    );
                    rom.game.display_name = Some(with_tags);
                }
            }
        }

        if !result_map.is_empty() {
            let matched = result_map
                .values()
                .filter(|r| r.matched_name.is_some())
                .count();
            tracing::debug!(
                "Hash-and-identify for {system}: {} hashed, {} matched No-Intro",
                result_map.len(),
                matched
            );
        }

        result_map
    }

    /// Write ROM list to SQLite game_library for persistent storage.
    /// Enriches with genre/players from the baked-in game databases during write.
    pub(super) async fn save_roms_to_db(
        &self,
        _storage: &StorageLocation,
        system: &str,
        roms: &[RomEntry],
        system_dir: &Path,
        hash_results: &HashMap<String, replay_control_core::rom_hash::HashResult>,
    ) {
        use replay_control_core::metadata_db::GameEntry;
        use replay_control_core::systems;
        use replay_control_core::{arcade_db, game_db};

        let mtime_secs = dir_mtime(system_dir).and_then(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|d| d.as_secs() as i64)
        });

        let is_arcade = systems::is_arcade_system(system);

        let cached_roms: Vec<GameEntry> = roms
            .iter()
            .filter_map(|r| {
                let rom_filename = &r.game.rom_filename;
                let stem = rom_filename
                    .rfind('.')
                    .map(|i| &rom_filename[..i])
                    .unwrap_or(rom_filename);

                // Two-tier genre: `genre` = detail/original, `genre_group` = normalized.
                // Also extract developer (manufacturer for arcade, empty for console — enriched later).
                // release_year comes from game_db (baked-in) or TOSEC tags (fallback).
                let (
                    genre,
                    genre_group,
                    players_lookup,
                    is_clone,
                    base_title,
                    developer,
                    release_year,
                ) = if is_arcade {
                    let arcade_stem = rom_filename.strip_suffix(".zip").unwrap_or(rom_filename);
                    match arcade_db::lookup_arcade_game(arcade_stem) {
                        Some(info) => {
                            // Skip BIOS entries — they're not playable games
                            if info.is_bios {
                                return None;
                            }
                            // genre = raw category (e.g., "Maze / Shooter")
                            let detail = if info.category.is_empty() {
                                None
                            } else {
                                Some(info.category.to_string())
                            };
                            // genre_group = normalized (e.g., "Maze")
                            let group = replay_control_core::genre::normalize_genre(info.category)
                                .to_string();
                            let dev = replay_control_core::developer::normalize_developer(
                                info.manufacturer,
                            );
                            let year: Option<u16> =
                                info.year.parse::<u16>().ok().filter(|&y| y > 0);
                            (
                                detail,
                                group,
                                Some(info.players),
                                info.is_clone,
                                replay_control_core::title_utils::base_title(info.display_name),
                                dev,
                                year,
                            )
                        }
                        None => (
                            None,
                            String::new(),
                            None,
                            false,
                            replay_control_core::title_utils::base_title(stem),
                            String::new(),
                            None,
                        ),
                    }
                } else {
                    // Try CRC-based lookup first (if we have a hash match),
                    // then fall back to filename-based lookup.
                    let hash_entry = hash_results
                        .get(rom_filename)
                        .and_then(|hr| hr.matched_name.as_ref())
                        .and_then(|name| game_db::lookup_game(system, name));
                    let entry = hash_entry.or_else(|| game_db::lookup_game(system, stem));
                    let game = entry.map(|e| e.game).or_else(|| {
                        let normalized = game_db::normalize_filename(stem);
                        game_db::lookup_by_normalized_title(system, &normalized)
                    });
                    let bt = r
                        .game
                        .display_name
                        .as_deref()
                        .map(replay_control_core::title_utils::base_title)
                        .unwrap_or_else(|| replay_control_core::title_utils::base_title(stem));
                    match game {
                        Some(g) => {
                            // genre = raw genre from game_db (e.g., "Shoot'em Up")
                            let detail = if g.genre.is_empty() {
                                None
                            } else {
                                Some(g.genre.to_string())
                            };
                            // genre_group = normalized (e.g., "Shooter")
                            let group =
                                replay_control_core::genre::normalize_genre(g.genre).to_string();
                            let year: Option<u16> = if g.year > 0 { Some(g.year) } else { None };
                            (
                                detail,
                                group,
                                if g.players > 0 { Some(g.players) } else { None },
                                false,
                                bt,
                                String::new(),
                                year,
                            )
                        }
                        None => (None, String::new(), None, false, bt, String::new(), None),
                    }
                };

                // Extract TOSEC structured metadata (year, publisher) from filename tags.
                // Used as fallback when baked-in DBs don't provide the data.
                let tosec = replay_control_core::rom_tags::extract_tosec_metadata(rom_filename);
                let release_year = release_year.or(tosec.year);
                let developer = if developer.is_empty() {
                    tosec
                        .publisher
                        .as_deref()
                        .map(replay_control_core::developer::normalize_developer)
                        .unwrap_or_default()
                } else {
                    developer
                };

                let (tier, region_priority, is_special) =
                    replay_control_core::rom_tags::classify(rom_filename);
                let is_translation = tier == replay_control_core::rom_tags::RomTier::Translation;
                let is_hack = tier == replay_control_core::rom_tags::RomTier::Hack;
                let region = match region_priority {
                    replay_control_core::rom_tags::RegionPriority::Usa => "usa",
                    replay_control_core::rom_tags::RegionPriority::Europe => "europe",
                    replay_control_core::rom_tags::RegionPriority::Japan => "japan",
                    replay_control_core::rom_tags::RegionPriority::World => "world",
                    replay_control_core::rom_tags::RegionPriority::Other => "other",
                    replay_control_core::rom_tags::RegionPriority::Unknown => "",
                };

                // Look up hash result for this ROM file.
                let hash = hash_results.get(rom_filename);

                // Compute series_key from base_title for franchise grouping.
                let series_key = replay_control_core::title_utils::series_key(&base_title);

                Some(GameEntry {
                    system: r.game.system.clone(),
                    rom_filename: rom_filename.clone(),
                    rom_path: r.game.rom_path.clone(),
                    display_name: r.game.display_name.clone(),
                    size_bytes: r.size_bytes,
                    is_m3u: r.is_m3u,
                    box_art_url: r.box_art_url.clone(),
                    driver_status: r.driver_status.clone(),
                    genre,
                    genre_group,
                    players: players_lookup.or(r.players),
                    rating: r.rating,
                    rating_count: None,
                    is_clone,
                    base_title,
                    region: region.to_string(),
                    is_translation,
                    is_hack,
                    is_special,
                    crc32: hash.map(|h| h.crc32),
                    hash_mtime: hash.map(|h| h.mtime_secs),
                    hash_matched_name: hash.and_then(|h| h.matched_name.clone()),
                    series_key,
                    developer,
                    release_year,
                })
            })
            .collect();

        tracing::debug!(
            "L2 write-through: saving {} ROMs for {system} (mtime={mtime_secs:?})",
            cached_roms.len()
        );
        let system_owned = system.to_string();
        let cached_roms_for_db = cached_roms.clone();
        let result = self
            .db
            .write(move |conn| {
                MetadataDb::save_system_entries(
                    conn,
                    &system_owned,
                    &cached_roms_for_db,
                    mtime_secs,
                )
            })
            .await;
        match result {
            Some(Ok(())) => {
                tracing::debug!("L2 write-through: {system} OK ({} ROMs)", cached_roms.len());

                // Populate TGDB aliases from embedded build-time data.
                self.populate_tgdb_aliases(system, &cached_roms).await;

                // Populate game_series from embedded Wikidata data.
                self.populate_wikidata_series(system, &cached_roms).await;
            }
            Some(Err(e)) => tracing::warn!("L2 write-through: {system} FAILED: {e}"),
            None => tracing::warn!("L2 write-through: {system} skipped (DB unavailable)"),
        }
    }
}
