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
5. **Refresh** — switch phase to `Parsing`; parse/build LaunchBox rows on the blocking pool, then apply them inside `external_metadata_pool.try_write_with_timeout`. The SQLite writer is held only while rows are written.
6. **Re-enrich every system** — switch phase to `Enriching`; call `Self::reenrich_all_systems(state)` which reads active systems directly from `game_library` and runs `enrich_system_cache` per system. Without this, post-boot refreshes silently produce stale UI until something else triggers enrichment.
7. **Complete** — switch phase to `Complete`; guard drops → `Idle`.

Failure paths set `phase = Failed` with an `error` string before dropping the guard.

The download-then-refresh path (`spawn_external_metadata_download_and_refresh`) claims its own guard for the `Downloading` phase, then hands the guard down to `phase_auto_import_inner` so the SSE stream doesn't flicker to `Idle` between phases.

## Phase 2: Cache Verification

**Method**: `phase_cache_verification()`

Claims `Activity::Startup { phase: Scanning }`. Works directly with the DB and filesystem (no cache layer) to avoid circular dependencies.

Loads `game_library_meta` to get cached directory mtimes and ROM counts, then detects three cases:

1. **Fresh DB**: `game_library_meta` is empty -- runs `populate_all_systems()` (single per-system pass: strict scan + inline enrich for every visible system).
2. **Stale mtime**: filesystem directory mtime differs from stored value -- re-scans that system via `scan_and_cache_system()` + `enrich_system_cache()`.
3. **Interrupted scan**: meta says `rom_count > 0` but `game_library` has 0 rows for that system -- re-scans.

For hash-eligible cartridge systems, scan inputs include cached CRC rows for that system unless the caller explicitly forces a rebuild. CRC cache validation uses the ROM filename plus the file size recorded with the hash. Exact `mtime + size` matches reuse the cached CRC; migrated rows with no stored hash size reuse only when mtime still matches; same-size mtime drift is reused as a conservative fast path for normal rescans. This avoids streaming unchanged large ROMs, especially N64/GBA/SNES sets on NFS, while manual rebuild remains the full verification path.

`populate_all_systems` no longer pre-walks the filesystem to count systems; it iterates `visible_systems()` directly and lets each per-system call decide what to write (strict reconcile rule). Empty walks on local storage reconcile to empty meta; on NFS they return `Err` and preserve cached state. See `replay-control-app/src/api/library/mod.rs` and the per-system reconcile tests there.

Long startup, rescan, rebuild, and watcher scans capture a storage generation token before they start. If `refresh_storage()` swaps storage, closes/reopens DB pools, or moves into a configured-storage error state, the generation changes. In-flight scans stop at the next system boundary or before the per-system DB write/enrichment step, so stale results cannot land in the wrong active storage DB. Cancellation preserves already-completed systems and leaves untouched systems' existing L2 rows in place.

After all systems are verified, the pipeline continues directly into thumbnail-index recovery. WAL databases rely on SQLite's automatic checkpointing, so startup no longer forces a broad post-scan `library_pool.checkpoint()`.

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
- Invalidates L1/user caches without pre-clearing L2.
- Strict-scans each affected system with normal CRC cache reuse and then runs `enrich_system_cache()` only when the scan succeeds.
- Top-level `roms/` changes iterate `visible_systems()` so newly-created system folders are discovered and removed local folders reconcile to empty.

## On-Demand Refresh Helpers

Two `BackgroundManager` static methods cover the user-triggered refresh paths:

- **`spawn_external_metadata_refresh(state)`** — fire-and-forget task that re-runs `phase_auto_import`. Used by the "Regenerate metadata" UI button (after wiping the stamp) and by `rebuild_corrupt_library` after the library DB is recreated.
- **`spawn_external_metadata_download_and_refresh(state)`** — claims `Activity::RefreshExternalMetadata { phase: Downloading }`, downloads `Metadata.zip` into `cache_dir` via the curl/unzip flow (with throttled byte-progress callback), then hands the guard to `phase_auto_import_inner` to parse the just-downloaded XML.
