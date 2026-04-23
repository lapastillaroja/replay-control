//! Pure matching algorithms for auto-matching ROMs to metadata.
//!
//! These functions take data in and return data out, with no DB or state access.
//! The app layer is responsible for gathering inputs and persisting results.

use std::collections::{HashMap, HashSet};

use crate::metadata_db::GameMetadata;
use replay_control_core::title_utils::filename_stem;

/// Result of matching a ROM to existing metadata via normalized title.
#[derive(Debug, Clone)]
pub struct MetadataMatch {
    /// The ROM filename that was matched.
    pub rom_filename: String,
    /// Cloned metadata from the donor entry, with source set to "launchbox-auto".
    pub metadata: GameMetadata,
}

/// Match unmatched ROMs to existing metadata entries by normalized title.
///
/// Builds a normalized-title index from `existing_metadata`, then checks each
/// ROM in `rom_filenames` that is not already in the metadata set. When a
/// normalized-title match is found, the donor's metadata is cloned for the new ROM.
///
/// Returns a list of `MetadataMatch` entries for newly matched ROMs.
///
/// # Arguments
/// * `system` - The system folder name (used for arcade detection).
/// * `rom_filenames` - All ROM filenames in the system.
/// * `existing_metadata` - Current `(rom_filename, GameMetadata)` pairs from the DB.
pub async fn match_roms_to_metadata(
    system: &str,
    rom_filenames: &[String],
    existing_metadata: &[(String, GameMetadata)],
) -> Vec<MetadataMatch> {
    use crate::launchbox::normalize_title;
    use replay_control_core::systems;

    if existing_metadata.is_empty() || rom_filenames.is_empty() {
        return Vec::new();
    }

    let is_arcade = systems::is_arcade_system(system);

    let arcade_map = if is_arcade {
        let mut stems: Vec<String> = existing_metadata
            .iter()
            .map(|(f, _)| filename_stem(f).to_string())
            .chain(rom_filenames.iter().map(|f| filename_stem(f).to_string()))
            .collect();
        stems.sort();
        stems.dedup();
        let refs: Vec<&str> = stems.iter().map(|s| s.as_str()).collect();
        crate::arcade_db::lookup_arcade_games_batch(&refs).await
    } else {
        HashMap::new()
    };
    let normalize_from_stem = |stem: &str| -> String {
        if is_arcade {
            arcade_map
                .get(stem)
                .map(|info| normalize_title(&info.display_name))
                .unwrap_or_else(|| normalize_title(stem))
        } else {
            normalize_title(stem)
        }
    };

    // Build a normalized-title -> metadata map from existing entries.
    let mut title_index: HashMap<String, &GameMetadata> = HashMap::new();
    for (rom_filename, meta) in existing_metadata {
        let normalized = normalize_from_stem(filename_stem(rom_filename));
        title_index.entry(normalized).or_insert(meta);
    }

    // Collect filenames of ROMs that already have metadata (by exact match).
    let has_metadata: HashSet<&str> = existing_metadata
        .iter()
        .map(|(filename, _)| filename.as_str())
        .collect();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    // Find unmatched ROMs and try normalized-title lookup.
    let mut matches: Vec<MetadataMatch> = Vec::new();
    for rom_filename in rom_filenames {
        if has_metadata.contains(rom_filename.as_str()) {
            continue;
        }

        let normalized = normalize_from_stem(filename_stem(rom_filename));

        if let Some(donor_meta) = title_index.get(&normalized) {
            matches.push(MetadataMatch {
                rom_filename: rom_filename.clone(),
                metadata: GameMetadata {
                    description: donor_meta.description.clone(),
                    rating: donor_meta.rating,
                    rating_count: donor_meta.rating_count,
                    publisher: donor_meta.publisher.clone(),
                    developer: donor_meta.developer.clone(),
                    genre: donor_meta.genre.clone(),
                    players: donor_meta.players,
                    release_date: donor_meta.release_date.clone(),
                    release_precision: donor_meta.release_precision,
                    release_region_used: donor_meta.release_region_used.clone(),
                    cooperative: donor_meta.cooperative,
                    fetched_at: now,
                    box_art_path: None,
                    screenshot_path: None,
                    title_path: None,
                },
            });
        }
    }

    matches
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_metadata(rating: Option<f64>, desc: Option<&str>) -> GameMetadata {
        GameMetadata {
            description: desc.map(String::from),
            rating,
            rating_count: None,
            publisher: None,
            developer: None,
            genre: None,
            players: None,
            release_date: None,
            release_precision: None,
            release_region_used: None,
            cooperative: false,
            fetched_at: 0,
            box_art_path: None,
            screenshot_path: None,
            title_path: None,
        }
    }

    #[tokio::test]
    async fn no_metadata_returns_empty() {
        let result = match_roms_to_metadata("nintendo_snes", &["game.sfc".into()], &[]).await;
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn no_roms_returns_empty() {
        let existing = vec![("game.sfc".into(), make_metadata(Some(4.0), None))];
        let result = match_roms_to_metadata("nintendo_snes", &[], &existing).await;
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn already_matched_rom_is_skipped() {
        let existing = vec![("game.sfc".into(), make_metadata(Some(4.0), None))];
        let roms = vec!["game.sfc".into()];
        let result = match_roms_to_metadata("nintendo_snes", &roms, &existing).await;
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn unmatched_rom_gets_donor_metadata_by_normalized_title() {
        // "Super Mario World (USA).sfc" has metadata
        // "Super Mario World (Europe).sfc" does not, but should match via normalized title
        let existing = vec![(
            "Super Mario World (USA).sfc".into(),
            make_metadata(Some(4.5), Some("A classic platformer")),
        )];
        let roms = vec![
            "Super Mario World (USA).sfc".into(),
            "Super Mario World (Europe).sfc".into(),
        ];
        let result = match_roms_to_metadata("nintendo_snes", &roms, &existing).await;
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].rom_filename, "Super Mario World (Europe).sfc");
        assert_eq!(result[0].metadata.rating, Some(4.5));
        assert_eq!(
            result[0].metadata.description.as_deref(),
            Some("A classic platformer")
        );
    }

    #[tokio::test]
    async fn non_matching_rom_not_included() {
        let existing = vec![(
            "Super Mario World (USA).sfc".into(),
            make_metadata(Some(4.5), None),
        )];
        let roms = vec![
            "Super Mario World (USA).sfc".into(),
            "Donkey Kong Country (USA).sfc".into(),
        ];
        let result = match_roms_to_metadata("nintendo_snes", &roms, &existing).await;
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn multiple_unmatched_roms_matched() {
        let existing = vec![(
            "Sonic The Hedgehog (USA).md".into(),
            make_metadata(Some(3.5), None),
        )];
        let roms = vec![
            "Sonic The Hedgehog (USA).md".into(),
            "Sonic The Hedgehog (Europe).md".into(),
            "Sonic The Hedgehog (Japan).md".into(),
        ];
        let result = match_roms_to_metadata("sega_smd", &roms, &existing).await;
        assert_eq!(result.len(), 2);
        assert!(
            result
                .iter()
                .any(|m| m.rom_filename == "Sonic The Hedgehog (Europe).md")
        );
        assert!(
            result
                .iter()
                .any(|m| m.rom_filename == "Sonic The Hedgehog (Japan).md")
        );
    }
}
