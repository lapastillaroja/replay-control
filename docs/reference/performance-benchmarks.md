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

---

## 2026-03-12: Recommendations Enabled (SSR, Full Enrichment)

### Changes

- **SSR recommendations**: All 5 sections render server-side via `Resource` + `Transition` — visible on first paint
- **SQL-powered queries**: Random picks, top-rated, genre counts, multiplayer count — all from `rom_cache` via SQL
- **L2 warmup on startup**: When rom_cache is empty, background task pre-populates all systems + enriches box art URLs and ratings
- **Box art + rating enrichment**: Image index resolution + LaunchBox ratings written to rom_cache during warmup
- **Five recommendation sections**: Random picks, favorites-based ("Because you love…"), top-rated, discover (genre/multiplayer pills)
- **Race condition fix**: LaunchBox auto-import no longer clears rom_cache (was wiping warmup data)
- **UNIQUE constraint fix**: `INSERT OR IGNORE` in save_system_roms handles duplicate ROM entries from M3U dedup

### L2 Warmup Times (ms, from fresh DB)

| Environment | Systems | ROMs   | Scan Time | Enrich Time | Total |
|-------------|---------|--------|-----------|-------------|-------|
| Pi USB      | 19      | 27,081 | 1.4s      | 0.5s        | 1.9s  |
| Local NFS   | 17      | 20,631 | 10.9s     | 0.5s        | 11.4s |

### Cold Start Times (ms, TTFB — first request after server restart, L2 populated)

| Endpoint       | Local NFS | Pi USB |
|----------------|-----------|--------|
| Home `/`       | 1093      | 350    |
| Games NES      | 13        | 2.3    |
| Games Arcade   | 6.3       | 21     |

Note: NFS cold-start dominated by NFS dir reads for recents box art. Pi cold-start includes background warmup/import startup overhead.

### Warm Cache Times (ms, TTFB median of 3 runs)

| Endpoint       | Local NFS | Pi USB |
|----------------|-----------|--------|
| Home `/`       | 21        | 37     |
| Games NES      | 0.5       | 1.4    |
| Games Arcade   | 5.0       | 10     |

### Load Test (50 requests, 10 concurrent, warm cache)

| Endpoint       | Local NFS req/s | Local NFS mean (ms) | Pi USB req/s | Pi USB mean (ms) |
|----------------|----------------|---------------------|-------------|-----------------|
| Home `/`       | 81             | 124                 | 38          | 267             |
| Games NES      | 5760           | 1.7                 | 1560        | 6.4             |

### GetRecommendations Endpoint (ms, TTFB median)

| Local NFS | Pi USB |
|-----------|--------|
| 0.3       | 0.5    |

### SSR Response Sizes (KB)

| Endpoint | Uncompressed | Gzip |
|----------|-------------|------|
| Home `/` | 54          | 8.1  |

### Key Insights

- **Full SSR recommendations**: All 5 sections visible in initial HTML — no client-side loading needed
- **Pi warm TTFB: 37ms** — excellent for single-user retro gaming device
- **Recommendation endpoint: 0.3-0.5ms** — SQL queries on enriched rom_cache are near-instant
- **Load test throughput lower than without recs** (81 vs 323 req/s local): expected, since SSR now renders 20+ game cards with box art. Irrelevant for single-user Pi use case
- **L2 warmup: 1.9s on Pi** — pre-populates 27K ROMs with box art and ratings from fresh DB
- **Enrichment coverage**: 13,483 ROMs with box art, 12,012 with ratings (from 27K total)
- **Games pages unaffected**: NES 5760 req/s, Arcade stays fast — recommendations only impact home page

---

## Cumulative Summary (Baseline → Final)

### End-to-End TTFB Improvements (ms, warm cache)

| Endpoint       | Baseline Local | Final Local | Improvement | Baseline Pi | Final Pi | Improvement |
|----------------|---------------|-------------|-------------|-------------|----------|-------------|
| Home `/`       | 689           | 21          | **-97%**    | 1084        | 37       | **-97%**    |
| Games NES      | 37            | 0.5         | **-99%**    | 7.0         | 1.4      | **-80%**    |
| Games Arcade   | 4397          | 5.0         | **-99.9%**  | 1662        | 10       | **-99.4%**  |

### Architecture

```
Request → L1 (in-memory HashMap) → L2 (SQLite rom_cache) → L3 (filesystem scan)
                                     ↑ write-through         ↑ write-through
```

- **L1**: Sub-millisecond, invalidated by mtime check (single `stat()`) or 5-min TTL
- **L2**: Persists across restarts, pre-populated on startup via background warmup (1.9s on Pi for 27K ROMs)
- **L3**: Full filesystem scan, only on first access or stale cache

### Optimization Stack

| Layer | Optimization | Files Changed |
|-------|-------------|---------------|
| Build | `wasm-opt -Oz`, `[profile.wasm-release]` with LTO | `build.sh`, `Cargo.toml` |
| Network | Pre-compressed `.wasm.gz`, `CompressionLayer` for dynamic gzip | `main.rs` |
| Cache L1 | In-memory favorites, recents, systems, ROMs with mtime invalidation | `cache.rs` |
| Cache L1 | Per-system image index (HashMap) for O(1) box art lookups | `cache.rs` |
| Cache L2 | SQLite `rom_cache` + `rom_cache_meta` with nolock-first NFS support | `metadata_db.rs` |
| Cache L2 | Background warmup + box art/rating enrichment on startup | `background.rs` |
| Render | `content-visibility: auto` on `.rom-item` | CSS |
| Render | Explicit `width`/`height` on thumbnail `<img>` | `rom_list.rs` |
| SSR | Recommendations rendered server-side via `Transition` | `home.rs` |
| SQL | All recommendation queries from enriched `rom_cache` — no filesystem | `recommendations.rs` |

### Key Numbers

| Metric | Value |
|--------|-------|
| WASM bundle (gzip) | 670 KB (was 1.2 MB) |
| Home page HTML (gzip) | 8.1 KB |
| Pi warm TTFB (home) | 37 ms |
| Pi cold start (L2 populated) | 350 ms |
| L2 warmup (27K ROMs, Pi) | 1.9s |
| Recommendation query | 0.3–0.5 ms |
| Enrichment coverage | 13,483 box art / 12,012 ratings (of 27K ROMs) |
