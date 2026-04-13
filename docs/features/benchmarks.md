# Performance Benchmarks

Last updated: 2026-04-13
Build: v0.3.0 (release profile)

All measurements taken on Raspberry Pi 5, 2GB RAM, USB storage, ~23K ROMs across 30+ systems.

## Single Request Latency (c=1, warm cache)

| Page | P50 | Req/s |
|---|---|---|
| Home (cache hit) | 14ms | 70 |
| Search "mario" | 47ms | 21 |
| Search "sonic" | 54ms | 18 |
| Search "street fighter" | 41ms | 24 |
| Search "a" (broad, 23K matches) | 194ms | 5.2 |
| System page | 1ms | 918 |
| Game detail | <1ms | 1,036 |

## Concurrent Load (50 requests per test)

### Homepage

| Concurrency | Req/s | P50 (ms) | P95 (ms) |
|---|---|---|---|
| 1 | 70 | 14 | 16 |
| 5 | 116 | 42 | 57 |
| 10 | 113 | 86 | 124 |
| 20 | 112 | 158 | 270 |
| 30 | 105 | 262 | 408 |

### Search "mario"

| Concurrency | Req/s | P50 (ms) | P95 (ms) |
|---|---|---|---|
| 1 | 21 | 47 | 48 |
| 5 | 22 | 224 | 228 |
| 10 | 22 | 444 | 454 |
| 20 | 22 | 847 | 907 |
| 30 | 23 | 1,104 | 1,327 |

### System pages (SNES, Mega Drive)

| Concurrency | Req/s | P50 (ms) | P95 (ms) |
|---|---|---|---|
| 1 | 918 | 1 | 2 |
| 5 | 1,607 | 3 | 5 |
| 10 | 1,637 | 6 | 8 |
| 20 | 1,658 | 11 | 16 |
| 30 | 1,971 | 13 | 20 |

### Game detail

| Concurrency | Req/s | P50 (ms) | P95 (ms) |
|---|---|---|---|
| 1 | 1,036 | <1 | 1 |
| 5 | 2,113 | 2 | 3 |
| 10 | 2,210 | 4 | 6 |
| 20 | 2,347 | 8 | 10 |
| 30 | 2,386 | 10 | 16 |

## Mixed Concurrent Test

4 endpoints simultaneously at c=5 each (20 total concurrent connections):

| Endpoint | Req/s | P50 (ms) | P95 (ms) |
|---|---|---|---|
| Homepage | 11.8 | 384 | 729 |
| Search "mario" | 6.8 | 715 | 998 |
| Search "sonic" | 6.7 | 701 | 1,058 |

## Asset Sizes (v0.3.0)

| Asset | Raw | Gzip |
|---|---|---|
| WASM bundle | 3,301 KB | 995 KB |
| CSS | 86 KB | 14 KB |
| Home HTML | 52 KB | — |
| System page HTML | 21 KB | — |

WASM is served gzip-compressed by the server.

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

| Metric | Pre-optimization | v0.2.0 | v0.3.0 |
|---|---|---|---|
| Home page (warm, c=1) | 940ms | 19ms | **14ms** |
| Search "mario" (c=1) | 348ms | 63ms | **47ms** |
| Memory after load test | 324 MB (glibc) | 67 MB (jemalloc) | 67 MB (jemalloc) |
| Mixed load: homepage req/s | 0.60 | 8.3 | **11.8** |
| WASM gzip | — | 1,778 KB | **995 KB** |

## Test Methodology

- Tool: [Apache Bench](https://httpd.apache.org/docs/current/programs/ab.html) (`ab`) via `tools/bench.sh` and `tools/load-test.sh`
- 50 requests per test with warmup pass
- Raw results in `tools/bench-results/`
