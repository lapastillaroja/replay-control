use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Instant, SystemTime};

use replay_control_core::image_matching::DirIndex;
use replay_control_core::thumbnail_manifest::ManifestFuzzyIndex;

use super::{CACHE_HARD_TTL, GameLibrary, dir_mtime};

/// Cached per-system image directory index for batch box art resolution.
/// Wraps a core `DirIndex` for filesystem-based matching, plus app-specific
/// fields for DB path lookups and on-demand manifest downloads.
pub struct ImageIndex {
    /// Core directory index: exact, case-insensitive, fuzzy, version-stripped.
    pub dir_index: DirIndex,
    /// DB paths: rom_filename → "boxart/{path}"
    pub db_paths: HashMap<String, String>,
    /// Manifest-backed fallback for images not yet downloaded.
    /// None if the thumbnail_index has no entries for this system.
    pub manifest: Option<ManifestFuzzyIndex>,
    dir_mtime: Option<SystemTime>,
    expires: Instant,
}

impl ImageIndex {
    pub(super) fn is_fresh(&self, boxart_dir: &std::path::Path) -> bool {
        if Instant::now() >= self.expires {
            return false;
        }
        match (self.dir_mtime, dir_mtime(boxart_dir)) {
            (Some(cached), Some(current)) => cached == current,
            _ => true,
        }
    }
}

impl GameLibrary {
    /// Get or build the image index for a system.
    /// The index maps normalized image names to actual paths, enabling O(1) box art lookups.
    pub fn get_image_index(
        &self,
        state: &crate::api::AppState,
        system: &str,
    ) -> std::sync::Arc<ImageIndex> {
        use replay_control_core::thumbnails::strip_version;

        let boxart_media = replay_control_core::thumbnails::ThumbnailKind::Boxart.media_dir();
        let media_base = state.storage().rc_dir().join("media").join(system);
        let boxart_dir = media_base.join(boxart_media);

        // Check cache freshness — return a cheap Arc::clone() on hit.
        if let Ok(guard) = self.images.read()
            && let Some(idx) = guard.get(system)
            && idx.is_fresh(&boxart_dir)
        {
            return Arc::clone(idx);
        }

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

        // Load user box art overrides first (separate lock, released before metadata_db).
        let user_overrides: HashMap<String, String> = state
            .user_data_db()
            .and_then(|guard| {
                guard
                    .as_ref()
                    .and_then(|db| db.get_system_overrides(system).ok())
            })
            .unwrap_or_default();

        // Load DB paths and raw manifest data under a brief lock, then build
        // the manifest fuzzy index outside the lock to avoid blocking other
        // threads that need metadata_db (tokio worker starvation).
        let (mut db_paths, raw_manifest_data) = if let Some(guard) = state.metadata_db() {
            if let Some(db) = guard.as_ref() {
                let paths = db.system_box_art_paths(system).unwrap_or_default();

                // Pre-fetch raw manifest data while we have the DB lock.
                let raw = if let Some(repo_names) =
                    replay_control_core::thumbnails::thumbnail_repo_names(system)
                {
                    let mut repo_data = Vec::new();
                    for display_name in repo_names {
                        let url_name =
                            replay_control_core::thumbnails::repo_url_name(display_name);
                        let source_name =
                            replay_control_core::thumbnails::libretro_source_name(display_name);
                        let branch = db
                            .get_data_source(&source_name)
                            .ok()
                            .flatten()
                            .and_then(|s| s.branch)
                            .unwrap_or_else(|| "master".to_string());
                        let entries = db
                            .query_thumbnail_index(
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
            } else {
                (HashMap::new(), None)
            }
        } else {
            (HashMap::new(), None)
        };
        // metadata_db lock released here.

        // Inject user box art overrides (highest priority — overwrites auto-matched paths).
        for (rom_filename, override_path) in user_overrides {
            db_paths.insert(rom_filename, override_path);
        }

        // Build manifest fuzzy index from pre-fetched data (no DB lock held).
        let manifest = raw_manifest_data.and_then(|repo_data| {
            let idx =
                replay_control_core::thumbnail_manifest::build_manifest_fuzzy_index_from_raw(
                    &repo_data,
                );
            if idx.exact.is_empty() {
                None
            } else {
                Some(idx)
            }
        });

        let arc = Arc::new(ImageIndex {
            dir_index,
            db_paths,
            manifest,
            dir_mtime: dir_mtime(&boxart_dir),
            expires: Instant::now() + CACHE_HARD_TTL,
        });

        if let Ok(mut guard) = self.images.write() {
            guard.insert(system.to_string(), Arc::clone(&arc));
        }

        arc
    }

    /// Resolve a box art URL for a single ROM using the cached image index.
    /// If no local image is found but the manifest has a match, a background
    /// download is queued and None is returned (image appears on next load).
    pub fn resolve_box_art(
        &self,
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
            return Some(format!("/media/{system}/{path}"));
        }

        // On-demand: check manifest for a remote thumbnail to download.
        if let Some(ref manifest) = index.manifest
            && let Some(m) = replay_control_core::thumbnail_manifest::find_in_manifest(
                manifest,
                rom_filename,
                system,
            )
        {
            self.queue_on_demand_download(state, system, m);
        }

        None
    }

    /// Queue a background download for a single thumbnail.
    /// Deduplicates concurrent requests for the same image.
    fn queue_on_demand_download(
        &self,
        state: &crate::api::AppState,
        system: &str,
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
        let pending = state.pending_downloads.clone();
        let cache = state.cache.clone();

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
                        // Invalidate image cache so the next page load picks up the new file.
                        cache.invalidate_system_images(&system);
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
}
