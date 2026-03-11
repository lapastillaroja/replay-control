# SQLite-Based Persistent Cache for Box Art URLs and ROM Enrichment Data

## 1. Problem Analysis

The current hot path in `get_roms_page()` (lines 172-219 of `roms.rs`) does per-ROM serial work:

1. **Box art resolution** (line 172-174): For each of 100 ROMs, calls `resolve_box_art_url()` which does a metadata DB lookup (per-ROM individual query via `db.lookup()`), then optionally falls back to `find_image_on_disk()` which does `read_dir` + fuzzy string matching against every file in the boxart directory.
2. **Rating lookup** (lines 206-219): Already batch-optimized via `lookup_ratings()`, but still does 100 individual `query_row` calls with a prepared statement.
3. **Players lookup** (lines 198-203): Per-ROM calls to `lookup_players()` which does `arcade_db::lookup_arcade_game()` or `game_db::lookup_game()` -- these are in-memory hash-table lookups so they are fast, but still redundant per-request.

The 35% of ROMs without `box_art_path` in the metadata DB trigger `find_image_on_disk()` every single request, which calls `read_dir` on the boxart directory and does fuzzy matching against all files. On NFS, `read_dir` is especially expensive (~50-100ms per call).

## 2. Design Decision: Separate `rom_cache` Table in Existing `metadata.db`

**Rationale**: Using the existing `metadata.db` file avoids a second SQLite connection, second nolock/WAL decision, and second DB handle in AppState. A separate table cleanly separates import-derived data (`game_metadata`) from runtime-resolved cache data (`rom_cache`).

The `game_metadata` table stores data from LaunchBox XML import (descriptions, ratings, publishers, image paths from import). The new `rom_cache` table stores runtime-resolved data (the final URL that `resolve_box_art_url` computed, directory scan timestamps for invalidation).

## 3. SQL Schema

```sql
-- New table: resolved image URL cache + per-ROM enrichment
-- Populated lazily on first request, invalidated by media directory mtime
CREATE TABLE IF NOT EXISTS rom_cache (
    system       TEXT NOT NULL,
    rom_filename TEXT NOT NULL,
    box_art_url  TEXT,           -- resolved URL like "/media/sega_smd/boxart/Sonic The Hedgehog.png"
    screenshot_url TEXT,         -- resolved URL like "/media/sega_smd/snap/Sonic The Hedgehog.png"
    cached_at    INTEGER NOT NULL, -- unix timestamp when this entry was resolved
    PRIMARY KEY (system, rom_filename)
);

-- New table: per-system directory mtime tracking for cache invalidation
CREATE TABLE IF NOT EXISTS cache_mtime (
    system       TEXT NOT NULL,
    dir_kind     TEXT NOT NULL,  -- 'boxart', 'snap', 'roms'
    mtime_secs   INTEGER,       -- directory mtime as unix timestamp
    mtime_nsecs  INTEGER,       -- nanosecond component (for sub-second precision)
    PRIMARY KEY (system, dir_kind)
);
```

**Why not extend `game_metadata`**:
- `game_metadata` has 19,035 entries from LaunchBox import, but there may be ROMs on disk that have no metadata entry at all (no match in LaunchBox). These ROMs still need box art caching.
- `game_metadata` is imported in bulk and cleared/regenerated. We do not want `clear()` on metadata to also blow away the box art cache -- re-resolving it requires re-scanning the filesystem.
- `rom_cache` has different invalidation semantics (mtime-based) vs `game_metadata` (import-based).

## 4. Schema Migration

Add migration in `MetadataDb::init()`, following the existing pattern of idempotent DDL:

```rust
// In metadata_db.rs init(), after existing migrations:
self.conn.execute_batch(
    "CREATE TABLE IF NOT EXISTS rom_cache (
        system       TEXT NOT NULL,
        rom_filename TEXT NOT NULL,
        box_art_url  TEXT,
        screenshot_url TEXT,
        cached_at    INTEGER NOT NULL,
        PRIMARY KEY (system, rom_filename)
    );
     CREATE TABLE IF NOT EXISTS cache_mtime (
        system   TEXT NOT NULL,
        dir_kind TEXT NOT NULL,
        mtime_secs  INTEGER,
        mtime_nsecs INTEGER,
        PRIMARY KEY (system, dir_kind)
    );"
)?;
```

This is fully backwards-compatible: `CREATE TABLE IF NOT EXISTS` is a no-op on existing databases, and old code never queries these tables.

## 5. New Methods on `MetadataDb`

Add to `replay-control-core/src/metadata_db.rs`:

**a) Batch box art URL lookup**:
```rust
/// Batch look up cached box art URLs for a list of ROMs on a single system.
/// Returns a map of rom_filename -> box_art_url.
pub fn lookup_cached_box_art(
    &self,
    system: &str,
    rom_filenames: &[&str],
) -> Result<HashMap<String, Option<String>>> {
    // Use a single prepared statement, iterate filenames
    let mut stmt = self.conn.prepare(
        "SELECT rom_filename, box_art_url FROM rom_cache
         WHERE system = ?1 AND rom_filename = ?2"
    )?;
    let mut map = HashMap::new();
    for filename in rom_filenames {
        if let Some((name, url)) = stmt.query_row(
            params![system, filename],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        ).optional()? {
            map.insert(name, url);
        }
    }
    Ok(map)
}
```

**b) System-wide box art lookup (for full-system cache warm)**:
```rust
/// Fetch all cached box art URLs for a system in one query.
pub fn system_cached_box_art(
    &self,
    system: &str,
) -> Result<HashMap<String, Option<String>>> {
    let mut stmt = self.conn.prepare(
        "SELECT rom_filename, box_art_url FROM rom_cache WHERE system = ?1"
    )?;
    let rows = stmt.query_map(params![system], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
    })?;
    let mut map = HashMap::new();
    for row in rows.flatten() {
        map.insert(row.0, row.1);
    }
    Ok(map)
}
```

**c) Bulk upsert for cache population**:
```rust
/// Bulk insert/update cached image URLs within a single transaction.
/// Each entry is (system, rom_filename, box_art_url, screenshot_url).
pub fn bulk_upsert_rom_cache(
    &mut self,
    entries: &[(String, String, Option<String>, Option<String>)],
) -> Result<usize> {
    let tx = self.conn.transaction()?;
    let now = unix_now();
    let mut count = 0;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO rom_cache (system, rom_filename, box_art_url, screenshot_url, cached_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(system, rom_filename) DO UPDATE SET
                box_art_url = excluded.box_art_url,
                screenshot_url = excluded.screenshot_url,
                cached_at = excluded.cached_at"
        )?;
        for (system, filename, boxart, snap) in entries {
            stmt.execute(params![system, filename, boxart, snap, now])?;
            count += 1;
        }
    }
    tx.commit()?;
    Ok(count)
}
```

**d) Mtime tracking**:
```rust
/// Get the stored mtime for a system+dir_kind pair.
pub fn get_cache_mtime(&self, system: &str, dir_kind: &str) -> Result<Option<(i64, i64)>> {
    self.conn.query_row(
        "SELECT mtime_secs, mtime_nsecs FROM cache_mtime WHERE system = ?1 AND dir_kind = ?2",
        params![system, dir_kind],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
    ).optional().map_err(|e| Error::Other(format!("mtime lookup: {e}")))
}

/// Store the mtime for a system+dir_kind pair.
pub fn set_cache_mtime(&self, system: &str, dir_kind: &str, secs: i64, nsecs: i64) -> Result<()> {
    self.conn.execute(
        "INSERT INTO cache_mtime (system, dir_kind, mtime_secs, mtime_nsecs)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(system, dir_kind) DO UPDATE SET
            mtime_secs = excluded.mtime_secs,
            mtime_nsecs = excluded.mtime_nsecs",
        params![system, dir_kind, secs, nsecs],
    )?;
    Ok(())
}
```

**e) Cache invalidation**:
```rust
/// Clear the rom_cache for a specific system.
pub fn clear_system_rom_cache(&self, system: &str) -> Result<usize> {
    let count = self.conn.execute(
        "DELETE FROM rom_cache WHERE system = ?1",
        params![system],
    )?;
    self.conn.execute(
        "DELETE FROM cache_mtime WHERE system = ?1",
        params![system],
    )?;
    Ok(count)
}

/// Clear all rom_cache entries.
pub fn clear_rom_cache(&self) -> Result<()> {
    self.conn.execute("DELETE FROM rom_cache", [])?;
    self.conn.execute("DELETE FROM cache_mtime", [])?;
    Ok(())
}
```

## 6. Read Path Changes

### 6a. `get_roms_page()` in `replay-control-app/src/server_fns/roms.rs`

Replace the per-ROM `resolve_box_art_url` loop (lines 172-174) with a batch approach:

```rust
// BEFORE (lines 172-174):
for rom in &mut roms {
    rom.box_art_url = resolve_box_art_url(&state, &system, &rom.game.rom_filename);
}

// AFTER:
populate_box_art_urls(&state, &system, &mut roms);
```

New function `populate_box_art_urls` in `server_fns/mod.rs`:

```rust
#[cfg(feature = "ssr")]
pub(crate) fn populate_box_art_urls(
    state: &crate::api::AppState,
    system: &str,
    roms: &mut [RomEntry],
) {
    let storage = state.storage();
    let media_base = storage.rc_dir().join("media").join(system);
    let boxart_dir = media_base.join("boxart");

    // 1. Check if the boxart directory mtime matches the cached mtime.
    //    If it does, all cached entries are still valid.
    let dir_mtime = std::fs::metadata(&boxart_dir)
        .ok()
        .and_then(|m| m.modified().ok());

    let cache_valid = if let Some(guard) = state.metadata_db() {
        if let Some(db) = guard.as_ref() {
            match (dir_mtime, db.get_cache_mtime(system, "boxart").ok().flatten()) {
                (Some(current), Some((cached_secs, cached_nsecs))) => {
                    let current_dur = current.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
                    current_dur.as_secs() as i64 == cached_secs
                        && current_dur.subsec_nanos() as i64 == cached_nsecs
                }
                (None, _) => true, // Can't read dir mtime (NFS flake), trust cache
                _ => false,
            }
        } else {
            false
        }
    } else {
        false
    };

    // 2. If cache is valid, do a batch lookup from rom_cache table.
    if cache_valid {
        if let Some(guard) = state.metadata_db() {
            if let Some(db) = guard.as_ref() {
                let filenames: Vec<&str> = roms.iter().map(|r| r.game.rom_filename.as_str()).collect();
                if let Ok(cached) = db.lookup_cached_box_art(system, &filenames) {
                    let mut misses = Vec::new();
                    for rom in roms.iter_mut() {
                        if let Some(url) = cached.get(&rom.game.rom_filename) {
                            rom.box_art_url = url.clone();
                        } else {
                            misses.push(rom.game.rom_filename.clone());
                        }
                    }
                    // For cache misses (new ROMs added since last cache), fall back to per-ROM resolution.
                    if !misses.is_empty() {
                        resolve_and_cache_misses(state, system, roms, &misses);
                    }
                    return;
                }
            }
        }
    }

    // 3. Cache invalid or unavailable: resolve all per-ROM, then persist to cache.
    let mut updates = Vec::new();
    for rom in roms.iter_mut() {
        let url = resolve_box_art_url(state, system, &rom.game.rom_filename);
        updates.push((
            system.to_string(),
            rom.game.rom_filename.clone(),
            url.clone(),
            None::<String>, // screenshot_url not resolved here
        ));
        rom.box_art_url = url;
    }
    // Persist to DB (fire-and-forget for this request's latency).
    persist_box_art_cache(state, system, &updates, dir_mtime);
}
```

**Key insight**: The mtime check is a single `stat()` call on the boxart directory. If it matches, all 100 box art URLs come from a single batch query (or preferably a `system_cached_box_art` full-system query). Only on mtime mismatch (new images added) do we fall back to per-ROM resolution.

### 6b. `get_recents()` in `server_fns/system.rs`

Similar pattern but for a small number of entries (typically 10-20 recent games across multiple systems). Here the overhead is lower, so a simpler approach works:

```rust
// Group recents by system, batch-lookup per system
let mut by_system: HashMap<String, Vec<usize>> = HashMap::new();
for (i, entry) in entries.iter().enumerate() {
    by_system.entry(entry.game.system.clone()).or_default().push(i);
}
// For each system, check cache validity and batch-lookup
```

This is lower priority since recents are typically < 20 items.

### 6c. `enrich_from_metadata_cache()` in `server_fns/mod.rs`

For the detail page (`get_rom_detail`), the current per-ROM approach is fine since it's a single ROM. However, we should still check `rom_cache` first before falling back to the filesystem scan:

```rust
// In enrich_from_metadata_cache, after metadata DB lookup:
// Check rom_cache for pre-resolved URLs before filesystem fallback
if info.box_art_url.is_none() {
    if let Some(guard) = state.metadata_db() {
        if let Some(db) = guard.as_ref() {
            if let Ok(cached) = db.lookup_cached_box_art(&info.system, &[&info.rom_filename]) {
                if let Some(url) = cached.get(&info.rom_filename) {
                    info.box_art_url = url.clone();
                }
            }
        }
    }
}
// Then filesystem fallback (existing code), with persist-on-resolve
```

## 7. Write Path

### 7a. Lazy Population (on first request for a system)

When `populate_box_art_urls` finds the cache invalid or empty for a system, it resolves URLs per-ROM as today, but then writes all results to `rom_cache` in a single transaction. This means the first page load is the same speed as today, but subsequent loads are fast.

The `persist_box_art_cache` helper:

```rust
#[cfg(feature = "ssr")]
fn persist_box_art_cache(
    state: &crate::api::AppState,
    system: &str,
    updates: &[(String, String, Option<String>, Option<String>)],
    dir_mtime: Option<std::time::SystemTime>,
) {
    if let Some(mut guard) = state.metadata_db() {
        if let Some(db) = guard.as_mut() {
            if let Err(e) = db.bulk_upsert_rom_cache(updates) {
                tracing::debug!("Failed to persist box art cache: {e}");
            }
            // Update mtime
            if let Some(mtime) = dir_mtime {
                let dur = mtime.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
                let _ = db.set_cache_mtime(
                    system,
                    "boxart",
                    dur.as_secs() as i64,
                    dur.subsec_nanos() as i64,
                );
            }
        }
    }
}
```

**Note on `&mut` vs `&`**: The `bulk_upsert_rom_cache` needs `&mut self` for the transaction. The current `metadata_db()` returns `MutexGuard<Option<MetadataDb>>` which gives mutable access through `guard.as_mut()`. This works because the Mutex already serializes access.

### 7b. After Image Import

When `import_system_thumbnails` or `rematch_system_images_blocking` completes, it already calls `bulk_update_image_paths` on `game_metadata`. We should also invalidate the `rom_cache` for that system:

```rust
// In import.rs, after db.bulk_update_image_paths():
let _ = db.clear_system_rom_cache(system);
```

This forces the next `get_roms_page` to re-resolve (which will then re-populate from the now-updated `game_metadata` + filesystem).

### 7c. After Metadata Import

When `run_import_blocking` completes (LaunchBox import), the `game_metadata` table has new `box_art_path` entries. Clear all `rom_cache`:

```rust
// At end of run_import_blocking, before putting DB back:
let _ = db.clear_rom_cache();
```

### 7d. On Storage Change

When `refresh_storage()` detects a change, `cache.invalidate()` clears the in-memory caches. The SQLite `rom_cache` is tied to a specific storage root, and since `MetadataDb` is at `<storage_root>/.replay-control/metadata.db`, switching storage inherently means a different DB file, so no explicit invalidation is needed.

## 8. Cold Start Optimization

Currently `scan_systems()` in `replay-control-core/src/roms.rs` (line 52) calls `count_roms_recursive()` for each of ~46 systems, doing `read_dir` calls. This is not related to box art but is a significant startup cost on NFS.

**Phase 2 (out of scope for initial implementation)**: Add a `system_summary_cache` table to persist the per-system game counts and sizes. The `RomCache::get_systems()` method would check the DB first, then verify with a single `stat` on each system directory.

For this plan, we focus only on the box art cache. The cold start for systems can use the existing in-memory `RomCache` which already has mtime-based invalidation with 5-minute TTL.

## 9. NFS Considerations

- The `MetadataDb` already handles NFS with `nolock` VFS fallback -- no changes needed.
- Directory mtime on NFS: NFS v3 and v4 both report mtime, but NFS v3 has 1-second granularity. Using `(secs, nsecs)` in `cache_mtime` handles both NFS v3 and local filesystems correctly.
- NFS attribute caching: NFS clients cache `stat` results for a configurable period (default `acregmin=3`, `acdirmin=30`). This means a `stat` on the boxart directory might not reflect changes for up to 30 seconds. This is acceptable for this use case -- the hard TTL in the in-memory `RomCache` (5 minutes) already provides a coarser invalidation window.
- On NFS, `read_dir` for fuzzy matching is the real bottleneck (~50-100ms per call). The cache eliminates this for all subsequent requests after the first.

## 10. Code Flow Diagrams

### Read Path (Hot Path: `get_roms_page`)

```
get_roms_page(system, offset=0, limit=100)
  |
  +-> cache.get_roms()          [in-memory, mtime-checked]
  +-> filter/search/sort
  +-> take page slice (100 items)
  |
  +-> populate_box_art_urls(state, system, &mut roms)
       |
       +-> stat(boxart_dir)                      [1 syscall]
       +-> db.get_cache_mtime(system, "boxart")  [1 query]
       |
       +-- mtime matches? ----YES----> db.system_cached_box_art(system)  [1 query, returns ALL]
       |                                |
       |                                +-> fill roms from cache
       |                                +-> any misses? -> resolve per-ROM, persist
       |                                +-> DONE
       |
       +-- mtime differs? ----NO-----> resolve per-ROM (existing logic)
                                        |
                                        +-> persist to rom_cache (1 transaction)
                                        +-> update cache_mtime
                                        +-> DONE
```

### Write Path (After Image Import)

```
import_system_thumbnails()
  |
  +-> copy images to media/system/boxart/
  +-> db.bulk_update_image_paths()   [game_metadata]
  +-> db.clear_system_rom_cache()    [invalidate rom_cache for system]
  |
  (next get_roms_page will re-resolve and re-populate rom_cache)
```

## 11. Files to Change

| File | Change |
|------|--------|
| `replay-control-core/src/metadata_db.rs` | Add `rom_cache` and `cache_mtime` table creation in `init()`. Add 7 new methods: `lookup_cached_box_art`, `system_cached_box_art`, `bulk_upsert_rom_cache`, `get_cache_mtime`, `set_cache_mtime`, `clear_system_rom_cache`, `clear_rom_cache`. |
| `replay-control-app/src/server_fns/mod.rs` | Add `populate_box_art_urls()` and `persist_box_art_cache()` helper functions. Optionally update `enrich_from_metadata_cache()` to check `rom_cache` before filesystem fallback. |
| `replay-control-app/src/server_fns/roms.rs` | Replace per-ROM `resolve_box_art_url` loop with call to `populate_box_art_urls`. |
| `replay-control-app/src/api/import.rs` | Add `db.clear_system_rom_cache(system)` after image import, `db.clear_rom_cache()` after metadata import. |
| `replay-control-app/src/server_fns/system.rs` | Optionally batch box art for `get_recents()` (lower priority). |

## 12. Estimated Performance Impact

**Warm cache (typical page load)**:
- Before: 100 individual DB queries + ~35 `read_dir` calls with fuzzy matching = ~200-500ms on NFS, ~50-100ms on USB
- After: 1 `stat` + 1 `SELECT ... WHERE system = ?` returning ~100 rows = ~2-5ms on any storage
- **Speedup: 40-100x for the box art portion**

**Cold cache (first request after server start or cache invalidation)**:
- Same as today (per-ROM resolution), plus ~5ms overhead to persist results
- Subsequent requests benefit from the warm cache

**Cache invalidation (after image import)**:
- One extra `DELETE FROM rom_cache WHERE system = ?` per system (~1ms)
- Next page load re-populates (~same cost as today's first request)

**Memory impact**:
- Negligible. The cache lives in SQLite, not in memory. The in-memory `RomCache` for ROM listings is unchanged.

**DB size impact**:
- ~100 bytes per row * ~20,000 ROMs = ~2MB additional DB size. Negligible compared to the 460MB LaunchBox XML.

## 13. Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| NFS stat caching hides mtime changes | Medium | Low | Acceptable -- changes propagate within NFS cache timeout (30s default). Users can force-refresh. The existing in-memory cache has the same limitation. |
| Stale cache after manual filesystem changes | Low | Low | The 5-minute hard TTL in the in-memory `RomCache` triggers a rescan. The SQLite mtime check catches directory-level changes. |
| Schema migration on existing installations | Very Low | Very Low | `CREATE TABLE IF NOT EXISTS` is idempotent and backward-compatible. No data migration needed. |
| Mutex contention on metadata_db | Low | Low | The existing `Mutex<Option<MetadataDb>>` already serializes all DB access. The batch queries reduce the number of lock acquisitions (1 instead of 100). |
| Transaction failure on NFS with nolock | Low | Medium | Wrapped in error handling. Fallback is the existing per-ROM resolution (no degradation vs status quo). |
| `system_cached_box_art` returns more rows than needed for the page slice | Low | Low | Even for a system with 5000 ROMs, returning all rows from `rom_cache` is ~0.5MB and takes <10ms. This is much cheaper than 100 individual `find_image_on_disk` calls. |

## 14. Implementation Sequence

1. **Phase 1**: Add schema + methods to `metadata_db.rs` (can be tested independently)
2. **Phase 2**: Add `populate_box_art_urls` + `persist_box_art_cache` to `server_fns/mod.rs`
3. **Phase 3**: Wire into `get_roms_page` in `roms.rs`
4. **Phase 4**: Add cache invalidation in `import.rs`
5. **Phase 5**: (Optional) Batch box art for `get_recents()` in `system.rs`
6. **Phase 6**: (Future) Persist system summaries for cold start optimization

### Critical Files for Implementation
- `<WORKSPACE>/replay-control-core/src/metadata_db.rs` - Core schema changes: new tables, 7 new query/write methods, migration in init()
- `<WORKSPACE>/replay-control-app/src/server_fns/mod.rs` - New batch resolution functions: populate_box_art_urls(), persist_box_art_cache(), mtime comparison logic
- `<WORKSPACE>/replay-control-app/src/server_fns/roms.rs` - Wire the batch approach into get_roms_page() replacing the per-ROM loop
- `<WORKSPACE>/replay-control-app/src/api/import.rs` - Cache invalidation hooks after image/metadata imports
- `<WORKSPACE>/replay-control-app/src/api/cache.rs` - Reference for existing mtime-based invalidation pattern to follow consistently
