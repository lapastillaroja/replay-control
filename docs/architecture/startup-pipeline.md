# Startup Pipeline

`BackgroundManager::start()` in `replay-control-app/src/api/background.rs` orchestrates an ordered background pipeline and independent filesystem watchers.

## Entry Point

```
BackgroundManager::start(state)
  -> tokio::spawn(run_pipeline)    // sequential phases
  -> spawn_storage_watcher()       // independent
  -> spawn_rom_watcher()           // independent
  -> tokio::spawn(update_check_loop)
```

If storage is unavailable at boot, only the storage watcher is spawned. When storage appears (None -> Some transition via `refresh_storage()`), the full pipeline starts.

## Phase 1: Auto-Import (LaunchBox refresh)

**Method**: `phase_auto_import_inner(state, existing_guard: Option<ActivityGuard>)`

Refreshes the host-global `external_metadata.db` from the LaunchBox XML when its content has changed (or the DB has never been populated). Replaces the legacy "DB-empty" gate that broke when ROMs were added after a one-shot import.

Flow:

1. **Locate XML** via `library_db::resolve_launchbox_xml(cache_dir, storage_rc_dir)`. Search order:
   - `/var/lib/replay-control/cache/launchbox-metadata.xml` (host-global, where `download_metadata` writes)
   - `<storage>/.replay-control/launchbox-metadata.xml` (per-storage legacy)
   - `<storage>/.replay-control/Metadata.xml` (legacy upstream filename)
2. **Claim activity slot** — `Activity::RefreshExternalMetadata { progress: { phase: Checking, ... } }`. Single-flight: a concurrent caller (UI button, second boot) sees `ActivityInFlight` and bails.
3. **Hash + stamp-read in parallel** (`tokio::join!`) — stream-CRC32 the XML on the blocking pool while reading the stored `external_meta.launchbox_xml_crc32` from the read pool.
4. **Skip if hashes match** — drop the guard, no work to do.
5. **Refresh** — switch phase to `Parsing`; call `external_metadata_refresh::refresh_launchbox(xml, &mut conn, on_progress)` inside `external_metadata_pool.write`. The closure runs on deadpool's blocking thread; the progress callback updates `RefreshMetadataProgress.source_entries` so the SSE banner ticks live during the 30–90 s parse.
6. **Re-enrich every system** — switch phase to `Enriching`; call `Self::reenrich_all_systems(state)` which iterates `cached_systems` and runs `enrich_system_cache` per system. Without this, post-boot refreshes silently produce stale UI until something else triggers enrichment.
7. **Complete** — switch phase to `Complete`; guard drops → `Idle`.

Failure paths set `phase = Failed` with an `error` string before dropping the guard.

The download-then-refresh path (`spawn_external_metadata_download_and_refresh`) claims its own guard for the `Downloading` phase, then hands the guard down to `phase_auto_import_inner` so the SSE stream doesn't flicker to `Idle` between phases.

## Phase 2: Cache Verification

**Method**: `phase_cache_verification()`

Claims `Activity::Startup { phase: Scanning }`. Works directly with the DB and filesystem (no cache layer) to avoid circular dependencies.

Loads `game_library_meta` to get cached directory mtimes and ROM counts, then detects three cases:

1. **Fresh DB**: `game_library_meta` is empty -- runs `populate_all_systems()` (full scan + enrich for every system with games).
2. **Stale mtime**: filesystem directory mtime differs from stored value -- re-scans that system via `scan_and_cache_system()` + `enrich_system_cache()`.
3. **Interrupted scan**: meta says `rom_count > 0` but `game_library` has 0 rows for that system -- re-scans.

After all systems are verified, runs `library_pool.checkpoint()` to fold WAL writes back into the main DB file.

## Phase 3: Thumbnail Index Rebuild

**Method**: `phase_auto_rebuild_thumbnail_index()`

Updates activity to `StartupPhase::RebuildingIndex`. Detects evidence of data loss:

- `external_metadata.data_source` has libretro-thumbnails entries but `external_metadata.thumbnail_manifest` is empty (DB was recreated after corruption).
- No `data_source` entries but image files exist on the active storage's `media/` dir (DB was deleted).

When triggered, scans `<storage>/.replay-control/media/<system>/boxart/` directories and bulk-inserts filenames into `external_metadata.thumbnail_manifest` (one transaction across all systems). This is a disk-only rebuild — no GitHub API calls needed.

Skips entirely when both tables are empty (first-time setup).

## Storage Watcher

**Method**: `spawn_storage_watcher()`

Dual mechanism:
1. **notify watcher** (inotify on Linux): watches `replay.cfg` for immediate config change detection (skin changes, storage mode changes).
2. **Poll loop**: 10-second interval while waiting for storage, 60-second interval once connected. Calls `refresh_storage()` which detects storage appearance/disappearance.

On storage transition (None -> Some), opens DB pools and starts the full background pipeline. On disappearance (Some -> None), closes pools.

## ROM Watcher

**Method**: `spawn_rom_watcher()`

Only starts for local storage kinds (SD, USB, NVMe) -- skipped for NFS because inotify doesn't detect changes from other NFS clients.

Uses `notify::recommended_watcher` in recursive mode on the `roms/` directory. Events are debounced (3-second window) to batch rapid filesystem changes (bulk copy). On change:

- Extracts the affected system folder name from the event path.
- Triggers `get_roms()` + `enrich_system_cache()` for that system.
- Top-level changes (new system directory) trigger a `get_systems()` refresh.

## On-Demand Refresh Helpers

Two `BackgroundManager` static methods cover the user-triggered refresh paths:

- **`spawn_external_metadata_refresh(state)`** — fire-and-forget task that re-runs `phase_auto_import`. Used by the "Regenerate metadata" UI button (after wiping the stamp) and by `rebuild_corrupt_library` after the library DB is recreated.
- **`spawn_external_metadata_download_and_refresh(state)`** — claims `Activity::RefreshExternalMetadata { phase: Downloading }`, downloads `Metadata.zip` into `cache_dir` via the curl/unzip flow (with throttled byte-progress callback), then hands the guard to `phase_auto_import_inner` to parse the just-downloaded XML.
