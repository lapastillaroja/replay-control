# Performance Benchmarks

Last updated: 2026-05-06
Build: v0.4.0-beta.9 (release profile, commit `92d211e`)

This page records what Replay Control actually costs to run — CPU, memory, page-load time, and download size — in normal use, plus what it does under artificial stress so we can spot regressions between releases.

Numbers below assume:

- ~23 K ROMs across 41 systems on USB storage.
- The Pi was rebooted before measurements.

## Tested Hardware

Each table reports numbers per platform side-by-side. New platforms can be added by filling the column.

| Platform | Status |
|---|---|
| Raspberry Pi 5, 2 GB RAM | Measured |
| Raspberry Pi 4 | Measurements pending |

## CPU Use

How much CPU `replay-control` uses, sampled with `tools/pi-cpu.sh`. Numbers are percent of **one core** — 100 % means one core is fully busy; 400 % means all four cores on a Pi are fully busy.

When a libretro core is running on RePlayOS, the now-playing detector wakes every 4 s and walks the running game's memory looking for the active ROM. That adds a small CPU cost on top of the numbers below — variable depending on the core and game. See [now-playing.md](now-playing.md).

| State | Pi 5 | Pi 4 |
|---|---|---|
| Idle (no requests, no game running) | 0.03 % | _pending_ |
| One user browsing (a click every couple of seconds) | 0.6 % | _pending_ |
| Heavy concurrent load | see [Stress Tests](#stress-tests) | _pending_ |

## Memory Use

Memory the `replay-control` process is using, read from `/proc` via `tools/pi-memory.sh`. "Memory in use" is the resident set the kernel reports — physical RAM the process is occupying right now.

Memory naturally shrinks over time. After a burst of activity, the heap keeps tapering for several minutes as unused pages are returned to the operating system. By the time you check minutes later, the process is back near its working set.

| State | Pi 5 | Pi 4 |
|---|---|---|
| Steady state (one or two users browsing sporadically) | ~60–110 MB | _pending_ |
| A few minutes after a burst of activity | ~110–130 MB | _pending_ |

## What Happens at Startup

When `replay-control` starts up — fresh boot, service restart, or first run on a new storage device — it runs a one-time burst of background work: metadata refresh, library scan, catalog verification, thumbnail manifest checks. While that work runs:

- **CPU** briefly rises to a few percent of one core.
- **Memory** peaks higher than usual (around 180–220 MB on Pi 5) for the first 30–60 seconds.

Both settle back to the steady-state numbers above within a couple of minutes. You only see the spike if you check `htop` immediately after a boot or service restart.

## Page Load Times

How long the most-visited pages take to render on the server (P50 = the typical request; half are faster, half are slower). What a user sees when they click a link.

| Page | Pi 5 | Pi 4 | Notes |
|---|---|---|---|
| Home | 5 ms | _pending_ | Cached |
| System page (SNES, Mega Drive) | 1 ms | _pending_ | |
| Game detail | 1 ms | _pending_ | |
| Search "mario" | 27 ms | _pending_ | |
| Search "sonic" | 28 ms | _pending_ | |
| Search "street fighter" | 23 ms | _pending_ | |
| Search "a" (broad — 23 K matches) | 133 ms | _pending_ | Worst-case search shape |

## Download Sizes

The web app's static files. WASM is served gzip-compressed by the server. These don't depend on the Pi model.

| File | Raw | Gzip |
|---|---|---|
| WASM bundle | 4,201 KB | 882 KB |
| CSS | 95 KB | 15 KB |
| Home HTML | 58 KB | — |
| System page HTML (NES) | 21 KB | — |

## Storage Caveat

The numbers above use **USB storage**. Switching to NFS over WiFi roughly **3–4× slows** the heavier pages — Home concurrent throughput dropped from 282 to 184 req/s, system pages from 933 to 241 req/s, and the rendered HTML for `/games/<system>` ballooned because the catalog content grew on the network share. The slowdown is dominated by the SQLite catalog and ROM index living on the network share, not by the app itself. Use USB or the internal SD/NVMe for performance, NFS for convenience.

## Stress Tests

These numbers come from Apache Bench (`ab`) firing dozens to hundreds of concurrent requests at the Pi for minutes at a time. **That isn't how the app is used in practice** — a typical install gets clicked by one or two people. These tests exist to:

- Detect performance regressions between releases.
- Probe upper bounds — what happens if a script or scraper hits the appliance.
- Compare versions on a like-for-like basis.

Read these as a robustness check, not as a representative resource budget. In the tables below:

- **Concurrency** is how many requests are in flight at the same time.
- **Req/s** is sustained requests-per-second.
- **P50 / P95** are the median and 95th-percentile response times — the "typical" and "near-worst-case" delays in the test.

### Concurrent throughput (50 requests per test)

#### Homepage

| Concurrency | Pi 5 Req/s | Pi 5 P50 (ms) | Pi 5 P95 (ms) | Pi 4 |
|---|---|---|---|---|
| 1 | 177 | 5 | 7 | _pending_ |
| 5 | 289 | 17 | 21 | _pending_ |
| 10 | 282 | 33 | 43 | _pending_ |
| 20 | 291 | 62 | 91 | _pending_ |
| 30 | 250 | 89 | 155 | _pending_ |

#### Search "mario"

| Concurrency | Pi 5 Req/s | Pi 5 P50 (ms) | Pi 5 P95 (ms) | Pi 4 |
|---|---|---|---|---|
| 1 | 37 | 27 | 29 | _pending_ |
| 5 | 43 | 116 | 131 | _pending_ |
| 10 | 43 | 227 | 245 | _pending_ |
| 20 | 43 | 452 | 485 | _pending_ |
| 30 | 41 | 560 | 730 | _pending_ |

#### System pages (SNES, Mega Drive)

| Concurrency | Pi 5 Req/s | Pi 5 P50 (ms) | Pi 5 P95 (ms) | Pi 4 |
|---|---|---|---|---|
| 1 | 703–714 | 1 | 2 | _pending_ |
| 5 | 1,102–1,595 | 3–4 | 4–6 | _pending_ |
| 10 | 1,626–1,721 | 5–6 | 8–9 | _pending_ |
| 20 | 1,704–1,743 | 9–10 | 15 | _pending_ |
| 30 | 1,776–1,825 | 13–14 | 21–23 | _pending_ |

#### Game detail

| Concurrency | Pi 5 Req/s | Pi 5 P50 (ms) | Pi 5 P95 (ms) | Pi 4 |
|---|---|---|---|---|
| 1 | 771 | 1 | 2 | _pending_ |
| 5 | 1,672 | 3 | 5 | _pending_ |
| 10 | 1,784 | 5 | 7 | _pending_ |
| 20 | 1,774 | 9 | 21 | _pending_ |
| 30 | 1,984 | 12 | 20 | _pending_ |

#### Mixed concurrent (4 endpoints simultaneously, c=5 each)

| Endpoint | Pi 5 Req/s | Pi 5 P50 (ms) | Pi 5 P95 (ms) | Pi 4 |
|---|---|---|---|---|
| Homepage | 29.0 | 180 | 200 | _pending_ |
| Search "mario" | 15.1 | 320 | 372 | _pending_ |
| Search "sonic" | 14.9 | 326 | 375 | _pending_ |
| Search "street fighter" | 14.7 | 330 | 375 | _pending_ |

### Memory under stress

| State | Pi 5 in use | Pi 5 peak | Pi 4 |
|---|---|---|---|
| Idle, post-startup-settle | ~110 MB | ~220 MB | _pending_ |
| Right after the stress burst | ~145 MB | ~310 MB | _pending_ |
| 60 s after the burst | ~130 MB | ~310 MB | _pending_ |
| Hours after the burst | ~110 MB | ~310 MB | _pending_ |

The peak is recorded forever (it's the highest the process ever reached), but current memory drops as unused pages are returned. Real-world workloads never approach this peak.

## Version Comparisons

Track release-over-release changes on the same hardware. Single-page latency, idle CPU, and idle RAM are flat or improving across the line; stress-test throughput has improved substantially.

### v0.4.0 → v0.4.0-beta.9 (Pi 5)

#### Single request

| Endpoint | v0.4.0 | v0.4.0-beta.9 | Change |
|---|---|---|---|
| Home | 5 ms, 176 req/s | 5 ms, 177 req/s | flat |
| Search "mario" | 38 ms, 26 req/s | 27 ms, 37 req/s | **+42 % throughput** |
| Search "sonic" | 40 ms, 25 req/s | 28 ms, 36 req/s | **+44 %** |
| Search "street fighter" | 30 ms, 33 req/s | 23 ms, 43 req/s | **+30 %** |
| Search "a" (broad) | 183 ms, 5.5 req/s | 133 ms, 7.5 req/s | **+36 %** |
| System page | 1 ms, 707–724 req/s | 1 ms, 703–714 req/s | flat |
| Game detail | 1 ms, 792 req/s | 1 ms, 771 req/s | flat |

#### Mixed concurrent (c=5 × 4 endpoints)

| Endpoint | v0.4.0 req/s | v0.4.0-beta.9 req/s | Change |
|---|---|---|---|
| Homepage | 15.9 | 29.0 | **+82 %** |
| Search "mario" | 8.9 | 15.1 | **+70 %** |
| Search "sonic" | 8.8 | 14.9 | **+69 %** |

#### Memory steady state

| Metric | v0.4.0 | v0.4.0-beta.9 |
|---|---|---|
| Idle, post-startup-settle | ~50 MB | ~60–110 MB |
| Steady, between bursts | ~68 MB | ~110–130 MB |

The steady-state climb is the database-pool redesign (one DB → four: catalog, library, external_metadata, user_data).

#### Downloads

| Asset | v0.4.0 gzip | v0.4.0-beta.9 gzip |
|---|---|---|
| WASM bundle | 843 KB | 882 KB |
| CSS | 14 KB | 15 KB |

### v0.3.0 → v0.4.0 (Pi 5)

| Endpoint | v0.3.0 | v0.4.0 | Change |
|---|---|---|---|
| Home (c=1) | 14 ms, 70 req/s | 5 ms, 176 req/s | **+151 % throughput** |
| Search "mario" (c=1) | 47 ms, 21 req/s | 38 ms, 26 req/s | **+24 %** |
| Search "a" (broad) | 194 ms, 5.2 req/s | 183 ms, 5.5 req/s | +6 % |
| Mixed homepage (c=5×4) | 11.8 req/s | 15.9 req/s | **+35 %** |
| WASM gzip | 995 KB | 843 KB | **−15 %** |
| Incremental build time | ~90 s | ~10 s | **−89 %** |

Key changes since v0.3.0: PHF→runtime SQLite catalog, async catalog pool, core/core-server split, subprocess async migration.

### v0.2.0 → v0.3.0 (Pi 5)

| Endpoint | v0.2.0 | v0.3.0 | Change |
|---|---|---|---|
| Home (c=1) | 19 ms, 51 req/s | 14 ms, 70 req/s | **+37 %** |
| Search "mario" (c=1) | 63 ms, 16 req/s | 47 ms, 21 req/s | **+33 %** |
| Mixed homepage (c=5×4) | 8.3 req/s | 11.8 req/s | **+42 %** |
| WASM gzip | 1,778 KB | 995 KB | **−44 %** |

Key improvements: GameInfo refactor, curl→reqwest migration, release-profile WASM optimizations.

## Historical Comparison (Pi 5)

| Metric | Pre-optimization | v0.2.0 | v0.3.0 | v0.4.0 | v0.4.0-beta.9 |
|---|---|---|---|---|---|
| Home page (warm, c=1) | 940 ms | 19 ms | 14 ms | 5 ms | **5 ms** |
| Home page (c=10, stress) | — | 74 req/s | 113 req/s | 278 req/s | **282 req/s** |
| Search "mario" (c=1) | 348 ms | 63 ms | 47 ms | 38 ms | **27 ms** |
| Steady-state memory (between bursts) | 324 MB (glibc) | 67 MB (jemalloc) | 67 MB | 68 MB | **~110 MB** |
| Mixed homepage req/s (stress) | 0.60 | 8.3 | 11.8 | 15.9 | **29.0** |
| WASM gzip | — | 1,778 KB | 995 KB | 843 KB | **882 KB** |
| Incremental build time | ~90 s | ~90 s | ~90 s | ~10 s | ~10 s |
| Idle CPU (one core) | — | — | — | — | **0.03 %** |
| One-user browse CPU (one core) | — | — | — | — | **0.6 %** |

Pi 4 history will be tracked once initial measurements are taken.

## Test Methodology

- **CPU**: `tools/pi-cpu.sh` reads `/proc/<pid>/stat` (utime + stime) at two timestamps, scales by `CLK_TCK` and the configured duration. CPU% is reported relative to one core. `--browse` simulates one user clicking through home / system / game / manuals / search every ~2 s.
- **Memory**: `tools/pi-memory.sh` reads `/proc/<PID>/status` — VmRSS (memory in use), VmHWM (peak since process start), RssAnon (heap portion).
- **Stress / load tests**: [Apache Bench](https://httpd.apache.org/docs/current/programs/ab.html) (`ab`) via `tools/bench.sh` and `tools/load-test.sh`. 50 requests per test with a warmup pass.
- All current measurements were taken on a freshly rebooted Pi, USB storage, no game running, default jemalloc configuration.
- ab's "Failed" column counts response-size variance, not HTTP errors — broad searches return slightly different result orderings between runs and ab flags them. All requests returned 200.
- Raw results in `tools/bench-results/`.
