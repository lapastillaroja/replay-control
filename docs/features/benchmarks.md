# Performance Benchmarks

Last updated: 2026-04-03
Build: v0.1.0-beta.4 (release profile)

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
| Favorites | 18ms | — |

## Concurrent Load (50 requests per test)

### Homepage (cached)

| Concurrency | Req/s | P50 (ms) | P95 (ms) |
|---|---|---|---|
| 1 | 51 | 19 | 21 |
| 5 | 88 | 54 | 75 |
| 10 | 86 | 104 | 160 |

### Search "mario"

| Concurrency | Req/s | P50 (ms) | P95 (ms) |
|---|---|---|---|
| 1 | 16 | 63 | 64 |
| 5 | 37 | 132 | 168 |
| 10 | 35 | 279 | 327 |

### System pages

| Concurrency | Req/s | P50 (ms) | P95 (ms) |
|---|---|---|---|
| 1 | 910 | 1 | 2 |
| 5 | 1,530 | 3 | 5 |
| 10 | 1,522 | 6 | 9 |

### Game detail

| Concurrency | Req/s | P50 (ms) | P95 (ms) |
|---|---|---|---|
| 1 | 1,107 | <1 | 2 |

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
| Mixed load: homepage req/s | 0.60 | 15.87 | **26x higher** |

## Test Methodology

- Tool: [Apache Bench](https://httpd.apache.org/docs/current/programs/ab.html) (`ab`) via `tools/load-test.sh`
- 50 requests per test with warmup pass
- Raw results in `tools/bench-results/`
