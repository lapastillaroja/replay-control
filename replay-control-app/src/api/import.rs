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

    /// Check if an image import is already running.
    fn is_image_import_running(&self) -> bool {
        use crate::server_fns::ImageImportState;
        let guard = self
            .image_import_progress
            .read()
            .expect("image_import_progress lock poisoned");
        guard.as_ref().is_some_and(|p| {
            matches!(
                p.state,
                ImageImportState::Cloning | ImageImportState::Copying
            )
        })
    }

    /// Import images for a single system (blocking, runs on current thread).
    /// Updates `image_import_progress` as it goes. Returns the import result.
    fn import_system_images_blocking(
        &self,
        system: &str,
        current_system: usize,
        total_systems: usize,
        start: std::time::Instant,
    ) {
        use crate::server_fns::{ImageImportProgress, ImageImportState};

        let system_display = replay_control_core::systems::find_system(system)
            .map(|s| s.display_name.to_string())
            .unwrap_or_else(|| system.to_string());

        let repo_names = match replay_control_core::thumbnails::thumbnail_repo_names(system) {
            Some(names) => names,
            None => {
                tracing::warn!("No thumbnail repo for {system}, skipping");
                return;
            }
        };

        let storage = self.storage();
        let storage_root = storage.root.clone();
        let clone_base = storage.rc_dir().join("tmp");
        let rom_filenames =
            replay_control_core::thumbnails::list_rom_filenames(&storage_root, system);

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
                    let mut guard = self.image_import_progress.write().expect("lock");
                    if let Some(ref mut p) = *guard {
                        p.state = ImageImportState::Failed;
                        p.error = Some(format!("Cannot open metadata DB: {e}"));
                        p.elapsed_secs = start.elapsed().as_secs();
                    }
                    return;
                }
            },
        };

        let mut total_boxart = 0usize;
        let mut total_snap = 0usize;
        let mut last_error: Option<String> = None;

        // Import from each repo in order. import_system_thumbnails skips ROMs
        // that already have images, so later repos only fill gaps.
        for (repo_idx, repo_name) in repo_names.iter().enumerate() {
            // Check for cancellation before each repo.
            if self
                .image_import_cancel
                .load(std::sync::atomic::Ordering::Relaxed)
            {
                let mut guard = self.image_import_progress.write().expect("lock");
                if let Some(ref mut p) = *guard {
                    p.state = ImageImportState::Cancelled;
                    p.elapsed_secs = start.elapsed().as_secs();
                }
                break;
            }

            let label = if repo_names.len() > 1 {
                format!("{system_display} ({repo_name})")
            } else {
                system_display.clone()
            };

            // Set cloning progress.
            {
                let mut guard = self.image_import_progress.write().expect("lock");
                *guard = Some(ImageImportProgress {
                    state: ImageImportState::Cloning,
                    system: system.to_string(),
                    system_display: label.clone(),
                    processed: 0,
                    total: rom_filenames.len(),
                    boxart_copied: total_boxart,
                    snap_copied: total_snap,
                    elapsed_secs: start.elapsed().as_secs(),
                    error: None,
                    current_system,
                    total_systems,
                });
            }

            // Check if an existing clone is stale (upstream has new images).
            // If stale, remove it so clone_thumbnail_repo does a fresh clone.
            let existing = clone_base.join("libretro-thumbnails").join(repo_name);
            if existing.join("Named_Boxarts").exists()
                && replay_control_core::thumbnails::is_repo_stale(&existing, repo_name)
            {
                tracing::info!("Repo {repo_name} is stale, re-cloning");
                let _ = std::fs::remove_dir_all(&existing);
            }

            let (repo_dir, freshly_cloned) =
                match replay_control_core::thumbnails::clone_thumbnail_repo(
                    repo_name,
                    Some(&clone_base),
                    Some(&self.image_import_cancel),
                ) {
                    Ok(result) => result,
                    Err(e) => {
                    // If cancelled during clone, set Cancelled state and stop.
                    if self
                        .image_import_cancel
                        .load(std::sync::atomic::Ordering::Relaxed)
                    {
                        let mut guard = self.image_import_progress.write().expect("lock");
                        if let Some(ref mut p) = *guard {
                            p.state = ImageImportState::Cancelled;
                            p.elapsed_secs = start.elapsed().as_secs();
                        }
                        break;
                    }
                    tracing::warn!("Clone failed for {repo_name}: {e}");
                    // For multi-repo systems, continue to next repo instead of failing entirely
                    if repo_idx == 0 && repo_names.len() == 1 {
                        let mut guard = self.image_import_progress.write().expect("lock");
                        if let Some(ref mut p) = *guard {
                            p.state = ImageImportState::Failed;
                            p.error = Some(format!("Clone failed: {e}"));
                            p.elapsed_secs = start.elapsed().as_secs();
                        }
                    } else {
                        last_error = Some(format!("Clone failed for {repo_name}: {e}"));
                    }
                    continue;
                }
            };

            // Check for cancellation after clone.
            if self
                .image_import_cancel
                .load(std::sync::atomic::Ordering::Relaxed)
            {
                let mut guard = self.image_import_progress.write().expect("lock");
                if let Some(ref mut p) = *guard {
                    p.state = ImageImportState::Cancelled;
                    p.elapsed_secs = start.elapsed().as_secs();
                }
                break;
            }

            // Resolve fake symlinks only on fresh clones (already done inside
            // clone_thumbnail_repo). Reused repos were resolved on their original clone.
            if !freshly_cloned {
                tracing::debug!("Skipping symlink resolution for reused repo {repo_name}");
            }

            // Update progress to Copying.
            {
                let mut guard = self.image_import_progress.write().expect("lock");
                if let Some(ref mut p) = *guard {
                    p.state = ImageImportState::Copying;
                    p.system_display = label;
                    p.total = rom_filenames.len();
                    p.elapsed_secs = start.elapsed().as_secs();
                }
            }

            let progress_ref = self.image_import_progress.clone();
            let cancel_ref = self.image_import_cancel.clone();
            let prev_boxart = total_boxart;
            let result = replay_control_core::thumbnails::import_system_thumbnails(
                &repo_dir,
                system,
                &storage_root,
                &mut db,
                &rom_filenames,
                |processed, images_found| {
                    let mut guard = progress_ref.write().expect("lock");
                    if let Some(ref mut p) = *guard {
                        p.processed = processed;
                        p.boxart_copied = prev_boxart + images_found;
                        p.elapsed_secs = start.elapsed().as_secs();
                    }
                    !cancel_ref.load(std::sync::atomic::Ordering::Relaxed)
                },
            );

            match result {
                Ok(stats) => {
                    total_boxart += stats.boxart_copied;
                    total_snap += stats.snap_copied;
                }
                Err(e) => {
                    last_error = Some(e.to_string());
                }
            }

            // Check for cancellation after copy.
            if self
                .image_import_cancel
                .load(std::sync::atomic::Ordering::Relaxed)
            {
                let mut guard = self.image_import_progress.write().expect("lock");
                if let Some(ref mut p) = *guard {
                    p.state = ImageImportState::Cancelled;
                    p.boxart_copied = total_boxart;
                    p.snap_copied = total_snap;
                    p.elapsed_secs = start.elapsed().as_secs();
                }
                break;
            }
        }

        // Put DB back.
        {
            let mut guard = self.metadata_db.lock().expect("metadata_db lock poisoned");
            *guard = Some(db);
        }

        // Update final progress for this system (skip if already cancelled).
        {
            let mut guard = self.image_import_progress.write().expect("lock");
            let already_cancelled = guard
                .as_ref()
                .map(|p| p.state == ImageImportState::Cancelled)
                .unwrap_or(false);
            if !already_cancelled {
                if last_error.is_some() && total_boxart == 0 && total_snap == 0 {
                    if let Some(ref mut p) = *guard {
                        p.state = ImageImportState::Failed;
                        p.error = last_error;
                        p.elapsed_secs = start.elapsed().as_secs();
                    }
                } else {
                    *guard = Some(ImageImportProgress {
                        state: ImageImportState::Complete,
                        system: system.to_string(),
                        system_display,
                        processed: rom_filenames.len(),
                        total: rom_filenames.len(),
                        boxart_copied: total_boxart,
                        snap_copied: total_snap,
                        elapsed_secs: start.elapsed().as_secs(),
                        error: None,
                        current_system,
                        total_systems,
                    });
                }
            }
        }
    }

    /// Start a background image import for a single system.
    /// Returns `false` if an import is already running.
    pub fn start_image_import(&self, system: String) -> bool {
        if self.is_image_import_running() {
            return false;
        }

        use crate::server_fns::{ImageImportProgress, ImageImportState};
        self.image_import_cancel
            .store(false, std::sync::atomic::Ordering::Relaxed);
        // Write initial progress before spawning so the first poll never returns None.
        {
            let mut guard = self.image_import_progress.write().expect("lock");
            *guard = Some(ImageImportProgress {
                state: ImageImportState::Cloning,
                system: system.clone(),
                system_display: system.clone(),
                processed: 0,
                total: 0,
                boxart_copied: 0,
                snap_copied: 0,
                elapsed_secs: 0,
                error: None,
                current_system: 1,
                total_systems: 1,
            });
        }
        let state = self.clone();
        tokio::task::spawn_blocking(move || {
            let start = std::time::Instant::now();
            state.import_system_images_blocking(&system, 1, 1, start);
        });

        true
    }

    /// Start a background image import for all supported systems.
    /// Returns `false` if an import is already running.
    pub fn start_all_images_import(&self) -> bool {
        if self.is_image_import_running() {
            return false;
        }

        // Collect systems that have a thumbnail repo and games on disk.
        let storage = self.storage();
        let systems = self.cache.get_systems(&storage);
        let supported: Vec<String> = systems
            .into_iter()
            .filter(|s| s.game_count > 0)
            .filter(|s| {
                replay_control_core::thumbnails::thumbnail_repo_names(&s.folder_name).is_some()
            })
            .map(|s| s.folder_name)
            .collect();

        if supported.is_empty() {
            return false;
        }

        use crate::server_fns::{ImageImportProgress, ImageImportState};
        self.image_import_cancel
            .store(false, std::sync::atomic::Ordering::Relaxed);
        // Write initial progress before spawning so the first poll never returns None.
        {
            let total = supported.len();
            let mut guard = self.image_import_progress.write().expect("lock");
            *guard = Some(ImageImportProgress {
                state: ImageImportState::Cloning,
                system: supported[0].clone(),
                system_display: String::new(),
                processed: 0,
                total: 0,
                boxart_copied: 0,
                snap_copied: 0,
                elapsed_secs: 0,
                error: None,
                current_system: 1,
                total_systems: total,
            });
        }
        let state = self.clone();
        let total = supported.len();
        tokio::task::spawn_blocking(move || {
            let start = std::time::Instant::now();
            for (i, system) in supported.iter().enumerate() {
                state.import_system_images_blocking(system, i + 1, total, start);

                // If the last system failed or was cancelled, stop the whole batch.
                {
                    use crate::server_fns::ImageImportState;
                    let guard = state.image_import_progress.read().expect("lock");
                    if let Some(ref p) = *guard {
                        if matches!(
                            p.state,
                            ImageImportState::Failed | ImageImportState::Cancelled
                        ) {
                            break;
                        }
                    }
                }
            }
        });

        true
    }

    /// Re-match images for all systems using already-cloned repos.
    /// Deletes existing media per system, resolves fake symlinks, and re-runs
    /// the image matching logic without re-downloading repos from GitHub.
    /// Returns `false` if an import is already running or no cloned repos exist.
    pub fn start_rematch_all_images(&self) -> bool {
        if self.is_image_import_running() {
            return false;
        }

        let storage = self.storage();
        let clone_base = storage
            .rc_dir()
            .join("tmp")
            .join("libretro-thumbnails");

        // Collect systems that have a cloned repo on disk.
        let systems = self.cache.get_systems(&storage);
        let supported: Vec<String> = systems
            .into_iter()
            .filter(|s| s.game_count > 0)
            .filter(|s| {
                replay_control_core::thumbnails::thumbnail_repo_names(&s.folder_name)
                    .is_some_and(|repos| {
                        repos
                            .iter()
                            .any(|r| clone_base.join(r).join("Named_Boxarts").exists())
                    })
            })
            .map(|s| s.folder_name)
            .collect();

        if supported.is_empty() {
            return false;
        }

        use crate::server_fns::{ImageImportProgress, ImageImportState};
        self.image_import_cancel
            .store(false, std::sync::atomic::Ordering::Relaxed);
        {
            let total = supported.len();
            let mut guard = self.image_import_progress.write().expect("lock");
            *guard = Some(ImageImportProgress {
                state: ImageImportState::Copying,
                system: supported[0].clone(),
                system_display: String::new(),
                processed: 0,
                total: 0,
                boxart_copied: 0,
                snap_copied: 0,
                elapsed_secs: 0,
                error: None,
                current_system: 1,
                total_systems: total,
            });
        }
        let state = self.clone();
        let total = supported.len();
        tokio::task::spawn_blocking(move || {
            let start = std::time::Instant::now();
            for (i, system) in supported.iter().enumerate() {
                state.rematch_system_images_blocking(system, i + 1, total, start);

                {
                    use crate::server_fns::ImageImportState;
                    let guard = state.image_import_progress.read().expect("lock");
                    if let Some(ref p) = *guard {
                        if matches!(
                            p.state,
                            ImageImportState::Failed | ImageImportState::Cancelled
                        ) {
                            break;
                        }
                    }
                }
            }
        });

        true
    }

    /// Re-match images for a single system using its already-cloned repo.
    /// Clears existing media and DB paths, resolves fake symlinks, then
    /// re-runs `import_system_thumbnails`.
    fn rematch_system_images_blocking(
        &self,
        system: &str,
        current_system: usize,
        total_systems: usize,
        start: std::time::Instant,
    ) {
        use crate::server_fns::{ImageImportProgress, ImageImportState};

        let system_display = replay_control_core::systems::find_system(system)
            .map(|s| s.display_name.to_string())
            .unwrap_or_else(|| system.to_string());

        let repo_names = match replay_control_core::thumbnails::thumbnail_repo_names(system) {
            Some(names) => names,
            None => return,
        };

        let storage = self.storage();
        let storage_root = storage.root.clone();
        let clone_base = storage.rc_dir().join("tmp").join("libretro-thumbnails");
        let rom_filenames =
            replay_control_core::thumbnails::list_rom_filenames(&storage_root, system);

        // Clear existing media files and DB image paths for this system.
        let _ = replay_control_core::thumbnails::clear_system_media(&storage_root, system);
        {
            let guard = self.metadata_db.lock().expect("metadata_db lock poisoned");
            if let Some(ref db) = *guard {
                let _ = db.clear_system_image_paths(system);
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
                    let mut guard = self.image_import_progress.write().expect("lock");
                    if let Some(ref mut p) = *guard {
                        p.state = ImageImportState::Failed;
                        p.error = Some(format!("Cannot open metadata DB: {e}"));
                        p.elapsed_secs = start.elapsed().as_secs();
                    }
                    return;
                }
            },
        };

        let mut total_boxart = 0usize;
        let mut total_snap = 0usize;

        for repo_name in repo_names {
            if self
                .image_import_cancel
                .load(std::sync::atomic::Ordering::Relaxed)
            {
                let mut guard = self.image_import_progress.write().expect("lock");
                if let Some(ref mut p) = *guard {
                    p.state = ImageImportState::Cancelled;
                    p.elapsed_secs = start.elapsed().as_secs();
                }
                break;
            }

            let mut repo_dir = clone_base.join(repo_name);
            if !repo_dir.join("Named_Boxarts").exists() {
                continue;
            }

            // Check if upstream has new images — re-clone if stale.
            let mut freshly_cloned = false;
            if replay_control_core::thumbnails::is_repo_stale(&repo_dir, repo_name) {
                tracing::info!("Re-cloning stale repo {repo_name}");
                let _ = std::fs::remove_dir_all(&repo_dir);
                let clone_base_parent = self.storage().rc_dir().join("tmp");
                match replay_control_core::thumbnails::clone_thumbnail_repo(
                    repo_name,
                    Some(&clone_base_parent),
                    Some(&self.image_import_cancel),
                ) {
                    Ok((dir, _)) => {
                        repo_dir = dir;
                        freshly_cloned = true;
                    }
                    Err(e) => {
                        tracing::warn!("Re-clone failed for {repo_name}: {e}");
                        continue;
                    }
                }
            }

            let label = if repo_names.len() > 1 {
                format!("{system_display} ({repo_name})")
            } else {
                system_display.clone()
            };

            // Resolve fake symlinks only if repo wasn't freshly cloned
            // (fresh clones resolve symlinks during clone_thumbnail_repo).
            if !freshly_cloned {
                replay_control_core::thumbnails::resolve_fake_symlinks_in_dir(&repo_dir);
            }

            // Update progress to Copying.
            {
                let mut guard = self.image_import_progress.write().expect("lock");
                *guard = Some(ImageImportProgress {
                    state: ImageImportState::Copying,
                    system: system.to_string(),
                    system_display: label,
                    processed: 0,
                    total: rom_filenames.len(),
                    boxart_copied: total_boxart,
                    snap_copied: total_snap,
                    elapsed_secs: start.elapsed().as_secs(),
                    error: None,
                    current_system,
                    total_systems,
                });
            }

            let progress_ref = self.image_import_progress.clone();
            let cancel_ref = self.image_import_cancel.clone();
            let prev_boxart = total_boxart;
            let result = replay_control_core::thumbnails::import_system_thumbnails(
                &repo_dir,
                system,
                &storage_root,
                &mut db,
                &rom_filenames,
                |processed, images_found| {
                    let mut guard = progress_ref.write().expect("lock");
                    if let Some(ref mut p) = *guard {
                        p.processed = processed;
                        p.boxart_copied = prev_boxart + images_found;
                        p.elapsed_secs = start.elapsed().as_secs();
                    }
                    !cancel_ref.load(std::sync::atomic::Ordering::Relaxed)
                },
            );

            match result {
                Ok(stats) => {
                    total_boxart += stats.boxart_copied;
                    total_snap += stats.snap_copied;
                }
                Err(e) => {
                    tracing::warn!("Re-match failed for {repo_name}: {e}");
                }
            }

            if self
                .image_import_cancel
                .load(std::sync::atomic::Ordering::Relaxed)
            {
                let mut guard = self.image_import_progress.write().expect("lock");
                if let Some(ref mut p) = *guard {
                    p.state = ImageImportState::Cancelled;
                    p.boxart_copied = total_boxart;
                    p.snap_copied = total_snap;
                    p.elapsed_secs = start.elapsed().as_secs();
                }
                break;
            }
        }

        // Put DB back.
        {
            let mut guard = self.metadata_db.lock().expect("metadata_db lock poisoned");
            *guard = Some(db);
        }

        // Update final progress (skip if already cancelled).
        {
            let mut guard = self.image_import_progress.write().expect("lock");
            let already_cancelled = guard
                .as_ref()
                .map(|p| p.state == ImageImportState::Cancelled)
                .unwrap_or(false);
            if !already_cancelled {
                *guard = Some(ImageImportProgress {
                    state: ImageImportState::Complete,
                    system: system.to_string(),
                    system_display,
                    processed: rom_filenames.len(),
                    total: rom_filenames.len(),
                    boxart_copied: total_boxart,
                    snap_copied: total_snap,
                    elapsed_secs: start.elapsed().as_secs(),
                    error: None,
                    current_system,
                    total_systems,
                });
            }
        }
    }
}
