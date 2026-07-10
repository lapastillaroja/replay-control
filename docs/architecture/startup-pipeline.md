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

If storage is unavailable at boot, only the storage watcher is spawned. When storage appears (None -> Some transition via `reload_config_and_redetect_storage()`), the full pipeline starts.

## Phase 0.5: First-Run Source Fetch

**Method**: `phase_first_run_seed()`

On a fresh install, Replay Control downloads optional source data before the
first library scan: LaunchBox XML and libretro thumbnail manifests. This is a
one-time startup cost, and waiting keeps the first build from indexing a partial
library that immediately needs catch-up enrichment. The UI shows the startup
metadata-fetch banner while this runs.

Network failures are logged and startup continues. Built-in catalog data still
provides offline names, genres, dates, and player counts; downloaded sources add
descriptions, ratings, provider media links, and thumbnail matching data when
available.

## Phase 1: Auto-Import (LaunchBox refresh)

**Method**: `phase_auto_import_inner(state, existing_guard: Option<ActivityGuard>)`

Refreshes the host-global `external_metadata.db` from the LaunchBox XML when its content has changed (or the DB has never been populated). Replaces the legacy "DB-empty" gate that broke when ROMs were added after a one-shot import.

Flow:

1. **Locate XML** via `library_db::resolve_launchbox_xml(download_dir, storage_rc_dir)`. Search order:
   - `/var/lib/replay-control/cache/launchbox-metadata.xml` (host-global download directory, where `download_metadata` writes)
   - `<storage>/.replay-control/launchbox-metadata.xml` (per-storage legacy)
   - `<storage>/.replay-control/Metadata.xml` (legacy upstream filename)
2. **Claim activity slot** — `Activity::RefreshExternalMetadata { progress: { phase: Checking, ... } }`. Single-flight: a concurrent caller (UI button, second boot) sees `ActivityInFlight` and bails.
3. **Hash + stamp-read in parallel** (`tokio::join!`) — stream-CRC32 the XML on the blocking pool while reading the stored `external_meta.launchbox_xml_crc32` from the read pool.
4. **Skip if hashes match** — drop the guard, no work to do.
5. **Refresh** — switch phase to `Parsing`; parse/build LaunchBox rows on the blocking pool, then apply them inside `external_metadata_pool.try_write_with_timeout`. The SQLite writer is held only while rows are written.
6. **Re-enrich every system** — switch phase to `Enriching`; call `Self::reenrich_all_systems(state)` which reads active systems directly from `game_library` and runs `enrich_system_library` per system. Without this, post-boot refreshes silently produce stale UI until something else triggers enrichment.
7. **Complete** — switch phase to `Complete`; guard drops → `Idle`.

Failure paths set `phase = Failed` with an `error` string before dropping the guard.

The download-then-refresh path (`spawn_external_metadata_download_and_refresh`) claims its own guard for the `Downloading` phase, then hands the guard down to `phase_auto_import_inner` so the SSE stream doesn't flicker to `Idle` between phases.

## Phase 2: Startup Library Scan

**Method**: `phase_library_verification()`

Claims `Activity::Startup { phase: Scanning }`. Works directly with the DB and filesystem (no request-time cache layer) to avoid circular dependencies.

Startup runs a full recursive walk over every `visible_systems()` platform,
then reconciles only systems whose durable scan fingerprint changed or whose
previous pipeline state was incomplete:

1. Iterate every visible system.
2. Strict-walk that system's ROM tree, including nested folders.
3. Compare the current per-system fingerprint with the last complete startup
   fingerprint stored in `library_meta`.
4. If the fingerprint matches and discovery, enrichment, and identity are all
   complete, skip the discovery write, enrichment, and identity queue for that
   system.
5. Otherwise save the successful scan with a scan-token reconcile: current rows
   are upserted in chunks, and stale rows are deleted only during finalization.
6. Run inline enrichment for that same system.
7. Queue hash identity work for new, stale, failed, or unresolved rows.

Startup intentionally does **not** rely on top-level system directory mtimes.
Users commonly organize ROMs in subfolders, and parent-directory mtimes are not
a reliable cross-storage signal for offline changes. A full walk is the
correctness boundary that catches ROMs added while the device was off. The
fingerprint is derived from the walked file list, relative paths, sizes, and
file mtimes; it is a post-walk no-op check, not a shortcut around discovery.
Manual rescans deliberately do not use this skip path: they always run
discovery and enrichment with reusable CRC identity so the user can force
metadata refresh without a full rebuild.

An interrupted rebuild or rescan is recovered by the same rule. Per-system
writes are transactional, so a system is either committed or rolled back. On the
next boot, startup walks every visible system again, repairs systems that were
not reached before shutdown, and resumes normal background identity work. It
does not automatically continue forced rebuild hashing; explicit manual rebuild
remains the deep verification path.

For hash-eligible cartridge systems, scan inputs include stored CRC rows for that system unless the caller explicitly forces a rebuild. Stored CRC validation uses the ROM filename plus the file size recorded with the hash. Exact `mtime + size` matches reuse the stored CRC; migrated rows with no stored hash size reuse only when mtime still matches; same-size mtime drift is reused as a conservative fast path for normal rescans. This avoids streaming unchanged large ROMs, especially N64/GBA/SNES sets on NFS, while manual rebuild remains the full verification path.

CRC identity work runs after the per-system scan/enrichment write, not inside the filesystem discovery writer closure. While it runs, the activity banner shows "Matching ROMs" progress based on 200-row mini-batches. Rebuild and rescan requests are blocked until identity finishes, so normal user actions do not cancel long NFS reads. Storage changes still cancel the identity phase through `storage_generation`, and unresolved rows remain retryable. The worker count defaults to 2 for every storage class, with an advanced override via `REPLAY_CONTROL_IDENTITY_WORKERS` (valid range: 1-4). Keep this bounded: the goal is to overlap storage latency without creating excessive CPU or I/O contention.

`populate_all_systems` no longer pre-walks the filesystem to count systems; it iterates `visible_systems()` directly and lets each per-system call decide what to write (strict reconcile rule). Empty walks on local storage reconcile to empty meta; on NFS they return `Err` and preserve stored state. See `replay-control-app/src/api/library/mod.rs` and the per-system reconcile tests there.

Long startup, rescan, rebuild, and watcher scans capture a storage generation token before they start. If `redetect_storage()` swaps storage, closes/reopens DB pools, or moves into a configured-storage error state, the generation changes. In-flight scans stop at the next system boundary or before the per-system DB write/enrichment step, so stale results cannot land in the wrong active storage DB. Cancellation preserves already-completed systems and leaves untouched systems' existing stored rows in place.

ROM file changes made by the user while a scan, rebuild, or identity pass is already running are not treated as a consistency guarantee for that same pass. The supported recovery is a later manual rescan after file changes settle.

After all systems are reconciled, the pipeline continues directly into thumbnail-index recovery. WAL databases rely on SQLite's automatic checkpointing, so startup no longer forces a broad post-scan `library_pool.checkpoint()`.

## Phase 3: Thumbnail Index Rebuild

**Method**: `phase_auto_rebuild_thumbnail_index()`

Updates activity to `StartupPhase::RebuildingIndex`. Detects evidence of data loss:

- `external_metadata.data_source` has libretro-thumbnails entries but `external_metadata.thumbnail_manifest` is empty (DB was recreated after corruption).
- No `data_source` entries but image files exist on the active storage's `media/` dir (DB was deleted).

When triggered, scans `<storage>/.replay-control/media/<system>/boxart/` directories and bulk-inserts filenames into `external_metadata.thumbnail_manifest` (one transaction across all systems). This is a disk-only rebuild — no GitHub API calls needed.

Skips entirely when both tables are empty (first-time setup).

## Storage Watcher

**Method**: `spawn_storage_watcher()` (Device mode only — Standalone has nothing to watch off-device)

Two event sources, both kernel-driven, no periodic poll:

1. **`notify` watcher** (inotify on Linux): watches `replay.cfg` for immediate config change detection (skin changes, storage mode changes, wifi/NFS/RetroAchievements writes triggered from the TV UI). Calls `reload_config_and_redetect_storage()` on change.
2. **`mountinfo_watcher`** (`POLLPRI` on `/proc/self/mountinfo`, Linux only): wakes on every mount-table change. Also calls `reload_config_and_redetect_storage()` — this covers the boot-recovery case where the app started in `ConfigUnavailable` and the SD mount arrives carrying a fresh `replay.cfg`, so both the storage *and* the config need re-detection together.

The previous 10s / 60s belt-and-suspenders poll was removed: on RePlayOS the kernel watchers cover all real events, and on dev hosts where mountinfo is a no-op, storage issues surface at request time via IO errors. User-triggered `refresh_storage` server fn (`reload_config_and_redetect_storage`) is also available for explicit re-detection.

On storage transition (None -> Some), opens DB pools and starts the full background pipeline. On disappearance (Some -> None), closes pools.

In **Standalone mode** (`--storage-path`), the watcher is a no-op: `replay.cfg` is RePlayOS-owned and not under the supplied folder, mount changes on the host are not meaningful for an off-device ROM manager, and folder-disappearance surfaces at the next ROM-read. The user-triggered `refresh_storage` server fn still runs and performs a liveness check on the standalone root.

## ROM Watcher

**Method**: `spawn_rom_watcher()`

Only starts for local storage kinds (SD, USB, NVMe) -- skipped for NFS because inotify doesn't detect changes from other NFS clients.

Uses `notify::recommended_watcher` in recursive mode on the `roms/` directory. Events are debounced (3-second window) to batch rapid filesystem changes (bulk copy). On change:

- Extracts the affected system folder name from the event path.
- Invalidates in-memory/user caches without pre-clearing durable rows.
- Strict-scans each affected system with normal stored CRC reuse and then runs `enrich_system_library()` only when the scan succeeds.
- Top-level `roms/` changes iterate `visible_systems()` so newly-created system folders are discovered and removed local folders reconcile to empty.

## On-Demand Refresh Helpers

Two `BackgroundManager` static methods cover the user-triggered refresh paths:

- **`spawn_external_metadata_refresh(state)`** — fire-and-forget task that re-runs `phase_auto_import`. Used by the "Regenerate metadata" UI button (after wiping the stamp) and by `rebuild_corrupt_library` after the library DB is recreated.
- **`spawn_external_metadata_download_and_refresh(state)`** — claims `Activity::RefreshExternalMetadata { phase: Downloading }`, downloads `Metadata.zip` into the host-global download directory via the curl/unzip flow (with throttled byte-progress callback), then hands the guard to `phase_auto_import_inner` to parse the just-downloaded XML.
