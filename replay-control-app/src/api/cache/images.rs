use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Instant, SystemTime};

use replay_control_core::thumbnail_manifest::ManifestFuzzyIndex;

use super::{CACHE_HARD_TTL, GameLibrary, dir_mtime};

/// Cached per-system image directory index for batch box art resolution.
/// Maps normalized base title → actual filename (without directory prefix).
pub struct ImageIndex {
    /// exact thumbnail_filename stem → "boxart/{filename}.png"
    pub exact: HashMap<String, String>,
    /// fuzzy base_title (lowercase, tags stripped) → "boxart/{filename}.png"
    pub fuzzy: HashMap<String, String>,
    /// version-stripped base_title → "boxart/{filename}.png"
    pub version: HashMap<String, String>,
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

        let media_base = state.storage().rc_dir().join("media").join(system);
        let boxart_dir = media_base.join("boxart");

        // Check cache freshness — return a cheap Arc::clone() on hit.
        if let Ok(guard) = self.images.read()
            && let Some(idx) = guard.get(system)
            && idx.is_fresh(&boxart_dir)
        {
            return Arc::clone(idx);
        }

        // Build the base index using the shared image matching module.
        // This indexes all valid (>= 200 byte) .png files.
        let dir_index = replay_control_core::image_matching::build_dir_index(&boxart_dir, "boxart");
        let mut exact = dir_index.exact;
        let mut fuzzy = dir_index.fuzzy;
        let mut version = dir_index.version;

        // Second pass: resolve fake symlinks (small text files pointing to real images).
        // These are skipped by build_dir_index since they're < 200 bytes.
        let base_title = replay_control_core::thumbnails::base_title;
        if let Ok(entries) = std::fs::read_dir(&boxart_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if let Some(img_stem) = name_str.strip_suffix(".png") {
                    if exact.contains_key(img_stem) {
                        continue; // Already indexed by build_dir_index.
                    }
                    let full = entry.path();
                    if let Some(resolved) =
                        replay_control_core::thumbnails::try_resolve_fake_symlink(&full, &boxart_dir)
                    {
                        let resolved_path = format!("boxart/{resolved}");
                        exact.insert(img_stem.to_string(), resolved_path.clone());
                        let bt = base_title(img_stem);
                        let vs = strip_version(&bt).to_string();
                        fuzzy
                            .entry(bt.clone())
                            .or_insert_with(|| resolved_path.clone());
                        if vs.len() < bt.len() {
                            version.entry(vs).or_insert(resolved_path);
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

        // Load DB paths for this system.
        let (db_paths, manifest) = if let Some(guard) = state.metadata_db() {
            if let Some(db) = guard.as_ref() {
                let mut paths = db.system_box_art_paths(system).unwrap_or_default();

                // Inject user box art overrides (highest priority — overwrites auto-matched paths).
                for (rom_filename, override_path) in user_overrides {
                    paths.insert(rom_filename, override_path);
                }

                // Build manifest fuzzy index for on-demand downloads.
                let mfi = if let Some(repo_names) =
                    replay_control_core::thumbnails::thumbnail_repo_names(system)
                {
                    let idx = replay_control_core::thumbnail_manifest::build_manifest_fuzzy_index(
                        db,
                        repo_names,
                        "Named_Boxarts",
                    );
                    if idx.exact.is_empty() {
                        None
                    } else {
                        Some(idx)
                    }
                } else {
                    None
                };
                (paths, mfi)
            } else {
                (HashMap::new(), None)
            }
        } else {
            (HashMap::new(), None)
        };

        let arc = Arc::new(ImageIndex {
            exact,
            fuzzy,
            version,
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
        use replay_control_core::thumbnails::{strip_version, thumbnail_filename};

        // 1. Try DB path first (already validated during index build).
        if let Some(db_path) = index.db_paths.get(rom_filename) {
            let stem = db_path.strip_prefix("boxart/").unwrap_or(db_path);
            let stem = stem.strip_suffix(".png").unwrap_or(stem);
            if index.exact.contains_key(stem) {
                return Some(format!("/media/{system}/{db_path}"));
            }
        }

        // 2. Exact thumbnail name match.
        let stem = rom_filename
            .rfind('.')
            .map(|i| &rom_filename[..i])
            .unwrap_or(rom_filename);
        let stem = replay_control_core::title_utils::strip_n64dd_prefix(stem);

        // For arcade ROMs, translate MAME codename to display name.
        let is_arcade = replay_control_core::systems::is_arcade_system(system);
        let display_name = if is_arcade {
            replay_control_core::arcade_db::lookup_arcade_game(stem)
                .map(|info| info.display_name)
        } else {
            None
        };
        let thumb_name = thumbnail_filename(display_name.unwrap_or(stem));

        if let Some(path) = index.exact.get(&thumb_name) {
            return Some(format!("/media/{system}/{path}"));
        }

        // Colon variants for arcade games (e.g., "Marvel vs. Capcom: Clash of Super Heroes").
        let source = display_name.unwrap_or(stem);
        if source.contains(':') {
            let dash_variant = thumbnail_filename(&source.replace(": ", " - ").replace(':', " -"));
            if let Some(path) = index.exact.get(&dash_variant) {
                return Some(format!("/media/{system}/{path}"));
            }
            let drop_variant = thumbnail_filename(&source.replace(": ", " ").replace(':', ""));
            if let Some(path) = index.exact.get(&drop_variant) {
                return Some(format!("/media/{system}/{path}"));
            }
        }

        // 3. Fuzzy match (strip tags).
        let base_title = replay_control_core::thumbnails::base_title;

        let rom_base = base_title(&thumb_name);
        if let Some(path) = index.fuzzy.get(&rom_base) {
            return Some(format!("/media/{system}/{path}"));
        }

        // 3b. Tilde dual-title match: "Name1 ~ Name2" -> try each half.
        let source = display_name.unwrap_or(stem);
        if source.contains(" ~ ") {
            for half in source.split(" ~ ") {
                let half_thumb = thumbnail_filename(half.trim());
                let half_base = base_title(&half_thumb);
                if let Some(path) = index.fuzzy.get(&half_base) {
                    return Some(format!("/media/{system}/{path}"));
                }
                // Also try exact match for each half
                if let Some(path) = index.exact.get(&half_thumb) {
                    return Some(format!("/media/{system}/{path}"));
                }
            }
        }

        // 4. Version-stripped match.
        let rom_base_no_version = strip_version(&rom_base);
        if rom_base_no_version.len() < rom_base.len()
            && let Some(path) = index.fuzzy.get(rom_base_no_version).or_else(|| index.version.get(rom_base_no_version))
        {
            return Some(format!("/media/{system}/{path}"));
        }

        // 5. On-demand: check manifest for a remote thumbnail to download.
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
            match download_thumbnail(&m, "Named_Boxarts") {
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
