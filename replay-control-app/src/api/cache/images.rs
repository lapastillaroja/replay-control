use std::collections::HashMap;

use replay_control_core::image_matching::DirIndex;
use replay_control_core::metadata_db::MetadataDb;
use replay_control_core::thumbnail_manifest::ManifestFuzzyIndex;
use replay_control_core::user_data_db::UserDataDb;

/// Per-system image directory index for batch box art resolution.
/// Wraps a core `DirIndex` for filesystem-based matching, plus app-specific
/// fields for DB path lookups and on-demand manifest downloads.
///
/// Built as a temporary value during enrichment — NOT cached across requests.
pub struct ImageIndex {
    /// Core directory index: exact, case-insensitive, fuzzy, version-stripped.
    pub dir_index: DirIndex,
    /// DB paths: rom_filename -> "boxart/{path}"
    pub db_paths: HashMap<String, String>,
    /// Manifest-backed fallback for images not yet downloaded.
    /// None if the thumbnail_index has no entries for this system.
    pub manifest: Option<ManifestFuzzyIndex>,
}

/// Build an image index for a system from scratch (no caching).
/// Called by the enrichment pipeline to resolve box art during background build.
pub async fn build_image_index(
    state: &crate::api::AppState,
    system: &str,
) -> ImageIndex {
    use replay_control_core::thumbnails::strip_version;

    let boxart_media = replay_control_core::thumbnails::ThumbnailKind::Boxart.media_dir();
    let media_base = state.storage().rc_dir().join("media").join(system);
    let boxart_dir = media_base.join(boxart_media);

    // Build the base index using the shared image matching module.
    // This indexes all valid .png files (skips stubs via is_valid_image).
    let mut dir_index =
        replay_control_core::image_matching::build_dir_index(&boxart_dir, boxart_media);

    // Second pass: resolve fake symlinks (small text files pointing to real images).
    // These are skipped by build_dir_index since they're < 200 bytes.
    let base_title = replay_control_core::thumbnails::base_title;
    if let Ok(entries) = std::fs::read_dir(&boxart_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if let Some(img_stem) = name_str.strip_suffix(".png") {
                if dir_index.exact.contains_key(img_stem) {
                    continue; // Already indexed by build_dir_index.
                }
                let full = entry.path();
                if let Some(resolved) =
                    replay_control_core::thumbnails::try_resolve_fake_symlink(
                        &full,
                        &boxart_dir,
                    )
                {
                    let resolved_path = format!("boxart/{resolved}");
                    dir_index
                        .exact
                        .insert(img_stem.to_string(), resolved_path.clone());
                    dir_index
                        .exact_ci
                        .entry(img_stem.to_lowercase())
                        .or_insert_with(|| resolved_path.clone());
                    let bt = base_title(img_stem);
                    let vs = strip_version(&bt).to_string();
                    dir_index
                        .fuzzy
                        .entry(bt.clone())
                        .or_insert_with(|| resolved_path.clone());
                    if vs.len() < bt.len() {
                        dir_index.version.entry(vs).or_insert(resolved_path);
                    }
                }
            }
        }
    }

    // Load user box art overrides first (separate pool, no contention with metadata).
    let system_owned = system.to_string();
    let user_overrides: HashMap<String, String> = state
        .user_data_pool
        .read(move |conn| UserDataDb::get_system_overrides(conn, &system_owned).ok())
        .await
        .flatten()
        .unwrap_or_default();

    // Load DB paths and raw manifest data from the pool, then build
    // the manifest fuzzy index outside the closure to avoid holding
    // the pool connection longer than necessary.
    let system_owned = system.to_string();
    let (mut db_paths, raw_manifest_data) = state
        .metadata_pool
        .read(move |conn| {
            let paths =
                MetadataDb::system_box_art_paths(conn, &system_owned).unwrap_or_default();

            // Pre-fetch raw manifest data while we have the DB lock.
            let raw = if let Some(repo_names) =
                replay_control_core::thumbnails::thumbnail_repo_names(&system_owned)
            {
                let mut repo_data = Vec::new();
                for display_name in repo_names {
                    let url_name = replay_control_core::thumbnails::repo_url_name(display_name);
                    let source_name =
                        replay_control_core::thumbnails::libretro_source_name(display_name);
                    let branch = MetadataDb::get_data_source(conn, &source_name)
                        .ok()
                        .flatten()
                        .and_then(|s| s.branch)
                        .unwrap_or_else(|| "master".to_string());
                    let entries = MetadataDb::query_thumbnail_index(
                        conn,
                        &source_name,
                        replay_control_core::thumbnails::ThumbnailKind::Boxart.repo_dir(),
                    )
                    .unwrap_or_default();
                    repo_data.push((url_name, branch, entries));
                }
                Some(repo_data)
            } else {
                None
            };
            (paths, raw)
        })
        .await
        .unwrap_or_else(|| (HashMap::new(), None));

    // Inject user box art overrides (highest priority — overwrites auto-matched paths).
    for (rom_filename, override_path) in user_overrides {
        db_paths.insert(rom_filename, override_path);
    }

    // Build manifest fuzzy index from pre-fetched data (no DB lock held).
    let manifest = raw_manifest_data.and_then(|repo_data| {
        let idx = replay_control_core::thumbnail_manifest::build_manifest_fuzzy_index_from_raw(
            &repo_data,
        );
        if idx.exact.is_empty() {
            None
        } else {
            Some(idx)
        }
    });

    ImageIndex {
        dir_index,
        db_paths,
        manifest,
    }
}

/// Resolve a box art URL for a single ROM using the image index.
/// If no local image is found but the manifest has a match, a background
/// download is queued and None is returned (image appears after next enrichment).
///
/// Used only by the enrichment pipeline — not at request time.
pub fn resolve_box_art(
    state: &crate::api::AppState,
    index: &ImageIndex,
    system: &str,
    rom_filename: &str,
) -> Option<String> {
    // For arcade ROMs, translate MAME codename to display name.
    let stem = rom_filename
        .rfind('.')
        .map(|i| &rom_filename[..i])
        .unwrap_or(rom_filename);
    let stem = replay_control_core::title_utils::strip_n64dd_prefix(stem);
    let is_arcade = replay_control_core::systems::is_arcade_system(system);
    let arcade_display = if is_arcade {
        replay_control_core::arcade_db::lookup_arcade_game(stem).map(|info| info.display_name)
    } else {
        None
    };

    // Delegate all filesystem-based matching tiers to core.
    let db_paths = if index.db_paths.is_empty() {
        None
    } else {
        Some(&index.db_paths)
    };
    if let Some(path) = replay_control_core::image_matching::find_best_match(
        &index.dir_index,
        rom_filename,
        arcade_display,
        db_paths,
    ) {
        let encoded_path: String = path
            .split('/')
            .map(|seg| urlencoding::encode(seg))
            .collect::<Vec<_>>()
            .join("/");
        return Some(format!("/media/{system}/{encoded_path}"));
    }

    // On-demand: check manifest for a remote thumbnail to download.
    // The download runs in a background thread and updates box_art_url in the
    // DB directly, so the art appears on the next page load.
    if let Some(ref manifest) = index.manifest
        && let Some(m) = replay_control_core::thumbnail_manifest::find_in_manifest(
            manifest,
            rom_filename,
            system,
        )
    {
        queue_on_demand_download(state, system, rom_filename, m);
    }

    None
}

/// Queue a background download for a single thumbnail.
/// Deduplicates concurrent requests for the same image.
fn queue_on_demand_download(
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
                    let encoded_filename = urlencoding::encode(&png_name);
                    let url = format!("/media/{system}/{boxart_dir}/{encoded_filename}");
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
