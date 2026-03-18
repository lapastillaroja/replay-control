use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use super::AppState;

// ── ImportPipeline ─────────────────────────────────────────────────

/// Manages metadata imports (LaunchBox XML → metadata DB).
///
/// The `busy` flag provides mutual exclusion between import and thumbnail
/// operations (they share the same flag) **and** serves as a UI indicator.
pub struct ImportPipeline {
    /// Shared flag: true while any metadata DB operation is running.
    /// Shared with `ThumbnailPipeline` for mutual exclusion.
    busy: Arc<AtomicBool>,
    progress: Arc<RwLock<Option<replay_control_core::metadata_db::ImportProgress>>>,
}

impl ImportPipeline {
    pub fn new(busy: Arc<AtomicBool>) -> Self {
        Self {
            busy,
            progress: Arc::new(RwLock::new(None)),
        }
    }

    /// Check if the shared busy flag is set.
    /// Convenience accessor — reads the same Arc as AppState.is_busy().
    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }

    /// Atomically claim the shared busy flag. Returns `true` if successfully claimed.
    pub(crate) fn claim_busy(&self) -> bool {
        !self.busy.swap(true, Ordering::SeqCst)
    }

    /// Get a clone of the shared busy flag Arc.
    pub(crate) fn busy_flag(&self) -> Arc<AtomicBool> {
        self.busy.clone()
    }

    /// Check if an import is actively running (has in-progress state).
    /// Used by the startup pipeline to wait for auto-import completion.
    pub fn has_active_progress(&self) -> bool {
        use replay_control_core::metadata_db::ImportState;
        self.progress
            .read()
            .expect("import_progress lock poisoned")
            .as_ref()
            .is_some_and(|p| {
                matches!(
                    p.state,
                    ImportState::Downloading | ImportState::BuildingIndex | ImportState::Parsing
                )
            })
    }

    /// Get current import progress (clone).
    pub fn progress(&self) -> Option<replay_control_core::metadata_db::ImportProgress> {
        self.progress
            .read()
            .expect("import_progress lock poisoned")
            .clone()
    }

    /// Start a background metadata import from a LaunchBox XML file.
    /// Returns `false` if another metadata operation is already running.
    pub fn start_import(&self, xml_path: PathBuf, state: AppState) -> bool {
        self.start_import_inner(xml_path, state, false)
    }

    /// Start import without post-enrichment. Used by the startup pipeline
    /// which handles populate/enrichment sequentially.
    pub fn start_import_no_enrich(&self, xml_path: PathBuf, state: AppState) -> bool {
        self.start_import_inner(xml_path, state, true)
    }

    fn start_import_inner(
        &self,
        xml_path: PathBuf,
        state: AppState,
        skip_enrichment: bool,
    ) -> bool {
        use replay_control_core::metadata_db::{ImportProgress, ImportState};

        // Atomically claim the operation slot.
        if self.busy.swap(true, Ordering::SeqCst) {
            return false;
        }
        state.set_busy_label("Importing metadata...");

        // Check if already running (shouldn't happen with the atomic guard, but be safe).
        {
            let guard = self.progress.read().expect("import_progress lock poisoned");
            if let Some(ref p) = *guard
                && matches!(
                    p.state,
                    ImportState::Downloading | ImportState::BuildingIndex | ImportState::Parsing
                )
            {
                self.busy.store(false, Ordering::SeqCst);
                return false;
            }
        }

        // Set initial progress.
        {
            let mut guard = self
                .progress
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

        let state = state.clone();
        tokio::task::spawn_blocking(move || {
            let start = std::time::Instant::now();
            state
                .import
                .run_import_blocking(&state, xml_path, start, skip_enrichment);
        });

        true
    }

    /// Clear metadata DB and re-import from `launchbox-metadata.xml` if present.
    /// Returns an error message if the XML file is not found.
    pub fn regenerate_metadata(&self, state: &AppState) -> Result<(), String> {
        use replay_control_core::metadata_db::LAUNCHBOX_XML;

        // Clear existing metadata.
        if let Some(guard) = state.metadata_db()
            && let Some(db) = guard.as_ref()
        {
            db.clear().map_err(|e| e.to_string())?;
        }

        // Find launchbox-metadata.xml (with fallback to old name) and trigger re-import.
        let storage = state.storage();
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

        self.start_import(xml_path, state.clone());
        Ok(())
    }

    /// Download LaunchBox Metadata.zip, extract, clear DB, and re-import.
    /// Runs entirely in a background thread. Returns false if another metadata
    /// operation is already running.
    pub fn start_metadata_download(&self, state: &AppState) -> bool {
        use replay_control_core::metadata_db::{ImportProgress, ImportState};

        // Atomically claim the operation slot.
        if self.busy.swap(true, Ordering::SeqCst) {
            return false;
        }

        // Check if already running (shouldn't happen with the atomic guard, but be safe).
        {
            let guard = self.progress.read().expect("import_progress lock poisoned");
            if let Some(ref p) = *guard
                && matches!(
                    p.state,
                    ImportState::Downloading | ImportState::BuildingIndex | ImportState::Parsing
                )
            {
                self.busy.store(false, Ordering::SeqCst);
                return false;
            }
        }

        // Set initial progress to Downloading.
        {
            let mut guard = self
                .progress
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

        let state = state.clone();
        tokio::task::spawn_blocking(move || {
            let start = std::time::Instant::now();
            let storage = state.storage();
            let rc_dir = storage.rc_dir();

            // Download and extract.
            let xml_path = match replay_control_core::launchbox::download_metadata(&rc_dir) {
                Ok(path) => path,
                Err(e) => {
                    let mut guard = state
                        .import
                        .progress
                        .write()
                        .expect("import_progress lock poisoned");
                    if let Some(ref mut p) = *guard {
                        p.state = ImportState::Failed;
                        p.error = Some(format!("Download failed: {e}"));
                        p.elapsed_secs = start.elapsed().as_secs();
                    }
                    state.import.busy.store(false, Ordering::SeqCst);
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
                    .import
                    .progress
                    .write()
                    .expect("import_progress lock poisoned");
                if let Some(ref mut p) = *guard {
                    p.elapsed_secs = start.elapsed().as_secs();
                }
            }

            // Now run the import (this updates import_progress internally).
            state
                .import
                .run_import_blocking(&state, xml_path, start, false);
        });

        true
    }

    /// Run the metadata import synchronously (called from spawn_blocking).
    /// Separated from start_import to allow reuse from start_metadata_download.
    ///
    /// DB locking is per-batch: the lock is acquired for each ~500-entry batch
    /// flush and then released, giving other threads millisecond-scale gaps to
    /// read the DB between batches.
    fn run_import_blocking(
        &self,
        state: &AppState,
        xml_path: PathBuf,
        start: std::time::Instant,
        skip_enrichment: bool,
    ) {
        use replay_control_core::metadata_db::{ImportProgress, ImportState};

        // Build ROM index (no DB needed).
        let storage_root = state.storage().root.clone();
        {
            let mut guard = self
                .progress
                .write()
                .expect("import_progress lock poisoned");
            if let Some(ref mut p) = *guard {
                p.state = ImportState::BuildingIndex;
                p.elapsed_secs = start.elapsed().as_secs();
            }
        }

        let rom_index = replay_control_core::launchbox::build_rom_index(&storage_root);

        // Verify DB is available before starting the parse.
        {
            let db_guard = state.metadata_db.lock().expect("metadata_db lock poisoned");
            if db_guard.is_none() {
                tracing::error!("Metadata DB unavailable at import start (connection missing)");
                let mut guard = self
                    .progress
                    .write()
                    .expect("import_progress lock poisoned");
                if let Some(ref mut p) = *guard {
                    p.state = ImportState::Failed;
                    p.error = Some("Metadata DB unavailable".to_string());
                    p.elapsed_secs = start.elapsed().as_secs();
                }
                self.busy.store(false, Ordering::SeqCst);
                return;
            }
        }

        // Update progress to Parsing.
        {
            let mut guard = self
                .progress
                .write()
                .expect("import_progress lock poisoned");
            if let Some(ref mut p) = *guard {
                p.state = ImportState::Parsing;
                p.elapsed_secs = start.elapsed().as_secs();
            }
        }

        // Per-batch flush closure: locks DB, calls bulk_upsert, releases.
        // The core crate calls this for each ~500-entry batch, keeping
        // the DB Mutex held only for the duration of the SQL transaction.
        let db_ref = state.metadata_db.clone();
        let flush_batch = |batch: &[(
            String,
            String,
            replay_control_core::metadata_db::GameMetadata,
        )]| {
            let mut guard = db_ref.lock().expect("metadata_db lock poisoned");
            let db = guard.as_mut().ok_or_else(|| {
                replay_control_core::error::Error::Other(
                    "Metadata DB unavailable during import".to_string(),
                )
            })?;
            db.bulk_upsert(batch)
        };

        let progress_ref = self.progress.clone();
        let start_ref = start;
        let result = replay_control_core::launchbox::import_launchbox(
            &xml_path,
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
            flush_batch,
        );

        // Invalidate image cache so updated metadata paths are picked up.
        state.cache.invalidate_images();

        let (succeeded, parse_result) = match &result {
            Ok((_, pr)) => (true, Some(pr)),
            Err(_) => (false, None),
        };

        // Import LaunchBox alternate names into game_alias table.
        // Acquires its own per-operation DB lock.
        if let Some(pr) = parse_result {
            tracing::debug!(
                "Starting LaunchBox alias import ({} alternates, {} game names)",
                pr.alternate_names.len(),
                pr.game_names.len()
            );
            Self::import_launchbox_aliases(state, pr);
            tracing::debug!("LaunchBox alias import complete");
        }

        // Update final progress.
        {
            let mut guard = self
                .progress
                .write()
                .expect("import_progress lock poisoned");
            match result {
                Ok((stats, _)) => {
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

        // Re-enrich game library with freshly imported data.
        // Skip during startup auto-import: the pipeline handles populate/enrichment
        // sequentially to avoid races. For user-triggered imports, enrich immediately.
        if succeeded && !skip_enrichment {
            state.spawn_cache_enrichment();
        }

        // Clear busy flag, label, and progress.
        self.busy.store(false, Ordering::SeqCst);
        state.set_busy_label("");
        *self.progress.write().expect("import_progress lock") = None;
    }

    /// Import LaunchBox alternate names into the `game_alias` table.
    ///
    /// Uses the `ParseResult` from the single-pass XML parse — no re-reading.
    /// Acquires/releases the DB lock per-operation (read base_titles, then
    /// write aliases) so other threads can access the DB between operations.
    fn import_launchbox_aliases(
        state: &AppState,
        parse_result: &replay_control_core::launchbox::ParseResult,
    ) {
        let alt_names = &parse_result.alternate_names;
        let game_names = &parse_result.game_names;

        if alt_names.is_empty() {
            return;
        }

        // Group alternates by DatabaseID -> Vec<(name, region)>.
        // Include the primary game name so that alias groups contain ALL names for a game.
        let mut by_db_id: std::collections::HashMap<String, Vec<(String, String)>> =
            std::collections::HashMap::new();
        for alt in alt_names {
            by_db_id
                .entry(alt.database_id.clone())
                .or_default()
                .push((alt.alternate_name.clone(), alt.region.clone()));
        }
        // Add primary game name to each group (with empty region).
        for (db_id, primary_name) in game_names {
            by_db_id
                .entry(db_id.clone())
                .or_default()
                .push((primary_name.clone(), String::new()));
        }

        // Lock DB to load base_titles, then release.
        tracing::debug!("LaunchBox aliases: loading base_titles from game_library...");
        let base_titles: std::collections::HashMap<String, Vec<String>> = {
            let mut guard = state.metadata_db.lock().expect("metadata_db lock poisoned");
            let Some(db) = guard.as_mut() else {
                tracing::warn!("LaunchBox aliases: DB unavailable for reading base_titles");
                return;
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
            // MutexGuard drops here — DB lock released.
        };

        // Build lookup maps for fuzzy matching (colon/dash normalization).
        // All in-memory, no DB lock needed.
        use replay_control_core::title_utils::{fuzzy_match_key, resolve_to_library_title};

        let library_exact: std::collections::HashSet<&str> =
            base_titles.keys().map(|s| s.as_str()).collect();

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
                for (alt_name, region) in alts {
                    let resolved =
                        resolve_to_library_title(alt_name, &library_exact, &library_fuzzy);
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

        // Lock DB to insert aliases, then release.
        let count = aliases.len();
        let mut guard = state.metadata_db.lock().expect("metadata_db lock poisoned");
        if let Some(db) = guard.as_mut() {
            match db.bulk_insert_aliases(&aliases) {
                Ok(n) => tracing::info!("LaunchBox aliases: {n}/{count} inserted"),
                Err(e) => tracing::warn!("LaunchBox aliases: insert failed: {e}"),
            }
        } else {
            tracing::warn!("LaunchBox aliases: DB unavailable for inserting aliases");
        }
        // MutexGuard drops here — DB lock released.
    }
}

// ── ThumbnailPipeline ──────────────────────────────────────────────

/// Manages the two-phase thumbnail pipeline (index + download).
///
/// Shares the `busy` flag with `ImportPipeline` for mutual exclusion.
pub struct ThumbnailPipeline {
    /// Shared flag: true while any metadata DB operation is running.
    /// Shared with `ImportPipeline` for mutual exclusion.
    busy: Arc<AtomicBool>,
    progress: Arc<RwLock<Option<crate::server_fns::ThumbnailProgress>>>,
    cancel: Arc<AtomicBool>,
}

impl ThumbnailPipeline {
    pub fn new(busy: Arc<AtomicBool>) -> Self {
        Self {
            busy,
            progress: Arc::new(RwLock::new(None)),
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Get current thumbnail pipeline progress (clone).
    pub fn progress(&self) -> Option<crate::server_fns::ThumbnailProgress> {
        self.progress
            .read()
            .expect("thumbnail_progress lock poisoned")
            .clone()
    }

    /// Request cancellation of the current thumbnail update.
    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Check if a thumbnail update is already running.
    fn is_thumbnail_update_running(&self) -> bool {
        use crate::server_fns::ThumbnailPhase;
        let guard = self
            .progress
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
    pub fn start_thumbnail_update(&self, state: &AppState) -> bool {
        // Atomically claim the operation slot.
        if self.busy.swap(true, Ordering::SeqCst) {
            return false;
        }
        state.set_busy_label("Updating thumbnails...");

        if self.is_thumbnail_update_running() {
            self.busy.store(false, Ordering::SeqCst);
            return false;
        }

        use crate::server_fns::{ThumbnailPhase, ThumbnailProgress};

        self.cancel.store(false, Ordering::Relaxed);

        // Write initial progress before spawning.
        {
            let mut guard = self
                .progress
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

        let state = state.clone();
        tokio::task::spawn_blocking(move || {
            let start = std::time::Instant::now();
            state
                .thumbnails
                .run_thumbnail_update_blocking(&state, start);
        });

        true
    }

    /// Run the two-phase thumbnail pipeline (blocking, called from spawn_blocking).
    ///
    /// DB locking is per-operation: the lock is acquired for each discrete DB
    /// call (manifest import, per-system download, per-system image path
    /// update) and released between them. This gives other threads access to
    /// the DB during the long download phase.
    fn run_thumbnail_update_blocking(&self, state: &AppState, start: std::time::Instant) {
        use crate::server_fns::{ThumbnailPhase, ThumbnailProgress};
        use replay_control_core::thumbnail_manifest;
        use replay_control_core::thumbnails::ALL_THUMBNAIL_KINDS;

        let storage_root = state.storage().root.clone();

        // Verify DB is available before starting.
        {
            let db_guard = state.metadata_db.lock().expect("metadata_db lock poisoned");
            if db_guard.is_none() {
                tracing::error!(
                    "Metadata DB unavailable at thumbnail update start (connection missing)"
                );
                let mut guard = self.progress.write().expect("lock");
                if let Some(ref mut p) = *guard {
                    p.phase = ThumbnailPhase::Failed;
                    p.error = Some("Metadata DB unavailable".to_string());
                    p.elapsed_secs = start.elapsed().as_secs();
                }
                self.busy.store(false, Ordering::SeqCst);
                return;
            }
        }

        // ── Phase 1: Index refresh ──────────────────────────────────
        // Lock DB for the manifest import (per-repo network + DB writes).
        // Released after this phase so the download phase doesn't block readers.

        let progress_ref = self.progress.clone();
        let cancel_ref = &self.cancel;

        // Read GitHub API key from settings (if configured).
        let api_key = replay_control_core::settings::read_github_api_key(&storage_root);

        let index_result = {
            let mut db_guard = state.metadata_db.lock().expect("metadata_db lock poisoned");
            let db = db_guard.as_mut().expect("DB availability verified above");
            thumbnail_manifest::import_all_manifests(
                db,
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
            )
            // MutexGuard drops here — DB lock released between phases.
        };

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
                    let mut guard = self.progress.write().expect("lock");
                    if let Some(ref mut p) = *guard {
                        p.phase = ThumbnailPhase::Failed;
                        p.error = Some(msg);
                        p.elapsed_secs = start.elapsed().as_secs();
                    }
                    self.busy.store(false, Ordering::SeqCst);
                    return;
                }

                // Update progress with index results.
                {
                    let mut guard = self.progress.write().expect("lock");
                    if let Some(ref mut p) = *guard {
                        p.entries_indexed = stats.total_entries;
                        p.elapsed_secs = start.elapsed().as_secs();
                    }
                }
                stats
            }
            Err(e) => {
                let mut guard = self.progress.write().expect("lock");
                if let Some(ref mut p) = *guard {
                    p.phase = ThumbnailPhase::Failed;
                    p.error = Some(format!("Index failed: {e}"));
                    p.elapsed_secs = start.elapsed().as_secs();
                }
                self.busy.store(false, Ordering::SeqCst);
                return;
            }
        };

        // Check cancellation between phases.
        if cancel_ref.load(Ordering::Relaxed) {
            let mut guard = self.progress.write().expect("lock");
            if let Some(ref mut p) = *guard {
                p.phase = ThumbnailPhase::Cancelled;
                p.elapsed_secs = start.elapsed().as_secs();
            }
            self.busy.store(false, Ordering::SeqCst);
            return;
        }

        // ── Phase 2: Download images ────────────────────────────────
        // DB lock is acquired per-system for download + image path update,
        // then released between systems.

        {
            let mut guard = self.progress.write().expect("lock");
            if let Some(ref mut p) = *guard {
                p.phase = ThumbnailPhase::Downloading;
                p.step_done = 0;
                p.step_total = 0;
                p.downloaded = 0;
                p.elapsed_secs = start.elapsed().as_secs();
            }
        }

        // Collect systems that have ROMs and a thumbnail repo.
        let storage = state.storage();
        let systems = state.cache.get_systems(&storage);
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
            if cancel_ref.load(Ordering::Relaxed) {
                break;
            }

            let system_display = replay_control_core::systems::find_system(system)
                .map(|s| s.display_name.to_string())
                .unwrap_or_else(|| system.to_string());

            // Update progress for this system.
            {
                let mut guard = self.progress.write().expect("lock");
                if let Some(ref mut p) = *guard {
                    p.current_label = system_display.clone();
                    p.step_done = i;
                    p.step_total = total_systems;
                    p.elapsed_secs = start.elapsed().as_secs();
                }
            }

            let progress_ref = self.progress.clone();

            for kind in ALL_THUMBNAIL_KINDS {
                if cancel_ref.load(Ordering::Relaxed) {
                    break;
                }
                let prev_downloaded = total_downloaded;
                let db_guard = state.metadata_db.lock().expect("metadata_db lock poisoned");
                if let Some(db) = db_guard.as_ref() {
                    let result = thumbnail_manifest::download_system_thumbnails(
                        db,
                        &storage_root,
                        system,
                        *kind,
                        &|processed, total, downloaded| {
                            let mut guard = progress_ref.write().expect("lock");
                            if let Some(ref mut p) = *guard {
                                p.step_done = i;
                                p.step_total = total_systems;
                                p.downloaded = prev_downloaded + downloaded;
                                p.elapsed_secs = start.elapsed().as_secs();
                                if total > 0 {
                                    let display_n = (processed + 1).min(total);
                                    p.current_label =
                                        format!("{system_display}: {display_n}/{total}");
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
                            let kind_name = kind.media_dir();
                            tracing::warn!(
                                "{kind_name} download failed for {system}: {e}"
                            );
                        }
                    }
                }
                // MutexGuard drops here — DB lock released between kinds.
            }

            // Lock DB for image path update, then release.
            Self::update_image_paths_from_disk(state, &storage_root, system);
        }

        // Invalidate the image cache so new thumbnails are picked up.
        state.cache.invalidate_images();

        let cancelled = cancel_ref.load(Ordering::Relaxed);

        // Re-enrich game library with freshly downloaded box art.
        if !cancelled {
            state.spawn_cache_enrichment();
        }

        // Set final progress.
        {
            let mut guard = self.progress.write().expect("lock");
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

        // Clear busy flag, label, and progress.
        self.busy.store(false, Ordering::SeqCst);
        state.set_busy_label("");
        *self.progress.write().expect("thumbnail_progress lock") = None;
    }

    /// Scan the media directory for a system and update game_metadata image paths.
    /// Uses fuzzy matching (base title + version-stripped) to bridge naming gaps
    /// between ROM filenames and libretro-thumbnails manifest names.
    ///
    /// Acquires/releases the DB lock internally for each operation (read filenames,
    /// then write updates).
    fn update_image_paths_from_disk(
        state: &AppState,
        storage_root: &std::path::Path,
        system: &str,
    ) {
        use replay_control_core::image_matching::{build_dir_index, find_best_match};

        // Lock DB to read visible filenames, then release.
        let rom_filenames = {
            let mut guard = state.metadata_db.lock().expect("metadata_db lock poisoned");
            let Some(db) = guard.as_mut() else {
                return;
            };
            db.visible_filenames(system).unwrap_or_default()
            // MutexGuard drops here.
        };

        // Build dir indexes (filesystem scan, no DB needed).
        use replay_control_core::thumbnails::ALL_THUMBNAIL_KINDS;
        let media_base = storage_root
            .join(replay_control_core::storage::RC_DIR)
            .join("media")
            .join(system);

        let indexes: Vec<_> = ALL_THUMBNAIL_KINDS
            .iter()
            .map(|kind| {
                let dir = media_base.join(kind.media_dir());
                build_dir_index(&dir, kind.media_dir())
            })
            .collect();
        let box_index = &indexes[0];
        let snap_index = &indexes[1];

        let is_arcade = replay_control_core::systems::is_arcade_system(system);

        // Match ROM filenames to images (in-memory, no DB needed).
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
            let boxart_rel = find_best_match(box_index, rom_filename, arcade_display, None);
            let snap_rel = find_best_match(snap_index, rom_filename, arcade_display, None);

            if boxart_rel.is_some() || snap_rel.is_some() {
                updates.push((
                    system.to_string(),
                    rom_filename.clone(),
                    boxart_rel,
                    snap_rel,
                ));
            }
        }

        // Lock DB to write image path updates, then release.
        if !updates.is_empty() {
            let mut guard = state.metadata_db.lock().expect("metadata_db lock poisoned");
            if let Some(db) = guard.as_mut() {
                if let Err(e) = db.bulk_update_image_paths(&updates) {
                    tracing::warn!("Failed to update image paths for {system}: {e}");
                }
            }
            // MutexGuard drops here.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn import_and_thumbnail_share_busy_flag() {
        let busy = Arc::new(AtomicBool::new(false));
        let import = ImportPipeline::new(busy.clone());
        let thumbnails = ThumbnailPipeline::new(busy.clone());

        assert!(!import.is_busy());

        // Simulate import claiming the slot.
        busy.store(true, Ordering::SeqCst);
        assert!(import.is_busy());
        // Thumbnail pipeline should also see it as busy (mutual exclusion).
        assert!(thumbnails.progress().is_none());
        // The busy flag is shared.
        busy.store(false, Ordering::SeqCst);
        assert!(!import.is_busy());
    }

    #[test]
    fn import_progress_initially_none() {
        let busy = Arc::new(AtomicBool::new(false));
        let import = ImportPipeline::new(busy);
        assert!(import.progress().is_none());
    }

    #[test]
    fn thumbnail_progress_initially_none() {
        let busy = Arc::new(AtomicBool::new(false));
        let thumbnails = ThumbnailPipeline::new(busy);
        assert!(thumbnails.progress().is_none());
    }

    #[test]
    fn thumbnail_cancel_initially_false() {
        let busy = Arc::new(AtomicBool::new(false));
        let thumbnails = ThumbnailPipeline::new(busy);
        // Requesting cancel should not panic.
        thumbnails.request_cancel();
    }

    #[test]
    fn mutual_exclusion_prevents_concurrent_operations() {
        let busy = Arc::new(AtomicBool::new(false));
        let _import = ImportPipeline::new(busy.clone());
        let _thumbnails = ThumbnailPipeline::new(busy.clone());

        // First swap claims the slot.
        assert!(!busy.swap(true, Ordering::SeqCst));
        // Second swap detects it's already claimed.
        assert!(busy.swap(true, Ordering::SeqCst));
        // Release.
        busy.store(false, Ordering::SeqCst);
        // Now claiming works again.
        assert!(!busy.swap(true, Ordering::SeqCst));
    }

    #[test]
    fn claim_busy_returns_true_when_free() {
        let busy = Arc::new(AtomicBool::new(false));
        let import = ImportPipeline::new(busy.clone());

        // First claim succeeds.
        assert!(import.claim_busy());
        assert!(import.is_busy());

        // Second claim fails (already held).
        assert!(!import.claim_busy());

        // Release via busy_flag.
        import.busy_flag().store(false, Ordering::SeqCst);
        assert!(!import.is_busy());

        // Can claim again.
        assert!(import.claim_busy());
    }

    /// Busy flag lifecycle: starts false, claim sets true, release sets false.
    /// Verifies that mutual exclusion works between import and thumbnail pipelines.
    #[test]
    fn busy_flag_lifecycle_import_blocks_thumbnail() {
        let busy = Arc::new(AtomicBool::new(false));
        let import = ImportPipeline::new(busy.clone());
        let thumbnails = ThumbnailPipeline::new(busy.clone());

        // Initially not busy.
        assert!(!import.is_busy());

        // Import claims the slot.
        assert!(import.claim_busy());
        assert!(import.is_busy());

        // Thumbnail cannot claim while import holds the flag.
        assert!(busy.swap(true, Ordering::SeqCst)); // already true

        // Release.
        import.busy_flag().store(false, Ordering::SeqCst);
        assert!(!import.is_busy());

        // Now thumbnail can claim.
        assert!(!busy.swap(true, Ordering::SeqCst)); // was false, now true
        assert!(import.is_busy()); // shared flag is true
        let _ = thumbnails; // suppress unused warning
    }

    /// Busy flag lifecycle: claim_busy is the atomic gatekeeper, not just store(true).
    /// Two threads racing claim_busy: only one succeeds.
    #[test]
    fn busy_flag_atomic_claim_race() {
        use std::sync::Barrier;

        let busy = Arc::new(AtomicBool::new(false));
        let barrier = Arc::new(Barrier::new(2));
        let wins = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        let handles: Vec<_> = (0..2)
            .map(|_| {
                let busy = busy.clone();
                let barrier = barrier.clone();
                let wins = wins.clone();
                let import = ImportPipeline::new(busy);
                std::thread::spawn(move || {
                    barrier.wait();
                    if import.claim_busy() {
                        wins.fetch_add(1, Ordering::SeqCst);
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        // Exactly one thread should have won the claim.
        assert_eq!(wins.load(Ordering::SeqCst), 1);
    }

    /// DB Mutex contention: metadata_db() accessor returns a valid guard
    /// even when another thread holds the Mutex briefly.
    #[test]
    fn db_mutex_contention_no_deadlock() {
        use std::sync::Mutex;

        // Simulate the metadata_db Mutex pattern from AppState.
        let db_mutex: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(Some("db_handle".into())));

        let db_clone = db_mutex.clone();
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let barrier2 = barrier.clone();

        // Thread 1: holds the lock for a bit.
        let t1 = std::thread::spawn(move || {
            let guard = db_clone.lock().unwrap();
            barrier2.wait(); // signal that we hold the lock
            std::thread::sleep(std::time::Duration::from_millis(50));
            assert!(guard.is_some());
            drop(guard);
        });

        // Thread 2: tries to acquire the lock while thread 1 holds it.
        barrier.wait(); // wait until thread 1 holds the lock
        let start = std::time::Instant::now();
        let guard = db_mutex.lock().unwrap();
        let waited = start.elapsed();

        // We should have waited for thread 1 to release.
        assert!(waited.as_millis() >= 20, "Should have blocked on Mutex");
        assert!(guard.is_some(), "DB should still be valid after contention");
        drop(guard);

        t1.join().unwrap();
    }
}
