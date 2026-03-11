# Performance Benchmarks

All measurements use `tools/bench.sh` — median of 3 runs with warm caches.

**Local**: x86_64, NFS-mounted storage (`<NFS_MOUNT>`)
**Pi**: Raspberry Pi 4, USB storage, accessed over LAN

## 2026-03-11: Tier 1 Optimizations

### Changes

- **B1**: `wasm-opt -Oz`, pre-compressed `.wasm.gz`, `CompressionLayer` for dynamic gzip, `[profile.wasm-release]`
- **B2**: In-memory favorites cache (`HashMap<String, HashSet<String>>`)
- **B3**: mtime-based cache invalidation (single `stat()` vs full rescan), 5-min hard TTL
- **B4**: Explicit `width="56" height="40"` on thumbnail `<img>`
- **B5**: CSS `content-visibility: auto` on `.rom-item`
- Recommendations section disabled (SSR hang over NFS)

### SSR Response Times (ms, TTFB median)

| Endpoint       | Baseline Local | Tier1 Local | Change   | Baseline Pi | Tier1 Pi | Change   |
|----------------|---------------|-------------|----------|-------------|----------|----------|
| Home `/`       | 689           | 680         | -1%      | 1084        | 1081     | ~0%      |
| Games NES      | 37            | 2.3         | **-94%** | 7.0         | 3.9      | **-44%** |
| Games Arcade   | 4397          | 4200        | -4%      | 1662        | 1638     | -1%      |

### SSR Response Sizes (KB, uncompressed)

| Endpoint       | Baseline Local | Tier1 Local | Notes                              |
|----------------|---------------|-------------|------------------------------------|
| Home `/`       | 45.7          | 6.6         | Gzip compression layer active      |
| Games NES      | 4.2           | 1.7         | Gzip compression layer active      |
| Games Arcade   | 121.8         | 9.3         | Gzip compression layer active      |

Note: Tier1 sizes are smaller because curl receives gzip-compressed responses via `CompressionLayer`.

### Asset Sizes (KB)

| Asset          | Baseline Raw | Tier1 Raw | Change   | Baseline Gzip | Tier1 Gzip | Change   |
|----------------|-------------|-----------|----------|---------------|------------|----------|
| WASM bundle    | 7426        | 2197      | **-70%** | 1240          | 670        | **-46%** |
| CSS            | 46.4        | 46.5      | —        | 7.4           | 7.4        | —        |

### Summary

| Optimization         | Impact                                                        |
|---------------------|---------------------------------------------------------------|
| wasm-opt -Oz        | WASM 7.4MB → 2.2MB raw (-70%), 1.2MB → 670KB gzip (-46%)    |
| Gzip compression    | HTML responses compressed on-the-fly; WASM served pre-compressed |
| mtime cache         | Games NES: 37ms → 2.3ms local (-94%), 7ms → 3.9ms Pi (-44%) |
| Favorites cache     | Eliminates per-request filesystem scan of `.fav` files        |
| img dimensions      | Reduces CLS (layout shift) during thumbnail loading           |
| content-visibility  | Browser skips rendering off-screen ROM items                  |

---

## 2026-03-12: Tier 2 Optimizations

### Changes

- **C1**: Use cached favorites count in `get_info()` — avoids full `list_favorites()` filesystem walk (was ~41ms on NFS)
- **C2**: Cache recents in memory with mtime-based invalidation — avoids `_recent/` dir scan per home page load
- **C3**: Batch box art resolution via per-system image index cache — single `read_dir` + HashMap lookups instead of N per-ROM filesystem scans

### SSR Response Times (ms, TTFB median)

| Endpoint       | Tier1 Local | Tier2 Local | Change      | Tier1 Pi | Tier2 Pi | Change      |
|----------------|-------------|-------------|-------------|----------|----------|-------------|
| Home `/`       | 680         | 12.4        | **-98%**    | 1081     | 16.5     | **-98%**    |
| Games NES      | 2.3         | 0.7         | **-70%**    | 3.9      | 1.6      | **-59%**    |
| Games Arcade   | 4200        | 5.2         | **-99.9%**  | 1638     | 10.5     | **-99.4%**  |

### Full Comparison (Baseline → Tier1 → Tier2)

| Endpoint       | Baseline Local | Tier2 Local | Total Change | Baseline Pi | Tier2 Pi | Total Change |
|----------------|---------------|-------------|-------------|-------------|----------|-------------|
| Home `/`       | 689           | 12.4        | **-98%**    | 1084        | 16.5     | **-98%**    |
| Games NES      | 37            | 0.7         | **-98%**    | 7.0         | 1.6      | **-77%**    |
| Games Arcade   | 4397          | 5.2         | **-99.9%**  | 1662        | 10.5     | **-99.4%**  |

### Summary

| Optimization                | Impact                                                              |
|----------------------------|---------------------------------------------------------------------|
| Cached favorites count     | Home: eliminates ~41ms NFS / ~10ms USB filesystem walk              |
| Cached recents             | Home: eliminates ~8ms NFS / ~5ms USB dir scan per load              |
| Image index cache          | Games pages: 1 dir read cached vs N per-ROM scans. Arcade: 4200ms → 5ms |

### Key Insight

The Tier 2 home page improvement (680ms → 12ms) is primarily from the **image index cache** applied to `get_recents()` box art resolution. Previously, each of the ~10 recent entries triggered a full `read_dir` + fuzzy match per system's boxart directory. Now, the directory is read once and indexed in a HashMap — subsequent lookups are O(1).

The arcade games page improvement (4200ms → 5ms) is the same effect at scale: 100 ROMs × full dir scan → 1 cached dir scan + 100 HashMap lookups.

---

## 2026-03-12: SQLite ROM Cache (L2 Persistent Cache)

### Changes

- **L2 cache**: `rom_cache` + `rom_cache_meta` SQLite tables persist ROM scan results across server restarts
- **Three-layer architecture**: L1 (in-memory) → L2 (SQLite) → L3 (filesystem scan) with write-through
- **mtime-based invalidation**: Single `stat()` validates L2 freshness; background startup verification
- **Nolock-first DB open**: Try `nolock=1` URI first (instant), fall back to WAL mode — eliminates 5s timeout on NFS
- **Background cache verification**: Startup task compares stored mtimes, re-scans stale systems
- **SQL-based recommendation queries**: `random_cached_roms`, `top_rated_cached_roms`, `genre_counts`, etc.

### Cold Start Times (ms, TTFB — first request after server restart)

| Endpoint    | Before (L2 empty) | After (L2 populated) Local NFS | After (L2) Pi USB |
|-------------|-------------------|-------------------------------|-------------------|
| Home `/`    | 6100 (NFS)        | 1226                          | 14                |
| Games NES   | 12                | 17                            | 2.5               |
| Games Arcade| 15                | 31                            | 18                |

### Warm Cache Times (ms, TTFB — subsequent requests)

| Endpoint       | Local NFS | Pi USB |
|----------------|-----------|--------|
| Home `/`       | 23        | 13     |
| Games NES      | 2         | 1.5    |
| Games Arcade   | 14        | 9      |

### Load Test (50 requests, 10 concurrent, warm cache)

| Endpoint       | Local NFS req/s | Local NFS mean (ms) | Pi USB req/s | Pi USB mean (ms) |
|----------------|----------------|---------------------|-------------|-----------------|
| Home `/`       | 323            | 31                  | 134         | 75              |
| Games NES      | 2668           | 3.7                 | 1711        | 5.8             |

### Key Insights

- **Pi USB cold start: 14ms** — L2 eliminates the 795ms filesystem scan entirely after first visit
- **NFS cold start: 1.2s → was 6.1s** — the 5s improvement comes from reversing the SQLite open order (nolock first). Remaining 1.2s is image index building for recents (NFS dir reads, not cacheable in L2 yet)
- **NES cold start: 3ms on Pi** — L2 restores 1000+ ROMs from SQLite faster than scanning the filesystem
- L2 has no measurable impact on warm cache (L1 is always faster)
