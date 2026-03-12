use std::path::PathBuf;

use super::AppState;

impl AppState {
    /// Start a background metadata import from a LaunchBox XML file.
    /// Returns `false` if an import is already running.
    pub fn start_import(&self, xml_path: PathBuf) -> bool {
        use replay_control_core::metadata_db::{ImportProgress, ImportState};

        // Check if already running.
        {
            let guard = self
                .import_progress
                .read()
                .expect("import_progress lock poisoned");
            if let Some(ref p) = *guard {
                if matches!(
                    p.state,
                    ImportState::Downloading | ImportState::BuildingIndex | ImportState::Parsing
                ) {
                    return false;
                }
            }
        }

        // Set initial progress.
        {
            let mut guard = self
                .import_progress
                .write()
                .expect("import_progress lock poisoned");
            *guard = Some(ImportProgress {
                state: ImportState::BuildingIndex,
                processed: 0,
                matched: 0,
                inserted: 0,
                elapsed_secs: 0,
                error: None,
            });
        }

        let state = self.clone();
        tokio::task::spawn_blocking(move || {
            let start = std::time::Instant::now();
            state.run_import_blocking(xml_path, start);
        });

        true
    }

    /// Clear metadata DB and re-import from `launchbox-metadata.xml` if present.
    /// Returns an error message if the XML file is not found.
    pub fn regenerate_metadata(&self) -> Result<(), String> {
        use replay_control_core::metadata_db::LAUNCHBOX_XML;

        // Clear existing metadata.
        if let Some(guard) = self.metadata_db() {
            if let Some(db) = guard.as_ref() {
                db.clear().map_err(|e| e.to_string())?;
            }
        }

        // Find launchbox-metadata.xml (with fallback to old name) and trigger re-import.
        let storage = self.storage();
        let rc_dir = storage.rc_dir();
        let xml_path = rc_dir.join(LAUNCHBOX_XML);
        let xml_path = if xml_path.exists() {
            xml_path
        } else {
            // Backwards-compat: check old upstream name.
            let old_path = rc_dir.join("Metadata.xml");
            if old_path.exists() { old_path } else { xml_path }
        };
        if !xml_path.exists() {
            return Err(format!("No {LAUNCHBOX_XML} found. Place it in the .replay-control folder to enable re-import."));
        }

        self.start_import(xml_path);
        Ok(())
    }

    /// Download LaunchBox Metadata.zip, extract, clear DB, and re-import.
    /// Runs entirely in a background thread. Returns false if an import is
    /// already running.
    pub fn start_metadata_download(&self) -> bool {
        use replay_control_core::metadata_db::{ImportProgress, ImportState};

        // Check if already running.
        {
            let guard = self
                .import_progress
                .read()
                .expect("import_progress lock poisoned");
            if let Some(ref p) = *guard {
                if matches!(
                    p.state,
                    ImportState::Downloading | ImportState::BuildingIndex | ImportState::Parsing
                ) {
                    return false;
                }
            }
        }

        // Set initial progress to Downloading.
        {
            let mut guard = self
                .import_progress
                .write()
                .expect("import_progress lock poisoned");
            *guard = Some(ImportProgress {
                state: ImportState::Downloading,
                processed: 0,
                matched: 0,
                inserted: 0,
                elapsed_secs: 0,
                error: None,
            });
        }

        let state = self.clone();
        tokio::task::spawn_blocking(move || {
            let start = std::time::Instant::now();
            let storage = state.storage();
            let rc_dir = storage.rc_dir();

            // Download and extract.
            let xml_path = match replay_control_core::launchbox::download_metadata(&rc_dir) {
                Ok(path) => path,
                Err(e) => {
                    let mut guard = state
                        .import_progress
                        .write()
                        .expect("import_progress lock poisoned");
                    if let Some(ref mut p) = *guard {
                        p.state = ImportState::Failed;
                        p.error = Some(format!("Download failed: {e}"));
                        p.elapsed_secs = start.elapsed().as_secs();
                    }
                    return;
                }
            };

            // Clear existing metadata before re-import.
            if let Some(guard) = state.metadata_db() {
                if let Some(db) = guard.as_ref() {
                    if let Err(e) = db.clear() {
                        tracing::warn!("Failed to clear metadata DB before re-import: {e}");
                    }
                }
            }

            // Update elapsed before starting import.
            {
                let mut guard = state
                    .import_progress
                    .write()
                    .expect("import_progress lock poisoned");
                if let Some(ref mut p) = *guard {
                    p.elapsed_secs = start.elapsed().as_secs();
                }
            }

            // Now run the import (this updates import_progress internally).
            state.run_import_blocking(xml_path, start);
        });

        true
    }

    /// Run the metadata import synchronously (called from spawn_blocking).
    /// Separated from start_import to allow reuse from start_metadata_download.
    fn run_import_blocking(&self, xml_path: PathBuf, start: std::time::Instant) {
        use replay_control_core::metadata_db::{ImportProgress, ImportState};

        // Build ROM index.
        let storage_root = self.storage().root.clone();
        {
            let mut guard = self
                .import_progress
                .write()
                .expect("import_progress lock poisoned");
            if let Some(ref mut p) = *guard {
                p.state = ImportState::BuildingIndex;
                p.elapsed_secs = start.elapsed().as_secs();
            }
        }

        let rom_index = replay_control_core::launchbox::build_rom_index(&storage_root);

        // Update progress to Parsing.
        {
            let mut guard = self
                .import_progress
                .write()
                .expect("import_progress lock poisoned");
            if let Some(ref mut p) = *guard {
                p.state = ImportState::Parsing;
                p.elapsed_secs = start.elapsed().as_secs();
            }
        }

        // Take DB from state.
        let db = {
            let mut guard = self.metadata_db.lock().expect("metadata_db lock poisoned");
            guard.take()
        };
        let mut db = match db {
            Some(db) => db,
            None => match replay_control_core::metadata_db::MetadataDb::open(&storage_root) {
                Ok(db) => db,
                Err(e) => {
                    let mut guard = self
                        .import_progress
                        .write()
                        .expect("import_progress lock poisoned");
                    if let Some(ref mut p) = *guard {
                        p.state = ImportState::Failed;
                        p.error = Some(format!("Cannot open metadata DB: {e}"));
                        p.elapsed_secs = start.elapsed().as_secs();
                    }
                    return;
                }
            },
        };

        let progress_ref = self.import_progress.clone();
        let start_ref = start;
        let result = replay_control_core::launchbox::import_launchbox(
            &xml_path,
            &mut db,
            &rom_index,
            |processed, matched, inserted| {
                let mut guard = progress_ref.write().expect("import_progress lock poisoned");
                if let Some(ref mut p) = *guard {
                    p.processed = processed;
                    p.matched = matched;
                    p.inserted = inserted;
                    p.elapsed_secs = start_ref.elapsed().as_secs();
                }
            },
        );

        // Put DB back.
        {
            let mut guard = self.metadata_db.lock().expect("metadata_db lock poisoned");
            *guard = Some(db);
        }

        // Update final progress.
        {
            let mut guard = self
                .import_progress
                .write()
                .expect("import_progress lock poisoned");
            match result {
                Ok(stats) => {
                    *guard = Some(ImportProgress {
                        state: ImportState::Complete,
                        processed: stats.total_source,
                        matched: stats.matched,
                        inserted: stats.inserted,
                        elapsed_secs: start.elapsed().as_secs(),
                        error: None,
                    });
                }
                Err(e) => {
                    if let Some(ref mut p) = *guard {
                        p.state = ImportState::Failed;
                        p.error = Some(e.to_string());
                        p.elapsed_secs = start.elapsed().as_secs();
                    }
                }
            }
        }
    }

    // ── Thumbnail Update (Manifest + Download) ──────────────────────

    /// Check if a thumbnail update is already running.
    fn is_thumbnail_update_running(&self) -> bool {
        use crate::server_fns::ThumbnailPhase;
        let guard = self
            .thumbnail_progress
            .read()
            .expect("thumbnail_progress lock poisoned");
        guard.as_ref().is_some_and(|p| {
            matches!(
                p.phase,
                ThumbnailPhase::Indexing | ThumbnailPhase::Downloading
            )
        })
    }

    /// Start the two-phase thumbnail pipeline in the background.
    /// Returns `false` if an update is already running.
    pub fn start_thumbnail_update(&self) -> bool {
        if self.is_thumbnail_update_running() {
            return false;
        }

        use crate::server_fns::{ThumbnailPhase, ThumbnailProgress};

        self.thumbnail_cancel
            .store(false, std::sync::atomic::Ordering::Relaxed);

        // Write initial progress before spawning.
        {
            let mut guard = self
                .thumbnail_progress
                .write()
                .expect("thumbnail_progress lock poisoned");
            *guard = Some(ThumbnailProgress {
                phase: ThumbnailPhase::Indexing,
                current_label: String::new(),
                step_done: 0,
                step_total: 0,
                downloaded: 0,
                entries_indexed: 0,
                elapsed_secs: 0,
                error: None,
            });
        }

        let state = self.clone();
        tokio::task::spawn_blocking(move || {
            let start = std::time::Instant::now();
            state.run_thumbnail_update_blocking(start);
        });

        true
    }

    /// Run the two-phase thumbnail pipeline (blocking, called from spawn_blocking).
    fn run_thumbnail_update_blocking(&self, start: std::time::Instant) {
        use crate::server_fns::{ThumbnailPhase, ThumbnailProgress};
        use replay_control_core::thumbnail_manifest;
        use replay_control_core::thumbnails::ThumbnailKind;

        let storage_root = self.storage().root.clone();

        // Take DB from state.
        let db = {
            let mut guard = self.metadata_db.lock().expect("metadata_db lock poisoned");
            guard.take()
        };
        let mut db = match db {
            Some(db) => db,
            None => {
                match replay_control_core::metadata_db::MetadataDb::open(&storage_root) {
                    Ok(db) => db,
                    Err(e) => {
                        let mut guard = self
                            .thumbnail_progress
                            .write()
                            .expect("lock");
                        if let Some(ref mut p) = *guard {
                            p.phase = ThumbnailPhase::Failed;
                            p.error = Some(format!("Cannot open metadata DB: {e}"));
                            p.elapsed_secs = start.elapsed().as_secs();
                        }
                        return;
                    }
                }
            }
        };

        // ── Phase 1: Index refresh ──────────────────────────────────

        let progress_ref = self.thumbnail_progress.clone();
        let cancel_ref = &self.thumbnail_cancel;

        let index_result = thumbnail_manifest::import_all_manifests(
            &mut db,
            &|repos_done, repos_total, current_repo| {
                let mut guard = progress_ref.write().expect("lock");
                if let Some(ref mut p) = *guard {
                    p.phase = ThumbnailPhase::Indexing;
                    p.step_done = repos_done;
                    p.step_total = repos_total;
                    p.current_label = current_repo.to_string();
                    p.elapsed_secs = start.elapsed().as_secs();
                }
            },
            cancel_ref,
        );

        let index_stats = match index_result {
            Ok(stats) => {
                if !stats.errors.is_empty() {
                    tracing::warn!(
                        "Thumbnail index: {} errors: {:?}",
                        stats.errors.len(),
                        stats.errors
                    );
                }
                // Update progress with index results.
                {
                    let mut guard = self
                        .thumbnail_progress
                        .write()
                        .expect("lock");
                    if let Some(ref mut p) = *guard {
                        p.entries_indexed = stats.total_entries;
                        p.elapsed_secs = start.elapsed().as_secs();
                    }
                }
                stats
            }
            Err(e) => {
                let mut guard = self
                    .thumbnail_progress
                    .write()
                    .expect("lock");
                if let Some(ref mut p) = *guard {
                    p.phase = ThumbnailPhase::Failed;
                    p.error = Some(format!("Index failed: {e}"));
                    p.elapsed_secs = start.elapsed().as_secs();
                }
                // Put DB back before returning.
                let mut guard = self.metadata_db.lock().expect("lock");
                *guard = Some(db);
                return;
            }
        };

        // Check cancellation between phases.
        if cancel_ref.load(std::sync::atomic::Ordering::Relaxed) {
            let mut guard = self
                .thumbnail_progress
                .write()
                .expect("lock");
            if let Some(ref mut p) = *guard {
                p.phase = ThumbnailPhase::Cancelled;
                p.elapsed_secs = start.elapsed().as_secs();
            }
            let mut guard = self.metadata_db.lock().expect("lock");
            *guard = Some(db);
            return;
        }

        // ── Phase 2: Download images ────────────────────────────────

        {
            let mut guard = self
                .thumbnail_progress
                .write()
                .expect("lock");
            if let Some(ref mut p) = *guard {
                p.phase = ThumbnailPhase::Downloading;
                p.step_done = 0;
                p.step_total = 0;
                p.downloaded = 0;
                p.elapsed_secs = start.elapsed().as_secs();
            }
        }

        // Collect systems that have ROMs and a thumbnail repo.
        let storage = self.storage();
        let systems = self.cache.get_systems(&storage);
        let supported: Vec<String> = systems
            .into_iter()
            .filter(|s| s.game_count > 0)
            .filter(|s| {
                replay_control_core::thumbnails::thumbnail_repo_names(&s.folder_name)
                    .is_some()
            })
            .map(|s| s.folder_name)
            .collect();

        let total_systems = supported.len();
        let mut total_downloaded = 0usize;
        let mut total_failed = 0usize;

        for (i, system) in supported.iter().enumerate() {
            if cancel_ref.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }

            let system_display = replay_control_core::systems::find_system(system)
                .map(|s| s.display_name.to_string())
                .unwrap_or_else(|| system.to_string());

            // Update progress for this system.
            {
                let mut guard = self
                    .thumbnail_progress
                    .write()
                    .expect("lock");
                if let Some(ref mut p) = *guard {
                    p.current_label = system_display.clone();
                    p.step_done = i;
                    p.step_total = total_systems;
                    p.elapsed_secs = start.elapsed().as_secs();
                }
            }

            let progress_ref = self.thumbnail_progress.clone();
            let prev_downloaded = total_downloaded;

            // Download boxart for this system.
            let result = thumbnail_manifest::download_system_thumbnails(
                &db,
                &storage_root,
                system,
                ThumbnailKind::Boxart,
                &|processed, total, downloaded| {
                    let mut guard = progress_ref.write().expect("lock");
                    if let Some(ref mut p) = *guard {
                        p.step_done = i;
                        p.step_total = total_systems;
                        p.downloaded = prev_downloaded + downloaded;
                        p.elapsed_secs = start.elapsed().as_secs();
                        // Encode per-system progress in current_label.
                        if total > 0 {
                            p.current_label =
                                format!("{system_display}: {processed}/{total}");
                        }
                    }
                },
                cancel_ref,
            );

            match result {
                Ok(stats) => {
                    total_downloaded += stats.downloaded;
                    total_failed += stats.failed;
                }
                Err(e) => {
                    tracing::warn!("Thumbnail download failed for {system}: {e}");
                }
            }

            // Also download snaps for this system.
            if !cancel_ref.load(std::sync::atomic::Ordering::Relaxed) {
                let prev_downloaded = total_downloaded;
                let result = thumbnail_manifest::download_system_thumbnails(
                    &db,
                    &storage_root,
                    system,
                    ThumbnailKind::Snap,
                    &|_processed, _total, downloaded| {
                        let mut guard = progress_ref.write().expect("lock");
                        if let Some(ref mut p) = *guard {
                            p.downloaded = prev_downloaded + downloaded;
                            p.elapsed_secs = start.elapsed().as_secs();
                        }
                    },
                    cancel_ref,
                );

                match result {
                    Ok(stats) => {
                        total_downloaded += stats.downloaded;
                        total_failed += stats.failed;
                    }
                    Err(e) => {
                        tracing::warn!("Snap download failed for {system}: {e}");
                    }
                }
            }

            // Update DB image paths for this system (same as the existing import does).
            // Use the rom_filenames to update game_metadata box_art_path / screenshot_path.
            Self::update_image_paths_from_disk(&mut db, &storage_root, system);
        }

        // Put DB back.
        {
            let mut guard = self.metadata_db.lock().expect("lock");
            *guard = Some(db);
        }

        // Invalidate the image cache so new thumbnails are picked up.
        self.cache.invalidate_images();

        // Set final progress.
        {
            let mut guard = self
                .thumbnail_progress
                .write()
                .expect("lock");
            if cancel_ref.load(std::sync::atomic::Ordering::Relaxed) {
                if let Some(ref mut p) = *guard {
                    p.phase = ThumbnailPhase::Cancelled;
                    p.downloaded = total_downloaded;
                    p.elapsed_secs = start.elapsed().as_secs();
                }
            } else {
                *guard = Some(ThumbnailProgress {
                    phase: ThumbnailPhase::Complete,
                    current_label: String::new(),
                    step_done: total_systems,
                    step_total: total_systems,
                    downloaded: total_downloaded,
                    entries_indexed: index_stats.total_entries,
                    elapsed_secs: start.elapsed().as_secs(),
                    error: if total_failed > 0 {
                        Some(format!("{total_failed} images failed to download"))
                    } else {
                        None
                    },
                });
            }
        }
    }

    /// Scan the media directory for a system and update game_metadata image paths.
    fn update_image_paths_from_disk(
        db: &mut replay_control_core::metadata_db::MetadataDb,
        storage_root: &std::path::Path,
        system: &str,
    ) {
        use replay_control_core::thumbnails;

        let rom_filenames = thumbnails::list_rom_filenames(storage_root, system);
        let media_base = storage_root
            .join(replay_control_core::storage::RC_DIR)
            .join("media")
            .join(system);

        let boxart_dir = media_base.join("boxart");
        let snap_dir = media_base.join("snap");

        let mut updates: Vec<(String, String, Option<String>, Option<String>)> = Vec::new();

        for rom_filename in &rom_filenames {
            let stem = rom_filename
                .rfind('.')
                .map(|i| &rom_filename[..i])
                .unwrap_or(rom_filename);
            let thumb_name = thumbnails::thumbnail_filename(stem);

            let boxart_path = boxart_dir.join(format!("{thumb_name}.png"));
            let snap_path = snap_dir.join(format!("{thumb_name}.png"));

            let boxart_rel = if boxart_path.exists() {
                Some(format!("boxart/{thumb_name}.png"))
            } else {
                None
            };
            let snap_rel = if snap_path.exists() {
                Some(format!("snap/{thumb_name}.png"))
            } else {
                None
            };

            if boxart_rel.is_some() || snap_rel.is_some() {
                updates.push((
                    system.to_string(),
                    rom_filename.clone(),
                    boxart_rel,
                    snap_rel,
                ));
            }
        }

        if !updates.is_empty() {
            if let Err(e) = db.bulk_update_image_paths(&updates) {
                tracing::warn!("Failed to update image paths for {system}: {e}");
            }
        }
    }
}
