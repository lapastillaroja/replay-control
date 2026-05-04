//! Manifest-based libretro-thumbnails: fetch file listings via GitHub REST API,
//! store in SQLite, and download individual images on demand.
//!
//! Phase 1: Manifest generation — build a local index of all available thumbnails
//!   using the GitHub Trees API (fast, no git clone required).
//! Phase 2: Image download — fetch matched images from raw.githubusercontent.com.

use std::collections::HashMap;
use std::path::Path;
#[cfg(feature = "http")]
use std::sync::atomic::{AtomicBool, Ordering};

use rusqlite::Connection;

use crate::external_metadata::{self, ThumbnailManifestEntry};
use crate::thumbnails::{self, ThumbnailKind};
use replay_control_core::error::{Error, Result};

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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct ManifestImportStats {
    pub repos_fetched: usize,
    pub total_entries: usize,
    pub errors: Vec<String>,
    /// Set when a 403 + `X-RateLimit-Remaining: 0` was observed during the
    /// run. `import_all_manifests` bails early when this flips to true — every
    /// further request would hit the same wall, and the WriteGate/HTTP loop
    /// would just keep churning. UI surfaces a "configure GitHub API key"
    /// hint when this is true.
    #[serde(default)]
    pub rate_limited: bool,
    /// Unix timestamp at which GitHub says the rate limit resets, when
    /// available from the `X-RateLimit-Reset` header on the offending response.
    #[serde(default)]
    pub rate_limit_reset_unix: Option<u64>,
}

/// Collect the unique list of libretro-thumbnails repos from all supported systems.
pub fn collect_all_repos() -> Vec<RepoInfo> {
    use replay_control_core::systems;

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
        | "Philips - CD-i" => "main",
        _ => "master",
    }
}

/// Outcome of a GitHub API GET, distinguishing rate-limit responses from
/// other errors so the manifest pipeline can bail early instead of charging
/// through 70 doomed requests.
#[cfg(feature = "http")]
#[derive(Debug)]
pub enum GhResponse {
    Json {
        body: serde_json::Value,
        rate_limit_remaining: Option<u64>,
    },
    /// 403 with `X-RateLimit-Remaining: 0`. `reset_unix` is the value of
    /// `X-RateLimit-Reset` if present.
    RateLimited {
        reset_unix: Option<u64>,
        message: String,
    },
    Error(replay_control_core::error::Error),
}

/// GET a GitHub API endpoint and return a structured outcome that the
/// caller can use to drive observable behaviour (logging, early bail).
#[cfg(feature = "http")]
async fn gh_api_get(
    url: &str,
    headers: &[(&str, &str)],
    timeout: std::time::Duration,
) -> GhResponse {
    let mut req = crate::http::shared_client().get(url).timeout(timeout);
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            return GhResponse::Error(replay_control_core::error::Error::Other(format!(
                "HTTP request failed for {url}: {e}"
            )));
        }
    };
    let status = resp.status();
    let parse_header = |name: &str| -> Option<u64> {
        resp.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
    };
    let rl_remaining = parse_header("x-ratelimit-remaining");
    let rl_reset = parse_header("x-ratelimit-reset");

    if status.as_u16() == 403 && rl_remaining == Some(0) {
        // Drain the body for the message field — best-effort.
        let body = resp.text().await.unwrap_or_default();
        let message = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(str::to_owned))
            .unwrap_or_else(|| "GitHub API rate limit exceeded".into());
        return GhResponse::RateLimited {
            reset_unix: rl_reset,
            message,
        };
    }
    if !status.is_success() {
        return GhResponse::Error(replay_control_core::error::Error::Other(format!(
            "HTTP {status} for {url}"
        )));
    }
    match resp.json::<serde_json::Value>().await {
        Ok(body) => GhResponse::Json {
            body,
            rate_limit_remaining: rl_remaining,
        },
        Err(e) => GhResponse::Error(replay_control_core::error::Error::Other(format!(
            "JSON parse error for {url}: {e}"
        ))),
    }
}

/// Fetch the full tree listing for a libretro-thumbnails repo via GitHub REST API.
/// Returns `(commit_sha, entries)`.
///
/// Uses `GET /repos/libretro-thumbnails/{url_name}/git/trees/{branch}?recursive=1`.
/// Filters and parses the response inline, returning only Named_Boxarts and Named_Snaps
/// entries as `ThumbnailEntry` values.
///
/// Returns the structured `GhResponse` so callers can distinguish rate-limit
/// responses from network/parse errors. Use `fetch_repo_tree_simple` if all
/// you need is the legacy `Result<(String, Vec)>` shape.
#[cfg(feature = "http")]
pub async fn fetch_repo_tree(
    url_name: &str,
    branch: &str,
    api_key: Option<&str>,
) -> std::result::Result<(String, Vec<ThumbnailEntry>), GhResponse> {
    let url = format!(
        "https://api.github.com/repos/libretro-thumbnails/{url_name}/git/trees/{branch}?recursive=1"
    );

    let mut headers = vec![("Accept", "application/vnd.github+json")];
    let auth_header;
    if let Some(key) = api_key {
        auth_header = format!("Bearer {key}");
        headers.push(("Authorization", auth_header.as_str()));
    }

    let json = match gh_api_get(&url, &headers, std::time::Duration::from_secs(60)).await {
        GhResponse::Json { body, .. } => body,
        other => return Err(other),
    };

    // Check for API error responses (e.g. rate limit message embedded in 200).
    if let Some(msg) = json.get("message").and_then(|v| v.as_str()) {
        return Err(GhResponse::Error(Error::Other(format!(
            "GitHub API error for {url_name}/{branch}: {msg}"
        ))));
    }

    let commit_sha = json
        .get("sha")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let tree = json.get("tree").and_then(|v| v.as_array()).ok_or_else(|| {
        GhResponse::Error(Error::Other(
            "Missing 'tree' field in API response".to_string(),
        ))
    })?;

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

/// Convert parsed thumbnail entries into the (kind, filename, symlink_target)
/// triples expected by `external_metadata::insert_thumbnail_manifest_rows`.
pub(crate) fn entries_to_tuples(
    entries: &[ThumbnailEntry],
) -> Vec<(String, String, Option<String>)> {
    entries
        .iter()
        .map(|e| {
            let symlink_target = if e.is_symlink {
                Some(String::new()) // Placeholder — resolved at download time.
            } else {
                None
            };
            (e.kind.clone(), e.filename.clone(), symlink_target)
        })
        .collect()
}

/// Orchestrate the full manifest import for all repos.
/// Calls `on_progress(repos_done, repos_total, current_repo_display_name)`.
/// Returns import stats. Skips repos whose commit SHA hasn't changed.
///
/// Writes go to the host-global `external_metadata.db` via the pool the app
/// constructs and passes in. Each repo's per-transaction write briefly
/// acquires the pool's single writer; the GitHub HTTP fetches between repos
/// run pool-free so they don't hold a write lock.
#[cfg(feature = "http")]
pub async fn import_all_manifests(
    em_pool: &crate::DbPool,
    on_progress: &(dyn Fn(usize, usize, &str) + Send + Sync),
    cancel: &AtomicBool,
    api_key: Option<&str>,
) -> Result<ManifestImportStats> {
    let repos = collect_all_repos();
    let total = repos.len();
    let mut stats = ManifestImportStats::default();

    tracing::info!(
        "Manifest import: starting ({} repos, api_key={})",
        total,
        if api_key.is_some() { "yes" } else { "no" }
    );

    // Bail out after this many repos in a row fail. Sustained failures are
    // almost always GitHub API rate limiting (HTTP 403) or no network — the
    // rest of the run will hit the same wall.
    const MAX_CONSECUTIVE_FAILURES: usize = 3;
    let mut consecutive_failures = 0usize;

    // Helper: when fetch_repo_tree / check_repo_freshness flag rate limiting,
    // record it on stats and emit one user-actionable warning per pipeline run.
    let mark_rate_limited = |stats: &mut ManifestImportStats,
                             reset_unix: Option<u64>,
                             message: &str| {
        if !stats.rate_limited {
            stats.rate_limited = true;
            stats.rate_limit_reset_unix = reset_unix;
            let hint = if api_key.is_none() {
                " (no GitHub API key configured; configure one in Settings → GitHub API key for 5 000 req/h)"
            } else {
                ""
            };
            let reset_msg = match reset_unix {
                Some(t) => format!(" (resets at unix={t})"),
                None => String::new(),
            };
            tracing::warn!("GitHub API rate limit exceeded: {message}{reset_msg}{hint}");
        }
    };

    for (i, repo) in repos.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        if stats.rate_limited {
            // No point continuing — every request will hit the same wall.
            // Surface the run as a structured rate-limit failure instead of
            // pretending it was a transient flake.
            tracing::warn!(
                "Manifest import: aborting after rate limit detected; {} repo(s) skipped",
                repos.len() - i
            );
            break;
        }
        if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
            tracing::warn!(
                "Manifest import: aborting after {consecutive_failures} consecutive failures; \
                 {} repo(s) skipped",
                repos.len() - i
            );
            break;
        }

        on_progress(i, total, &repo.display_name);

        let source_name = thumbnails::libretro_source_name(&repo.display_name);

        // Check if repo has changed since last import.
        let source_name_read = source_name.clone();
        let existing = em_pool
            .read(move |conn| {
                external_metadata::get_data_source(conn, &source_name_read)
                    .ok()
                    .flatten()
            })
            .await
            .flatten();
        if let Some(status) = existing {
            let existing_hash = status.version_hash.as_deref().unwrap_or("");
            if !existing_hash.is_empty() {
                match check_repo_freshness(&repo.url_name, existing_hash, api_key).await {
                    FreshnessOutcome::Unchanged => {
                        stats.total_entries += status.entry_count;
                        stats.repos_fetched += 1;
                        consecutive_failures = 0;
                        continue;
                    }
                    FreshnessOutcome::Changed => { /* Re-fetch below. */ }
                    FreshnessOutcome::Unknown(e) => {
                        tracing::debug!(
                            "Freshness check inconclusive for {}: {e}",
                            repo.display_name
                        );
                    }
                    FreshnessOutcome::RateLimited {
                        reset_unix,
                        message,
                    } => {
                        mark_rate_limited(&mut stats, reset_unix, &message);
                        continue; // Will trip the early-bail check at top of loop.
                    }
                }
            }
        }

        let branch = default_branch(&repo.display_name);
        let (commit_sha, entries, actual_branch) =
            match fetch_repo_tree(&repo.url_name, branch, api_key).await {
                Ok((sha, entries)) => (sha, entries, branch),
                Err(GhResponse::RateLimited {
                    reset_unix,
                    message,
                }) => {
                    mark_rate_limited(&mut stats, reset_unix, &message);
                    continue;
                }
                Err(_) => {
                    // Try the other branch before giving up.
                    let alt = if branch == "master" { "main" } else { "master" };
                    match fetch_repo_tree(&repo.url_name, alt, api_key).await {
                        Ok((sha, entries)) => (sha, entries, alt),
                        Err(GhResponse::RateLimited {
                            reset_unix,
                            message,
                        }) => {
                            mark_rate_limited(&mut stats, reset_unix, &message);
                            continue;
                        }
                        Err(GhResponse::Error(e)) => {
                            stats.errors.push(format!("{}: {e}", repo.display_name));
                            consecutive_failures += 1;
                            continue;
                        }
                        Err(GhResponse::Json { .. }) => {
                            // Unreachable: Ok path returns Json upstream.
                            stats.errors.push(format!(
                                "{}: unexpected upstream response shape",
                                repo.display_name
                            ));
                            consecutive_failures += 1;
                            continue;
                        }
                    }
                }
            };

        // Single transaction per repo on the pool's write connection: upsert
        // data_source, replace manifest rows, then patch the source row's
        // count. Atomic so partial state never leaves rows pointing at a
        // count=0 row.
        let source_name_w = source_name.clone();
        let actual_branch_w = actual_branch.to_string();
        let result = em_pool
            .write(move |conn| -> Result<usize> {
                let tx = conn
                    .transaction()
                    .map_err(|e| Error::Other(format!("begin: {e}")))?;
                external_metadata::upsert_data_source(
                    &tx,
                    &source_name_w,
                    "libretro-thumbnails",
                    &commit_sha,
                    &actual_branch_w,
                    0,
                )?;
                external_metadata::delete_thumbnail_manifest(&tx, &source_name_w)?;
                let tuples = entries_to_tuples(&entries);
                let count = external_metadata::insert_thumbnail_manifest_rows(
                    &tx,
                    &source_name_w,
                    &tuples,
                )?;
                external_metadata::upsert_data_source(
                    &tx,
                    &source_name_w,
                    "libretro-thumbnails",
                    &commit_sha,
                    &actual_branch_w,
                    count,
                )?;
                tx.commit()
                    .map_err(|e| Error::Other(format!("commit: {e}")))?;
                Ok(count)
            })
            .await;

        match result {
            Some(Ok(count)) => {
                stats.total_entries += count;
                stats.repos_fetched += 1;
                consecutive_failures = 0;
            }
            Some(Err(e)) => {
                stats.errors.push(format!("{}: {e}", repo.display_name));
                consecutive_failures += 1;
            }
            None => {
                stats.errors.push(format!(
                    "{}: external_metadata pool unavailable",
                    repo.display_name
                ));
                consecutive_failures += 1;
            }
        }
    }

    on_progress(total, total, "");

    tracing::info!(
        "Manifest import: complete — {} repo(s) fetched, {} entries indexed, {} error(s){}",
        stats.repos_fetched,
        stats.total_entries,
        stats.errors.len(),
        if stats.rate_limited {
            ", rate-limited"
        } else {
            ""
        }
    );

    Ok(stats)
}

/// Outcome of `check_repo_freshness`. Distinguishes "unchanged / changed /
/// unknown" from rate-limit responses so the caller can bail rather than
/// falling through to a re-fetch (which would also be rate-limited).
#[cfg(feature = "http")]
enum FreshnessOutcome {
    Unchanged,
    Changed,
    Unknown(String),
    RateLimited {
        reset_unix: Option<u64>,
        message: String,
    },
}

/// Check if a repo has changed by comparing the latest commit SHA via GitHub API
/// with the stored hash. Returns `FreshnessOutcome` so the caller can react to
/// rate-limit responses without re-fetching.
///
/// Uses `GET /repos/libretro-thumbnails/{url_name}/commits/HEAD`. We send
/// `Accept: application/vnd.github+json` (instead of the SHA media type) so
/// rate-limit responses come back as JSON we can introspect via `gh_api_get`.
#[cfg(feature = "http")]
async fn check_repo_freshness(
    url_name: &str,
    stored_hash: &str,
    api_key: Option<&str>,
) -> FreshnessOutcome {
    let url = format!("https://api.github.com/repos/libretro-thumbnails/{url_name}/commits/HEAD");

    let mut headers = vec![("Accept", "application/vnd.github+json")];
    let auth_header;
    if let Some(key) = api_key {
        auth_header = format!("Bearer {key}");
        headers.push(("Authorization", auth_header.as_str()));
    }

    match gh_api_get(&url, &headers, std::time::Duration::from_secs(15)).await {
        GhResponse::Json { body, .. } => {
            let sha = body.get("sha").and_then(|v| v.as_str()).unwrap_or("");
            if sha.is_empty() {
                FreshnessOutcome::Unknown("empty 'sha' in response".to_string())
            } else if sha == stored_hash {
                FreshnessOutcome::Unchanged
            } else {
                FreshnessOutcome::Changed
            }
        }
        GhResponse::RateLimited {
            reset_unix,
            message,
        } => FreshnessOutcome::RateLimited {
            reset_unix,
            message,
        },
        GhResponse::Error(e) => FreshnessOutcome::Unknown(e.to_string()),
    }
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
    /// base_title(stem) -> ManifestMatch (tilde split + article normalization + lowercase)
    pub by_base_title: HashMap<String, ManifestMatch>,
    /// Aggressively normalized (all punctuation stripped, spaces preserved) -> ManifestMatch
    pub by_aggressive: HashMap<String, ManifestMatch>,
    /// Compact-aggressive normalization (punctuation AND spaces stripped) -> ManifestMatch.
    /// Last-resort tier for the case where one side has internal whitespace
    /// and the other doesn't, e.g. arcade catalog `"Galaga88"` vs libretro
    /// `"Galaga '88"` — both collapse to `"galaga88"` here.
    pub by_aggressive_compact: HashMap<String, ManifestMatch>,
}

/// Load the raw `(url_name, branch, entries)` triples for a list of libretro
/// repos and a single image `kind` from an `external_metadata` read connection.
///
/// Pure data fetch — keep DB-side work tight so the caller can drop the
/// connection before invoking `build_manifest_fuzzy_index_from_raw`.
pub fn load_repo_manifest_data(
    em_conn: &Connection,
    repo_display_names: &[&str],
    kind: &str,
) -> Vec<(String, String, Vec<ThumbnailManifestEntry>)> {
    let mut out = Vec::with_capacity(repo_display_names.len());
    for display_name in repo_display_names {
        let url_name = thumbnails::repo_url_name(display_name);
        let source_name = thumbnails::libretro_source_name(display_name);
        let branch = external_metadata::get_data_source(em_conn, &source_name)
            .ok()
            .flatten()
            .and_then(|s| s.branch)
            .unwrap_or_else(|| "master".to_string());
        let entries = external_metadata::query_thumbnail_manifest(em_conn, &source_name, kind)
            .unwrap_or_default();
        out.push((url_name, branch, entries));
    }
    out
}

/// Convenience: load + build in one call. Sync — caller passes the
/// `external_metadata` read connection.
pub fn build_manifest_fuzzy_index(
    em_conn: &Connection,
    repo_display_names: &[&str],
    kind: &str,
) -> ManifestFuzzyIndex {
    let repo_data = load_repo_manifest_data(em_conn, repo_display_names, kind);
    build_manifest_fuzzy_index_from_raw(&repo_data)
}

/// Build a manifest fuzzy index from pre-fetched raw data.
///
/// Each element in `repo_data` is `(repo_url_name, branch, entries)` where
/// entries were queried from `thumbnail_manifest`. The pre-loaded shape lets
/// the caller release the DB connection before the expensive index construction.
pub fn build_manifest_fuzzy_index_from_raw(
    repo_data: &[(String, String, Vec<ThumbnailManifestEntry>)],
) -> ManifestFuzzyIndex {
    use replay_control_core::title_utils::{
        base_title, normalize_aggressive, normalize_aggressive_compact,
    };
    use thumbnails::{strip_tags, strip_version};

    let mut exact = HashMap::new();
    let mut exact_ci = HashMap::new();
    let mut by_tags = HashMap::new();
    let mut by_version = HashMap::new();
    let mut by_base_title = HashMap::new();
    let mut by_aggressive = HashMap::new();
    let mut by_aggressive_compact = HashMap::new();

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
                by_version
                    .entry(version_key.to_string())
                    .or_insert_with(|| m.clone());
            }

            // Tier 4: base_title (tilde split + article normalization)
            let bt = base_title(&entry.filename);
            if bt != key {
                by_base_title.entry(bt.clone()).or_insert_with(|| m.clone());
            }

            // Tier 5: aggressive normalization (strip all punctuation, keep spaces)
            let agg = normalize_aggressive(&bt);
            by_aggressive.entry(agg).or_insert_with(|| m.clone());

            // Tier 6: compact-aggressive (also strips spaces). See doc on
            // ManifestFuzzyIndex::by_aggressive_compact for the rationale.
            let agg_compact = normalize_aggressive_compact(&bt);
            if !agg_compact.is_empty() {
                by_aggressive_compact.entry(agg_compact).or_insert(m);
            }
        }
    }

    ManifestFuzzyIndex {
        exact,
        exact_ci,
        by_tags,
        by_version,
        by_base_title,
        by_aggressive,
        by_aggressive_compact,
    }
}

/// Look up a ROM in the manifest fuzzy index.
/// Returns the matching manifest entry, or None.
///
/// Matching tiers mirror `image_matching::find_best_match`:
/// 1. Exact thumbnail_filename match
/// 2. Colon variants (": " → " - " and ": " → " ")
/// 3. Case-insensitive exact
/// 4. Strip tags (region/version parentheses)
/// 5. Base title (tilde split + article normalization)
/// 6. Tilde dual-title split — try each half through tags + base_title
/// 7. Version-stripped
/// 8. Slash dual-name matching (arcade " / " separator)
pub fn find_in_manifest<'a>(
    index: &'a ManifestFuzzyIndex,
    rom_filename: &str,
    arcade_display: Option<&str>,
) -> Option<&'a ManifestMatch> {
    use replay_control_core::title_utils::{base_title, filename_stem, strip_n64dd_prefix};
    use thumbnails::{strip_tags, strip_version, thumbnail_filename};

    let stem = strip_n64dd_prefix(filename_stem(rom_filename));

    let source: &str = arcade_display.unwrap_or(stem);
    let thumb_name = thumbnail_filename(source);

    // Tier 1: exact match.
    if let Some(m) = index.exact.get(&thumb_name) {
        return Some(m);
    }

    // Colon variants (same logic as import_system_thumbnails in thumbnails.rs).
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

    // Tier 2: case-insensitive exact (preserves region tags like "(USA)", "(Europe)")
    if let Some(m) = index.exact_ci.get(&thumb_name.to_lowercase()) {
        return Some(m);
    }

    // Tier 3: strip tags.
    let key = strip_tags(&thumb_name).to_lowercase();
    if let Some(m) = index.by_tags.get(&key) {
        return Some(m);
    }

    // Tier 4: base title (tilde split + article normalization).
    let base = base_title(&thumb_name);
    if base != key
        && let Some(m) = index
            .by_tags
            .get(&base)
            .or_else(|| index.by_base_title.get(&base))
    {
        return Some(m);
    }

    // Tier 5: tilde dual-title split — try each half through tags + base_title.
    if source.contains(" ~ ") {
        for half in source.split(" ~ ") {
            let half = half.trim();
            let half_thumb = thumbnail_filename(half);
            if let Some(m) = index.exact.get(&half_thumb) {
                return Some(m);
            }
            let half_key = strip_tags(&half_thumb).to_lowercase();
            if let Some(m) = index.by_tags.get(&half_key) {
                return Some(m);
            }
            let half_base = base_title(&half_thumb);
            if half_base != half_key
                && let Some(m) = index
                    .by_tags
                    .get(&half_base)
                    .or_else(|| index.by_base_title.get(&half_base))
            {
                return Some(m);
            }
        }
    }

    // Tier 6: version-stripped.
    let version_key = strip_version(&key);
    if version_key.len() < key.len()
        && let Some(m) = index
            .by_tags
            .get(version_key)
            .or_else(|| index.by_version.get(version_key))
    {
        return Some(m);
    }

    // Tier 7: slash dual-name matching.
    // Arcade display names often contain " / " separating English and Japanese
    // titles (e.g., "Animal Basket / Hustle Tamaire Kyousou"). The thumbnail
    // repo may list only the primary (English) name. Try each side independently.
    //
    // IMPORTANT: Only split on " _ " when the original source contains " / ".
    // thumbnail_filename() converts both "/" and "&" to "_", so splitting on
    // " _ " without checking the source would cause false positives:
    //   "Battletoads & Double Dragon" → "Battletoads _ Double Dragon"
    //   → wrongly splits and matches "Battletoads" alone.
    let search_key = if version_key.len() < key.len() {
        version_key
    } else {
        &key
    };
    let has_real_slash = source.contains(" / ");
    if search_key.contains(" / ") || (has_real_slash && search_key.contains(" _ ")) {
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

    // Tier 8: aggressive normalization (strip all punctuation, keep spaces).
    let agg_key = replay_control_core::title_utils::normalize_aggressive(&base);
    if !agg_key.is_empty()
        && let Some(m) = index.by_aggressive.get(&agg_key)
    {
        return Some(m);
    }

    // Tier 9: compact-aggressive (also strips spaces). Concrete case:
    // arcade catalog ships `display_name="Galaga88"` (no space, no
    // apostrophe) while libretro-thumbnails ships `"Galaga '88"`
    // (apostrophe → space at tier 8, leaves a space mismatch). Both
    // collapse to `"galaga88"` here.
    //
    // **Guard**: only fire when the source's aggressive form has no
    // internal whitespace. Without this guard, transliterated names like
    // "Dong Gu Ri Te Chi Jak Jeon" would compact-match thumbnails like
    // "Dongguri Techi Jakjeon" — sometimes the same game, sometimes not,
    // and those callers go through `hash_matched_name` for a more
    // reliable fix. The compact tier is for the catalog-stripped-name
    // case where one side has *no spaces at all*.
    if !agg_key.contains(' ') {
        let agg_compact_key = replay_control_core::title_utils::normalize_aggressive_compact(&base);
        if !agg_compact_key.is_empty()
            && let Some(m) = index.by_aggressive_compact.get(&agg_compact_key)
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

#[cfg(feature = "http")]
const PNG_MAGIC: [u8; 4] = [0x89, b'P', b'N', b'G'];

/// Download a thumbnail image, handling symlink resolution transparently.
/// Returns the raw PNG bytes on success.
#[cfg(feature = "http")]
pub async fn download_thumbnail(m: &ManifestMatch, kind: &str) -> Result<Vec<u8>> {
    let url = thumbnail_download_url(m, kind);
    let bytes = download_bytes(&url).await?;

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

        let real_bytes = download_bytes(&target_url).await?;

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

/// Download raw bytes from a URL.
#[cfg(feature = "http")]
pub async fn download_bytes(url: &str) -> Result<Vec<u8>> {
    crate::http::get_bytes_with_timeout(url, std::time::Duration::from_secs(15)).await
}

/// Save a downloaded PNG to the media directory. Runs the `mkdir -p` and
/// `write` on the blocking pool.
pub async fn save_thumbnail(
    storage_root: &Path,
    system: &str,
    kind: ThumbnailKind,
    matched_stem: &str,
    png_bytes: Vec<u8>,
) -> Result<std::path::PathBuf> {
    let media_dir = storage_root
        .join(crate::storage::RC_DIR)
        .join("media")
        .join(system)
        .join(kind.media_dir());
    let dest = media_dir.join(format!("{matched_stem}.png"));

    let work = {
        let media_dir = media_dir.clone();
        let dest = dest.clone();
        move || -> Result<()> {
            std::fs::create_dir_all(&media_dir).map_err(|e| Error::io(&media_dir, e))?;
            std::fs::write(&dest, &png_bytes).map_err(|e| Error::io(&dest, e))
        }
    };

    {
        tokio::task::spawn_blocking(work)
            .await
            .map_err(|e| Error::Other(format!("save_thumbnail task panicked: {e}")))??;
    }
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

/// Find all box art variants for a ROM.
///
/// Two-layer discovery:
/// 1. **Filesystem scan** (source-agnostic): finds all downloaded `.png`/`.jpg`
///    images in the boxart directory whose base title matches the ROM.
/// 2. **Manifest index** (libretro): finds undownloaded variants from
///    `thumbnail_index` with preview URLs pointing to GitHub raw content.
///
/// Results from both layers are de-duplicated by filename stem.
pub fn find_boxart_variants(
    em_conn: &Connection,
    system: &str,
    rom_filename: &str,
    arcade_display: Option<&str>,
    storage_root: &std::path::Path,
    active_box_art_url: Option<&str>,
) -> Vec<BoxArtVariant> {
    use crate::thumbnails::{self, strip_tags, thumbnail_filename};
    use std::collections::HashSet;

    let stem = replay_control_core::title_utils::filename_stem(rom_filename);
    let source = arcade_display.unwrap_or(stem);
    let thumb_name = thumbnail_filename(source);
    let base_title = strip_tags(&thumb_name).to_lowercase();

    // For tilde dual-title ROMs (e.g., "Bare Knuckle ~ Streets of Rage"),
    // also match either half individually.
    let tilde_halves = super::matching::tilde_halves(source);

    let media_base = storage_root
        .join(crate::storage::RC_DIR)
        .join("media")
        .join(system)
        .join(ThumbnailKind::Boxart.media_dir());

    let mut variants = Vec::new();
    let mut seen_stems: HashSet<String> = HashSet::new();

    // ── Layer 1: Filesystem scan (downloaded images, any source) ─────
    if let Ok(entries) = std::fs::read_dir(&media_base) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            let Some(img_stem) = thumbnails::strip_image_ext(&name_str) else {
                continue;
            };

            // Check title match before stat (avoid I/O for non-matching files).
            let entry_base = strip_tags(img_stem).to_lowercase();
            if entry_base != base_title
                && !tilde_halves.contains(&entry_base)
                && !super::matching::base_titles_match_with_tags(
                    &base_title,
                    &thumb_name,
                    &entry_base,
                    img_stem,
                )
            {
                continue;
            }

            // Stat only after title matched (avoids thousands of stat calls on exFAT).
            if !thumbnails::is_valid_image_sync(&entry.path()) {
                continue;
            }

            if !seen_stems.insert(img_stem.to_string()) {
                continue;
            }

            let image_url = format!("/media/{system}/boxart/{name_str}");
            let is_active = active_box_art_url
                .map(|url| url == image_url)
                .unwrap_or(false);

            variants.push(BoxArtVariant {
                filename: img_stem.to_string(),
                region_label: extract_region_label(img_stem),
                is_downloaded: true,
                image_url,
                is_active,
                repo_url_name: String::new(),
                branch: String::new(),
            });
        }
    }

    // ── Layer 2: Manifest index (undownloaded libretro variants) ─────
    if let Some(repo_names) = thumbnails::thumbnail_repo_names(system) {
        for repo_display in repo_names {
            let url_name = thumbnails::repo_url_name(repo_display);
            let source_name = thumbnails::libretro_source_name(repo_display);

            let branch = external_metadata::get_data_source(em_conn, &source_name)
                .ok()
                .flatten()
                .and_then(|s| s.branch)
                .unwrap_or_else(|| "master".to_string());
            let entries = external_metadata::query_thumbnail_manifest(
                em_conn,
                &source_name,
                ThumbnailKind::Boxart.repo_dir(),
            )
            .unwrap_or_default();

            for entry in &entries {
                let entry_base = strip_tags(&entry.filename).to_lowercase();
                if entry_base != base_title
                    && !tilde_halves.contains(&entry_base)
                    && !super::matching::base_titles_match_with_tags(
                        &base_title,
                        &thumb_name,
                        &entry_base,
                        &entry.filename,
                    )
                {
                    continue;
                }

                // Skip symlinks — GitHub raw serves the symlink text, not the image.
                if entry.symlink_target.is_some() {
                    continue;
                }

                // Skip if already found on disk in Layer 1.
                if !seen_stems.insert(entry.filename.clone()) {
                    continue;
                }

                // Undownloaded variant — preview from GitHub raw.
                let encoded_name = encode_uri_component(&entry.filename);
                let image_url = format!(
                    "https://raw.githubusercontent.com/libretro-thumbnails/{}/{}/Named_Boxarts/{encoded_name}.png",
                    url_name, branch
                );

                variants.push(BoxArtVariant {
                    filename: entry.filename.clone(),
                    region_label: extract_region_label(&entry.filename),
                    is_downloaded: false,
                    image_url,
                    is_active: false,
                    repo_url_name: url_name.clone(),
                    branch: branch.clone(),
                });
            }
        }
    }

    variants
}

/// Count distinct box art variants for a ROM without building the full list.
/// Faster than `find_boxart_variants()` — queries manifest index only (no filesystem scan).
///
/// Used in game list views where per-ROM filesystem scans would be an N+1 problem.
/// The full `find_boxart_variants()` (with filesystem scan) is only called on the
/// game detail page.
pub fn count_boxart_variants(
    em_conn: &Connection,
    system: &str,
    rom_filename: &str,
    arcade_display: Option<&str>,
) -> usize {
    use crate::thumbnails::{self, strip_tags, thumbnail_filename};
    use std::collections::HashSet;

    let repo_names = match thumbnails::thumbnail_repo_names(system) {
        Some(names) => names,
        None => return 0,
    };

    let stem = replay_control_core::title_utils::filename_stem(rom_filename);
    let source = arcade_display.unwrap_or(stem);
    let thumb_name = thumbnail_filename(source);
    let base_title = strip_tags(&thumb_name).to_lowercase();

    let tilde_halves = super::matching::tilde_halves(source);

    let mut seen_targets: HashSet<String> = HashSet::new();

    for repo_display in repo_names {
        let source_name = thumbnails::libretro_source_name(repo_display);

        let entries = external_metadata::query_thumbnail_manifest(
            em_conn,
            &source_name,
            ThumbnailKind::Boxart.repo_dir(),
        )
        .unwrap_or_default();

        for entry in &entries {
            let entry_base = strip_tags(&entry.filename).to_lowercase();
            if entry_base != base_title
                && !tilde_halves.contains(&entry_base)
                && !super::matching::base_titles_match_with_tags(
                    &base_title,
                    &thumb_name,
                    &entry_base,
                    &entry.filename,
                )
            {
                continue;
            }

            if entry.symlink_target.is_some() {
                continue;
            }

            seen_targets.insert(entry.filename.clone());
        }
    }

    seen_targets.len()
}

/// Extract a descriptive label from a thumbnail filename.
///
/// Collects all parenthesized tags, skipping language-only tags like `(En)`,
/// `(En,Fr,De,Es,It)`, and `(Ja)` which don't help distinguish variants visually.
///
/// "Sonic the Hedgehog (USA, Europe)" -> "USA, Europe"
/// "Sonic Spinball (USA) (Alt 1)" -> "USA, Alt 1"
/// "Sonic The Hedgehog 2 (World) (Rev A) (Sonic Classic Collection)" -> "World, Rev A, Sonic Classic Collection"
/// "Sonic the Hedgehog" -> "" (no tags)
fn extract_region_label(filename: &str) -> String {
    let mut parts = Vec::new();
    let mut rest = filename;
    while let Some(start) = rest.find(" (") {
        let after = &rest[start + 2..];
        if let Some(end) = after.find(')') {
            let tag = &after[..end];
            // Skip language-only tags (e.g., "En", "En,Fr,De,Es,It", "Ja")
            let is_lang_only = tag.split(',').all(|p| {
                let t = p.trim();
                t.len() <= 3
                    && t.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                    && t.chars().skip(1).all(|c| c.is_ascii_lowercase())
            });
            if !is_lang_only {
                parts.push(tag.to_string());
            }
            rest = &after[end + 1..];
        } else {
            break;
        }
    }
    parts.join(", ")
}

/// Stats from a download operation.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct DownloadStats {
    pub total: usize,
    pub downloaded: usize,
    pub skipped: usize,
    pub failed: usize,
}

/// Result of planning which thumbnails need downloading.
pub struct DownloadPlan {
    pub work: Vec<ManifestMatch>,
    pub total: usize,
    pub skipped: usize,
}

/// Plan which thumbnails need downloading from preloaded manifest data.
///
/// This does CPU work and filesystem checks but no DB work. Callers should
/// load manifest rows under an `external_metadata` read connection, release
/// that connection, then call this helper.
pub fn plan_system_thumbnails_from_repo_data(
    repo_data: &[(String, String, Vec<ThumbnailManifestEntry>)],
    storage_root: &Path,
    system: &str,
    kind: ThumbnailKind,
    arcade_lookup: &crate::image_resolution::ArcadeInfoLookup,
) -> Result<DownloadPlan> {
    let manifest_index = build_manifest_fuzzy_index_from_raw(repo_data);

    let rom_filenames = thumbnails::list_rom_filenames(storage_root, system);
    let total = rom_filenames.len();

    let media_dir = storage_root
        .join(crate::storage::RC_DIR)
        .join("media")
        .join(system)
        .join(kind.media_dir());

    let mut work: Vec<(String, ManifestMatch)> = Vec::new();
    let mut skipped = 0usize;
    for rom_filename in &rom_filenames {
        let stem = replay_control_core::title_utils::filename_stem(rom_filename);
        let arcade_display = arcade_lookup.get(stem).map(|i| i.display_name.as_str());
        if let Some(m) = find_in_manifest(&manifest_index, rom_filename, arcade_display) {
            let local_path = media_dir.join(format!("{}.png", m.filename));
            if local_path.exists() {
                skipped += 1;
            } else {
                work.push((rom_filename.clone(), m.clone()));
            }
        }
    }

    // Deduplicate by manifest filename (multiple ROMs can match the same thumbnail).
    {
        let mut seen = std::collections::HashSet::new();
        work.retain(|(_, m)| seen.insert(m.filename.clone()));
    }

    let matches: Vec<ManifestMatch> = work.into_iter().map(|(_, m)| m).collect();
    Ok(DownloadPlan {
        work: matches,
        total,
        skipped,
    })
}

/// Execute planned thumbnail downloads with async concurrency.
/// Does not need a DB connection — call `plan_system_thumbnails_from_repo_data` first.
///
/// `on_progress(processed, total, downloaded)` is called periodically.
#[cfg(feature = "http")]
pub async fn download_system_thumbnails(
    plan: &DownloadPlan,
    storage_root: &Path,
    system: &str,
    kind: ThumbnailKind,
    on_progress: &(dyn Fn(usize, usize, usize) + Send + Sync),
    cancel: &AtomicBool,
) -> Result<DownloadStats> {
    let DownloadPlan {
        work,
        total,
        skipped,
    } = plan;
    let total = *total;
    let skipped = *skipped;

    on_progress(skipped, total, 0);

    if work.is_empty() || cancel.load(Ordering::Relaxed) {
        return Ok(DownloadStats {
            total,
            downloaded: 0,
            skipped,
            failed: 0,
        });
    }

    // JoinSet collects tasks in completion order so a single slow request
    // can't block progress reporting on the ones that already finished.
    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(10));

    let kind_dir = kind.repo_dir().to_string();
    let root = storage_root.to_path_buf();
    let sys = system.to_string();

    let mut tasks: tokio::task::JoinSet<(ManifestMatch, Result<Vec<u8>>)> =
        tokio::task::JoinSet::new();

    for m in work {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let semaphore = semaphore.clone();
        let m = m.clone();
        let kind_dir = kind_dir.clone();
        tasks.spawn(async move {
            let _permit = match semaphore.acquire_owned().await {
                Ok(p) => p,
                Err(_) => {
                    return (m, Err(Error::Other("semaphore closed".into())));
                }
            };
            let bytes = download_thumbnail(&m, &kind_dir).await;
            (m, bytes)
        });
    }

    let mut downloaded_count = 0usize;
    let mut failed_count = 0usize;
    let mut processed = 0usize;

    while let Some(join_result) = tasks.join_next().await {
        if cancel.load(Ordering::Relaxed) {
            // JoinSet aborts remaining tasks on drop.
            break;
        }
        match join_result {
            Ok((m, Ok(bytes))) => {
                match save_thumbnail(&root, &sys, kind, &m.filename, bytes).await {
                    Ok(_) => downloaded_count += 1,
                    Err(e) => {
                        tracing::warn!("Failed to save {}: {e}", m.filename);
                        failed_count += 1;
                    }
                }
            }
            Ok((m, Err(e))) => {
                tracing::debug!("Failed to download {}: {e}", m.filename);
                failed_count += 1;
            }
            Err(e) => {
                tracing::debug!("Download task panicked: {e}");
                failed_count += 1;
            }
        }

        processed += 1;
        on_progress(skipped + processed, total, downloaded_count);
    }

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
            by_base_title: HashMap::new(),
            by_aggressive: HashMap::new(),
            by_aggressive_compact: HashMap::new(),
        };

        // ROM "Sonic the Hedgehog 3 (USA).md" (lowercase "the") should match USA via CI-exact
        let result = find_in_manifest(&index, "Sonic the Hedgehog 3 (USA).md", None);
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
            by_base_title: HashMap::new(),
            by_aggressive: HashMap::new(),
            by_aggressive_compact: HashMap::new(),
        };

        let result = find_in_manifest(&index, "Game (USA).md", None);
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
            by_base_title: HashMap::new(),
            by_aggressive: HashMap::new(),
            by_aggressive_compact: HashMap::new(),
        };

        // ROM stem after tag stripping matches
        let result = find_in_manifest(&index, "Completely Different Name (Europe).md", None);
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
            by_base_title: HashMap::new(),
            by_aggressive: HashMap::new(),
            by_aggressive_compact: HashMap::new(),
        };

        // Simulate: ROM "anmlbskt.zip" resolves via arcade_db to
        // "Animal Basket / Hustle Tamaire Kyousou (19 Jan 2005)"
        // After thumbnail_filename: "Animal Basket _ Hustle Tamaire Kyousou (19 Jan 2005)"
        // After strip_tags: "animal basket _ hustle tamaire kyousou"
        // Tiers 1-3 fail. Tier 4 splits on " _ " and tries "animal basket" — match.
        let source = "Animal Basket / Hustle Tamaire Kyousou (19 Jan 2005)";
        let thumb = "Animal Basket _ Hustle Tamaire Kyousou (19 Jan 2005)";
        let result = find_in_manifest_with_thumb_name(&index, thumb, source);
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
            by_base_title: HashMap::new(),
            by_aggressive: HashMap::new(),
            by_aggressive_compact: HashMap::new(),
        };

        // After thumbnail_filename and strip_tags, the search key would be
        // "mushiking iv _ v _ vi" — parts "v" and "vi" are < 5 chars.
        let source = "Mushiking IV / V / VI (World)";
        let thumb = "Mushiking IV _ V _ VI (World)";
        let result = find_in_manifest_with_thumb_name(&index, thumb, source);
        assert!(result.is_none(), "Should not match on short slash parts");
    }

    #[test]
    fn find_in_manifest_ampersand_not_treated_as_slash() {
        // "Battletoads & Double Dragon (USA)" — the "&" becomes "_" via
        // thumbnail_filename, producing "Battletoads _ Double Dragon (USA)".
        // This must NOT be split as a dual-name (only real " / " should split).
        let m = ManifestMatch {
            filename: "Battletoads (Europe)".to_string(),
            is_symlink: false,
            repo_url_name: "test".to_string(),
            branch: "master".to_string(),
        };

        let mut by_tags = HashMap::new();
        by_tags.insert("battletoads".to_string(), m.clone());

        let index = ManifestFuzzyIndex {
            exact: HashMap::new(),
            exact_ci: HashMap::new(),
            by_tags,
            by_version: HashMap::new(),
            by_base_title: HashMap::new(),
            by_aggressive: HashMap::new(),
            by_aggressive_compact: HashMap::new(),
        };

        // Source has "&" not "/", so " _ " splitting should be suppressed.
        let source = "Battletoads & Double Dragon (USA)";
        let thumb = "Battletoads _ Double Dragon (USA)";
        let result = find_in_manifest_with_thumb_name(&index, thumb, source);
        assert!(
            result.is_none(),
            "Ampersand title should not match via slash splitting"
        );
    }

    #[test]
    fn find_in_manifest_spiderman_ampersand_no_false_positive() {
        // "Spider-Man & Venom - Maximum Carnage (USA)" — the "&" becomes "_".
        // Must NOT split and match "Spider-Man" alone.
        let m = ManifestMatch {
            filename: "Spider-Man".to_string(),
            is_symlink: false,
            repo_url_name: "test".to_string(),
            branch: "master".to_string(),
        };

        let mut by_tags = HashMap::new();
        by_tags.insert("spider-man".to_string(), m.clone());

        let index = ManifestFuzzyIndex {
            exact: HashMap::new(),
            exact_ci: HashMap::new(),
            by_tags,
            by_version: HashMap::new(),
            by_base_title: HashMap::new(),
            by_aggressive: HashMap::new(),
            by_aggressive_compact: HashMap::new(),
        };

        let source = "Spider-Man & Venom - Maximum Carnage (USA)";
        let thumb = "Spider-Man _ Venom - Maximum Carnage (USA)";
        let result = find_in_manifest_with_thumb_name(&index, thumb, source);
        assert!(
            result.is_none(),
            "Ampersand title should not match via slash splitting"
        );
    }

    #[test]
    fn find_in_manifest_galaga88_full_repo_simulation() {
        // Simulate arcade_fbneo's real manifest: both FBNeo and MAME entries indexed
        // via `build_manifest_fuzzy_index_from_raw`. FBNeo has "Galaga '88"; MAME has
        // "Galaga '88 (set 1)" and "Galaga '88 (Japan)".
        let fbneo_entries = vec![ThumbnailManifestEntry {
            filename: "Galaga '88".to_string(),
            symlink_target: None,
        }];
        let mame_entries = vec![
            ThumbnailManifestEntry {
                filename: "Galaga '88 (set 1)".to_string(),
                symlink_target: None,
            },
            ThumbnailManifestEntry {
                filename: "Galaga '88 (Japan)".to_string(),
                symlink_target: None,
            },
        ];
        let repo_data = vec![
            (
                "FBNeo_-_Arcade_Games".to_string(),
                "master".to_string(),
                fbneo_entries,
            ),
            ("MAME".to_string(), "master".to_string(), mame_entries),
        ];
        let index = build_manifest_fuzzy_index_from_raw(&repo_data);

        // galaga88 (parent) → MAME 2003+ display "Galaga '88 (set 1)" → Tier 1 exact
        // matches MAME's "Galaga '88 (set 1)".
        let hit = find_in_manifest(&index, "galaga88.zip", Some("Galaga '88 (set 1)"))
            .expect("galaga88 must match");
        assert_eq!(hit.filename, "Galaga '88 (set 1)");

        // galaga88a (clone, FBNeo-driven display "Galaga '88 (02-03-88)") — no exact
        // match in either repo. Tier 2 strip_tags reduces to "galaga '88" which must
        // match FBNeo's "Galaga '88".
        let hit = find_in_manifest(&index, "galaga88a.zip", Some("Galaga '88 (02-03-88)"))
            .expect("galaga88a must match");
        // Any of the three candidates is acceptable (HashMap insertion order
        // picks one), but matching MUST succeed.
        assert!(
            hit.filename.starts_with("Galaga '88"),
            "expected a Galaga '88 entry, got '{}'",
            hit.filename
        );
    }

    #[test]
    fn find_in_manifest_galaga88_apostrophe_fuzzy_match() {
        // Regression test: "Galaga '88" in libretro FBNeo repo must match the
        // MAME 2003+ display name "Galaga '88 (set 1)" for ROM galaga88.zip.
        // Also the FBNeo display for clone galaga88a.zip: "Galaga '88 (02-03-88)".
        //
        // The apostrophe passes through thumbnail_filename() unchanged, so the
        // strip_tags tier should collapse both names to "galaga '88" and match.
        let m = ManifestMatch {
            filename: "Galaga '88".to_string(),
            is_symlink: false,
            repo_url_name: "FBNeo_-_Arcade_Games".to_string(),
            branch: "master".to_string(),
        };

        let mut exact = HashMap::new();
        let mut exact_ci = HashMap::new();
        let mut by_tags = HashMap::new();
        exact.insert("Galaga '88".to_string(), m.clone());
        exact_ci.insert("galaga '88".to_string(), m.clone());
        by_tags.insert("galaga '88".to_string(), m.clone());

        let index = ManifestFuzzyIndex {
            exact,
            exact_ci,
            by_tags,
            by_version: HashMap::new(),
            by_base_title: HashMap::new(),
            by_aggressive: HashMap::new(),
            by_aggressive_compact: HashMap::new(),
        };

        // Parent: MAME 2003+ display "Galaga '88 (set 1)".
        let result = find_in_manifest(&index, "galaga88.zip", Some("Galaga '88 (set 1)"));
        assert!(
            result.is_some(),
            "Galaga '88 (set 1) should match manifest entry \"Galaga '88\" via strip_tags"
        );

        // Clone: FBNeo display "Galaga '88 (02-03-88)".
        let result = find_in_manifest(&index, "galaga88a.zip", Some("Galaga '88 (02-03-88)"));
        assert!(
            result.is_some(),
            "Galaga '88 (02-03-88) should match manifest entry \"Galaga '88\" via strip_tags"
        );
    }

    /// Helper for testing find_in_manifest with a pre-computed thumbnail name,
    /// bypassing the arcade_db lookup and filename extraction.
    ///
    /// `source` is the original display name (before `thumbnail_filename`), used
    /// to decide whether ` _ ` splitting is valid (only when `source` has ` / `).
    fn find_in_manifest_with_thumb_name<'a>(
        index: &'a ManifestFuzzyIndex,
        thumb_name: &str,
        source: &str,
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
        if version_key.len() < key.len()
            && let Some(m) = index
                .by_tags
                .get(version_key)
                .or_else(|| index.by_version.get(version_key))
        {
            return Some(m);
        }
        // Tier 4: slash dual-name (only split on " _ " if source had " / ")
        let search_key = if version_key.len() < key.len() {
            version_key
        } else {
            &key
        };
        let has_real_slash = source.contains(" / ");
        if search_key.contains(" / ") || (has_real_slash && search_key.contains(" _ ")) {
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

    /// Regression: real-world arcade match where the catalog ships
    /// `display_name="Galaga88"` (no apostrophe, no space — that's how
    /// the build-time arcade DB stripped MAME's "Galaga '88 (set 1)")
    /// while libretro-thumbnails ships `"Galaga '88.png"` (apostrophe +
    /// space). At tier 8 (`normalize_aggressive`) the apostrophe expands
    /// to a space, leaving `"galaga 88"` ≠ `"galaga88"`. Tier 9
    /// (`normalize_aggressive_compact`) collapses both to `"galaga88"`.
    /// Without this tier, the FBNeo Galaga '88 thumbnail never downloads
    /// for any of the three galaga88 ROM variants.
    #[test]
    fn find_in_manifest_galaga88_no_apostrophe_catalog_matches_apostrophe_thumbnail() {
        let entries = vec![
            ThumbnailManifestEntry {
                filename: "Galaga '88".into(),
                symlink_target: None,
            },
            ThumbnailManifestEntry {
                filename: "Galaga '88 (Japan)".into(),
                symlink_target: None,
            },
        ];
        let index = build_manifest_fuzzy_index_from_raw(&[(
            "FBNeo_-_Arcade_Games".to_string(),
            "master".to_string(),
            entries,
        )]);

        // Catalog says "Galaga88" — drives the find_in_manifest source arg.
        let m = find_in_manifest(&index, "galaga88.zip", Some("Galaga88"));
        assert!(
            m.is_some(),
            "Galaga88 (catalog) must match Galaga '88 (libretro) via tier 9"
        );
        assert_eq!(m.unwrap().filename, "Galaga '88");

        // Clones too: "Galaga88 (02-03-88)" should still find the
        // unsuffixed Galaga '88 entry via tier-strip.
        let m = find_in_manifest(&index, "galaga88a.zip", Some("Galaga88 (02-03-88)"));
        assert!(m.is_some());

        let m = find_in_manifest(&index, "galaga88j.zip", Some("Galaga88 (Japan)"));
        assert!(m.is_some());
    }

    #[test]
    fn manifest_import_stats_default_is_not_rate_limited() {
        let s = ManifestImportStats::default();
        assert!(!s.rate_limited);
        assert!(s.rate_limit_reset_unix.is_none());
        assert_eq!(s.repos_fetched, 0);
        assert_eq!(s.total_entries, 0);
        assert!(s.errors.is_empty());
    }

    #[test]
    fn manifest_import_stats_serde_round_trip_preserves_rate_limit() {
        let s = ManifestImportStats {
            repos_fetched: 12,
            total_entries: 4321,
            errors: vec!["one: bad".to_string()],
            rate_limited: true,
            rate_limit_reset_unix: Some(1_733_000_000),
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: ManifestImportStats = serde_json::from_str(&json).unwrap();
        assert!(back.rate_limited);
        assert_eq!(back.rate_limit_reset_unix, Some(1_733_000_000));
    }

    /// Backwards-compat: stats serialized by older clients (without the
    /// rate_limited / rate_limit_reset_unix fields) must still deserialize.
    #[test]
    fn manifest_import_stats_deserializes_legacy_payload() {
        let json = r#"{"repos_fetched":3,"total_entries":900,"errors":[]}"#;
        let s: ManifestImportStats = serde_json::from_str(json).unwrap();
        assert_eq!(s.repos_fetched, 3);
        assert!(!s.rate_limited);
        assert!(s.rate_limit_reset_unix.is_none());
    }
}
