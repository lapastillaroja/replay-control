# Cache & SQLite Optimization Analysis

## Current Architecture

### What's cached (in-memory `RomCache`)
| Data | Key | Invalidation | Notes |
|------|-----|-------------|-------|
| System list | global | mtime + 5min TTL | `scan_systems()` — reads all 46 system dirs |
| ROM lists | per-system | mtime + 5min TTL | `list_roms()` — full directory tree walk |
| Favorites set | global | mtime + 5min TTL | `list_favorites()` — recursive `_favorites/` walk |
| Recents | global | mtime + 5min TTL | `list_recents()` — `_recent/` dir scan *(added Tier 2)* |
| Image index | per-system | mtime + 5min TTL | boxart dir scan + DB paths *(added Tier 2)* |

### What's NOT cached
| Data | Per-request cost | Called from |
|------|-----------------|------------|
| ~~Recents~~ | ~~`read_dir(_recent)` + read each `.rec` file~~ | ~~Home page~~ *(now cached — Tier 2)* |
| ~~Box art resolution~~ | ~~Per-ROM filesystem dir scan~~ | ~~Every list page~~ *(now cached via image index — Tier 2)* |
| ~~Favorites count~~ | ~~Full `list_favorites()` filesystem walk~~ | ~~Home page~~ *(now uses cached favorites — Tier 2)* |
| Disk usage | Subprocess: `df -B1` (~3ms) | Home page (`get_info`) |
| Network IPs | Subprocess: `ip` x2 (~10ms) | Home page (`get_info`) |

### What's in SQLite (`metadata.db`)
| Data | Rows | Coverage |
|------|------|----------|
| Game metadata (description, publisher) | 19,035 | Imported from LaunchBox |
| Ratings | 19,035 | From LaunchBox community ratings |
| Box art paths | 12,365 / 19,035 | 65% — only for systems with imported images |
| Screenshot paths | ~similar | Same source |

---

## Where Time Goes: Home Page (~680ms local NFS, ~1080ms Pi USB)

The home page SSR awaits **three parallel Resources**: `get_info()`, `get_recents()`, `get_systems()`.

### `get_info()` — the bottleneck (~500-700ms on NFS)

| Operation | NFS time | Pi USB time | Notes |
|-----------|----------|-------------|-------|
| `cache.get_systems()` | ~430ms cold / ~0ms warm | ~95ms cold / ~0ms warm | Reads all 46 system dirs |
| `list_favorites()` | ~41ms | ~10ms | Recursive `_favorites/` walk — **NOT using cache** |
| `disk_usage()` (subprocess `df`) | ~3ms | ~5ms | |
| `get_network_ips()` (subprocess `ip` x2) | ~10ms | ~15ms | |

**Key finding**: `get_info()` calls `list_favorites()` directly instead of using the cached `get_favorites_set()`. It only needs the count (`favorites.len()`), not the full list.

### `get_recents()` — (~50-100ms)

| Operation | NFS time | Pi USB time | Notes |
|-----------|----------|-------------|-------|
| `list_recents()` | ~8ms | ~5ms | Dir scan + file reads |
| `resolve_box_art_url()` x N | ~30-80ms | ~20-50ms | Per-entry DB lookup + possible dir scan |

### `get_systems()` — (~0ms warm, same as get_info's cache call)

All three run in parallel via Leptos Resources, so total = max(get_info, get_recents, get_systems) ≈ `get_info` time.

---

## Where Time Goes: Games Page

### Cache hit path (NES: ~2.3ms local, ~3.9ms Pi)

Already fast. The mtime check (single `stat()`) is working well.

### Cache miss path (Arcade MAME: ~4200ms local NFS, ~1638ms Pi USB)

| Operation | Notes |
|-----------|-------|
| `list_roms()` directory walk | Arcade MAME has thousands of files |
| M3U dedup parsing | Reads first 8KB of each M3U |
| Sort by display name/tier/region | O(n log n) on full list |

### Per-page enrichment (on every request, even cache hit)

| Operation | Per-item cost | For 100 items |
|-----------|--------------|---------------|
| `get_favorites_set()` | O(1) HashSet lookup | Fast (cached) |
| `resolve_box_art_url()` | 1 DB query + 1-2 stat + possible dir scan | **100 DB queries + 100-200 stats** |
| `arcade_db::lookup_arcade_game()` | O(1) hash lookup | Fast (in-memory) |
| `lookup_players()` | O(1) hash lookup | Fast (in-memory) |
| `db.lookup_ratings()` | 1 batch SQL query | Fast (single query) |

**Box art resolution is the remaining per-page bottleneck.** For ROMs with metadata DB entries that have `box_art_path` set, it's: 1 SQL query + 1 stat (to validate file exists). For ROMs without DB entries, it falls through to `find_image_on_disk()` which does a full `read_dir` + fuzzy match.

---

## Optimization Opportunities

### Tier 2A: Cache box art lookups in memory (HIGH IMPACT)

**Problem**: `resolve_box_art_url()` does per-ROM I/O (DB query + stat) on every page load.

**Solution**: Add a per-system image filename cache to `RomCache`:
```
HashMap<String, HashMap<String, String>>  // system → (rom_filename → box_art_url)
```

- Build lazily on first request for a system
- Pre-scan the `media/{system}/boxart/` directory once, build a `HashMap<normalized_name, filename>`
- For each ROM, do the 3-tier match (exact → fuzzy → version-stripped) against the in-memory map
- Also query metadata DB in batch (`WHERE system = ? AND box_art_path IS NOT NULL`)
- Cache result: `rom_filename → resolved_url`
- Invalidate on image import only

**Impact**: Eliminates 100 DB queries + 100-200 filesystem stats per page load. On NFS, this could save 50-200ms per page.

### Tier 2B: Cache recents in memory (MEDIUM IMPACT)

**Problem**: `list_recents()` scans `_recent/` directory every home page load.

**Solution**: Add recents to `RomCache` with mtime-based invalidation (same pattern as existing caches).

**Impact**: Saves ~8ms NFS / ~5ms USB per home page load. Small but free.

### Tier 2C: Use cached favorites count in `get_info()` (MEDIUM IMPACT)

**Problem**: `get_info()` calls `list_favorites()` directly (full filesystem walk) instead of using the cached favorites.

**Solution**: Add `get_favorites_count()` to `RomCache` that returns the total count from the cached data.

**Impact**: Saves ~41ms NFS / ~10ms USB per home page load.

### Tier 2D: Batch box art resolution (MEDIUM IMPACT)

**Problem**: Box art URL is resolved per-ROM in a loop.

**Solution**: `resolve_box_art_urls_batch(state, system, &[rom_filename])` that:
1. Single batch query to metadata DB: `SELECT rom_filename, box_art_path FROM game_metadata WHERE system = ? AND rom_filename IN (...)`
2. For ROMs not in DB: single `read_dir` of `media/{system}/boxart/` → build fuzzy index → match all at once
3. Return `HashMap<rom_filename, url>`

**Impact**: 1 DB query + 1 dir read instead of N queries + N dir reads. Biggest win for systems without full metadata DB coverage.

### Tier 2E: Parallelize home page server calls (LOW-MEDIUM IMPACT)

**Problem**: `get_info()` runs `get_systems()`, `list_favorites()`, `disk_usage()`, `get_network_ips()` sequentially.

**Solution**: Use `tokio::join!` to run independent operations in parallel:
- `cache.get_systems()` (might block on filesystem)
- `favorites count` (from cache)
- `disk_usage()` (subprocess)
- `get_network_ips()` (subprocess)

**Caveat**: These are sync filesystem/subprocess calls in an async context. Would need `spawn_blocking` for true parallelism. The favorites and network IPs are small enough that the overhead may not be worth it.

---

## SQLite as Primary Data Store

### Current state: hybrid filesystem + SQLite

The app uses a **dual-source** model:
- **Filesystem**: `.fav` and `.rec` marker files (source of truth, compatible with RePlayOS)
- **SQLite**: Imported metadata, ratings, image paths (cache/enrichment layer)

### Should more data move to SQLite?

#### Recents → SQLite table

```sql
CREATE TABLE recents (
    system TEXT NOT NULL,
    rom_filename TEXT NOT NULL,
    rom_path TEXT NOT NULL,
    last_played INTEGER NOT NULL,
    PRIMARY KEY (system, rom_filename)
);
CREATE INDEX idx_recents_played ON recents(last_played DESC);
```

**Pros**:
- Instant queries: `SELECT ... ORDER BY last_played DESC LIMIT 20`
- No filesystem traversal
- Index makes "most recent" O(1) instead of O(n)

**Cons**:
- Must keep `.rec` files in sync (RePlayOS creates them on game launch)
- Two sources of truth → sync complexity
- `.rec` files are created by RePlayOS, not the app → need watcher or sync-on-read

**Verdict**: **Not worth it.** The `_recent/` directory typically has <50 files. At 8ms NFS / 5ms USB, the filesystem read is fast enough. Adding a sync layer adds complexity for marginal gain. Better to just cache in memory (Tier 2B).

#### Favorites → SQLite table

```sql
CREATE TABLE favorites (
    system TEXT NOT NULL,
    rom_filename TEXT NOT NULL,
    marker_filename TEXT NOT NULL,
    subfolder TEXT DEFAULT '',
    date_added INTEGER,
    PRIMARY KEY (system, rom_filename)
);
```

**Pros**:
- Fast `is_favorite()` checks: indexed lookup
- Fast `COUNT(*)` for home page stats
- JOINs with metadata for organized favorites
- No recursive directory walk

**Cons**:
- Must keep `.fav` files in sync (RePlayOS reads them directly for favorites menu)
- Two sources of truth
- `.fav` files can be created/deleted outside the app (by RePlayOS, by user)
- Organization subfolders are the actual source of truth for RePlayOS

**Verdict**: **Not worth it.** The in-memory `FavoritesCache` already solves the read performance problem. The filesystem is the canonical source, and `.fav` files must exist for RePlayOS compatibility. A SQLite mirror would add sync complexity without meaningful performance gain over the in-memory cache.

#### ROM file list → SQLite table

```sql
CREATE TABLE rom_files (
    system TEXT NOT NULL,
    rom_filename TEXT NOT NULL,
    rom_path TEXT NOT NULL,
    display_name TEXT,
    size_bytes INTEGER,
    is_m3u BOOLEAN DEFAULT 0,
    box_art_url TEXT,
    genre TEXT,
    players INTEGER,
    rating REAL,
    driver_status TEXT,
    scanned_at INTEGER,
    PRIMARY KEY (system, rom_filename)
);
CREATE INDEX idx_rom_display ON rom_files(system, display_name);
```

**Pros**:
- Persist the full enriched ROM list across restarts (no cold-cache penalty)
- SQL filtering/sorting instead of in-memory Vec operations
- Pre-computed box art URLs, genre, players — no per-request enrichment
- Could enable full-text search via FTS5
- Survives server restarts (current in-memory cache doesn't)

**Cons**:
- Must detect filesystem changes (new/deleted/renamed ROMs)
- Adds SQLite write path on every ROM scan
- M3U dedup logic is complex, hard to express in SQL
- Still need filesystem scan to detect new ROMs
- NFS + SQLite locking issues (already handled for metadata DB with `nolock` VFS)

**Verdict**: **This is the most promising SQLite optimization.** Not as a replacement for filesystem scanning, but as a **persistent cache** that survives restarts. The current in-memory cache starts cold on every server restart, requiring a full directory walk that takes 430ms (NFS) or 95ms (USB) for `scan_systems()` alone. A SQLite-backed cache could:

1. On startup: load cached ROM lists from DB instantly
2. In background: verify filesystem hasn't changed (mtime check)
3. If changed: re-scan, update DB
4. Store pre-resolved box art URLs, eliminating per-request I/O

#### Box art URL cache → SQLite (extend existing metadata DB)

The metadata DB already stores `box_art_path` but only for imported games (65% coverage). For the other 35%, the app falls back to filesystem scanning.

**Solution**: After `find_image_on_disk()` resolves a path, store it in the DB:
```sql
-- Extend existing table or add a separate cache table
CREATE TABLE image_cache (
    system TEXT NOT NULL,
    rom_filename TEXT NOT NULL,
    box_art_path TEXT,
    snap_path TEXT,
    resolved_at INTEGER,
    PRIMARY KEY (system, rom_filename)
);
```

**Pros**: One-time cost for fuzzy matching, then instant lookups forever
**Cons**: Need invalidation when images are imported/deleted

**Verdict**: **Good incremental improvement.** Can be done independently.

---

## Recommended Approach: Persistent ROM Cache in SQLite

### Design

Extend `metadata.db` with a `rom_cache` table:

```sql
CREATE TABLE rom_cache (
    system TEXT NOT NULL,
    rom_filename TEXT NOT NULL,
    rom_path TEXT NOT NULL,
    display_name TEXT,
    size_bytes INTEGER NOT NULL,
    is_m3u BOOLEAN DEFAULT 0,
    box_art_url TEXT,
    driver_status TEXT,
    genre TEXT,
    players INTEGER,
    rating REAL,
    PRIMARY KEY (system, rom_filename)
);

-- For "last scanned" tracking per system
CREATE TABLE rom_cache_meta (
    system TEXT PRIMARY KEY,
    dir_mtime INTEGER,        -- filesystem mtime of system dir
    scanned_at INTEGER,       -- when we last scanned
    rom_count INTEGER,
    total_size_bytes INTEGER
);
```

### Flow

**On request for ROM list:**
1. Check `rom_cache_meta` for system — compare `dir_mtime` with current filesystem mtime
2. If fresh: `SELECT * FROM rom_cache WHERE system = ? ORDER BY display_name` → return directly
3. If stale: scan filesystem, update `rom_cache` + `rom_cache_meta`, return

**On server startup:**
1. Load `rom_cache_meta` for all systems → in-memory summary (system list + counts)
2. No filesystem scanning needed for initial page load
3. Background task: verify mtimes, re-scan stale systems

**Benefits over current in-memory cache:**
- **Zero cold-start penalty**: First request after restart is instant
- **Pre-computed enrichment**: box_art_url, genre, players, rating stored per-ROM
- **SQL filtering**: `WHERE genre = 'Action' AND rating >= 4.0` instead of in-memory Vec filtering
- **Pagination in SQL**: `LIMIT 100 OFFSET 200` instead of loading entire list
- **Persistent across restarts**: No re-scanning after deploy/crash

### Complexity estimate

This is a significant refactor touching:
- `cache.rs` — replace in-memory cache with SQLite-backed cache
- `roms.rs` server fn — query DB instead of cache + per-item enrichment
- `system.rs` server fn — query DB for system summaries
- `metadata_db.rs` — new tables and queries
- Build/startup — background re-scan task

### Risk

- SQLite on NFS with `nolock` VFS — already proven to work for metadata DB
- Migration path — old installations without the table need graceful fallback
- Correctness — filesystem is still source of truth, DB is just a cache

---

## Recommended Priority

| # | Optimization | Impact | Effort | Status |
|---|-------------|--------|--------|--------|
| 1 | ~~**Tier 2C**: Use cached favorites count in `get_info()`~~ | -41ms NFS home | Trivial | **Done** |
| 2 | ~~**Tier 2B**: Cache recents in memory~~ | -8ms NFS home | Small | **Done** |
| 3 | ~~**Tier 2D+2A**: Batch box art via per-system image index cache~~ | -50-4000ms per page | Medium | **Done** |
| 4 | **Box art URL in SQLite** | Persistent across restarts | Medium | Pending |
| 5 | **Persistent ROM cache in SQLite** | Zero cold-start, SQL queries | Large | Pending |

### Post-Tier 2 Results

All warm-cache page loads are now under 17ms on both local NFS and Pi USB:

| Endpoint | Baseline | After Tier 1+2 | Speedup |
|----------|----------|----------------|---------|
| Home (NFS) | 689ms | 12ms | **57x** |
| Home (Pi) | 1084ms | 17ms | **64x** |
| Games NES (NFS) | 37ms | 0.7ms | **53x** |
| Games NES (Pi) | 7ms | 1.6ms | **4x** |
| Games Arcade (NFS) | 4397ms | 5ms | **879x** |
| Games Arcade (Pi) | 1662ms | 11ms | **151x** |

The remaining optimization opportunities (SQLite persistence) would primarily help **cold starts** (first request after server restart), which still require full filesystem scans.
