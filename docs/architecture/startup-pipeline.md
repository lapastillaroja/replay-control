# Startup Pipeline

`BackgroundManager::start()` in `replay-control-app/src/api/background.rs` orchestrates an ordered background pipeline and independent filesystem watchers.

## Entry Point

```
BackgroundManager::start(state)
  -> tokio::spawn(run_pipeline)    // sequential phases
  -> spawn_storage_watcher()       // independent
  -> spawn_rom_watcher()           // independent
```

If storage is unavailable at boot, only the storage watcher is spawned. When storage appears (None -> Some transition via `refresh_storage()`), the full pipeline starts.

## Phase 1: Auto-Import

**Method**: `phase_auto_import()`

Checks for `<storage>/.replay-control/launchbox-metadata.xml` (or legacy `Metadata.xml`). If the file exists **and** the `game_metadata` table is empty, triggers `ImportPipeline::start_import_no_enrich()`.

The import claims its own `Activity::Import` via `try_start_activity`. The pipeline waits in a 500ms poll loop for the import to finish before proceeding.

## Phase 2: Cache Verification

**Method**: `phase_cache_verification()`

Claims `Activity::Startup { phase: Scanning }`. Works directly with the DB and filesystem (no cache layer) to avoid circular dependencies.

Loads `game_library_meta` to get cached directory mtimes and ROM counts, then detects three cases:

1. **Fresh DB**: `game_library_meta` is empty -- runs `populate_all_systems()` (full scan + enrich for every system with games)
2. **Stale mtime**: filesystem directory mtime differs from stored value -- re-scans that system via `scan_and_cache_system()` + `enrich_system_cache()`
3. **Interrupted scan**: meta says `rom_count > 0` but `game_library` has 0 rows for that system -- re-scans

After all systems are verified, runs `metadata_pool.checkpoint()` to fold WAL writes back into the main DB file.

## Phase 3: Thumbnail Index Rebuild

**Method**: `phase_auto_rebuild_thumbnail_index()`

Updates activity to `StartupPhase::RebuildingIndex`. Detects evidence of data loss:

- `data_sources` has libretro-thumbnails entries but `thumbnail_index` is empty (DB was recreated after corruption)
- No `data_sources` entries but image files exist on disk (DB was deleted)

When triggered, scans `<storage>/.replay-control/media/<system>/boxart/` directories and bulk-inserts filenames into `thumbnail_index`. This is a disk-only rebuild -- no GitHub API calls needed.

Skips entirely when both tables are empty (first-time setup).

## Storage Watcher

**Method**: `spawn_storage_watcher()`

Dual mechanism:
1. **notify watcher** (inotify on Linux): watches `replay.cfg` for immediate config change detection (skin changes, storage mode changes)
2. **Poll loop**: 10-second interval while waiting for storage, 60-second interval once connected. Calls `refresh_storage()` which detects storage appearance/disappearance

On storage transition (None -> Some), opens DB pools and starts the full background pipeline. On disappearance (Some -> None), closes pools.

## ROM Watcher

**Method**: `spawn_rom_watcher()`

Only starts for local storage kinds (SD, USB, NVMe) -- skipped for NFS because inotify doesn't detect changes from other NFS clients.

Uses `notify::recommended_watcher` in recursive mode on the `roms/` directory. Events are debounced (3-second window) to batch rapid filesystem changes (bulk copy). On change:

- Extracts the affected system folder name from the event path
- Triggers `get_roms()` + `enrich_system_cache()` for that system
- Top-level changes (new system directory) trigger a `get_systems()` refresh
