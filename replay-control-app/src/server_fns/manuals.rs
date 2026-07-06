use super::*;
#[cfg(feature = "ssr")]
use replay_control_core_server::library_db::LibraryDb;
#[cfg(feature = "ssr")]
use replay_control_core_server::user_data_db::{ManualEntry, ManualOrigin, UserDataDb};

pub use replay_control_core::game_docs::GameDocument;
#[cfg(feature = "ssr")]
use replay_control_core::resource_kind;
pub use replay_control_core::retrokit_manuals::ManualRecommendation;

/// Upper bound on a downloaded manual, matching the upload cap. Bounds memory
/// and disk against an oversized or hostile (SSRF-redirected) response.
#[cfg(feature = "ssr")]
const MAX_MANUAL_DOWNLOAD_BYTES: u64 = 64 * 1024 * 1024;

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
    /// Original source URL, when this is a RePlay-owned downloaded manual.
    pub source_url: Option<String>,
    /// Provider/source that supplied the manual.
    pub provider: Option<String>,
    /// Opaque id for RePlay-owned saved manuals. Legacy/ROM-folder manuals are read-only.
    pub delete_id: Option<String>,
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
            Some(d) => replay_control_core_server::game_docs::scan_game_documents(&d),
            None => Vec::new(),
        }
    })
    .await
    .map_err(|e| ServerFnError::new(format!("Task failed: {e}")))?;

    Ok(docs)
}

/// Get locally saved manuals for a game plus read-only legacy manuals.
#[server(prefix = "/sfn")]
pub async fn get_local_manuals(
    system: String,
    base_title: String,
) -> Result<Vec<LocalManual>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();

    // Resolve alias base_titles for cross-name sharing.
    let mut all_titles = vec![base_title.clone()];
    if let Some(aliases) = state
        .library_reader
        .read({
            let system = system.clone();
            let base_title = base_title.clone();
            move |conn| LibraryDb::alias_base_titles(conn, &system, &base_title)
        })
        .await
    {
        all_titles.extend(aliases);
    }

    let saved_manuals = state
        .user_data_reader
        .read({
            let system = system.clone();
            let titles = all_titles.clone();
            move |conn| {
                let refs: Vec<&str> = titles.iter().map(String::as_str).collect();
                UserDataDb::get_game_manuals(conn, &system, &refs).unwrap_or_default()
            }
        })
        .await
        .unwrap_or_default();

    let owned_root = state.storage().rc_dir().join("manuals");
    let mut manuals: Vec<LocalManual> = saved_manuals
        .into_iter()
        .filter_map(|entry| local_manual_from_user_entry(&owned_root, entry))
        .collect();

    let folder =
        replay_control_core_server::retrokit_manuals::manual_folder_name(&system).to_string();
    let manuals_dir = state.storage().manuals_dir().join(&folder);

    // Run all blocking filesystem I/O off the async runtime to avoid stalling
    // the tokio worker pool on slow USB or NFS storage.
    let legacy_manuals = tokio::task::spawn_blocking(move || {
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

            let matches = all_titles
                .iter()
                .any(|bt| bt.eq_ignore_ascii_case(&file_base));

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
                source_url: None,
                provider: None,
                delete_id: None,
            });
        }

        manuals
    })
    .await
    .map_err(|e| ServerFnError::new(format!("Task failed: {e}")))?;

    manuals.extend(legacy_manuals);
    Ok(manuals)
}

/// Bundled/library manual suggestions copied into the library DB by enrichment.
#[server(prefix = "/sfn")]
pub async fn get_game_manual_suggestions(
    system: String,
    rom_filename: String,
    base_title: String,
) -> Result<Vec<ManualRecommendation>, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    library_manual_recommendations(&state, &system, &rom_filename, &base_title).await
}

#[cfg(feature = "ssr")]
async fn library_manual_recommendations(
    state: &crate::api::AppState,
    system: &str,
    rom_filename: &str,
    base_title: &str,
) -> Result<Vec<ManualRecommendation>, ServerFnError> {
    let preferred_langs = {
        let primary = replay_control_core_server::settings::read_language_primary(&state.settings);
        let secondary =
            replay_control_core_server::settings::read_language_secondary(&state.settings);
        let region = state.region_preference();
        replay_control_core_server::settings::preferred_languages(
            primary.as_deref(),
            secondary.as_deref(),
            region,
        )
    };

    let saved_keys = saved_manual_resource_keys(state, system, base_title).await;
    let mut results: Vec<ManualRecommendation> = state
        .library_reader
        .read({
            let system = system.to_string();
            let rom_filename = rom_filename.to_string();
            move |conn| {
                LibraryDb::game_resources(conn, &system, &rom_filename, resource_kind::MANUAL)
                    .unwrap_or_default()
            }
        })
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|row| !saved_keys.contains(&format!("url:{}", row.url)))
        .map(|row| ManualRecommendation {
            source: row.source,
            title: row.title.unwrap_or_else(|| base_title.to_string()),
            url: row.url,
            size_bytes: None,
            language: row.languages.filter(|l| !l.trim().is_empty()),
            source_id: row.resource_id,
        })
        .collect();

    results.sort_by_key(|r| {
        replay_control_core_server::settings::language_match_score(
            r.language.as_deref().unwrap_or(""),
            &preferred_langs,
        )
    });
    Ok(results)
}

/// Download a manual PDF from a URL and save it locally.
#[server(prefix = "/sfn")]
pub async fn download_manual(
    system: String,
    rom_filename: String,
    base_title: String,
    url: String,
    language: Option<String>,
    title: Option<String>,
    source: Option<String>,
) -> Result<String, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    super::require_storage_mutation_allowed(&state, "download manuals").await?;

    // Validate inputs
    if base_title.contains("..") || base_title.contains('/') || base_title.contains('\\') {
        return Err(ServerFnError::new("Invalid title"));
    }

    if rom_filename.contains("..") || rom_filename.contains('/') || rom_filename.contains('\\') {
        return Err(ServerFnError::new("Invalid ROM filename"));
    }

    let canonical_url = canonical_manual_url(&url);
    let manual_id = stable_url_id(&canonical_url);
    let safe_id = manual_id.replace(':', "_");
    let manuals_dir = state.storage().rc_dir().join("manuals").join(&system);
    let tmp_path = manuals_dir.join(format!("{safe_id}.tmp"));

    // Encode the URL path for curl — retrokit TSV URLs often contain raw
    // spaces, parentheses, and apostrophes that curl rejects as malformed.
    let encoded_url = encode_url_path(&canonical_url);

    tokio::fs::create_dir_all(&manuals_dir)
        .await
        .map_err(|e| ServerFnError::new(format!("Failed to create manuals directory: {e}")))?;

    // Download with reqwest
    tracing::info!(
        "Downloading manual: {encoded_url} -> {}",
        tmp_path.display()
    );

    let size = match replay_control_core_server::http::download_to_file(
        &encoded_url,
        &tmp_path,
        std::time::Duration::from_secs(120),
        MAX_MANUAL_DOWNLOAD_BYTES,
    )
    .await
    {
        Ok(size) => size,
        Err(e) => {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(ServerFnError::new(format!("Download failed: {e}")));
        }
    };

    if size == 0 {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(ServerFnError::new("Downloaded file is empty"));
    }

    let (extension, mime_type) = match validate_downloaded_manual(&tmp_path).await {
        Ok(v) => v,
        Err(e) => {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(ServerFnError::new(e));
        }
    };
    let filename = format!("{safe_id}.{extension}");
    let target_path = manuals_dir.join(&filename);
    if let Err(e) = tokio::fs::rename(&tmp_path, &target_path).await {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(ServerFnError::new(format!("Failed to save manual: {e}")));
    }

    let storage_path = format!("{system}/{filename}");
    let title = title
        .map(|title| title.trim().to_string())
        .filter(|title| !title.is_empty())
        .or_else(|| Some(base_title.clone()));
    let provider = source
        .map(|source| source.trim().to_string())
        .filter(|source| !source.is_empty());
    let entry = ManualEntry {
        manual_id: manual_id.clone(),
        resource_key: format!("url:{canonical_url}"),
        title,
        origin: ManualOrigin::Downloaded,
        provider,
        url: Some(canonical_url.clone()),
        storage_path: Some(storage_path.clone()),
        original_filename: Some(filename.clone()),
        languages: language.unwrap_or_default(),
        mime_type: mime_type.to_string(),
        size_bytes: Some(size),
        added_at: unix_now_secs(),
    };
    let db_result = state
        .user_data_writer
        .try_write({
            let system = system.clone();
            let rom_filename = rom_filename.clone();
            let base_title = base_title.clone();
            move |conn| {
                UserDataDb::add_game_manual(conn, &system, &rom_filename, &base_title, &entry)
            }
        })
        .await;
    match db_result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            let _ = tokio::fs::remove_file(&target_path).await;
            return Err(ServerFnError::new(format!(
                "Failed to save manual metadata: {e}"
            )));
        }
        Err(e) => {
            let _ = tokio::fs::remove_file(&target_path).await;
            return Err(ServerFnError::new(format!(
                "User data database unavailable: {e}"
            )));
        }
    }

    tracing::info!("Manual saved: {} ({} bytes)", filename, size);

    let serve_url = format!("/owned-manuals/{}", urlencoding::encode(&storage_path));
    Ok(serve_url)
}

/// Delete a RePlay-owned saved manual.
#[server(prefix = "/sfn")]
pub async fn delete_manual(system: String, manual_id: String) -> Result<(), ServerFnError> {
    // Path traversal protection
    if manual_id.contains("..") || manual_id.contains('/') || manual_id.contains('\\') {
        return Err(ServerFnError::new("Invalid manual id"));
    }

    let state = expect_context::<crate::api::AppState>();
    super::require_storage_mutation_allowed(&state, "delete manuals").await?;

    let removed = state
        .user_data_writer
        .try_write({
            let system = system.clone();
            let manual_id = manual_id.clone();
            move |conn| UserDataDb::remove_game_manual(conn, &system, &manual_id)
        })
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))?
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    let Some(entry) = removed else {
        return Err(ServerFnError::new("Manual file not found"));
    };
    if let Some(rel) = entry.storage_path
        && let Some(target_path) =
            safe_owned_manual_path(&state.storage().rc_dir().join("manuals"), &rel)
    {
        match tokio::fs::remove_file(&target_path).await {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(ServerFnError::new(format!("Failed to delete manual: {e}"))),
        }
    }

    tracing::info!("Manual deleted: {manual_id}");
    Ok(())
}

/// Percent-encode unsafe characters in the path portion of a URL.
///
/// Retrokit TSV URLs often contain raw spaces, parentheses, and apostrophes
/// which curl rejects as malformed. This function encodes only the path
/// segments while preserving the scheme, host, and `/` separators.
#[cfg(feature = "ssr")]
fn encode_url_path(url: &str) -> String {
    let url = canonical_manual_url(url);
    // Find the start of the path (after "https://host")
    let path_start = if let Some(rest) = url.strip_prefix("https://") {
        rest.find('/').map(|i| i + "https://".len())
    } else if let Some(rest) = url.strip_prefix("http://") {
        rest.find('/').map(|i| i + "http://".len())
    } else {
        None
    };

    let Some(path_start) = path_start else {
        return url;
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

#[cfg(feature = "ssr")]
fn canonical_manual_url(url: &str) -> String {
    let sonicretro_image_path = url
        .strip_prefix("http://info.sonicretro.org/images/")
        .or_else(|| url.strip_prefix("https://info.sonicretro.org/images/"));

    if let Some(path) = sonicretro_image_path {
        return match path {
            // The SonicRetro file page still exists, but the local media URL
            // returns 404. The file is available through the linked CDN mirror.
            "6/6e/Sonic_Blast_GG_US_Manual.pdf" => {
                "https://retrocdn.net/images/6/6e/Sonic_Blast_GG_US_Manual.pdf".to_string()
            }
            _ => format!("https://info.sonicretro.org/images/{path}"),
        };
    }

    url.to_string()
}

#[cfg(feature = "ssr")]
fn local_manual_from_user_entry(
    owned_root: &std::path::Path,
    entry: ManualEntry,
) -> Option<LocalManual> {
    let rel = entry.storage_path.as_deref()?;
    let path = safe_owned_manual_path(owned_root, rel)?;
    let filename = entry
        .original_filename
        .clone()
        .unwrap_or_else(|| entry.manual_id.clone());
    let label = entry.title.unwrap_or_else(|| filename.clone());
    let size_bytes = entry
        .size_bytes
        .or_else(|| std::fs::metadata(&path).ok().map(|m| m.len()))
        .unwrap_or(0);
    let language = if entry.languages.trim().is_empty() {
        None
    } else {
        Some(entry.languages)
    };
    Some(LocalManual {
        filename,
        label,
        size_bytes,
        language,
        url: format!("/owned-manuals/{}", urlencoding::encode(rel)),
        source_url: entry.url,
        provider: entry.provider,
        delete_id: Some(entry.manual_id),
    })
}

#[cfg(feature = "ssr")]
async fn saved_manual_resource_keys(
    state: &crate::api::AppState,
    system: &str,
    base_title: &str,
) -> std::collections::HashSet<String> {
    let mut all_titles = vec![base_title.to_string()];
    if let Some(aliases) = state
        .library_reader
        .read({
            let system = system.to_string();
            let base_title = base_title.to_string();
            move |conn| LibraryDb::alias_base_titles(conn, &system, &base_title)
        })
        .await
    {
        all_titles.extend(aliases);
    }
    state
        .user_data_reader
        .read({
            let system = system.to_string();
            let titles = all_titles.clone();
            move |conn| {
                let refs: Vec<&str> = titles.iter().map(String::as_str).collect();
                UserDataDb::get_game_manuals(conn, &system, &refs)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|m| m.resource_key)
                    .collect::<std::collections::HashSet<_>>()
            }
        })
        .await
        .unwrap_or_default()
}

#[cfg(feature = "ssr")]
async fn validate_downloaded_manual(
    path: &std::path::Path,
) -> Result<(&'static str, &'static str), String> {
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|e| format!("Failed to validate manual: {e}"))?;
    if bytes.starts_with(b"%PDF-") {
        return Ok(("pdf", "application/pdf"));
    }
    if std::str::from_utf8(&bytes).is_ok() {
        return Ok(("txt", "text/plain"));
    }
    Err("Downloaded file is not an allowed manual type (PDF or text).".to_string())
}

#[cfg(feature = "ssr")]
fn safe_owned_manual_path(owned_root: &std::path::Path, rel: &str) -> Option<std::path::PathBuf> {
    let rel_path = std::path::Path::new(rel);
    if rel_path.is_absolute()
        || rel_path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return None;
    }
    Some(owned_root.join(rel_path))
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
                b'-' | b'_'
                    | b'.'
                    | b'~'
                    | b'!'
                    | b'*'
                    | b':'
                    | b'@'
                    | b'+'
                    | b','
                    | b';'
                    | b'='
                    | b'&'
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
        if s.ends_with(')') { &s[..pos] } else { s }
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
    svm_path
        .parent()
        .filter(|p| p.is_dir())
        .map(|p| p.to_path_buf())
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
                if svm_candidate.exists()
                    && let Some(dir) = resolve_svm_game_dir(&svm_candidate, roms_dir)
                {
                    return Some(dir);
                }
                // Try relative to roms_dir
                let rel_svm = roms_dir.join(line);
                if rel_svm.exists()
                    && let Some(dir) = resolve_svm_game_dir(&rel_svm, roms_dir)
                {
                    return Some(dir);
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
            encode_url_path(
                "https://archive.org/download/super-baseball-2020-usa/Super Baseball 2020 (USA).pdf"
            ),
            "https://archive.org/download/super-baseball-2020-usa/Super%20Baseball%202020%20%28USA%29.pdf"
        );
    }

    #[test]
    fn encode_url_path_zip_embedded() {
        assert_eq!(
            encode_url_path(
                "https://archive.org/download/exov5_2/Content/XODOSMetadata.zip/Manuals/MS-DOS/10th Frame (1987).pdf"
            ),
            "https://archive.org/download/exov5_2/Content/XODOSMetadata.zip/Manuals/MS-DOS/10th%20Frame%20%281987%29.pdf"
        );
    }

    #[test]
    fn encode_url_path_apostrophe() {
        assert_eq!(
            encode_url_path(
                "https://archive.org/download/exov5_2/Content/XODOSMetadata.zip/Manuals/MS-DOS/'Nam 1965-1975 (1991).pdf"
            ),
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
        let url =
            "https://segaretro.org/images/a/aa/The_Adventures_of_Batman_&_Robin_MD_BR_Manual.pdf";
        assert_eq!(encode_url_path(url), url);
    }

    #[test]
    fn canonical_manual_url_rewrites_legacy_sonicretro_host() {
        assert_eq!(
            canonical_manual_url("http://info.sonicretro.org/images/0/0a/Sonic3_MD_JP_manual.pdf"),
            "https://info.sonicretro.org/images/0/0a/Sonic3_MD_JP_manual.pdf"
        );
    }

    #[test]
    fn encode_url_path_rewrites_legacy_sonicretro_host() {
        assert_eq!(
            encode_url_path("http://info.sonicretro.org/images/4/40/Chaotix_32X_JP_manual.pdf"),
            "https://info.sonicretro.org/images/4/40/Chaotix_32X_JP_manual.pdf"
        );
    }

    #[test]
    fn canonical_manual_url_repairs_stale_sonic_blast_media_url() {
        assert_eq!(
            canonical_manual_url(
                "http://info.sonicretro.org/images/6/6e/Sonic_Blast_GG_US_Manual.pdf"
            ),
            "https://retrocdn.net/images/6/6e/Sonic_Blast_GG_US_Manual.pdf"
        );
    }
}
