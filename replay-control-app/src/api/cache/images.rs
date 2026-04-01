use replay_control_core::enrichment::{self, ImageIndex};
use replay_control_core::user_data_db::UserDataDb;

/// Build an image index for a system.
///
/// Orchestrates pool access (user_data + metadata) and delegates to
/// the core `enrichment::build_image_index` which does pure DB + filesystem work.
pub async fn build_image_index(
    state: &crate::api::AppState,
    system: &str,
) -> ImageIndex {
    // Load user box art overrides first (separate pool, no contention with metadata).
    let system_owned = system.to_string();
    let user_overrides = state
        .user_data_pool
        .read(move |conn| UserDataDb::get_system_overrides(conn, &system_owned).ok())
        .await
        .flatten()
        .unwrap_or_default();

    // Build the image index using the metadata pool connection.
    let sys = system.to_string();
    let storage_root = state.storage().root.clone();
    state
        .metadata_pool
        .read(move |conn| {
            enrichment::build_image_index(conn, &sys, &storage_root, user_overrides)
        })
        .await
        .unwrap_or_else(|| {
            // Pool unavailable — return an empty index.
            ImageIndex {
                dir_index: replay_control_core::image_matching::DirIndex {
                    exact: Default::default(),
                    exact_ci: Default::default(),
                    fuzzy: Default::default(),
                    version: Default::default(),
                },
                db_paths: Default::default(),
                manifest: None,
            }
        })
}

/// Queue a background download for a single thumbnail.
/// Deduplicates concurrent requests for the same image.
pub(super) fn queue_on_demand_download(
    state: &crate::api::AppState,
    system: &str,
    rom_filename: &str,
    m: &replay_control_core::thumbnail_manifest::ManifestMatch,
) {
    use replay_control_core::thumbnail_manifest::{download_thumbnail, save_thumbnail};
    use replay_control_core::thumbnails::ThumbnailKind;

    let download_key = format!("{system}/{}", m.filename);

    // Check and insert atomically to prevent duplicate downloads.
    {
        let mut pending = state.pending_downloads.write().expect("pending lock");
        if !pending.insert(download_key.clone()) {
            return; // Already queued.
        }
    }

    let m = m.clone();
    let storage_root = state.storage().root.clone();
    let system = system.to_string();
    let rom_filename = rom_filename.to_string();
    let pending = state.pending_downloads.clone();
    let metadata_pool = state.metadata_pool.clone();
    let response_cache = state.response_cache.clone();

    std::thread::spawn(move || {
        match download_thumbnail(&m, ThumbnailKind::Boxart.repo_dir()) {
            Ok(bytes) => {
                if let Err(e) = save_thumbnail(
                    &storage_root,
                    &system,
                    ThumbnailKind::Boxart,
                    &m.filename,
                    &bytes,
                ) {
                    tracing::debug!("On-demand save failed for {}: {e}", m.filename);
                } else {
                    // Update box_art_url in the DB so it's visible immediately.
                    let boxart_dir = ThumbnailKind::Boxart.media_dir();
                    let png_name = format!("{}.png", m.filename);
                    let url = replay_control_core::enrichment::format_box_art_url(
                        &system,
                        &format!("{boxart_dir}/{png_name}"),
                    );
                    let sys = system.clone();
                    let rom = rom_filename.clone();
                    let _ = tokio::runtime::Handle::current().block_on(
                        metadata_pool.write(move |conn| {
                            let _ = conn.execute(
                                "UPDATE game_library SET box_art_url = ?1 WHERE system = ?2 AND rom_filename = ?3",
                                [&url, &sys, &rom],
                            );
                        }),
                    );
                    // Clear response cache so next page load picks up the new art.
                    response_cache.invalidate_all();
                }
            }
            Err(e) => {
                tracing::debug!("On-demand download failed for {}: {e}", m.filename);
            }
        }

        // Remove from pending set.
        if let Ok(mut guard) = pending.write() {
            guard.remove(&download_key);
        }
    });
}
