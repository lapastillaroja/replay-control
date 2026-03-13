# Recommendations Performance Optimization Plan

> **Status:** Implemented. Recommendations now use the SQLite game library (no per-ROM filesystem I/O). Genre/multiplayer aggregation runs via SQL queries against `game_library`. Box art URLs are cached in the `game_library` table. Client-side loading (non-blocking SSR) is implemented.

## 1. Root Cause Breakdown

The `get_recommendations()` server function at `<WORKSPACE>/replay-control-app/src/server_fns/recommendations.rs` performs three categories of expensive work, each with different cost profiles.

### Operation Call Graph and Costs

**Phase A: Random Picks (lines 59-212)**

The random picks loop calls `state.cache.get_roms()` up to `count * 4` times (24 times for count=6), once per system attempted. On the first call for each system, `get_roms()` triggers `list_roms()` which performs a recursive filesystem scan (`collect_roms_recursive` + `apply_m3u_dedup` + sort). On subsequent calls, the cache returns a clone of the `Vec<RomEntry>`.

Per-system cost breakdown on NFS:
- **Cache miss (first load)**: `std::fs::read_dir` recursively + `metadata()` per file. Over NFS, each filesystem operation incurs 1-5ms of network latency. A system with 500 ROMs generates ~1500 syscalls (read_dir entries + metadata per file + M3U parsing). Estimated: **500ms-2s per system**.
- **Cache hit**: Clones a `Vec<RomEntry>`. For a system with 500 entries, this is a heap allocation + memcpy. Estimated: **<1ms**.
- `resolve_box_art_url()` (line 129): Called per picked game. This does a metadata DB lookup (fast, ~0.1ms) followed by filesystem checks (`is_valid_image` = stat call, `find_image_on_disk` = `read_dir` + multiple stat calls). Over NFS: **5-50ms per game** depending on image directory size. For 6 picks: **30-300ms**.

**Phase B: Genre/Multiplayer Aggregation (lines 214-241) -- THE MAIN BOTTLENECK**

This is the critical section. Lines 215-233 iterate over **every system** and **every ROM in every system**:

```rust
for sys in &systems {
    if sys.game_count == 0 { continue; }
    let roms = match state.cache.get_roms(&storage, &sys.folder_name, region_pref) { ... };
    for rom in &roms {
        let genre = super::search::lookup_genre(&sys.folder_name, &rom.game.rom_filename);
        let players = super::search::lookup_players(&sys.folder_name, &rom.game.rom_filename);
    }
}
```

For a library with ~20 systems containing games out of 44 total:
- **`get_roms()` x 20 systems**: If caches are cold (first page load after startup), each triggers a full filesystem scan. Estimated: **10-40 seconds total over NFS** (20 systems x 500ms-2s each). If caches are warm: **<20ms total**.
- **`lookup_genre()` + `lookup_players()` per ROM**: Each calls `systems::find_system()` (linear scan of ~44 systems, fast), then `game_db::lookup_game()` or `arcade_db::lookup_arcade_game()` (PHF hash lookup, ~100ns). But for the fallback path, `lookup_genre` also calls `game_db::normalize_filename()` + `lookup_by_normalized_title()`. For a library of 10,000 ROMs across 20 systems, this is **20,000 DB lookups**. Even at 1us each, that is only ~20ms. This is not the bottleneck.
- The real cost is in the **cloning**: `get_roms()` returns `Vec<RomEntry>` by clone. Each `RomEntry` contains multiple heap-allocated strings (`rom_filename`, `rom_path`, `display_name`, `system`, `system_display`). For 10,000 ROMs, the clone allocations alone are significant: estimated **50-200ms** depending on library size.

**Phase C: Favorites-Based Picks (lines 244-246, function at 266-415)**

- `list_favorites()`: Filesystem scan of `_favorites/` directory. Over NFS: **50-500ms** depending on favorites count.
- `get_roms()` for the top system: Single system, likely already cached from Phase B. **<1ms** if warm.
- `lookup_genre()` per ROM for genre scoring: Same as above, fast per-call.
- `metadata_db.system_ratings()`: Single SQLite query. **<5ms**.

**Phase D: Top-Rated Picks (lines 249-251, function at 420-578)**

- `metadata_db.all_ratings()`: Scans entire `game_metadata` table. For a large DB: **10-50ms**.
- `get_roms()` per system to verify ROM existence: Multiple cache calls. If warm: **<1ms each**.

### Total Estimated Cost

| Scenario | Cold Cache (NFS) | Warm Cache (NFS) | Warm Cache (USB/SD) |
|---|---|---|---|
| Phase A: Random Picks | 2-10s | 30-300ms | 5-50ms |
| Phase B: Genre/Multiplayer | **10-40s** | **200-500ms** | **50-200ms** |
| Phase C: Favorites | 0.5-2s | <10ms | <5ms |
| Phase D: Top Rated | 0.1-1s | 10-50ms | 5-20ms |
| **Total** | **13-53s** | **250-860ms** | **65-275ms** |

The cold-cache NFS scenario is the one that causes the SSR hang. The first load after server startup triggers filesystem scans for all ~20 systems with games over NFS, which can take **30+ seconds**. Even with warm caches, the Genre/Multiplayer aggregation phase is the dominant cost due to cloning all ROMs for all systems.

## 2. Data Dependencies

What recommendations actually need:

| Data | Used By | Source | Pre-computable? |
|---|---|---|---|
| Random N games with box art | Random Picks | ROM cache (any system) | No (random each time, but cheap if cache is warm) |
| Genre counts across all systems | Discover links | game_db + arcade_db per ROM | **Yes** -- only changes when library changes |
| Multiplayer count | Discover links | game_db + arcade_db per ROM | **Yes** -- only changes when library changes |
| Favorites per system | Favorites picks | Favorites filesystem | **Yes** -- only changes on favorite add/remove |
| Top-rated games | Top Rated picks | metadata_db | **Yes** -- only changes on metadata import |

The genre counts and multiplayer count are the most expensive to compute yet the most stable. They only change when a user adds/removes ROMs -- which happens rarely compared to page loads.

## 3. Recommended Optimization Strategy

After analyzing all five strategies, the recommended approach is a combination of **(a) client-side lazy loading** and **(b) pre-computed stats cache**, because:

1. **Client-side loading unblocks SSR immediately** -- zero cost to the home page's initial HTML render
2. **Pre-computed stats eliminate the all-systems scan** -- the most expensive operation becomes a cache lookup
3. Both are independently valuable and composable
4. Neither requires a complex background scheduler

Here is the concrete design:

### Strategy 1: Move recommendations to client-side resource (quick win)

Change the Leptos `Resource` to use `send_wrapper::SendWrapper` with `blocking=false` so it never executes during SSR. In Leptos 0.7, this means using a `Resource` that returns `Ok(None)` on the server and fetches via the server function on the client after hydration.

The simplest approach: wrap the recommendation resource in `Effect::new` so it only fires client-side, or use `#[cfg(not(feature = "ssr"))]` to gate the resource creation. On SSR, render the recommendation sections as empty placeholders.

This alone solves the hang with zero backend changes. Estimated effort: 30 minutes.

### Strategy 2: Pre-computed aggregate stats (eliminates the hot loop)

Add a new field to `GameLibrary`:

```rust
struct AggregateStats {
    genre_counts: Vec<(String, usize)>,   // sorted by count descending
    multiplayer_count: usize,
    computed_at: Instant,
}
```

Compute this lazily on first request (after the ROM cache for each system is warm), then cache it. Invalidate when any system's ROM cache is invalidated. The computation reuses the already-cached ROM data (no filesystem I/O) and only calls the fast PHF lookups.

The `get_recommendations()` function would then:
1. Read `genre_counts` and `multiplayer_count` from the stats cache (O(1))
2. Pick random games from already-cached system ROM lists (O(count))
3. Resolve box art for the 6-12 picked games only (O(count))
4. Skip the all-systems iteration entirely

### Strategy 3: Separate the expensive aggregation from the per-request work

Split `get_recommendations()` into two endpoints:
- `get_recommendation_picks(count)`: Returns random picks + favorites picks + top rated. Only accesses 1-3 systems. Fast.
- `get_discover_stats()`: Returns genre counts + multiplayer count. Uses the pre-computed stats cache. Instant if warm.

The home page loads picks immediately and stats in parallel, but neither blocks SSR.

## 4. Architecture Decision

**Recommended architecture**: Client-side resource with pre-computed stats cache.

The recommendations resource should NOT block SSR. It should be loaded after hydration via client-side fetch. The visual impact is minimal -- the recommendation sections appear ~100ms after the page loads, which feels like smooth progressive loading.

Pre-computed stats should be stored in `GameLibrary` alongside the existing systems/roms caches, using the same mtime-based invalidation pattern. This is consistent with the existing architecture and avoids introducing a background scheduler.

## 5. Integration with Existing Cache Infrastructure

The `GameLibrary` at `<WORKSPACE>/replay-control-app/src/api/cache.rs` already has the right patterns:

- `RwLock`-protected entries with mtime-based + TTL invalidation
- `invalidate()` and `invalidate_system()` methods called after mutations
- The aggregate stats cache would follow the same pattern

If a SQLite-based ROM cache is being designed in parallel, genre and player data could be pre-computed into the database during the import/scan phase. The `game_metadata` table already has a system+rom_filename primary key. Adding `genre` and `players` columns (or a separate `rom_stats` table) would allow:

```sql
SELECT genre, COUNT(*) FROM rom_stats
WHERE genre != '' GROUP BY genre ORDER BY COUNT(*) DESC LIMIT 4;

SELECT COUNT(*) FROM rom_stats WHERE players >= 2;
```

These queries execute in <1ms on SQLite. This would make `get_discover_stats()` a pure database operation with no iteration.

## 6. Concrete Implementation Steps

**Step 1 (unblock SSR)**:
- In `home.rs`, gate the recommendations `Resource` so it only fetches client-side. Use Leptos 0.7's `Resource::new` with a closure that returns `None` during SSR. The recommendation sections render as empty `<div>` placeholders during SSR and populate after hydration.
- File: `<WORKSPACE>/replay-control-app/src/pages/home.rs`

**Step 2 (add aggregate stats cache)**:
- Add `AggregateStats` struct and cache field to `GameLibrary` in `cache.rs`.
- Add `get_aggregate_stats()` method that computes genre counts and multiplayer count by iterating already-cached ROM data. Uses a `RwLock<Option<CacheEntry<AggregateStats>>>` with mtime-based invalidation against the main roms directory.
- File: `<WORKSPACE>/replay-control-app/src/api/cache.rs`

**Step 3 (refactor the server function)**:
- Remove the all-systems iteration loop (lines 214-241 of `recommendations.rs`).
- Replace with a call to `state.cache.get_aggregate_stats(&storage)`.
- The random picks and favorites logic remain unchanged (they already use per-system cache hits).
- File: `<WORKSPACE>/replay-control-app/src/server_fns/recommendations.rs`

**Step 4 (optimize box art resolution)**:
- For the 6-12 recommendation picks, `resolve_box_art_url()` currently does filesystem I/O per game. If the metadata DB has `box_art_path` populated, use that first (fast SQLite lookup). The existing code already does this but falls through to a `read_dir` + fuzzy scan. Consider batch-loading box art paths from the metadata DB for all picked games in one query.
- File: `<WORKSPACE>/replay-control-app/src/server_fns/mod.rs`

**Step 5 (warm cache eagerly)**:
- In the `spawn_storage_watcher` background task, after detecting storage, pre-warm the ROM cache for all systems by calling `get_roms()` for each system sequentially. This moves the cold-cache cost to startup time rather than first page load.
- File: `<WORKSPACE>/replay-control-app/src/api/background.rs`

### Critical Files for Implementation
- `<WORKSPACE>/replay-control-app/src/pages/home.rs` - Must re-add recommendation sections with client-side-only resource loading to avoid SSR blocking
- `<WORKSPACE>/replay-control-app/src/api/cache.rs` - Add AggregateStats cache (genre counts, multiplayer count) following existing RwLock+mtime pattern
- `<WORKSPACE>/replay-control-app/src/server_fns/recommendations.rs` - Refactor to use pre-computed stats cache instead of all-systems iteration loop
- `<WORKSPACE>/replay-control-app/src/api/background.rs` - Add eager cache warming on startup to eliminate cold-cache penalty on first page load
- `<WORKSPACE>/replay-control-app/src/server_fns/mod.rs` - Optimize resolve_box_art_url for batch lookups in recommendation context
