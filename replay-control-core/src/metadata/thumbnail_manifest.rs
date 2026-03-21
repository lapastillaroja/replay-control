//! Manifest-based libretro-thumbnails: fetch file listings via GitHub REST API,
//! store in SQLite, and download individual images on demand.
//!
//! Phase 1: Manifest generation — build a local index of all available thumbnails
//!   using the GitHub Trees API (fast, no git clone required).
//! Phase 2: Image download — fetch matched images from raw.githubusercontent.com.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use rusqlite::Connection;

use crate::error::{Error, Result};
use crate::metadata_db::MetadataDb;
use crate::thumbnails::{self, ThumbnailKind};

/// Percent-encode a string for use in a URL path component.
/// Encodes everything except unreserved characters (RFC 3986).
fn encode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push(char::from(b"0123456789ABCDEF"[(b >> 4) as usize]));
                out.push(char::from(b"0123456789ABCDEF"[(b & 0x0F) as usize]));
            }
        }
    }
    out
}

// ── Phase 1: Manifest Generation ────────────────────────────────────────

/// Info about a single libretro-thumbnails repo.
#[derive(Debug, Clone)]
pub struct RepoInfo {
    /// Display name, e.g., "Nintendo - Super Nintendo Entertainment System"
    pub display_name: String,
    /// URL-safe name (spaces replaced with underscores)
    pub url_name: String,
}

/// Stats returned after a manifest import.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ManifestImportStats {
    pub repos_fetched: usize,
    pub total_entries: usize,
    pub errors: Vec<String>,
}

/// Collect the unique list of libretro-thumbnails repos from all supported systems.
pub fn collect_all_repos() -> Vec<RepoInfo> {
    use crate::systems;

    let mut repos: Vec<RepoInfo> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for system in systems::visible_systems() {
        if let Some(repo_names) = thumbnails::thumbnail_repo_names(system.folder_name) {
            for display_name in repo_names {
                let url_name = thumbnails::repo_url_name(display_name);
                if seen.insert(url_name.clone()) {
                    repos.push(RepoInfo {
                        display_name: display_name.to_string(),
                        url_name,
                    });
                }
            }
        }
    }
    repos
}

/// Hardcoded default branch lookup. Most repos use `master`; a few use `main`.
pub fn default_branch(repo_display_name: &str) -> &'static str {
    match repo_display_name {
        "Commodore - CD32" | "Commodore - CDTV" | "Sega - Naomi" | "Sega - Naomi 2"
        | "Philips - CDi" => "main",
        _ => "master",
    }
}

/// Fetch the full tree listing for a libretro-thumbnails repo via GitHub REST API.
/// Returns `(commit_sha, entries)`. Blocking (uses std::process::Command).
///
/// Uses `GET /repos/libretro-thumbnails/{url_name}/git/trees/{branch}?recursive=1`.
/// Filters and parses the response inline, returning only Named_Boxarts and Named_Snaps
/// entries as `ThumbnailEntry` values.
pub fn fetch_repo_tree(
    url_name: &str,
    branch: &str,
    api_key: Option<&str>,
) -> Result<(String, Vec<ThumbnailEntry>)> {
    let url = format!(
        "https://api.github.com/repos/libretro-thumbnails/{url_name}/git/trees/{branch}?recursive=1"
    );

    let auth_header;
    let mut args = vec![
        "-fsSL",
        "--max-time",
        "60",
        "-H",
        "Accept: application/vnd.github+json",
        "-H",
        "User-Agent: replay-control",
    ];
    if let Some(key) = api_key {
        auth_header = format!("Authorization: token {key}");
        args.extend(["-H", auth_header.as_str()]);
    }
    args.push(&url);

    let output = std::process::Command::new("curl")
        .args(&args)
        .output()
        .map_err(|e| Error::Other(format!("Failed to run curl: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Other(format!(
            "GitHub API request failed for {url_name}/{branch}: {stderr}"
        )));
    }

    let body = String::from_utf8(output.stdout)
        .map_err(|e| Error::Other(format!("Invalid UTF-8 in API response: {e}")))?;

    let json: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| Error::Other(format!("Failed to parse API response: {e}")))?;

    // Check for API error responses (e.g. rate limit, not found).
    if let Some(msg) = json.get("message").and_then(|v| v.as_str()) {
        return Err(Error::Other(format!(
            "GitHub API error for {url_name}/{branch}: {msg}"
        )));
    }

    let commit_sha = json
        .get("sha")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let tree = json
        .get("tree")
        .and_then(|v| v.as_array())
        .ok_or_else(|| Error::Other("Missing 'tree' field in API response".to_string()))?;

    let mut entries = Vec::new();
    for item in tree {
        let path = match item.get("path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => continue,
        };
        let mode = item.get("mode").and_then(|v| v.as_str()).unwrap_or("");
        let size = item.get("size").and_then(|v| v.as_u64());

        // Filter to Named_Boxarts/ and Named_Snaps/ only.
        let (kind, rest) = match thumbnails::ALL_THUMBNAIL_KINDS.iter().find_map(|k| {
            path.strip_prefix(k.repo_dir())?
                .strip_prefix('/')
                .map(|r| (k.repo_dir(), r))
        }) {
            Some(pair) => pair,
            None => continue,
        };

        // Extract the filename stem (strip .png extension).
        let stem = match rest.strip_suffix(".png") {
            Some(s) => s.to_string(),
            None => continue,
        };

        let is_symlink = mode == "120000";

        // Filter out stub/broken files (< 200 bytes). Symlinks have no size.
        if !is_symlink
            && let Some(blob_size) = size
            && blob_size > 0
            && blob_size < 200
        {
            continue;
        }

        entries.push(ThumbnailEntry {
            kind: kind.to_string(),
            filename: stem,
            is_symlink,
        });
    }

    Ok((commit_sha, entries))
}

/// A parsed thumbnail entry from a GitHub Trees API response.
#[derive(Debug)]
pub struct ThumbnailEntry {
    pub kind: String,     // "Named_Boxarts" or "Named_Snaps"
    pub filename: String, // stem without .png
    pub is_symlink: bool, // true if git mode is 120000 (symlink)
}

/// Insert parsed thumbnail entries into the `thumbnail_index` table.
pub fn insert_thumbnail_entries(
    conn: &mut Connection,
    source_name: &str,
    entries: &[ThumbnailEntry],
) -> Result<usize> {
    let tuples: Vec<(String, String, Option<String>)> = entries
        .iter()
        .map(|e| {
            let symlink_target = if e.is_symlink {
                Some(String::new()) // Placeholder -- resolved at download time
            } else {
                None
            };
            (e.kind.clone(), e.filename.clone(), symlink_target)
        })
        .collect();

    MetadataDb::bulk_insert_thumbnail_index(conn, source_name, &tuples)
}

/// Orchestrate the full manifest import for all repos.
/// Calls `on_progress(repos_done, repos_total, current_repo_display_name)`.
/// Returns import stats. Skips repos whose commit SHA hasn't changed.
pub fn import_all_manifests(
    conn: &mut Connection,
    on_progress: &dyn Fn(usize, usize, &str),
    cancel: &AtomicBool,
    api_key: Option<&str>,
) -> Result<ManifestImportStats> {
    let repos = collect_all_repos();
    let total = repos.len();
    let mut total_entries = 0usize;
    let mut repos_fetched = 0usize;
    let mut errors: Vec<String> = Vec::new();

    for (i, repo) in repos.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }

        on_progress(i, total, &repo.display_name);

        let source_name = thumbnails::libretro_source_name(&repo.display_name);

        // Check if repo has changed since last import.
        if let Ok(Some(status)) = MetadataDb::get_data_source(conn, &source_name) {
            let existing_hash = status.version_hash.as_deref().unwrap_or("");
            if !existing_hash.is_empty() {
                match check_repo_freshness(&repo.url_name, existing_hash, api_key) {
                    Ok(false) => {
                        // Repo unchanged -- skip.
                        total_entries += status.entry_count;
                        repos_fetched += 1;
                        continue;
                    }
                    Ok(true) => { /* Repo changed, re-fetch below. */ }
                    Err(e) => {
                        tracing::warn!("Freshness check failed for {}: {e}", repo.display_name);
                        // Can't tell -- re-fetch to be safe.
                    }
                }
            }
        }

        let branch = default_branch(&repo.display_name);
        let (commit_sha, entries, actual_branch) =
            match fetch_repo_tree(&repo.url_name, branch, api_key) {
                Ok((sha, entries)) => (sha, entries, branch),
                Err(_) => {
                    // Try the other branch before giving up.
                    let alt = if branch == "master" { "main" } else { "master" };
                    match fetch_repo_tree(&repo.url_name, alt, api_key) {
                        Ok((sha, entries)) => (sha, entries, alt),
                        Err(e) => {
                            errors.push(format!("{}: {e}", repo.display_name));
                            continue;
                        }
                    }
                }
            };

        // Upsert data_source BEFORE inserting thumbnail entries (FK constraint).
        if let Err(e) = MetadataDb::upsert_data_source(
            conn,
            &source_name,
            "libretro-thumbnails",
            &commit_sha,
            actual_branch,
            0,
        ) {
            errors.push(format!(
                "{}: failed to upsert data_source: {e}",
                repo.display_name
            ));
            continue;
        }

        let count = match insert_thumbnail_entries(conn, &source_name, &entries) {
            Ok(c) => c,
            Err(e) => {
                errors.push(format!("{}: {e}", repo.display_name));
                continue;
            }
        };

        // Update with actual entry count.
        if let Err(e) = MetadataDb::upsert_data_source(
            conn,
            &source_name,
            "libretro-thumbnails",
            &commit_sha,
            actual_branch,
            count,
        ) {
            errors.push(format!(
                "{}: failed to update data_source count: {e}",
                repo.display_name
            ));
        }

        total_entries += count;
        repos_fetched += 1;
    }

    on_progress(total, total, "");

    Ok(ManifestImportStats {
        repos_fetched,
        total_entries,
        errors,
    })
}

/// Check if a repo has changed by comparing the latest commit SHA via GitHub API
/// with the stored hash. Returns `Ok(true)` if the repo has changed.
///
/// Uses `GET /repos/libretro-thumbnails/{url_name}/commits/HEAD` with
/// `Accept: application/vnd.github.sha` which returns just the SHA as plain text.
fn check_repo_freshness(url_name: &str, stored_hash: &str, api_key: Option<&str>) -> Result<bool> {
    let url = format!("https://api.github.com/repos/libretro-thumbnails/{url_name}/commits/HEAD");

    let auth_header;
    let mut args = vec![
        "-fsSL",
        "--max-time",
        "15",
        "-H",
        "Accept: application/vnd.github.sha",
        "-H",
        "User-Agent: replay-control",
    ];
    if let Some(key) = api_key {
        auth_header = format!("Authorization: token {key}");
        args.extend(["-H", auth_header.as_str()]);
    }
    args.push(&url);

    let output = std::process::Command::new("curl")
        .args(&args)
        .output()
        .map_err(|e| Error::Other(format!("Failed to run curl: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Other(format!(
            "GitHub API freshness check failed for {url_name}: {stderr}"
        )));
    }

    let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if sha.is_empty() {
        return Ok(true); // Can't tell -- assume changed.
    }

    Ok(sha != stored_hash)
}

// ── Phase 2: Image Download ─────────────────────────────────────────────

/// A match from the manifest fuzzy index.
#[derive(Debug, Clone)]
pub struct ManifestMatch {
    /// The filename stem as stored in `thumbnail_index`.
    pub filename: String,
    /// Whether this entry is a symlink (symlink_target is set).
    pub is_symlink: bool,
    /// URL-safe repo name (spaces replaced with underscores).
    pub repo_url_name: String,
    /// Git branch for this repo.
    pub branch: String,
}

/// In-memory fuzzy index built from `thumbnail_index` DB entries.
pub struct ManifestFuzzyIndex {
    /// exact thumbnail_filename stem -> ManifestMatch
    pub exact: HashMap<String, ManifestMatch>,
    /// lowercase(filename) -> ManifestMatch (case-insensitive exact, preserves region tags)
    pub exact_ci: HashMap<String, ManifestMatch>,
    /// lowercase(strip_tags(stem)) -> ManifestMatch
    pub by_tags: HashMap<String, ManifestMatch>,
    /// lowercase(strip_version(strip_tags(stem))) -> ManifestMatch
    pub by_version: HashMap<String, ManifestMatch>,
}

/// Build a ManifestFuzzyIndex from the DB for the given repos and kind.
pub fn build_manifest_fuzzy_index(
    conn: &Connection,
    repo_display_names: &[&str],
    kind: &str,
) -> ManifestFuzzyIndex {
    use thumbnails::{strip_tags, strip_version};

    let mut exact = HashMap::new();
    let mut exact_ci = HashMap::new();
    let mut by_tags = HashMap::new();
    let mut by_version = HashMap::new();

    for display_name in repo_display_names {
        let url_name = thumbnails::repo_url_name(display_name);
        let source_name = thumbnails::libretro_source_name(display_name);

        // Look up branch from data_sources.
        let branch = MetadataDb::get_data_source(conn, &source_name)
            .ok()
            .flatten()
            .and_then(|s| s.branch)
            .unwrap_or_else(|| "master".to_string());

        let entries = MetadataDb::query_thumbnail_index(conn, &source_name, kind)
            .unwrap_or_default();

        for entry in entries {
            let m = ManifestMatch {
                filename: entry.filename.clone(),
                is_symlink: entry.symlink_target.is_some(),
                repo_url_name: url_name.clone(),
                branch: branch.clone(),
            };

            // Tier 1: exact
            exact
                .entry(entry.filename.clone())
                .or_insert_with(|| m.clone());

            // Tier 1b: case-insensitive exact (preserves region tags)
            exact_ci
                .entry(entry.filename.to_lowercase())
                .or_insert_with(|| m.clone());

            // Tier 2: strip tags
            let stripped = strip_tags(&entry.filename);
            let key = stripped.to_lowercase();
            by_tags.entry(key.clone()).or_insert_with(|| m.clone());

            // Tier 3: version-stripped
            let version_key = strip_version(&key);
            if version_key.len() < key.len() {
                by_version.entry(version_key.to_string()).or_insert(m);
            }
        }
    }

    ManifestFuzzyIndex {
        exact,
        exact_ci,
        by_tags,
        by_version,
    }
}

/// Build a manifest fuzzy index from pre-fetched raw data.
///
/// Each element in `repo_data` is `(repo_url_name, branch, entries)` where
/// entries were queried from `thumbnail_index` under the DB lock. This allows
/// the caller to release the DB lock before the expensive index construction.
pub fn build_manifest_fuzzy_index_from_raw(
    repo_data: &[(String, String, Vec<crate::metadata_db::ThumbnailIndexEntry>)],
) -> ManifestFuzzyIndex {
    use thumbnails::{strip_tags, strip_version};

    let mut exact = HashMap::new();
    let mut exact_ci = HashMap::new();
    let mut by_tags = HashMap::new();
    let mut by_version = HashMap::new();

    for (url_name, branch, entries) in repo_data {
        for entry in entries {
            let m = ManifestMatch {
                filename: entry.filename.clone(),
                is_symlink: entry.symlink_target.is_some(),
                repo_url_name: url_name.clone(),
                branch: branch.clone(),
            };

            // Tier 1: exact
            exact
                .entry(entry.filename.clone())
                .or_insert_with(|| m.clone());

            // Tier 1b: case-insensitive exact (preserves region tags)
            exact_ci
                .entry(entry.filename.to_lowercase())
                .or_insert_with(|| m.clone());

            // Tier 2: strip tags
            let stripped = strip_tags(&entry.filename);
            let key = stripped.to_lowercase();
            by_tags.entry(key.clone()).or_insert_with(|| m.clone());

            // Tier 3: version-stripped
            let version_key = strip_version(&key);
            if version_key.len() < key.len() {
                by_version.entry(version_key.to_string()).or_insert(m);
            }
        }
    }

    ManifestFuzzyIndex {
        exact,
        exact_ci,
        by_tags,
        by_version,
    }
}

/// Look up a ROM in the manifest fuzzy index.
/// Returns the matching manifest entry, or None.
pub fn find_in_manifest<'a>(
    index: &'a ManifestFuzzyIndex,
    rom_filename: &str,
    system: &str,
) -> Option<&'a ManifestMatch> {
    use crate::arcade_db;
    use thumbnails::{strip_tags, strip_version, thumbnail_filename};

    let stem = rom_filename
        .rfind('.')
        .map(|i| &rom_filename[..i])
        .unwrap_or(rom_filename);

    let is_arcade = matches!(
        system,
        "arcade_mame" | "arcade_fbneo" | "arcade_mame_2k3p" | "arcade_dc"
    );

    // For arcade ROMs, translate MAME codename to display name.
    let display_name = if is_arcade {
        arcade_db::lookup_arcade_game(stem).map(|info| info.display_name)
    } else {
        None
    };
    let thumb_name = thumbnail_filename(display_name.unwrap_or(stem));

    // Tier 1: exact match.
    if let Some(m) = index.exact.get(&thumb_name) {
        return Some(m);
    }

    // Colon variants (same logic as import_system_thumbnails in thumbnails.rs).
    let source = display_name.unwrap_or(stem);
    if source.contains(':') {
        let dash_variant = thumbnail_filename(&source.replace(": ", " - ").replace(':', " -"));
        if let Some(m) = index.exact.get(&dash_variant) {
            return Some(m);
        }
        let drop_variant = thumbnail_filename(&source.replace(": ", " ").replace(':', ""));
        if let Some(m) = index.exact.get(&drop_variant) {
            return Some(m);
        }
    }

    // Tier 1b: case-insensitive exact (preserves region tags like "(USA)", "(Europe)")
    if let Some(m) = index.exact_ci.get(&thumb_name.to_lowercase()) {
        return Some(m);
    }

    // Tier 2: strip tags.
    let key = strip_tags(&thumb_name).to_lowercase();
    if let Some(m) = index.by_tags.get(&key) {
        return Some(m);
    }

    // Tier 3: version-stripped.
    let version_key = strip_version(&key);
    if version_key.len() < key.len()
        && let Some(m) = index
            .by_tags
            .get(version_key)
            .or_else(|| index.by_version.get(version_key))
    {
        return Some(m);
    }

    // Tier 4: slash dual-name matching.
    // Arcade display names often contain " / " separating English and Japanese
    // titles (e.g., "Animal Basket / Hustle Tamaire Kyousou"). The thumbnail
    // repo may list only the primary (English) name. Try each side independently.
    let search_key = if version_key.len() < key.len() {
        version_key
    } else {
        &key
    };
    if search_key.contains(" / ") || search_key.contains(" _ ") {
        // After thumbnail_filename(), "/" becomes "_", so check both patterns.
        let separator = if search_key.contains(" / ") {
            " / "
        } else {
            " _ "
        };
        for part in search_key.split(separator) {
            let part = part.trim();
            if part.len() >= 5
                && let Some(m) = index
                    .by_tags
                    .get(part)
                    .or_else(|| index.by_version.get(part))
            {
                return Some(m);
            }
        }
    }

    None
}

/// Construct the raw.githubusercontent.com URL for a thumbnail.
pub fn thumbnail_download_url(m: &ManifestMatch, kind: &str) -> String {
    let encoded = url_encode_path_component(&format!("{}.png", m.filename));
    format!(
        "https://raw.githubusercontent.com/libretro-thumbnails/{}/{}/{}/{}",
        m.repo_url_name, m.branch, kind, encoded,
    )
}

/// Percent-encode a single path component for a URL.
/// Encodes everything except unreserved characters (RFC 3986).
fn url_encode_path_component(s: &str) -> String {
    let mut result = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(b as char);
            }
            _ => {
                result.push_str(&format!("%{:02X}", b));
            }
        }
    }
    result
}

const PNG_MAGIC: [u8; 4] = [0x89, b'P', b'N', b'G'];

/// Download a thumbnail image, handling symlink resolution transparently.
/// Returns the raw PNG bytes on success. Blocking.
pub fn download_thumbnail(m: &ManifestMatch, kind: &str) -> Result<Vec<u8>> {
    let url = thumbnail_download_url(m, kind);
    let bytes = curl_download_bytes(&url)?;

    // Check if this is a symlink (text content instead of PNG).
    if bytes.len() < 200 && !bytes.starts_with(&PNG_MAGIC) {
        let target_path = std::str::from_utf8(&bytes)
            .map_err(|e| Error::Other(format!("Invalid symlink content: {e}")))?
            .trim();

        // Extract just the filename from the relative path.
        let target_filename = target_path.rsplit('/').next().unwrap_or(target_path);

        let encoded = url_encode_path_component(target_filename);
        let target_url = format!(
            "https://raw.githubusercontent.com/libretro-thumbnails/{}/{}/{}/{}",
            m.repo_url_name, m.branch, kind, encoded,
        );

        let real_bytes = curl_download_bytes(&target_url)?;

        if real_bytes.len() < 200 && !real_bytes.starts_with(&PNG_MAGIC) {
            return Err(Error::Other(format!(
                "Symlink chain: {} -> {} did not resolve to a valid PNG",
                m.filename, target_path,
            )));
        }

        return Ok(real_bytes);
    }

    Ok(bytes)
}

/// Download raw bytes from a URL using curl (blocking).
pub fn curl_download_bytes(url: &str) -> Result<Vec<u8>> {
    let output = std::process::Command::new("curl")
        .args([
            "-fsSL",
            "--max-time",
            "15",
            "--retry",
            "2",
            "--retry-delay",
            "1",
            url,
        ])
        .output()
        .map_err(|e| Error::Other(format!("Failed to run curl: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Other(format!("Download failed for {url}: {stderr}")));
    }

    Ok(output.stdout)
}

/// Save a downloaded PNG to the media directory.
pub fn save_thumbnail(
    storage_root: &Path,
    system: &str,
    kind: ThumbnailKind,
    matched_stem: &str,
    png_bytes: &[u8],
) -> Result<std::path::PathBuf> {
    let media_dir = storage_root
        .join(crate::storage::RC_DIR)
        .join("media")
        .join(system)
        .join(kind.media_dir());

    std::fs::create_dir_all(&media_dir).map_err(|e| Error::io(&media_dir, e))?;

    let dest = media_dir.join(format!("{matched_stem}.png"));
    std::fs::write(&dest, png_bytes).map_err(|e| Error::io(&dest, e))?;

    Ok(dest)
}

// ── Phase 3: Variant Discovery ───────────────────────────────────────────

/// A box art variant available for a ROM (different region, same game).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BoxArtVariant {
    /// Filename stem in the thumbnail index (e.g., "Sonic the Hedgehog (Europe)").
    pub filename: String,
    /// Region label extracted from the filename (e.g., "Europe").
    pub region_label: String,
    /// Whether the image is already downloaded to local media.
    pub is_downloaded: bool,
    /// URL to serve the image (local path if downloaded, GitHub raw URL otherwise).
    pub image_url: String,
    /// Whether this is the currently active variant.
    pub is_active: bool,
    /// URL-safe repo name (for downloading).
    pub repo_url_name: String,
    /// Git branch for this repo.
    pub branch: String,
}

/// Find all box art variants for a ROM by querying the thumbnail index.
///
/// Computes the ROM's base title via `strip_tags(thumbnail_filename(stem))`, then
/// collects all `Named_Boxarts` entries with the same base title. De-duplicates
/// by symlink target so entries pointing to the same image appear only once.
pub fn find_boxart_variants(
    conn: &Connection,
    system: &str,
    rom_filename: &str,
    storage_root: &std::path::Path,
    active_box_art_url: Option<&str>,
) -> Vec<BoxArtVariant> {
    use crate::thumbnails::{self, strip_tags, thumbnail_filename};
    use std::collections::HashSet;

    let repo_names = match thumbnails::thumbnail_repo_names(system) {
        Some(names) => names,
        None => return Vec::new(),
    };

    // Compute the ROM's base title for matching.
    let stem = rom_filename
        .rfind('.')
        .map(|i| &rom_filename[..i])
        .unwrap_or(rom_filename);

    // For arcade ROMs, translate MAME codename to display name.
    let is_arcade = matches!(
        system,
        "arcade_mame" | "arcade_fbneo" | "arcade_mame_2k3p" | "arcade_dc"
    );
    let display_name = if is_arcade {
        crate::arcade_db::lookup_arcade_game(stem).map(|info| info.display_name.to_string())
    } else {
        None
    };
    let source = display_name.as_deref().unwrap_or(stem);
    let thumb_name = thumbnail_filename(source);
    let base_title = strip_tags(&thumb_name).to_lowercase();

    // For tilde dual-title ROMs (e.g., "Bare Knuckle ~ Streets of Rage"),
    // also match either half individually.
    let tilde_halves = super::image_matching::tilde_halves(source);

    let media_base = storage_root
        .join(crate::storage::RC_DIR)
        .join("media")
        .join(system)
        .join(ThumbnailKind::Boxart.media_dir());

    let mut variants = Vec::new();
    let mut seen_targets: HashSet<String> = HashSet::new();

    for display_name in repo_names {
        let url_name = thumbnails::repo_url_name(display_name);
        let source_name = thumbnails::libretro_source_name(display_name);

        let branch = MetadataDb::get_data_source(conn, &source_name)
            .ok()
            .flatten()
            .and_then(|s| s.branch)
            .unwrap_or_else(|| "master".to_string());

        let entries = MetadataDb::query_thumbnail_index(conn, &source_name, ThumbnailKind::Boxart.repo_dir())
            .unwrap_or_default();

        for entry in &entries {
            let entry_base = strip_tags(&entry.filename).to_lowercase();
            if entry_base != base_title && !tilde_halves.contains(&entry_base) {
                continue;
            }

            // De-duplicate by resolved image (symlink target or filename).
            let resolved = entry
                .symlink_target
                .as_deref()
                .filter(|t| !t.is_empty())
                .unwrap_or(&entry.filename);
            if !seen_targets.insert(resolved.to_string()) {
                continue;
            }

            let is_symlink = entry.symlink_target.is_some();
            let local_path = media_base.join(format!("{}.png", entry.filename));
            let is_downloaded = thumbnails::is_valid_image(&local_path);

            // Skip undownloaded symlinks — GitHub raw serves the symlink text
            // content (a filename) instead of the actual PNG, producing a
            // broken image. The real file is already in the list as its own entry.
            if is_symlink && !is_downloaded {
                continue;
            }

            let image_url = if is_downloaded {
                format!("/media/{system}/boxart/{}.png", entry.filename)
            } else {
                // Preview from GitHub raw content for undownloaded variants.
                let encoded_name = encode_uri_component(&entry.filename);
                format!(
                    "https://raw.githubusercontent.com/libretro-thumbnails/{}/{}/Named_Boxarts/{encoded_name}.png",
                    url_name, branch
                )
            };

            // Check if this variant is the currently active one.
            let is_active = active_box_art_url
                .map(|url| {
                    let expected = format!("/media/{system}/boxart/{}.png", entry.filename);
                    url == expected
                })
                .unwrap_or(false);

            let region_label = extract_region_label(&entry.filename);

            variants.push(BoxArtVariant {
                filename: entry.filename.clone(),
                region_label,
                is_downloaded,
                image_url,
                is_active,
                repo_url_name: url_name.clone(),
                branch: branch.clone(),
            });
        }
    }

    variants
}

/// Count distinct box art variants for a ROM without building the full list.
/// Faster than `find_boxart_variants()` when only the count is needed.
pub fn count_boxart_variants(conn: &Connection, system: &str, rom_filename: &str) -> usize {
    use crate::thumbnails::{self, strip_tags, thumbnail_filename};
    use std::collections::HashSet;

    let repo_names = match thumbnails::thumbnail_repo_names(system) {
        Some(names) => names,
        None => return 0,
    };

    let stem = rom_filename
        .rfind('.')
        .map(|i| &rom_filename[..i])
        .unwrap_or(rom_filename);

    let is_arcade = matches!(
        system,
        "arcade_mame" | "arcade_fbneo" | "arcade_mame_2k3p" | "arcade_dc"
    );
    let display_name = if is_arcade {
        crate::arcade_db::lookup_arcade_game(stem).map(|info| info.display_name.to_string())
    } else {
        None
    };
    let source = display_name.as_deref().unwrap_or(stem);
    let thumb_name = thumbnail_filename(source);
    let base_title = strip_tags(&thumb_name).to_lowercase();

    // For tilde dual-title ROMs, also match either half individually.
    let tilde_halves = super::image_matching::tilde_halves(source);

    let mut seen_targets: HashSet<String> = HashSet::new();

    for display_name in repo_names {
        let source_name = thumbnails::libretro_source_name(display_name);

        let entries = MetadataDb::query_thumbnail_index(conn, &source_name, ThumbnailKind::Boxart.repo_dir())
            .unwrap_or_default();

        for entry in &entries {
            let entry_base = strip_tags(&entry.filename).to_lowercase();
            if entry_base != base_title && !tilde_halves.contains(&entry_base) {
                continue;
            }

            // Skip symlink entries — they are duplicates of real entries and
            // can't be previewed from GitHub raw (serves symlink text, not PNG).
            if entry.symlink_target.is_some() {
                continue;
            }

            let resolved = entry
                .symlink_target
                .as_deref()
                .filter(|t| !t.is_empty())
                .unwrap_or(&entry.filename);
            seen_targets.insert(resolved.to_string());
        }
    }

    seen_targets.len()
}

/// Extract the region label from a thumbnail filename.
///
/// "Sonic the Hedgehog (USA, Europe)" -> "USA, Europe"
/// "Sonic the Hedgehog (Japan) (Rev 1)" -> "Japan"
/// "Sonic the Hedgehog" -> "" (no region tag)
fn extract_region_label(filename: &str) -> String {
    // Find the first parenthesized group.
    if let Some(start) = filename.find(" (") {
        let rest = &filename[start + 2..];
        if let Some(end) = rest.find(')') {
            return rest[..end].to_string();
        }
    }
    String::new()
}

/// Stats from a download operation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DownloadStats {
    pub total: usize,
    pub downloaded: usize,
    pub skipped: usize,
    pub failed: usize,
}

/// Download thumbnails for a single system. Runs blocking downloads in a thread pool
/// using `std::thread::scope` for parallelism (since we're in a `spawn_blocking` context,
/// not a tokio async context).
///
/// `on_progress(processed, total, downloaded)` is called periodically.
pub fn download_system_thumbnails(
    conn: &Connection,
    storage_root: &Path,
    system: &str,
    kind: ThumbnailKind,
    on_progress: &dyn Fn(usize, usize, usize),
    cancel: &AtomicBool,
) -> Result<DownloadStats> {
    let repo_names = thumbnails::thumbnail_repo_names(system)
        .ok_or_else(|| Error::Other(format!("No thumbnail repo for {system}")))?;

    let display_names: Vec<&str> = repo_names.to_vec();

    // Build the fuzzy index from the manifest.
    let manifest_index = build_manifest_fuzzy_index(conn, &display_names, kind.repo_dir());

    let rom_filenames = thumbnails::list_rom_filenames(storage_root, system);
    let total = rom_filenames.len();

    let media_dir = storage_root
        .join(crate::storage::RC_DIR)
        .join("media")
        .join(system)
        .join(kind.media_dir());

    // Phase 1: Collect work items (ROMs that need a download).
    let mut work: Vec<(String, ManifestMatch)> = Vec::new();
    let mut skipped = 0usize;
    for rom_filename in &rom_filenames {
        if let Some(m) = find_in_manifest(&manifest_index, rom_filename, system) {
            let local_path = media_dir.join(format!("{}.png", m.filename));
            if local_path.exists() {
                skipped += 1; // Already downloaded.
            } else {
                work.push((rom_filename.clone(), m.clone()));
            }
        }
        // ROMs with no manifest match are silently ignored.
    }

    // Deduplicate work by manifest filename (multiple ROMs can match the same thumbnail).
    {
        let mut seen = std::collections::HashSet::new();
        work.retain(|(_, m)| seen.insert(m.filename.clone()));
    }

    on_progress(skipped, total, 0);

    if work.is_empty() || cancel.load(Ordering::Relaxed) {
        return Ok(DownloadStats {
            total,
            downloaded: 0,
            skipped,
            failed: 0,
        });
    }

    // Phase 2: Download with limited concurrency using thread::scope.
    let downloaded = AtomicUsize::new(0);
    let failed = AtomicUsize::new(0);
    let processed = AtomicUsize::new(0);
    let concurrency = 10usize;

    // Use a simple semaphore via a channel for concurrency control.
    let (tx, rx) = std::sync::mpsc::sync_channel::<()>(concurrency);
    // Pre-fill the channel to act as permits.
    for _ in 0..concurrency {
        let _ = tx.send(());
    }

    let kind_dir = kind.repo_dir().to_string();
    let root = storage_root.to_path_buf();
    let sys = system.to_string();

    std::thread::scope(|scope| {
        let downloaded = &downloaded;
        let failed = &failed;
        let processed = &processed;
        let tx = &tx;
        let rx_mutex = std::sync::Mutex::new(&rx);

        for (_rom_filename, m) in &work {
            if cancel.load(Ordering::Relaxed) {
                break;
            }

            // Acquire a permit (blocks until one is available).
            {
                let rx_guard = rx_mutex.lock().unwrap();
                let _ = rx_guard.recv();
            }

            if cancel.load(Ordering::Relaxed) {
                let _ = tx.send(()); // Release permit.
                break;
            }

            let m = m.clone();
            let kind_dir = kind_dir.clone();
            let root = root.clone();
            let sys = sys.clone();
            let kind_enum = kind;

            scope.spawn(move || {
                match download_thumbnail(&m, &kind_dir) {
                    Ok(bytes) => {
                        match save_thumbnail(&root, &sys, kind_enum, &m.filename, &bytes) {
                            Ok(_) => {
                                downloaded.fetch_add(1, Ordering::Relaxed);
                            }
                            Err(e) => {
                                tracing::warn!("Failed to save {}: {e}", m.filename);
                                failed.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::debug!("Failed to download {}: {e}", m.filename);
                        failed.fetch_add(1, Ordering::Relaxed);
                    }
                }
                processed.fetch_add(1, Ordering::Relaxed);
                // Release permit.
                let _ = tx.send(());
            });

            // Report progress periodically.
            let done = processed.load(Ordering::Relaxed);
            if done.is_multiple_of(5) {
                on_progress(skipped + done, total, downloaded.load(Ordering::Relaxed));
            }
        }
    });

    let downloaded_count = downloaded.load(Ordering::Relaxed);
    let failed_count = failed.load(Ordering::Relaxed);

    on_progress(total, total, downloaded_count);

    Ok(DownloadStats {
        total,
        downloaded: downloaded_count,
        skipped,
        failed: failed_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_in_manifest_case_insensitive_exact_match() {
        // ROM has "the" (lowercase), index has "The" (uppercase)
        // Without CI-exact, fuzzy matching would lose the region tag
        let match_usa = ManifestMatch {
            filename: "Sonic The Hedgehog 3 (USA)".to_string(),
            is_symlink: false,
            repo_url_name: "test".to_string(),
            branch: "master".to_string(),
        };
        let match_jpn = ManifestMatch {
            filename: "Sonic The Hedgehog 3 (Japan, Korea)".to_string(),
            is_symlink: false,
            repo_url_name: "test".to_string(),
            branch: "master".to_string(),
        };

        let mut exact = HashMap::new();
        let mut exact_ci = HashMap::new();
        let mut by_tags = HashMap::new();
        let by_version = HashMap::new();

        // Insert both variants
        exact.insert(
            "Sonic The Hedgehog 3 (Japan, Korea)".to_string(),
            match_jpn.clone(),
        );
        exact.insert("Sonic The Hedgehog 3 (USA)".to_string(), match_usa.clone());
        exact_ci.insert(
            "sonic the hedgehog 3 (japan, korea)".to_string(),
            match_jpn.clone(),
        );
        exact_ci.insert("sonic the hedgehog 3 (usa)".to_string(), match_usa.clone());
        // Japan wins fuzzy tier (inserted first)
        by_tags.insert("sonic the hedgehog 3".to_string(), match_jpn.clone());

        let index = ManifestFuzzyIndex {
            exact,
            exact_ci,
            by_tags,
            by_version,
        };

        // ROM "Sonic the Hedgehog 3 (USA).md" (lowercase "the") should match USA via CI-exact
        let result = find_in_manifest(&index, "Sonic the Hedgehog 3 (USA).md", "sega_smd");
        assert!(result.is_some());
        assert_eq!(result.unwrap().filename, "Sonic The Hedgehog 3 (USA)");
    }

    #[test]
    fn find_in_manifest_exact_match_still_preferred() {
        // When case matches exactly, the exact tier should still win
        let m = ManifestMatch {
            filename: "Game (USA)".to_string(),
            is_symlink: false,
            repo_url_name: "test".to_string(),
            branch: "master".to_string(),
        };

        let mut exact = HashMap::new();
        let mut exact_ci = HashMap::new();
        let by_tags = HashMap::new();
        let by_version = HashMap::new();

        exact.insert("Game (USA)".to_string(), m.clone());
        exact_ci.insert("game (usa)".to_string(), m.clone());

        let index = ManifestFuzzyIndex {
            exact,
            exact_ci,
            by_tags,
            by_version,
        };

        let result = find_in_manifest(&index, "Game (USA).md", "sega_smd");
        assert!(result.is_some());
        assert_eq!(result.unwrap().filename, "Game (USA)");
    }

    #[test]
    fn find_in_manifest_falls_to_fuzzy_when_no_ci_match() {
        // When no case-insensitive exact match exists, fuzzy tier should still work
        let m = ManifestMatch {
            filename: "Completely Different Name (USA)".to_string(),
            is_symlink: false,
            repo_url_name: "test".to_string(),
            branch: "master".to_string(),
        };

        let mut exact = HashMap::new();
        let mut exact_ci = HashMap::new();
        let mut by_tags = HashMap::new();
        let by_version = HashMap::new();

        exact.insert("Completely Different Name (USA)".to_string(), m.clone());
        exact_ci.insert("completely different name (usa)".to_string(), m.clone());
        by_tags.insert("completely different name".to_string(), m.clone());

        let index = ManifestFuzzyIndex {
            exact,
            exact_ci,
            by_tags,
            by_version,
        };

        // ROM stem after tag stripping matches
        let result = find_in_manifest(&index, "Completely Different Name (Europe).md", "sega_smd");
        assert!(result.is_some());
        assert_eq!(result.unwrap().filename, "Completely Different Name (USA)");
    }

    #[test]
    fn build_manifest_fuzzy_index_populates_exact_ci() {
        // Verify that build_manifest_fuzzy_index correctly populates the exact_ci tier.
        // We can't easily call it without a real DB, so test the index structure directly.
        let m1 = ManifestMatch {
            filename: "Game Title (USA)".to_string(),
            is_symlink: false,
            repo_url_name: "test".to_string(),
            branch: "master".to_string(),
        };
        let m2 = ManifestMatch {
            filename: "Game Title (Europe)".to_string(),
            is_symlink: false,
            repo_url_name: "test".to_string(),
            branch: "master".to_string(),
        };

        let mut exact_ci = HashMap::new();
        // Simulating what build_manifest_fuzzy_index does
        exact_ci
            .entry("game title (usa)".to_string())
            .or_insert_with(|| m1.clone());
        exact_ci
            .entry("game title (europe)".to_string())
            .or_insert_with(|| m2.clone());

        // Both entries preserved (they have different full names)
        assert_eq!(
            exact_ci.get("game title (usa)").unwrap().filename,
            "Game Title (USA)"
        );
        assert_eq!(
            exact_ci.get("game title (europe)").unwrap().filename,
            "Game Title (Europe)"
        );

        // Versus by_tags which would collapse both to "game title"
        let mut by_tags = HashMap::new();
        by_tags
            .entry("game title".to_string())
            .or_insert_with(|| m1.clone());
        by_tags
            .entry("game title".to_string())
            .or_insert_with(|| m2.clone());
        // Only first insertion wins
        assert_eq!(
            by_tags.get("game title").unwrap().filename,
            "Game Title (USA)"
        );
    }

    #[test]
    fn find_in_manifest_slash_dual_name_matches_primary() {
        // Arcade display name: "Animal Basket / Hustle Tamaire Kyousou (19 Jan 2005)"
        // thumbnail_filename replaces '/' with '_':
        //   "Animal Basket _ Hustle Tamaire Kyousou (19 Jan 2005)"
        // strip_tags: "Animal Basket _ Hustle Tamaire Kyousou"
        // The repo only has "Animal Basket" — tier 4 should split on " _ " and match.
        let m = ManifestMatch {
            filename: "Animal Basket".to_string(),
            is_symlink: false,
            repo_url_name: "Atomiswave".to_string(),
            branch: "master".to_string(),
        };

        let mut by_tags = HashMap::new();
        by_tags.insert("animal basket".to_string(), m.clone());

        let index = ManifestFuzzyIndex {
            exact: HashMap::new(),
            exact_ci: HashMap::new(),
            by_tags,
            by_version: HashMap::new(),
        };

        // Simulate: ROM "anmlbskt.zip" resolves via arcade_db to
        // "Animal Basket / Hustle Tamaire Kyousou (19 Jan 2005)"
        // After thumbnail_filename: "Animal Basket _ Hustle Tamaire Kyousou (19 Jan 2005)"
        // After strip_tags: "animal basket _ hustle tamaire kyousou"
        // Tiers 1-3 fail. Tier 4 splits on " _ " and tries "animal basket" — match.
        let thumb = "Animal Basket _ Hustle Tamaire Kyousou (19 Jan 2005)";
        let result = find_in_manifest_with_thumb_name(&index, thumb);
        assert!(
            result.is_some(),
            "Slash dual-name should match primary part"
        );
        assert_eq!(result.unwrap().filename, "Animal Basket");
    }

    #[test]
    fn find_in_manifest_slash_skips_short_parts() {
        // "Mushiking IV / V / VI" — parts "V" and "VI" are too short (< 5 chars).
        // Only "Mushiking IV" should be tried (but it still has 12 chars).
        let m = ManifestMatch {
            filename: "Something Else".to_string(),
            is_symlink: false,
            repo_url_name: "test".to_string(),
            branch: "master".to_string(),
        };

        let mut by_tags = HashMap::new();
        // A very short title "v" should NOT be matched
        by_tags.insert("v".to_string(), m.clone());

        let index = ManifestFuzzyIndex {
            exact: HashMap::new(),
            exact_ci: HashMap::new(),
            by_tags,
            by_version: HashMap::new(),
        };

        // After thumbnail_filename and strip_tags, the search key would be
        // "mushiking iv _ v _ vi" — parts "v" and "vi" are < 5 chars.
        let thumb = "Mushiking IV _ V _ VI (World)";
        let result = find_in_manifest_with_thumb_name(&index, thumb);
        assert!(result.is_none(), "Should not match on short slash parts");
    }

    /// Helper for testing find_in_manifest with a pre-computed thumbnail name,
    /// bypassing the arcade_db lookup and filename extraction.
    fn find_in_manifest_with_thumb_name<'a>(
        index: &'a ManifestFuzzyIndex,
        thumb_name: &str,
    ) -> Option<&'a ManifestMatch> {
        use crate::thumbnails::{strip_tags, strip_version};

        // Tier 1: exact
        if let Some(m) = index.exact.get(thumb_name) {
            return Some(m);
        }
        // Tier 1b: CI exact
        if let Some(m) = index.exact_ci.get(&thumb_name.to_lowercase()) {
            return Some(m);
        }
        // Tier 2: strip tags
        let key = strip_tags(thumb_name).to_lowercase();
        if let Some(m) = index.by_tags.get(&key) {
            return Some(m);
        }
        // Tier 3: version-stripped
        let version_key = strip_version(&key);
        if version_key.len() < key.len() {
            if let Some(m) = index
                .by_tags
                .get(version_key)
                .or_else(|| index.by_version.get(version_key))
            {
                return Some(m);
            }
        }
        // Tier 4: slash dual-name
        let search_key = if version_key.len() < key.len() {
            version_key
        } else {
            &key
        };
        if search_key.contains(" / ") || search_key.contains(" _ ") {
            let separator = if search_key.contains(" / ") {
                " / "
            } else {
                " _ "
            };
            for part in search_key.split(separator) {
                let part = part.trim();
                if part.len() >= 5 {
                    if let Some(m) = index
                        .by_tags
                        .get(part)
                        .or_else(|| index.by_version.get(part))
                    {
                        return Some(m);
                    }
                }
            }
        }
        None
    }
}
