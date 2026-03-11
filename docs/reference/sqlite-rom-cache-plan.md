# Persistent SQLite ROM Cache — Implementation Plan

## Goal

Add a persistent L2 cache (SQLite) between the in-memory L1 cache (`RomCache`) and L3 filesystem scans. This eliminates cold-start penalties (430ms NFS / 95ms USB for systems, 4200ms for arcade_mame) and enables SQL-based queries for recommendations.

## Architecture

```
Request → L1 (in-memory RomCache) → L2 (SQLite rom_cache) → L3 (filesystem scan)
                                      ↑ write-through ←──────────┘
```

- **L1 hit**: Same as today — mtime + 5min TTL, ~0ms
- **L2 hit**: SQLite query, ~2-5ms. Loaded into L1 for subsequent requests
- **L3 miss**: Full filesystem scan + enrichment, written to both L2 and L1
- **Cold start**: L1 is empty → L2 serves immediately → background re-validates

## Schema

Extends existing `metadata.db` with two new tables:

```sql
CREATE TABLE IF NOT EXISTS rom_cache (
    system TEXT NOT NULL,
    rom_filename TEXT NOT NULL,
    rom_path TEXT NOT NULL,
    display_name TEXT,
    size_bytes INTEGER NOT NULL DEFAULT 0,
    is_m3u INTEGER NOT NULL DEFAULT 0,
    box_art_url TEXT,
    driver_status TEXT,
    genre TEXT,
    players INTEGER,
    rating REAL,
    PRIMARY KEY (system, rom_filename)
);

CREATE TABLE IF NOT EXISTS rom_cache_meta (
    system TEXT PRIMARY KEY,
    dir_mtime_secs INTEGER,
    scanned_at INTEGER NOT NULL,
    rom_count INTEGER NOT NULL DEFAULT 0,
    total_size_bytes INTEGER NOT NULL DEFAULT 0
);
```

## Phases

### Phase 1: Schema + Core DB Methods (`metadata_db.rs`)

New structs: `CachedRom`, `CachedSystemMeta`

New methods on `MetadataDb`:
- `save_system_roms(system, roms, dir_mtime_secs)` — transactional bulk upsert
- `load_system_roms(system) -> Vec<CachedRom>` — load all cached ROMs for a system
- `load_system_meta(system) -> Option<CachedSystemMeta>` — per-system cache metadata
- `load_all_system_meta() -> Vec<CachedSystemMeta>` — all systems' cache metadata
- `update_rom_enrichment(system, enrichments)` — batch update box_art_url, genre, players, rating, driver_status
- `clear_system_rom_cache(system)` — delete one system from rom_cache + rom_cache_meta
- `clear_all_rom_cache()` — delete all

### Phase 2: Cache Write-Through (`cache.rs`)

- `RomCache` stores `Arc<Mutex<Option<MetadataDb>>>` (shared with `AppState`)
- After L3 filesystem scan in `get_roms()`, write results to SQLite via `save_system_roms()`
- After `get_systems()` scan, write system summaries via `save_system_roms()` (meta only)
- Lock ordering: never hold L1 RwLock and DB Mutex simultaneously

### Phase 3: Cache Read-Through (`cache.rs`)

- On L1 miss in `get_roms()`, try `load_system_roms()` from SQLite before filesystem scan
- Check `rom_cache_meta.dir_mtime_secs` against current filesystem mtime (one stat call)
- If L2 fresh: convert `CachedRom` → `RomEntry`, store in L1, return
- If L2 stale: fall through to L3 filesystem scan
- On L1 miss in `get_systems()`, try `load_all_system_meta()` to reconstruct `SystemSummary` list

### Phase 4: Background Mtime Verification (`background.rs`)

- On startup, spawn a one-shot background task (after 2s delay)
- For each system in `rom_cache_meta`, compare `dir_mtime_secs` with current filesystem mtime
- Re-scan stale systems in the background (writes to L1 + L2)
- Non-blocking: uses `tokio::task::spawn_blocking` for filesystem I/O

### Phase 5: SQL-Based Recommendations (`metadata_db.rs`)

New query methods (for future recommendation optimization):
- `random_cached_roms(system, count)` — random ROMs from a system (with box art)
- `top_rated_cached_roms(count)` — highest rated ROMs across all systems
- `genre_counts()` — `SELECT genre, COUNT(*) ... GROUP BY genre`
- `multiplayer_count()` — `SELECT COUNT(*) WHERE players >= 2`
- `system_roms_excluding(system, exclude_filenames, count)` — for favorites-based picks

### Phase 6: Invalidation Wiring

- `delete_rom()` / `rename_rom()` → `clear_system_rom_cache(system)` + existing `invalidate_system()`
- `refresh_storage()` (storage changed) → `clear_all_rom_cache()` + existing `invalidate()`
- Image import completion → batch `update_rom_enrichment()` for box_art_url
- `invalidate()` and `invalidate_system()` updated to also clear L2

## Filesystem Change Detection

- **Mechanism**: Directory mtime (single `stat()` call per system directory)
- **Granularity**: Per-system. Adding/removing a ROM in `roms/nintendo_nes/` updates that directory's mtime
- **Hard TTL**: 5 minutes (same as L1). Even if mtime check passes, re-scan after TTL
- **Limitations**: mtime-based detection catches file additions/deletions but may miss in-place overwrites on some filesystems. The 5-minute hard TTL provides a safety net
- **External changes**: User adding ROMs via USB/NFS will be detected on next mtime check (within seconds on local FS, within 5 minutes at worst via hard TTL)
- **Acceptable delay**: A few seconds for directory mtime propagation, up to 5 minutes in edge cases

## Impact on Recommendations

With the rom_cache table populated:
- **Random picks**: `SELECT ... ORDER BY RANDOM() LIMIT 6` — instant, no system iteration
- **Genre counts**: Single SQL query instead of iterating all systems' ROMs in memory
- **Multiplayer count**: Single SQL query instead of per-ROM lookup
- **Top-rated**: `SELECT ... ORDER BY rating DESC LIMIT 6` — pre-joined with enrichment data
- **Favorites-based**: `SELECT ... WHERE system = ? AND rom_filename NOT IN (...) ORDER BY rating DESC`

Estimated cold-start improvement: 3500ms+ → <50ms for recommendations.

## Risk Mitigation

- **NFS + SQLite**: Already proven with `nolock` VFS for `metadata.db`
- **Migration**: Tables created with `IF NOT EXISTS`; old installations gracefully upgrade
- **Correctness**: Filesystem remains source of truth. L2 is a cache, not a replacement
- **Fallback**: If DB is unavailable, gracefully falls back to L1 → L3 (current behavior)
