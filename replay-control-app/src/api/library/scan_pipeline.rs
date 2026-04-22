use std::collections::HashMap;
use std::path::Path;

use replay_control_core::rom_tags::RegionPreference;
use replay_control_core::roms::RomEntry;
use replay_control_core::storage::StorageLocation;

use replay_control_core::metadata_db::MetadataDb;

use super::{LibraryService, dir_mtime};
use crate::api::DbPool;

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
        db: &DbPool,
    ) -> HashMap<String, replay_control_core::rom_hash::HashResult> {
        use replay_control_core::rom_hash::{self, HashResult};

        if !rom_hash::is_hash_eligible(system) {
            return HashMap::new();
        }

        // Load cached hashes from L2 (database).
        let system_owned = system.to_string();
        let cached_hashes = db
            .read(move |conn| MetadataDb::load_cached_hashes(conn, &system_owned))
            .await
            .and_then(|r| r.ok())
            .unwrap_or_default();

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

        let results =
            rom_hash::hash_and_identify(system, &rom_files, &cached_hashes, &storage.root).await;

        // Build a lookup map for applying results.
        let mut result_map: HashMap<String, HashResult> = HashMap::new();
        for result in results {
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
                replay_control_core::game_db::display_names_batch(system, &refs).await;
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
        hash_results: &HashMap<String, replay_control_core::rom_hash::HashResult>,
        region_pref: RegionPreference,
        region_secondary: Option<RegionPreference>,
        db: &DbPool,
    ) {
        let mtime_secs = dir_mtime(system_dir).and_then(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|d| d.as_secs() as i64)
        });

        // Delegate ROM->GameEntry conversion, clone inference, and disambiguation to core.
        let cached_roms =
            replay_control_core::game_entry_builder::build_game_entries(system, roms, hash_results)
                .await;

        tracing::debug!(
            "L2 write-through: saving {} ROMs for {system} (mtime={mtime_secs:?})",
            cached_roms.len()
        );
        let system_owned = system.to_string();
        let cached_roms_for_db = cached_roms.clone();
        let result = db
            .write(move |conn| {
                MetadataDb::save_system_entries(
                    conn,
                    &system_owned,
                    &cached_roms_for_db,
                    mtime_secs,
                )
            })
            .await;
        match result {
            Some(Ok(())) => {
                tracing::debug!("L2 write-through: {system} OK ({} ROMs)", cached_roms.len());

                // Populate TGDB aliases from embedded build-time data.
                self.populate_tgdb_aliases(system, &cached_roms, db).await;

                // Populate game_series from embedded Wikidata data.
                self.populate_wikidata_series(system, &cached_roms, db)
                    .await;

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
                // LaunchBox enrichment later runs `seed_release_dates_from_metadata`
                // to upgrade to day-precision USA dates.
                let static_data =
                    replay_control_core::metadata_db::fetch_static_release_data().await;
                let _ = db
                    .write(move |conn| {
                        let _ = MetadataDb::seed_release_dates_from_static(conn, static_data);
                        let _ = MetadataDb::seed_release_dates_from_library(conn, "builder");
                        let _ = MetadataDb::resolve_release_date_for_library(
                            conn,
                            region_pref,
                            region_secondary,
                        );
                    })
                    .await;
            }
            Some(Err(e)) => tracing::warn!("L2 write-through: {system} FAILED: {e}"),
            None => tracing::warn!("L2 write-through: {system} skipped (DB unavailable)"),
        }
    }
}
