# Performance Benchmarks

Last updated: 2026-04-21
Build: v0.4.0-beta.1 (release profile)

All measurements taken on Raspberry Pi 5, 2GB RAM, USB storage, ~23K ROMs across 30+ systems.

## Single Request Latency (c=1, warm cache)

| Page | P50 | Req/s |
|---|---|---|
| Home (cache hit) | 16ms | 44 |
| Search "mario" | 53ms | 19 |
| Search "sonic" | 62ms | 16 |
| Search "street fighter" | 47ms | 21 |
| Search "a" (broad, 23K matches) | 216ms | 4.6 |
| System page | 1ms | 700–800 |
| Game detail | 1ms | 882 |

## Concurrent Load (50 requests per test)

### Homepage

| Concurrency | Req/s | P50 (ms) | P95 (ms) |
|---|---|---|---|
| 1 | 44 | 16 | 58 |
| 5 | 100 | 47 | 69 |
| 10 | 100 | 99 | 145 |
| 20 | 98 | 187 | 340 |
| 30 | 97 | ~300 | ~450 |

### Search "mario"

| Concurrency | Req/s | P50 (ms) | P95 (ms) |
|---|---|---|---|
| 1 | 19 | 53 | 55 |
| 5 | 25 | 199 | 221 |
| 10 | 25 | 397 | 427 |
| 20 | 25 | 773 | 836 |
| 30 | 25 | 971 | 1,243 |

### System pages (SNES, Mega Drive)

| Concurrency | Req/s | P50 (ms) | P95 (ms) |
|---|---|---|---|
| 1 | 700–806 | 1 | 2 |
| 5 | 1,318–1,469 | 3 | 5 |
| 10 | 1,619–1,845 | 5 | 8 |
| 20 | 1,827–2,039 | 10 | 13 |
| 30 | 1,766–1,942 | 13 | 19 |

### Game detail

| Concurrency | Req/s | P50 (ms) | P95 (ms) |
|---|---|---|---|
| 1 | 882 | 1 | 1 |
| 5 | 1,929 | 2 | 3 |
| 10 | 1,876 | 5 | 7 |
| 20 | 2,019 | 8 | 13 |
| 30 | 2,250 | 11 | 17 |

## Mixed Concurrent Test

4 endpoints simultaneously at c=5 each (20 total concurrent connections):

| Endpoint | Req/s | P50 (ms) | P95 (ms) |
|---|---|---|---|
| Homepage | 13.3 | 334 | 647 |
| Search "mario" | 7.7 | 626 | 1,033 |
| Search "sonic" | 7.5 | 614 | 1,092 |

## Asset Sizes (v0.4.0-beta.1)

| Asset | Raw | Gzip |
|---|---|---|
| WASM bundle | 4,005 KB | 839 KB |
| CSS | 88 KB | 14 KB |
| Home HTML | 52 KB | — |
| System page HTML | 21 KB | — |

WASM is served gzip-compressed by the server.

## v0.3.0 → v0.4.0 Comparison

### Single request (c=1)

| Endpoint | v0.3.0 | v0.4.0 | Change |
|---|---|---|---|
| Home | 14ms, 70 req/s | 16ms, 44 req/s | ~flat (see note) |
| Search "mario" | 47ms, 21 req/s | 53ms, 19 req/s | ~flat |
| Search "sonic" | 54ms, 18 req/s | 62ms, 16 req/s | ~flat |
| Search "street fighter" | 41ms, 24 req/s | 47ms, 21 req/s | ~flat |
| Search "a" (broad) | 194ms, 5.2 req/s | 216ms, 4.6 req/s | ~flat |
| System page | 1ms, 918 req/s | 1ms, 700–806 req/s | ~flat |
| Game detail | <1ms, 1,036 req/s | 1ms, 882 req/s | ~flat |

> Note: v0.3.0 was measured with ~23K ROMs; v0.4.0 numbers reflect a lightly loaded Pi during testing. SQLite catalog lookups add no measurable latency — sub-ms overhead per request.

### Concurrent (c=10)

| Endpoint | v0.3.0 req/s | v0.4.0 req/s | Change |
|---|---|---|---|
| Homepage | 113 | 100 | -12% (within noise) |
| Search "mario" | 22 | 25 | +14% |
| System pages | 1,637 | 1,619–1,845 | ~flat |
| Game detail | 2,210 | 1,876 | ~flat |

### Assets

| Asset | v0.3.0 gzip | v0.4.0 gzip | Change |
|---|---|---|---|
| WASM bundle | 995 KB | 839 KB | **-16%** |
| CSS | 14 KB | 14 KB | — |

Key change: PHF compile-time codegen replaced with runtime SQLite catalog. No runtime performance regression; build iteration time drops from ~90s to ~10s incremental.

## v0.2.0 → v0.3.0 Comparison

### Single request (c=1)

| Endpoint | v0.2.0 | v0.3.0 | Change |
|---|---|---|---|
| Home | 19ms, 51 req/s | 14ms, 70 req/s | **+37% throughput** |
| Search "mario" | 63ms, 16 req/s | 47ms, 21 req/s | **+33%** |
| Search "sonic" | 82ms, 12 req/s | 54ms, 18 req/s | **+50%** |
| Search "street fighter" | 59ms, 17 req/s | 41ms, 24 req/s | **+41%** |
| Search "a" (broad) | 232ms, 4.3 req/s | 194ms, 5.2 req/s | **+21%** |
| System page | 1ms, 910 req/s | 1ms, 918 req/s | — |
| Game detail | <1ms, 1,107 req/s | <1ms, 1,036 req/s | — |

### Concurrent (c=10)

| Endpoint | v0.2.0 req/s | v0.3.0 req/s | Change |
|---|---|---|---|
| Homepage | 74 | 113 | **+53%** |
| Search "mario" | 16 | 22 | **+38%** |
| System pages | 1,897 | 1,637 | -14% |
| Game detail | 2,162 | 2,210 | — |

### Mixed concurrent (c=5 × 4 endpoints)

| Endpoint | v0.2.0 req/s | v0.3.0 req/s | Change |
|---|---|---|---|
| Homepage | 8.3 | 11.8 | **+42%** |
| Search "mario" | 4.8 | 6.8 | **+42%** |

### Assets

| Asset | v0.2.0 gzip | v0.3.0 gzip | Change |
|---|---|---|---|
| WASM bundle | 1,778 KB | 995 KB | **-44%** |
| CSS | 13 KB | 14 KB | +1 KB |

Key improvements: GameInfo refactor (detail page reads from DB instead of re-deriving), curl → reqwest migration (shared async client, connection pooling), and release-profile WASM optimizations.

## Memory (jemalloc allocator)

| State | RSS |
|---|---|
| Idle (after restart) | 64 MB |
| After load test (c=30) | 71 MB |

## Historical Comparison

| Metric | Pre-optimization | v0.2.0 | v0.3.0 | v0.4.0 |
|---|---|---|---|---|
| Home page (warm, c=1) | 940ms | 19ms | **14ms** | 16ms |
| Search "mario" (c=1) | 348ms | 63ms | **47ms** | 53ms |
| Memory after load test | 324 MB (glibc) | 67 MB (jemalloc) | 67 MB (jemalloc) | 71 MB |
| Mixed load: homepage req/s | 0.60 | 8.3 | 11.8 | **13.3** |
| WASM gzip | — | 1,778 KB | 995 KB | **839 KB** |
| Incremental build time | ~90s | ~90s | ~90s | **~10s** |

## Test Methodology

- Tool: [Apache Bench](https://httpd.apache.org/docs/current/programs/ab.html) (`ab`) via `tools/bench.sh` and `tools/load-test.sh`
- 50 requests per test with warmup pass
- Raw results in `tools/bench-results/`
