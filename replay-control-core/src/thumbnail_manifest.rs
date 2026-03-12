//! Manifest-based libretro-thumbnails: fetch file listings via git protocol,
//! store in SQLite, and download individual images on demand.
//!
//! Phase 1: Manifest generation — build a local index of all available thumbnails
//!   using `git clone --filter=blob:none` and `git ls-tree` (no GitHub API rate limits).
//! Phase 2: Image download — fetch matched images from raw.githubusercontent.com.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::error::{Error, Result};
use crate::metadata_db::MetadataDb;
use crate::thumbnails::{self, ThumbnailKind};

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
                let url_name = display_name.replace(' ', "_");
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

/// Fetch the full tree listing for a libretro-thumbnails repo via git protocol.
/// Returns `(commit_sha, ls_tree_output)`. Blocking (uses std::process::Command).
///
/// Clones a bare, blobless repo into a temp dir, reads the tree, and cleans up.
/// This avoids GitHub REST API rate limits (60/hour unauthenticated).
pub fn fetch_repo_tree(url_name: &str, branch: &str) -> Result<(String, String)> {
    let repo_url = format!("https://github.com/libretro-thumbnails/{url_name}.git");
    let tmp_dir = format!("/tmp/libretro-thumb-{url_name}");

    // Clean up any leftover dir from a previous failed run.
    let _ = std::fs::remove_dir_all(&tmp_dir);

    // Ensure cleanup on all exit paths.
    struct CleanupGuard<'a>(&'a str);
    impl Drop for CleanupGuard<'_> {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(self.0);
        }
    }
    let _guard = CleanupGuard(&tmp_dir);

    // Clone bare repo with blob filter (downloads only tree objects, no file content).
    let clone_output = std::process::Command::new("git")
        .args([
            "clone",
            "--filter=blob:none",
            "--bare",
            "--depth",
            "1",
            "--branch",
            branch,
            &repo_url,
            &tmp_dir,
        ])
        .output()
        .map_err(|e| Error::Other(format!("Failed to run git clone: {e}")))?;

    if !clone_output.status.success() {
        let stderr = String::from_utf8_lossy(&clone_output.stderr);
        return Err(Error::Other(format!(
            "git clone failed for {url_name}/{branch}: {stderr}"
        )));
    }

    // Get commit SHA.
    let rev_output = std::process::Command::new("git")
        .args(["-C", &tmp_dir, "rev-parse", "HEAD"])
        .output()
        .map_err(|e| Error::Other(format!("Failed to run git rev-parse: {e}")))?;

    if !rev_output.status.success() {
        return Err(Error::Other("git rev-parse HEAD failed".to_string()));
    }

    let commit_sha = String::from_utf8_lossy(&rev_output.stdout)
        .trim()
        .to_string();

    // List all files in the tree.
    let ls_output = std::process::Command::new("git")
        .args(["-C", &tmp_dir, "ls-tree", "-r", "HEAD"])
        .output()
        .map_err(|e| Error::Other(format!("Failed to run git ls-tree: {e}")))?;

    if !ls_output.status.success() {
        return Err(Error::Other("git ls-tree failed".to_string()));
    }

    let ls_tree = String::from_utf8(ls_output.stdout)
        .map_err(|e| Error::Other(format!("Invalid UTF-8 in ls-tree output: {e}")))?;

    // _guard drops here, cleaning up tmp_dir.
    Ok((commit_sha, ls_tree))
}

/// A parsed thumbnail entry from a git ls-tree listing.
#[derive(Debug)]
pub struct ThumbnailEntry {
    pub kind: String,     // "Named_Boxarts" or "Named_Snaps"
    pub filename: String, // stem without .png
    pub is_symlink: bool, // true if git mode is 120000 (symlink)
}

/// Parse `git ls-tree -r HEAD` output, extracting Named_Boxarts and Named_Snaps entries.
///
/// Each line has the format: `<mode> <type> <sha>\t<path>`
/// - Mode `120000` = symlink
/// - Mode `100644` = regular file
pub fn parse_tree_entries(ls_tree_output: &str) -> Result<Vec<ThumbnailEntry>> {
    let mut entries = Vec::new();

    for line in ls_tree_output.lines() {
        // Split on tab: left part is "mode type sha", right part is path.
        let (meta, path) = match line.split_once('\t') {
            Some(parts) => parts,
            None => continue,
        };

        // Filter to Named_Boxarts/ and Named_Snaps/ only.
        let (kind, rest) = if let Some(rest) = path.strip_prefix("Named_Boxarts/") {
            ("Named_Boxarts", rest)
        } else if let Some(rest) = path.strip_prefix("Named_Snaps/") {
            ("Named_Snaps", rest)
        } else {
            continue;
        };

        // Extract the filename stem (strip .png extension).
        let stem = match rest.strip_suffix(".png") {
            Some(s) => s.to_string(),
            None => continue,
        };

        // Detect symlinks from the git file mode (first field).
        let is_symlink = meta.starts_with("120000");

        entries.push(ThumbnailEntry {
            kind: kind.to_string(),
            filename: stem,
            is_symlink,
        });
    }

    Ok(entries)
}

/// Insert parsed thumbnail entries into the `thumbnail_index` table.
pub fn insert_thumbnail_entries(
    db: &mut MetadataDb,
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

    db.bulk_insert_thumbnail_index(source_name, &tuples)
}

/// Orchestrate the full manifest import for all repos.
/// Calls `on_progress(repos_done, repos_total, current_repo_display_name)`.
/// Returns import stats. Skips repos whose commit SHA hasn't changed.
pub fn import_all_manifests(
    db: &mut MetadataDb,
    on_progress: &dyn Fn(usize, usize, &str),
    cancel: &AtomicBool,
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

        let source_name = format!("libretro:{}", repo.url_name);

        // Check if repo has changed since last import.
        if let Ok(Some(status)) = db.get_data_source(&source_name) {
            let existing_hash = status.version_hash.as_deref().unwrap_or("");
            if !existing_hash.is_empty() {
                match check_repo_freshness(&repo.url_name, existing_hash) {
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
        let (commit_sha, ls_tree, actual_branch) = match fetch_repo_tree(&repo.url_name, branch) {
            Ok((sha, tree)) => (sha, tree, branch),
            Err(_) => {
                // Try the other branch before giving up.
                let alt = if branch == "master" { "main" } else { "master" };
                match fetch_repo_tree(&repo.url_name, alt) {
                    Ok((sha, tree)) => (sha, tree, alt),
                    Err(e) => {
                        errors.push(format!("{}: {e}", repo.display_name));
                        continue;
                    }
                }
            }
        };

        let entries = match parse_tree_entries(&ls_tree) {
            Ok(r) => r,
            Err(e) => {
                errors.push(format!("{}: {e}", repo.display_name));
                continue;
            }
        };

        // Upsert data_source BEFORE inserting thumbnail entries (FK constraint).
        if let Err(e) = db.upsert_data_source(
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

        let count = match insert_thumbnail_entries(db, &source_name, &entries) {
            Ok(c) => c,
            Err(e) => {
                errors.push(format!("{}: {e}", repo.display_name));
                continue;
            }
        };

        // Update with actual entry count.
        if let Err(e) = db.upsert_data_source(
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

/// Check if a repo has changed by comparing the latest commit SHA (via `git ls-remote`)
/// with the stored hash. Returns `Ok(true)` if the repo has changed.
///
/// Uses git protocol instead of GitHub REST API to avoid rate limits.
fn check_repo_freshness(url_name: &str, stored_hash: &str) -> Result<bool> {
    let repo_url = format!("https://github.com/libretro-thumbnails/{url_name}.git");

    let output = std::process::Command::new("git")
        .args(["ls-remote", &repo_url, "HEAD"])
        .output()
        .map_err(|e| Error::Other(format!("Failed to run git ls-remote: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Other(format!(
            "git ls-remote failed for {url_name}: {stderr}"
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Output format: "{sha}\tHEAD\n"
    if let Some(sha) = stdout.split('\t').next() {
        let sha = sha.trim();
        if !sha.is_empty() {
            return Ok(sha != stored_hash);
        }
    }

    // Couldn't parse -- assume changed to be safe.
    Ok(true)
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
    /// lowercase(strip_tags(stem)) -> ManifestMatch
    pub by_tags: HashMap<String, ManifestMatch>,
    /// lowercase(strip_version(strip_tags(stem))) -> ManifestMatch
    pub by_version: HashMap<String, ManifestMatch>,
}

/// Build a ManifestFuzzyIndex from the DB for the given repos and kind.
pub fn build_manifest_fuzzy_index(
    db: &MetadataDb,
    repo_display_names: &[&str],
    kind: &str,
) -> ManifestFuzzyIndex {
    use thumbnails::{strip_tags, strip_version};

    let mut exact = HashMap::new();
    let mut by_tags = HashMap::new();
    let mut by_version = HashMap::new();

    for display_name in repo_display_names {
        let url_name = display_name.replace(' ', "_");
        let source_name = format!("libretro:{url_name}");

        // Look up branch from data_sources.
        let branch = db
            .get_data_source(&source_name)
            .ok()
            .flatten()
            .and_then(|s| s.branch)
            .unwrap_or_else(|| "master".to_string());

        let entries = db
            .query_thumbnail_index(&source_name, kind)
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

    // Tier 2: strip tags.
    let key = strip_tags(&thumb_name).to_lowercase();
    if let Some(m) = index.by_tags.get(&key) {
        return Some(m);
    }

    // Tier 3: version-stripped.
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
    db: &MetadataDb,
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
    let manifest_index = build_manifest_fuzzy_index(db, &display_names, kind.repo_dir());

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
            if done % 5 == 0 {
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
