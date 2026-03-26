use std::collections::{HashMap, HashSet};
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

        let mut cached_roms: Vec<GameEntry> = roms
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

        // Phase 2: Infer is_clone for TOSEC bracket-tagged entries.
        // For non-arcade systems, entries with TOSEC bracket flags ([a], [t], [cr], etc.)
        // are marked as clones when a clean sibling (same base_title, no bracket flags) exists.
        if !is_arcade {
            // Collect base_titles that have at least one clean (non-bracket-flagged) entry.
            let clean_base_titles: HashSet<String> = cached_roms
                .iter()
                .filter(|e| !replay_control_core::rom_tags::has_tosec_bracket_flag(&e.rom_filename))
                .map(|e| e.base_title.clone())
                .collect();

            // Mark bracket-flagged entries as clones if a clean sibling exists.
            for entry in &mut cached_roms {
                if !entry.is_clone
                    && replay_control_core::rom_tags::has_tosec_bracket_flag(&entry.rom_filename)
                    && clean_base_titles.contains(&entry.base_title)
                {
                    entry.is_clone = true;
                }
            }
        }

        // Phase 3: Disambiguate display names for non-clone entries that share
        // the same display name. Appends year, publisher, date, or bracket
        // descriptors to make entries distinguishable.
        disambiguate_display_names(&mut cached_roms);

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

/// Disambiguate display names for entries that share the same display name.
///
/// After clone inference, remaining non-clone entries may still have identical
/// display names (e.g., multiple clean builds of "Cross Chase" with different dates,
/// or "Barbarian" from different publishers). This function appends distinguishing
/// suffixes to make them unique.
///
/// Disambiguation priority:
/// 1. Publisher (if different publishers exist in the group)
/// 2. Year (if different years exist)
/// 3. Full date (if same year but different dates)
/// 4. Bracket descriptors (game-specific tags like [joystick], [experimental])
///
/// Only non-clone entries are disambiguated. Clone entries keep their original
/// display name since they're typically hidden by the clone filter.
fn disambiguate_display_names(entries: &mut [replay_control_core::metadata_db::GameEntry]) {
    use replay_control_core::rom_tags;

    // Group non-clone entries by display_name to find duplicates.
    // We need indices because we'll mutate the entries.
    let mut display_groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, entry) in entries.iter().enumerate() {
        if entry.is_clone {
            continue;
        }
        let display = entry
            .display_name
            .as_deref()
            .unwrap_or(&entry.rom_filename);
        display_groups
            .entry(display.to_string())
            .or_default()
            .push(i);
    }

    // For each group with duplicates, compute disambiguation suffixes.
    for (_display, indices) in &display_groups {
        if indices.len() <= 1 {
            continue;
        }

        // Extract TOSEC metadata for each entry in the group.
        let metadata: Vec<(
            rom_tags::TosecMetadata,
            Vec<String>,
        )> = indices
            .iter()
            .map(|&i| {
                let tosec = rom_tags::extract_tosec_metadata(&entries[i].rom_filename);
                let descriptors = rom_tags::extract_bracket_descriptors(&entries[i].rom_filename);
                (tosec, descriptors)
            })
            .collect();

        // Determine which fields vary across the group.
        let publishers: HashSet<Option<&str>> = metadata
            .iter()
            .map(|(m, _)| m.publisher.as_deref())
            .collect();
        let dates: HashSet<Option<&str>> = metadata.iter().map(|(m, _)| m.date.as_deref()).collect();
        let has_different_publishers = publishers.len() > 1;
        let has_different_dates = dates.len() > 1;

        // Check if showing just the year would cause collisions within this group.
        // If any two entries share the same year, we need full dates.
        let use_full_dates = if has_different_dates {
            let mut year_counts: HashMap<Option<u16>, usize> = HashMap::new();
            for (m, _) in &metadata {
                *year_counts.entry(m.year).or_insert(0) += 1;
            }
            year_counts.values().any(|&c| c > 1)
        } else {
            false
        };

        // Build suffix for each entry.
        for (j, &idx) in indices.iter().enumerate() {
            let (tosec, descriptors) = &metadata[j];
            let mut suffix_parts: Vec<String> = Vec::new();

            // Priority 1: Publisher
            if has_different_publishers {
                if let Some(ref publisher) = tosec.publisher {
                    // Use the already-normalized developer from the entry, or fall back to raw publisher.
                    let dev = &entries[idx].developer;
                    if !dev.is_empty() {
                        suffix_parts.push(dev.clone());
                    } else {
                        suffix_parts.push(publisher.clone());
                    }
                }
            }

            // Priority 2/3: Date-based disambiguation.
            // Use the most specific date that disambiguates:
            // - If all entries have different years, just show the year
            // - If some entries share a year (but differ by full date), show the full date
            if has_different_dates {
                if let Some(ref date) = tosec.date {
                    if use_full_dates {
                        suffix_parts.push(date.clone());
                    } else if let Some(year) = tosec.year {
                        suffix_parts.push(year.to_string());
                    }
                }
            }

            // Priority 4: Bracket descriptors
            if !descriptors.is_empty() {
                suffix_parts.extend(descriptors.iter().cloned());
            }

            // Apply suffix if we have something to add.
            if !suffix_parts.is_empty() {
                let suffix = suffix_parts.join(", ");
                let current = entries[idx]
                    .display_name
                    .as_deref()
                    .unwrap_or(&entries[idx].rom_filename);

                // Check if the current display name already has a parenthesized suffix.
                // If so, insert the disambiguation info before the closing paren.
                // E.g., "Game (USA)" → "Game (USA, 2017)" not "Game (USA) (2017)".
                let new_display = if let Some(paren_start) = current.rfind('(')
                    && current.ends_with(')')
                {
                    let before = &current[..paren_start];
                    let existing = &current[paren_start + 1..current.len() - 1];
                    format!("{before}({existing}, {suffix})")
                } else {
                    format!("{current} ({suffix})")
                };
                entries[idx].display_name = Some(new_display);
            }
        }
    }
}
