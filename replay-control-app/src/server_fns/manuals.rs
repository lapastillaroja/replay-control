use super::*;
#[cfg(feature = "ssr")]
use replay_control_core::metadata_db::MetadataDb;

#[cfg(feature = "ssr")]
pub use replay_control_core::game_docs::GameDocument;
#[cfg(feature = "ssr")]
pub use replay_control_core::retrokit_manuals::ManualRecommendation;

#[cfg(not(feature = "ssr"))]
pub use crate::types::GameDocument;
#[cfg(not(feature = "ssr"))]
pub use crate::types::ManualRecommendation;

/// A local manual file found on disk in `<storage>/manuals/<system>/`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalManual {
    /// Filename on disk (e.g., "Super Mario World (en).pdf")
    pub filename: String,
    /// Display label (e.g., "Super Mario World (en)")
    pub label: String,
    /// File size in bytes
    pub size_bytes: u64,
    /// Language parsed from filename, if any
    pub language: Option<String>,
    /// URL to serve the file
    pub url: String,
}

/// Get in-folder documents for a game's ROM directory.
#[server(prefix = "/sfn")]
pub async fn get_game_documents(
    system: String,
    rom_filename: String,
) -> Result<Vec<GameDocument>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();

    // Resolve the ROM's directory path
    let roms_dir = storage.roms_dir().join(&system);
    let rom_path = roms_dir.join(&rom_filename);

    // Run all blocking filesystem I/O off the async runtime to avoid stalling
    // the tokio worker pool on slow USB or NFS storage.
    let docs = tokio::task::spawn_blocking(move || {
        // Resolve the game directory based on the ROM type:
        // - .svm: read file contents to find the game directory
        // - .m3u: playlist referencing a .svm, resolve the game directory from that
        // - directory: use directly
        // - single file: no in-folder documents
        let game_dir = if rom_filename.ends_with(".svm") {
            resolve_svm_game_dir(&rom_path, &roms_dir)
        } else if rom_filename.ends_with(".m3u") {
            resolve_m3u_game_dir(&rom_path, &roms_dir)
        } else if rom_path.is_dir() {
            Some(rom_path)
        } else {
            None
        };

        match game_dir {
            Some(d) => replay_control_core::game_docs::scan_game_documents(&d),
            None => Vec::new(),
        }
    })
    .await
    .map_err(|e| ServerFnError::new(format!("Task failed: {e}")))?;

    Ok(docs)
}

/// Get locally saved manuals for a game (from `<storage>/manuals/<system>/`).
#[server(prefix = "/sfn")]
pub async fn get_local_manuals(
    system: String,
    base_title: String,
) -> Result<Vec<LocalManual>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();

    // Resolve alias base_titles for cross-name sharing.
    let mut all_titles = vec![base_title.clone()];
    if let Some(aliases) = state.metadata_pool.read(|conn| {
        MetadataDb::alias_base_titles(conn, &system, &base_title)
    }) {
        all_titles.extend(aliases);
    }

    let folder = replay_control_core::retrokit_manuals::manual_folder_name(&system).to_string();
    let manuals_dir = state.storage().manuals_dir().join(&folder);

    // Run all blocking filesystem I/O off the async runtime to avoid stalling
    // the tokio worker pool on slow USB or NFS storage.
    let manuals = tokio::task::spawn_blocking(move || {
        if !manuals_dir.is_dir() {
            return Vec::new();
        }

        let entries = match std::fs::read_dir(&manuals_dir) {
            Ok(e) => e,
            Err(_) => return Vec::new(),
        };

        let mut manuals = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let filename = match entry.file_name().to_str() {
                Some(f) => f.to_string(),
                None => continue,
            };
            if !filename.to_lowercase().ends_with(".pdf") {
                continue;
            }

            // Check if this manual matches any of the base_titles
            let stem = filename
                .strip_suffix(".pdf")
                .or_else(|| filename.strip_suffix(".PDF"))
                .unwrap_or(&filename);
            let file_base = extract_manual_base_title(stem);

            let matches = all_titles.iter().any(|bt| bt.eq_ignore_ascii_case(&file_base));

            if !matches {
                continue;
            }

            let size_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let language = extract_language_from_filename(stem);
            let label = stem.to_string();
            let url = format!("/manuals/{folder}/{}", urlencoding::encode(&filename));

            manuals.push(LocalManual {
                filename,
                label,
                size_bytes,
                language,
                url,
            });
        }

        manuals
    })
    .await
    .map_err(|e| ServerFnError::new(format!("Task failed: {e}")))?;

    Ok(manuals)
}

/// Search for game manuals via two-tier lookup:
/// 1. Retrokit TSV (deterministic, cached)
/// 2. Archive.org search API (fuzzy fallback)
#[server(prefix = "/sfn")]
pub async fn search_game_manuals(
    system: String,
    base_title: String,
    display_name: String,
) -> Result<Vec<ManualRecommendation>, ServerFnError> {
    use replay_control_core::retrokit_manuals;

    // Normalize the title for matching
    let normalized = retrokit_manuals::normalize_retrokit_title(&base_title);

    // Load user's language preferences for sorting results
    let preferred_langs = {
        let state = expect_context::<crate::api::AppState>();
        let storage = state.storage();
        let primary = replay_control_core::settings::read_language_primary(&storage.root);
        let secondary = replay_control_core::settings::read_language_secondary(&storage.root);
        let region = state.region_preference();
        replay_control_core::settings::preferred_languages(
            primary.as_deref(),
            secondary.as_deref(),
            region,
        )
    };

    // Tier 1: Retrokit TSV lookup
    if let Some(folder) = retrokit_manuals::retrokit_folder_name(&system) {
        match load_retrokit_index(folder).await {
            Ok(index) => {
                if let Some(sources) = index.get(&normalized) {
                    let mut results: Vec<ManualRecommendation> = sources
                        .iter()
                        .map(|s| ManualRecommendation {
                            source: "retrokit".to_string(),
                            title: s.title.clone(),
                            url: s.url.clone(),
                            size_bytes: None,
                            language: Some(s.language.clone()),
                            source_id: String::new(),
                        })
                        .collect();
                    if !results.is_empty() {
                        // Sort by language preference
                        results.sort_by_key(|r| {
                            let lang = r.language.as_deref().unwrap_or("");
                            replay_control_core::settings::language_match_score(lang, &preferred_langs)
                        });
                        tracing::info!(
                            "Manual search: retrokit hit for \"{normalized}\" ({} results)",
                            results.len()
                        );
                        return Ok(results);
                    }
                }
                tracing::info!("Manual search: retrokit miss for \"{normalized}\", trying Archive.org");
            }
            Err(e) => {
                tracing::warn!("Manual search: retrokit TSV load failed for {folder}: {e}");
            }
        }
    }

    // Tier 2: Archive.org Advanced Search API fallback
    let clean_title = replay_control_core::title_utils::strip_tags(&display_name).trim();
    let platform_terms = retrokit_manuals::platform_search_terms(&system);

    let query = if platform_terms.is_empty() {
        format!("collection:(consolemanuals OR gamemanuals) AND title:({clean_title})")
    } else {
        format!(
            "collection:(consolemanuals OR gamemanuals OR arcademanuals) AND title:({clean_title}) AND ({platform_terms})"
        )
    };

    let encoded_query = urlencoding::encode(&query);
    let api_url = format!(
        "https://archive.org/advancedsearch.php?q={encoded_query}&output=json&fl[]=identifier&fl[]=title&fl[]=description&fl[]=item_size&rows=10&page=1"
    );

    tracing::info!("Manual search: Archive.org query=\"{query}\"");

    match curl_get_json(&api_url, 15).await {
        Ok(body) => {
            let docs = body
                .pointer("/response/docs")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            if docs.is_empty() {
                tracing::info!("Manual search: Archive.org returned 0 results");
                return Ok(Vec::new());
            }

            tracing::info!(
                "Manual search: Archive.org returned {} results",
                docs.len()
            );

            let mut results = Vec::new();
            for doc in &docs {
                let identifier = doc
                    .get("identifier")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let title = doc
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Manual");
                let item_size = doc.get("item_size").and_then(|v| v.as_u64());

                if identifier.is_empty() {
                    continue;
                }

                // For Archive.org items, construct a direct PDF URL.
                // The actual PDF filename varies — we'll use the item download page
                // and let the user download from there, or try to find the PDF.
                let url = format!("https://archive.org/details/{identifier}");

                results.push(ManualRecommendation {
                    source: "archive.org".to_string(),
                    title: title.to_string(),
                    url,
                    size_bytes: item_size,
                    language: None,
                    source_id: identifier.to_string(),
                });
            }

            Ok(results)
        }
        Err(e) => {
            tracing::error!("Manual search: Archive.org request failed: {e}");
            Err(ServerFnError::new(format!(
                "Manual search unavailable: {e}"
            )))
        }
    }
}

/// Download a manual PDF from a URL and save it locally.
#[server(prefix = "/sfn")]
pub async fn download_manual(
    system: String,
    base_title: String,
    url: String,
    language: Option<String>,
) -> Result<String, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();

    // Validate inputs
    if base_title.contains("..") || base_title.contains('/') || base_title.contains('\\') {
        return Err(ServerFnError::new("Invalid title"));
    }

    let folder = replay_control_core::retrokit_manuals::manual_folder_name(&system);
    let manuals_dir = state.storage().manuals_dir().join(folder);

    // Build filename: "<base_title> (<lang>).pdf" or "<base_title>.pdf"
    let filename = if let Some(ref lang) = language {
        if lang.is_empty() {
            format!("{base_title}.pdf")
        } else {
            format!("{base_title} ({lang}).pdf")
        }
    } else {
        format!("{base_title}.pdf")
    };

    // Validate filename
    if filename.contains("..") || filename.contains('/') || filename.contains('\\') {
        return Err(ServerFnError::new("Invalid filename"));
    }

    let target_path = manuals_dir.join(&filename);

    // Encode the URL path for curl — retrokit TSV URLs often contain raw
    // spaces, parentheses, and apostrophes that curl rejects as malformed.
    let encoded_url = encode_url_path(&url);

    // Create directory if needed (blocking I/O — run off the async runtime)
    let dir = manuals_dir.clone();
    tokio::task::spawn_blocking(move || std::fs::create_dir_all(&dir))
        .await
        .map_err(|e| ServerFnError::new(format!("Task failed: {e}")))?
        .map_err(|e| ServerFnError::new(format!("Failed to create manuals directory: {e}")))?;

    // Download with curl
    tracing::info!("Downloading manual: {encoded_url} -> {}", target_path.display());

    let output = tokio::process::Command::new("curl")
        .args([
            "-sSL",
            "--max-time",
            "120",
            "-o",
            &target_path.to_string_lossy(),
            &encoded_url,
        ])
        .output()
        .await
        .map_err(|e| ServerFnError::new(format!("curl spawn failed: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Clean up partial download
        let tp = target_path.clone();
        let _ = tokio::task::spawn_blocking(move || std::fs::remove_file(&tp)).await;
        return Err(ServerFnError::new(format!("Download failed: {stderr}")));
    }

    // Verify the downloaded file exists and is not empty (blocking I/O)
    let tp = target_path.clone();
    let size = tokio::task::spawn_blocking(move || {
        std::fs::metadata(&tp).map(|m| m.len()).unwrap_or(0)
    })
    .await
    .unwrap_or(0);

    if size == 0 {
        let tp = target_path.clone();
        let _ = tokio::task::spawn_blocking(move || std::fs::remove_file(&tp)).await;
        return Err(ServerFnError::new("Downloaded file is empty"));
    }

    tracing::info!("Manual saved: {} ({} bytes)", filename, size);

    let serve_url = format!(
        "/manuals/{folder}/{}",
        urlencoding::encode(&filename)
    );
    Ok(serve_url)
}

/// Delete a previously downloaded manual PDF.
#[server(prefix = "/sfn")]
pub async fn delete_manual(system: String, filename: String) -> Result<(), ServerFnError> {
    // Path traversal protection
    if filename.contains("..") || filename.contains('/') || filename.contains('\\') {
        return Err(ServerFnError::new("Invalid filename"));
    }

    let state = expect_context::<crate::api::AppState>();
    let folder = replay_control_core::retrokit_manuals::manual_folder_name(&system).to_string();
    let target_path = state.storage().manuals_dir().join(&folder).join(&filename);

    tokio::task::spawn_blocking(move || {
        if target_path.is_file() {
            std::fs::remove_file(&target_path)
                .map_err(|e| format!("Failed to delete manual: {e}"))
        } else {
            Err("Manual file not found".to_string())
        }
    })
    .await
    .map_err(|e| ServerFnError::new(format!("Task failed: {e}")))?
    .map_err(ServerFnError::new)?;

    tracing::info!("Manual deleted: {folder}/{filename}");
    Ok(())
}

/// Load a retrokit TSV index from cache or fetch it.
#[cfg(feature = "ssr")]
async fn load_retrokit_index(
    folder: &str,
) -> Result<replay_control_core::retrokit_manuals::RetrokitIndex, String> {
    use std::sync::LazyLock;
    use std::sync::Mutex;
    use std::time::Instant;

    struct CachedIndex {
        index: replay_control_core::retrokit_manuals::RetrokitIndex,
        loaded_at: Instant,
    }

    static CACHE: LazyLock<Mutex<std::collections::HashMap<String, CachedIndex>>> =
        LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

    const TTL_SECS: u64 = 24 * 3600; // 24 hours

    // Check cache
    {
        let cache = CACHE.lock().map_err(|e| e.to_string())?;
        if let Some(entry) = cache.get(folder)
            && entry.loaded_at.elapsed().as_secs() < TTL_SECS
        {
            return Ok(entry.index.clone());
        }
    }

    // Fetch TSV
    let url = format!(
        "https://archive.org/download/retrokit-manuals/{folder}/{folder}-sources.tsv"
    );
    tracing::info!("Fetching retrokit TSV: {url}");

    let output = tokio::process::Command::new("curl")
        .args(["-sSL", "--max-time", "30", &url])
        .output()
        .await
        .map_err(|e| format!("curl spawn failed: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("TSV fetch failed: {stderr}"));
    }

    let tsv_data =
        String::from_utf8(output.stdout).map_err(|e| format!("TSV decode failed: {e}"))?;

    let index = replay_control_core::retrokit_manuals::parse_retrokit_tsv(&tsv_data);
    tracing::info!(
        "Retrokit TSV loaded: {folder} ({} titles)",
        index.len()
    );

    // Store in cache
    {
        let mut cache = CACHE.lock().map_err(|e| e.to_string())?;
        cache.insert(
            folder.to_string(),
            CachedIndex {
                index: index.clone(),
                loaded_at: Instant::now(),
            },
        );
    }

    Ok(index)
}

/// Fetch a URL with curl and parse the response as JSON.
#[cfg(feature = "ssr")]
async fn curl_get_json(url: &str, timeout_secs: u64) -> Result<serde_json::Value, String> {
    let output = tokio::process::Command::new("curl")
        .args(["-sS", "--max-time", &timeout_secs.to_string(), url])
        .output()
        .await
        .map_err(|e| format!("curl spawn failed: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("curl failed: {stderr}"));
    }
    serde_json::from_slice(&output.stdout).map_err(|e| format!("JSON parse error: {e}"))
}

/// Percent-encode unsafe characters in the path portion of a URL.
///
/// Retrokit TSV URLs often contain raw spaces, parentheses, and apostrophes
/// which curl rejects as malformed. This function encodes only the path
/// segments while preserving the scheme, host, and `/` separators.
#[cfg(feature = "ssr")]
fn encode_url_path(url: &str) -> String {
    // Find the start of the path (after "https://host")
    let path_start = if let Some(rest) = url.strip_prefix("https://") {
        rest.find('/').map(|i| i + "https://".len())
    } else if let Some(rest) = url.strip_prefix("http://") {
        rest.find('/').map(|i| i + "http://".len())
    } else {
        None
    };

    let Some(path_start) = path_start else {
        return url.to_string();
    };

    let (prefix, path) = url.split_at(path_start);

    // Encode each path segment individually, preserving '/' separators
    let encoded_path: String = path
        .split('/')
        .map(|segment| {
            // Encode only characters that are unsafe in URL paths.
            // Keep already-encoded sequences (%XX) intact.
            encode_path_segment(segment)
        })
        .collect::<Vec<_>>()
        .join("/");

    format!("{prefix}{encoded_path}")
}

/// Percent-encode a single URL path segment.
///
/// Preserves characters that are valid in URL paths (alphanumeric, `-`, `_`,
/// `.`, `~`, `!`, `*`, `:`, `@`, `+`, `,`, `;`, `=`) and already-encoded
/// `%XX` sequences. Encodes everything else (spaces, parens, apostrophes, etc.).
#[cfg(feature = "ssr")]
fn encode_path_segment(segment: &str) -> String {
    let bytes = segment.as_bytes();
    let mut result = String::with_capacity(segment.len());
    let mut i = 0;

    while i < bytes.len() {
        let b = bytes[i];

        // Preserve existing percent-encoded sequences
        if b == b'%' && i + 2 < bytes.len() {
            let hex1 = bytes[i + 1];
            let hex2 = bytes[i + 2];
            if hex1.is_ascii_hexdigit() && hex2.is_ascii_hexdigit() {
                result.push('%');
                result.push(hex1 as char);
                result.push(hex2 as char);
                i += 3;
                continue;
            }
        }

        // Characters that are safe in URL path segments (RFC 3986 unreserved + sub-delims + ':' + '@')
        if b.is_ascii_alphanumeric()
            || matches!(
                b,
                b'-' | b'_' | b'.' | b'~' | b'!' | b'*' | b':' | b'@' | b'+' | b',' | b';'
                    | b'=' | b'&'
            )
        {
            result.push(b as char);
        } else {
            // Percent-encode everything else
            result.push_str(&format!("%{b:02X}"));
        }
        i += 1;
    }

    result
}

/// Extract the base title from a manual filename (strip language suffix).
/// "Super Mario World (en)" -> "super mario world"
/// "Super Mario World" -> "super mario world"
#[cfg(feature = "ssr")]
fn extract_manual_base_title(stem: &str) -> String {
    let s = stem.trim();
    // Strip trailing " (lang)" pattern
    let stripped = if let Some(pos) = s.rfind(" (") {
        if s.ends_with(')') {
            &s[..pos]
        } else {
            s
        }
    } else {
        s
    };
    stripped.to_lowercase()
}

/// Extract language code from a manual filename.
/// "Super Mario World (en)" -> Some("en")
/// "Super Mario World" -> None
#[cfg(feature = "ssr")]
fn extract_language_from_filename(stem: &str) -> Option<String> {
    let s = stem.trim();
    if let Some(pos) = s.rfind(" (")
        && s.ends_with(')')
    {
        let lang = &s[pos + 2..s.len() - 1];
        // Sanity check: language codes are short (2-20 chars)
        if !lang.is_empty() && lang.len() <= 20 {
            return Some(lang.to_string());
        }
    }
    None
}

/// Resolve game directory from a .svm file.
/// The .svm contains a path to the game directory (absolute or relative to roms_dir).
#[cfg(feature = "ssr")]
fn resolve_svm_game_dir(
    svm_path: &std::path::Path,
    roms_dir: &std::path::Path,
) -> Option<std::path::PathBuf> {
    let content = std::fs::read_to_string(svm_path).ok()?;
    let svm_target = content.trim();
    let candidate = std::path::PathBuf::from(svm_target);
    if candidate.is_dir() {
        return Some(candidate);
    }
    let rel = roms_dir.join(svm_target);
    if rel.is_dir() {
        return Some(rel);
    }
    // Fallback: the directory containing the .svm file may BE the game directory
    svm_path.parent().filter(|p| p.is_dir()).map(|p| p.to_path_buf())
}

/// Resolve game directory from an .m3u playlist file.
/// The .m3u typically references a .svm file. We also check for a sibling
/// directory with the same base name (common ScummVM layout where the .m3u
/// sits next to the game folder).
#[cfg(feature = "ssr")]
fn resolve_m3u_game_dir(
    m3u_path: &std::path::Path,
    roms_dir: &std::path::Path,
) -> Option<std::path::PathBuf> {
    // Strategy 1: Check for a sibling directory with the same base name
    let stem = m3u_path.file_stem()?.to_str()?;
    let parent = m3u_path.parent()?;
    let sibling_dir = parent.join(stem);
    if sibling_dir.is_dir() {
        return Some(sibling_dir);
    }

    // Strategy 2: Read the .m3u and follow .svm references
    if let Ok(content) = std::fs::read_to_string(m3u_path) {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            // If this line references a .svm, resolve the game directory from it
            if line.to_lowercase().ends_with(".svm") {
                let svm_candidate = std::path::PathBuf::from(line);
                if svm_candidate.exists() {
                    if let Some(dir) = resolve_svm_game_dir(&svm_candidate, roms_dir) {
                        return Some(dir);
                    }
                }
                // Try relative to roms_dir
                let rel_svm = roms_dir.join(line);
                if rel_svm.exists() {
                    if let Some(dir) = resolve_svm_game_dir(&rel_svm, roms_dir) {
                        return Some(dir);
                    }
                }
                // Try the parent directory of the referenced .svm
                let svm_path = std::path::PathBuf::from(line);
                if let Some(svm_parent) = svm_path.parent() {
                    let dir = if svm_parent.is_absolute() {
                        svm_parent.to_path_buf()
                    } else {
                        roms_dir.join(svm_parent)
                    };
                    if dir.is_dir() {
                        return Some(dir);
                    }
                }
            }
        }
    }

    None
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use super::*;

    #[test]
    fn encode_url_path_spaces() {
        assert_eq!(
            encode_url_path("https://archive.org/download/super-baseball-2020-usa/Super Baseball 2020 (USA).pdf"),
            "https://archive.org/download/super-baseball-2020-usa/Super%20Baseball%202020%20%28USA%29.pdf"
        );
    }

    #[test]
    fn encode_url_path_zip_embedded() {
        assert_eq!(
            encode_url_path("https://archive.org/download/exov5_2/Content/XODOSMetadata.zip/Manuals/MS-DOS/10th Frame (1987).pdf"),
            "https://archive.org/download/exov5_2/Content/XODOSMetadata.zip/Manuals/MS-DOS/10th%20Frame%20%281987%29.pdf"
        );
    }

    #[test]
    fn encode_url_path_apostrophe() {
        assert_eq!(
            encode_url_path("https://archive.org/download/exov5_2/Content/XODOSMetadata.zip/Manuals/MS-DOS/'Nam 1965-1975 (1991).pdf"),
            "https://archive.org/download/exov5_2/Content/XODOSMetadata.zip/Manuals/MS-DOS/%27Nam%201965-1975%20%281991%29.pdf"
        );
    }

    #[test]
    fn encode_url_path_already_clean() {
        let url = "https://segaretro.org/images/b/be/TAoBaR_md_us_manual.pdf";
        assert_eq!(encode_url_path(url), url);
    }

    #[test]
    fn encode_url_path_preserves_existing_percent() {
        let url = "https://archive.org/download/retrokit-manuals/megadrive/Sonic%20the%20Hedgehog%20%28USA%2C%20Europe%29.pdf";
        assert_eq!(encode_url_path(url), url);
    }

    #[test]
    fn encode_url_path_ampersand_preserved() {
        let url = "https://segaretro.org/images/a/aa/The_Adventures_of_Batman_&_Robin_MD_BR_Manual.pdf";
        assert_eq!(encode_url_path(url), url);
    }
}

