use replay_control_core_server::DbPool;
use replay_control_core_server::db_pool::rusqlite;
use replay_control_core_server::library_db::LibraryDb;
use replay_control_core_server::update as update_io;
use std::time::Duration;

use super::AppState;
use super::activity::{
    Activity, RebuildPhase, RebuildProgress, RefreshMetadataPhase, RefreshMetadataProgress,
    StartupPhase,
};
use super::library::dir_mtime;

#[derive(Clone, Copy)]
pub(crate) enum PopulateProgress {
    /// Boot pipeline / post-import. Updates `Activity::Startup` if active and
    /// reuses the L2-cached systems list (`cached_systems`).
    Startup,
    /// Destructive rebuild path. Updates `Activity::Rebuild` and reuses
    /// `cached_systems` — by the time we get here `game_library_meta` is
    /// already empty (`cache.invalidate` truncated it), so cached_systems
    /// falls through to a fresh L3 filesystem scan automatically.
    Rebuild { start: std::time::Instant },
    /// Additive rescan. Updates `Activity::Rebuild` (with `is_rescan: true`).
    /// Forces an explicit `scan_systems` walk because `game_library_meta`
    /// still holds the old `rom_count = 0` rows for systems whose folders
    /// were empty before — cached_systems would silently filter them out.
    Rescan { start: std::time::Instant },
}

impl PopulateProgress {
    fn rebuild_start(&self) -> Option<std::time::Instant> {
        match self {
            Self::Startup => None,
            Self::Rebuild { start } | Self::Rescan { start } => Some(*start),
        }
    }
}

/// Update `Activity::Rebuild`'s progress in place. No-op if a different
/// activity variant is currently active (e.g. someone replaced the guard).
fn update_rebuild_progress(state: &AppState, f: impl FnOnce(&mut RebuildProgress)) {
    state.update_activity(|act| {
        if let Activity::Rebuild { progress } = act {
            f(progress);
        }
    });
}

/// Push per-system progress into whichever Activity variant the caller
/// owns. Startup carries a single label string; Rebuild/Rescan carry
/// counters + elapsed seconds.
fn report_system(
    state: &AppState,
    progress: PopulateProgress,
    i: usize,
    display_name: &str,
    enriching: bool,
) {
    match progress {
        PopulateProgress::Startup => state.update_activity(|act| {
            if let Activity::Startup { system, .. } = act {
                *system = if enriching {
                    format!("{display_name} (enriching)")
                } else {
                    display_name.to_string()
                };
            }
        }),
        PopulateProgress::Rebuild { start } | PopulateProgress::Rescan { start } => {
            update_rebuild_progress(state, |p| {
                p.current_system = display_name.to_string();
                p.systems_done = i;
                p.elapsed_secs = start.elapsed().as_secs();
            });
        }
    }
}

/// Read a value from the pool or skip the calling phase. Pool unavailability
/// is logged at `debug` ("transient — retry later"), inner SQL errors at
/// `warn`. Used by destructive cascade gates that must distinguish
/// "DB unavailable" from "DB has no rows" — defaulting unavailability to
/// "empty" is what triggered the
/// `2026-05-01-library-wal-unlink-under-live-connections` regression.
async fn try_read_or_skip<T, F>(pool: &DbPool, phase: &'static str, f: F) -> Option<T>
where
    F: FnOnce(&rusqlite::Connection) -> replay_control_core::error::Result<T> + Send + 'static,
    T: Send + 'static,
{
    match pool.try_read(f).await {
        Ok(Ok(value)) => Some(value),
        Ok(Err(e)) => {
            tracing::warn!("{phase}: SQL failed: {e}");
            None
        }
        Err(e) => {
            tracing::debug!("{phase}: pool unavailable ({e}); skipping");
            None
        }
    }
}

/// How often the background task re-checks storage (in seconds).
const STORAGE_CHECK_INTERVAL: u64 = 60;

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
        update_io::nuke_update_dir();

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

        // Phase 0: wait for the roms_dir to be readable AND non-empty before
        // attempting any scan. On NFS / autofs / USB hot-plug the storage
        // root may resolve before subdirectories surface; without this, the
        // first L3 scan returns "all systems empty" and persists zeros into
        // game_library_meta. Capped at 30s — the worst case (legitimately
        // empty library) just falls through to a no-op populate.
        // See `2026-04-29-nfs-startup-race-and-thumbnail-silent-failure.md`.
        let storage = state.storage();
        if let Err(e) = replay_control_core_server::roms::wait_for_storage_ready(
            &storage.roms_dir(),
            Duration::from_secs(30),
        )
        .await
        {
            tracing::warn!(
                "Startup: roms_dir readiness check timed out: {e}. Proceeding; \
                 subsequent scans will retry on demand."
            );
        }
        drop(storage);

        // Phase 0.5: On first boot, silently pre-fetch LaunchBox XML and the
        // libretro thumbnail manifest so Phase 2 enrichment has data without
        // requiring a manual "Refresh metadata" before the first scan.
        Self::phase_first_run_seed(state).await;

        // Phase 1: Auto-import (if launchbox XML exists + DB empty).
        // Import claims/releases its own Activity::Import via try_start_activity.
        Self::phase_auto_import(state).await;

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
            state.library_pool.checkpoint().await;

            Self::phase_auto_rebuild_thumbnail_index(state).await;

            // Pre-warm the metadata-page snapshot so the very first user
            // request after boot gets a hot cache instead of paying the
            // ~250 ms compute on demand. Invalidations later in the run
            // (post-import / post-thumbnail-update) drop it again so the
            // next reload picks up fresh state.
            let _ = state.cache.metadata_page_snapshot(state).await;

            // _guard drops → Idle
        }
    }

    /// Spawn a background task that re-runs `phase_auto_import`. Used by the
    /// "Regenerate metadata" UI button and other on-demand triggers.
    pub fn spawn_external_metadata_refresh(state: AppState) {
        tokio::spawn(async move {
            Self::phase_auto_import(&state).await;
        });
    }

    /// Spawn a background task that downloads the LaunchBox `Metadata.zip`
    /// into the host-global cache directory, extracts the XML, then triggers
    /// the standard refresh path against it.
    ///
    /// Uses an HTTP ETag check to skip the 100+ MB download when the upstream
    /// file hasn't changed since the last successful download.
    pub fn spawn_external_metadata_download_and_refresh(state: AppState) {
        tokio::spawn(async move {
            use replay_control_core_server::external_metadata::{self, meta_keys};

            // Claim the slot. Start at Checking so the banner shows while we
            // do the HEAD request before committing to a full download.
            let guard = match state.try_start_activity(Activity::RefreshExternalMetadata {
                progress: RefreshMetadataProgress {
                    phase: RefreshMetadataPhase::Checking,
                    ..RefreshMetadataProgress::initial()
                },
            }) {
                Ok(g) => g,
                Err(e) => {
                    tracing::warn!("download+refresh: activity busy: {e}");
                    return;
                }
            };

            let start = std::time::Instant::now();
            let cache_dir = state.data_dir.cache_dir();

            let stored_etag = state
                .external_metadata_pool
                .read(|conn| external_metadata::read_meta(conn, meta_keys::LAUNCHBOX_UPSTREAM_ETAG))
                .await
                .flatten();

            // Single HEAD request — captures ETag (freshness check) and Content-Length
            // (passed to download_metadata to avoid a redundant second HEAD).
            let upstream_head = tokio::task::spawn_blocking(
                replay_control_core_server::launchbox::fetch_upstream_head,
            )
            .await
            .unwrap_or(replay_control_core_server::launchbox::HeadHeaders {
                content_length: None,
                etag: None,
            });

            if stored_etag.is_some() && stored_etag == upstream_head.etag {
                tracing::info!(
                    "LaunchBox ETag matches ({}) — skipping download, re-enriching",
                    upstream_head.etag.as_deref().unwrap_or("")
                );
                // Skip the download and XML re-parse, but still enrich so any
                // ROMs added since the last refresh pick up their metadata.
                state.update_activity(|act| {
                    if let Activity::RefreshExternalMetadata { progress } = act {
                        progress.phase = RefreshMetadataPhase::Enriching;
                    }
                });
                Self::reenrich_all_systems(&state).await;
                state.update_activity(|act| {
                    if let Activity::RefreshExternalMetadata { progress } = act {
                        progress.phase = RefreshMetadataPhase::Complete;
                        progress.elapsed_secs = start.elapsed().as_secs();
                    }
                });
                return; // guard drops → Activity::Idle
            }

            // ETags differ (or unavailable) — proceed with the full download.
            state.update_activity(|act| {
                if let Activity::RefreshExternalMetadata { progress } = act {
                    progress.phase = RefreshMetadataPhase::Downloading;
                }
            });

            let upstream_etag = upstream_head.etag;
            let upstream_content_length = upstream_head.content_length;
            let download_result = {
                let state_for_progress = state.clone();
                tokio::task::spawn_blocking(move || {
                    // Throttle: each curl read is ~64 KB; updating activity per
                    // chunk is 3000+ RwLock+broadcast cycles per 200 MB
                    // download. Only fire when we cross a 1 MiB boundary.
                    // `download_metadata` takes `Fn`, so we need interior
                    // mutability for the watermark.
                    use std::sync::atomic::{AtomicU64, Ordering};
                    const THROTTLE_BYTES: u64 = 1024 * 1024;
                    let last_reported = AtomicU64::new(0);
                    replay_control_core_server::launchbox::download_metadata(
                        &cache_dir,
                        upstream_content_length,
                        |bytes, _total| {
                            let prev = last_reported.load(Ordering::Relaxed);
                            if bytes - prev < THROTTLE_BYTES && bytes != 0 {
                                return;
                            }
                            last_reported.store(bytes, Ordering::Relaxed);
                            state_for_progress.update_activity(|act| {
                                if let Activity::RefreshExternalMetadata { progress } = act {
                                    progress.downloaded_bytes = bytes;
                                }
                            });
                        },
                    )
                })
                .await
            };

            match download_result {
                Ok(Ok(xml_path)) => {
                    tracing::info!("LaunchBox metadata downloaded to {}", xml_path.display());
                    // Store the upstream ETag so the next "Refresh metadata" can
                    // detect an unchanged file without re-downloading.
                    if let Some(etag) = upstream_etag {
                        let _ = state
                            .external_metadata_pool
                            .write(move |conn| {
                                external_metadata::write_meta(
                                    conn,
                                    meta_keys::LAUNCHBOX_UPSTREAM_ETAG,
                                    Some(&etag),
                                )
                            })
                            .await;
                    }
                    Self::phase_auto_import_inner(&state, Some(guard)).await;
                }
                Ok(Err(e)) => {
                    tracing::warn!("LaunchBox download failed: {e}");
                    state.update_activity(|act| {
                        if let Activity::RefreshExternalMetadata { progress } = act {
                            progress.phase = RefreshMetadataPhase::Failed;
                            progress.error = Some(e.to_string());
                            progress.elapsed_secs = start.elapsed().as_secs();
                        }
                    });
                }
                Err(e) => {
                    tracing::warn!("LaunchBox download task panicked: {e}");
                    state.update_activity(|act| {
                        if let Activity::RefreshExternalMetadata { progress } = act {
                            progress.phase = RefreshMetadataPhase::Failed;
                            progress.error = Some(format!("task panicked: {e}"));
                            progress.elapsed_secs = start.elapsed().as_secs();
                        }
                    });
                }
            }
        });
    }

    /// Phase 0.5: On first boot, silently download the LaunchBox XML and the
    /// libretro thumbnail manifest so Phase 2 enrichment runs with data.
    ///
    /// First-run conditions (checked independently):
    ///   - LaunchBox: no `launchbox_xml_crc32` in `external_meta` AND no XML on disk.
    ///   - Libretro: `data_source` has no rows.
    ///
    /// Any network failure is warn-logged and the pipeline continues normally.
    /// Phase 1 will detect and parse the downloaded XML via its usual hash check.
    async fn phase_first_run_seed(state: &AppState) {
        use replay_control_core_server::external_metadata::{self, meta_keys};
        use replay_control_core_server::library_db::resolve_launchbox_xml;

        let storage = state.storage();
        let rc_dir = storage.rc_dir();
        let cache_dir = state.data_dir.cache_dir();
        drop(storage);

        let seed_check = state
            .external_metadata_pool
            .read(|conn| {
                let has_crc32 =
                    external_metadata::read_meta(conn, meta_keys::LAUNCHBOX_XML_CRC32).is_some();
                let has_sources =
                    external_metadata::get_data_source_stats(conn, "libretro-thumbnails")
                        .ok()
                        .map(|s| s.repo_count > 0)
                        .unwrap_or(false);
                (has_crc32, has_sources)
            })
            .await;

        let (has_crc32, has_libretro_sources) = match seed_check {
            Some(v) => v,
            None => {
                tracing::debug!("phase_first_run_seed: pool unavailable, skipping");
                return;
            }
        };

        let xml_on_disk = resolve_launchbox_xml(&cache_dir, &rc_dir).is_some();
        let needs_launchbox = !has_crc32 && !xml_on_disk;
        let needs_libretro = !has_libretro_sources;

        if !needs_launchbox && !needs_libretro {
            tracing::debug!("phase_first_run_seed: not a first-run install, skipping");
            return;
        }

        tracing::info!(
            "phase_first_run_seed: first-run detected \
             (launchbox={needs_launchbox}, libretro={needs_libretro})"
        );

        let _guard = match state.try_start_activity(Activity::Startup {
            phase: StartupPhase::FetchingMetadata,
            system: String::new(),
        }) {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!("phase_first_run_seed: activity busy: {e}");
                return;
            }
        };

        if needs_launchbox {
            let dest = cache_dir.clone();
            let result = tokio::task::spawn_blocking(move || {
                replay_control_core_server::launchbox::download_metadata(&dest, None, |_, _| {})
            })
            .await;
            match result {
                Ok(Ok(p)) => tracing::info!(
                    "phase_first_run_seed: LaunchBox XML downloaded to {}",
                    p.display()
                ),
                Ok(Err(e)) => {
                    tracing::warn!("phase_first_run_seed: LaunchBox download failed: {e}")
                }
                Err(e) => tracing::warn!("phase_first_run_seed: LaunchBox task panicked: {e}"),
            }
        }

        if needs_libretro {
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let api_key =
                replay_control_core_server::settings::read_github_api_key(&state.settings);
            match replay_control_core_server::thumbnail_manifest::import_all_manifests(
                &state.external_metadata_pool,
                &|_, _, _| {},
                &cancel,
                api_key.as_deref(),
            )
            .await
            {
                Ok(stats) => tracing::info!(
                    "phase_first_run_seed: libretro manifest fetched \
                     ({} repos, {} entries{})",
                    stats.repos_fetched,
                    stats.total_entries,
                    if stats.rate_limited {
                        ", rate-limited"
                    } else {
                        ""
                    }
                ),
                Err(e) => tracing::warn!("phase_first_run_seed: libretro manifest failed: {e}"),
            }
        }

        // _guard drops → Activity::Idle
    }

    /// Phase 1: Refresh `external_metadata.db` from the LaunchBox XML when its
    /// content has changed (or the DB has never been populated).
    ///
    /// Freshness is content-derived: hash the XML, compare against the stored
    /// `external_meta.launchbox_xml_crc32`. mtime is unreliable across copies /
    /// rsync / clock skew. Skips entirely when no XML is present — users can
    /// still get scan-time + catalog enrichment.
    async fn phase_auto_import(state: &AppState) {
        Self::phase_auto_import_inner(state, None).await;
    }

    /// Inner entry point with optional caller-owned activity guard. Used by
    /// `spawn_external_metadata_download_and_refresh` to thread its
    /// `Downloading`-phase guard into the parse step without releasing it
    /// (avoiding an Idle flicker on the SSE stream).
    async fn phase_auto_import_inner(
        state: &AppState,
        existing_guard: Option<crate::api::ActivityGuard>,
    ) {
        use replay_control_core_server::external_metadata::{self, meta_keys};
        use replay_control_core_server::library_db::resolve_launchbox_xml;

        let storage = state.storage();
        let rc_dir = storage.rc_dir();
        let cache_dir = state.data_dir.cache_dir();

        let Some(xml_path) = resolve_launchbox_xml(&cache_dir, &rc_dir) else {
            tracing::debug!(
                "phase_auto_import: no LaunchBox XML in {} or {} — skipping",
                cache_dir.display(),
                rc_dir.display()
            );
            return;
        };

        // Claim the activity slot first (or reuse the caller's guard) so the
        // hash check itself is single-flight — two concurrent boots can't
        // both pass the hash mismatch and then race on the write.
        let guard = match existing_guard {
            Some(g) => g,
            None => match state.try_start_activity(Activity::RefreshExternalMetadata {
                progress: RefreshMetadataProgress::initial(),
            }) {
                Ok(g) => g,
                Err(e) => {
                    tracing::info!("phase_auto_import: another refresh in flight: {e}");
                    return;
                }
            },
        };

        let start = std::time::Instant::now();

        // Hash + stamp-read are independent — let the slowest dictate the
        // wall-clock instead of running them back-to-back.
        let xml_for_hash = xml_path.clone();
        let hash_fut =
            tokio::task::spawn_blocking(move || external_metadata::hash_file_crc32(&xml_for_hash));
        let stamp_fut = state
            .external_metadata_pool
            .read(|conn| external_metadata::read_meta(conn, meta_keys::LAUNCHBOX_XML_CRC32));
        let (hash_join, stored_hash) = tokio::join!(hash_fut, stamp_fut);
        let stored_hash = stored_hash.flatten();

        let current_hash = match hash_join {
            Ok(Ok(h)) => h,
            Ok(Err(e)) => {
                tracing::warn!(
                    "phase_auto_import: hash failed for {}: {e}",
                    xml_path.display()
                );
                return;
            }
            Err(e) => {
                tracing::warn!("phase_auto_import: hash task panicked: {e}");
                return;
            }
        };

        if stored_hash.as_deref() == Some(current_hash.as_str()) {
            tracing::debug!(
                "phase_auto_import: LaunchBox XML hash matches stamp ({current_hash}) — skipping refresh"
            );
            return;
        }

        tracing::info!(
            "phase_auto_import: refreshing external_metadata.db from {} (hash {current_hash})",
            xml_path.display()
        );

        state.update_activity(|act| {
            if let Activity::RefreshExternalMetadata { progress } = act {
                progress.phase = RefreshMetadataPhase::Parsing;
            }
        });

        // Surface parse progress to SSE so the UI banner doesn't sit frozen
        // for the 30–90 s parse on Pi. The closure runs on the blocking pool
        // (deadpool's interact thread); update_activity is RwLock-only +
        // broadcast, no async work.
        let xml_for_task = xml_path.clone();
        let progress_state = state.clone();
        let result = state
            .external_metadata_pool
            .write(move |conn| {
                replay_control_core_server::library::external_metadata_refresh::refresh_launchbox(
                    &xml_for_task,
                    conn,
                    move |processed| {
                        progress_state.update_activity(|act| {
                            if let Activity::RefreshExternalMetadata { progress } = act {
                                progress.source_entries = processed;
                            }
                        });
                    },
                )
            })
            .await;

        let stats = match result {
            Some(Ok(stats)) => stats,
            Some(Err(e)) => {
                tracing::warn!("phase_auto_import: refresh failed: {e}");
                state.update_activity(|act| {
                    if let Activity::RefreshExternalMetadata { progress } = act {
                        progress.phase = RefreshMetadataPhase::Failed;
                        progress.error = Some(e.to_string());
                        progress.elapsed_secs = start.elapsed().as_secs();
                    }
                });
                return;
            }
            None => {
                tracing::warn!("phase_auto_import: external_metadata pool unavailable");
                state.update_activity(|act| {
                    if let Activity::RefreshExternalMetadata { progress } = act {
                        progress.phase = RefreshMetadataPhase::Failed;
                        progress.error = Some("external_metadata pool unavailable".into());
                        progress.elapsed_secs = start.elapsed().as_secs();
                    }
                });
                return;
            }
        };

        tracing::info!(
            "phase_auto_import: refresh complete — {} games, {} alternates from {} source entries",
            stats.games_written,
            stats.alternates_written,
            stats.source_entries
        );

        // Re-enrichment: launchbox data just changed, so flush it through
        // game_library + game_description for every system the user has.
        // Without this, the request path keeps showing pre-refresh data
        // until something else triggers enrichment (storage swap, rebuild).
        state.update_activity(|act| {
            if let Activity::RefreshExternalMetadata { progress } = act {
                progress.phase = RefreshMetadataPhase::Enriching;
                progress.source_entries = stats.source_entries;
            }
        });
        Self::reenrich_all_systems(state).await;

        state.update_activity(|act| {
            if let Activity::RefreshExternalMetadata { progress } = act {
                progress.phase = RefreshMetadataPhase::Complete;
                progress.source_entries = stats.source_entries;
                progress.elapsed_secs = start.elapsed().as_secs();
            }
        });
        // `guard` drops at end of scope → ActivityGuard::Drop broadcasts Idle.
        drop(guard);
    }

    /// After an external_metadata refresh, iterate every system in the
    /// library cache and re-run enrichment so the new launchbox data
    /// flows into `game_library` + `game_description`. Does nothing on a
    /// fresh / empty library.
    async fn reenrich_all_systems(state: &AppState) {
        let storage = state.storage();
        let systems = state
            .cache
            .cached_systems(&storage, &state.library_pool)
            .await;
        let active: Vec<String> = systems
            .into_iter()
            .filter(|s| s.game_count > 0)
            .map(|s| s.folder_name)
            .collect();
        if active.is_empty() {
            return;
        }
        tracing::info!(
            "post-refresh re-enrichment starting for {} system(s)",
            active.len()
        );
        for system in active {
            state.cache.enrich_system_cache(state, system).await;
        }
        // Drop the metadata-page snapshot — coverage just changed.
        state.cache.invalidate_metadata_page().await;
        state.invalidate_user_caches().await;
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

        let Some(cached_meta) = try_read_or_skip(
            &state.library_pool,
            "cache_verification",
            LibraryDb::load_all_system_meta,
        )
        .await
        else {
            return;
        };

        if cached_meta.is_empty() {
            // Fresh DB — full populate.
            Self::populate_all_systems(
                state,
                &storage,
                region_pref,
                region_secondary,
                PopulateProgress::Startup,
            )
            .await;
            return;
        }

        // Query actual game_library row counts per system to detect interrupted scans.
        let actual_counts: std::collections::HashMap<String, usize> = state
            .library_pool
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
                        &state.library_pool,
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
        use replay_control_core_server::external_metadata;

        // Check data_source for libretro-thumbnails entries and thumbnail_manifest emptiness.
        let (has_sources, index_empty) = match state
            .external_metadata_pool
            .read(|conn| {
                let stats =
                    external_metadata::get_data_source_stats(conn, "libretro-thumbnails").ok()?;
                let index_count: i64 =
                    external_metadata::thumbnail_manifest_count(conn).unwrap_or(0);
                Some((stats.repo_count > 0, index_count == 0))
            })
            .await
            .flatten()
        {
            Some(result) => result,
            None => return, // pool unavailable
        };

        if !has_sources {
            // No data_sources entries. Check if images exist on disk — if so,
            // someone previously downloaded thumbnails but the DB was deleted.
            let has_images_on_disk = replay_control_core_server::thumbnails::any_images_on_disk(
                &state.storage().rc_dir(),
            );
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

        state.update_activity(|act| {
            if let Activity::Startup { phase, .. } = act {
                *phase = StartupPhase::RebuildingIndex;
            }
        });

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
                replay_control_core_server::thumbnails::thumbnail_repo_names(&system_str)
            else {
                continue;
            };

            let all_entries =
                replay_control_core_server::thumbnails::scan_system_images(&system_entry.path());

            if all_entries.is_empty() {
                continue;
            }

            system_data.push(SystemImageData {
                system_str,
                repo_names,
                entries: all_entries,
            });
        }

        // Now write all collected data to external_metadata.db in one txn.
        let write_result = state
            .external_metadata_pool
            .write(move |db| {
                let tx = match db.transaction() {
                    Ok(t) => t,
                    Err(e) => {
                        tracing::warn!("phase_auto_rebuild_thumbnail_index: begin failed: {e}");
                        return (0usize, 0usize);
                    }
                };
                let mut w_total_entries = 0usize;
                let mut w_total_repos = 0usize;

                for data in &system_data {
                    let repo_display = data.repo_names[0];
                    let source_name =
                        replay_control_core_server::thumbnails::libretro_source_name(repo_display);
                    let branch = replay_control_core_server::thumbnail_manifest::default_branch(
                        repo_display,
                    );
                    let entry_count = data.entries.len();

                    if let Err(e) = external_metadata::upsert_data_source(
                        &tx,
                        &source_name,
                        "libretro-thumbnails",
                        "disk-rebuild",
                        branch,
                        entry_count,
                    ) {
                        tracing::warn!("Failed to upsert data source {source_name}: {e}");
                    }

                    let _ = external_metadata::delete_thumbnail_manifest(&tx, &source_name);
                    match external_metadata::insert_thumbnail_manifest_rows(
                        &tx,
                        &source_name,
                        &data.entries,
                    ) {
                        Ok(_) => w_total_entries += entry_count,
                        Err(e) => tracing::warn!(
                            "Failed to insert disk-based index for {}: {e}",
                            data.system_str
                        ),
                    }

                    // Register additional repos for multi-repo systems (e.g., arcade_dc → Naomi + Naomi 2).
                    for extra_repo in &data.repo_names[1..] {
                        let extra_source =
                            replay_control_core_server::thumbnails::libretro_source_name(
                                extra_repo,
                            );
                        let extra_branch =
                            replay_control_core_server::thumbnail_manifest::default_branch(
                                extra_repo,
                            );
                        if let Err(e) = external_metadata::upsert_data_source(
                            &tx,
                            &extra_source,
                            "libretro-thumbnails",
                            "disk-rebuild",
                            extra_branch,
                            0,
                        ) {
                            tracing::warn!("Failed to upsert data source {extra_source}: {e}");
                        }
                    }
                    w_total_repos += data.repo_names.len();
                }

                if let Err(e) = tx.commit() {
                    tracing::warn!("phase_auto_rebuild_thumbnail_index: commit failed: {e}");
                }
                (w_total_entries, w_total_repos)
            })
            .await;

        let Some((total_entries, total_repos)) = write_result else {
            return; // DB unavailable
        };

        if total_entries > 0 {
            // Checkpoint WAL after the bulk thumbnail index writes.
            state.library_pool.checkpoint().await;
            tracing::info!(
                "Thumbnail index rebuilt from disk: {total_entries} entries across {total_repos} repos"
            );
        }
    }

    /// Pre-populate L2 cache for all systems that have games. Walks ROM
    /// directories, hashes new files, and enriches box art / ratings.
    pub(crate) async fn populate_all_systems(
        state: &AppState,
        storage: &replay_control_core_server::storage::StorageLocation,
        region_pref: replay_control_core::rom_tags::RegionPreference,
        region_secondary: Option<replay_control_core::rom_tags::RegionPreference>,
        progress: PopulateProgress,
    ) {
        let systems = match progress {
            PopulateProgress::Startup | PopulateProgress::Rebuild { .. } => {
                state
                    .cache
                    .cached_systems(storage, &state.library_pool)
                    .await
            }
            PopulateProgress::Rescan { .. } => {
                match replay_control_core_server::roms::scan_systems(storage).await {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!("populate_all_systems: scan_systems failed: {e}");
                        return;
                    }
                }
            }
        };
        let with_games: Vec<_> = systems.iter().filter(|s| s.game_count > 0).collect();
        let total = with_games.len();
        tracing::info!("L2 warmup: populating {} system(s) with games", total);

        let start = std::time::Instant::now();

        if let Some(rb_start) = progress.rebuild_start() {
            update_rebuild_progress(state, |p| {
                p.phase = RebuildPhase::Scanning;
                p.current_system = String::new();
                p.systems_done = 0;
                p.systems_total = total;
                p.elapsed_secs = rb_start.elapsed().as_secs();
            });
        }

        let mut total_roms = 0usize;
        for (i, sys) in with_games.iter().enumerate() {
            report_system(state, progress, i, &sys.display_name, false);
            match state
                .cache
                .scan_and_cache_system(
                    storage,
                    &sys.folder_name,
                    region_pref,
                    region_secondary,
                    &state.library_pool,
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
            total,
            start.elapsed().as_secs_f64()
        );

        if let Some(rb_start) = progress.rebuild_start() {
            update_rebuild_progress(state, |p| {
                p.phase = RebuildPhase::Enriching;
                p.current_system = String::new();
                p.systems_done = 0;
                p.systems_total = total;
                p.elapsed_secs = rb_start.elapsed().as_secs();
            });
        }

        for (i, sys) in with_games.iter().enumerate() {
            report_system(state, progress, i, &sys.display_name, true);
            state
                .cache
                .enrich_system_cache(state, sys.folder_name.clone())
                .await;
        }

        tracing::info!(
            "L2 warmup: done -- {} ROMs across {} systems in {:.1}s",
            total_roms,
            total,
            start.elapsed().as_secs_f64()
        );

        state.cache.invalidate_metadata_page().await;
    }
    // ── Update system ─────────────────────────────────────────────────

    /// GitHub repository for release checks and downloads.
    const REPO: &'static str = "lapastillaroja/replay-control";
    /// Maximum time for the entire StartUpdate operation (5 minutes).
    const UPDATE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

    /// Periodically checks GitHub for new releases.
    async fn update_check_loop(state: AppState) {
        // Delay first check to let WiFi come up on Pi.
        tokio::time::sleep(Duration::from_secs(60)).await;

        let analytics = super::analytics::AnalyticsClient::new(
            replay_control_core_server::http::shared_client().clone(),
            super::analytics::ENDPOINT,
        );

        loop {
            if state.has_storage() {
                match Self::perform_update_check_background(&state).await {
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

            tokio::time::sleep(Duration::from_secs(24 * 60 * 60)).await;
        }
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
            Self::REPO,
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
                    .config_tx
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
            Self::REPO,
            &channel,
            skipped.as_deref(),
            github_key.as_deref(),
        )
        .await?
        {
            Some(available) => {
                update_io::write_available_update(&available).ok();
                let _ = state.config_tx.send(super::ConfigEvent::UpdateAvailable {
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

        match tokio::time::timeout(Self::UPDATE_TIMEOUT, Self::start_update_inner(state, tag)).await
        {
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
        let assets =
            update_io::resolve_asset_urls(&base_url, Self::REPO, tag, github_key.as_deref())
                .await?;

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
        Self::extract_tarball(&binary_archive, &binary_dir).await?;
        Self::extract_tarball(&site_archive, &site_dir).await?;

        let mut catalog_path: Option<std::path::PathBuf> = None;
        if assets.catalog_url.is_some() {
            let catalog_dir = update_dir.join("catalog");
            tokio::fs::create_dir_all(&catalog_dir).await?;
            Self::extract_tarball(&catalog_archive, &catalog_dir).await?;
            catalog_path = Some(
                Self::find_extracted_file(&catalog_dir, "catalog.sqlite")
                    .await
                    .ok_or("Extracted catalog archive does not contain catalog.sqlite")?,
            );
        }

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
        let script = Self::generate_update_script(
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

            let Some(is_empty) = try_read_or_skip(&state.library_pool, "post_import", |conn| {
                LibraryDb::load_all_system_meta(conn).map(|m| m.is_empty())
            })
            .await
            else {
                return;
            };

            if is_empty {
                tracing::info!("Post-import: game library is empty, running full populate");
                // Per-write gating happens inside `pool.write()` — SSR readers
                // stay responsive between the populate's individual writes.
                BackgroundManager::populate_all_systems(
                    &state,
                    &storage,
                    region_pref,
                    region_secondary,
                    PopulateProgress::Startup,
                )
                .await;
                state.library_pool.checkpoint().await;
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
                .cached_systems(&storage, &state.library_pool)
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

            // Coverage / image_stats / library_summary all changed.
            state.cache.invalidate_metadata_page().await;
        });
    }

    /// Run cache enrichment as part of a rebuild operation (with an ActivityGuard).
    /// Updates `Activity::Rebuild` progress as it goes. The guard drops → Idle on completion.
    pub fn spawn_rebuild_enrichment(&self, guard: super::activity::ActivityGuard) {
        let state = self.clone();
        let start = std::time::Instant::now();

        tokio::spawn(async move {
            let storage = state.storage();
            let region_pref = state.region_preference();
            let region_secondary = state.region_preference_secondary();

            let Some(is_empty) = try_read_or_skip(&state.library_pool, "rebuild", |conn| {
                LibraryDb::load_all_system_meta(conn).map(|m| m.is_empty())
            })
            .await
            else {
                return;
            };

            if is_empty {
                tracing::info!("Rebuild: game library is empty, running full populate");
                // Per-write gating happens inside `pool.write()` — SSR readers
                // stay responsive between the populate's individual writes.
                BackgroundManager::populate_all_systems(
                    &state,
                    &storage,
                    region_pref,
                    region_secondary,
                    PopulateProgress::Rebuild { start },
                )
                .await;
                state.library_pool.checkpoint().await;
            }

            // Enrichment phase: update box art URLs and ratings for all systems.
            // NOTE: enrichment writes are NOT gated because enrich_system_cache
            // reads from the DB and the write gate blocks ALL reads on the same pool.
            // Enrichment writes are small per-system UPDATEs, not bulk INSERTs.
            let systems = state
                .cache
                .cached_systems(&storage, &state.library_pool)
                .await;
            let with_games: Vec<_> = systems.into_iter().filter(|s| s.game_count > 0).collect();
            let total = with_games.len();

            update_rebuild_progress(&state, |p| {
                p.phase = RebuildPhase::Enriching;
                p.current_system = String::new();
                p.systems_done = 0;
                p.systems_total = total;
                p.elapsed_secs = start.elapsed().as_secs();
            });

            if !with_games.is_empty() {
                tracing::info!("Rebuild enrichment: updating {total} system(s)");
                let enrich_start = std::time::Instant::now();
                for (i, sys) in with_games.iter().enumerate() {
                    report_system(
                        &state,
                        PopulateProgress::Rebuild { start },
                        i,
                        &sys.display_name,
                        true,
                    );
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

            state.cache.invalidate_metadata_page().await;

            update_rebuild_progress(&state, |p| {
                p.phase = RebuildPhase::Complete;
                p.current_system = String::new();
                p.systems_done = total;
                p.systems_total = total;
                p.elapsed_secs = start.elapsed().as_secs();
                p.error = None;
            });

            drop(guard);
        });
    }

    /// Spawn an additive rescan: walk all ROM directories, insert any new ROMs
    /// into `game_library` (via `INSERT OR IGNORE`), and run enrichment.
    /// Existing rows are preserved.
    pub fn spawn_rescan(&self, guard: super::activity::ActivityGuard) {
        let state = self.clone();
        let start = std::time::Instant::now();

        tokio::spawn(async move {
            let storage = state.storage();
            let region_pref = state.region_preference();
            let region_secondary = state.region_preference_secondary();

            // Walking ROM directories on a slow NFS share can take seconds to
            // minutes before the per-system progress starts firing. Set a
            // status hint up front so the UI doesn't sit on an empty label.
            update_rebuild_progress(&state, |p| {
                p.phase = RebuildPhase::Scanning;
                p.current_system = "scanning ROM directories".to_string();
                p.elapsed_secs = start.elapsed().as_secs();
            });

            BackgroundManager::populate_all_systems(
                &state,
                &storage,
                region_pref,
                region_secondary,
                PopulateProgress::Rescan { start },
            )
            .await;
            state.library_pool.checkpoint().await;

            update_rebuild_progress(&state, |p| {
                p.phase = RebuildPhase::Complete;
                p.current_system = String::new();
                p.systems_done = p.systems_total;
                p.elapsed_secs = start.elapsed().as_secs();
                p.error = None;
            });

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
                let mut favorites_changed = false;
                let mut recents_changed = false;
                Self::collect_rom_event_systems(
                    &event,
                    &roms_dir,
                    &mut affected_systems,
                    &mut roms_dir_changed,
                    &mut favorites_changed,
                    &mut recents_changed,
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
                                    &mut favorites_changed,
                                    &mut recents_changed,
                                );
                            }
                        }
                        Ok(None) => break, // Channel closed
                        Err(_) => break,   // Debounce window expired
                    }
                }

                // Favorites/recents invalidation is cheap (just clears the
                // cache; the next request rebuilds). Do it regardless of
                // whether a background scan is in progress, and regardless of
                // whether any ROM-file systems were affected in this batch.
                if favorites_changed {
                    tracing::debug!("ROM watcher: _favorites/ changed, invalidating cache");
                    state.cache.invalidate_favorites().await;
                    state.invalidate_user_caches().await;
                }
                if recents_changed {
                    tracing::debug!("ROM watcher: _recent/ changed, invalidating cache");
                    state.cache.invalidate_recents().await;
                    state.cache.invalidate_recommendations().await;
                }

                // Skip the heavier system rescan if any activity is running
                // (startup, import, etc.).
                if !state.is_idle() {
                    if !affected_systems.is_empty() || roms_dir_changed {
                        tracing::debug!(
                            "Background operation in progress, skipping ROM watcher rescan"
                        );
                    }
                    continue;
                }

                // Run the rescan as an async task.
                let storage = state.storage();
                let region_pref = state.region_preference();
                let region_secondary = state.region_preference_secondary();

                // Invalidate L1+L2 for each affected system so get_roms
                // does a fresh L3 filesystem scan.
                for system in &affected_systems {
                    if let Err(e) = state
                        .cache
                        .invalidate_system(system.clone(), &state.library_pool)
                        .await
                    {
                        tracing::debug!("rom-watch invalidate_system({system}) skipped: {e}");
                    }
                    state.invalidate_user_caches().await;
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
                                &state.library_pool,
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
                        .cached_systems(&storage, &state.library_pool)
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
                                    &state.library_pool,
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
                        .cached_systems(&storage, &state.library_pool)
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

    /// Extract system folder names from event paths, detect top-level roms/
    /// directory changes, and flag favorites/recents changes.
    fn collect_rom_event_systems(
        event: &notify::Event,
        roms_dir: &std::path::Path,
        affected_systems: &mut std::collections::HashSet<String>,
        roms_dir_changed: &mut bool,
        favorites_changed: &mut bool,
        recents_changed: &mut bool,
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

            // Internal marker directories trigger targeted cache invalidation
            // so remote/OS-level edits (e.g. a .fav symlink written outside
            // the UI) propagate without a restart.
            match system_name.as_ref() {
                "_favorites" => {
                    *favorites_changed = true;
                    continue;
                }
                "_recent" => {
                    *recents_changed = true;
                    continue;
                }
                _ => {}
            }

            // Skip any other internal directory.
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // ── generate_update_script ──────────────────────────────────────

    #[test]
    fn script_includes_catalog_handling_when_path_some() {
        let script = BackgroundManager::generate_update_script(
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
        let script = BackgroundManager::generate_update_script(
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
        let script = BackgroundManager::generate_update_script(
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
        BackgroundManager::extract_tarball(&archive_path, &dest)
            .await
            .unwrap();

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
