# Startup Flow Analysis: Race Conditions and Missing Logic

## Current Startup Sequence

```
main.rs:run()
  |
  +-- AppState::new()
  |     MetadataDb::open()     -- creates/opens metadata.db (empty tables if fresh)
  |     UserDataDb::open()     -- creates/opens userdata.db
  |     RomCache::new()        -- empty in-memory cache (L1)
  |
  +-- spawn_storage_watcher()  -- tokio::spawn; first tick skipped (60s poll)
  |
  +-- spawn_cache_verification()
  |     tokio::spawn -> sleep(2s) -> spawn_blocking:
  |       if metadata_operation_in_progress -> SKIP (return)
  |       load_all_system_meta() from rom_cache_meta (L2)
  |       if empty -> populate_all_systems() [get_roms + enrich per system]
  |       if non-empty -> verify mtimes, re-scan stale systems
  |
  +-- spawn_auto_import()  [runs check SYNCHRONOUSLY on main thread]
  |     metadata_db.is_empty()?  -- checks game_metadata table
  |     if empty AND xml exists:
  |       start_import()  -- sets metadata_operation_in_progress=true
  |                       -- spawns spawn_blocking for import
  |
  +-- register server functions
  +-- build router
  +-- start HTTP server
```

### Import Completion (runs later in background):

```
run_import_blocking()
  |
  +-- build_rom_index()
  +-- import_launchbox() -> writes to game_metadata table ONLY
  +-- put DB connection back into mutex
  +-- invalidate_images()
  +-- if succeeded: spawn_cache_enrichment()
  |     std::thread::spawn:
  |       get_systems() -> L1 empty, L2 empty -> L3 filesystem scan
  |       for each system with games:
  |         enrich_system_cache()
  |           reads rom_filenames from L1 roms cache -> EMPTY -> returns immediately
  |
  +-- sleep(10s) then clear metadata_operation_in_progress
```

## Identified Issues

### Issue 1: Cache Verification Permanently Skipped After Auto-Import (CRITICAL) -- FIXED

**Status:** Fixed in commit `309b8e4` — `spawn_cache_enrichment` now checks if rom_cache is empty and calls `populate_all_systems` when needed.

**Severity:** Critical
**File:** `replay-control-app/src/api/background.rs:31-39`

When `spawn_auto_import` sets `metadata_operation_in_progress = true` before
`spawn_cache_verification` wakes up (2s delay), verification is skipped entirely.
Cache verification is the **only** startup path that calls `populate_all_systems()`,
which is the only function that populates L1+L2 rom caches for all systems.

Once skipped, nothing re-triggers it. The flag clears 10 seconds after import
finishes, but by then no task is waiting.

**Impact:** After DB deletion + restart, systems' ROM lists are not pre-populated.
Users must browse each system individually for its ROMs to appear in the cache.
Recommendations, global search, and genre browsing see empty data for un-visited
systems.

**Proposed fix:** After import completes successfully, run `populate_all_systems()`
before `spawn_cache_enrichment()`. Or better: replace `spawn_cache_enrichment()` with
a new method `spawn_cache_population_and_enrichment()` that does both steps. This is
exactly what `populate_all_systems` already does (get_roms + enrich for each system).

```rust
// In run_import_blocking(), replace:
if succeeded {
    self.spawn_cache_enrichment();
}
// With:
if succeeded {
    self.spawn_full_cache_warmup();
}
```

Where `spawn_full_cache_warmup` calls `populate_all_systems` (which already
includes enrichment).

### Issue 2: spawn_cache_enrichment Reads from Empty L1 Cache (CRITICAL) -- FIXED

**Status:** Fixed in commit `309b8e4` — same fix as Issue 1; when rom_cache is empty, `populate_all_systems` runs `get_roms` (populating L1+L2) before enriching.

**Severity:** Critical
**File:** `replay-control-app/src/api/cache.rs:1046-1059`, `background.rs:148-167`

`enrich_system_cache()` reads ROM filenames exclusively from the L1 in-memory
cache (`self.roms.read()`). If L1 has not been populated for a system,
`rom_filenames` is empty and the function returns immediately without enriching.

`spawn_cache_enrichment` calls `get_systems()` (which does populate the systems
list via L3 fallback) but does NOT call `get_roms()` for individual systems. It
goes straight to `enrich_system_cache()`, which finds empty L1 data.

**Impact:** After auto-import on a fresh DB, enrichment (box art URLs, ratings)
is silently skipped for all systems. The user sees ROMs without cover art until
they browse a system AND another enrichment is triggered.

**Proposed fix:** `spawn_cache_enrichment` should call `get_roms()` for any
system that is missing from the L1 cache before calling `enrich_system_cache()`.
Or, replace it with `populate_all_systems()` which already handles both.

### Issue 3: Metadata Page Stats Depend on rom_cache (HIGH)

**Severity:** High
**File:** `replay-control-core/src/metadata/metadata_db.rs:598-617, 707-727`

Both `entries_per_system()` and `images_per_system()` use `INNER JOIN rom_cache`
to compute per-system counts. If `rom_cache` is empty (because cache verification
was skipped and no systems have been browsed), these queries return 0 for all
systems -- even when `game_metadata` has thousands of entries.

The `get_system_coverage` server function (`server_fns/metadata.rs:58-106`) then
combines these counts with the system list from `get_systems()`. Since
`entries_per_system` returns empty, `with_metadata` is 0 for all systems.

Meanwhile, `get_metadata_stats()` reads directly from `game_metadata` (no join)
and will correctly show total_entries, with_description, with_rating. So the user
sees "12,345 entries imported" but "0 matched to ROMs" per system -- confusing.

**Proposed fix (short-term):** When computing system coverage for display, fall
back to counting `game_metadata` entries per system directly (without the join)
if `rom_cache` appears empty. This gives the user correct import stats even before
the cache is warmed.

**Proposed fix (long-term):** Ensure the rom_cache is always populated before the
metadata page can be accessed (fixes Issue 1).

### Issue 4: 10-Second Flag Delay Creates a Dead Window (MEDIUM)

**Severity:** Medium
**File:** `replay-control-app/src/api/import.rs:319-331`

After import completes, `metadata_operation_in_progress` stays true for 10 more
seconds (to let SSE clients read the terminal state). During this window:

- Any request to `metadata_db()` returns `None` if the DB guard happens to be
  `None` (though in practice the DB was put back, so the guard is `Some` and the
  flag check is bypassed). This is not currently a real issue.
- However, any new metadata operation (manual re-import, thumbnail update) cannot
  start for 10 seconds after the previous one finishes. Not a correctness issue
  but a UX delay.

**Proposed fix:** Use a separate flag for the SSE progress window vs. the "DB
operation in progress" guard. The SSE display delay should not block the DB
availability flag.

### Issue 5: Race Between Auto-Import and Cache Verification Timing (LOW)

**Severity:** Low (currently masked by 2s delay)
**File:** `background.rs:21, 170-210`

`spawn_cache_verification` has a hardcoded 2-second sleep before it checks the
flag. `spawn_auto_import` runs synchronously on the main thread, so `start_import()`
sets the flag before the verification task wakes up. This works by accident --
if the auto-import check became async or the sleep were removed, the race could
flip.

**Current behavior:** The 2s sleep is long enough for the synchronous auto-import
decision + `start_import()` call. This is fragile but not currently broken.

**Proposed fix:** Instead of relying on timing, make cache verification aware of
auto-import. Options:
- Pass a "skip if auto-import just started" signal
- Run cache verification after auto-import decision (sequentially)
- Check the flag in a retry loop with a small delay

### Issue 6: No Post-Browse Enrichment (MEDIUM)

**Severity:** Medium
**File:** `server_fns/roms.rs:63-66`

When a user browses a system page, `get_roms_page` calls `get_roms()` which
populates L1+L2 via an L3 scan. But `enrich_system_cache()` is never called
after this lazy population. The ROM entries are saved to L2 with
`box_art_url: None`.

Enrichment only runs in three places:
1. `populate_all_systems` (startup, if cache is empty)
2. `spawn_cache_enrichment` (after import/thumbnail update)
3. Background verification of stale systems

If a user browses a system after import but before the next enrichment event,
they see ROMs without cover art. Subsequent visits serve the unenriched L2 cache.

**Proposed fix:** After a cache miss + L3 scan in `get_roms`, trigger
`enrich_system_cache` for that system. This can be done asynchronously to avoid
blocking the response.

## Specific Scenario: DB Deletion + Restart

### Preconditions
- metadata.db is deleted (or both metadata.db and any external rom_cache)
- ROMs and thumbnail images exist on disk from a previous session
- `launchbox-metadata.xml` exists in `.replay-control/`

### Current Behavior (step by step)

| Time | Event | Result |
|------|-------|--------|
| T+0 | `AppState::new()` | Creates fresh metadata.db with empty tables |
| T+0 | `spawn_cache_verification()` | Spawns task, will check at T+2s |
| T+0 | `spawn_auto_import()` | Checks `game_metadata` -> empty, XML exists -> calls `start_import()` which sets `metadata_operation_in_progress=true` |
| T+2s | Cache verification wakes | Sees flag=true, **skips entirely** |
| T+30-60s | Import finishes | Writes game_metadata entries. Calls `spawn_cache_enrichment()` |
| T+30-60s | Enrichment runs | `get_systems()` -> L3 scan (populates systems list). For each system: `enrich_system_cache()` -> reads L1 roms -> **empty** -> **returns immediately** |
| T+40-70s | Flag cleared | `metadata_operation_in_progress = false` |
| ... | User browses a system | `get_roms()` -> L3 scan -> populates L1+L2 (but no enrichment) |

### Expected Behavior (ideal)

| Time | Event | Result |
|------|-------|--------|
| T+0 | Same as above | Same |
| T+30-60s | Import finishes | Populates rom_cache for ALL systems (get_roms per system), THEN enriches all with box art/ratings |
| T+30-60s | Metadata page | Shows correct per-system coverage stats |
| ... | User browses any system | Sees ROMs with box art and ratings immediately |

## Proposed Fix Priority

| Priority | Issue | Fix Effort | Impact |
|----------|-------|------------|--------|
| P0 | #1 + #2: Post-import should populate + enrich | Small | **DONE** (commit `309b8e4`) |
| P1 | #3: Metadata stats depend on rom_cache | Small | Fixes confusing stats display |
| P2 | #6: No post-browse enrichment | Medium | Fixes lazy-load box art gap |
| P3 | #4: 10s flag delay | Small | UX polish |
| P4 | #5: Fragile timing | Small | Defensive hardening |

## Recommended Implementation

### P0 Fix: Add `spawn_full_cache_warmup` method

In `background.rs`, add a new method that both populates and enriches, then use
it from import completion:

```rust
/// Populate L1+L2 cache for all systems and enrich with metadata.
/// Used after import completes to ensure the cache is fully warm.
pub fn spawn_full_cache_warmup(&self) {
    let state = self.clone();
    std::thread::spawn(move || {
        let storage = state.storage();
        let region_pref = state.region_preference();
        Self::populate_all_systems(&state, &storage, region_pref);
    });
}
```

In `import.rs:run_import_blocking()`, replace:
```rust
if succeeded {
    self.spawn_cache_enrichment();
}
```
with:
```rust
if succeeded {
    self.spawn_full_cache_warmup();
}
```

This reuses the existing `populate_all_systems` which already does get_roms +
enrich for every system.

### P1 Fix: Add standalone entries_per_system query

In `metadata_db.rs`, add a query that counts `game_metadata` entries per system
without joining rom_cache. Use it as fallback in `get_system_coverage` when the
rom_cache appears empty.

### P2 Fix: Trigger enrichment after cache miss

In `cache.rs:get_roms()`, after an L3 scan (line 320-332), call
`enrich_system_cache` asynchronously. This requires passing the AppState, which
the cache doesn't currently hold. Options:
- Add an enrichment callback to `get_roms`
- Make the server function call enrichment after get_roms
- Use an event/channel to notify the background system

## Files Referenced

- `<WORKSPACE>/replay-control-app/src/main.rs` -- startup sequence
- `<WORKSPACE>/replay-control-app/src/api/mod.rs` -- AppState definition, metadata_db() accessor
- `<WORKSPACE>/replay-control-app/src/api/background.rs` -- spawn_cache_verification, populate_all_systems, spawn_cache_enrichment, spawn_auto_import
- `<WORKSPACE>/replay-control-app/src/api/import.rs` -- start_import, run_import_blocking
- `<WORKSPACE>/replay-control-app/src/api/cache.rs` -- RomCache, get_systems, get_roms, enrich_system_cache
- `<WORKSPACE>/replay-control-app/src/server_fns/metadata.rs` -- get_metadata_stats, get_system_coverage
- `<WORKSPACE>/replay-control-core/src/metadata/metadata_db.rs` -- stats(), entries_per_system(), images_per_system(), is_empty()
