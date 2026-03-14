//! LaunchBox metadata XML parser and importer.
//!
//! Streams the ~460 MB XML file (`launchbox-metadata.xml`) and extracts game
//! metadata, matching entries to ROMs on disk via normalized title comparison.

use std::collections::HashMap;
use std::io::BufRead;
use std::path::Path;

use quick_xml::Reader;
use quick_xml::events::Event;

use crate::arcade_db;
use crate::error::{Error, Result};
use crate::metadata_db::{GameMetadata, ImportStats, MetadataDb};

/// Build the LaunchBox platform → system folder mapping from the centralized
/// system definitions in `systems.rs`. Adding a new system with
/// `launchbox_platforms` automatically enables LaunchBox import for it.
fn platform_map() -> HashMap<&'static str, Vec<&'static str>> {
    crate::systems::launchbox_platform_map()
}

/// Parsed game entry from LaunchBox XML.
struct LbGame {
    name: String,
    overview: String,
    rating: Option<f64>,
    publisher: String,
    genre: String,
    max_players: Option<u8>,
}

/// Normalize a game title for fuzzy matching.
/// - Strips parenthetical tags `(...)` and `[...]`
/// - Handles "Title, The" → "The Title" reordering (No-Intro convention)
/// - Lowercases and removes punctuation
pub fn normalize_title(name: &str) -> String {
    // Step 1: Remove anything in parentheses or brackets.
    let mut stripped = String::with_capacity(name.len());
    let mut depth = 0u32;
    for ch in name.chars() {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => {
                depth = depth.saturating_sub(1);
            }
            _ if depth == 0 => stripped.push(ch),
            _ => {}
        }
    }
    let stripped = stripped.trim();

    // Step 2: Handle "Title, The" → "The Title" (also "A", "An").
    // No-Intro uses "Legend of Zelda, The - A Link to the Past"
    // while LaunchBox uses "The Legend of Zelda: A Link to the Past".
    // Pattern: look for ", The", ", A ", ", An " after the last comma.
    let reordered = if let Some(idx) = stripped.rfind(", ") {
        let (before, after_comma) = stripped.split_at(idx);
        let after_comma = &after_comma[2..]; // skip ", "
        // Extract the first word after the comma.
        let first_word_end = after_comma
            .find(|c: char| !c.is_alphabetic())
            .unwrap_or(after_comma.len());
        let first_word = &after_comma[..first_word_end];
        let first_word_lower = first_word.to_ascii_lowercase();
        if first_word_lower == "the" || first_word_lower == "a" || first_word_lower == "an" {
            let rest = after_comma[first_word_end..].trim_start_matches([' ', '-']);
            if rest.is_empty() {
                format!("{first_word} {before}")
            } else {
                format!("{first_word} {before} {rest}")
            }
        } else {
            stripped.to_string()
        }
    } else {
        stripped.to_string()
    };

    // Step 3: Strip TOSEC version strings (e.g., "v1.000", "v2.0").
    // TOSEC filenames like "Game v1.000 (1999)(Sega)(PAL)" have the (...)
    // content removed in step 1, leaving "Game v1.000". The version suffix
    // prevents matching against LaunchBox titles like "Game".
    let version_stripped = crate::thumbnails::strip_version(&reordered);

    // Step 4: Keep only alphanumeric, lowercase.
    version_stripped
        .chars()
        .filter(|c| c.is_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Import metadata from a LaunchBox metadata XML file into the metadata DB.
///
/// `rom_index` maps `(system_folder, normalized_title)` → `rom_filename` for all ROMs on disk.
/// This is built by the caller by scanning the ROM directories.
pub fn import_launchbox(
    xml_path: &Path,
    db: &mut MetadataDb,
    rom_index: &HashMap<(String, String), Vec<String>>,
    mut on_progress: impl FnMut(usize, usize, usize),
) -> Result<ImportStats> {
    let file = std::fs::File::open(xml_path).map_err(|e| Error::io(xml_path, e))?;
    let reader = std::io::BufReader::with_capacity(256 * 1024, file);

    let platforms = platform_map();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let mut stats = ImportStats {
        total_source: 0,
        matched: 0,
        inserted: 0,
        skipped: 0,
    };

    // Batch buffer for bulk inserts.
    let mut batch: Vec<(String, String, GameMetadata)> = Vec::with_capacity(1000);

    parse_xml(reader, &platforms, |game, system_folder| {
        stats.total_source += 1;

        // Skip entries with no useful data.
        if game.overview.is_empty() && game.rating.is_none() && game.genre.is_empty() && game.max_players.is_none() {
            stats.skipped += 1;
            return;
        }

        let norm = normalize_title(&game.name);
        let key = (system_folder.to_string(), norm);

        if let Some(filenames) = rom_index.get(&key) {
            stats.matched += 1;
            for filename in filenames {
                let meta = GameMetadata {
                    description: if game.overview.is_empty() {
                        None
                    } else {
                        Some(game.overview.clone())
                    },
                    rating: game.rating,
                    publisher: if game.publisher.is_empty() {
                        None
                    } else {
                        Some(game.publisher.clone())
                    },
                    genre: if game.genre.is_empty() {
                        None
                    } else {
                        Some(game.genre.clone())
                    },
                    players: game.max_players,
                    source: "launchbox".to_string(),
                    fetched_at: now,
                    box_art_path: None,
                    screenshot_path: None,
                };
                batch.push((system_folder.to_string(), filename.clone(), meta));
            }
        }

        // Flush batch periodically.
        if batch.len() >= 500 {
            if let Ok(n) = db.bulk_upsert(&batch) {
                stats.inserted += n;
            }
            batch.clear();
        }

        // Report progress every 5000 entries.
        if stats.total_source.is_multiple_of(5000) {
            on_progress(stats.total_source, stats.matched, stats.inserted);
        }
    })?;

    // Flush remaining.
    if !batch.is_empty()
        && let Ok(n) = db.bulk_upsert(&batch)
    {
        stats.inserted += n;
    }

    tracing::info!(
        "LaunchBox import: {} source entries, {} matched, {} inserted, {} skipped",
        stats.total_source,
        stats.matched,
        stats.inserted,
        stats.skipped,
    );

    Ok(stats)
}

/// Stream-parse the LaunchBox XML, calling `on_game` for each game entry
/// whose platform maps to one of our systems.
///
/// When a platform maps to multiple system folders, `on_game` is called once
/// per folder so the caller can match against all of them.
fn parse_xml<R: BufRead>(
    reader: R,
    platforms: &HashMap<&str, Vec<&str>>,
    mut on_game: impl FnMut(&LbGame, &str),
) -> Result<()> {
    let mut xml = Reader::from_reader(reader);
    xml.config_mut().trim_text(true);

    let mut buf = Vec::with_capacity(4096);
    let mut in_game = false;
    let mut current_tag = String::new();

    // Current game fields being accumulated.
    let mut name = String::new();
    let mut platform = String::new();
    let mut overview = String::new();
    let mut rating: Option<f64> = None;
    let mut publisher = String::new();
    let mut genre = String::new();
    let mut max_players: Option<u8> = None;

    loop {
        match xml.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let qname = e.name();
                let tag = std::str::from_utf8(qname.as_ref()).unwrap_or("");
                if tag == "Game" {
                    in_game = true;
                    name.clear();
                    platform.clear();
                    overview.clear();
                    rating = None;
                    publisher.clear();
                    genre.clear();
                    max_players = None;
                } else if in_game {
                    current_tag = tag.to_string();
                }
            }
            Ok(Event::Text(ref e)) => {
                if in_game {
                    let text = e.unescape().unwrap_or_default();
                    match current_tag.as_str() {
                        "Name" => name.push_str(&text),
                        "Platform" => platform.push_str(&text),
                        "Overview" => overview.push_str(&text),
                        "CommunityRating" => {
                            rating = text.parse::<f64>().ok();
                        }
                        "Publisher" => publisher.push_str(&text),
                        "Genres" => genre.push_str(&text),
                        "MaxPlayers" => {
                            if let Ok(n) = text.parse::<u8>() {
                                if n >= 1 && n <= 8 {
                                    max_players = Some(n);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let qname = e.name();
                let tag = std::str::from_utf8(qname.as_ref()).unwrap_or("");
                if tag == "Game" && in_game {
                    in_game = false;
                    if let Some(system_folders) = platforms.get(platform.as_str()) {
                        let game = LbGame {
                            name: std::mem::take(&mut name),
                            overview: std::mem::take(&mut overview),
                            rating,
                            publisher: std::mem::take(&mut publisher),
                            genre: std::mem::take(&mut genre),
                            max_players,
                        };
                        for folder in system_folders {
                            on_game(&game, folder);
                        }
                    }
                }
                current_tag.clear();
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                tracing::warn!("XML parse error at position {}: {e}", xml.error_position());
                // Continue parsing despite errors.
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(())
}

/// The LaunchBox metadata download URL.
const METADATA_URL: &str = "https://gamesdb.launchbox-app.com/Metadata.zip";

/// Download LaunchBox Metadata.zip and extract to `launchbox-metadata.xml` in the given directory.
///
/// Uses `curl` for download and `unzip` for extraction (available on all targets).
/// The zip internally contains `Metadata.xml`, which is renamed after extraction.
/// Returns the path to the extracted XML file.
pub fn download_metadata(dest_dir: &Path) -> Result<std::path::PathBuf> {
    use crate::metadata_db::LAUNCHBOX_XML;

    std::fs::create_dir_all(dest_dir).map_err(|e| {
        Error::Other(format!(
            "Cannot create directory {}: {e}",
            dest_dir.display()
        ))
    })?;

    let zip_path = dest_dir.join("Metadata.zip");
    let extracted_path = dest_dir.join("Metadata.xml"); // name inside the zip
    let xml_path = dest_dir.join(LAUNCHBOX_XML);

    // Download with curl.
    tracing::info!("Downloading LaunchBox metadata from {METADATA_URL}");
    let output = std::process::Command::new("curl")
        .args(["-fSL", "-o"])
        .arg(&zip_path)
        .arg(METADATA_URL)
        .output()
        .map_err(|e| Error::Other(format!("Failed to run curl: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Clean up partial download.
        let _ = std::fs::remove_file(&zip_path);
        return Err(Error::Other(format!("Download failed: {stderr}")));
    }

    // Extract Metadata.xml from the zip (upstream filename inside the archive).
    tracing::info!("Extracting Metadata.xml from {}", zip_path.display());
    let output = std::process::Command::new("unzip")
        .args(["-o", "-j"]) // overwrite, junk paths
        .arg(&zip_path)
        .arg("Metadata.xml")
        .arg("-d")
        .arg(dest_dir)
        .output()
        .map_err(|e| Error::Other(format!("Failed to run unzip: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Other(format!("Extraction failed: {stderr}")));
    }

    // Remove the zip to save space.
    let _ = std::fs::remove_file(&zip_path);

    if !extracted_path.exists() {
        return Err(Error::Other(
            "Metadata.xml not found in archive".to_string(),
        ));
    }

    // Rename from upstream name to our canonical name.
    std::fs::rename(&extracted_path, &xml_path).map_err(|e| {
        Error::Other(format!(
            "Failed to rename Metadata.xml to {LAUNCHBOX_XML}: {e}"
        ))
    })?;

    tracing::info!("{LAUNCHBOX_XML} extracted to {}", xml_path.display());
    Ok(xml_path)
}

/// Build a ROM index from the filesystem: maps `(system_folder, normalized_title)` → `[rom_filename]`.
///
/// Scans the given ROM directories recursively and normalizes each filename
/// for matching against LaunchBox titles.
pub fn build_rom_index(storage_root: &Path) -> HashMap<(String, String), Vec<String>> {
    let roms_dir = storage_root.join("roms");
    let mut index: HashMap<(String, String), Vec<String>> = HashMap::new();

    let entries = match std::fs::read_dir(&roms_dir) {
        Ok(e) => e,
        Err(_) => return index,
    };

    for entry in entries.flatten() {
        let system = entry.file_name().to_string_lossy().to_string();
        // Skip special directories.
        if system.starts_with('_') {
            continue;
        }

        let system_dir = entry.path();
        scan_rom_dir_recursive(&system_dir, &system, &mut index);
    }

    let total: usize = index.values().map(|v| v.len()).sum();
    tracing::info!(
        "ROM index: {} unique titles, {} total files",
        index.len(),
        total
    );

    index
}

/// System folders that use MAME-style ROM zip naming (codenames, not human titles).
const ARCADE_SYSTEMS: &[&str] = &[
    "arcade_mame",
    "arcade_fbneo",
    "arcade_mame_2k3p",
    "arcade_dc",
];

/// Recursively scan a directory for ROM files, adding them to the index.
///
/// For arcade systems, ROM filenames are MAME codenames (e.g. `sf2.zip`) that
/// don't match LaunchBox's human-readable titles. We use `arcade_db` to look up
/// the display name and normalize that instead. For clones, we also index under
/// the parent ROM's display name so they match the parent's LaunchBox entry.
fn scan_rom_dir_recursive(
    dir: &Path,
    system: &str,
    index: &mut HashMap<(String, String), Vec<String>>,
) {
    let is_arcade = ARCADE_SYSTEMS.contains(&system);
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if !name_str.starts_with('_') {
                scan_rom_dir_recursive(&path, system, index);
            }
        } else {
            let filename = entry.file_name().to_string_lossy().to_string();
            // Strip extension to get the title stem.
            let stem = match filename.rfind('.') {
                Some(i) => &filename[..i],
                None => &filename,
            };

            if is_arcade {
                // Use arcade_db to get the human-readable display name.
                if let Some(info) = arcade_db::lookup_arcade_game(stem) {
                    let norm = normalize_title(info.display_name);
                    let key = (system.to_string(), norm);
                    index.entry(key).or_default().push(filename.clone());

                    // For clones, also index under the parent's display name
                    // so they can match the parent's LaunchBox entry.
                    if info.is_clone
                        && !info.parent.is_empty()
                        && let Some(parent_info) = arcade_db::lookup_arcade_game(info.parent)
                    {
                        let parent_norm = normalize_title(parent_info.display_name);
                        if parent_norm != normalize_title(info.display_name) {
                            let parent_key = (system.to_string(), parent_norm);
                            index.entry(parent_key).or_default().push(filename);
                        }
                    }
                } else {
                    // ROM not in arcade_db — fall back to normalizing the stem directly.
                    let norm = normalize_title(stem);
                    let key = (system.to_string(), norm);
                    index.entry(key).or_default().push(filename);
                }
            } else {
                let norm = normalize_title(stem);
                let key = (system.to_string(), norm);
                index.entry(key).or_default().push(filename);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_title_basic() {
        assert_eq!(normalize_title("Super Mario World"), "supermarioworld");
    }

    #[test]
    fn normalize_title_strips_tags() {
        assert_eq!(
            normalize_title("Sonic The Hedgehog (USA)"),
            "sonicthehedgehog"
        );
        assert_eq!(
            normalize_title("Game [!] (Europe)"),
            "game"
        );
    }

    #[test]
    fn normalize_title_reorders_article() {
        assert_eq!(
            normalize_title("Legend of Zelda, The"),
            "thelegendofzelda"
        );
        assert_eq!(
            normalize_title("Legend of Zelda, The - A Link to the Past"),
            "thelegendofzeldaalinktothepast"
        );
    }

    #[test]
    fn normalize_title_strips_tosec_version() {
        // TOSEC-named Dreamcast ROMs: "(...)"-wrapped metadata is stripped,
        // but the bare version string "v1.000" must also be removed.
        assert_eq!(
            normalize_title("The House of the Dead 2 v1.000 (1999)(Sega)(PAL)(M6)[!]"),
            "thehouseofthedead2"
        );
        assert_eq!(
            normalize_title("Metropolis Street Racer v1.009 (2000)(Sega)(PAL)(M5)[!]"),
            "metropolisstreetracer"
        );
    }

    #[test]
    fn normalize_title_preserves_v_in_words() {
        // "vs" and "v" in normal words should NOT be stripped
        assert_eq!(
            normalize_title("Alien vs Predator"),
            "alienvspredator"
        );
        assert_eq!(normalize_title("Marvel"), "marvel");
    }

    #[test]
    fn normalize_title_version_with_multiple_dots() {
        assert_eq!(
            normalize_title("Game v1.2.3"),
            "game"
        );
    }

    #[test]
    fn normalize_title_version_at_end_only() {
        // "v2 Special Edition" has non-version text after — should NOT strip
        assert_eq!(
            normalize_title("Game v2 Special Edition"),
            "gamev2specialedition"
        );
    }
}
