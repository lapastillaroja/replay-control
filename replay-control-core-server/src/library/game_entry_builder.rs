//! Pure logic for converting scanned ROM entries into enriched `GameEntry` records.
//!
//! Extracts genre, developer, year, region, clone status, and series key from
//! baked-in game databases and filename tags. No I/O — all inputs are passed in.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::arcade_db::ArcadeGameInfo;
use crate::game_db::{CanonicalGame, GameEntry as CatalogGameEntry};
use crate::library_db::{GameEntry, IdentityState};
use crate::rc_hash_disc;
use crate::rom_hash::HashResult;
use crate::roms::RomEntry;
use crate::{arcade_db, game_db};
use replay_control_core::arcade_board::ArcadeBoard;
use replay_control_core::{developer, game_ref, genre, rom_tags, systems, title_utils};

/// Pre-fetched catalog lookups for a system, keyed by filename stem.
#[derive(Default)]
struct CatalogLookup {
    arcade: HashMap<String, ArcadeGameInfo>,
    by_stem: HashMap<String, CatalogGameEntry>,
    by_normalized: HashMap<String, CanonicalGame>,
}

/// Intermediate metadata extracted from game/arcade databases and filename tags.
struct RomMetadata {
    genre_detail: Option<String>,
    genre_group: String,
    players: Option<u8>,
    is_clone: bool,
    base_title: String,
    developer: String,
    release_year: Option<u16>,
    cooperative: bool,
    board: Option<ArcadeBoard>,
    ra_id: String,
    is_mature: bool,
}

/// Build `GameEntry` records from scanned ROM entries.
///
/// Enriches each ROM with genre, developer, year, region, clone status, and
/// series key using the baked-in game/arcade databases and filename tags.
/// Also applies TOSEC clone inference and display name disambiguation.
pub async fn build_game_entries(
    system: &str,
    roms: &[RomEntry],
    hash_results: &HashMap<String, HashResult>,
) -> Vec<GameEntry> {
    let is_arcade = systems::is_arcade_system(system);

    let batch = prefetch_catalog(system, roms, hash_results, is_arcade).await;

    let mut entries: Vec<GameEntry> = roms
        .iter()
        .filter_map(|r| build_single_entry(r, hash_results, is_arcade, &batch))
        .collect();

    // Populate normalized titles for the enrichment matcher. Done here (not
    // per-row in `build_single_entry`) so we can resolve arcade clone
    // parents in a single pass over the assembled entries.
    populate_normalized_titles(system, &mut entries, &batch.arcade);

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
    r: &RomEntry,
    hash_results: &HashMap<String, HashResult>,
    is_arcade: bool,
    batch: &CatalogLookup,
) -> Option<GameEntry> {
    let rom_filename = &r.game.rom_filename;
    let stem = title_utils::filename_stem(rom_filename);

    // Two-tier genre: `genre` = detail/original, `genre_group` = normalized.
    // Also extract developer (manufacturer for arcade, empty for console — enriched later).
    // release_year comes from game_db (baked-in) or TOSEC tags (fallback).
    let RomMetadata {
        genre_detail,
        genre_group,
        players: players_lookup,
        is_clone,
        base_title,
        developer: dev,
        release_year,
        cooperative,
        board,
        ra_id,
        is_mature,
    } = if is_arcade {
        build_arcade_metadata(stem, batch)
    } else {
        build_console_metadata(r, rom_filename, stem, hash_results, batch)
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

    // Override display_name using CRC hash matching when available.
    // The matched name is re.filename_stem from the catalog — for No-Intro that
    // IS human-readable, but for TOSEC it carries year/publisher tags (e.g.
    // "Nitro (1990)(Psygnosis)(US)"). Always resolve through batch.by_stem so
    // the canonical_game.display_name is used (e.g. "Nitro (US)"), then re-apply
    // tags from the visible filename. Excluded for translations/hacks/specials
    // whose filename-derived names carry useful tags.
    let display_name = if !is_arcade
        && !is_translation
        && !is_hack
        && !is_special
        && let Some(matched) = hash.and_then(|h| h.matched_name.as_deref())
    {
        let resolved_base = batch
            .by_stem
            .get(matched)
            .map(|e| e.game.display_name.as_str());
        Some(game_ref::console_display_name(resolved_base, rom_filename))
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
        hash_size_bytes: hash.map(|h| h.size_bytes),
        hash_matched_name: hash.and_then(|h| h.matched_name.clone()),
        identity_state: if !is_identity_applicable(&r.game.system, rom_filename) {
            IdentityState::NotApplicable
        } else if let Some(hash) = hash {
            identity_state_from_hash(hash)
        } else {
            IdentityState::Pending
        },
        series_key,
        developer: developer_name,
        release_date: release_year.map(|y| format!("{y:04}")),
        release_precision: release_year.map(|_| crate::library_db::DatePrecision::Year),
        release_region_used: None,
        cooperative,
        normalized_title: String::new(),
        normalized_title_alt: String::new(),
        board,
        is_mature,
        ra_id,
        rc_hash: hash.and_then(|h| h.rc_hash.clone()),
    })
}

/// How a system's ROMs are hash-identified in the identity phase. The single
/// source of truth for the disc-vs-cart-vs-none dispatch, so callers don't each
/// re-ask `is_disc_rc_hash_system` / `is_hash_eligible`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashIdentificationMethod {
    /// Cartridge whole-file No-Intro CRC (+ header rc_hash for some systems).
    Cart,
    /// Disc boot-file rc_hash (PSX, Sega CD, Saturn, Dreamcast, 3DO, …).
    Disc,
    /// No runtime hash identification for this system.
    None,
}

/// The hash-identification method for `system` (see [`HashIdentificationMethod`]).
pub fn hash_identification_method(system: &str) -> HashIdentificationMethod {
    if rc_hash_disc::is_disc_rc_hash_system(system) {
        HashIdentificationMethod::Disc
    } else if crate::rom_hash::is_hash_eligible(system) {
        HashIdentificationMethod::Cart
    } else {
        HashIdentificationMethod::None
    }
}

/// Whether a ROM file participates in hash identification (gets a CRC/rc_hash and
/// an `ra_id`). Disc systems accept disc images/playlists; cart systems use the
/// No-Intro eligibility. Files that aren't candidates are marked `NotApplicable`
/// and never hashed.
fn is_identity_applicable(system: &str, rom_filename: &str) -> bool {
    match hash_identification_method(system) {
        HashIdentificationMethod::Disc => is_disc_identity_candidate(rom_filename),
        HashIdentificationMethod::Cart => {
            crate::rom_hash::is_file_hash_eligible(system, rom_filename)
        }
        HashIdentificationMethod::None => false,
    }
}

fn is_disc_identity_candidate(rom_filename: &str) -> bool {
    let ext = Path::new(rom_filename)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "m3u" | "chd" | "cue" | "bin" | "iso" | "img" | "gdi"
    )
}

/// Terminal identity state from a computed hash. Matched when it resolved to a
/// catalog name OR an `ra_id` — discs match via `ra_id` with no No-Intro name,
/// so checking `matched_name` alone would mis-label a hash-matched disc.
fn identity_state_from_hash(hash: &HashResult) -> IdentityState {
    if hash.matched_name.is_some() || hash.ra_id.is_some() {
        IdentityState::CompleteMatched
    } else {
        IdentityState::from_hash_match(None)
    }
}

/// Populate `normalized_title` (and arcade clone-parent `normalized_title_alt`)
/// for every entry. Stored in the row so the enrichment matcher does
/// hashmap lookups instead of normalizing each filename per pass.
///
/// Mirrors the legacy logic from `enrichment::rom_normalized_titles`:
/// - Console: normalize the canonical filename stem.
/// - Arcade: normalize the arcade-db `display_name`. For clones, also store
///   the parent's normalized display name (if different) as the secondary
///   key so the matcher can fall back to parent metadata.
///
/// Uses `title_utils::normalize_title_for_metadata` (re-exported by the
/// LaunchBox import module as `normalize_title`). Source-neutral: when a
/// second metadata source is added, it can reuse the same key.
fn populate_normalized_titles(
    system: &str,
    entries: &mut [GameEntry],
    arcade_lookup: &HashMap<String, ArcadeGameInfo>,
) {
    let is_arcade = systems::is_arcade_system(system);
    for entry in entries.iter_mut() {
        let stem = title_utils::filename_stem(&entry.rom_filename);
        if is_arcade {
            if let Some(info) = arcade_lookup.get(stem) {
                entry.normalized_title =
                    title_utils::normalize_title_for_metadata(&info.display_name);
                if info.is_clone
                    && !info.parent.is_empty()
                    && let Some(parent) = arcade_lookup.get(&info.parent)
                {
                    let parent_norm =
                        title_utils::normalize_title_for_metadata(&parent.display_name);
                    if parent_norm != entry.normalized_title {
                        entry.normalized_title_alt = parent_norm;
                    }
                }
            } else {
                entry.normalized_title = title_utils::normalize_title_for_metadata(stem);
            }
        } else {
            entry.normalized_title = title_utils::normalize_title_for_metadata(stem);
        }
    }
}

/// Batch all catalog lookups for the per-ROM builder into one or two queries
/// per system.
async fn prefetch_catalog(
    system: &str,
    roms: &[RomEntry],
    hash_results: &HashMap<String, HashResult>,
    is_arcade: bool,
) -> CatalogLookup {
    if is_arcade {
        let stems: Vec<String> = roms
            .iter()
            .map(|r| title_utils::filename_stem(&r.game.rom_filename).to_string())
            .collect();
        let refs: Vec<&str> = stems.iter().map(|s| s.as_str()).collect();
        let arcade = arcade_db::lookup_arcade_games_batch(system, &refs).await;
        CatalogLookup {
            arcade,
            ..Default::default()
        }
    } else {
        let mut stems: Vec<String> = roms
            .iter()
            .map(|r| title_utils::filename_stem(&r.game.rom_filename).to_string())
            .collect();
        // Include CRC-matched canonical names as additional lookup keys.
        for r in roms {
            if let Some(hr) = hash_results.get(&r.game.rom_filename)
                && let Some(name) = &hr.matched_name
            {
                stems.push(name.clone());
            }
        }
        stems.sort();
        stems.dedup();
        let stem_refs: Vec<&str> = stems.iter().map(|s| s.as_str()).collect();
        let by_stem = game_db::lookup_games_batch(system, &stem_refs).await;

        // Normalized-title fallback for ROMs that didn't match a stem.
        let missing_norms: Vec<String> = roms
            .iter()
            .filter_map(|r| {
                let fname = &r.game.rom_filename;
                let stem = title_utils::filename_stem(fname);
                if by_stem.contains_key(stem) {
                    return None;
                }
                let matched = hash_results
                    .get(fname)
                    .and_then(|hr| hr.matched_name.as_deref())
                    .map(|n| by_stem.contains_key(n))
                    .unwrap_or(false);
                if matched {
                    return None;
                }
                Some(game_db::normalize_filename(stem))
            })
            .collect();
        let norm_refs: Vec<&str> = missing_norms.iter().map(|s| s.as_str()).collect();
        let by_normalized = game_db::lookup_by_normalized_titles_batch(system, &norm_refs).await;

        CatalogLookup {
            arcade: HashMap::new(),
            by_stem,
            by_normalized,
        }
    }
}

/// Extract metadata from arcade databases. Returns `None` for BIOS entries.
fn build_arcade_metadata(stem: &str, batch: &CatalogLookup) -> Option<RomMetadata> {
    match batch.arcade.get(stem).cloned() {
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
            let group = genre::normalize_genre(&info.category).to_string();
            let dev = developer::normalize_developer(&info.manufacturer);
            let year: Option<u16> = info.year.parse::<u16>().ok().filter(|&y| y > 0);
            Some(RomMetadata {
                genre_detail: detail,
                genre_group: group,
                players: Some(info.players),
                is_clone: info.is_clone,
                base_title: title_utils::base_title(&info.display_name),
                developer: dev,
                release_year: year,
                cooperative: false,
                board: info.board,
                // Arcade RA id: hash-matched at catalog-build time from the
                // romset name (md5), carried straight through from arcade_db.
                ra_id: info.ra_id,
                is_mature: info.is_mature,
            })
        }
        None => Some(RomMetadata {
            genre_detail: None,
            genre_group: String::new(),
            players: None,
            is_clone: false,
            base_title: title_utils::base_title(stem),
            developer: String::new(),
            release_year: None,
            cooperative: false,
            board: None,
            ra_id: String::new(),
            is_mature: false,
        }),
    }
}

/// Extract metadata from console game databases.
/// Always returns `Some` (console ROMs are never filtered out).
fn build_console_metadata(
    r: &RomEntry,
    rom_filename: &str,
    stem: &str,
    hash_results: &HashMap<String, HashResult>,
    batch: &CatalogLookup,
) -> Option<RomMetadata> {
    // Try CRC-based lookup first (if we have a hash match),
    // then fall back to filename-based lookup.
    let hash_entry = hash_results
        .get(rom_filename)
        .and_then(|hr| hr.matched_name.as_ref())
        .and_then(|name| batch.by_stem.get(name.as_str()).cloned());
    // ra_id is per-dump and only trustworthy from a CONTENT match (CRC32 or
    // runtime rc_hash) — never from stem/filename resolution. This is a
    // deliberate precision-over-coverage choice: a flag shown must be correct.
    //
    //   - Header carts (NES/SNES/N64): resolved at scan time via the runtime
    //     rc_hash → ra_hash lookup, carried on `HashResult::ra_id`.
    //   - Whole-file carts: resolved from the CRC-matched `rom_entry` (`hash_entry`).
    //
    // Note `hash_entry` requires a CRC content match, so a merely RENAMED dump
    // still resolves here (its bytes/CRC are unchanged) and keeps its ra_id. The
    // ONLY time `ra_id` is dropped is when the content hash did NOT match and we
    // fall back to `batch.by_stem.get(stem)` below — i.e. the user's file bytes
    // aren't the verified dump. Carrying that row's ra_id would re-introduce the
    // title-match false positives this design exists to eliminate, so we don't.
    let ra_id = hash_results
        .get(rom_filename)
        .and_then(|hr| hr.ra_id.clone())
        .or_else(|| {
            hash_entry
                .as_ref()
                .map(|e| e.ra_id.clone())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_default();
    // Filename/stem fallback (content NOT verified) — intentionally does not
    // contribute `ra_id` (see above); it only supplies display/genre metadata.
    let entry = hash_entry.or_else(|| batch.by_stem.get(stem).cloned());
    let game = entry.map(|e| e.game).or_else(|| {
        let normalized = game_db::normalize_filename(stem);
        batch.by_normalized.get(&normalized).cloned()
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
            let group = genre::normalize_genre(&g.genre).to_string();
            let year: Option<u16> = if g.year > 0 { Some(g.year) } else { None };
            let cooperative = g.coop.unwrap_or(false);
            Some(RomMetadata {
                genre_detail: detail,
                genre_group: group,
                players: if g.players > 0 { Some(g.players) } else { None },
                is_clone: false,
                base_title: bt,
                developer: developer::normalize_developer(&g.developer),
                release_year: year,
                cooperative,
                board: None,
                ra_id,
                is_mature: false,
            })
        }
        // No catalog title row for display/genre — but `ra_id` was resolved
        // independently from the content hash (rc_hash/CRC → ra_hash) and must
        // survive. Hash-matched discs in particular often lack a canonical_game
        // row yet still carry a verified RA id.
        None => Some(RomMetadata {
            genre_detail: None,
            genre_group: String::new(),
            players: None,
            is_clone: false,
            base_title: bt,
            developer: String::new(),
            release_year: None,
            cooperative: false,
            board: None,
            ra_id,
            is_mature: false,
        }),
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

#[cfg(test)]
mod tests {
    use super::*;
    use replay_control_core::game_ref::GameRef;

    fn rom_entry(system: &str, filename: &str, display_name: Option<&str>) -> RomEntry {
        RomEntry {
            game: GameRef::from_parts(
                system,
                filename.to_string(),
                filename.to_string(),
                display_name.map(str::to_string),
            ),
            size_bytes: 1,
            mtime_nanos: None,
            is_m3u: false,
            is_favorite: false,
            box_art_url: None,
            driver_status: None,
            rating: None,
            players: None,
        }
    }

    fn rom_entry_finalized(system: &str, filename: &str, display_name: Option<&str>) -> RomEntry {
        RomEntry {
            game: GameRef::new_with_display(
                system,
                filename.to_string(),
                filename.to_string(),
                display_name.map(str::to_string),
            ),
            size_bytes: 1,
            mtime_nanos: None,
            is_m3u: false,
            is_favorite: false,
            box_art_url: None,
            driver_status: None,
            rating: None,
            players: None,
        }
    }

    fn canonical_game(display_name: &str, developer: &str) -> CanonicalGame {
        CanonicalGame {
            display_name: display_name.to_string(),
            year: 2024,
            genre: "Compilation".to_string(),
            developer: developer.to_string(),
            publisher: "AmigaVision Project".to_string(),
            players: 1,
            coop: Some(false),
            rating: String::new(),
            normalized_genre: "Action".to_string(),
            description: "Curated collection".to_string(),
            source: "community".to_string(),
        }
    }

    #[test]
    fn console_metadata_uses_catalog_developer() {
        let mut batch = CatalogLookup::default();
        batch.by_stem.insert(
            "AmigaVision".to_string(),
            CatalogGameEntry {
                canonical_name: "AmigaVision".to_string(),
                region: String::new(),
                crc32: 0,
                ra_id: String::new(),
                game: canonical_game("AmigaVision", "AmigaVision Project"),
            },
        );
        let rom = rom_entry("commodore_ami", "AmigaVision.hdf", Some("AmigaVision"));
        let hash_results = HashMap::new();

        let meta = build_console_metadata(
            &rom,
            "AmigaVision.hdf",
            "AmigaVision",
            &hash_results,
            &batch,
        )
        .expect("metadata should resolve");

        assert_eq!(meta.developer, "AmigaVision Project");
    }

    #[test]
    fn disc_m3u_with_ra_hash_result_is_matched_identity_row() {
        let mut rom = rom_entry("sony_psx", "Game.m3u", Some("Game"));
        rom.is_m3u = true;
        let mut hash_results = HashMap::new();
        hash_results.insert(
            "Game.m3u".to_string(),
            HashResult {
                rom_filename: "Game.m3u".to_string(),
                crc32: 0,
                mtime_secs: 123,
                size_bytes: 456,
                matched_name: None,
                ra_id: Some("9876".to_string()),
                rc_hash: Some("disc-ra-hash".to_string()),
            },
        );

        let entry = build_single_entry(&rom, &hash_results, false, &CatalogLookup::default())
            .expect("m3u identity row should build");

        assert_eq!(entry.rom_filename, "Game.m3u");
        assert!(entry.is_m3u);
        assert_eq!(entry.identity_state, IdentityState::CompleteMatched);
        assert_eq!(entry.ra_id, "9876");
        assert_eq!(entry.rc_hash.as_deref(), Some("disc-ra-hash"));
        assert_eq!(entry.hash_size_bytes, Some(456));
    }

    fn hash_result(filename: &str, matched_name: &str) -> HashResult {
        HashResult {
            rom_filename: filename.to_string(),
            crc32: 0x1234,
            mtime_secs: 123,
            size_bytes: 456,
            matched_name: Some(matched_name.to_string()),
            ra_id: None,
            rc_hash: None,
        }
    }

    #[test]
    fn hash_matched_console_display_preserves_filename_tags() {
        let rom = rom_entry(
            "nintendo_snes",
            "Super Mario World (Japan) (Rev 2).sfc",
            Some("Super Mario World (Japan, Rev 2)"),
        );
        let mut hash_results = HashMap::new();
        hash_results.insert(
            rom.game.rom_filename.clone(),
            hash_result(&rom.game.rom_filename, "Super Mario World (Japan) (Rev 2)"),
        );
        let mut batch = CatalogLookup::default();
        batch.by_stem.insert(
            "Super Mario World (Japan) (Rev 2)".to_string(),
            CatalogGameEntry {
                canonical_name: "Super Mario World (Japan) (Rev 2)".to_string(),
                region: "Japan".to_string(),
                crc32: 0x1234,
                ra_id: String::new(),
                game: canonical_game("Super Mario World", "Nintendo"),
            },
        );

        let entry = build_single_entry(&rom, &hash_results, false, &batch)
            .expect("hash-matched row should build");

        assert_eq!(
            entry.display_name.as_deref(),
            Some("Super Mario World (Japan, Rev 2)")
        );
    }

    #[test]
    fn hash_matched_console_without_catalog_does_not_double_suffix() {
        let rom = rom_entry(
            "nintendo_snes",
            "Super Mario World (Japan) (Rev 2).sfc",
            Some("Super Mario World (Japan, Rev 2)"),
        );
        let mut hash_results = HashMap::new();
        hash_results.insert(
            rom.game.rom_filename.clone(),
            hash_result(&rom.game.rom_filename, "Super Mario World (Japan) (Rev 2)"),
        );

        let entry = build_single_entry(&rom, &hash_results, false, &CatalogLookup::default())
            .expect("hash-matched row should build");

        assert_eq!(
            entry.display_name.as_deref(),
            Some("Super Mario World (Japan, Rev 2)")
        );
    }

    #[test]
    fn hash_matched_m3u_display_uses_visible_playlist_tags() {
        let mut rom = rom_entry(
            "sony_psx",
            "Metal Gear Solid (USA).m3u",
            Some("Metal Gear Solid (USA)"),
        );
        rom.is_m3u = true;
        let mut hash_results = HashMap::new();
        hash_results.insert(
            rom.game.rom_filename.clone(),
            hash_result(&rom.game.rom_filename, "Metal Gear Solid (USA) (Disc 1)"),
        );
        let mut batch = CatalogLookup::default();
        batch.by_stem.insert(
            "Metal Gear Solid (USA) (Disc 1)".to_string(),
            CatalogGameEntry {
                canonical_name: "Metal Gear Solid (USA) (Disc 1)".to_string(),
                region: "USA".to_string(),
                crc32: 0x1234,
                ra_id: String::new(),
                game: canonical_game("Metal Gear Solid", "Konami"),
            },
        );

        let entry = build_single_entry(&rom, &hash_results, false, &batch)
            .expect("hash-matched m3u row should build");

        assert_eq!(
            entry.display_name.as_deref(),
            Some("Metal Gear Solid (USA)")
        );
    }

    #[test]
    fn translation_display_keeps_already_finalized_filename_title() {
        let rom = rom_entry_finalized(
            "nintendo_gba",
            "Mother 3 (Japan) [T+Eng1.3].gba",
            Some("Mother 3 (Japan, EN Translation)"),
        );
        let mut hash_results = HashMap::new();
        hash_results.insert(
            rom.game.rom_filename.clone(),
            hash_result(&rom.game.rom_filename, "Mother 3 (Japan)"),
        );
        let batch = CatalogLookup::default();

        let entry = build_single_entry(&rom, &hash_results, false, &batch)
            .expect("translation row should build");

        assert_eq!(
            entry.display_name.as_deref(),
            Some("Mother 3 (Japan, EN Translation)")
        );
    }
}
