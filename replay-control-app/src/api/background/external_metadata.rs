//! External-metadata refresh: download + parse the host-global
//! `external_metadata.db` (LaunchBox XML), plus the first-run setup downloads.
//! Free functions over `AppState`, part of the `background` module.

use crate::api::AppState;
use crate::api::activity::{Activity, RefreshMetadataPhase, RefreshMetadataProgress};

/// Spawn a background task that re-runs `phase_auto_import`. Used by the
/// "Regenerate metadata" UI button and other on-demand triggers.
pub fn spawn_external_metadata_refresh(state: AppState) {
    tokio::spawn(async move {
        super::phase_auto_import(&state).await;
    });
}

/// Spawn a background task that downloads the LaunchBox `Metadata.zip`
/// into the host-global download directory, extracts the XML, then triggers
/// the standard refresh path against it.
///
/// Uses an HTTP ETag check to skip the 100+ MB download when the upstream
/// file hasn't changed since the last successful download.
pub fn spawn_external_metadata_download_and_refresh(state: AppState) {
    tokio::spawn(async move {
        let _ = download_external_metadata_and_refresh(&state).await;
    });
}

/// First-run setup wrapper: one UI action can fill both external metadata
/// sources. LaunchBox runs first; when it releases the activity slot, the
/// thumbnail manifest update starts from the same click.
pub fn spawn_setup_metadata_downloads(
    state: AppState,
    needs_launchbox: bool,
    needs_thumbnail_index: bool,
) {
    tokio::spawn(async move {
        if needs_launchbox && !download_external_metadata_and_refresh(&state).await {
            return;
        }

        if needs_thumbnail_index && !state.thumbnails.start_thumbnail_update(&state) {
            tracing::warn!("setup metadata: thumbnail update could not start; activity busy");
        }
    });
}

async fn download_external_metadata_and_refresh(state: &AppState) -> bool {
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
            return false;
        }
    };

    let start = std::time::Instant::now();
    let download_dir = state.data_dir.download_dir();

    let stored_etag = state
        .external_metadata_reader
        .read(|conn| external_metadata::read_meta(conn, meta_keys::LAUNCHBOX_UPSTREAM_ETAG))
        .await
        .flatten();

    // Single HEAD request — captures ETag (freshness check) and Content-Length
    // (passed to download_metadata to avoid a redundant second HEAD).
    let upstream_head =
        tokio::task::spawn_blocking(replay_control_core_server::launchbox::fetch_upstream_head)
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
        super::reenrich_all_systems(state).await;
        state.update_activity(|act| {
            if let Activity::RefreshExternalMetadata { progress } = act {
                progress.phase = RefreshMetadataPhase::Complete;
                progress.elapsed_secs = start.elapsed().as_secs();
            }
        });
        return true; // guard drops → Activity::Idle
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
                &download_dir,
                upstream_content_length,
                |bytes, total| {
                    let prev = last_reported.load(Ordering::Relaxed);
                    if bytes - prev < THROTTLE_BYTES && bytes != 0 {
                        return;
                    }
                    last_reported.store(bytes, Ordering::Relaxed);
                    state_for_progress.update_activity(|act| {
                        if let Activity::RefreshExternalMetadata { progress } = act {
                            progress.downloaded_bytes = bytes;
                            progress.total_bytes = total;
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
                match state
                    .external_metadata_writer
                    .try_write(move |conn| {
                        external_metadata::write_meta(
                            conn,
                            meta_keys::LAUNCHBOX_UPSTREAM_ETAG,
                            Some(&etag),
                        )
                    })
                    .await
                {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => {
                        tracing::warn!("LaunchBox upstream ETag SQL failed: {e}");
                    }
                    Err(e) => {
                        tracing::warn!("LaunchBox upstream ETag write failed: {e}");
                    }
                }
            }
            super::phase_auto_import_inner(state, Some(guard)).await;
            true
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
            false
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
            false
        }
    }
}
