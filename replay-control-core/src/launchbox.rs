//! LaunchBox Metadata.xml parser and importer.
//!
//! Streams the ~460 MB XML file and extracts game metadata,
//! matching entries to ROMs on disk via normalized title comparison.

use std::collections::HashMap;
use std::io::BufRead;
use std::path::Path;

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::error::{Error, Result};
use crate::metadata_db::{GameMetadata, ImportStats, MetadataDb};

/// Mapping from LaunchBox platform names to our system folder names.
fn platform_map() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    // Arcade
    m.insert("Arcade", "arcade_mame");
    // Atari
    m.insert("Atari 2600", "atari_2600");
    m.insert("Atari 5200", "atari_5200");
    m.insert("Atari 7800", "atari_7800");
    m.insert("Atari Jaguar", "atari_jaguar");
    m.insert("Atari Lynx", "atari_lynx");
    // Computers
    m.insert("Amstrad CPC", "amstrad_cpc");
    m.insert("Commodore Amiga", "commodore_ami");
    m.insert("Commodore 64", "commodore_c64");
    m.insert("MS-DOS", "ibm_pc");
    m.insert("Microsoft MSX", "microsoft_msx");
    m.insert("Microsoft MSX2", "microsoft_msx");
    m.insert("Sharp X68000", "sharp_x68k");
    m.insert("Sinclair ZX Spectrum", "sinclair_zx");
    // NEC
    m.insert("NEC TurboGrafx-16", "nec_pce");
    m.insert("NEC TurboGrafx-CD", "nec_pcecd");
    m.insert("NEC PC Engine", "nec_pce");
    m.insert("NEC PC Engine CD-ROM", "nec_pcecd");
    // Nintendo
    m.insert("Nintendo DS", "nintendo_ds");
    m.insert("Nintendo Game Boy", "nintendo_gb");
    m.insert("Nintendo Game Boy Advance", "nintendo_gba");
    m.insert("Nintendo Game Boy Color", "nintendo_gbc");
    m.insert("Nintendo 64", "nintendo_n64");
    m.insert("Nintendo Entertainment System", "nintendo_nes");
    m.insert("Super Nintendo Entertainment System", "nintendo_snes");
    // Panasonic / Philips
    m.insert("3DO Interactive Multiplayer", "panasonic_3do");
    m.insert("Philips CD-i", "philips_cdi");
    // Sega
    m.insert("Sega 32X", "sega_32x");
    m.insert("Sega CD", "sega_cd");
    m.insert("Sega Dreamcast", "sega_dc");
    m.insert("Sega Game Gear", "sega_gg");
    m.insert("Sega Genesis", "sega_smd");
    m.insert("Sega Mega Drive", "sega_smd");
    m.insert("Sega Master System", "sega_sms");
    m.insert("Sega Saturn", "sega_st");
    m.insert("Sega SG-1000", "sega_sg");
    // SNK
    m.insert("SNK Neo Geo AES", "snk_ng");
    m.insert("SNK Neo Geo MVS", "snk_ng");
    m.insert("SNK Neo Geo CD", "snk_ngcd");
    m.insert("SNK Neo Geo Pocket", "snk_ngp");
    m.insert("SNK Neo Geo Pocket Color", "snk_ngp");
    // Sony
    m.insert("Sony Playstation", "sony_psx");
    m
}

/// Parsed game entry from LaunchBox XML.
struct LbGame {
    name: String,
    platform: String,
    overview: String,
    rating: Option<f64>,
    publisher: String,
}

/// Normalize a game title for fuzzy matching.
/// - Strips parenthetical tags `(...)` and `[...]`
/// - Handles "Title, The" → "The Title" reordering (No-Intro convention)
/// - Lowercases and removes punctuation
fn normalize_title(name: &str) -> String {
    // Step 1: Remove anything in parentheses or brackets.
    let mut stripped = String::with_capacity(name.len());
    let mut depth = 0u32;
    for ch in name.chars() {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => {
                if depth > 0 {
                    depth -= 1;
                }
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
            let rest = after_comma[first_word_end..].trim_start_matches(|c: char| c == ' ' || c == '-');
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

    // Step 3: Keep only alphanumeric, lowercase.
    reordered
        .chars()
        .filter(|c| c.is_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Import metadata from a LaunchBox Metadata.xml file into the metadata DB.
///
/// `rom_index` maps `(system_folder, normalized_title)` → `rom_filename` for all ROMs on disk.
/// This is built by the caller by scanning the ROM directories.
pub fn import_launchbox(
    xml_path: &Path,
    db: &mut MetadataDb,
    rom_index: &HashMap<(String, String), Vec<String>>,
    mut on_progress: impl FnMut(usize, usize, usize),
) -> Result<ImportStats> {
    let file = std::fs::File::open(xml_path)
        .map_err(|e| Error::io(xml_path, e))?;
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
        if game.overview.is_empty() && game.rating.is_none() {
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
        if stats.total_source % 5000 == 0 {
            on_progress(stats.total_source, stats.matched, stats.inserted);
        }
    })?;

    // Flush remaining.
    if !batch.is_empty() {
        if let Ok(n) = db.bulk_upsert(&batch) {
            stats.inserted += n;
        }
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
fn parse_xml<R: BufRead>(
    reader: R,
    platforms: &HashMap<&str, &str>,
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
                        _ => {}
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let qname = e.name();
                let tag = std::str::from_utf8(qname.as_ref()).unwrap_or("");
                if tag == "Game" && in_game {
                    in_game = false;
                    if let Some(system_folder) = platforms.get(platform.as_str()) {
                        let game = LbGame {
                            name: std::mem::take(&mut name),
                            platform: std::mem::take(&mut platform),
                            overview: std::mem::take(&mut overview),
                            rating,
                            publisher: std::mem::take(&mut publisher),
                        };
                        on_game(&game, system_folder);
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

/// Download LaunchBox Metadata.zip and extract Metadata.xml to the given directory.
///
/// Uses `curl` for download and `unzip` for extraction (available on all targets).
/// Returns the path to the extracted Metadata.xml.
pub fn download_metadata(dest_dir: &Path) -> Result<std::path::PathBuf> {
    std::fs::create_dir_all(dest_dir)
        .map_err(|e| Error::Other(format!("Cannot create directory {}: {e}", dest_dir.display())))?;

    let zip_path = dest_dir.join("Metadata.zip");
    let xml_path = dest_dir.join("Metadata.xml");

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

    // Extract just Metadata.xml from the zip.
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

    if !xml_path.exists() {
        return Err(Error::Other("Metadata.xml not found in archive".to_string()));
    }

    tracing::info!("Metadata.xml extracted to {}", xml_path.display());
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
    tracing::info!("ROM index: {} unique titles, {} total files", index.len(), total);

    index
}

/// Recursively scan a directory for ROM files, adding them to the index.
fn scan_rom_dir_recursive(
    dir: &Path,
    system: &str,
    index: &mut HashMap<(String, String), Vec<String>>,
) {
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

            let norm = normalize_title(stem);
            let key = (system.to_string(), norm);
            index.entry(key).or_default().push(filename);
        }
    }
}
