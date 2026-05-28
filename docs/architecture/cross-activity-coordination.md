# Cross-Activity Coordination

This document audits the write paths across the three SQLite databases (`library.db`, `external_metadata.db`, `user_data.db`), classifies which guards each path holds, and lists the conflict pairs that real-world traffic can exercise. Use it to decide whether a new write needs an `Activity` guard, a `storage_generation` check, both, or neither.

For background see [Activity System](activity-system.md), [Connection Pooling](connection-pooling.md), and the "Write-isolation rule" in [Database Schema](database-schema.md).

## Coordination primitives

- **Activity mutex** — `AppState::try_start_activity` (`replay-control-app/src/api/activity.rs`). At most one `Activity != Idle` at a time. The returned `ActivityGuard` resets to `Idle` on drop, so a panic still releases the slot.
- **`storage_generation: AtomicU64`** on `AppState`. Bumped inside `redetect_storage` (and the deferred-storage paths in `cancel_storage_scans_if_ready`). Long scans capture the generation at start, thread it through `ScanInputs`/`ScanCancellation`, and call `state.ensure_storage_generation(expected)` at every system boundary plus before each writer transaction.
- **`rom_watcher_generation: AtomicU64`** on `AppState`. Bumped by `restart_rom_watcher`; the watcher loop self-terminates on mismatch. Independent of `storage_generation`.
- **`is_idle()` gate** on `AppState`. Used by the ROM watcher to suppress its own work during any non-`Idle` activity.
- **`identity_can_run()` gate** on `AppState`. Identity workers are allowed while `Activity::Identity` owns the activity slot, but stop when any foreground activity or storage-generation change appears.
- **`require_configured_storage_ready_for_mutation`** — refreshes storage, then rejects mutations when the configured target is not `Ready`. Single-shot, not a serializing mutex.
- **Pool drain on `reset_to_empty` / `reopen` / `replace_with_file`** — the pool waits for in-flight `Object`s to release before unlinking files. A stalled closure aborts the destructive op rather than racing.

## 1. Inventory of write paths

### 1.1 `library.db` writers

| # | Path | Activity | `storage_generation` | Other gating |
|---|---|---|---|---|
| L1 | `populate_all_systems` (Startup pipeline + `spawn_populate`) | `Activity::Startup{Scanning}` or `Activity::Rebuild` | yes — between systems and inside `scan_inputs_for_system` | n/a |
| L2 | Startup full reconciliation via `phase_cache_verification` | `Activity::Startup{Scanning}` | yes | same per-system strict reconcile path as L1 |
| L3 | Background identity matching after scan/rebuild | `Activity::Identity` | yes — before claim, before/after hashing, and before writes | owns activity slot; rebuild/rescan are blocked while it runs |
| L4 | ROM watcher rescan | none — fires only when `is_idle()` is true | yes | `is_idle()` precondition; `rom_watcher_generation` self-cancels |
| L5 | `enrich_system_cache_with_cancellation` | inherits caller's guard | yes via `cancellation.ensure_current()` | n/a |
| L6 | On-demand box-art download hook (`update_box_art_url` from thumbnail orchestrator) | none | none | none — INSERT OR REPLACE upsert is race-tolerant |
| L7 | `cleanup_orphaned_images` | `Activity::Maintenance{CleanupOrphans}` | none | mutation guard |
| L8 | `clear_images` | `Activity::Maintenance{ClearImages}` | none | mutation guard |
| L9 | `delete_rom_cleanup` | none | none | mutation guard |
| L10 | `rename_rom_cascade` | none | none | mutation guard |
| L11 | `set_boxart_override` / `reset_boxart_override` | none | none | mutation guard |
| L12 | `save_region_preference` / `_secondary` (writes settings, invalidates L1, then runs `resolve_release_date_for_library`) | **none** | none | mutation guard not present |
| L13 | `rebuild_corrupt_library` (`reset_to_empty`) | none | none | mutation guard + corruption flag |
| L14 | `phase_title_norm_reconcile` (idempotent rebuild of `normalized_title`) | runs ahead of Startup guard | none | n/a |
| L15 | Storage-swap reopen (`library_writer.reopen`) | none — runs synchronously inside `redetect_storage` | drives generation bumps itself | storage RwLock + pool drain |

### 1.2 `external_metadata.db` writers

| # | Path | Activity |
|---|---|---|
| E1 | `phase_first_run_seed` (libretro manifest fetch) | `Activity::Startup{FetchingMetadata}` |
| E2 | `phase_auto_import_inner` (LaunchBox refresh + `Enriching` re-enrichment loop) | `Activity::RefreshExternalMetadata` (single-flight) |
| E3 | `spawn_external_metadata_download_and_refresh` | `Activity::RefreshExternalMetadata{Checking → … → Complete}` |
| E4 | `phase_auto_rebuild_thumbnail_index` | inherits Startup guard |
| E5 | Thumbnail pipeline phase 1 (`import_all_manifests` + fetched-at stamp) | `Activity::ThumbnailUpdate{Indexing}` |
| E6 | `clear_metadata` | `Activity::Maintenance{ClearMetadata}` |
| E7 | `regenerate_metadata` | **none for the clear**, then spawns the refresh which claims `RefreshExternalMetadata` |
| E8 | `clear_thumbnail_index` | `Activity::Maintenance{ClearThumbnailIndex}` |

### 1.3 `user_data.db` writers

| # | Path | Activity |
|---|---|---|
| U1 | `set_boxart_override` / `reset_boxart_override` | none, mutation guard |
| U2 | `add_game_video` / `remove_game_video` | none, mutation guard |
| U3 | `delete_rom_cleanup` | none, mutation guard |
| U4 | `rename_rom_cascade` | none, mutation guard |
| U5 | `repair_corrupt_user_data` (`reset_to_empty`) | none |
| U6 | `restore_user_data_backup` (`replace_with_file`, fallback `reset_to_empty`) | none |
| U7 | Storage-swap (`reopen_user_data_or_mark_corrupt`) | none — inside `redetect_storage` |

`user_data.db` runs in DELETE mode on most storages (exFAT/NFS-friendly). Per-`try_write` `WriteGate` activation serializes readers vs the single-slot writer, so concurrency between U1–U4 is harmless serialization at the pool layer.

## 2. Conflict matrix

Pairs with non-trivial overlap. "OK" means existing guards prevent the bad outcome; "gap" means it can be observed and there is no compensating mechanism.

| Pair | Can overlap? | Outcome | Status |
|---|---|---|---|
| Rebuild (L1) ↔ Startup (L1/L2) | No — both claim the activity mutex | n/a | OK |
| Rebuild (L1) ↔ Storage swap reopen (L15) | Yes by design — generation bump cancels in-flight scan | Cancelled scan releases the Rebuild guard; pool reopens after the next system boundary. The follow-up `spawn_pipeline` may briefly fail to claim Startup. | **Gap F-1** |
| Rebuild (L1) ↔ Identity (L3) | No — both claim the activity mutex | n/a | OK |
| Rebuild (L1) ↔ ROM watcher rescan (L4) | No — watcher gates on `is_idle()` | n/a | OK |
| Rebuild (L1) ↔ Settings writes (L12) | Yes — `save_region_preference` doesn't claim a guard | No table clear occurs anymore; the handler only invalidates L1 and rewrites `release_date` mirror columns for rows currently present. | OK for data preservation |
| Rebuild (L1) ↔ External-metadata refresh re-enrichment (E2 + L5) | No — both claim activity mutex | n/a | OK |
| Identity (L3) ↔ Storage swap (L15) | Yes by design — generation bump cancels in-flight identity | Workers stop before applying stale results, and unresolved rows remain retryable. | OK |
| ROM watcher (L4) ↔ Storage swap (L15) | Yes — `restart_rom_watcher` bumps the watcher generation; an in-flight debounce can complete one cycle | Per-system writes inside that cycle pass `ensure_storage_generation` → `Err(StorageChanged)` → cancelled | OK |
| Maintenance (L7/L8/E6/E8) ↔ Rebuild | No — activity mutex | n/a | OK |
| Maintenance ↔ User mutation (U1–U4, L9–L11) | Maintenance holds activity; user mutations don't | All concrete pairs touch disjoint columns; pool layer serializes | OK (harmless) |
| `regenerate_metadata` clear (E7) ↔ concurrent `Activity::Rebuild` reading enrichment (L5) | Yes — E7 clears provider metadata without claiming activity, then *tries* to claim `RefreshExternalMetadata` | If the spawn-claim fails (busy), provider tables are gone and re-enrichment silently runs against an empty source | **Gap F-2** |
| `rebuild_corrupt_library` (L13) ↔ in-flight Rebuild | Yes — L13 calls `reset_to_empty` without claiming `Rebuild` | Pool drain blocks until rebuild's writer connection releases; on timeout, L13 aborts cleanly | OK (drain semantics) |
| `repair_corrupt_user_data` / `restore_user_data_backup` ↔ user mutations | Yes — none claim activity | Pool drain blocks; on timeout aborts cleanly | OK |
| Concurrent `reload_config_and_redetect_storage` invocations (config-file watcher + mountinfo watcher + mutation gate + HTTP refresh_storage) | Yes — `redetect_storage` is not protected by a serializing mutex | Two callers can both bump `storage_generation`, both `reopen` pools, both emit `StorageChanged`. Storage status oscillates `Activating → Ready → Activating → Ready`. Correctness preserved (each bump invalidates its predecessor's scans) but pool warmup cost doubles. | **Gap F-4** |
| L1 cache invalidation during Rebuild (favorites/recents inotify cache wipes) ↔ Rebuild populating L2 | Yes — favorites/recents L1 invalidations are deliberately ungated | Rebuild does not own those caches; next request rebuilds them | OK |

## 3. Findings

Severity-ordered. F-3 is kept under [Resolved findings](#6-resolved-findings) because it documents a real data-loss class that the region-preference handlers must not reintroduce.

### F-1: storage-swap during Rebuild loses the new pipeline

**Severity: medium (silent failure-to-populate).**

Sequence on a storage swap while `Activity::Rebuild` is held:

1. `redetect_storage` calls `bump_storage_generation()`. In-flight rebuild scans see `Err(StorageChanged)` at their next gate and start unwinding.
2. `redetect_storage` then calls `BackgroundManager::spawn_pipeline(self.clone())`.
3. `spawn_pipeline` runs `run_pipeline`, which calls `try_start_activity(Activity::Startup{...})`.
4. **Race window**: the cancelled rebuild task hasn't dropped its `Activity::Rebuild` guard yet (still unwinding). `try_start_activity` returns `Err("Another operation is already running")`, the new pipeline aborts with a `warn!` log, and there is no retry.

Result: the new storage's `library.db` is not (re)populated until the next reboot or a manual rebuild. No banner, no automatic recovery.

The retry helper exists for the inverse case (`claim_startup_activity` retries on activity-busy) but is only used by `run_pipeline`'s top-level Startup→Rebuild sequencing, not by `spawn_pipeline` on storage swap.

**Fix**: route `spawn_pipeline` through the existing `claim_startup_activity` retry helper so it lands once the rebuild guard drops.

### F-2: `regenerate_metadata` is a non-atomic clear-then-spawn

**Severity: medium (silent metadata wipe with no recovery prompt).**

`regenerate_metadata` clears the LaunchBox tables on the writer pool, then spawns `spawn_external_metadata_refresh`, which itself tries to claim `Activity::RefreshExternalMetadata`. If the slot is busy (e.g. a thumbnail update is running, or the user clicked "Update Thumbnails" a second earlier), the spawned refresh logs `"phase_auto_import: another refresh in flight"` and returns. The launchbox tables remain empty. The user sees blank metadata until they click "Update" again, and the cause isn't surfaced.

**Fix**: move the `clear_launchbox` call inside `phase_auto_import_inner` (after the guard is claimed), guarded by a `force_clear: bool` flag that `regenerate_metadata` passes through. The user-visible operation becomes "claim slot or fail loudly", and the clear+refresh stays atomic relative to other activities.

### F-4: `redetect_storage` is not single-flight

**Severity: low (UI flicker + duplicated pool warmup).**

After the watcher simplification, four sources call `reload_config_and_redetect_storage` (which calls `redetect_storage`) without a serializing mutex:

- the `notify` config-file watcher (debounced)
- `mountinfo_watcher` (debounced)
- every user mutation entry, via `require_configured_storage_ready_for_mutation`
- the user-triggered HTTP `refresh_storage` server fn

`redetect_storage` reads `self.storage` under a short read-lock, awaits `probe_storage_ready` (NFS-bound, can take seconds), then takes the write-lock. Two callers that both observe a real change run the full reopen sequence in sequence, double-warming the pools and double-emitting `ConfigEvent::StorageChanged`. On a flapping mount the storage status oscillates `Activating → Ready → Activating → Ready` publicly visible to clients.

(The previous 10 s / 60 s poll was removed; that takes one chronic source off the table but does not close the race between the remaining four.)

Correctness is preserved (each `bump_storage_generation` invalidates its own predecessor's scans), but the duplicated pool reopens are observable as longer "Activating" windows on the UI banner, and any background pipeline that wedged into the gap between two reopens is operating against a pool that will be yanked.

**Fix**: add a `tokio::sync::Mutex<()>` (e.g. `redetect_storage_lock` on `AppState`) and acquire it at the top of `redetect_storage`. Concurrent callers serialize and the second one re-reads the now-current state, almost always returning `Ok(false)` immediately. Every existing call site keeps working without changes.

## 4. Recommended action order

1. **F-1** — silent failure-to-populate after storage swap; small fix (route through existing retry helper).
2. **F-2** — silent metadata wipe; small structural change (clear inside guard).
3. **F-4** — low-severity polish; add a serializing mutex.

Each is a self-contained patch; none depends on the others.

## 5. New writes — checklist

When adding a new write path, decide:

- **Does it write `library.db`?** If yes, claim a relevant `Activity` (Rebuild / Maintenance) before touching the writer pool. Skip only if the write is per-row idempotent (INSERT OR REPLACE) and the column is owned by an L5-class hook.
- **Does it run for more than a couple of seconds?** If yes, capture `storage_generation` at start, plumb it through `ScanInputs`, and check `ensure_current()` before each `try_write(...)` boundary.
- **Is it user-initiated?** Gate with `require_configured_storage_ready_for_mutation` so it fails loudly when storage is misconfigured, instead of silently writing to fallback storage.
- **Does it touch L1 caches?** Caches are independent of activity guards by design; invalidate freely.
- **Is it a destructive lifecycle op (`reset_to_empty`, `reopen`, `replace_with_file`)?** Trust the pool drain; do not invent a parallel mutex.

## 6. Resolved findings

### F-3: region-preference change could wipe an in-progress Rebuild

**Previous severity: high (silent data loss).**

`save_region_preference` and `save_region_preference_secondary` used to call `state.cache.invalidate(&state.library_writer)`, which ran `LibraryDb::clear_all_game_library`. A user changing region while a Rebuild or auto-import re-enrichment pass was mid-flight could truncate rows that the long operation had already written.

The handlers now call only `invalidate_l1()` plus `invalidate_user_caches()`, then run `resolve_release_date_for_library` to rewrite the region-dependent `release_date` mirror columns. Do not restore a library-wide clear in this path; if a future change needs destructive library work, claim an `Activity` guard first.
