//! Pure logic for converting scanned ROM entries into enriched `GameEntry` records.
//!
//! Extracts genre, developer, year, region, clone status, and series key from
//! baked-in game databases and filename tags. No I/O — all inputs are passed in.

use std::collections::{HashMap, HashSet};

use crate::metadata_db::GameEntry;
use crate::rom_hash::HashResult;
use crate::roms::RomEntry;
use crate::{arcade_db, developer, game_db, genre, rom_tags, systems, title_utils};

/// Intermediate metadata extracted from game/arcade databases and filename tags.
///
/// Fields: (genre_detail, genre_group, players, is_clone, base_title, developer, release_year, cooperative)
type RomMetadata = (
    Option<String>,
    String,
    Option<u8>,
    bool,
    String,
    String,
    Option<u16>,
    bool,
);

/// Build `GameEntry` records from scanned ROM entries.
///
/// Enriches each ROM with genre, developer, year, region, clone status, and
/// series key using the baked-in game/arcade databases and filename tags.
/// Also applies TOSEC clone inference and display name disambiguation.
pub fn build_game_entries(
    system: &str,
    roms: &[RomEntry],
    hash_results: &HashMap<String, HashResult>,
) -> Vec<GameEntry> {
    let is_arcade = systems::is_arcade_system(system);

    let mut entries: Vec<GameEntry> = roms
        .iter()
        .filter_map(|r| build_single_entry(system, r, hash_results, is_arcade))
        .collect();

    // Phase 2: Infer is_clone for TOSEC bracket-tagged entries.
    // For non-arcade systems, entries with TOSEC bracket flags ([a], [t], [cr], etc.)
    // are marked as clones when a clean sibling (same base_title, no bracket flags) exists.
    if !is_arcade {
        infer_tosec_clones(&mut entries);
    }

    // Phase 3: Disambiguate display names for non-clone entries that share
    // the same display name. Appends year, publisher, date, or bracket
    // descriptors to make entries distinguishable.
    disambiguate_display_names(&mut entries);

    entries
}

/// Build a single `GameEntry` from a `RomEntry`.
fn build_single_entry(
    system: &str,
    r: &RomEntry,
    hash_results: &HashMap<String, HashResult>,
    is_arcade: bool,
) -> Option<GameEntry> {
    let rom_filename = &r.game.rom_filename;
    let stem = rom_filename
        .rfind('.')
        .map(|i| &rom_filename[..i])
        .unwrap_or(rom_filename);

    // Two-tier genre: `genre` = detail/original, `genre_group` = normalized.
    // Also extract developer (manufacturer for arcade, empty for console — enriched later).
    // release_year comes from game_db (baked-in) or TOSEC tags (fallback).
    let (
        genre_detail,
        genre_group,
        players_lookup,
        is_clone,
        base_title,
        dev,
        release_year,
        cooperative,
    ) = if is_arcade {
        build_arcade_metadata(rom_filename, stem)
    } else {
        build_console_metadata(system, r, rom_filename, stem, hash_results)
    }?;

    // Extract TOSEC structured metadata (year, publisher) from filename tags.
    // Used as fallback when baked-in DBs don't provide the data.
    let tosec = rom_tags::extract_tosec_metadata(rom_filename);
    let release_year = release_year.or(tosec.year);
    let developer_name = if dev.is_empty() {
        tosec
            .publisher
            .as_deref()
            .map(developer::normalize_developer)
            .unwrap_or_default()
    } else {
        dev
    };

    let (tier, region_priority, is_special) = rom_tags::classify(rom_filename);
    let is_translation = tier == rom_tags::RomTier::Translation;
    let is_hack = tier == rom_tags::RomTier::Hack;
    let region = match region_priority {
        rom_tags::RegionPriority::Usa => "usa",
        rom_tags::RegionPriority::Europe => "europe",
        rom_tags::RegionPriority::Japan => "japan",
        rom_tags::RegionPriority::World => "world",
        rom_tags::RegionPriority::Other => "other",
        rom_tags::RegionPriority::Unknown => {
            // Fallback: check for TOSEC lowercase language codes
            // to populate region for game detail page variants.
            rom_tags::extract_tosec_language_as_region(rom_filename).unwrap_or("")
        }
    };

    // Look up hash result for this ROM file.
    let hash = hash_results.get(rom_filename);

    // Override display_name with the No-Intro canonical name from CRC hash matching.
    // More authoritative than filename-derived names (e.g., "Dongguri Techi Jakjeon (Korea)"
    // instead of "Dong Gu Ri Te Chi Jak Jeon (Korea)"). Excluded for translations/hacks/specials
    // whose filename-derived names carry useful tags (e.g., "PT-BR Translation").
    let display_name = if !is_translation
        && !is_hack
        && !is_special
        && let Some(matched) = hash.and_then(|h| h.matched_name.as_deref())
    {
        Some(matched.to_string())
    } else {
        r.game.display_name.clone()
    };

    // Compute series_key from base_title for franchise grouping.
    let series_key = title_utils::series_key(&base_title);

    Some(GameEntry {
        system: r.game.system.clone(),
        rom_filename: rom_filename.clone(),
        rom_path: r.game.rom_path.clone(),
        display_name,
        size_bytes: r.size_bytes,
        is_m3u: r.is_m3u,
        box_art_url: r.box_art_url.clone(),
        driver_status: r.driver_status.clone(),
        genre: genre_detail,
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
        developer: developer_name,
        release_date: release_year.map(|y| format!("{y:04}")),
        release_precision: release_year.map(|_| crate::metadata_db::DatePrecision::Year),
        release_region_used: None,
        cooperative,
    })
}

/// Extract metadata from arcade databases. Returns `None` for BIOS entries.
fn build_arcade_metadata(rom_filename: &str, stem: &str) -> Option<RomMetadata> {
    let arcade_stem = rom_filename.strip_suffix(".zip").unwrap_or(rom_filename);
    match arcade_db::lookup_arcade_game(arcade_stem) {
        Some(info) => {
            // Skip BIOS entries — they're not playable games
            if info.is_bios {
                return None;
            }
            let detail = if info.category.is_empty() {
                None
            } else {
                Some(info.category.to_string())
            };
            let group = genre::normalize_genre(info.category).to_string();
            let dev = developer::normalize_developer(info.manufacturer);
            let year: Option<u16> = info.year.parse::<u16>().ok().filter(|&y| y > 0);
            Some((
                detail,
                group,
                Some(info.players),
                info.is_clone,
                title_utils::base_title(info.display_name),
                dev,
                year,
                false,
            ))
        }
        None => Some((
            None,
            String::new(),
            None,
            false,
            title_utils::base_title(stem),
            String::new(),
            None,
            false,
        )),
    }
}

/// Extract metadata from console game databases.
/// Always returns `Some` (console ROMs are never filtered out).
fn build_console_metadata(
    system: &str,
    r: &RomEntry,
    rom_filename: &str,
    stem: &str,
    hash_results: &HashMap<String, HashResult>,
) -> Option<RomMetadata> {
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
        .map(title_utils::base_title)
        .unwrap_or_else(|| title_utils::base_title(stem));
    match game {
        Some(g) => {
            let detail = if g.genre.is_empty() {
                None
            } else {
                Some(g.genre.to_string())
            };
            let group = genre::normalize_genre(g.genre).to_string();
            let year: Option<u16> = if g.year > 0 { Some(g.year) } else { None };
            let cooperative = g.coop.unwrap_or(false);
            Some((
                detail,
                group,
                if g.players > 0 { Some(g.players) } else { None },
                false,
                bt,
                String::new(),
                year,
                cooperative,
            ))
        }
        None => Some((
            None,
            String::new(),
            None,
            false,
            bt,
            String::new(),
            None,
            false,
        )),
    }
}

/// Infer clone status for TOSEC bracket-tagged entries.
///
/// Entries with TOSEC bracket flags ([a], [t], [cr], etc.) are marked as clones
/// when a clean sibling (same base_title, no bracket flags) exists.
fn infer_tosec_clones(entries: &mut [GameEntry]) {
    let clean_base_titles: HashSet<String> = entries
        .iter()
        .filter(|e| !rom_tags::has_tosec_bracket_flag(&e.rom_filename))
        .map(|e| e.base_title.clone())
        .collect();

    for entry in entries.iter_mut() {
        if !entry.is_clone
            && rom_tags::has_tosec_bracket_flag(&entry.rom_filename)
            && clean_base_titles.contains(&entry.base_title)
        {
            entry.is_clone = true;
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
fn disambiguate_display_names(entries: &mut [GameEntry]) {
    // Group ALL entries by display_name to find duplicates.
    let mut display_groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, entry) in entries.iter().enumerate() {
        let display = entry.display_name.as_deref().unwrap_or(&entry.rom_filename);
        display_groups
            .entry(display.to_string())
            .or_default()
            .push(i);
    }

    // For each group with duplicates, compute disambiguation suffixes.
    for indices in display_groups.values() {
        if indices.len() <= 1 {
            continue;
        }

        // Extract TOSEC metadata for each entry in the group.
        let metadata: Vec<(rom_tags::TosecMetadata, Vec<String>)> = indices
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
        let dates: HashSet<Option<&str>> =
            metadata.iter().map(|(m, _)| m.date.as_deref()).collect();
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
            if has_different_publishers && let Some(ref publisher) = tosec.publisher {
                // Use the already-normalized developer from the entry, or fall back to raw publisher.
                let dev = &entries[idx].developer;
                if !dev.is_empty() {
                    suffix_parts.push(dev.clone());
                } else {
                    suffix_parts.push(publisher.clone());
                }
            }

            // Priority 2/3: Date-based disambiguation.
            if has_different_dates && let Some(ref date) = tosec.date {
                if use_full_dates {
                    suffix_parts.push(date.clone());
                } else if let Some(year) = tosec.year {
                    suffix_parts.push(year.to_string());
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
                // E.g., "Game (USA)" -> "Game (USA, 2017)" not "Game (USA) (2017)".
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

    // Second pass: append file format suffix for entries that still share
    // a display name but differ by file extension.
    disambiguate_by_format(entries);
}

/// Append file format suffix (e.g., "[DSK]", "[CDT]") to entries that still
/// share a display name after all other disambiguation, when they differ by
/// file extension.
fn disambiguate_by_format(entries: &mut [GameEntry]) {
    // Re-group by display_name
    let mut display_groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, entry) in entries.iter().enumerate() {
        let display = entry.display_name.as_deref().unwrap_or(&entry.rom_filename);
        display_groups
            .entry(display.to_string())
            .or_default()
            .push(i);
    }

    for indices in display_groups.values() {
        if indices.len() <= 1 {
            continue;
        }

        // Check if entries differ by file extension
        let extensions: Vec<String> = indices
            .iter()
            .map(|&i| {
                entries[i]
                    .rom_filename
                    .rsplit('.')
                    .next()
                    .unwrap_or("")
                    .to_uppercase()
            })
            .collect();

        // Only add format suffix if there are at least 2 different extensions
        let unique_exts: HashSet<&str> = extensions.iter().map(|s| s.as_str()).collect();
        if unique_exts.len() <= 1 {
            continue;
        }

        // Append format suffix to each entry
        for (j, &idx) in indices.iter().enumerate() {
            let ext = &extensions[j];
            if ext.is_empty() {
                continue;
            }
            let current = entries[idx]
                .display_name
                .as_deref()
                .unwrap_or(&entries[idx].rom_filename)
                .to_string();
            entries[idx].display_name = Some(format!("{current} [{ext}]"));
        }
    }
}
