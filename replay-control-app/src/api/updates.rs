//! Self-update subsystem: GitHub release polling, download, checksum
//! verification, extraction, and swap-script generation. Split out of
//! the `background` module — it's a cohesive concern that shares no state with
//! the library scan/enrichment pipeline. Free functions over `&AppState`, like
//! the rest of the background work.

use std::time::Duration;

use replay_control_core_server::update as update_io;

use super::AppState;
use super::background::env_duration_secs;

/// Delay before the first update check, letting Wi-Fi come up on the Pi.
const UPDATE_INITIAL_DELAY_SECS: u64 = 60;
/// Interval between periodic update checks.
const UPDATE_INTERVAL_SECS: u64 = 24 * 60 * 60;

// ── Update system ─────────────────────────────────────────────────

/// GitHub repository for release checks and downloads.
pub(crate) const REPO: &str = "lapastillaroja/replay-control";
/// Maximum time for the entire StartUpdate operation (5 minutes).
const UPDATE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

/// Periodically checks GitHub for new releases.
pub(crate) async fn update_check_loop(state: AppState) {
    // Delay first check to let WiFi come up on Pi.
    tokio::time::sleep(update_initial_delay()).await;

    let analytics = super::analytics::AnalyticsClient::new(
        replay_control_core_server::http::shared_client().clone(),
        super::analytics::ENDPOINT,
    );

    loop {
        if state.has_storage() {
            match perform_update_check_background(&state).await {
                Ok(_) => {}
                Err(e) => tracing::debug!("Background update check failed: {e}"),
            }

            // Analytics ping — independent from update check, same 24h cadence.
            if let Some((ping, is_install)) =
                super::analytics::build_analytics_ping(&state.settings)
            {
                let success = analytics.send(&ping).await;
                if is_install && success {
                    super::analytics::mark_version_reported(&state.settings);
                }
            }
        }

        tokio::time::sleep(update_interval()).await;
    }
}

fn update_initial_delay() -> Duration {
    env_duration_secs(
        "REPLAY_CONTROL_UPDATE_INITIAL_DELAY_SECS",
        UPDATE_INITIAL_DELAY_SECS,
        0,
    )
}

fn update_interval() -> Duration {
    env_duration_secs(
        "REPLAY_CONTROL_UPDATE_INTERVAL_SECS",
        UPDATE_INTERVAL_SECS,
        1,
    )
}

/// Background check variant: does NOT nuke before checking (preserves existing
/// available.json on error). On success: nuke then write. On no-update: nuke.
async fn perform_update_check_background(
    state: &AppState,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let settings = state.settings.load();
    let channel =
        replay_control_core::update::UpdateChannel::from_str_value(settings.update_channel());
    let skipped = settings.skipped_version().map(|s| s.to_string());
    let github_key = settings.github_api_key().map(|s| s.to_string());
    drop(settings);

    match update_io::check_github_update(
        crate::VERSION,
        &update_io::github_api_base_url(),
        REPO,
        &channel,
        skipped.as_deref(),
        github_key.as_deref(),
    )
    .await?
    {
        Some(available) => {
            // Race guard: verify channel still matches before writing.
            let current_channel =
                replay_control_core_server::settings::read_update_channel(&state.settings);
            if current_channel != channel {
                tracing::debug!(
                    "Update channel changed during check ({} -> {}), discarding result",
                    channel.as_str(),
                    current_channel.as_str()
                );
                return Ok(());
            }
            update_io::nuke_update_dir();
            update_io::write_available_update(&available).ok();
            let _ = state
                .events_tx
                .send(super::ConfigEvent::UpdateAvailable { update: available });
        }
        None => {
            // No update found — nuke stale state.
            update_io::nuke_update_dir();
        }
    }
    Ok(())
}

/// Manual check: nukes first, checks, writes if found, broadcasts SSE.
pub async fn perform_update_check(
    state: &AppState,
) -> Result<
    Option<replay_control_core::update::AvailableUpdate>,
    Box<dyn std::error::Error + Send + Sync>,
> {
    update_io::nuke_update_dir();

    let settings = state.settings.load();
    let channel =
        replay_control_core::update::UpdateChannel::from_str_value(settings.update_channel());
    let skipped = settings.skipped_version().map(|s| s.to_string());
    let github_key = settings.github_api_key().map(|s| s.to_string());

    match update_io::check_github_update(
        crate::VERSION,
        &update_io::github_api_base_url(),
        REPO,
        &channel,
        skipped.as_deref(),
        github_key.as_deref(),
    )
    .await?
    {
        Some(available) => {
            update_io::write_available_update(&available).ok();
            let _ = state.events_tx.send(super::ConfigEvent::UpdateAvailable {
                update: available.clone(),
            });
            Ok(Some(available))
        }
        None => Ok(None),
    }
}

/// Generate the helper shell script that performs the actual file swap + restart.
/// `catalog_path` is `None` for releases that don't ship a catalog asset
/// (< v0.4.0-beta.3); the script then leaves the existing catalog in place.
pub fn generate_update_script(
    binary_path: &std::path::Path,
    site_path: &std::path::Path,
    catalog_path: Option<&std::path::Path>,
    version: &str,
) -> String {
    let catalog_src = catalog_path
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    format!(
        r#"#!/bin/bash

# Auto-generated by replay-control {version}
# Performs file swap, restart, health check, and rollback on failure.

validate_version() {{
    local v="$1"
    if ! echo "$v" | grep -qE '^[0-9a-zA-Z._-]+$'; then
        echo "Invalid version string: $v"
        exit 1
    fi
}}

validate_version "{version}"

# Source environment for PORT variable
if [ -f /etc/default/replay-control ]; then
    . /etc/default/replay-control
fi
PORT="${{PORT:-8080}}"

BINARY_SRC="{binary_src}"
SITE_SRC="{site_src}"
CATALOG_SRC="{catalog_src}"
BINARY_DST="/usr/local/bin/replay-control-app"
SITE_DST="/usr/local/share/replay/site"
CATALOG_DST="/usr/local/bin/catalog.sqlite"

# Asset helpers — applied to each (src, dst) pair.
# An empty SRC means "skip this asset" (e.g. catalog on releases < v0.4.0-beta.3).
backup()  {{ local dst="$1"; [ -e "$dst" ] && cp -a "$dst" "${{dst}}.bak" 2>/dev/null || true; }}
# swap returns non-zero when src is empty so callers' `&& chmod` is skipped.
swap()    {{ local src="$1" dst="$2"; [ -n "$src" ] || return 1; rm -rf "$dst"; mv "$src" "$dst"; }}
unbak()   {{ rm -rf "$1.bak"; }}
restore() {{ local dst="$1"; [ -e "${{dst}}.bak" ] || return 0; rm -rf "$dst"; mv "${{dst}}.bak" "$dst"; }}

# Wait for the HTTP response to reach the client
sleep 2

# Back up current files
backup "$BINARY_DST"
backup "$SITE_DST"
backup "$CATALOG_DST"

# Swap files
swap "$BINARY_SRC" "$BINARY_DST" && chmod +x "$BINARY_DST"
swap "$SITE_SRC"   "$SITE_DST"
swap "$CATALOG_SRC" "$CATALOG_DST" && chmod 644 "$CATALOG_DST"

# Restart service
systemctl restart replay-control

# Health check: poll every 2s, up to 30 attempts (60s total)
ATTEMPT=0
MAX_ATTEMPTS=30
while [ "$ATTEMPT" -lt "$MAX_ATTEMPTS" ]; do
    sleep 2
    ATTEMPT=$((ATTEMPT + 1))
    if curl -sf --max-time 10 "http://localhost:${{PORT}}/api/version" > /dev/null 2>&1; then
        # Success: remove backups
        unbak "$BINARY_DST"
        unbak "$SITE_DST"
        unbak "$CATALOG_DST"
        rm -rf "{update_dir}"
        rm -f "$0"
        exit 0
    fi
done

# Failure: restore backups
restore "$BINARY_DST"
restore "$SITE_DST"
restore "$CATALOG_DST"
systemctl restart replay-control
rm -rf "{update_dir}"
rm -f "$0"
exit 1
"#,
        version = version,
        binary_src = binary_path.display(),
        site_src = site_path.display(),
        catalog_src = catalog_src,
        update_dir = replay_control_core::update::UPDATE_DIR,
    )
}

/// Execute the full update flow: resolve URLs, download, extract, generate + spawn helper.
pub async fn start_update(
    state: &super::AppState,
    tag: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if !replay_control_core::update::validate_version(tag) {
        return Err(format!("Invalid version tag: {tag}").into());
    }

    match tokio::time::timeout(UPDATE_TIMEOUT, start_update_inner(state, tag)).await {
        Ok(result) => result,
        Err(_) => {
            update_io::nuke_update_dir();
            Err("Update timed out after 5 minutes".into())
        }
    }
}

async fn start_update_inner(
    state: &super::AppState,
    tag: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use super::activity::{UpdatePhase, UpdateProgress};

    let start_time = std::time::Instant::now();

    // Acquire activity lock.
    let guard = state
        .try_start_activity(super::activity::Activity::Update {
            progress: UpdateProgress {
                phase: UpdatePhase::Downloading,
                downloaded_bytes: 0,
                total_bytes: 0,
                phase_detail: "Resolving download URLs...".to_string(),
                elapsed_secs: 0,
                error: None,
            },
        })
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.into() })?;

    let result = start_update_download(state, tag, &guard, start_time).await;

    match result {
        Ok(()) => {
            // Set Restarting and leak guard — process will be killed by helper script.
            state.update_activity(|act| {
                if let super::activity::Activity::Update { progress } = act {
                    progress.phase = UpdatePhase::Restarting;
                    progress.phase_detail = "Restarting service...".to_string();
                    progress.elapsed_secs = start_time.elapsed().as_secs();
                }
            });
            std::mem::forget(guard);
            Ok(())
        }
        Err(ref e) => {
            let error_msg = e.to_string();
            guard.update(|act| {
                if let super::activity::Activity::Update { progress } = act {
                    progress.phase = UpdatePhase::Failed;
                    progress.phase_detail = error_msg.clone();
                    progress.error = Some(error_msg.clone());
                    progress.elapsed_secs = start_time.elapsed().as_secs();
                }
            });
            update_io::nuke_update_dir();
            result
        }
    }
}

async fn start_update_download(
    state: &super::AppState,
    tag: &str,
    guard: &super::activity::ActivityGuard,
    start_time: std::time::Instant,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use super::activity::UpdatePhase;
    use replay_control_core::update::{UPDATE_DIR, UPDATE_LOCK, UPDATE_SCRIPT};

    let github_key = replay_control_core_server::settings::read_github_api_key(&state.settings);
    let base_url = update_io::github_api_base_url();
    let update_dir = std::path::PathBuf::from(UPDATE_DIR);

    // Acquire file lock (outside update dir, survives nukes).
    let lock_file = std::fs::File::create(UPDATE_LOCK)?;
    use std::os::unix::io::AsRawFd;
    let fd = lock_file.as_raw_fd();
    if unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err("Another update is already in progress".into());
    }

    // Nuke update dir before starting.
    update_io::nuke_update_dir();
    tokio::fs::create_dir_all(&update_dir).await?;

    // Resolve asset URLs.
    let assets = update_io::resolve_asset_urls(&base_url, REPO, tag, github_key.as_deref()).await?;

    // Fetch the checksum manifest up front. Present on current releases;
    // absent on older ones (then we verify via TLS alone, as before).
    let checksums = match &assets.checksums_url {
        Some(url) => Some(update_io::fetch_checksums(url).await?),
        None => {
            tracing::warn!(
                "release {tag} has no checksums.sha256 asset; verifying downloads via TLS only"
            );
            None
        }
    };

    // Use actual sizes from available.json for progress reporting.
    let stored_update = update_io::read_available_update();
    let binary_size = stored_update.as_ref().map(|u| u.binary_size).unwrap_or(0);
    let site_size = stored_update.as_ref().map(|u| u.site_size).unwrap_or(0);
    // Catalog may be absent on releases < v0.4.0-beta.3 — assets.catalog_url
    // is the source of truth; the size hint just feeds progress reporting.
    let catalog_size = stored_update
        .as_ref()
        .map(|u| u.catalog_size)
        .filter(|_| assets.catalog_url.is_some())
        .unwrap_or(0);
    let total_bytes = binary_size + site_size + catalog_size;

    // Check disk space (require 2x total for archives + extracted).
    if total_bytes > 0 {
        let stat = nix::sys::statvfs::statvfs(update_dir.to_str().unwrap_or("/var/tmp"))?;
        let available = stat.blocks_available() as u64 * stat.fragment_size() as u64;
        let required = total_bytes * 2;
        if available < required {
            return Err(format!(
                "Insufficient disk space: need {} MB, have {} MB",
                required / (1024 * 1024),
                available / (1024 * 1024),
            )
            .into());
        }
    }

    // Download binary.
    let binary_archive = update_dir.join("binary.tar.gz");
    {
        let activity_state = state.activity.clone();
        let activity_tx = state.activity_tx.clone();
        let start = start_time;

        update_io::download_asset(&assets.binary_url, &binary_archive, &move |bytes| {
            let mut act = activity_state.write().expect("activity lock");
            if let super::activity::Activity::Update { progress } = &mut *act {
                progress.downloaded_bytes = bytes;
                progress.total_bytes = total_bytes;
                progress.phase_detail = "Downloading binary...".to_string();
                progress.elapsed_secs = start.elapsed().as_secs();
            }
            let activity = act.clone();
            drop(act);
            let _ = activity_tx.send(activity);
        })
        .await?;
    }
    verify_archive(&binary_archive, &assets.binary_url, &checksums).await?;

    // Download site archive.
    let site_archive = update_dir.join("site.tar.gz");
    {
        let activity_state = state.activity.clone();
        let activity_tx = state.activity_tx.clone();
        let start = start_time;

        update_io::download_asset(&assets.site_url, &site_archive, &move |bytes| {
            let mut act = activity_state.write().expect("activity lock");
            if let super::activity::Activity::Update { progress } = &mut *act {
                progress.downloaded_bytes = binary_size + bytes;
                progress.total_bytes = total_bytes;
                progress.phase_detail = "Downloading site assets...".to_string();
                progress.elapsed_secs = start.elapsed().as_secs();
            }
            let activity = act.clone();
            drop(act);
            let _ = activity_tx.send(activity);
        })
        .await?;
    }
    verify_archive(&site_archive, &assets.site_url, &checksums).await?;

    let catalog_archive = update_dir.join("catalog.tar.gz");
    if let Some(catalog_url) = &assets.catalog_url {
        let activity_state = state.activity.clone();
        let activity_tx = state.activity_tx.clone();
        let start = start_time;
        let downloaded_so_far = binary_size + site_size;

        update_io::download_asset(catalog_url, &catalog_archive, &move |bytes| {
            let mut act = activity_state.write().expect("activity lock");
            if let super::activity::Activity::Update { progress } = &mut *act {
                progress.downloaded_bytes = downloaded_so_far + bytes;
                progress.total_bytes = total_bytes;
                progress.phase_detail = "Downloading catalog...".to_string();
                progress.elapsed_secs = start.elapsed().as_secs();
            }
            let activity = act.clone();
            drop(act);
            let _ = activity_tx.send(activity);
        })
        .await?;

        verify_archive(&catalog_archive, catalog_url, &checksums).await?;
    }

    // Extract archives.
    guard.update(|act| {
        if let super::activity::Activity::Update { progress } = act {
            progress.phase = UpdatePhase::Installing;
            progress.phase_detail = "Extracting archives...".to_string();
            progress.elapsed_secs = start_time.elapsed().as_secs();
        }
    });

    let binary_dir = update_dir.join("binary");
    let site_dir = update_dir.join("site");
    tokio::fs::create_dir_all(&binary_dir).await?;
    tokio::fs::create_dir_all(&site_dir).await?;
    extract_tarball(&binary_archive, &binary_dir).await?;
    extract_tarball(&site_archive, &site_dir).await?;

    let mut catalog_path: Option<std::path::PathBuf> = None;
    if assets.catalog_url.is_some() {
        let catalog_dir = update_dir.join("catalog");
        tokio::fs::create_dir_all(&catalog_dir).await?;
        extract_tarball(&catalog_archive, &catalog_dir).await?;
        catalog_path = Some(
            find_extracted_file(&catalog_dir, "catalog.sqlite")
                .await
                .ok_or("Extracted catalog archive does not contain catalog.sqlite")?,
        );
    }

    // Resilient: search for the binary within extracted contents.
    let binary_path = find_extracted_file(&binary_dir, "replay-control-app")
        .await
        .ok_or("Extracted binary not found")?;

    // Resilient: search for pkg/ directory within extracted site.
    let actual_site_dir = find_extracted_dir_containing(&site_dir, "pkg")
        .await
        .ok_or("Extracted site directory does not contain pkg/")?;

    // Generate helper script.
    let version = tag.strip_prefix('v').unwrap_or(tag);
    let script = generate_update_script(
        &binary_path,
        &actual_site_dir,
        catalog_path.as_deref(),
        version,
    );
    let script_path = std::path::PathBuf::from(UPDATE_SCRIPT);
    tokio::fs::write(&script_path, &script).await?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755)).await?;
    }

    // Spawn the helper script via systemd-run so it survives our restart.
    std::process::Command::new("systemd-run")
        .args([
            "--scope",
            "--unit=replay-control-update",
            "--quiet",
            "/bin/bash",
            script_path.to_str().unwrap(),
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;

    // Keep lock_file alive until here.
    drop(lock_file);

    Ok(())
}

/// Verify a downloaded archive against the release checksum manifest.
/// Fails closed if the manifest is present but the file's name is missing
/// from it or its SHA-256 doesn't match. When no manifest exists (older
/// releases), integrity rests on TLS to GitHub, as it always has.
async fn verify_archive(
    path: &std::path::Path,
    url: &str,
    checksums: &Option<std::collections::HashMap<String, String>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let Some(map) = checksums else {
        return Ok(());
    };
    // The manifest lists the release's original asset filenames; each
    // download URL ends with that name.
    let name = url.rsplit('/').next().unwrap_or("");
    let expected = map
        .get(name)
        .ok_or_else(|| format!("no checksum listed for update asset {name}"))?;
    update_io::verify_sha256(path, expected).await?;
    tracing::info!("verified checksum for update asset {name}");
    Ok(())
}

/// Streaming gunzip + untar of a `.tar.gz` archive into `dest`.
async fn extract_tarball(
    archive: &std::path::Path,
    dest: &std::path::Path,
) -> Result<(), std::io::Error> {
    let archive = archive.to_path_buf();
    let dest = dest.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let gz = flate2::read::GzDecoder::new(std::fs::File::open(&archive)?);
        tar::Archive::new(gz).unpack(&dest)
    })
    .await
    .map_err(std::io::Error::other)?
}

/// Search for a file by name within an extracted directory tree.
async fn find_extracted_file(dir: &std::path::Path, name: &str) -> Option<std::path::PathBuf> {
    // Check direct child first.
    let direct = dir.join(name);
    if direct.exists() {
        return Some(direct);
    }
    // Search one level deep.
    let mut entries = tokio::fs::read_dir(dir).await.ok()?;
    while let Ok(Some(entry)) = entries.next_entry().await {
        if entry.file_type().await.ok()?.is_dir() {
            let candidate = entry.path().join(name);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Search for a directory containing a specific subdirectory.
async fn find_extracted_dir_containing(
    dir: &std::path::Path,
    subdir: &str,
) -> Option<std::path::PathBuf> {
    if dir.join(subdir).exists() {
        return Some(dir.to_path_buf());
    }
    let mut entries = tokio::fs::read_dir(dir).await.ok()?;
    while let Ok(Some(entry)) = entries.next_entry().await {
        if entry.file_type().await.ok()?.is_dir() && entry.path().join(subdir).exists() {
            return Some(entry.path());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // ── generate_update_script ──────────────────────────────────────

    #[test]
    fn script_includes_catalog_handling_when_path_some() {
        let script = super::generate_update_script(
            Path::new("/tmp/extracted/replay-control-app"),
            Path::new("/tmp/extracted/site"),
            Some(Path::new("/tmp/extracted/catalog.sqlite")),
            "0.5.0",
        );
        assert!(script.contains(r#"CATALOG_SRC="/tmp/extracted/catalog.sqlite""#));
        assert!(script.contains(r#"CATALOG_DST="/usr/local/bin/catalog.sqlite""#));
        assert!(script.contains(r#"backup "$CATALOG_DST""#));
        assert!(script.contains(r#"swap "$CATALOG_SRC" "$CATALOG_DST""#));
        assert!(script.contains(r#"restore "$CATALOG_DST""#));
        assert!(script.contains("chmod 644"));
    }

    #[test]
    fn script_omits_catalog_swap_when_path_none() {
        let script = super::generate_update_script(
            Path::new("/tmp/extracted/replay-control-app"),
            Path::new("/tmp/extracted/site"),
            None,
            "0.5.0",
        );
        // Empty CATALOG_SRC makes swap return non-zero; backup/restore are
        // still emitted but become no-ops on a non-existent backup file.
        assert!(script.contains(r#"CATALOG_SRC="""#));
        // Helper functions are always declared (they're cheap).
        assert!(script.contains(r#"swap()"#));
    }

    #[test]
    fn script_validates_version() {
        let script = super::generate_update_script(
            Path::new("/tmp/binary"),
            Path::new("/tmp/site"),
            None,
            "0.5.0",
        );
        assert!(script.contains(r#"validate_version "0.5.0""#));
        assert!(script.contains("Invalid version string"));
    }

    // ── extract_tarball ─────────────────────────────────────────────

    fn build_tarball(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let gz = flate2::write::GzEncoder::new(&mut buf, flate2::Compression::fast());
            let mut tar = tar::Builder::new(gz);
            for (path, content) in files {
                let mut header = tar::Header::new_gnu();
                header.set_size(content.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                tar.append_data(&mut header, path, *content).unwrap();
            }
            tar.into_inner().unwrap().finish().unwrap();
        }
        buf
    }

    #[tokio::test]
    async fn extract_tarball_writes_expected_files() {
        let dir = tempfile::tempdir().unwrap();
        let archive_path = dir.path().join("test.tar.gz");
        std::fs::write(
            &archive_path,
            build_tarball(&[("hello.txt", b"hello"), ("nested/world.txt", b"world")]),
        )
        .unwrap();

        let dest = dir.path().join("out");
        std::fs::create_dir_all(&dest).unwrap();
        super::extract_tarball(&archive_path, &dest).await.unwrap();

        assert_eq!(
            std::fs::read_to_string(dest.join("hello.txt")).unwrap(),
            "hello"
        );
        assert_eq!(
            std::fs::read_to_string(dest.join("nested/world.txt")).unwrap(),
            "world"
        );
    }
}
