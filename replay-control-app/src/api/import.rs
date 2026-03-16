use std::path::PathBuf;

use super::AppState;

impl AppState {
    /// Start a background metadata import from a LaunchBox XML file.
    /// Returns `false` if another metadata operation is already running.
    pub fn start_import(&self, xml_path: PathBuf) -> bool {
        use replay_control_core::metadata_db::{ImportProgress, ImportState};

        // Atomically claim the operation slot.
        if self
            .metadata_operation_in_progress
            .swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            return false;
        }

        // Check if already running (shouldn't happen with the atomic guard, but be safe).
        {
            let guard = self
                .import_progress
                .read()
                .expect("import_progress lock poisoned");
            if let Some(ref p) = *guard
                && matches!(
                    p.state,
                    ImportState::Downloading | ImportState::BuildingIndex | ImportState::Parsing
                )
            {
                self.metadata_operation_in_progress
                    .store(false, std::sync::atomic::Ordering::SeqCst);
                return false;
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
        if let Some(guard) = self.metadata_db()
            && let Some(db) = guard.as_ref()
        {
            db.clear().map_err(|e| e.to_string())?;
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
            if old_path.exists() {
                old_path
            } else {
                xml_path
            }
        };
        if !xml_path.exists() {
            return Err(format!(
                "No {LAUNCHBOX_XML} found. Place it in the .replay-control folder to enable re-import."
            ));
        }

        self.start_import(xml_path);
        Ok(())
    }

    /// Download LaunchBox Metadata.zip, extract, clear DB, and re-import.
    /// Runs entirely in a background thread. Returns false if another metadata
    /// operation is already running.
    pub fn start_metadata_download(&self) -> bool {
        use replay_control_core::metadata_db::{ImportProgress, ImportState};

        // Atomically claim the operation slot.
        if self
            .metadata_operation_in_progress
            .swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            return false;
        }

        // Check if already running (shouldn't happen with the atomic guard, but be safe).
        {
            let guard = self
                .import_progress
                .read()
                .expect("import_progress lock poisoned");
            if let Some(ref p) = *guard
                && matches!(
                    p.state,
                    ImportState::Downloading | ImportState::BuildingIndex | ImportState::Parsing
                )
            {
                self.metadata_operation_in_progress
                    .store(false, std::sync::atomic::Ordering::SeqCst);
                return false;
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
                    state
                        .metadata_operation_in_progress
                        .store(false, std::sync::atomic::Ordering::SeqCst);
                    return;
                }
            };

            // Clear existing metadata before re-import.
            if let Some(guard) = state.metadata_db()
                && let Some(db) = guard.as_ref()
                && let Err(e) = db.clear()
            {
                tracing::warn!("Failed to clear metadata DB before re-import: {e}");
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
                    self.metadata_operation_in_progress
                        .store(false, std::sync::atomic::Ordering::SeqCst);
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

        // Invalidate image cache so updated metadata paths are picked up.
        self.cache.invalidate_images();

        let succeeded = result.is_ok();

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

        // Import LaunchBox alternate names into game_alias table.
        if succeeded {
            self.import_launchbox_aliases(&xml_path);
        }

        // Re-enrich game library with freshly imported ratings.
        if succeeded {
            self.spawn_cache_enrichment();
        }

        // Clear progress after a short delay so SSE clients can read the
        // terminal state, then the slot becomes None for future operations.
        let progress_ref = self.import_progress.clone();
        let flag_ref = self.metadata_operation_in_progress.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(10));
            let mut guard = progress_ref.write().expect("lock");
            if guard
                .as_ref()
                .is_some_and(|p| matches!(p.state, ImportState::Complete | ImportState::Failed))
            {
                *guard = None;
            }
            flag_ref.store(false, std::sync::atomic::Ordering::SeqCst);
        });
    }

    /// Import LaunchBox `<GameAlternateName>` entries into the `game_alias` table.
    ///
    /// Parses alternate names from the XML, groups them by DatabaseID,
    /// then matches them to `game_library` entries via the LaunchBox `<Game>` entries.
    fn import_launchbox_aliases(&self, xml_path: &std::path::Path) {
        use replay_control_core::launchbox;

        // Parse alternate names from XML.
        let alt_names = match launchbox::parse_alternate_names(xml_path) {
            Ok(names) => names,
            Err(e) => {
                tracing::warn!("Failed to parse LaunchBox alternate names: {e}");
                return;
            }
        };

        if alt_names.is_empty() {
            return;
        }

        // Parse primary game names (DatabaseID -> Name) to include in alias groups.
        let game_names = launchbox::parse_game_names(xml_path).unwrap_or_default();

        // Group alternates by DatabaseID -> Vec<(name, region)>.
        // Include the primary game name so that alias groups contain ALL names for a game.
        let mut by_db_id: std::collections::HashMap<String, Vec<(String, String)>> =
            std::collections::HashMap::new();
        for alt in &alt_names {
            by_db_id
                .entry(alt.database_id.clone())
                .or_default()
                .push((alt.alternate_name.clone(), alt.region.clone()));
        }
        // Add primary game name to each group (with empty region).
        for (db_id, primary_name) in &game_names {
            by_db_id
                .entry(db_id.clone())
                .or_default()
                .push((primary_name.clone(), String::new()));
        }

        // We need to match DatabaseIDs to game_library base_titles.
        // The LaunchBox <Game> entries have DatabaseID and Name fields.
        // During import, we matched games by normalized title to rom_filename.
        // We need to re-parse the XML to build a DatabaseID -> Name mapping,
        // but that would be expensive. Instead, we'll use a simpler approach:
        // for each alternate name group, normalize the primary game name (from the
        // <Game> element with that DatabaseID) and find its base_title in game_library.
        //
        // Since we don't have a direct DatabaseID -> base_title mapping yet,
        // we'll match alternate names against game_library base_titles directly:
        // normalize each alternate name and check if it matches any base_title.
        // If it does, we add the mapping.
        //
        // This is a simplified approach that catches the most common cases.

        // Load all base_titles from game_library.
        let base_titles: std::collections::HashMap<String, Vec<String>> = {
            let guard = self.metadata_db.lock().expect("metadata_db lock");
            let db = match guard.as_ref() {
                Some(db) => db,
                None => return,
            };
            let systems = db.active_systems().unwrap_or_default();
            let mut map: std::collections::HashMap<String, Vec<String>> =
                std::collections::HashMap::new();
            for system in &systems {
                if let Ok(entries) = db.load_system_entries(system) {
                    for entry in entries {
                        if !entry.base_title.is_empty() {
                            map.entry(entry.base_title.clone())
                                .or_default()
                                .push(system.clone());
                        }
                    }
                }
            }
            map
        };

        // Build lookup maps for fuzzy matching (colon/dash normalization).
        use replay_control_core::title_utils::{fuzzy_match_key, resolve_to_library_title};

        let library_exact: std::collections::HashSet<&str> = base_titles
            .keys()
            .map(|s| s.as_str())
            .collect();

        let library_fuzzy: std::collections::HashMap<String, &str> = base_titles
            .keys()
            .map(|bt| (fuzzy_match_key(bt), bt.as_str()))
            .collect();

        // For each DatabaseID group, check if any alternate name resolves to a known base_title.
        // If it does, create alias entries linking the other alternates to that base_title.
        let mut aliases: Vec<(String, String, String, String, String)> = Vec::new();

        for alts in by_db_id.values() {
            // Find which alternate resolves to a library base_title.
            let mut matched_bt: Option<(String, String)> = None; // (base_title, system)
            for (alt_name, _) in alts {
                let resolved = resolve_to_library_title(alt_name, &library_exact, &library_fuzzy);
                if let Some(systems) = base_titles.get(&resolved) {
                    matched_bt = Some((resolved, systems[0].clone()));
                    break;
                }
            }

            if let Some((bt, system)) = matched_bt {
                // Insert all other alternates as aliases of this base_title.
                // resolve_to_library_title ensures alias_name uses the library's
                // actual base_title (with dashes) rather than the external name (with colons).
                for (alt_name, region) in alts {
                    let resolved = resolve_to_library_title(alt_name, &library_exact, &library_fuzzy);
                    if resolved != bt && !resolved.is_empty() {
                        aliases.push((
                            system.clone(),
                            bt.clone(),
                            resolved,
                            region.clone(),
                            "launchbox".to_string(),
                        ));
                    }
                }
            }
        }

        if aliases.is_empty() {
            tracing::debug!("LaunchBox aliases: no matches found");
            return;
        }

        let count = aliases.len();
        let mut guard = self.metadata_db.lock().expect("metadata_db lock");
        if let Some(db) = guard.as_mut() {
            match db.bulk_insert_aliases(&aliases) {
                Ok(n) => tracing::info!("LaunchBox aliases: {n}/{count} inserted"),
                Err(e) => tracing::warn!("LaunchBox aliases: insert failed: {e}"),
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
    /// Returns `false` if another metadata operation is already running.
    pub fn start_thumbnail_update(&self) -> bool {
        // Atomically claim the operation slot.
        if self
            .metadata_operation_in_progress
            .swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            return false;
        }

        if self.is_thumbnail_update_running() {
            self.metadata_operation_in_progress
                .store(false, std::sync::atomic::Ordering::SeqCst);
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
            None => match replay_control_core::metadata_db::MetadataDb::open(&storage_root) {
                Ok(db) => db,
                Err(e) => {
                    let mut guard = self.thumbnail_progress.write().expect("lock");
                    if let Some(ref mut p) = *guard {
                        p.phase = ThumbnailPhase::Failed;
                        p.error = Some(format!("Cannot open metadata DB: {e}"));
                        p.elapsed_secs = start.elapsed().as_secs();
                    }
                    self.metadata_operation_in_progress
                        .store(false, std::sync::atomic::Ordering::SeqCst);
                    return;
                }
            },
        };

        // ── Phase 1: Index refresh ──────────────────────────────────

        let progress_ref = self.thumbnail_progress.clone();
        let cancel_ref = &self.thumbnail_cancel;

        // Read GitHub API key from settings (if configured).
        let api_key = replay_control_core::settings::read_github_api_key(&storage_root);

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
            api_key.as_deref(),
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

                // If ALL repos failed (0 entries), report as Failed.
                if stats.total_entries == 0 && !stats.errors.is_empty() {
                    let is_rate_limited = stats
                        .errors
                        .iter()
                        .any(|e| e.contains("403") || e.contains("rate"));
                    let msg = if is_rate_limited {
                        format!(
                            "GitHub API rate limit exceeded ({}/{} repos failed). Wait ~1 hour and try again.",
                            stats.errors.len(),
                            stats.errors.len() + stats.repos_fetched,
                        )
                    } else {
                        format!(
                            "All repos failed to index ({} errors). First: {}",
                            stats.errors.len(),
                            stats
                                .errors
                                .first()
                                .map(|s| s.as_str())
                                .unwrap_or("unknown"),
                        )
                    };
                    let mut guard = self.thumbnail_progress.write().expect("lock");
                    if let Some(ref mut p) = *guard {
                        p.phase = ThumbnailPhase::Failed;
                        p.error = Some(msg);
                        p.elapsed_secs = start.elapsed().as_secs();
                    }
                    let mut guard = self.metadata_db.lock().expect("lock");
                    *guard = Some(db);
                    self.metadata_operation_in_progress
                        .store(false, std::sync::atomic::Ordering::SeqCst);
                    return;
                }

                // Update progress with index results.
                {
                    let mut guard = self.thumbnail_progress.write().expect("lock");
                    if let Some(ref mut p) = *guard {
                        p.entries_indexed = stats.total_entries;
                        p.elapsed_secs = start.elapsed().as_secs();
                    }
                }
                stats
            }
            Err(e) => {
                let mut guard = self.thumbnail_progress.write().expect("lock");
                if let Some(ref mut p) = *guard {
                    p.phase = ThumbnailPhase::Failed;
                    p.error = Some(format!("Index failed: {e}"));
                    p.elapsed_secs = start.elapsed().as_secs();
                }
                // Put DB back before returning.
                let mut guard = self.metadata_db.lock().expect("lock");
                *guard = Some(db);
                self.metadata_operation_in_progress
                    .store(false, std::sync::atomic::Ordering::SeqCst);
                return;
            }
        };

        // Check cancellation between phases.
        if cancel_ref.load(std::sync::atomic::Ordering::Relaxed) {
            let mut guard = self.thumbnail_progress.write().expect("lock");
            if let Some(ref mut p) = *guard {
                p.phase = ThumbnailPhase::Cancelled;
                p.elapsed_secs = start.elapsed().as_secs();
            }
            let mut guard = self.metadata_db.lock().expect("lock");
            *guard = Some(db);
            self.metadata_operation_in_progress
                .store(false, std::sync::atomic::Ordering::SeqCst);
            return;
        }

        // ── Phase 2: Download images ────────────────────────────────

        {
            let mut guard = self.thumbnail_progress.write().expect("lock");
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
                replay_control_core::thumbnails::thumbnail_repo_names(&s.folder_name).is_some()
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
                let mut guard = self.thumbnail_progress.write().expect("lock");
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
                        // Encode per-system progress in current_label (1-based for display).
                        if total > 0 {
                            let display_n = (processed + 1).min(total);
                            p.current_label = format!("{system_display}: {display_n}/{total}");
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
            // Use the ROM filenames to update game_metadata box_art_path / screenshot_path.
            Self::update_image_paths_from_disk(&mut db, &storage_root, system);
        }

        // Put DB back.
        {
            let mut guard = self.metadata_db.lock().expect("lock");
            *guard = Some(db);
        }

        // Invalidate the image cache so new thumbnails are picked up.
        self.cache.invalidate_images();

        let cancelled = cancel_ref.load(std::sync::atomic::Ordering::Relaxed);

        // Re-enrich game library with freshly downloaded box art.
        if !cancelled {
            self.spawn_cache_enrichment();
        }

        // Set final progress.
        {
            let mut guard = self.thumbnail_progress.write().expect("lock");
            if cancelled {
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
                    error: {
                        let mut parts = Vec::new();
                        if !index_stats.errors.is_empty() {
                            parts.push(format!(
                                "{} repos failed to index",
                                index_stats.errors.len()
                            ));
                        }
                        if total_failed > 0 {
                            parts.push(format!("{total_failed} images failed to download"));
                        }
                        if parts.is_empty() {
                            None
                        } else {
                            Some(parts.join("; "))
                        }
                    },
                });
            }
        }

        // Clear progress after a short delay so SSE clients can read the
        // terminal state, then the slot becomes None for future operations.
        let progress_ref = self.thumbnail_progress.clone();
        let flag_ref = self.metadata_operation_in_progress.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(10));
            let mut guard = progress_ref.write().expect("lock");
            if guard.as_ref().is_some_and(|p| {
                matches!(
                    p.phase,
                    ThumbnailPhase::Complete | ThumbnailPhase::Failed | ThumbnailPhase::Cancelled
                )
            }) {
                *guard = None;
            }
            flag_ref.store(false, std::sync::atomic::Ordering::SeqCst);
        });
    }

    /// Scan the media directory for a system and update game_metadata image paths.
    /// Uses fuzzy matching (base title + version-stripped) to bridge naming gaps
    /// between ROM filenames and libretro-thumbnails manifest names.
    fn update_image_paths_from_disk(
        db: &mut replay_control_core::metadata_db::MetadataDb,
        storage_root: &std::path::Path,
        system: &str,
    ) {
        use std::collections::HashMap;

        let rom_filenames = db.visible_filenames(system).unwrap_or_default();
        let media_base = storage_root
            .join(replay_control_core::storage::RC_DIR)
            .join("media")
            .join(system);

        type DirIndex = (HashMap<String, String>, HashMap<String, String>, HashMap<String, String>, HashMap<String, String>);

        /// Build a filename index for a media subdirectory (boxart or snap).
        /// Returns (exact, exact_ci, fuzzy, version) maps from normalized keys to relative paths.
        fn build_dir_index(
            dir: &std::path::Path,
            kind: &str,
        ) -> DirIndex {
            use replay_control_core::thumbnails::{base_title, strip_version};

            let mut exact = HashMap::new();
            let mut exact_ci = HashMap::new();
            let mut fuzzy = HashMap::new();
            let mut version = HashMap::new();

            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    let name_str = name.to_string_lossy();
                    if let Some(img_stem) = name_str.strip_suffix(".png") {
                        // Skip tiny files (fake symlinks / stubs).
                        let valid = entry
                            .metadata()
                            .map(|m| m.len() >= 200)
                            .unwrap_or(false);
                        if !valid {
                            continue;
                        }
                        let path = format!("{kind}/{name_str}");
                        exact.insert(img_stem.to_string(), path.clone());
                        exact_ci.entry(img_stem.to_lowercase()).or_insert_with(|| path.clone());
                        let bt = base_title(img_stem);
                        let vs = strip_version(&bt).to_string();
                        fuzzy.entry(bt.clone()).or_insert_with(|| path.clone());
                        if vs.len() < bt.len() {
                            version.entry(vs).or_insert(path);
                        }
                    }
                }
            }
            (exact, exact_ci, fuzzy, version)
        }

        /// Look up a ROM in the directory index using the same fuzzy tiers
        /// as resolve_box_art / find_image_on_disk.
        /// For arcade systems, `arcade_display` provides the translated display name.
        fn find_match(
            rom_filename: &str,
            arcade_display: Option<&str>,
            exact: &HashMap<String, String>,
            exact_ci: &HashMap<String, String>,
            fuzzy: &HashMap<String, String>,
            version: &HashMap<String, String>,
        ) -> Option<String> {
            use replay_control_core::thumbnails::{base_title, strip_version, thumbnail_filename};

            let stem = rom_filename
                .rfind('.')
                .map(|i| &rom_filename[..i])
                .unwrap_or(rom_filename);
            let stem = replay_control_core::title_utils::strip_n64dd_prefix(stem);
            let source = arcade_display.unwrap_or(stem);
            let thumb_name = thumbnail_filename(source);

            // Tier 1: exact
            if let Some(path) = exact.get(&thumb_name) {
                return Some(path.clone());
            }

            // Colon variants for arcade games.
            if source.contains(':') {
                let dash = thumbnail_filename(&source.replace(": ", " - ").replace(':', " -"));
                if let Some(path) = exact.get(&dash) {
                    return Some(path.clone());
                }
                let drop = thumbnail_filename(&source.replace(": ", " ").replace(':', ""));
                if let Some(path) = exact.get(&drop) {
                    return Some(path.clone());
                }
            }

            // Tier 1b: case-insensitive exact (preserves region tags)
            if let Some(path) = exact_ci.get(&thumb_name.to_lowercase()) {
                return Some(path.clone());
            }

            // Tier 2: base title (strip tags)
            let base = base_title(&thumb_name);
            if let Some(path) = fuzzy.get(&base) {
                return Some(path.clone());
            }

            // Tier 3: version-stripped
            let vs = strip_version(&base);
            if vs.len() < base.len()
                && let Some(path) = fuzzy.get(vs).or_else(|| version.get(vs))
            {
                return Some(path.clone());
            }

            None
        }

        let boxart_dir = media_base.join("boxart");
        let snap_dir = media_base.join("snap");

        let (box_exact, box_exact_ci, box_fuzzy, box_version) = build_dir_index(&boxart_dir, "boxart");
        let (snap_exact, snap_exact_ci, snap_fuzzy, snap_version) = build_dir_index(&snap_dir, "snap");

        let is_arcade = matches!(
            system,
            "arcade_mame" | "arcade_fbneo" | "arcade_mame_2k3p" | "arcade_dc"
        );

        let mut updates: Vec<(String, String, Option<String>, Option<String>)> = Vec::new();

        for rom_filename in &rom_filenames {
            let arcade_display = if is_arcade {
                let stem = rom_filename
                    .rfind('.')
                    .map(|i| &rom_filename[..i])
                    .unwrap_or(rom_filename);
                replay_control_core::arcade_db::lookup_arcade_game(stem)
                    .map(|info| info.display_name)
            } else {
                None
            };
            let boxart_rel = find_match(rom_filename, arcade_display, &box_exact, &box_exact_ci, &box_fuzzy, &box_version);
            let snap_rel = find_match(rom_filename, arcade_display, &snap_exact, &snap_exact_ci, &snap_fuzzy, &snap_version);

            if boxart_rel.is_some() || snap_rel.is_some() {
                updates.push((
                    system.to_string(),
                    rom_filename.clone(),
                    boxart_rel,
                    snap_rel,
                ));
            }
        }

        if !updates.is_empty()
            && let Err(e) = db.bulk_update_image_paths(&updates)
        {
            tracing::warn!("Failed to update image paths for {system}: {e}");
        }
    }
}
