# Investigation: Parallel WASM + SSR Builds

**Date:** 2026-03-12
**Status:** Feasible with caveats -- not recommended

## Summary

The WASM (hydrate) and SSR (server) builds in `build.sh` can be parallelized using
separate target directories. The two builds have almost no dependency overlap and
target completely different architectures. However, the savings are more modest than
they appear due to Cargo's global locking and shared CPU resources.

## Current Build Pipeline

`build.sh` runs these steps sequentially:

1. `cargo build -p replay-control-app --lib --target wasm32-unknown-unknown --profile wasm-release --features hydrate`
2. `wasm-bindgen` (generates JS glue + processed .wasm)
3. `wasm-opt -Oz` (shrinks .wasm)
4. `gzip -9` (pre-compresses .wasm for static serving)
5. Copy CSS + static assets
6. `cargo build -p replay-control-app --bin replay-control-app --release --features ssr`

## Measured Build Times (incremental, single source file change)

| Step | Time | Notes |
|------|------|-------|
| WASM cargo build (wasm-release) | 96s | LTO + codegen-units=1, CPU-bound |
| wasm-bindgen | 0.4s | Fast |
| wasm-opt -Oz | 4.5s | CPU-bound (multi-threaded, 8.5x parallelism) |
| gzip + asset copy | <1s | Negligible |
| SSR cargo build (release) | 6s | Only app crate, incremental=true |
| **Full build.sh (sequential)** | **105s** | Measured end-to-end |

## Key Findings

### 1. No Shared Build Artifacts Between WASM and SSR

The WASM build (hydrate feature) does **not** compile `replay-control-core` at all.
The core crate with its expensive build.rs (PHF game databases, arcade data) is only
pulled in by the `ssr` feature via `dep:replay-control-core`. This means:

- **WASM build:** `replay-control-app` lib crate only, with `hydrate` feature
  (leptos/hydrate, web-sys, gloo-timers)
- **SSR build:** `replay-control-app` bin + `replay-control-core` with `ssr` feature
  (leptos/ssr, axum, tokio, rusqlite, etc.)
- No build.rs contention -- `replay-control-core/build.rs` (which parses XML/JSON/CSV
  data files and generates PHF maps) only runs for SSR, never for WASM

The dependency trees barely overlap. The WASM build pulls in browser-side crates
(web-sys, wasm-bindgen, gloo-*), while the SSR build pulls in server-side crates
(axum, tokio, tower, rusqlite, reqwest). The only shared dependencies are leptos
core, serde, and server_fn -- but these compile to different targets anyway.

### 2. Cargo's Target Directory Lock Prevents Same-Dir Parallelism

Cargo acquires an exclusive file lock on the target directory (via `.package-cache`
and the build directory lock). When two cargo processes share the same target
directory, the second blocks until the first finishes:

```
Blocking waiting for file lock on package cache
Blocking waiting for file lock on artifact directory
```

**Verified experimentally:** Running both builds in parallel against the default
`target/` directory produced identical wall-clock time to sequential builds. The
second cargo process waited for the first to release the lock, then ran alone.

- Sequential (touch lib.rs, WASM then SSR): 28.9s wall (debug profile)
- Parallel same target dir: 32.8s wall -- actually slower due to lock overhead

### 3. Separate Target Dirs Enable True Parallelism (with Caveats)

Using `CARGO_TARGET_DIR=target-wasm` for one build and the default `target/` for
the other allows both cargo processes to compile simultaneously. Each target
directory has its own lock.

**However**, both processes still contend for:
- `~/.cargo/.package-cache` -- the global registry lock (held briefly during
  dependency resolution, then released)
- CPU cores -- both builds compete for the same physical cores

**Measured (debug profile, warm caches):**
- Sequential: 28.9s wall
- Parallel with separate target dirs: 27.3s wall (both builds overlapped)
- Speedup: ~6% -- modest because SSR build is short relative to WASM

### 4. Separate Target Dir = Cold-Compile Penalty + Disk Cost

Using a separate `target-wasm/` directory means all WASM dependencies must be
compiled from scratch on first run. Measured first-run cost: **2m07s** (debug) for
the WASM build alone. After warming, incremental builds are normal.

**Disk usage measured:**
- `target/` (main): 65GB (contains debug, release, wasm-release, aarch64)
- `target-wasm/` (separate): 3.6GB (wasm32 artifacts + host-compiled proc macros)
  - `wasm32-unknown-unknown/`: 3.0GB
  - `debug/` (proc macros, build scripts): 624MB

The separate dir duplicates proc-macro and build-script artifacts that are always
compiled for the host, adding ~600MB per additional target directory.

### 5. CPU Contention Limits Parallel Gains

Both builds are CPU-intensive:

- **WASM `wasm-release`:** `lto = true, codegen-units = 1` -- the final LTO link
  phase is single-threaded and takes the bulk of the 96s build time. Earlier
  compilation phases use multiple cores.
- **SSR `release`:** `incremental = true` -- uses multiple cores during
  compilation, but the total work is much less (~6s for app-only changes).

On this machine, running both simultaneously means the SSR build's multi-core
compilation phase competes with WASM's multi-core compilation phase for the
same cores, slightly slowing both. During the WASM LTO phase (single-threaded),
the SSR build can use remaining cores freely.

### 6. The WASM Build Is the Overwhelming Bottleneck

The time breakdown makes the asymmetry clear:

- WASM build: 96s (91% of total)
- Post-processing (wasm-bindgen + wasm-opt + gzip): 5s (5% of total)
- SSR build: 6s (6% of total, for app-only changes)

Even with perfect parallelism (zero overhead), the total build time would drop
from 105s to 96s -- a saving of only 9 seconds (8.5%).

## Implementation Options Evaluated

### Option A: Same target dir, background jobs

```bash
cargo build ... --target wasm32-unknown-unknown ... &
cargo build ... --features ssr ... &
wait
```

**Verdict: Does not work.** Cargo serializes access to the same target directory.
The second process blocks on the artifact directory lock until the first finishes.
Measured: 32.8s vs 28.9s sequential -- actually slower due to lock acquisition
overhead.

### Option B: Separate target dirs

```bash
CARGO_TARGET_DIR=target-wasm cargo build ... --target wasm32-unknown-unknown --profile wasm-release --features hydrate ... &
PID_WASM=$!

cargo build ... --release --features ssr ... &
PID_SSR=$!

wait $PID_WASM
wasm-bindgen target-wasm/wasm32-unknown-unknown/wasm-release/...
wasm-opt ...

wait $PID_SSR
```

**Verdict: Works, but marginal benefit.** Measured parallel wall time was 27.3s vs
28.9s sequential (debug profile). For release builds, the savings would be ~6-9s
off a 105s total.

**Drawbacks:**
- `target-wasm/` must be kept warm or face a ~2min cold-compile penalty
- Adds ~3.6GB disk usage (grows over time with incremental artifacts)
- wasm-bindgen/wasm-opt paths must reference the separate target dir
- Build output becomes interleaved and harder to read
- Error handling is more complex (need to capture exit codes from both)
- `.gitignore` must be updated for the new target dir

### Option C: Overlap SSR with WASM post-processing

```bash
cargo build ... --target wasm32-unknown-unknown ...

# After WASM cargo build finishes, start SSR and post-processing in parallel
cargo build ... --features ssr ... &
PID_SSR=$!

wasm-bindgen ...   # ~0.4s, no cargo lock
wasm-opt ...       # ~4.5s, no cargo lock
gzip ...           # <1s
copy_assets

wait $PID_SSR
```

**Verdict: Works and is simple.** After the WASM `cargo build` finishes and
releases the target lock, the SSR `cargo build` can start immediately while
wasm-bindgen/wasm-opt run in the foreground. These external tools do not hold
any Cargo locks.

**Savings:** Up to ~5s overlap (wasm-bindgen + wasm-opt + gzip run concurrently
with the SSR cargo build). This is the simplest approach -- no separate target
dirs, no cold-compile penalty, no extra disk usage. But the savings are small.

### Option D: Do nothing

**Verdict: The pragmatic choice.** The WASM build dominates at 96s. The SSR
build adds only 6s. Total: 105s. Even perfect parallelism saves at most 6s.

## Recommendation

**Don't parallelize.** The complexity is not justified by the savings.

1. **WASM dominates:** At 96s, the WASM build is 91% of the total 105s build
   time. SSR adds only 6s for typical app-only changes. Even perfect parallelism
   saves at most 6-9 seconds.
2. **Option C is the only cheap win:** Overlapping SSR with WASM post-processing
   (Option C) saves ~5s with minimal complexity. But it's barely noticeable.
3. **Separate target dirs (Option B) are not worth it:** 3.6GB extra disk, a 2min
   cold-compile penalty on first run, and interleaved output -- all for ~6s.
4. **Build preferences:** "Never run builds in parallel" is the stated preference.

**Better optimization targets for build speed:**

- **LTO strategy:** The `wasm-release` profile uses `lto = true` (full LTO) with
  `codegen-units = 1`. Switching to `lto = "thin"` would significantly reduce the
  single-threaded LTO link phase at the cost of slightly larger WASM output. This
  could save 30-50s.
- **Profile tuning:** The SSR `release` profile has `incremental = true` which is
  already optimal for iterative development. Consider a `release-dev` profile
  (which already exists with `opt-level = 1`) for faster SSR iteration.
- **Tooling:** `cargo-leptos` handles WASM+SSR coordination natively and could
  potentially orchestrate parallel builds with proper lock management.

## Raw Measurements

**Environment:** Fedora 42, rustc 1.94.0, cargo 1.94.0, NVMe SSD

### Individual Step Timings (incremental, app source touched)

| Step | Wall time | CPU | Notes |
|------|-----------|-----|-------|
| WASM cargo build (wasm-release) | 96s | 92s user | LTO=true, codegen-units=1 |
| wasm-bindgen | 0.4s | 0.65s user | 4x parallelism |
| wasm-opt -Oz | 4.5s | 38s user | 8.5x parallelism |
| SSR cargo build (release) | 6.3s | 3.5s user | incremental=true |
| Full build.sh end-to-end | 105s | 134s user | Sequential |

### Parallelism Tests (debug profile, app source touched)

| Configuration | Wall time | Notes |
|---------------|-----------|-------|
| Sequential (WASM then SSR) | 28.9s | Baseline |
| Parallel, same target dir | 32.8s | Serialized by cargo lock -- slower |
| Parallel, separate target dirs (warm) | 27.3s | Both compiled concurrently |
| Parallel, separate target dirs (cold) | 2m38s | Second dir compiled from scratch |

### Disk Usage

| Directory | Size |
|-----------|------|
| `target/` (main, all profiles) | 65GB |
| `target/wasm32-unknown-unknown/` | 4.7GB |
| `target-wasm/` (separate, after one build) | 3.6GB |
| `target-wasm/debug/` (proc macros, build scripts) | 624MB |

### Build Profiles (from workspace Cargo.toml)

```toml
[profile.release]
incremental = true

[profile.wasm-release]
inherits = "release"
opt-level = "z"
lto = true
codegen-units = 1
incremental = false
```
