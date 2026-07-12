# Performance Benchmarks

Last updated: 2026-07-12
Primary benchmark build: 1.0.0 release build, commit `433e899`. Measured over HTTPS (now on by default) with an authenticated session, so server times include the TLS handshake and the session check — a few ms above the earlier plain-HTTP figures. Search and stress-load figures are carried over from 0.9.0 (the load harness has not yet been re-run against the authenticated HTTPS endpoint).

> **2026-07-12 re-measurement round (web-framework upgrade).** A candidate build carrying the leptos 0.7→0.8 upgrade was benchmarked against the immediately-preceding release (1.1.3, still on leptos 0.7) on the same Pi 5 and the same libraries (USB 26,777 games, NFS 93,649). The A/B isolates what the framework upgrade costs from what two months of new features cost. Headline: the only framework-attributable regression is a **larger download bundle** (+37% compressed); everyday memory and CPU are unchanged (memory is slightly lower), and search is untouched. The separate, larger drop in raw page throughput since 1.0.0 is **feature growth, not the framework** — see [Regressions and Analysis](#regressions-and-analysis). Baselines below still reflect 1.0.0 unless noted.

Replay Control is designed to run quietly on a Raspberry Pi while still handling large game libraries. The practical result from the 1.0.0 measurements is:

- Normal browsing is fast. Home renders in about 9 ms of server time over HTTPS (TLS + session check included), system and game pages in 5-6 ms, and common searches in about 35 ms (USB) on a Pi 5.
- Idle CPU is negligible: effectively 0% of one core with no requests, and under 1% (USB) while one person browses.
- Idle memory settles to about 99 MB resident (41 MB heap) within ~3 minutes of startup. It is modestly higher than earlier releases, mostly because the on-device game catalog is memory-mapped for fast lookups — shared, file-backed cache the kernel reclaims under pressure, not heap. See [Regressions and Analysis](#regressions-and-analysis).
- Heavy artificial load peaks variably (around 0.7-1.1 GB across runs, as large result sets are built in memory) and the service stays responsive; it settles back to roughly 190-250 MB, though the allocator retains the grown heap rather than returning fully to idle.
- Search cost scales with the number of games, not with where they are stored. The larger (NFS-sourced) test library makes broad search heavier purely because it has more rows. The library database that search reads is on the Pi's local disk regardless of storage mode; the user database (favorites, recents, custom box art) does live on the ROM storage, but search does not read it.

Two libraries were measured on the same Pi 5: a **USB** library with **26,777** games and a larger **NFS** development library with **103,896** games. Page rendering and memory are nearly identical between the two because the catalog and library databases live on the Pi's local disk; storage mode mostly affects scanning and search-result volume.

## Tested Hardware

| Platform | Status |
|---|---|
| Raspberry Pi 5, 2 GB RAM | Measured |
| Raspberry Pi 4 | Measurements pending |

## Everyday Use

### CPU Use

CPU is reported as percent of one core. On a Pi 5, 100% means one core is fully busy; 400% would be the whole Pi.

| State | Pi 5 - USB (26,777) | Pi 5 - NFS (103,896) | Pi 4 |
|---|---:|---:|---|
| Idle, no requests | 0.00% | 0.03% | _pending_ |
| One user browsing, no game running | 0.87% | 2.13% | _pending_ |
| Heavy concurrent load | see [Stress Tests](#stress-tests) | see [Stress Tests](#stress-tests) | _pending_ |

The larger NFS library costs more CPU per request because each search and listing touches more rows.

### Memory Use

Memory is the resident set (VmRSS) for the `replay-control` process; "heap" is the anonymous portion (RssAnon). The rest is shared, file-backed cache (mostly the memory-mapped catalog), which the kernel reclaims under pressure. Memory is essentially storage-independent — the catalog and library databases sit on the Pi's local disk regardless of storage mode — so these were measured on the larger NFS library (the worst case for heap and load peaks). Each state is sampled right after the event, then 1, 3, and 5 minutes later; "Peak" is Linux's high-water mark (VmHWM) since process start.

| State | right after | +1 min | +3 min | +5 min | Peak |
|---|---:|---:|---:|---:|---:|
| Idle after startup — resident | 60 MB | 113 MB | 99 MB | 99 MB | 131 MB |
| Idle after startup — heap | 37 MB | 57 MB | 41 MB | 41 MB | — |
| After full load test — resident | 212-257 MB | 193-251 MB | 193-251 MB | 193-251 MB | 0.7-1.1 GB |
| After full load test — heap | 148-191 MB | 129-186 MB | 129-186 MB | 129-186 MB | — |

Idle settles to about 99 MB resident (41 MB heap) within ~3 minutes; the 1-minute bump is startup verification touching catalog pages, which the kernel then reclaims. Heavy broad-search stress peaks **variably — between about 730 MB and 1.1 GB across runs** — as large result sets are built in memory, then settles to roughly 190-250 MB; the allocator retains the grown heap (~130-190 MB) rather than returning to the idle 41 MB. The peak is run-dependent because it tracks how many concurrent broad searches overlap, which is exactly what the broad-search cap in [Regressions and Analysis](#regressions-and-analysis) would bound. The 2 GB Pi stayed up with headroom throughout (≥1.5 GB reported available).

### Startup

A warm restart on an unchanged library settles in a few seconds (the per-system stats stay fresh, so only verification runs). A cold storage switch triggers a rescan: switching to the 26,777-game USB library and reaching "all systems fresh" took under a minute this round. First scans and full rebuilds of large libraries do much more work and are covered in [Library Maintenance on NFS](#library-maintenance-on-nfs).

## Page Load Times

Warm, single-user requests on Pi 5. "Server time" is time-to-first-byte (SSR processing); "Total" includes transfer.

| Page | USB server time | USB total | NFS server time | NFS total | Notes |
|---|---:|---:|---:|---:|---|
| Home | 8.6 ms | 10.4 ms | 8.9 ms | 9.3 ms | Main library view |
| System list (NES) | 5.5 ms | 6.0 ms | 5.2 ms | 11.8 ms | NFS NES list is larger |
| System list (Arcade) | 5.8 ms | 11.1 ms | 5.3 ms | 12.4 ms | |
| Game detail | ~1 ms | ~2 ms | ~1 ms | ~2 ms | From load test, c=1 median |
| Search "mario" | 35 ms | — | 114 ms | — | c=1 median, scales with library |
| Search "street fighter" | 27 ms | — | 89 ms | — | c=1 median |
| Search "a" | 304 ms | — | 1,357 ms | — | Broad worst-case search |

## Download Sizes

The web app's static files. WASM is served gzip-compressed by the server.

| File | Raw | Gzip | Brotli |
|---|---:|---:|---:|
| WASM bundle | 4,695 KB | 1,429 KB | 944 KB |
| CSS | 177 KB | 31 KB | - |
| Home HTML | 57 KB | - | - |

This is a CI release build, so the WASM bundle includes the `wasm-opt -Oz` pass — the raw bundle is 4,725 KB, down from 0.9.0's 5,318 KB. This benchmark build served the gzip variant (1,431 KB, at maximum gzip). 1.0.0 adds brotli pre-compression: browsers that accept `br` download **944 KB — about 485 KB (34%) smaller than gzip**. Gzip remains the fallback for clients or builds without brotli.

**leptos 0.8 upgrade (2026-07-12 round).** The framework upgrade grows the WASM bundle materially. Both figures below are release builds with the same `wasm-opt -Oz` pass; the only difference is the framework version, so the delta is framework codegen, not features (confirmed by A/B against the leptos-0.7 v1.1.3 release, which matches the 1.0.0 column).

| WASM bundle | Raw | Gzip | Brotli |
|---|---:|---:|---:|
| leptos 0.7 (1.0.0 / v1.1.3) | 4,695–4,805 KB | 1,429–1,438 KB | 944–946 KB |
| leptos 0.8 (candidate) | 6,139 KB | 1,955 KB | 1,296 KB |
| **Δ** | **+27.8%** | **+36.8%** | **+37.3%** |

Modern browsers accept `br`, so the practical cost is **+350 KB (+37%) per cold load**. This is the one clear framework-attributable regression; it lands on first-load download, not on per-request server time.

## Stress Tests

These numbers come from Apache Bench (`ab`) issuing 50 requests per endpoint at increasing concurrency. This is intentionally heavier than normal use; it is a regression and robustness check. `ab`'s "Failed" column counts response-size variance (search results differ run to run), not HTTP errors — all requests returned successfully.

### USB Library (26,777 games)

#### Homepage

| Concurrency | Req/s | P50 | P95 |
|---|---:|---:|---:|
| 1 | 152.9 | 6 ms | 8 ms |
| 5 | 203.7 | 23 ms | 28 ms |
| 10 | 209.2 | 47 ms | 52 ms |
| 20 | 207.4 | 91 ms | 101 ms |
| 30 | 215.7 | 123 ms | 138 ms |

#### Search

| Query | c=1 Req/s | c=1 P50 | c=1 P95 | c=10 Req/s | c=10 P50 | c=10 P95 |
|---|---:|---:|---:|---:|---:|---:|
| "mario" | 28.2 | 35 ms | 37 ms | 28.7 | 348 ms | 352 ms |
| "sonic" | 25.4 | 39 ms | 41 ms | 27.7 | 359 ms | 366 ms |
| "street fighter" | 37.3 | 27 ms | 30 ms | 40.1 | 245 ms | 258 ms |
| "a" | 3.2 | 303 ms | 313 ms | 5.0 | 1,971 ms | 2,034 ms |

#### System and Game Pages

| Endpoint | c=1 Req/s | c=1 P50 | c=1 P95 | c=10 Req/s | c=10 P50 | c=10 P95 |
|---|---:|---:|---:|---:|---:|---:|
| SNES games | 563.6 | 2 ms | 2 ms | 1,374.0 | 7 ms | 10 ms |
| Mega Drive games | 626.7 | 2 ms | 2 ms | 1,340.2 | 7 ms | 10 ms |
| Game detail | 699.6 | 1 ms | 2 ms | 1,551.2 | 6 ms | 8 ms |

#### Mixed Concurrent Test

Four endpoints hit at the same time, each at concurrency 5.

| Endpoint | Req/s | P50 | P95 |
|---|---:|---:|---:|
| Home | 11.1 | 478 ms | 510 ms |
| Search "mario" | 10.5 | 499 ms | 512 ms |
| Search "sonic" | 10.2 | 498 ms | 511 ms |
| Search "street fighter" | 10.0 | 499 ms | 517 ms |

### NFS Library (103,896 games)

Search is much heavier here because the NFS development library is roughly four times larger.

#### Homepage

| Concurrency | Req/s | P50 | P95 |
|---|---:|---:|---:|
| 1 | 168.4 | 6 ms | 7 ms |
| 5 | 262.1 | 18 ms | 25 ms |
| 10 | 282.7 | 33 ms | 37 ms |
| 20 | 295.7 | 61 ms | 71 ms |
| 30 | 292.7 | 86 ms | 111 ms |

#### Search

| Query | c=1 Req/s | c=1 P50 | c=1 P95 | c=10 Req/s | c=10 P50 | c=10 P95 |
|---|---:|---:|---:|---:|---:|---:|
| "mario" | 8.8 | 114 ms | 118 ms | 9.3 | 1,071 ms | 1,081 ms |
| "sonic" | 8.6 | 113 ms | 120 ms | 9.4 | 1,065 ms | 1,071 ms |
| "street fighter" | 11.2 | 89 ms | 92 ms | 12.9 | 771 ms | 784 ms |
| "a" | 0.7 | 1,357 ms | 1,407 ms | 1.1 | 9,443 ms | 9,530 ms |

#### System and Game Pages

| Endpoint | c=1 Req/s | c=1 P50 | c=1 P95 | c=10 Req/s | c=10 P50 | c=10 P95 |
|---|---:|---:|---:|---:|---:|---:|
| SNES games | 572.7 | 2 ms | 2 ms | 1,296.3 | 7 ms | 10 ms |
| Mega Drive games | 628.7 | 1 ms | 2 ms | 1,370.9 | 7 ms | 9 ms |
| Game detail | 662.2 | 1 ms | 2 ms | 1,471.0 | 6 ms | 8 ms |

#### Mixed Concurrent Test

Four endpoints hit at the same time, each at concurrency 5.

| Endpoint | Req/s | P50 | P95 |
|---|---:|---:|---:|
| Home | 3.7 | 1,430 ms | 1,526 ms |
| Search "mario" | 3.5 | 1,510 ms | 1,561 ms |
| Search "sonic" | 3.4 | 1,506 ms | 1,522 ms |
| Search "street fighter" | 3.3 | 1,509 ms | 1,530 ms |

## Library Maintenance on NFS

NFS is a harder workload because scans must walk a remote ROM tree and rebuilds may stream large files to recompute CRCs. Current builds keep the catalog, library database, and external-metadata database on the Pi, so normal page rendering is much less tied to NFS latency than library maintenance is.

The following are earlier (0.4.0-era) NFS library-maintenance measurements; they were not re-timed in the 0.9.0 round and are kept here as directional references.

Maintenance on a 95,495-ROM development library:

| Operation | Duration | Hash behavior |
|---|---:|---|
| Startup library verification, already fresh | ~4.5 s from service start | No system rescan needed |
| Manual rescan | 194.1 s | Reused 17,490 exact stored CRC entries and 16 same-size entries; recomputed 2 hashes |
| Manual rebuild | 636.0 s | Forced 17,508 CRC reads; skipped 2 CD/image entries in hybrid folders |

Deferred-identity pipeline on a 99,964-ROM NFS library:

| Operation | Duration | Hash behavior |
|---|---:|---|
| Foreground populate | 280.1 s | Reconciled every visible system and enriched rows before identity finished |
| Background identity | 437.9 s | Forced 19,019 hash-eligible rows through two 200-row workers |
| End-to-end build | 718.0 s | Library remained browsable while identity continued |

The key result is responsiveness: the foreground library becomes available before the hash tail finishes, and the app stays usable during the remaining NFS reads.

## Historical Comparison

0.9.0 keeps normal browsing in the same fast range as 0.4.0 while the library has grown and gained much richer per-game metadata (achievements, resources, series, expanded catalog). The two visible costs are a modest rise in idle memory (the memory-mapped catalog, plus heap the allocator retains after heavy load) and heavier worst-case search on very large libraries.

| Metric | Pre-optimization | v0.2.0 | v0.3.0 | 0.4.0 | 0.9.0 |
|---|---:|---:|---:|---:|---:|
| Home page, warm c=1 (server time) | 940 ms | 19 ms | 14 ms | 9 ms | 6 ms |
| Search "mario", c=1 (USB) | 348 ms | 63 ms | 47 ms | 29 ms | 35 ms |
| Search "a", c=1 (NFS) | - | - | 194 ms | 800 ms* | 1,357 ms |
| Mixed homepage stress (USB) | 0.60 req/s | 8.3 req/s | 11.8 req/s | 26.1 req/s | 11.1 req/s |
| Steady memory after startup | 324 MB | 67 MB | 67 MB | 45-61 MB | ~99 MB |
| WASM gzip | - | 1,778 KB | 995 KB | 1,014 KB | 1,110 KB |

\* The 0.4.0 "a" figure is from its NFS section (102 K library); the 0.4.0 USB "a" was 158 ms.

## Regressions and Analysis

Two metrics moved the wrong way versus 0.4.0. Both are understood; neither is a correctness bug.

### Higher memory use (idle ≈55 MB → ≈99 MB; heap retained after load)

**What it is.** Settled idle memory is about 99 MB resident (41 MB heap) — up from 0.4.0's 45-61 MB, but far less than an early mis-measurement suggested. Resident memory taken right after startup is lower (~60 MB) and briefly bumps to ~113 MB during startup verification before the kernel reclaims it. The more durable effect is after heavy load: resident settles to roughly 190-250 MB and the allocator holds onto ~130-190 MB of heap rather than returning to the idle 41 MB. The transient peak during broad-search stress is run-dependent and ranged ~0.7-1.1 GB across repeats.

**Cause.** Two things. First, the on-device game catalog is memory-mapped for fast lookups (about 64 MB of mapping against a ~62 MB catalog) and the catalog has grown; those pages are shared, file-backed, and reclaimable, not heap. Second, the allocator (jemalloc) retains freed pages after a memory spike (broad-search stress) instead of returning them to the OS, so heap stays elevated until the next spike reuses it. Available system memory stayed around 1.6 GB throughout on the 2 GB Pi.

**Options.**
- Accept it — idle stays ~99 MB, well within the 2 GB budget and mostly reclaimable cache.
- Apply the already-investigated allocator tuning (a `MALLOC_CONF` setting) to return freed pages sooner; it shrinks the retained heap after load at no measured throughput cost.
- Reduce the catalog memory-map size (`REPLAY_CATALOG_MMAP_MB`, default 64). Measured on the device: capping it at 16 MB cut process resident memory by ~19 MB with no change to warm page times — but it did **not** free system RAM. `MemAvailable` was unchanged because the catalog pages simply relocate from the process's mmap into the kernel page cache. So this lowers the *reported* process RSS for monitoring, not actual memory pressure. (A value of `0` is ignored — the env reader treats it as unset — so use a small positive value to reduce, not disable.)

### Heavier worst-case search on very large libraries

**What it is.** Common multi-word searches are on par with 0.4.0, but the pathological single-letter query ("a") is about 1.7× slower on the larger library (c=1 800 ms → 1,357 ms). The same query on the smaller library is far cheaper (304 ms), and longer queries (e.g. "street fighter") actually improved at c=1.

**This is about library size, not storage.** The catalog, library, and metadata databases live on the Pi's local disk for every storage mode; only the user database (favorites, recents, custom box art) lives on the ROM storage. Search reads just the local library database — not the user database — so it never waits on NFS. Custom box art is a good example: overrides are kept in the user database but are folded into the local library rows during enrichment, so they are not looked up per search result. The USB-vs-NFS labels here are therefore just "small library" vs "large library"; the protocol is irrelevant to search, and the cost tracks the number of matched rows. (User-data-backed features such as favorites, recents, and saving resources are the ones that do feel storage latency, since they read the user database on the ROM share.)

**Cause.** Text search matches a substring anywhere in each game's searchable text, then loads *every* match into memory and ranks it before paginating — there is no early cap. Measured on the device, the database scan itself is cheap (~46 ms even for a query that matches 99,943 of 103,896 rows); the expense is **materializing and ranking ~100k full rows in Rust**, amplified by how much metadata each result now carries (which has grown since 0.4.0). The ~4× small-vs-large gap (26,777 vs 103,896 games) confirms row count is the dominant factor, not an algorithmic regression.

**Options.**
- Require a minimum query length — ignore free-text queries shorter than 2 characters after trimming whitespace. This removes the single-character worst case outright and matches common search-box behavior. Keep the threshold at **two, not three**: two-character board shorthands like "f3" (Taito F3) and "cps" must keep working. Those already resolve through the separate arcade-board recognizer (a board-tag lookup, independent of free-text search), so they return results regardless of this guard or the text index. Cheapest guard, but it only eliminates 1-character queries; 2+ character broad free-text queries still scan and materialize.
- Cap the candidate set with a generous in-database limit before ranking, so even a broad query never loads the entire library into memory. This also lowers the broad-search load-test memory peak (observed ~0.7-1.1 GB), which comes from the same unbounded materialization.
- Add a full-text index for the searchable text. A leading-wildcard substring match cannot use a normal index, so today every text search is a full scan. Measured on the device, an FTS5 index over the search column costs about 6-9 s to build at library-build time and ~20 MB on disk (trigram, which supports substring matching), and turns the per-query lookup from ~46-67 ms into well under 1 ms while returning only matching rows to rank. Trigram matches need ≥3 characters, so two-character free-text queries would keep a `LIKE` fallback (rare and cheap — few rows match), and board shorthands stay on the recognizer path. It pairs naturally with the minimum-length guard above. This is the deeper fix.

### The leptos 0.8 upgrade: bigger bundle, slightly heavier renders (2026-07-12)

Upgrading the web framework (leptos 0.7 → 0.8) was benchmarked as an A/B on one Pi 5 against the immediately-preceding release (1.1.3, still leptos 0.7), same USB (26,777) and NFS (93,649) libraries, authenticated HTTPS. Because the two builds share the same feature set and catalog, every difference is the framework.

**What moved:**
- **Download bundle: +37% compressed (944 → 1,296 KB brotli; +27.8% raw).** The one clear regression. It is paid once per cold load, not per request. See [Download Sizes](#download-sizes).
- **Server-side render time: up, and it scales with page size.** Small pages barely move (game detail ~−6% throughput; a small system list within noise); large list pages pay the most — the arcade list warm TTFB roughly doubled (5.2 → 12.3 ms). At c=1 the leptos-0.7 baseline and 1.0.0 agree, so this delta is the framework, not features.

**What did *not* move:**
- **Memory is neutral, even slightly better.** Settled idle was 97 MB resident / 36 MB heap on leptos 0.8 vs 108 / 45 MB on the leptos-0.7 build; post-load peak was comparable (~500 MB). The larger WASM lives in a file-backed mmap, not server heap.
- **CPU is unchanged** (idle ~0%, one-user browsing 0.90% vs 0.87%).
- **Search is untouched** — it is database-bound, so the framework version is irrelevant (mario c=1 ~42 ms, "a" c=1 ~1,284 ms on NFS, matching the leptos-0.7 build).

**Don't confuse this with feature drift.** Raw light-page throughput is far lower than 1.0.0 (game detail c=1 700 → 344 req/s, SNES list 563 → 290). Almost all of that happened *before* leptos 0.8: the leptos-0.7 v1.1.3 release already measures ~365 / ~352 req/s. The cause is the per-render work added by two months of features (now-playing detection, RetroAchievements, series, richer per-game metadata), not the framework. leptos 0.8 adds only the last ~−6 to −17% on top.

**Options.**
- Accept it — the bundle is a one-time cold-load cost that brotli already keeps near 1.3 MB, and per-request time stays in single-digit-to-low-double-digit milliseconds for normal pages.
- If cold-load size matters, revisit WASM size levers independent of the framework (feature-gating rarely-used client code, `wasm-opt` flag tuning). The catalog/library mmap and server memory are unaffected either way.
- The per-render feature cost (the larger driver of the throughput drop) is addressable separately — it is per-page work in the SSR path, not a framework limit.

## Test Methodology

- **CPU**: `tools/pi-cpu.sh` reads `/proc/<pid>/stat` and reports CPU relative to one core. `--browse` simulates one user clicking through home, system pages, search, game detail, and manuals every ~2 s.
- **Memory**: `tools/pi-memory.sh` reads `/proc/<PID>/status`: VmRSS (memory in use), VmHWM (peak since process start), and RssAnon (heap).
- **Page and asset benchmarks**: `tools/bench.sh`, with Lighthouse skipped for the recorded release runs.
- **Stress tests**: `tools/load-test.sh`, which uses Apache Bench (`ab`) with 50 requests per endpoint.
- **Storage switching**: the USB and NFS rounds were measured on the same Pi by switching the active storage and rebooting, then waiting for the rescan to settle (all systems reporting fresh stats) before measuring.
- All measurements were taken on a Pi 5 (4 cores, 2 GB), no game running, default allocator configuration, on the 0.9.0 release build at commit `ac518f9`.
- Raw results are stored in `tools/bench-results/`.
