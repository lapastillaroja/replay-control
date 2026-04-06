use replay_control_core::metadata_db::MetadataDb;
use std::time::Duration;

use super::AppState;
use super::activity::{Activity, StartupPhase};
use super::import::ImportPipeline;
use super::library::dir_mtime;

/// How often the background task re-checks storage (in seconds).
const STORAGE_CHECK_INTERVAL: u64 = 60;

/// Download URLs for a specific release, resolved fresh from GitHub API.
#[derive(Debug)]
pub struct AssetUrls {
    pub binary_url: String,
    pub site_url: String,
}

/// Orchestrates the ordered background startup pipeline and long-running watchers.
///
/// Pipeline phases (sequential, async):
///   1. Auto-import — if a LaunchBox XML file exists and the DB is empty
///   2. Cache populate/verify — scan all systems, enrich box art + ratings
///   3. Auto-rebuild thumbnail index — if data_sources exist but index is empty (data loss)
///
/// Filesystem watchers (config file, ROM directory) run independently.
pub struct BackgroundManager;

impl BackgroundManager {
    /// Start the ordered background pipeline.
    pub fn start(state: AppState) {
        // Clean up stale update temp files from a previous run.
        Self::nuke_update_dir();

        // Spawn the ordered pipeline as an async task.
        let pipeline_state = state.clone();
        tokio::spawn(async move {
            Self::run_pipeline(&pipeline_state).await;
        });

        // Start watchers immediately (they're independent of the pipeline).
        state.clone().spawn_storage_watcher();
        state.spawn_rom_watcher();

        // Spawn update checker (independent of pipeline, no activity lock needed).
        let update_state = state.clone();
        tokio::spawn(async move {
            Self::update_check_loop(update_state).await;
        });
    }

    /// Run the ordered startup pipeline (async).
    async fn run_pipeline(state: &AppState) {
        // Brief delay to let the server start accepting requests.
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Phase 1: Auto-import (if launchbox XML exists + DB empty).
        // Import claims/releases its own Activity::Import via try_start_activity.
        Self::phase_auto_import(state).await;

        // Wait for auto-import to finish (check activity state).
        while ImportPipeline::has_active_import(state) {
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        // Phase 2+3: Claim Activity::Startup for populate + thumbnail rebuild.
        // Guard drops → Idle on completion or panic.
        {
            let _guard = match state.try_start_activity(Activity::Startup {
                phase: StartupPhase::Scanning,
                system: String::new(),
            }) {
                Ok(g) => g,
                Err(e) => {
                    tracing::warn!("Could not start startup pipeline: {e}");
                    return;
                }
            };

            Self::phase_cache_verification(state).await;
            // Checkpoint after Phase 2 writes (game_library inserts/updates).
            state.metadata_pool.checkpoint().await;

            state.update_activity(|act| {
                if let Activity::Startup { phase, .. } = act {
                    *phase = StartupPhase::RebuildingIndex;
                }
            });
            Self::phase_auto_rebuild_thumbnail_index(state).await;

            // _guard drops → Idle
        }
    }

    /// Phase 1: Auto-import metadata on startup if `launchbox-metadata.xml` exists and DB is empty.
    async fn phase_auto_import(state: &AppState) {
        use replay_control_core::metadata_db::LAUNCHBOX_XML;

        let storage = state.storage();
        let rc_dir = storage.rc_dir();
        let xml_path = rc_dir.join(LAUNCHBOX_XML);
        // Backwards-compat: fall back to old upstream name if user placed it manually.
        let xml_path = if xml_path.exists() {
            xml_path
        } else {
            let old_path = rc_dir.join("Metadata.xml");
            if old_path.exists() {
                old_path
            } else {
                xml_path
            }
        };

        if !xml_path.exists() {
            tracing::debug!(
                "No {} at {}, skipping auto-import",
                LAUNCHBOX_XML,
                xml_path.display()
            );
            return;
        }

        let should_import = state
            .metadata_pool
            .read(|conn| MetadataDb::is_empty(conn).unwrap_or(false))
            .await
            .unwrap_or(false);

        if should_import {
            tracing::info!("Auto-importing metadata from {}", xml_path.display());
            state.import.start_import_no_enrich(xml_path, state.clone());
        }
    }

    /// Phase 2: Verify L2 cache freshness on startup and re-scan stale/incomplete systems.
    ///
    /// Works directly with the DB and filesystem — does NOT use the cache layer
    /// (cached_systems, cached_roms, etc.) to avoid circular dependencies.
    ///
    /// Detects three cases:
    /// - **Fresh DB**: `game_library_meta` is empty → full populate
    /// - **Stale mtime**: directory mtime changed since last scan → re-scan
    /// - **Interrupted scan**: meta says rom_count > 0 but game_library has 0 rows → re-scan
    async fn phase_cache_verification(state: &AppState) {
        let storage = state.storage();
        let roms_dir = storage.roms_dir();
        let region_pref = state.region_preference();
        let region_secondary = state.region_preference_secondary();

        // Load cached system metadata directly from DB (no cache layer).
        let cached_meta = state
            .metadata_pool
            .read(|conn| MetadataDb::load_all_system_meta(conn).ok())
            .await
            .flatten()
            .unwrap_or_default();

        if cached_meta.is_empty() {
            // Fresh DB — full populate.
            Self::populate_all_systems(state, &storage, region_pref, region_secondary).await;
            return;
        }

        // Query actual game_library row counts per system to detect interrupted scans.
        let actual_counts: std::collections::HashMap<String, usize> = state
            .metadata_pool
            .read(|conn| {
                let mut stmt = conn
                    .prepare("SELECT system, COUNT(*) FROM game_library GROUP BY system")
                    .ok()?;
                let rows = stmt
                    .query_map([], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
                    })
                    .ok()?;
                Some(rows.flatten().collect())
            })
            .await
            .flatten()
            .unwrap_or_default();

        let mut rescan_count = 0usize;
        for meta in &cached_meta {
            let system_dir = roms_dir.join(&meta.system);
            let current_mtime_secs = dir_mtime(&system_dir).and_then(|t| {
                t.duration_since(std::time::UNIX_EPOCH)
                    .ok()
                    .map(|d| d.as_secs() as i64)
            });

            let is_stale = match (meta.dir_mtime_secs, current_mtime_secs) {
                (Some(cached), Some(current)) => cached != current,
                (Some(_), None) => false, // Can't read — trust cache
                (None, _) => true,        // No mtime stored — re-scan
            };

            // Interrupted scan: meta says ROMs exist but game_library has none.
            let is_incomplete =
                meta.rom_count > 0 && actual_counts.get(&meta.system).copied().unwrap_or(0) == 0;

            if is_stale || is_incomplete {
                let reason = if is_incomplete {
                    "incomplete"
                } else {
                    "mtime changed"
                };
                tracing::info!("Background re-scan: {} ({reason})", meta.system);
                let display_name = replay_control_core::systems::find_system(&meta.system)
                    .map(|s| s.display_name.to_string())
                    .unwrap_or_else(|| meta.system.clone());
                state.update_activity(|act| {
                    if let Activity::Startup { system, .. } = act {
                        *system = display_name;
                    }
                });
                let _ = state
                    .cache
                    .scan_and_cache_system(
                        &storage,
                        &meta.system,
                        region_pref,
                        region_secondary,
                        &state.metadata_pool,
                    )
                    .await;
                state
                    .cache
                    .enrich_system_cache(state, meta.system.clone())
                    .await;
                rescan_count += 1;
            }
        }

        if rescan_count > 0 {
            tracing::info!("Background cache verification: re-scanned {rescan_count} system(s)");
        } else {
            tracing::debug!(
                "Background cache verification: all {} system(s) fresh",
                cached_meta.len()
            );
        }
    }

    /// Phase 3: Rebuild thumbnail index if there's evidence of data loss.
    ///
    /// Triggers when `data_sources` has libretro-thumbnails entries (meaning the user
    /// previously ran "Update Thumbnails") but `thumbnail_index` is empty (data lost,
    /// e.g., due to DB corruption and auto-recreate). Does NOT download images — only
    /// rebuilds the index so box art variant picker and on-demand downloads work.
    ///
    /// Skips when both tables are empty (first-time setup — user hasn't configured
    /// thumbnails yet) to avoid wasting time on GitHub API calls when offline.
    async fn phase_auto_rebuild_thumbnail_index(state: &AppState) {
        // Check data_sources for libretro-thumbnails entries and thumbnail_index emptiness.
        let (has_sources, index_empty) = match state
            .metadata_pool
            .read(|conn| {
                let stats = MetadataDb::get_data_source_stats(conn, "libretro-thumbnails").ok()?;
                let index_count: i64 = MetadataDb::thumbnail_index_count(conn).unwrap_or(0);
                Some((stats.repo_count > 0, index_count == 0))
            })
            .await
            .flatten()
        {
            Some(result) => result,
            None => return, // DB unavailable
        };

        if !has_sources {
            // No data_sources entries. Check if images exist on disk — if so,
            // someone previously downloaded thumbnails but the DB was deleted.
            let has_images_on_disk =
                replay_control_core::thumbnails::any_images_on_disk(&state.storage().rc_dir());
            if !has_images_on_disk {
                tracing::debug!(
                    "No libretro-thumbnails data sources and no images on disk, skipping thumbnail index rebuild"
                );
                return;
            }
            tracing::info!(
                "Fresh DB but images exist on disk — rebuilding thumbnail index from GitHub API"
            );
        } else if !index_empty {
            tracing::debug!("Thumbnail index already populated, skipping rebuild");
            return;
        } else {
            tracing::info!(
                "Thumbnail data sources exist but index is empty (data loss?) — rebuilding index from GitHub API"
            );
        }

        // Rebuild index from images on disk — no GitHub API needed.
        // Scan media/<system>/boxart/ directories and insert filenames into thumbnail_index.
        let storage = state.storage();
        let media_dir = storage.rc_dir().join("media");

        let Ok(systems) = std::fs::read_dir(&media_dir) else {
            return;
        };

        // Collect all system image data from disk first (no DB needed).
        struct SystemImageData {
            system_str: String,
            repo_names: &'static [&'static str],
            entries: Vec<(String, String, Option<String>)>,
        }

        let mut system_data: Vec<SystemImageData> = Vec::new();
        for system_entry in systems.flatten() {
            let system_name = system_entry.file_name();
            let system_str = system_name.to_string_lossy().into_owned();

            let Some(repo_names) =
                replay_control_core::thumbnails::thumbnail_repo_names(&system_str)
            else {
                continue;
            };

            let all_entries =
                replay_control_core::thumbnails::scan_system_images(&system_entry.path());

            if all_entries.is_empty() {
                continue;
            }

            system_data.push(SystemImageData {
                system_str,
                repo_names,
                entries: all_entries,
            });
        }

        // Now write all collected data to the DB in a single write() call.
        let write_result = state
            .metadata_pool
            .write(move |db| {
                let mut w_total_entries = 0usize;
                let mut w_total_repos = 0usize;

                for data in &system_data {
                    let repo_display = data.repo_names[0];
                    let source_name =
                        replay_control_core::thumbnails::libretro_source_name(repo_display);
                    let branch =
                        replay_control_core::thumbnail_manifest::default_branch(repo_display);
                    let entry_count = data.entries.len();

                    let _ = MetadataDb::upsert_data_source(
                        db,
                        &source_name,
                        "libretro-thumbnails",
                        "disk-rebuild",
                        branch,
                        entry_count,
                    );

                    match MetadataDb::bulk_insert_thumbnail_index(db, &source_name, &data.entries) {
                        Ok(_) => w_total_entries += entry_count,
                        Err(e) => tracing::warn!(
                            "Failed to insert disk-based index for {}: {e}",
                            data.system_str
                        ),
                    }

                    // Register additional repos for multi-repo systems (e.g., arcade_dc → Naomi + Naomi 2).
                    for extra_repo in &data.repo_names[1..] {
                        let extra_source =
                            replay_control_core::thumbnails::libretro_source_name(extra_repo);
                        let extra_branch =
                            replay_control_core::thumbnail_manifest::default_branch(extra_repo);
                        let _ = MetadataDb::upsert_data_source(
                            db,
                            &extra_source,
                            "libretro-thumbnails",
                            "disk-rebuild",
                            extra_branch,
                            0,
                        );
                    }
                    w_total_repos += data.repo_names.len();
                }

                (w_total_entries, w_total_repos)
            })
            .await;

        let Some((total_entries, total_repos)) = write_result else {
            return; // DB unavailable
        };

        if total_entries > 0 {
            // Checkpoint WAL after the bulk thumbnail index writes.
            state.metadata_pool.checkpoint().await;
            tracing::info!(
                "Thumbnail index rebuilt from disk: {total_entries} entries across {total_repos} repos"
            );
        }
    }

    /// Pre-populate L2 cache for all systems that have games.
    /// Called on startup when the game library is empty (fresh DB or after clear).
    /// After populating ROMs, enriches box art URLs and ratings.
    pub(crate) async fn populate_all_systems(
        state: &AppState,
        storage: &replay_control_core::storage::StorageLocation,
        region_pref: replay_control_core::rom_tags::RegionPreference,
        region_secondary: Option<replay_control_core::rom_tags::RegionPreference>,
    ) {
        let systems = state
            .cache
            .cached_systems(storage, &state.metadata_pool)
            .await;
        let with_games: Vec<_> = systems.iter().filter(|s| s.game_count > 0).collect();
        tracing::info!(
            "L2 warmup: populating {} system(s) with games",
            with_games.len()
        );

        let start = std::time::Instant::now();
        let mut total_roms = 0usize;
        for sys in &with_games {
            state.update_activity(|act| {
                if let Activity::Startup { system, .. } = act {
                    *system = sys.display_name.clone();
                }
            });
            match state
                .cache
                .scan_and_cache_system(
                    storage,
                    &sys.folder_name,
                    region_pref,
                    region_secondary,
                    &state.metadata_pool,
                )
                .await
            {
                Ok(roms) => {
                    tracing::debug!("L2 warmup: {} — {} ROMs", sys.folder_name, roms.len());
                    total_roms += roms.len();
                }
                Err(e) => tracing::warn!("L2 warmup: failed to scan {}: {e}", sys.folder_name),
            }
        }

        tracing::info!(
            "L2 warmup: scanned {} ROMs across {} systems in {:.1}s, enriching...",
            total_roms,
            with_games.len(),
            start.elapsed().as_secs_f64()
        );

        // Enrich box art URLs and ratings for all systems.
        for sys in &with_games {
            state.update_activity(|act| {
                if let Activity::Startup { system, .. } = act {
                    *system = format!("{} (enriching)", sys.display_name);
                }
            });
            state
                .cache
                .enrich_system_cache(state, sys.folder_name.clone())
                .await;
        }

        tracing::info!(
            "L2 warmup: done -- {} ROMs across {} systems in {:.1}s",
            total_roms,
            with_games.len(),
            start.elapsed().as_secs_f64()
        );
    }
    // ── Update system ─────────────────────────────────────────────────

    /// GitHub repository for release checks and downloads.
    const REPO: &'static str = "lapastillaroja/replay-control";
    /// Maximum time for the entire StartUpdate operation (5 minutes).
    const UPDATE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

    /// GitHub API base URL. Overridable via `REPLAY_GITHUB_API_URL` for testing.
    pub fn github_api_base_url() -> String {
        std::env::var("REPLAY_GITHUB_API_URL")
            .unwrap_or_else(|_| "https://api.github.com".to_string())
    }

    /// Nuke the update temp directory (idempotent).
    pub fn nuke_update_dir() {
        use replay_control_core::update::{UPDATE_DIR, UPDATE_SCRIPT};
        let dir = std::path::Path::new(UPDATE_DIR);
        if dir.exists() {
            let _ = std::fs::remove_dir_all(dir);
        }
        let script = std::path::Path::new(UPDATE_SCRIPT);
        if script.exists() {
            let _ = std::fs::remove_file(script);
        }
    }

    /// Read `available.json` from the update temp directory.
    pub fn read_available_update() -> Option<replay_control_core::update::AvailableUpdate> {
        let path =
            std::path::Path::new(replay_control_core::update::UPDATE_DIR).join("available.json");
        let data = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&data).ok()
    }

    /// Write `available.json` to the update temp directory.
    fn write_available_update(
        update: &replay_control_core::update::AvailableUpdate,
    ) -> std::io::Result<()> {
        let dir = std::path::Path::new(replay_control_core::update::UPDATE_DIR);
        std::fs::create_dir_all(dir)?;
        let path = dir.join("available.json");
        let json = serde_json::to_string(update).map_err(std::io::Error::other)?;
        std::fs::write(path, json)
    }

    /// Periodically checks GitHub for new releases.
    async fn update_check_loop(state: AppState) {
        // Delay first check to let WiFi come up on Pi.
        tokio::time::sleep(Duration::from_secs(60)).await;

        loop {
            if state.has_storage() {
                match Self::perform_update_check_background(&state).await {
                    Ok(_) => {}
                    Err(e) => tracing::debug!("Background update check failed: {e}"),
                }
            }

            tokio::time::sleep(Duration::from_secs(24 * 60 * 60)).await;
        }
    }

    /// Background check variant: does NOT nuke before checking (preserves existing
    /// available.json on error). On success: nuke then write. On no-update: nuke.
    async fn perform_update_check_background(
        state: &AppState,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let storage = state.storage();
        let settings = replay_control_core::settings::load_settings(&storage.root);
        let channel =
            replay_control_core::update::UpdateChannel::from_str_value(settings.update_channel());
        let skipped = settings.skipped_version().map(|s| s.to_string());
        let github_key = settings.github_api_key().map(|s| s.to_string());
        drop(settings);

        match Self::check_github_update(
            crate::VERSION,
            &Self::github_api_base_url(),
            &channel,
            skipped.as_deref(),
            github_key.as_deref(),
        )
        .await?
        {
            Some(available) => {
                // Race guard: verify channel still matches before writing.
                let current_channel =
                    replay_control_core::settings::read_update_channel(&storage.root);
                if current_channel != channel {
                    tracing::debug!(
                        "Update channel changed during check ({} -> {}), discarding result",
                        channel.as_str(),
                        current_channel.as_str()
                    );
                    return Ok(());
                }
                Self::nuke_update_dir();
                Self::write_available_update(&available).ok();
                let _ = state
                    .config_tx
                    .send(super::ConfigEvent::UpdateAvailable { update: available });
            }
            None => {
                // No update found — nuke stale state.
                Self::nuke_update_dir();
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
        Self::nuke_update_dir();

        let storage = state.storage();
        let settings = replay_control_core::settings::load_settings(&storage.root);
        let channel =
            replay_control_core::update::UpdateChannel::from_str_value(settings.update_channel());
        let skipped = settings.skipped_version().map(|s| s.to_string());
        let github_key = settings.github_api_key().map(|s| s.to_string());

        match Self::check_github_update(
            crate::VERSION,
            &Self::github_api_base_url(),
            &channel,
            skipped.as_deref(),
            github_key.as_deref(),
        )
        .await?
        {
            Some(available) => {
                Self::write_available_update(&available).ok();
                let _ = state.config_tx.send(super::ConfigEvent::UpdateAvailable {
                    update: available.clone(),
                });
                Ok(Some(available))
            }
            None => Ok(None),
        }
    }

    /// Check GitHub for a newer release than the running version.
    pub async fn check_github_update(
        current_version: &str,
        base_url: &str,
        channel: &replay_control_core::update::UpdateChannel,
        skipped_version: Option<&str>,
        github_api_key: Option<&str>,
    ) -> Result<
        Option<replay_control_core::update::AvailableUpdate>,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        let release = match channel {
            replay_control_core::update::UpdateChannel::Beta => {
                Self::fetch_latest_beta(current_version, base_url, Self::REPO, github_api_key)
                    .await?
            }
            replay_control_core::update::UpdateChannel::Stable => {
                Self::fetch_latest_stable(base_url, Self::REPO, github_api_key).await?
            }
        };

        let Some(release) = release else {
            return Ok(None);
        };

        if !replay_control_core::update::is_newer(current_version, &release.version) {
            return Ok(None);
        }

        if let Some(skipped) = skipped_version
            && release.version == skipped
        {
            return Ok(None);
        }

        Ok(Some(release))
    }

    /// Fetch the latest stable release via /releases/latest.
    /// Returns Ok(None) if no stable release exists (GitHub returns 404).
    async fn fetch_latest_stable(
        base_url: &str,
        repo: &str,
        api_key: Option<&str>,
    ) -> Result<
        Option<replay_control_core::update::AvailableUpdate>,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        let url = format!("{base_url}/repos/{repo}/releases/latest");
        match Self::github_get(&url, api_key).await {
            Ok(json) => Ok(Self::parse_release(&json)),
            Err(e) => {
                // 404 means no stable release exists — not an error.
                if e.to_string().contains("404") {
                    Ok(None)
                } else {
                    Err(e)
                }
            }
        }
    }

    /// Fetch the latest beta by querying /releases and picking newest by semver.
    async fn fetch_latest_beta(
        current_version: &str,
        base_url: &str,
        repo: &str,
        api_key: Option<&str>,
    ) -> Result<
        Option<replay_control_core::update::AvailableUpdate>,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        let url = format!("{base_url}/repos/{repo}/releases?per_page=10");
        let json = Self::github_get(&url, api_key).await?;

        let empty = vec![];
        let releases = json.as_array().unwrap_or(&empty);

        let mut best: Option<replay_control_core::update::AvailableUpdate> = None;
        for release in releases {
            if let Some(parsed) = Self::parse_release(release)
                && replay_control_core::update::is_newer(current_version, &parsed.version)
                && best.as_ref().is_none_or(|b| {
                    replay_control_core::update::is_newer(&b.version, &parsed.version)
                })
            {
                best = Some(parsed);
            }
        }
        Ok(best)
    }

    /// Parse a GitHub release JSON object into an AvailableUpdate.
    pub fn parse_release(
        json: &serde_json::Value,
    ) -> Option<replay_control_core::update::AvailableUpdate> {
        let tag = json.get("tag_name")?.as_str()?;
        let version = tag.strip_prefix('v').unwrap_or(tag);

        if semver::Version::parse(version).is_err() {
            return None;
        }

        let prerelease = json
            .get("prerelease")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let html_url = json
            .get("html_url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let published_at = json
            .get("published_at")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let assets = json.get("assets").and_then(|v| v.as_array());
        let mut binary_size = 0u64;
        let mut site_size = 0u64;
        if let Some(assets) = assets {
            for asset in assets {
                let name = asset.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let size = asset.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
                if name.contains("replay-control-app") {
                    binary_size = size;
                } else if name.contains("site") {
                    site_size = size;
                }
            }
        }

        Some(replay_control_core::update::AvailableUpdate {
            version: version.to_string(),
            tag: tag.to_string(),
            prerelease,
            release_notes_url: html_url,
            published_at,
            binary_size,
            site_size,
        })
    }

    /// Shared HTTP client for all GitHub API and download requests.
    /// Uses a 10s default timeout; callers can override per-request.
    fn http_client() -> &'static reqwest::Client {
        use std::sync::OnceLock;
        static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
        CLIENT.get_or_init(|| {
            reqwest::Client::builder()
                .user_agent(format!("replay-control/{}", crate::VERSION))
                .timeout(Duration::from_secs(10))
                .build()
                .expect("Failed to create HTTP client")
        })
    }

    /// HTTP GET with optional Authorization for the GitHub API.
    pub async fn github_get(
        url: &str,
        api_key: Option<&str>,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        let mut req = Self::http_client()
            .get(url)
            .header("Accept", "application/vnd.github+json");

        if let Some(key) = api_key {
            req = req.header("Authorization", format!("Bearer {key}"));
        }

        let resp = req.send().await?.error_for_status()?;
        Ok(resp.json().await?)
    }

    /// Resolve fresh download URLs for a given release tag.
    pub async fn resolve_asset_urls(
        base_url: &str,
        tag: &str,
        api_key: Option<&str>,
    ) -> Result<AssetUrls, Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("{base_url}/repos/{}/releases/tags/{tag}", Self::REPO);
        let release = Self::github_get(&url, api_key).await?;

        let assets = release
            .get("assets")
            .and_then(|v| v.as_array())
            .ok_or("No assets found in release")?;

        let mut binary_url = None;
        let mut site_url = None;

        for asset in assets {
            let name = asset.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let download_url = asset.get("browser_download_url").and_then(|v| v.as_str());
            if let Some(url) = download_url {
                if name.contains("replay-control-app") && name.ends_with(".tar.gz") {
                    binary_url = Some(url.to_string());
                } else if name.contains("site") && name.ends_with(".tar.gz") {
                    site_url = Some(url.to_string());
                }
            }
        }

        Ok(AssetUrls {
            binary_url: binary_url.ok_or("Binary asset not found in release")?,
            site_url: site_url.ok_or("Site asset not found in release")?,
        })
    }

    /// Download a file from a URL to a local path, reporting progress via callback.
    /// Progress callback is throttled to at most once per 250ms.
    pub async fn download_asset(
        url: &str,
        dest: &std::path::Path,
        progress_cb: &(dyn Fn(u64) + Send + Sync),
    ) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
        use tokio::io::AsyncWriteExt;
        use tokio_stream::StreamExt;

        let resp = Self::http_client()
            .get(url)
            .timeout(Duration::from_secs(300))
            .send()
            .await?
            .error_for_status()?;

        let mut file = tokio::fs::File::create(dest).await?;
        let mut stream = resp.bytes_stream();
        let mut downloaded = 0u64;
        let mut last_report = std::time::Instant::now();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            file.write_all(&chunk).await?;
            downloaded += chunk.len() as u64;

            if last_report.elapsed() >= std::time::Duration::from_millis(250) {
                progress_cb(downloaded);
                last_report = std::time::Instant::now();
            }
        }

        progress_cb(downloaded);
        file.flush().await?;
        Ok(downloaded)
    }

    /// Generate the helper shell script that performs the actual file swap + restart.
    pub fn generate_update_script(
        binary_path: &std::path::Path,
        site_path: &std::path::Path,
        version: &str,
    ) -> String {
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
BINARY_DST="/usr/local/bin/replay-control-app"
SITE_DST="/usr/local/share/replay/site"

# Wait for the HTTP response to reach the client
sleep 2

# Back up current files
cp "$BINARY_DST" "${{BINARY_DST}}.bak" 2>/dev/null
cp -a "$SITE_DST" "${{SITE_DST}}.bak" 2>/dev/null

# Swap files
mv "$BINARY_SRC" "$BINARY_DST"
rm -rf "$SITE_DST"
mv "$SITE_SRC" "$SITE_DST"
chmod +x "$BINARY_DST"

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
        rm -f "${{BINARY_DST}}.bak"
        rm -rf "${{SITE_DST}}.bak"
        rm -rf "{update_dir}"
        rm -f "$0"
        exit 0
    fi
done

# Failure: restore backups
if [ -f "${{BINARY_DST}}.bak" ]; then
    mv "${{BINARY_DST}}.bak" "$BINARY_DST"
fi
if [ -d "${{SITE_DST}}.bak" ]; then
    rm -rf "$SITE_DST"
    mv "${{SITE_DST}}.bak" "$SITE_DST"
fi
systemctl restart replay-control
rm -rf "{update_dir}"
rm -f "$0"
exit 1
"#,
            version = version,
            binary_src = binary_path.display(),
            site_src = site_path.display(),
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

        match tokio::time::timeout(Self::UPDATE_TIMEOUT, Self::start_update_inner(state, tag)).await
        {
            Ok(result) => result,
            Err(_) => {
                Self::nuke_update_dir();
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

        let result = Self::start_update_download(state, tag, &guard, start_time).await;

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
                Self::nuke_update_dir();
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

        let storage = state.storage();
        let github_key = replay_control_core::settings::read_github_api_key(&storage.root);
        let base_url = Self::github_api_base_url();
        let update_dir = std::path::PathBuf::from(UPDATE_DIR);

        // Acquire file lock (outside update dir, survives nukes).
        let lock_file = std::fs::File::create(UPDATE_LOCK)?;
        use std::os::unix::io::AsRawFd;
        let fd = lock_file.as_raw_fd();
        if unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err("Another update is already in progress".into());
        }

        // Nuke update dir before starting.
        Self::nuke_update_dir();
        tokio::fs::create_dir_all(&update_dir).await?;

        // Resolve asset URLs.
        let assets = Self::resolve_asset_urls(&base_url, tag, github_key.as_deref()).await?;

        // Use actual sizes from available.json for progress reporting.
        let stored_update = Self::read_available_update();
        let binary_size = stored_update.as_ref().map(|u| u.binary_size).unwrap_or(0);
        let total_bytes = stored_update
            .map(|u| u.binary_size + u.site_size)
            .unwrap_or(0);

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

            Self::download_asset(&assets.binary_url, &binary_archive, &move |bytes| {
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

        // Download site archive.
        let site_archive = update_dir.join("site.tar.gz");
        {
            let activity_state = state.activity.clone();
            let activity_tx = state.activity_tx.clone();
            let start = start_time;

            Self::download_asset(&assets.site_url, &site_archive, &move |bytes| {
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

        // Extract binary tarball.
        let binary_archive_path = binary_archive.clone();
        let binary_dir_path = binary_dir.clone();
        tokio::task::spawn_blocking(move || {
            let file = std::fs::File::open(&binary_archive_path)?;
            let gz = flate2::read::GzDecoder::new(file);
            let mut archive = tar::Archive::new(gz);
            archive.unpack(&binary_dir_path)?;
            Ok::<_, std::io::Error>(())
        })
        .await??;

        // Extract site tarball.
        let site_archive_path = site_archive.clone();
        let site_dir_path = site_dir.clone();
        tokio::task::spawn_blocking(move || {
            let file = std::fs::File::open(&site_archive_path)?;
            let gz = flate2::read::GzDecoder::new(file);
            let mut archive = tar::Archive::new(gz);
            archive.unpack(&site_dir_path)?;
            Ok::<_, std::io::Error>(())
        })
        .await??;

        // Resilient: search for the binary within extracted contents.
        let binary_path = Self::find_extracted_file(&binary_dir, "replay-control-app")
            .await
            .ok_or("Extracted binary not found")?;

        // Resilient: search for pkg/ directory within extracted site.
        let actual_site_dir = Self::find_extracted_dir_containing(&site_dir, "pkg")
            .await
            .ok_or("Extracted site directory does not contain pkg/")?;

        // Generate helper script.
        let version = tag.strip_prefix('v').unwrap_or(tag);
        let script = Self::generate_update_script(&binary_path, &actual_site_dir, version);
        let script_path = std::path::PathBuf::from(UPDATE_SCRIPT);
        tokio::fs::write(&script_path, &script).await?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))
                .await?;
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
}

// ── Methods that remain on AppState ────────────────────────────────
//
// These are the long-running watchers and the cache enrichment helper
// that various parts of the code still call on AppState.
impl AppState {
    /// Re-enrich game library for all systems after a metadata or thumbnail import.
    /// If game library is empty (e.g., DB was deleted and recreated during import),
    /// does a full populate first (scan ROMs + enrich). Otherwise just enriches
    /// existing entries with updated box art URLs and ratings.
    pub fn spawn_cache_enrichment(&self) {
        let state = self.clone();
        tokio::spawn(async move {
            let storage = state.storage();
            let region_pref = state.region_preference();
            let region_secondary = state.region_preference_secondary();

            // Check if game library is empty -- if so, populate before enriching.
            let is_empty = state
                .metadata_pool
                .read(|conn| {
                    MetadataDb::load_all_system_meta(conn)
                        .map(|m| m.is_empty())
                        .unwrap_or(true)
                })
                .await
                .unwrap_or(true);

            if is_empty {
                tracing::info!("Post-import: game library is empty, running full populate");
                // Gate reads during heavy writes to prevent exFAT corruption.
                let _write_gate = super::WriteGate::activate(state.metadata_pool.write_gate_flag());
                BackgroundManager::populate_all_systems(
                    &state,
                    &storage,
                    region_pref,
                    region_secondary,
                )
                .await;
                state.metadata_pool.checkpoint().await;
                drop(_write_gate);
            }

            // Enrichment phase: update box art URLs and ratings for all systems.
            // NOTE: enrichment writes are NOT gated because enrich_system_cache
            // reads from the DB (LaunchBox metadata, existing genres, etc.) and
            // the write gate blocks ALL reads on the same pool. Gating here would
            // cause enrichment reads to return None, silently skipping all updates.
            // Enrichment writes are small per-system UPDATEs (not bulk INSERTs),
            // so the exFAT corruption risk is low.
            let systems = state
                .cache
                .cached_systems(&storage, &state.metadata_pool)
                .await;
            let with_games: Vec<_> = systems.into_iter().filter(|s| s.game_count > 0).collect();

            if !with_games.is_empty() {
                tracing::info!(
                    "Post-import enrichment: updating {} system(s)",
                    with_games.len()
                );
                let enrich_start = std::time::Instant::now();
                for sys in &with_games {
                    state
                        .cache
                        .enrich_system_cache(&state, sys.folder_name.clone())
                        .await;
                }
                tracing::info!(
                    "Post-import enrichment: done in {:.1}s",
                    enrich_start.elapsed().as_secs_f64()
                );
            }
        });
    }

    /// Run cache enrichment as part of a rebuild operation (with an ActivityGuard).
    /// Updates `Activity::Rebuild` progress as it goes. The guard drops → Idle on completion.
    pub fn spawn_rebuild_enrichment(&self, guard: super::activity::ActivityGuard) {
        use super::activity::RebuildPhase;

        let state = self.clone();
        let start = std::time::Instant::now();

        tokio::spawn(async move {
            let storage = state.storage();
            let region_pref = state.region_preference();
            let region_secondary = state.region_preference_secondary();

            // Check if game library is empty -- if so, populate before enriching.
            let is_empty = state
                .metadata_pool
                .read(|conn| {
                    MetadataDb::load_all_system_meta(conn)
                        .map(|m| m.is_empty())
                        .unwrap_or(true)
                })
                .await
                .unwrap_or(true);

            if is_empty {
                tracing::info!("Rebuild: game library is empty, running full populate");
                state.update_activity(|act| {
                    if let Activity::Rebuild { progress } = act {
                        progress.phase = RebuildPhase::Scanning;
                        progress.elapsed_secs = start.elapsed().as_secs();
                    }
                });
                // Gate reads during heavy writes to prevent exFAT corruption.
                let _write_gate = super::WriteGate::activate(state.metadata_pool.write_gate_flag());
                BackgroundManager::populate_all_systems(
                    &state,
                    &storage,
                    region_pref,
                    region_secondary,
                )
                .await;
                state.metadata_pool.checkpoint().await;
                drop(_write_gate);
            }

            // Enrichment phase: update box art URLs and ratings for all systems.
            // NOTE: enrichment writes are NOT gated because enrich_system_cache
            // reads from the DB and the write gate blocks ALL reads on the same pool.
            // Enrichment writes are small per-system UPDATEs, not bulk INSERTs.
            let systems = state
                .cache
                .cached_systems(&storage, &state.metadata_pool)
                .await;
            let with_games: Vec<_> = systems.into_iter().filter(|s| s.game_count > 0).collect();

            state.update_activity(|act| {
                if let Activity::Rebuild { progress } = act {
                    progress.phase = RebuildPhase::Enriching;
                    progress.current_system = String::new();
                    progress.systems_done = 0;
                    progress.systems_total = with_games.len();
                    progress.elapsed_secs = start.elapsed().as_secs();
                }
            });

            if !with_games.is_empty() {
                tracing::info!(
                    "Rebuild enrichment: updating {} system(s)",
                    with_games.len()
                );
                let enrich_start = std::time::Instant::now();
                for (i, sys) in with_games.iter().enumerate() {
                    state.update_activity(|act| {
                        if let Activity::Rebuild { progress } = act {
                            progress.current_system = sys.display_name.clone();
                            progress.systems_done = i;
                            progress.elapsed_secs = start.elapsed().as_secs();
                        }
                    });
                    state
                        .cache
                        .enrich_system_cache(&state, sys.folder_name.clone())
                        .await;
                }
                tracing::info!(
                    "Rebuild enrichment: done in {:.1}s",
                    enrich_start.elapsed().as_secs_f64()
                );
            }

            // Mark rebuild complete (terminal state).
            state.update_activity(|act| {
                if let Activity::Rebuild { progress } = act {
                    progress.phase = RebuildPhase::Complete;
                    progress.current_system = String::new();
                    progress.systems_done = with_games.len();
                    progress.systems_total = with_games.len();
                    progress.elapsed_secs = start.elapsed().as_secs();
                    progress.error = None;
                }
            });

            // guard drops here → Idle
            drop(guard);
        });
    }

    /// Spawn a background task that watches `replay.cfg` for changes and
    /// periodically re-checks storage as a fallback.
    ///
    /// Uses `notify` (inotify on Linux) to react immediately when the config
    /// file is modified. Falls back to the 60-second poll if filesystem
    /// watching cannot be set up (e.g., on NFS).
    pub fn spawn_storage_watcher(self) {
        let config_path = self.config_file_path();
        let state = self.clone();

        // Spawn the filesystem watcher in a blocking thread (notify uses
        // its own event loop that blocks the thread).
        let watcher_state = self.clone();
        let watcher_config_path = config_path.clone();

        tokio::spawn(async move {
            let watcher_active =
                Self::try_start_config_watcher(watcher_state, watcher_config_path).await;

            if watcher_active {
                tracing::info!("Config file watcher active; 60s poll runs as fallback");
            } else {
                tracing::info!("Config file watcher unavailable; using 60s poll only");
            }

            // Poll loop: 10s when waiting for storage, 60s once connected.
            loop {
                let delay = if state.has_storage() {
                    STORAGE_CHECK_INTERVAL
                } else {
                    10
                };
                tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                match state.refresh_storage().await {
                    Ok(true) => tracing::info!("Background storage re-detection: storage changed"),
                    Ok(false) => {}
                    Err(e) => tracing::warn!("Background storage re-detection failed: {e}"),
                }
            }
        });
    }

    /// Try to set up a `notify` filesystem watcher on the config file.
    /// Returns `true` if the watcher was started successfully.
    async fn try_start_config_watcher(state: AppState, config_path: std::path::PathBuf) -> bool {
        use notify::{RecursiveMode, Watcher, recommended_watcher};

        // Watch the parent directory -- the file itself may not exist yet, and
        // some editors write to a temp file then rename, which only shows up as
        // an event on the directory.
        let watch_dir = match config_path.parent() {
            Some(dir) if dir.exists() => dir.to_path_buf(),
            Some(dir) => {
                tracing::warn!(
                    "Config directory does not exist ({}), cannot set up file watcher",
                    dir.display()
                );
                return false;
            }
            None => {
                tracing::warn!("Cannot determine parent directory of config path");
                return false;
            }
        };

        let config_filename = config_path
            .file_name()
            .map(|n| n.to_os_string())
            .unwrap_or_default();

        let (tx, mut rx) = tokio::sync::mpsc::channel(16);

        // Create the watcher. The callback sends events through the channel
        // so we can process them on the async side.
        let mut watcher =
            match recommended_watcher(move |res: Result<notify::Event, notify::Error>| match res {
                Ok(event) => {
                    let _ = tx.blocking_send(event);
                }
                Err(e) => {
                    tracing::warn!("File watcher error: {e}");
                }
            }) {
                Ok(w) => w,
                Err(e) => {
                    tracing::warn!("Failed to create file watcher: {e}");
                    return false;
                }
            };

        if let Err(e) = watcher.watch(&watch_dir, RecursiveMode::NonRecursive) {
            tracing::warn!("Failed to watch directory {}: {e}", watch_dir.display());
            return false;
        }

        tracing::info!("Watching {} for config changes", watch_dir.display());

        // Spawn the event-processing loop. We keep `watcher` alive by moving
        // it into this task -- dropping it would stop watching.
        tokio::spawn(async move {
            let _watcher = watcher; // prevent drop

            // Debounce: after the first relevant event, wait before refreshing
            // so that rapid successive writes (common with text editors) only
            // trigger a single refresh.
            const DEBOUNCE: Duration = Duration::from_secs(2);

            loop {
                // Wait for the next event.
                let Some(event) = rx.recv().await else {
                    tracing::warn!("Config file watcher channel closed");
                    break;
                };

                if !Self::is_config_event(&event, &config_filename) {
                    continue;
                }

                tracing::debug!("Config change detected ({:?}), debouncing...", event.kind);

                // Drain any further events that arrive within the debounce window.
                let deadline = tokio::time::Instant::now() + DEBOUNCE;
                loop {
                    match tokio::time::timeout_at(deadline, rx.recv()).await {
                        Ok(Some(ev)) => {
                            if Self::is_config_event(&ev, &config_filename) {
                                tracing::debug!(
                                    "Additional config event during debounce ({:?})",
                                    ev.kind
                                );
                            }
                        }
                        Ok(None) => {
                            // Channel closed
                            break;
                        }
                        Err(_) => {
                            // Timeout -- debounce window expired
                            break;
                        }
                    }
                }

                tracing::info!("Config file changed, refreshing storage");
                match state.refresh_storage().await {
                    Ok(true) => tracing::info!("Storage updated after config change"),
                    Ok(false) => tracing::debug!("Config changed but storage unchanged"),
                    Err(e) => tracing::warn!("Failed to refresh storage after config change: {e}"),
                }
            }
        });

        true
    }

    /// Check whether a notify event is relevant to our config file.
    fn is_config_event(event: &notify::Event, config_filename: &std::ffi::OsStr) -> bool {
        use notify::EventKind;

        // Only react to creates, modifications, and renames (some editors
        // write a temp file then rename it over the original).
        matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_))
            && event
                .paths
                .iter()
                .any(|p| p.file_name().is_some_and(|n| n == config_filename))
    }

    /// Spawn a filesystem watcher on the `roms/` directory for local storage.
    ///
    /// Only starts for local storage kinds (`Sd`, `Usb`, `Nvme`) where
    /// inotify works reliably. NFS is excluded because inotify does not
    /// detect changes made by other NFS clients. For NFS, users trigger
    /// rescans manually via the metadata page "Update" button.
    pub fn spawn_rom_watcher(&self) {
        let storage = self.storage();
        if !storage.kind.is_local() {
            tracing::debug!(
                "ROM watcher skipped for {:?} storage (inotify unreliable on NFS)",
                storage.kind
            );
            return;
        }

        let roms_dir = storage.roms_dir();
        if !roms_dir.exists() {
            tracing::debug!(
                "ROM watcher skipped: roms directory does not exist ({})",
                roms_dir.display()
            );
            return;
        }

        let state = self.clone();
        tokio::spawn(async move {
            let watcher_active = Self::try_start_rom_watcher(state, roms_dir).await;
            if watcher_active {
                tracing::info!("ROM directory watcher active");
            } else {
                tracing::warn!(
                    "ROM directory watcher could not be started; \
                     new ROMs will be detected on page visit or next restart"
                );
            }
        });
    }

    /// Try to set up a `notify` filesystem watcher on the `roms/` directory.
    /// Returns `true` if the watcher was started successfully.
    ///
    /// Watches recursively for create/modify/remove events. On change,
    /// extracts the affected system folder name from the event path and
    /// triggers `get_roms` + `enrich_system_cache` after a debounce window.
    ///
    /// When a top-level change is detected in the `roms/` directory itself
    /// (new system directory created), triggers a `get_systems` refresh.
    async fn try_start_rom_watcher(state: AppState, roms_dir: std::path::PathBuf) -> bool {
        use notify::{RecursiveMode, Watcher, recommended_watcher};

        let (tx, mut rx) = tokio::sync::mpsc::channel(64);

        let mut watcher =
            match recommended_watcher(move |res: Result<notify::Event, notify::Error>| match res {
                Ok(event) => {
                    let _ = tx.blocking_send(event);
                }
                Err(e) => {
                    tracing::warn!("ROM watcher error: {e}");
                }
            }) {
                Ok(w) => w,
                Err(e) => {
                    tracing::warn!("Failed to create ROM watcher: {e}");
                    return false;
                }
            };

        if let Err(e) = watcher.watch(&roms_dir, RecursiveMode::Recursive) {
            tracing::warn!("Failed to watch roms directory {}: {e}", roms_dir.display());
            return false;
        }

        tracing::info!("Watching {} for ROM changes", roms_dir.display());

        tokio::spawn(async move {
            let _watcher = watcher; // prevent drop

            // Debounce: batch rapid filesystem events (e.g., bulk copy) before
            // triggering a rescan. 3 seconds balances responsiveness vs thrashing.
            const DEBOUNCE: Duration = Duration::from_secs(3);

            loop {
                // Wait for the next event.
                let Some(event) = rx.recv().await else {
                    tracing::warn!("ROM watcher channel closed");
                    break;
                };

                if !Self::is_rom_event(&event) {
                    continue;
                }

                // Collect affected system folder names from this and subsequent
                // events within the debounce window.
                let mut affected_systems = std::collections::HashSet::new();
                let mut roms_dir_changed = false;
                Self::collect_rom_event_systems(
                    &event,
                    &roms_dir,
                    &mut affected_systems,
                    &mut roms_dir_changed,
                );

                tracing::debug!("ROM change detected ({:?}), debouncing...", event.kind);

                // Drain further events within the debounce window.
                let deadline = tokio::time::Instant::now() + DEBOUNCE;
                loop {
                    match tokio::time::timeout_at(deadline, rx.recv()).await {
                        Ok(Some(ev)) => {
                            if Self::is_rom_event(&ev) {
                                Self::collect_rom_event_systems(
                                    &ev,
                                    &roms_dir,
                                    &mut affected_systems,
                                    &mut roms_dir_changed,
                                );
                            }
                        }
                        Ok(None) => break, // Channel closed
                        Err(_) => break,   // Debounce window expired
                    }
                }

                // Skip if any activity is running (startup, import, etc.).
                if !state.is_idle() {
                    tracing::debug!(
                        "Background operation in progress, skipping ROM watcher rescan"
                    );
                    continue;
                }

                // Run the rescan as an async task.
                let storage = state.storage();
                let region_pref = state.region_preference();
                let region_secondary = state.region_preference_secondary();

                // Invalidate L1+L2 for each affected system so get_roms
                // does a fresh L3 filesystem scan.
                for system in &affected_systems {
                    state
                        .cache
                        .invalidate_system(system.clone(), &state.metadata_pool)
                        .await;
                    state.response_cache.invalidate_all();
                }

                // Re-scan each affected system.
                if !affected_systems.is_empty() {
                    tracing::info!(
                        "ROM watcher: re-scanning {} system(s): {}",
                        affected_systems.len(),
                        affected_systems
                            .iter()
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                    for system in &affected_systems {
                        let _ = state
                            .cache
                            .scan_and_cache_system(
                                &storage,
                                system,
                                region_pref,
                                region_secondary,
                                &state.metadata_pool,
                            )
                            .await;
                        state
                            .cache
                            .enrich_system_cache(&state, system.clone())
                            .await;
                    }
                }

                // If the roms/ directory itself changed (new subdirectory
                // created or removed), refresh the systems list to discover
                // new systems and update game counts.
                if roms_dir_changed {
                    tracing::info!("ROM watcher: roms/ directory changed, refreshing systems");
                    let systems = state
                        .cache
                        .cached_systems(&storage, &state.metadata_pool)
                        .await;
                    for sys in &systems {
                        if sys.game_count > 0 && !affected_systems.contains(&sys.folder_name) {
                            let _ = state
                                .cache
                                .scan_and_cache_system(
                                    &storage,
                                    &sys.folder_name,
                                    region_pref,
                                    region_secondary,
                                    &state.metadata_pool,
                                )
                                .await;
                            state
                                .cache
                                .enrich_system_cache(&state, sys.folder_name.clone())
                                .await;
                        }
                    }
                } else if !affected_systems.is_empty() {
                    let _ = state
                        .cache
                        .cached_systems(&storage, &state.metadata_pool)
                        .await;
                }
            }
        });

        true
    }

    /// Check whether a notify event is relevant to ROM files/directories.
    fn is_rom_event(event: &notify::Event) -> bool {
        use notify::EventKind;

        matches!(
            event.kind,
            EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
        )
    }

    /// Extract system folder names from event paths and detect top-level
    /// roms/ directory changes.
    fn collect_rom_event_systems(
        event: &notify::Event,
        roms_dir: &std::path::Path,
        affected_systems: &mut std::collections::HashSet<String>,
        roms_dir_changed: &mut bool,
    ) {
        for path in &event.paths {
            let relative = match path.strip_prefix(roms_dir) {
                Ok(rel) => rel,
                Err(_) => continue,
            };

            // Get the first path component (the system folder name).
            let mut components = relative.components();
            let Some(first) = components.next() else {
                // Event on roms/ directory itself.
                *roms_dir_changed = true;
                continue;
            };

            let system_name = first.as_os_str().to_string_lossy();

            // Skip internal directories (e.g., _favorites, _recent).
            if system_name.starts_with('_') {
                continue;
            }

            // If the event path has only one component (no further child),
            // it's a direct child of roms/ -- either a new system directory
            // was created or an entry was removed.
            if components.next().is_none() {
                *roms_dir_changed = true;
            }

            affected_systems.insert(system_name.into_owned());
        }
    }
}
