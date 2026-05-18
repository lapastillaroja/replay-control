# Performance Benchmarks

Last updated: 2026-05-18
Primary benchmark build: 0.4.0 release build, commit `f26a103`.

Replay Control is designed to run quietly on a Raspberry Pi while still handling large game libraries. The practical result from the 0.4.0 measurements is:

- Normal browsing is fast. Home renders in about 10 ms, game pages in 1-3 ms, and common searches in about 26-31 ms on a Pi 5.
- Idle memory is small after startup settles: roughly 45-61 MB on the measured USB library.
- Light browsing stays modest: about 74-77 MB and about 0.6% of one CPU core when no game is running.
- Heavy artificial load raises memory temporarily, but the service remains responsive and memory drops back afterward.
- Large NFS libraries are slower to scan and search, but the app remains usable while maintenance work continues.

The main 0.4.0 numbers below use USB storage with 23,666 games. A separate NFS section records the larger 102 K+ ROM development library.

## Tested Hardware

| Platform | Status |
|---|---|
| Raspberry Pi 5, 2 GB RAM | Measured |
| Raspberry Pi 4 | Measurements pending |

## Everyday Use

### CPU Use

CPU is reported as percent of one core. On a Pi 5, 100% means one core is fully busy.

| State | Pi 5 | Pi 4 |
|---|---:|---|
| Idle, no requests | 0.03% | _pending_ |
| One user browsing, no game running | 0.60% | _pending_ |
| Heavy concurrent load | see [Stress Tests](#stress-tests) | _pending_ |

### Memory Use

Memory is the resident set reported by Linux for the `replay-control` process.

| State | Pi 5 memory in use | Pi 5 peak | Pi 4 |
|---|---:|---:|---|
| After startup scan completed, before browsing | 45-61 MB | 62-66 MB | _pending_ |
| Immediately after light browsing, no game running | 77 MB | 78 MB | _pending_ |
| 60 s after light browsing, no game running | 74 MB | 78 MB | _pending_ |
| Immediately after full load test, no game running | 155 MB | 369 MB | _pending_ |
| 60 s after full load test, no game running | 122 MB | 369 MB | _pending_ |

The peak column is Linux's high-water mark for the process. It does not go down until the process restarts, even after current memory has dropped.

### Startup

On an unchanged USB library, startup verification completed in 1.4 s after the service restart used for the memory test. The app settled into a roughly 45-61 MB idle range once startup work was finished.

First scans and full rebuilds do more work because they must discover files, enrich metadata, queue thumbnails, and, for hash-matched systems, identify ROMs by CRC. Those longer operations are covered in [Library Maintenance on NFS](#library-maintenance-on-nfs).

## Page Load Times

Warm, single-user page requests on Pi 5 with USB storage:

| Page | Typical server time | Notes |
|---|---:|---|
| Home | 9.3 ms | Main library view |
| NES games | 2.5 ms | System list page |
| Arcade games | 2.0 ms | System list page |
| Game detail | 1 ms | From load test, c=1 median |
| Search "mario" | 29 ms | Common search |
| Search "sonic" | 30 ms | Common search |
| Search "street fighter" | 26 ms | Multi-word search |
| Search "a" | 158 ms | Broad worst-case search |

## Download Sizes

The web app's static files. WASM is served gzip-compressed by the server.

| File | Raw | Gzip |
|---|---:|---:|
| WASM bundle | 4,630 KB | 1,014 KB |
| CSS | 98 KB | 16 KB |
| Home HTML | 57 KB | - |
| NES games HTML | 22 KB | - |

## USB Stress Tests

These numbers come from Apache Bench (`ab`) issuing 50 requests per endpoint. This is intentionally heavier than normal use; it is a regression and robustness check.

### Concurrent Throughput

#### Homepage

| Concurrency | Req/s | P50 | P95 |
|---|---:|---:|---:|
| 1 | 9.7 | 7 ms | 8 ms |
| 5 | 253.0 | 19 ms | 24 ms |
| 10 | 250.2 | 38 ms | 45 ms |
| 20 | 255.2 | 72 ms | 85 ms |
| 30 | 263.2 | 97 ms | 119 ms |

The c=1 homepage run included one long outlier, so Req/s is not representative there. The median and P95 are the useful values for that row.

#### Search

| Query | c=1 Req/s | c=1 P50 | c=1 P95 | c=10 Req/s | c=10 P50 | c=10 P95 |
|---|---:|---:|---:|---:|---:|---:|
| "mario" | 34.1 | 29 ms | 31 ms | 36.9 | 264 ms | 289 ms |
| "sonic" | 32.8 | 30 ms | 34 ms | 36.7 | 268 ms | 284 ms |
| "street fighter" | 38.6 | 26 ms | 29 ms | 55.5 | 171 ms | 192 ms |
| "a" | 6.2 | 158 ms | 166 ms | 9.4 | 1,056 ms | 1,172 ms |

#### System and Game Pages

| Endpoint | c=1 Req/s | c=1 P50 | c=1 P95 | c=10 Req/s | c=10 P50 | c=10 P95 |
|---|---:|---:|---:|---:|---:|---:|
| SNES games | 638.1 | 2 ms | 2 ms | 1,350.5 | 7 ms | 9 ms |
| Mega Drive games | 664.8 | 1 ms | 2 ms | 1,338.6 | 7 ms | 10 ms |
| Game detail | 682.0 | 1 ms | 2 ms | 1,623.9 | 5 ms | 8 ms |

#### Mixed Concurrent Test

Four endpoints were hit at the same time, each with concurrency 5.

| Endpoint | Req/s | P50 | P95 |
|---|---:|---:|---:|
| Home | 26.1 | 204 ms | 216 ms |
| Search "mario" | 13.8 | 367 ms | 384 ms |
| Search "sonic" | 13.3 | 378 ms | 408 ms |
| Search "street fighter" | 13.1 | 378 ms | 409 ms |

## Library Maintenance on NFS

NFS is a harder workload because scans must walk a remote ROM tree and rebuilds may stream large files to recompute CRCs. Current builds keep `catalog.sqlite`, `library.db`, and `external_metadata.db` on the Pi, so normal page rendering is much less tied to NFS latency than library maintenance is.

Earlier NFS library maintenance measurements on a 95,495-ROM development library:

| Operation | Duration | Hash behavior |
|---|---:|---|
| Startup cache verification, already fresh | ~4.5 s from service start | No system rescan needed |
| Manual rescan | 194.1 s | Reused 17,490 exact CRC cache entries and 16 same-size entries; recomputed 2 hashes |
| Manual rebuild | 636.0 s | Forced 17,508 CRC reads; skipped 2 CD/image entries in hybrid folders |

0.4.0 validation on a larger 99,964-ROM NFS library measured the deferred-identity pipeline:

| Operation | Duration | Hash behavior |
|---|---:|---|
| Foreground populate | 280.1 s | Reconciled every visible system and enriched rows before identity finished |
| Background identity | 437.9 s | Forced 19,019 hash-eligible rows through two 200-row workers |
| End-to-end build | 718.0 s | Library remained browsable while identity continued |

Follow-up validation on a 102,662-ROM NFS library measured a normal manual rescan after identity had already completed:

| Operation | Duration | Hash behavior |
|---|---:|---|
| Manual rescan | 313.3 s | Cached identity reused; 12 hash-eligible systems skipped because no rows needed matching |

The key result is responsiveness. The foreground library becomes available before the hash tail finishes, and the app stays usable during the remaining NFS reads.

## NFS Large-Library Serving

Measured on Pi 5 against the 102 K+ ROM NFS development library after deploying the same 0.4.0 release build from commit `f26a103`.

### Warm Page Requests

| Page | TTFB | Total | Response size |
|---|---:|---:|---:|
| Home | 7.7 ms | 8.0 ms | 8.6 KB |
| NES games | 2.0 ms | 9.0 ms | 10.9 KB |
| Arcade games | 2.0 ms | 9.4 ms | 9.6 KB |

### NFS Load Test

Search is much heavier here than on USB because the NFS development library is more than four times larger.

| Endpoint | c=1 Req/s | c=1 P50 | c=1 P95 | c=10 Req/s | c=10 P50 | c=10 P95 |
|---|---:|---:|---:|---:|---:|---:|
| Home | 148.5 | 7 ms | 8 ms | 282.2 | 33 ms | 40 ms |
| Search "mario" | 9.5 | 104 ms | 111 ms | 10.2 | 966 ms | 1,042 ms |
| Search "sonic" | 9.0 | 111 ms | 115 ms | 9.6 | 1,015 ms | 1,098 ms |
| Search "street fighter" | 5.4 | 94 ms | 98 ms | 14.2 | 682 ms | 768 ms |
| Search "a" | 1.3 | 800 ms | 839 ms | 1.8 | 5,410 ms | 5,888 ms |
| SNES games | 643.5 | 1 ms | 2 ms | 1,382.8 | 7 ms | 9 ms |
| Mega Drive games | 646.7 | 1 ms | 2 ms | 1,414.1 | 6 ms | 10 ms |
| Game detail | 710.0 | 1 ms | 2 ms | 1,593.6 | 6 ms | 8 ms |

Memory around the same run:

| State | Memory in use | Peak since service start |
|---|---:|---:|
| Idle after release install | 55 MB | 90 MB |
| Immediately after full load test | 157 MB | 967 MB |
| 60 s after full load test | 157 MB | 967 MB |

The high-water mark came from artificial broad-search stress on a very large library. It is not a normal browsing footprint, but it is useful regression data: the process stayed running and served every request class through the stress run.

## Historical Comparison

The current 0.4.0 release is dramatically faster than the original unoptimized implementation and remains comfortably within Pi 5 limits for normal use.

| Metric | Pre-optimization | v0.2.0 | v0.3.0 | 0.4.0 |
|---|---:|---:|---:|---:|
| Home page, warm c=1 | 940 ms | 19 ms | 14 ms | 9 ms |
| Search "mario", c=1 | 348 ms | 63 ms | 47 ms | 29 ms |
| Search "a", c=1 | - | - | 194 ms | 158 ms |
| Mixed homepage stress | 0.60 req/s | 8.3 req/s | 11.8 req/s | 26.1 req/s |
| Steady memory after startup | 324 MB | 67 MB | 67 MB | 45-61 MB |
| WASM gzip | - | 1,778 KB | 995 KB | 1,014 KB |
| Incremental build time | ~90 s | ~90 s | ~90 s | ~10 s |

Older intermediate 0.4.0 validation runs had slightly faster home stress throughput and a smaller WASM bundle. The final 0.4.0 numbers above are the release baseline because they include the completed library-build, metadata, and startup-scan changes.

## Test Methodology

- **CPU**: `tools/pi-cpu.sh` reads `/proc/<pid>/stat` and reports CPU relative to one core. `--browse` simulates one user clicking through home, system pages, search, game detail, and manuals every ~2 s.
- **Memory**: `tools/pi-memory.sh` reads `/proc/<PID>/status`: VmRSS (memory in use), VmHWM (peak since process start), and RssAnon.
- **Page and asset benchmarks**: `tools/bench.sh`, with Lighthouse skipped for the recorded release runs.
- **Stress tests**: `tools/load-test.sh`, which uses Apache Bench (`ab`) with 50 requests per endpoint.
- All current USB measurements were taken on a freshly restarted Pi 5, USB storage, no game running, default jemalloc configuration.
- ab's "Failed" column counts response-size variance, not HTTP errors. All requests returned successfully.
- Raw results are stored in `tools/bench-results/`.
