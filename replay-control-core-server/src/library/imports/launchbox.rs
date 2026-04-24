//! LaunchBox metadata XML parser and importer.
//!
//! Streams the ~460 MB XML file (`launchbox-metadata.xml`) and extracts game
//! metadata, matching entries to ROMs on disk via normalized title comparison.

use std::collections::HashMap;
use std::io::BufRead;
use std::path::Path;

use quick_xml::Reader;
use quick_xml::events::Event;

use crate::library_db::{DatePrecision, GameMetadata, ImportStats};
use replay_control_core::error::{Error, Result};

/// Build the LaunchBox platform → system folder mapping from the centralized
/// system definitions in `systems.rs`. Adding a new system with
/// `launchbox_platforms` automatically enables LaunchBox import for it.
fn platform_map() -> HashMap<&'static str, Vec<&'static str>> {
    replay_control_core::systems::launchbox_platform_map()
}

/// Parsed game entry from LaunchBox XML.
struct LbGame {
    name: String,
    overview: String,
    rating: Option<f64>,
    rating_count: Option<u32>,
    publisher: String,
    developer: String,
    genre: String,
    max_players: Option<u8>,
    release_date: Option<String>,
    release_precision: Option<DatePrecision>,
    cooperative: bool,
}

/// Parse a LaunchBox `ReleaseDate` ISO 8601 datetime string.
///
/// LaunchBox stores dates like `1991-08-23T00:00:00-05:00`. We extract the
/// `YYYY-MM-DD` prefix. If the date is `YYYY-01-01`, we treat it as year-only
/// (year-only approximations are commonly stored this way). Otherwise we emit
/// day-precision.
fn parse_launchbox_release_date(text: &str) -> Option<(String, DatePrecision)> {
    let date_portion = text.get(..10).unwrap_or(text);
    if date_portion.len() < 10 || date_portion.as_bytes().get(4) != Some(&b'-') {
        // Fall back to year-only if format isn't recognizable.
        return text.get(..4).and_then(|y| {
            y.parse::<u16>()
                .ok()
                .map(|_| (y.to_string(), DatePrecision::Year))
        });
    }
    // Validate the shape YYYY-MM-DD.
    let bytes = date_portion.as_bytes();
    if bytes[7] != b'-' || !bytes[..4].iter().all(|b| b.is_ascii_digit()) {
        return text
            .get(..4)
            .and_then(|y| y.parse::<u16>().ok())
            .map(|y| (format!("{y:04}"), DatePrecision::Year));
    }
    // `-01-01` heuristic: likely a year-only approximation.
    if &date_portion[5..] == "01-01" {
        return Some((date_portion[..4].to_string(), DatePrecision::Year));
    }
    Some((date_portion.to_string(), DatePrecision::Day))
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
    let version_stripped = replay_control_core::title_utils::strip_version(&reordered);

    // Step 4: Keep only alphanumeric, lowercase.
    version_stripped
        .chars()
        .filter(|c| c.is_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Import metadata from a LaunchBox metadata XML file.
///
/// `rom_index` maps `(system_folder, normalized_title)` → `rom_filename` for all ROMs on disk.
/// This is built by the caller by scanning the ROM directories.
///
/// `flush_batch` is called for each batch of ~500 matched entries. The caller
/// is responsible for persisting them (e.g., locking a DB mutex, calling
/// `bulk_upsert`, then releasing). This keeps the core crate unaware of any
/// concurrency primitives — the app crate handles locking policy.
pub fn import_launchbox(
    xml_path: &Path,
    rom_index: &HashMap<(String, String), Vec<String>>,
    mut on_progress: impl FnMut(usize, usize, usize),
    mut flush_batch: impl FnMut(&[(String, String, GameMetadata)]) -> Result<usize>,
) -> Result<(ImportStats, ParseResult)> {
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

    let parse_result = parse_xml(reader, &platforms, |game, system_folder| {
        stats.total_source += 1;

        // Skip entries with no useful data.
        if game.overview.is_empty()
            && game.rating.is_none()
            && game.genre.is_empty()
            && game.max_players.is_none()
            && game.developer.is_empty()
            && game.release_date.is_none()
            && !game.cooperative
        {
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
                    rating_count: game.rating_count,
                    publisher: if game.publisher.is_empty() {
                        None
                    } else {
                        Some(game.publisher.clone())
                    },
                    developer: if game.developer.is_empty() {
                        None
                    } else {
                        Some(game.developer.clone())
                    },
                    genre: if game.genre.is_empty() {
                        None
                    } else {
                        Some(game.genre.clone())
                    },
                    players: game.max_players,
                    release_date: game.release_date.clone(),
                    release_precision: game.release_precision,
                    release_region_used: None,
                    cooperative: game.cooperative,
                    fetched_at: now,
                    box_art_path: None,
                    screenshot_path: None,
                    title_path: None,
                };
                batch.push((system_folder.to_string(), filename.clone(), meta));
            }
        }

        // Flush batch periodically.
        if batch.len() >= 500 {
            if let Ok(n) = flush_batch(&batch) {
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
        && let Ok(n) = flush_batch(&batch)
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

    Ok((stats, parse_result))
}

/// Stream-parse the LaunchBox XML, calling `on_game` for each game entry
/// whose platform maps to one of our systems.
///
/// When a platform maps to multiple system folders, `on_game` is called once
/// per folder so the caller can match against all of them.
/// Result of a single-pass XML parse: game metadata + alternate names + DatabaseID→Name map.
pub struct ParseResult {
    /// Alternate names grouped by DatabaseID.
    pub alternate_names: Vec<LbAlternateName>,
    /// DatabaseID → primary game name mapping (for including primary name in alias groups).
    pub game_names: HashMap<String, String>,
}

fn parse_xml<R: BufRead>(
    reader: R,
    platforms: &HashMap<&str, Vec<&str>>,
    mut on_game: impl FnMut(&LbGame, &str),
) -> Result<ParseResult> {
    let mut xml = Reader::from_reader(reader);
    xml.config_mut().trim_text(true);

    let mut buf = Vec::with_capacity(4096);

    // State tracking for which element type we're inside.
    #[derive(PartialEq)]
    enum Context {
        None,
        Game,
        AlternateName,
    }
    let mut ctx = Context::None;
    let mut current_tag = String::new();

    // Game fields.
    let mut name = String::new();
    let mut database_id = String::new();
    let mut platform = String::new();
    let mut overview = String::new();
    let mut rating: Option<f64> = None;
    let mut rating_count: Option<u32> = None;
    let mut publisher = String::new();
    let mut developer = String::new();
    let mut genre = String::new();
    let mut max_players: Option<u8> = None;
    let mut release_date: Option<String> = None;
    let mut release_precision: Option<DatePrecision> = None;
    let mut cooperative = false;

    // AlternateName fields.
    let mut alt_name = String::new();
    let mut alt_db_id = String::new();
    let mut alt_region = String::new();

    // Collected results.
    let mut alternate_names = Vec::new();
    let mut game_names = HashMap::new();

    loop {
        match xml.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let qname = e.name();
                let tag = std::str::from_utf8(qname.as_ref()).unwrap_or("");
                match tag {
                    "Game" => {
                        ctx = Context::Game;
                        name.clear();
                        database_id.clear();
                        platform.clear();
                        overview.clear();
                        rating = None;
                        rating_count = None;
                        publisher.clear();
                        developer.clear();
                        genre.clear();
                        max_players = None;
                        release_date = None;
                        release_precision = None;
                        cooperative = false;
                    }
                    "GameAlternateName" => {
                        ctx = Context::AlternateName;
                        alt_name.clear();
                        alt_db_id.clear();
                        alt_region.clear();
                    }
                    _ => {
                        if ctx != Context::None {
                            current_tag = tag.to_string();
                        }
                    }
                }
            }
            Ok(Event::Text(ref e)) => {
                let text = e.decode().unwrap_or_default();
                match ctx {
                    Context::Game => match current_tag.as_str() {
                        "Name" => name.push_str(&text),
                        "DatabaseID" => database_id.push_str(&text),
                        "Platform" => platform.push_str(&text),
                        "Overview" => overview.push_str(&text),
                        "CommunityRating" => {
                            rating = text.parse::<f64>().ok();
                        }
                        "CommunityRatingCount" => {
                            rating_count = text.parse::<u32>().ok();
                        }
                        "Publisher" => publisher.push_str(&text),
                        "Developer" => developer.push_str(&text),
                        "Genres" => genre.push_str(&text),
                        "MaxPlayers" => {
                            if let Ok(n) = text.parse::<u8>()
                                && (1..=8).contains(&n)
                            {
                                max_players = Some(n);
                            }
                        }
                        "ReleaseDate" if text.len() >= 4 => {
                            if let Some((date, precision)) = parse_launchbox_release_date(&text) {
                                release_date = Some(date);
                                release_precision = Some(precision);
                            }
                        }
                        "Cooperative" => {
                            cooperative = text.trim().eq_ignore_ascii_case("true");
                        }
                        _ => {}
                    },
                    Context::AlternateName => match current_tag.as_str() {
                        "AlternateName" => alt_name.push_str(&text),
                        "DatabaseID" => alt_db_id.push_str(&text),
                        "Region" => alt_region.push_str(&text),
                        _ => {}
                    },
                    Context::None => {}
                }
            }
            Ok(Event::End(ref e)) => {
                let qname = e.name();
                let tag = std::str::from_utf8(qname.as_ref()).unwrap_or("");
                match tag {
                    "Game" if ctx == Context::Game => {
                        ctx = Context::None;
                        // Collect DatabaseID → Name mapping for all games.
                        if !name.is_empty() && !database_id.is_empty() {
                            game_names.insert(database_id.clone(), name.clone());
                        }
                        // Callback for games matching our platforms.
                        if let Some(system_folders) = platforms.get(platform.as_str()) {
                            let game = LbGame {
                                name: std::mem::take(&mut name),
                                overview: std::mem::take(&mut overview),
                                rating,
                                rating_count,
                                publisher: std::mem::take(&mut publisher),
                                developer: std::mem::take(&mut developer),
                                genre: std::mem::take(&mut genre),
                                max_players,
                                release_date: release_date.take(),
                                release_precision: release_precision.take(),
                                cooperative,
                            };
                            for folder in system_folders {
                                on_game(&game, folder);
                            }
                        }
                    }
                    "GameAlternateName" if ctx == Context::AlternateName => {
                        ctx = Context::None;
                        if !alt_name.is_empty() && !alt_db_id.is_empty() {
                            alternate_names.push(LbAlternateName {
                                database_id: std::mem::take(&mut alt_db_id),
                                alternate_name: std::mem::take(&mut alt_name),
                                region: std::mem::take(&mut alt_region),
                            });
                        }
                    }
                    _ => {}
                }
                current_tag.clear();
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                tracing::warn!("XML parse error at position {}: {e}", xml.error_position());
            }
            _ => {}
        }
        buf.clear();
    }

    tracing::info!(
        "LaunchBox alternate names: {} entries parsed, {} game names collected",
        alternate_names.len(),
        game_names.len()
    );

    Ok(ParseResult {
        alternate_names,
        game_names,
    })
}

/// A parsed alternate name entry from LaunchBox XML.
pub struct LbAlternateName {
    pub database_id: String,
    pub alternate_name: String,
    pub region: String,
}

/// The LaunchBox metadata download URL.
const METADATA_URL: &str = "https://gamesdb.launchbox-app.com/Metadata.zip";

/// HEAD request to get Content-Length. Returns `None` on failure.
fn get_content_length(url: &str) -> Option<u64> {
    let output = std::process::Command::new("curl")
        .args(["-sI", "--max-time", "5", url])
        .output()
        .ok()?;
    let headers = String::from_utf8_lossy(&output.stdout);
    for line in headers.lines() {
        if let Some(val) = line
            .strip_prefix("content-length:")
            .or_else(|| line.strip_prefix("Content-Length:"))
        {
            return val.trim().parse().ok();
        }
    }
    None
}

/// Download LaunchBox Metadata.zip and extract to `launchbox-metadata.xml` in the given directory.
///
/// Uses `curl` with streaming stdout for download progress and `unzip` for extraction.
/// The zip internally contains `Metadata.xml`, which is renamed after extraction.
///
/// `on_progress` is called with `(bytes_downloaded, total_bytes)` during the download.
/// `total_bytes` is `None` if the server didn't provide Content-Length.
pub fn download_metadata(
    dest_dir: &Path,
    on_progress: impl Fn(u64, Option<u64>),
) -> Result<std::path::PathBuf> {
    use std::io::{Read, Write};

    use crate::library_db::LAUNCHBOX_XML;

    std::fs::create_dir_all(dest_dir).map_err(|e| {
        Error::Other(format!(
            "Cannot create directory {}: {e}",
            dest_dir.display()
        ))
    })?;

    let zip_path = dest_dir.join("Metadata.zip");
    let extracted_path = dest_dir.join("Metadata.xml"); // name inside the zip
    let xml_path = dest_dir.join(LAUNCHBOX_XML);

    // Step 1: Get Content-Length via HEAD request.
    let total_bytes = get_content_length(METADATA_URL);
    tracing::info!(
        "Downloading LaunchBox metadata from {METADATA_URL} (size: {})",
        total_bytes.map_or("unknown".to_string(), |n| format!("{n} bytes")),
    );

    // Step 2: Stream download with piped stdout.
    let mut child = std::process::Command::new("curl")
        .args(["-fsSL", "-o", "-", METADATA_URL])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| Error::Other(format!("Failed to spawn curl: {e}")))?;

    let stdout = child.stdout.take().expect("piped stdout");
    let mut reader = std::io::BufReader::with_capacity(64 * 1024, stdout);
    let mut file = std::fs::File::create(&zip_path).map_err(|e| Error::io(&zip_path, e))?;

    let mut downloaded: u64 = 0;
    let mut buf = [0u8; 64 * 1024];
    on_progress(0, total_bytes);

    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| Error::Other(format!("Read error during download: {e}")))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| Error::io(&zip_path, e))?;
        downloaded += n as u64;
        on_progress(downloaded, total_bytes);
    }

    let status = child
        .wait()
        .map_err(|e| Error::Other(format!("curl wait failed: {e}")))?;
    if !status.success() {
        let _ = std::fs::remove_file(&zip_path);
        // Try to read stderr for error details.
        let stderr_msg = child
            .stderr
            .as_mut()
            .and_then(|s| {
                let mut buf = String::new();
                s.read_to_string(&mut buf).ok().map(|_| buf)
            })
            .unwrap_or_default();
        return Err(Error::Other(format!(
            "Download failed (curl exit {}): {stderr_msg}",
            status.code().unwrap_or(-1),
        )));
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
pub async fn build_rom_index(storage_root: &Path) -> HashMap<(String, String), Vec<String>> {
    // Walk the filesystem on the blocking pool — a full import-time scan
    // issues one `read_dir` per system folder and per ROM subdirectory.
    let roms_dir = storage_root.join("roms");
    let system_files: Vec<(String, Vec<String>)> = {
        let walk = move || -> Vec<(String, Vec<String>)> {
            let entries = match std::fs::read_dir(&roms_dir) {
                Ok(e) => e,
                Err(_) => return Vec::new(),
            };
            let mut out = Vec::new();
            for entry in entries.flatten() {
                let system = entry.file_name().to_string_lossy().to_string();
                if system.starts_with('_') {
                    continue;
                }
                let mut rom_files: Vec<String> = Vec::new();
                collect_rom_filenames(&entry.path(), &mut rom_files);
                out.push((system, rom_files));
            }
            out
        };
        {
            tokio::task::spawn_blocking(walk).await.unwrap_or_else(|e| {
                tracing::warn!("build_rom_index walk panicked: {e}");
                Vec::new()
            })
        }
    };

    let mut index: HashMap<(String, String), Vec<String>> = HashMap::new();
    for (system, rom_files) in system_files {
        let arcade_lookup = if replay_control_core::systems::is_arcade_system(&system) {
            crate::image_resolution::ArcadeInfoLookup::build(&system, &rom_files).await
        } else {
            crate::image_resolution::ArcadeInfoLookup::default()
        };
        build_index_entries(&rom_files, &system, &arcade_lookup, &mut index);
    }

    let total: usize = index.values().map(|v| v.len()).sum();
    tracing::info!(
        "ROM index: {} unique titles, {} total files",
        index.len(),
        total
    );

    index
}

/// Collect ROM filenames (not stems) under `dir` recursively, skipping
/// `_`-prefixed directories.
fn collect_rom_filenames(dir: &Path, out: &mut Vec<String>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name();
            if !name.to_string_lossy().starts_with('_') {
                collect_rom_filenames(&path, out);
            }
        } else {
            out.push(entry.file_name().to_string_lossy().to_string());
        }
    }
}

/// Build LaunchBox index entries from a pre-collected ROM filename list.
///
/// For arcade systems, ROM filenames are MAME codenames (e.g. `sf2.zip`) that
/// don't match LaunchBox's human-readable titles. We use `arcade_db` to look up
/// the display name and normalize that instead. For clones, we also index under
/// the parent ROM's display name so they match the parent's LaunchBox entry.
fn build_index_entries(
    rom_files: &[String],
    system: &str,
    arcade_lookup: &crate::image_resolution::ArcadeInfoLookup,
    index: &mut HashMap<(String, String), Vec<String>>,
) {
    let is_arcade = replay_control_core::systems::is_arcade_system(system);

    for filename in rom_files {
        let stem = replay_control_core::title_utils::filename_stem(filename);

        if is_arcade {
            if let Some(info) = arcade_lookup.get(stem) {
                let norm = normalize_title(&info.display_name);
                let key = (system.to_string(), norm);
                index.entry(key).or_default().push(filename.clone());

                if info.is_clone
                    && !info.parent.is_empty()
                    && let Some(parent_info) = arcade_lookup.get(&info.parent)
                {
                    let parent_norm = normalize_title(&parent_info.display_name);
                    if parent_norm != normalize_title(&info.display_name) {
                        let parent_key = (system.to_string(), parent_norm);
                        index.entry(parent_key).or_default().push(filename.clone());
                    }
                }
            } else {
                let norm = normalize_title(stem);
                let key = (system.to_string(), norm);
                index.entry(key).or_default().push(filename.clone());
            }
        } else {
            let norm = normalize_title(stem);
            let key = (system.to_string(), norm);
            index.entry(key).or_default().push(filename.clone());
        }
    }
}

/// Run a LaunchBox XML import end-to-end: streams the XML in a blocking task,
/// flushes per-batch inserts via `pool.write`, and reports progress ticks
/// (`processed`, `matched`, `inserted`) to `on_progress`.
///
/// The caller owns any higher-level activity state; this fn just does I/O.
/// Callers that need write-gate semantics should activate one around this call.
pub async fn run_bulk_import(
    pool: &crate::DbPool,
    xml_path: &Path,
    rom_index: HashMap<(String, String), Vec<String>>,
    on_progress: impl Fn(usize, usize, usize) + Send + Sync + 'static,
) -> Result<(ImportStats, ParseResult)> {
    let pool = pool.clone();
    let xml_path = xml_path.to_path_buf();
    let on_progress = std::sync::Arc::new(on_progress);

    tokio::task::spawn_blocking(move || {
        let handle = tokio::runtime::Handle::current();
        let flush_batch = |batch: &[(String, String, GameMetadata)]| {
            let batch = batch.to_vec();
            let pool = pool.clone();
            handle
                .block_on(
                    pool.write(move |db| crate::library_db::LibraryDb::bulk_upsert(db, &batch)),
                )
                .ok_or_else(|| Error::Other("Library DB unavailable during import".to_string()))?
        };

        import_launchbox(
            &xml_path,
            &rom_index,
            |processed, matched, inserted| on_progress(processed, matched, inserted),
            flush_batch,
        )
    })
    .await
    .unwrap_or_else(|e| Err(Error::Other(e.to_string())))
}

/// Import LaunchBox alternate names into the `game_alias` table.
///
/// Reads base titles from `game_library` to match alternates against, resolves
/// via `alias_matching::resolve_launchbox_aliases`, then bulk-inserts.
pub async fn import_launchbox_aliases(pool: &crate::DbPool, parse_result: &ParseResult) {
    if parse_result.alternate_names.is_empty() {
        return;
    }

    tracing::debug!("LaunchBox aliases: loading base_titles from game_library...");
    let base_titles: HashMap<String, Vec<String>> = match pool
        .read(|conn| {
            let systems = crate::library_db::LibraryDb::active_systems(conn).unwrap_or_default();
            let mut map: HashMap<String, Vec<String>> = HashMap::new();
            for system in &systems {
                if let Ok(entries) = crate::library_db::LibraryDb::load_system_entries(conn, system)
                {
                    for entry in entries {
                        if !entry.base_title.is_empty() {
                            map.entry(entry.base_title.clone())
                                .or_default()
                                .push(system.clone());
                        }
                    }
                }
            }
            map
        })
        .await
    {
        Some(map) => map,
        None => {
            tracing::warn!("LaunchBox aliases: DB unavailable for reading base_titles");
            return;
        }
    };

    let aliases = crate::alias_matching::resolve_launchbox_aliases(
        &parse_result.alternate_names,
        &parse_result.game_names,
        &base_titles,
    );

    if aliases.is_empty() {
        tracing::debug!("LaunchBox aliases: no matches found");
        return;
    }

    let count = aliases.len();
    if let Some(result) = pool
        .write(move |db| crate::library_db::LibraryDb::bulk_insert_aliases(db, &aliases))
        .await
    {
        match result {
            Ok(n) => tracing::info!("LaunchBox aliases: {n}/{count} inserted"),
            Err(e) => tracing::warn!("LaunchBox aliases: insert failed: {e}"),
        }
    } else {
        tracing::warn!("LaunchBox aliases: DB unavailable for inserting aliases");
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
        assert_eq!(normalize_title("Game [!] (Europe)"), "game");
    }

    #[test]
    fn normalize_title_reorders_article() {
        assert_eq!(normalize_title("Legend of Zelda, The"), "thelegendofzelda");
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
        assert_eq!(normalize_title("Alien vs Predator"), "alienvspredator");
        assert_eq!(normalize_title("Marvel"), "marvel");
    }

    #[test]
    fn normalize_title_version_with_multiple_dots() {
        assert_eq!(normalize_title("Game v1.2.3"), "game");
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
