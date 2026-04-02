# Performance Benchmarks

Last updated: 2026-04-02

All measurements taken on Raspberry Pi 5, 2GB RAM, USB storage, ~23K ROMs.

## Single Request Latency (c=1, warm cache)

| Page | P50 | Req/s |
|---|---|---|
| Home (cache hit) | 18ms | 54 |
| Home (cache miss) | 136ms | -- |
| Search "mario" | 37ms | 26 |
| Search "sonic" | 50ms | 20 |
| System page (Mega Drive) | 1ms | 794 |
| Game detail | 1ms | 930 |
| Favorites | 18ms | -- |

## Concurrent Load (50 requests per test)

### Homepage (cached)

| Concurrency | Req/s | P50 (ms) | P95 (ms) |
|---|---|---|---|
| 1 | 54 | 18 | 20 |
| 5 | 88 | 54 | 75 |
| 10 | 86 | 104 | 160 |

### Search "mario"

| Concurrency | Req/s | P50 (ms) | P95 (ms) |
|---|---|---|---|
| 1 | 26 | 37 | 46 |
| 5 | 37 | 132 | 168 |
| 10 | 35 | 279 | 327 |

### System pages (SNES, Mega Drive)

| Concurrency | Req/s | P50 (ms) | P95 (ms) |
|---|---|---|---|
| 1 | 794 | 1 | 2 |
| 5 | 1530 | 3 | 5 |
| 10 | 1522 | 6 | 9 |

## Mixed Concurrent Test

4 endpoints simultaneously at c=5 each (20 total concurrent connections):

| Endpoint | Req/s | P50 (ms) | P95 (ms) |
|---|---|---|---|
| Homepage | 15.87 | 297 | 431 |
| Search "mario" | 9.91 | 493 | 613 |
| Search "sonic" | 9.55 | 521 | 699 |
| Search "street fighter" | 9.55 | 485 | 827 |

## Memory (jemalloc)

| State | RSS |
|---|---|
| Idle (after restart) | 44 MB |
| Normal browsing | 80 MB |
| After load test (c=30) | 120 MB peak |
| 3 min after load test | 113 MB (jemalloc returning memory) |

## Historical Comparison

| Metric | Before (2026-03-31) | After (2026-04-02) | Improvement |
|---|---|---|---|
| Home page (warm, c=1) | 940ms | 19ms | 49x faster |
| Search "mario" (c=1) | 348ms | 39ms | 9x faster |
| Memory (normal browsing) | 109 MB | 80 MB | -27% |
| Mixed load: homepage req/s | 0.60 | 15.87 | 26x higher |
