//! LaunchBox metadata XML parser and importer.
//!
//! Streams the ~460 MB XML file (`launchbox-metadata.xml`) and extracts game
//! metadata, matching entries to ROMs on disk via normalized title comparison.

use std::collections::HashMap;
use std::io::BufRead;
use std::path::Path;

use quick_xml::Reader;
use quick_xml::events::Event;

use crate::library_db::DatePrecision;
use replay_control_core::error::{Error, Result};

/// Build the LaunchBox platform → system folder mapping from the centralized
/// system definitions in `systems.rs`. Adding a new system with
/// `launchbox_platforms` automatically enables LaunchBox import for it.
pub(crate) fn platform_map() -> HashMap<&'static str, Vec<&'static str>> {
    replay_control_core::systems::launchbox_platform_map()
}

/// Parsed game entry from LaunchBox XML.
pub(crate) struct LbGame {
    pub(crate) name: String,
    pub(crate) database_id: String,
    pub(crate) overview: String,
    pub(crate) rating: Option<f64>,
    pub(crate) rating_count: Option<u32>,
    pub(crate) publisher: String,
    pub(crate) developer: String,
    pub(crate) genre: String,
    pub(crate) max_players: Option<u8>,
    pub(crate) release_date: Option<String>,
    pub(crate) release_precision: Option<DatePrecision>,
    pub(crate) cooperative: bool,
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

/// Normalize a game title for fuzzy matching against LaunchBox metadata.
/// Re-export of the canonical implementation in `replay_control_core::title_utils`,
/// kept under this name for back-compat with the existing import-side callers.
pub use replay_control_core::title_utils::normalize_title_for_metadata as normalize_title;

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

pub(crate) fn parse_xml<R: BufRead>(
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
                                database_id: database_id.clone(),
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

/// Result of a HEAD request: content length and ETag, either may be absent.
#[derive(Default)]
pub struct HeadHeaders {
    pub content_length: Option<u64>,
    pub etag: Option<String>,
}

/// HEAD request. Returns `None` fields on failure or missing headers.
fn fetch_head_headers(url: &str) -> HeadHeaders {
    let output = match std::process::Command::new("curl")
        .args(["-sI", "--max-time", "10", url])
        .output()
    {
        Ok(o) => o,
        Err(_) => return HeadHeaders::default(),
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let mut content_length = None;
    let mut etag = None;
    for line in text.lines() {
        let lower = line.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("content-length:") {
            content_length = v.trim().parse().ok();
        } else if lower.starts_with("etag:") {
            // ETags are case-sensitive (RFC 7232 §2.3) — extract from original line.
            etag = line.get("etag:".len()..).map(|v| v.trim().to_string());
        }
    }
    HeadHeaders {
        content_length,
        etag,
    }
}

/// Fetch the HEAD headers for the upstream LaunchBox ZIP without downloading it.
/// Returns `None` fields if the server doesn't respond or omits a header.
pub fn fetch_upstream_head() -> HeadHeaders {
    fetch_head_headers(METADATA_URL)
}

/// Download LaunchBox Metadata.zip and extract to `launchbox-metadata.xml` in the given directory.
///
/// Uses `curl` with streaming stdout for download progress and `unzip` for extraction.
/// The zip internally contains `Metadata.xml`, which is renamed after extraction.
///
/// `content_length` — if the caller already did a HEAD request (e.g. for an ETag check),
/// pass the Content-Length here to skip a redundant HEAD. Pass `None` to fetch it.
///
/// `on_progress` is called with `(bytes_downloaded, total_bytes)` during the download.
/// `total_bytes` is `None` if the server didn't provide Content-Length.
pub fn download_metadata(
    dest_dir: &Path,
    content_length: Option<u64>,
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

    let total_bytes = content_length.or_else(|| fetch_head_headers(METADATA_URL).content_length);
    tracing::info!(
        "Downloading LaunchBox metadata from {METADATA_URL} (size: {})",
        total_bytes.map_or("unknown".to_string(), |n| format!("{n} bytes")),
    );

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
