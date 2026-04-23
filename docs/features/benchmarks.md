# Performance Benchmarks

Last updated: 2026-04-23
Build: v0.4.0 (release profile, commit `db4acba`)

All measurements taken on Raspberry Pi 5, 2GB RAM, USB storage, ~23K ROMs across 41 systems.

## Single Request Latency (c=1, warm cache)

| Page | P50 | Req/s |
|---|---|---|
| Home (cache hit) | 5ms | 176 |
| Search "mario" | 39ms | 26 |
| Search "sonic" | 40ms | 25 |
| Search "street fighter" | 30ms | 33 |
| Search "a" (broad, 23K matches) | 180ms | 5.5 |
| System page | 1ms | 666–748 |
| Game detail | 1ms | 873 |

## Concurrent Load (50 requests per test)

### Homepage

| Concurrency | Req/s | P50 (ms) | P95 (ms) |
|---|---|---|---|
| 1 | 176 | 5 | 7 |
| 5 | 269 | 18 | 21 |
| 10 | 265 | 37 | 44 |
| 20 | 256 | 70 | 98 |
| 30 | 249 | 97 | 144 |

### Search "mario"

| Concurrency | Req/s | P50 (ms) | P95 (ms) |
|---|---|---|---|
| 1 | 26 | 39 | 40 |
| 5 | 28 | 180 | 184 |
| 10 | 28 | 360 | 363 |
| 20 | 28 | 703 | 728 |
| 30 | 27 | 870 | 1,099 |

### System pages (SNES, Mega Drive)

| Concurrency | Req/s | P50 (ms) | P95 (ms) |
|---|---|---|---|
| 1 | 666–748 | 1 | 2 |
| 5 | 1,227–1,671 | 3–4 | 4–6 |
| 10 | 1,637–1,793 | 5 | 8 |
| 20 | 1,769–1,844 | 10 | 14–16 |
| 30 | 1,755–1,924 | 12 | 20 |

### Game detail

| Concurrency | Req/s | P50 (ms) | P95 (ms) |
|---|---|---|---|
| 1 | 873 | 1 | 1 |
| 5 | 2,029 | 2 | 4 |
| 10 | 1,949 | 5 | 7 |
| 20 | 2,212 | 8 | 13 |
| 30 | 2,182 | 11 | 17 |

## Mixed Concurrent Test

4 endpoints simultaneously at c=5 each (20 total concurrent connections):

| Endpoint | Req/s | P50 (ms) | P95 (ms) |
|---|---|---|---|
| Homepage | 16.2 | 281 | 498 |
| Search "mario" | 8.8 | 544 | 834 |
| Search "sonic" | 8.7 | 542 | 895 |
| Search "street fighter" | 8.6 | 541 | 949 |

## Asset Sizes (v0.4.0)

| Asset | Raw | Gzip |
|---|---|---|
| WASM bundle | 4,000 KB | 848 KB |
| CSS | 88 KB | 14 KB |
| Home HTML | 58 KB | — |
| System page HTML | 20 KB | — |

WASM is served gzip-compressed by the server.

## v0.3.0 → v0.4.0 Comparison

### Single request (c=1)

| Endpoint | v0.3.0 | v0.4.0 | Change |
|---|---|---|---|
| Home | 14ms, 70 req/s | 5ms, 176 req/s | **-64% latency / +151% throughput** |
| Search "mario" | 47ms, 21 req/s | 39ms, 26 req/s | **-17% / +24%** |
| Search "sonic" | 54ms, 18 req/s | 40ms, 25 req/s | **-26% / +39%** |
| Search "street fighter" | 41ms, 24 req/s | 30ms, 33 req/s | **-27% / +38%** |
| Search "a" (broad) | 194ms, 5.2 req/s | 180ms, 5.5 req/s | -7% / +6% |
| System page | 1ms, 918 req/s | 1ms, 666–748 req/s | -19% throughput |
| Game detail | <1ms, 1,036 req/s | 1ms, 873 req/s | -16% throughput |

> Major gains on home (2.8× throughput, ~3× faster) and searches (+24–39%). Small regressions on the already-fast system and game-detail pages (~15–20% throughput) — P50 stays at 1ms, so unmeasurable on the UI.

### Concurrent (c=10)

| Endpoint | v0.3.0 req/s | v0.4.0 req/s | Change |
|---|---|---|---|
| Homepage | 113 | 265 | **+135%** |
| Search "mario" | 22 | 28 | **+27%** |
| System pages | 1,637 | 1,637–1,793 | flat |
| Game detail | 2,210 | 1,949 | -12% |

### Mixed concurrent (c=5 × 4 endpoints)

| Endpoint | v0.3.0 req/s | v0.4.0 req/s | Change |
|---|---|---|---|
| Homepage | 11.8 | 16.2 | **+37%** |
| Search "mario" | 6.8 | 8.8 | **+29%** |
| Search "sonic" | 7.5 | 8.7 | **+16%** |

### Assets

| Asset | v0.3.0 gzip | v0.4.0 gzip | Change |
|---|---|---|---|
| WASM bundle | 995 KB | 848 KB | **-15%** |
| CSS | 14 KB | 14 KB | — |

Key changes since v0.3.0:
- **PHF → runtime SQLite catalog** (the v0.3.0→v0.4.0 headline change): cuts incremental build time from ~90s to ~10s.
- **Async catalog pool** with `deadpool-sqlite` + `prepare_cached` + batch APIs eliminates the single-mutex bottleneck on concurrent lookups.
- **Core split** (`replay-control-core` / `replay-control-core-server`): 89 `#[cfg(target_arch = "wasm32")]` attributes eliminated, 17 wire-type mirrors in `app/src/types.rs` deleted. Build-time wins, no runtime impact expected.
- **Subprocess async migration**: `df`, `ip`, `journalctl`, `tail`, `systemctl`, `pgrep` all use `tokio::process::Command` instead of blocking the reactor.

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

Measured via `/proc/<PID>/status` on the Pi using `tools/pi-memory.sh`. VmRSS is resident set size (physical memory actually in use); VmHWM is the peak RSS since process start.

| State | VmRSS | RssAnon | VmHWM (peak) |
|---|---|---|---|
| Idle (warm, after a few page hits post-restart) | 47 MB | 19 MB | 47 MB |
| Right after full load test (c=30 across all endpoints) | **68 MB** | 39 MB | **189 MB** |
| 60s post-load-test | 62 MB | 33 MB | 189 MB |

Pi 5 2GB host has ~1,720 MB available after OS + buff/cache.

> **jemalloc returns memory well.** VmHWM hit 189 MB during the broad-search burst (`/search?q=a` at c=30, ~3,400ms per response for 50 concurrent requests) where the heap inflates. Steady-state RSS settles to 62 MB within 60 seconds — a drop of ~120 MB back to the OS. Under glibc malloc the retained portion would not be returned (v0.2.0 pre-jemalloc: 324 MB steady-state for the same workload).

## Historical Comparison

| Metric | Pre-optimization | v0.2.0 | v0.3.0 | v0.4.0 |
|---|---|---|---|---|
| Home page (warm, c=1) | 940ms | 19ms | 14ms | **5ms** |
| Home page (c=10) | — | 74 req/s | 113 req/s | **265 req/s** |
| Search "mario" (c=1) | 348ms | 63ms | 47ms | **39ms** |
| Steady-state memory | 324 MB (glibc) | 67 MB (jemalloc) | 67 MB | **62 MB** |
| Mixed load: homepage req/s | 0.60 | 8.3 | 11.8 | **16.2** |
| WASM gzip | — | 1,778 KB | 995 KB | **848 KB** |
| Incremental build time | ~90s | ~90s | ~90s | **~10s** |

## Test Methodology

- Tool: [Apache Bench](https://httpd.apache.org/docs/current/programs/ab.html) (`ab`) via `tools/bench.sh` and `tools/load-test.sh`
- 50 requests per test with warmup pass
- Raw results in `tools/bench-results/`
- Memory read from `/proc/<PID>/status` after the full load-test suite completes
