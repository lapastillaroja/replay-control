# Compile Time Analysis

Date: 2026-03-11
Rust: 1.94.0, Cargo 1.94.0
Platform: Linux x86_64, 16 cores
Workspace: 2 crates (replay-control-core, replay-control-app)
Total source: ~19K lines Rust + 141K lines generated code

## Current Measurements

| Build                                        | Time     | Notes                                       |
|----------------------------------------------|----------|---------------------------------------------|
| Full clean SSR release                       | 2m 38s   | 381 unique crates, hit compile error at end |
| Full clean WASM hydrate release              | 3m 06s   | 239 unique crates                           |
| Incremental SSR release (touch .rs)          | 1m 53s   | Build.rs re-ran; app crate fully recompiled |
| Incremental WASM release (touch .rs)         | 3m 57s   | Entire app crate recompiled from scratch    |
| Incremental SSR debug (touch .rs)            | 11s      | Incremental compilation enabled by default  |
| Build.rs execution alone                     | 13s      | Processes ~178MB of data, generates 19MB RS |
| Core crate release rebuild (build.rs + compile) | 1m 01s | 13s build.rs + ~48s compiling 141K lines   |
| Full `build.sh` pipeline (WASM + SSR)        | ~6m      | Serial: WASM first, then SSR               |

### Key observation

**Incremental release builds are nearly as slow as clean builds.** Rust disables
incremental compilation in `release` profile by default, so any change to the
single-crate app causes a full recompilation. The WASM incremental case (3m 57s)
is actually *slower* than a clean build (3m 06s) due to artifact lock contention
from the concurrent SSR build artifacts.

## Findings

### 1. Linker

**Current**: LLD 21.1.8 (bundled with rustc) -- already a fast linker.

**mold**: Available in Fedora repos (`mold 2.40.4`), not currently installed or
configured. `.cargo/config.toml` only has cross-compilation settings for
`aarch64`.

**Impact**: Low. LLD is already fast. Mold may save 1-3 seconds on the link step,
which is a small fraction of the total build time. The bottleneck is compilation,
not linking.

### 2. Build Script Cost

The `replay-control-core/build.rs` processes:
- `thegamesdb-latest.json` (138MB) -- TheGamesDB dump, 110K entries
- `fbneo-arcade.dat` (13MB) -- 8K arcade entries
- `mame2003plus.xml` (22MB) -- 5K entries
- `mame0285-arcade.xml` (3.7MB) -- 27K entries
- `catver.ini` + `catver-mame-current.ini` (2.5MB) -- 53K category mappings
- 9 No-Intro DAT files, genre files, maxusers files -- 34K ROM entries
- `flycast_games.csv` (25KB) -- 301 entries

**Execution time**: 13 seconds, 798MB peak RAM.

**Output**: Two generated files totaling 19MB / 141K lines:
- `arcade_db.rs` (9.1MB, 34K lines) -- PHF map of 28,593 arcade games
- `game_db.rs` (10MB, 107K lines) -- PHF maps + static arrays for 9 systems

**Caching**: Uses `cargo::rerun-if-changed` for specific files and directories.
When input data does not change, the build script correctly skips re-execution.
However, `rerun-if-changed` on directories (`no-intro/`, `libretro-meta/genre/`,
`libretro-meta/maxusers/`) means that adding, removing, or renaming any file in
those directories triggers a full re-run.

**The real cost is not the script itself but compiling the output.** The 141K
lines of generated PHF map code takes ~48 seconds to compile in release mode.
This runs on every core crate rebuild, which happens whenever the build.rs
re-runs.

### 3. Codegen Units

**Current**: Default settings (no `[profile]` sections in any `Cargo.toml`).
- Release: `codegen-units = 16`, `opt-level = 3`, `lto = false`, `incremental = false`
- Dev: `codegen-units = 256`, `opt-level = 0`, `incremental = true`

**Key problem**: `incremental = false` in release means every change to the app
crate triggers a full recompilation of that crate. Since the app crate is a
single monolithic crate with all pages, components, and server functions, this
is extremely expensive.

Setting `codegen-units = 256` and `incremental = true` for release would
significantly speed up incremental release builds at the cost of slightly
less optimized output.

### 4. Macro Expansion

Top files by `view!` macro usage:
- `metadata.rs`: 30 invocations (787 lines)
- `favorites.rs`: 29 invocations (696 lines)
- `home.rs`: 21 invocations (187 lines)
- `game_detail.rs`: 18 invocations (596 lines)
- `rom_list.rs`: 16 invocations (646 lines)
- `search.rs`: 15 invocations (671 lines)

Total: 198 `view!` invocations across 23 files.

**Impact**: Moderate. The Leptos `view!` macro generates significant code per
invocation, but 198 total invocations across 11K lines is not extreme.
Splitting the largest files would help if they were in separate crates (see
workspace structure below), but within a single crate it only affects
incremental debug builds.

### 5. Feature Gating

Feature gating is **well structured**. The app crate cleanly separates:
- `ssr` feature: pulls in axum, tokio, replay-control-core, etc.
- `hydrate` feature: pulls in web-sys, gloo-timers

These are mutually exclusive and correctly gated with `--no-default-features`.
The core crate is only compiled for SSR, not for WASM. No significant waste.

The `metadata` feature on core is always enabled when pulled in by the app
(hardcoded in `replay-control-app/Cargo.toml`). This adds `rusqlite` (with
bundled SQLite C compilation) and `quick-xml` to every SSR build. If metadata
were optional in practice, gating it would save the SQLite compilation.

### 6. sccache / Build Caching

**Not installed.** Neither `sccache` nor any other build cache is configured.

**Impact**: High for clean builds (CI, new machines). sccache can cache compiled
crate artifacts and reuse them across clean builds. It would reduce clean build
times from ~3 minutes to seconds for cached deps. It would not help incremental
builds where only the app crate changes.

### 7. Workspace Structure

Current: 2 crates.
- `replay-control-core`: Data layer, build script, ~7.6K lines + 141K generated
- `replay-control-app`: Leptos app, UI, server functions, ~11K lines

**Problem**: The app crate is a monolith. Any change to any page, component, or
server function recompiles the entire app crate. In release mode (no incremental),
this means recompiling all 11K lines + all macro expansions.

**Opportunity**: Splitting the app crate would improve incremental release builds:
- `replay-control-ui`: Leptos components and pages (the part that changes most)
- `replay-control-server`: Server binary, axum setup, server functions

However, Leptos's architecture makes this split non-trivial because `view!` macros
generate different code for SSR vs hydrate, and the `#[server]` macro ties server
functions to the same crate as the components that call them.

A more practical split:
- Extract **server functions** into their own crate (they're pure Rust + core calls)
- Keep the UI components in the app crate

This way, changing a server function doesn't recompile all UI code and vice versa.

### 8. Profile Settings

**No custom profiles defined.** The workspace uses pure Rust defaults.

Missing optimizations:
- No `strip = true` for release (binary is 71MB unstripped)
- No LTO (could reduce binary size but increases build time)
- No `opt-level` tuning for dependencies vs own code
- No `codegen-units` tuning

### 9. Dependency Tree

**SSR build**: 381 unique crates
**WASM build**: 239 unique crates
**Shared**: 235 crates compiled twice (different targets, cannot share)

Heavy dependency chains:
- **reqwest** (HTTP client): Used in exactly 1 place (`videos.rs` line 136).
  Pulls in ring (crypto with C compilation), rustls, hyper-rustls, and ~17
  TLS-related crates. Consider replacing with a lighter HTTP client or
  using `hyper` directly (already in the dep tree via axum).
- **ring** (crypto): ~11 reverse dependants, all through reqwest's rustls-tls.
  Has a C compilation step.
- **rusqlite** (SQLite): Bundled SQLite means compiling ~230K lines of C.
  Only used for metadata feature. Already optional on the core crate but
  always enabled from the app.
- **notify** (file watcher): Used for config file watching. Pulls in
  inotify + crossbeam ecosystem. Not heavy but adds compile units.
- **clap** (CLI parser): Used for server CLI arguments. Moderate compile
  cost with `derive` feature.

### 10. Parallel Frontend/Backend Compilation

`build.sh` builds WASM first, then SSR, sequentially. They **cannot share
compiled artifacts** because they target different architectures
(wasm32-unknown-unknown vs native x86_64). The Cargo target directories
are separate.

However, they could be run **in parallel** since they don't conflict:
- WASM writes to `target/wasm32-unknown-unknown/`
- SSR writes to `target/release/`
- Cargo handles concurrent builds to the same target dir with locking

Running both in parallel could reduce total `build.sh` time from ~6 minutes
to ~4 minutes (limited by the slower WASM build).

**Caveat**: Running both in parallel will increase peak memory usage and
CPU contention. On a 16-core machine this should be manageable.

## Prioritized Recommendations

### Quick Wins (< 30 minutes, high impact)

#### 1. Enable incremental compilation for release builds
**Estimated impact**: Reduce incremental release rebuilds from ~2m to ~20-30s.
**Effort**: 5 minutes.

Add to workspace `Cargo.toml`:
```toml
[profile.release]
incremental = true
```

Trade-off: Slightly larger/slower binary, larger target directory. For
development iteration this is the single biggest win. Use a separate profile
for final production builds if needed.

#### 2. Parallel WASM + SSR in build.sh
**Estimated impact**: Reduce full build.sh from ~6m to ~4m.
**Effort**: 15 minutes.

Run the WASM and SSR cargo builds in parallel using background processes.
The wasm-bindgen step must wait for WASM to finish, but can overlap with SSR
compilation.

```bash
cargo build -p "$CRATE" --lib --target wasm32-unknown-unknown --release \
  --features hydrate --no-default-features &
WASM_PID=$!

cargo build -p "$CRATE" --bin "$CRATE" --release \
  --features ssr --no-default-features &
SSR_PID=$!

wait $WASM_PID
# Run wasm-bindgen after WASM finishes
wasm-bindgen ...

wait $SSR_PID
```

#### 3. Strip release binaries
**Estimated impact**: Reduce binary from 71MB to ~15-20MB. Marginal link time savings.
**Effort**: 2 minutes.

```toml
[profile.release]
strip = true
```

#### 4. Add a `[profile.release-fast]` for development
**Estimated impact**: Faster iteration builds with reasonable optimization.
**Effort**: 5 minutes.

```toml
[profile.release-dev]
inherits = "release"
incremental = true
codegen-units = 256
opt-level = 1
```

Use with `cargo build --profile release-dev` for development. Keep the default
`release` profile for production builds.

### Medium Effort (1-4 hours, moderate impact)

#### 5. Install and configure mold linker
**Estimated impact**: Save 1-3s per link. Marginal for this project.
**Effort**: 30 minutes (install, configure, test).

```bash
sudo dnf install mold
```

Add to `.cargo/config.toml`:
```toml
[target.x86_64-unknown-linux-gnu]
linker = "clang"
rustflags = ["-C", "link-arg=-fuse-ld=mold"]
```

Low priority since LLD is already fast and linking is not the bottleneck.

#### 6. Install sccache
**Estimated impact**: Reduce clean builds from ~3m to ~30s (after first cache).
**Effort**: 1 hour (install, configure, warm cache).

Helps CI and fresh checkouts. Does not help incremental local development.

```bash
cargo install sccache
export RUSTC_WRAPPER=sccache
```

#### 7. Reduce generated code size
**Estimated impact**: Reduce core crate release compilation from ~48s to ~15-20s.
**Effort**: 2-4 hours.

The 141K lines of generated PHF code is the biggest compilation bottleneck.
Options:
- **Compress the PHF maps**: Instead of generating inline struct literals,
  generate binary data (e.g., `include_bytes!`) with a thin runtime decoder.
  This turns 141K lines of Rust into a binary blob that compiles instantly.
- **Reduce entries**: The arcade DB has 28,593 entries, many of which are
  MAME-current-only and not practically useful. Filtering to only the ~8K
  games that FBNeo/MAME2003+ actually emulate would reduce the map by ~70%.
- **Switch from PHF to a binary search**: A sorted `&[(&str, Info)]` with
  binary search compiles much faster than a PHF map and has acceptable
  runtime performance for this use case.

#### 8. Use `reqwest` more efficiently or replace it
**Estimated impact**: Remove ~17 TLS crates (ring C compilation, rustls, etc.).
**Effort**: 1-2 hours.

`reqwest` is used in exactly 1 place for fetching YouTube video metadata.
Options:
- Use `hyper` directly (already in the dep tree via axum)
- Use `ureq` (much lighter, no async runtime dependency)
- Shell out to `curl` for this single use case

### Larger Changes (1+ days, high impact)

#### 9. Split app crate into UI + server crates
**Estimated impact**: Incremental release rebuilds limited to the changed crate only.
**Effort**: 1-2 days.

Split `replay-control-app` into:
- `replay-control-server-fns`: Server functions (the bridge between UI and core)
- `replay-control-app`: UI components, pages (depends on server-fns)
- Binary crate: Just the main.rs + axum setup

This is the architecturally cleanest approach but requires careful handling of
Leptos's server function registration.

#### 10. Pre-compile generated data as a binary format
**Estimated impact**: Eliminate the 13s build.rs + 48s compilation entirely.
**Effort**: 1-2 days.

Instead of generating Rust source code at build time:
1. Run the data processing as a standalone tool (not build.rs)
2. Output a binary format (e.g., `bincode`, `rkyv`, or custom)
3. Use `include_bytes!` in the library code with a thin runtime parser
4. Only re-run the tool when data files actually change (Makefile/script)

This eliminates both the build script execution and the compilation of
generated code, saving ~1 minute on every core crate rebuild.

## Summary Table

| # | Recommendation                        | Impact         | Effort    | Type       |
|---|---------------------------------------|----------------|-----------|------------|
| 1 | Incremental release builds            | Very High      | 5 min     | Quick win  |
| 2 | Parallel WASM + SSR in build.sh       | High           | 15 min    | Quick win  |
| 3 | Strip release binaries                | Low (size)     | 2 min     | Quick win  |
| 4 | Release-dev profile                   | High           | 5 min     | Quick win  |
| 5 | Install mold linker                   | Low            | 30 min    | Medium     |
| 6 | Install sccache                       | High (CI)      | 1 hour    | Medium     |
| 7 | Reduce generated code size            | High           | 2-4 hours | Medium     |
| 8 | Replace reqwest with lighter client   | Moderate       | 1-2 hours | Medium     |
| 9 | Split app crate                       | High           | 1-2 days  | Large      |
| 10| Pre-compile data to binary format     | Very High      | 1-2 days  | Large      |

The biggest single win is **recommendation #1** (enabling incremental release builds).
It takes 5 minutes to implement and should reduce iterative release rebuilds from
~2 minutes to ~20-30 seconds. Combined with **#4** (release-dev profile), this
would make the development cycle much faster without affecting production build
quality.

For the full `build.sh` pipeline, **#2** (parallel builds) is the easiest win,
and **#10** (binary data format) is the highest-impact longer-term investment.
