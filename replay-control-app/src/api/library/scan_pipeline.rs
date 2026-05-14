use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use replay_control_core::error::{Error, Result};
use replay_control_core::rom_tags::RegionPreference;
use replay_control_core_server::rom_hash::{CachedHash, HashStats};
use replay_control_core_server::roms::RomEntry;
use replay_control_core_server::storage::StorageLocation;

use replay_control_core_server::library_db::LibraryDb;

use super::{LibraryService, dir_mtime_secs};
use crate::api::db_pools::{LIBRARY_MAINTENANCE_WRITE_TIMEOUT, LibraryWritePool};

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ScanOptions {
    pub force_rehash: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ScanInputs {
    cached_hashes: HashMap<String, CachedHash>,
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
        options: ScanOptions,
        cancellation: Option<ScanCancellation>,
    ) -> Self {
        Self {
            cached_hashes,
            options,
            cancellation,
        }
    }

    pub(crate) fn cancellation(&self) -> Option<&ScanCancellation> {
        self.cancellation.as_ref()
    }

    pub(crate) fn ensure_current(&self) -> Result<()> {
        if let Some(cancellation) = &self.cancellation {
            cancellation.ensure_current()?;
        }
        Ok(())
    }
}

impl LibraryService {
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
    pub(super) async fn hash_roms_for_system(
        &self,
        storage: &StorageLocation,
        system: &str,
        roms: &mut [RomEntry],
        scan_inputs: &ScanInputs,
    ) -> HashMap<String, replay_control_core_server::rom_hash::HashResult> {
        use replay_control_core_server::rom_hash::{self, HashResult};

        if !rom_hash::is_hash_eligible(system) {
            return HashMap::new();
        }

        // Build input list: (rom_filename, rom_path, size_bytes).
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

        let hash_result = rom_hash::hash_and_identify_with_options(
            system,
            &rom_files,
            &scan_inputs.cached_hashes,
            &storage.root,
            rom_hash::HashOptions {
                force_rehash: scan_inputs.options.force_rehash,
            },
        )
        .await;
        let stats = hash_result.stats;
        log_hash_stats(system, stats);

        // Build a lookup map for applying results.
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

        result_map
    }

    /// Write ROM list to SQLite game_library for persistent storage.
    /// Enriches with genre/players from the baked-in game databases during write.
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn save_roms_to_db(
        &self,
        _storage: &StorageLocation,
        system: &str,
        roms: &[RomEntry],
        system_dir: &Path,
        hash_results: &HashMap<String, replay_control_core_server::rom_hash::HashResult>,
        region_pref: RegionPreference,
        region_secondary: Option<RegionPreference>,
        db: &LibraryWritePool,
        scan_inputs: &ScanInputs,
    ) -> Result<()> {
        let mtime_secs = dir_mtime_secs(system_dir);

        // Delegate ROM->GameEntry conversion, clone inference, and disambiguation to core.
        let cached_roms = replay_control_core_server::game_entry_builder::build_game_entries(
            system,
            roms,
            hash_results,
        )
        .await;

        tracing::debug!(
            "L2 write-through: saving {} ROMs for {system} (mtime={mtime_secs:?})",
            cached_roms.len()
        );
        let system_owned = system.to_string();
        let system_for_save = system_owned.clone();
        let cached_roms_for_db = cached_roms.clone();
        scan_inputs.ensure_current()?;
        let result = db
            .try_write_with_timeout(LIBRARY_MAINTENANCE_WRITE_TIMEOUT, move |conn| {
                LibraryDb::save_system_entries(
                    conn,
                    &system_for_save,
                    &cached_roms_for_db,
                    mtime_secs,
                )
            })
            .await;
        match result {
            Ok(Ok(())) => {
                tracing::debug!("L2 write-through: {system} OK ({} ROMs)", cached_roms.len());

                // Populate TGDB aliases from embedded build-time data.
                self.populate_tgdb_aliases(system, &cached_roms, db, scan_inputs)
                    .await?;

                // Populate game_series from embedded Wikidata data.
                self.populate_wikidata_series(system, &cached_roms, db, scan_inputs)
                    .await?;
                scan_inputs.ensure_current()?;

                // Seed `game_release_date` in three steps:
                //  1. Build-time static emit: TGDB per-region dates + arcade
                //     MAME/FBNeo/Naomi year rows. This gives us multi-region
                //     coverage (USA/Japan/Europe dates from TGDB) immediately.
                //  2. `game_library` mirror columns from the builder — year
                //     fallback for games TGDB didn't classify per-region
                //     (CanonicalGame.year → release_date = "YYYY").
                //  3. Resolve per-region mirror columns from the user's
                //     region preference.
                //
                // The later L2 enrichment pass upserts LaunchBox-sourced
                // rows (`launchbox` source, world region, day-precision
                // when the XML provides it) before re-running the resolver.
                let static_data =
                    replay_control_core_server::library_db::fetch_static_release_data().await;
                scan_inputs.ensure_current()?;
                let release_result = db
                    .try_write_with_timeout(LIBRARY_MAINTENANCE_WRITE_TIMEOUT, move |conn| {
                        if let Err(e) = LibraryDb::seed_release_dates_from_static_for_system(
                            conn,
                            &system_owned,
                            static_data,
                        ) {
                            tracing::warn!(
                                "Static release-date seed failed for {system_owned}: {e}"
                            );
                        }
                        if let Err(e) = LibraryDb::seed_release_dates_from_library_for_system(
                            conn,
                            &system_owned,
                            "builder",
                        ) {
                            tracing::warn!(
                                "Library release-date seed failed for {system_owned}: {e}"
                            );
                        }
                        if let Err(e) = LibraryDb::resolve_release_date_for_system(
                            conn,
                            &system_owned,
                            region_pref,
                            region_secondary,
                        ) {
                            tracing::warn!("Release-date resolve failed for {system_owned}: {e}");
                        }
                    })
                    .await;
                if let Err(e) = release_result {
                    tracing::warn!("Release-date write failed for {system}: {e}");
                }
                Ok(())
            }
            Ok(Err(e)) => {
                tracing::warn!("L2 write-through: {system} FAILED: {e}");
                Err(e)
            }
            Err(e) => {
                tracing::warn!("L2 write-through: {system} write failed: {e}");
                Err(Error::Other(format!(
                    "L2 write-through failed for {system}: {e}"
                )))
            }
        }
    }
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
}
