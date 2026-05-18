use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use replay_control_core::error::{Error, Result};
use replay_control_core_server::rom_hash::{CachedHash, HashResult, HashStats};
use replay_control_core_server::roms::RomEntry;
use replay_control_core_server::storage::StorageLocation;

use replay_control_core_server::library_db::{
    DISCOVERY_SAVE_CHUNK_ROWS, DiscoveryFinalizeStats, LibraryDb,
};

use super::{LibraryService, dir_mtime_secs};
use crate::api::db_pools::{LIBRARY_MAINTENANCE_WRITE_TIMEOUT, LibraryWritePool};

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ScanOptions {
    pub force_rehash: bool,
    pub skip_unchanged_startup: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ScanInputs {
    cached_hashes: HashMap<String, CachedHash>,
    clean_startup_fingerprint: Option<String>,
    mtime_probe_trustworthy: bool,
    options: ScanOptions,
    /// None is valid for unit tests and in-process harnesses that do not need
    /// storage-swap cancellation.
    cancellation: Option<ScanCancellation>,
}

#[derive(Debug, Clone)]
pub(crate) struct ScanCancellation {
    expected_generation: u64,
    current_generation: Arc<AtomicU64>,
}

impl ScanCancellation {
    pub(crate) fn new(current_generation: Arc<AtomicU64>, expected_generation: u64) -> Self {
        Self {
            expected_generation,
            current_generation,
        }
    }

    pub(crate) fn ensure_current(&self) -> Result<()> {
        if self.current_generation.load(Ordering::Relaxed) != self.expected_generation {
            return Err(Error::StorageChanged);
        }
        Ok(())
    }
}

impl ScanInputs {
    pub(crate) fn new(
        cached_hashes: HashMap<String, CachedHash>,
        clean_startup_fingerprint: Option<String>,
        mtime_probe_trustworthy: bool,
        options: ScanOptions,
        cancellation: Option<ScanCancellation>,
    ) -> Self {
        Self {
            cached_hashes,
            clean_startup_fingerprint,
            mtime_probe_trustworthy,
            options,
            cancellation,
        }
    }

    pub(crate) fn cancellation(&self) -> Option<&ScanCancellation> {
        self.cancellation.as_ref()
    }

    pub(crate) fn force_rehash(&self) -> bool {
        self.options.force_rehash
    }

    pub(crate) fn startup_skip_enabled(&self) -> bool {
        self.options.skip_unchanged_startup
    }

    pub(crate) fn startup_can_skip(&self, current_fingerprint: &str) -> bool {
        self.options.skip_unchanged_startup
            && self.mtime_probe_trustworthy
            && self.clean_startup_fingerprint.as_deref() == Some(current_fingerprint)
    }

    pub(crate) fn ensure_current(&self) -> Result<()> {
        if let Some(cancellation) = &self.cancellation {
            cancellation.ensure_current()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ScanOutcome {
    pub roms: Arc<Vec<RomEntry>>,
    pub discovery_changed: bool,
}

pub(crate) struct DiscoverySaveRequest<'a> {
    pub system: &'a str,
    pub roms: &'a [RomEntry],
    pub system_dir: &'a Path,
    pub hash_results: &'a HashMap<String, HashResult>,
    pub db: &'a LibraryWritePool,
    pub scan_inputs: &'a ScanInputs,
    pub scan_fingerprint: &'a str,
}

impl LibraryService {
    /// Build hash-result input for discovery without reading ROM bytes.
    ///
    /// Valid cached identity is carried into the discovery save so the UI does
    /// not regress on normal rescans or hidden rebuilds. New/stale rows stay
    /// identity-pending and are handled by the later identity phase.
    pub(super) fn cached_identity_for_discovery(
        &self,
        storage: &StorageLocation,
        system: &str,
        roms: &[RomEntry],
        scan_inputs: &ScanInputs,
    ) -> HashMap<String, replay_control_core_server::rom_hash::HashResult> {
        use replay_control_core_server::rom_hash;

        if !rom_hash::is_hash_eligible(system) {
            return HashMap::new();
        }

        let mut result = HashMap::new();
        for rom in roms.iter().filter(|rom| !rom.is_m3u) {
            let rom_filename = &rom.game.rom_filename;
            if !rom_hash::is_file_hash_eligible(system, rom_filename) {
                continue;
            }
            let Some(cached) = scan_inputs.cached_hashes.get(rom_filename) else {
                continue;
            };
            let abs_path = storage.root.join(rom.game.rom_path.trim_start_matches('/'));
            let Some(current_mtime) = file_mtime_secs(&abs_path) else {
                continue;
            };
            if let Some(hash) =
                rom_hash::reusable_cached_hash(rom_filename, cached, current_mtime, rom.size_bytes)
            {
                result.insert(rom_filename.clone(), hash);
            }
        }
        result
    }

    /// Hash ROM files for a hash-eligible system and apply identification results.
    ///
    /// For eligible systems (cartridge-based with No-Intro CRC data), this:
    /// 1. Loads cached hashes from the database
    /// 2. Computes CRC32 for new/modified files
    /// 3. Looks up CRC32 in the No-Intro index
    /// 4. Overrides display names for matched ROMs (via `GameRef::new()` with the
    ///    canonical No-Intro name)
    ///
    /// Returns a map of rom_filename -> HashResult for use by save_roms_to_db.
    pub(crate) async fn hash_roms_for_system(
        &self,
        storage: &StorageLocation,
        system: &str,
        roms: &mut [RomEntry],
        scan_inputs: &ScanInputs,
        hash_cancel: Option<Arc<AtomicBool>>,
    ) -> (
        HashMap<String, replay_control_core_server::rom_hash::HashResult>,
        HashStats,
    ) {
        use replay_control_core_server::rom_hash::{self, HashResult};

        if !rom_hash::is_hash_eligible(system) {
            return (HashMap::new(), HashStats::default());
        }

        let hash_profile_started = Instant::now();

        // Build input list: (rom_filename, rom_path, size_bytes).
        let input_started = Instant::now();
        let rom_files: Vec<(String, String, u64)> = roms
            .iter()
            .filter(|r| !r.is_m3u) // Skip M3U playlists
            .map(|r| {
                (
                    r.game.rom_filename.clone(),
                    r.game.rom_path.clone(),
                    r.size_bytes,
                )
            })
            .collect();
        let input_ms = input_started.elapsed().as_millis();

        let hash_started = Instant::now();
        let hash_result = rom_hash::hash_and_identify_with_options_and_cancel(
            system,
            &rom_files,
            &scan_inputs.cached_hashes,
            &storage.root,
            rom_hash::HashOptions {
                force_rehash: scan_inputs.options.force_rehash,
            },
            hash_cancel.clone(),
        )
        .await;
        let hash_ms = hash_started.elapsed().as_millis();
        let stats = hash_result.stats;
        log_hash_stats(system, stats);
        let cancelled = hash_cancel
            .as_ref()
            .is_some_and(|cancel| cancel.load(Ordering::Relaxed));

        // Build a lookup map for applying results.
        let display_started = Instant::now();
        let mut result_map: HashMap<String, HashResult> = HashMap::new();
        for result in hash_result.results {
            result_map.insert(result.rom_filename.clone(), result);
        }

        // Apply hash-matched display names to RomEntries. The matched_name
        // is the No-Intro canonical filename stem (e.g., "Super Mario World
        // (USA)"); look it up to get the clean display title and re-apply
        // tags from the original filename.
        let canonical_filenames: Vec<String> = roms
            .iter()
            .filter_map(|rom| {
                result_map
                    .get(&rom.game.rom_filename)
                    .and_then(|hr| hr.matched_name.as_ref())
                    .map(|matched| format!("{matched}.rom"))
            })
            .collect();
        if !canonical_filenames.is_empty() {
            let refs: Vec<&str> = canonical_filenames.iter().map(String::as_str).collect();
            let display_map =
                replay_control_core_server::game_db::display_names_batch(system, &refs).await;
            for rom in roms.iter_mut() {
                if let Some(hash_result) = result_map.get(&rom.game.rom_filename)
                    && let Some(ref matched_name) = hash_result.matched_name
                {
                    let canonical_filename = format!("{matched_name}.rom");
                    if let Some(display) = display_map.get(&canonical_filename) {
                        let with_tags = replay_control_core::rom_tags::display_name_with_tags(
                            display,
                            &rom.game.rom_filename,
                        );
                        rom.game.display_name = Some(with_tags);
                    }
                }
            }
        }
        let display_ms = display_started.elapsed().as_millis();

        if !result_map.is_empty() {
            let matched = result_map
                .values()
                .filter(|r| r.matched_name.is_some())
                .count();
            tracing::debug!(
                "Hash-and-identify for {system}: {} hashed, {} matched No-Intro",
                result_map.len(),
                matched
            );
        }

        tracing::info!(
            "L2 hash profile: {system}: files={} results={} exact={} migrated={} size_only={} computed={} forced={} skipped={} cancelled={cancelled} input_ms={input_ms} hash_ms={hash_ms} display_ms={display_ms} total_ms={}",
            rom_files.len(),
            result_map.len(),
            stats.reused_exact,
            stats.reused_migrated,
            stats.reused_size_only,
            stats.computed,
            stats.forced_computed,
            stats.skipped,
            hash_profile_started.elapsed().as_millis()
        );

        (result_map, stats)
    }

    /// Write ROM list to SQLite game_library for persistent storage.
    /// Enriches with genre/players from the baked-in game databases during write.
    pub(crate) async fn save_roms_to_db(&self, request: DiscoverySaveRequest<'_>) -> Result<()> {
        let DiscoverySaveRequest {
            system,
            roms,
            system_dir,
            hash_results,
            db,
            scan_inputs,
            scan_fingerprint,
        } = request;
        let save_profile_started = Instant::now();
        let mtime_started = Instant::now();
        let mtime_secs = dir_mtime_secs(system_dir);
        let mtime_ms = mtime_started.elapsed().as_millis();

        // Delegate ROM->GameEntry conversion, clone inference, and disambiguation to core.
        let build_started = Instant::now();
        let cached_roms = replay_control_core_server::game_entry_builder::build_game_entries(
            system,
            roms,
            hash_results,
        )
        .await;
        let build_ms = build_started.elapsed().as_millis();

        tracing::debug!(
            "L2 write-through: saving {} ROMs for {system} (mtime={mtime_secs:?})",
            cached_roms.len()
        );
        let mut seen = HashSet::new();
        let unique_roms: Vec<_> = cached_roms
            .iter()
            .filter(|rom| seen.insert(rom.rom_filename.as_str()))
            .cloned()
            .collect();
        let total_size: u64 = unique_roms.iter().map(|rom| rom.size_bytes).sum();

        scan_inputs.ensure_current()?;
        let save_write_started = Instant::now();
        let begin_system = system.to_string();
        let begin_result = db
            .try_write_with_timeout(LIBRARY_MAINTENANCE_WRITE_TIMEOUT, move |conn| {
                LibraryDb::begin_system_discovery(conn, &begin_system)
            })
            .await;
        let (scan_token, scanned_at) = match begin_result {
            Ok(Ok(token)) => token,
            Ok(Err(e)) => {
                tracing::warn!("L2 write-through: begin {system} FAILED: {e}");
                return Err(e);
            }
            Err(e) => {
                tracing::warn!("L2 write-through: begin {system} write failed: {e}");
                return Err(Error::Other(format!(
                    "L2 write-through begin failed for {system}: {e}"
                )));
            }
        };

        for chunk in unique_roms.chunks(DISCOVERY_SAVE_CHUNK_ROWS) {
            scan_inputs.ensure_current()?;
            let chunk_system = system.to_string();
            let chunk_rows = chunk.to_vec();
            let chunk_result = db
                .try_write_with_timeout(LIBRARY_MAINTENANCE_WRITE_TIMEOUT, move |conn| {
                    LibraryDb::save_system_entries_chunk(
                        conn,
                        &chunk_system,
                        scan_token,
                        &chunk_rows,
                    )
                })
                .await;
            match chunk_result {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => {
                    tracing::warn!("L2 write-through: chunk {system} FAILED: {e}");
                    return Err(e);
                }
                Err(e) => {
                    tracing::warn!("L2 write-through: chunk {system} write failed: {e}");
                    return Err(Error::Other(format!(
                        "L2 write-through chunk failed for {system}: {e}"
                    )));
                }
            }
        }

        scan_inputs.ensure_current()?;
        let finalize_system = system.to_string();
        let finalize_fingerprint = scan_fingerprint.to_string();
        let rom_count = unique_roms.len();
        let finalize_result = db
            .try_write_with_timeout(LIBRARY_MAINTENANCE_WRITE_TIMEOUT, move |conn| {
                LibraryDb::finalize_system_discovery(
                    conn,
                    &finalize_system,
                    scan_token,
                    DiscoveryFinalizeStats {
                        dir_mtime_secs: mtime_secs,
                        rom_count,
                        total_size,
                        scanned_at,
                        scan_fingerprint: Some(&finalize_fingerprint),
                    },
                )
            })
            .await;
        let save_write_ms = save_write_started.elapsed().as_millis();
        match finalize_result {
            Ok(Ok(())) => {
                tracing::debug!("L2 write-through: {system} OK ({} ROMs)", unique_roms.len());

                tracing::info!(
                    "L2 discovery save profile: {system}: roms={} mtime_ms={mtime_ms} build_ms={build_ms} save_write_ms={save_write_ms} total_ms={}",
                    unique_roms.len(),
                    save_profile_started.elapsed().as_millis()
                );
                Ok(())
            }
            Ok(Err(e)) => {
                tracing::warn!("L2 write-through: finalize {system} FAILED: {e}");
                Err(e)
            }
            Err(e) => {
                tracing::warn!("L2 write-through: finalize {system} write failed: {e}");
                Err(Error::Other(format!(
                    "L2 write-through finalize failed for {system}: {e}"
                )))
            }
        }
    }
}

fn file_mtime_secs(path: &Path) -> Option<i64> {
    std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
}

fn log_hash_stats(system: &str, stats: HashStats) {
    if stats == HashStats::default() {
        return;
    }
    tracing::info!(
        "Hash-and-identify for {system}: exact={}, migrated={}, size_only={}, computed={}, forced={}, skipped={}",
        stats.reused_exact,
        stats.reused_migrated,
        stats.reused_size_only,
        stats.computed,
        stats.forced_computed,
        stats.skipped,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_inputs_without_cancellation_is_current() {
        assert!(ScanInputs::default().ensure_current().is_ok());
    }

    #[test]
    fn scan_inputs_detects_storage_generation_change() {
        let current_generation = Arc::new(AtomicU64::new(7));
        let inputs = ScanInputs::new(
            HashMap::new(),
            None,
            false,
            ScanOptions::default(),
            Some(ScanCancellation::new(current_generation.clone(), 7)),
        );
        assert!(inputs.ensure_current().is_ok());

        current_generation.store(8, Ordering::Relaxed);
        assert!(matches!(
            inputs.ensure_current(),
            Err(Error::StorageChanged)
        ));
    }

    #[test]
    fn startup_skip_requires_trustworthy_mtime_probe() {
        let options = ScanOptions {
            force_rehash: false,
            skip_unchanged_startup: true,
        };
        let missing_probe = ScanInputs::new(
            HashMap::new(),
            Some("fingerprint".to_string()),
            false,
            options,
            None,
        );
        assert!(!missing_probe.startup_can_skip("fingerprint"));

        let trusted_probe = ScanInputs::new(
            HashMap::new(),
            Some("fingerprint".to_string()),
            true,
            options,
            None,
        );
        assert!(trusted_probe.startup_can_skip("fingerprint"));
    }
}
