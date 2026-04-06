# Performance Benchmarks

Last updated: 2026-04-06
Build: v0.2.0 (release profile)

All measurements taken on Raspberry Pi 5, 2GB RAM, USB storage, ~23K ROMs across 30+ systems.

## Single Request Latency (c=1, warm cache)

| Page | P50 | Req/s |
|---|---|---|
| Home (cache hit) | 19ms | 51 |
| Home (cache miss) | 164ms | — |
| Search "mario" | 63ms | 16 |
| Search "sonic" | 82ms | 12 |
| Search "street fighter" | 59ms | 17 |
| Search "a" (broad, 23K matches) | 232ms | 4.3 |
| System page | 1ms | 910 |
| Game detail | <1ms | 1,107 |

## Concurrent Load (50 requests per test)

### Homepage

| Concurrency | Req/s | P50 (ms) | P95 (ms) |
|---|---|---|---|
| 1 | 51 | 19 | 21 |
| 5 | 75 | 61 | 99 |
| 10 | 74 | 123 | 214 |
| 20 | 73 | 272 | 455 |
| 30 | 73 | 390 | 626 |

### Search "mario"

| Concurrency | Req/s | P50 (ms) | P95 (ms) |
|---|---|---|---|
| 1 | 16 | 63 | 64 |
| 5 | 16 | 309 | 314 |
| 10 | 16 | 619 | 624 |
| 20 | 16 | 1,143 | 1,237 |
| 30 | 16 | 1,581 | 1,848 |

### System pages (SNES, Mega Drive)

| Concurrency | Req/s | P50 (ms) | P95 (ms) |
|---|---|---|---|
| 1 | 910 | 1 | 2 |
| 5 | 1,875 | 2 | 4 |
| 10 | 1,897 | 5 | 7 |
| 20 | 1,905 | 10 | 13 |
| 30 | 1,969 | 12 | 20 |

### Game detail

| Concurrency | Req/s | P50 (ms) | P95 (ms) |
|---|---|---|---|
| 1 | 1,107 | <1 | 2 |
| 5 | 2,116 | 2 | 4 |
| 10 | 2,162 | 4 | 7 |
| 20 | 1,868 | 9 | 13 |
| 30 | 2,471 | 11 | 16 |

## Mixed Concurrent Test

4 endpoints simultaneously at c=5 each (20 total concurrent connections):

| Endpoint | Req/s | P50 (ms) | P95 (ms) |
|---|---|---|---|
| Homepage | 8.3 | 576 | 875 |
| Search "mario" | 4.8 | 1,032 | 1,247 |
| Search "sonic" | 4.7 | 1,029 | 1,356 |

## Memory (jemalloc allocator)

| State | RSS |
|---|---|
| Idle (after restart) | 43 MB |
| After load test (c=30) | 67 MB |

## Historical Comparison

| Metric | Before optimization | After | Improvement |
|---|---|---|---|
| Home page (warm, c=1) | 940ms | 19ms | **49x faster** |
| Search "mario" (c=1) | 348ms | 63ms | **6x faster** |
| Memory after load test | 324 MB (glibc) | 67 MB (jemalloc) | **-79%** |
| Mixed load: homepage req/s | 0.60 | 8.3 | **14x higher** |

## Asset Sizes (v0.2.0)

| Asset | Raw | Gzip |
|---|---|---|
| WASM bundle | 13,394 KB | 1,778 KB |
| CSS | 82 KB | 13 KB |
| Home HTML | 60 KB | — |
| System page HTML | 21 KB | — |

WASM is served gzip-compressed by the server.

## Test Methodology

- Tool: [Apache Bench](https://httpd.apache.org/docs/current/programs/ab.html) (`ab`) via `tools/bench.sh`
- 50 requests per test with warmup pass
- Raw results in `tools/bench-results/`
