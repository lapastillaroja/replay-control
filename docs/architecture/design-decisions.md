# Design Decisions

Replay Control is a single-user retro game management web UI that runs on Raspberry Pi. Every design decision is shaped by this target environment.

## System Constraints

RePlayOS is a custom libretro frontend for retro gaming on Raspberry Pi (3 and newer). The companion web app runs alongside the frontend and emulators on the same device, sharing CPU and RAM. This means the app must be as lightweight as possible — every megabyte it holds is a megabyte unavailable to the emulator. Tested on a Pi 5 with 2GB RAM, managing a 23K+ game library with a single user on the local network.

Storage is typically USB (exFAT) or NFS, both with significant limitations. exFAT doesn't support SQLite WAL mode and has slow directory reads (~100ms for `read_dir` on 2000 files). NFS adds network latency and lacks inotify for change detection. These constraints drive many of the design decisions below — from how box art is resolved to how SQLite connections are configured.

The filesystem is auto-detected at startup via `/proc/mounts` in `db_common::open_connection()` (`replay-control-core-server/src/metadata/db_common.rs`). WAL-capable filesystems (ext4, btrfs) get WAL + `synchronous=NORMAL`. Non-WAL filesystems (exFAT, NFS) get `nolock=1` + DELETE journal. No caller-supplied hints needed.

## Memory Budget

The app must minimize memory usage because RePlayOS, the libretro frontend, and emulators all share the Pi's RAM — often just 1-2GB total. Every design decision below is evaluated against this budget.

Measured on Pi 5 (2GB) with a 23K game library: idle RSS is ~44MB (binary + embedded data), normal use sits around ~80MB, and peak under heavy load (full metadata import) reaches ~120MB before settling to ~113MB steady-state. jemalloc was chosen specifically for its memory return behavior — glibc malloc retained ~296MB after the same workload (see decision #2 below).

## Performance Design Decisions

### 1. Box art resolution: DB source of truth

Box art URLs are stored in `game_library.box_art_url` during background enrichment. The request path reads from DB only — no filesystem access, no in-memory image index. See the "In-memory ImageIndex cache" entry in Rejected Alternatives for the previous approach and why it was replaced.

**Files**: `replay-control-app/src/api/cache/enrichment.rs`, `replay-control-core-server/src/metadata/enrichment.rs`

### 2. jemalloc allocator

```rust
// replay-control-app/src/main.rs:4-5
#[cfg(feature = "ssr")]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;
```

glibc malloc retained ~296MB RSS after a heavy metadata import; jemalloc returns to ~113MB. On a 1GB Pi, this difference determines whether the app can coexist with emulators.

**Dependency**: `tikv-jemallocator` in `replay-control-app/Cargo.toml`, gated behind `ssr` feature.

### 3. SQLite cache_size = 500

```rust
// replay-control-app/src/api/mod.rs:85
conn.execute_batch("PRAGMA cache_size = 500;")?;
```

Reduced from the SQLite default of 2000 pages (8MB at 4KB/page) to 500 pages (2MB). With the connection pools, this saves ~6MB per connection. Trade-off: queries that scan large result sets may need more disk I/O, but most queries are indexed lookups returning <100 rows.

The base `open_connection()` in `db_common.rs` sets `cache_size = -8000` (8MB), then the pool manager overrides to 500 per-connection. This means the warmup connection (used once) gets the larger cache, while the pooled connections stay lean.

**File**: `replay-control-app/src/api/mod.rs` (SqliteManager::create)

### 4. One read connection per pool

```rust
// replay-control-app/src/api/mod.rs:192
const READ_POOL_SIZE: usize = 1;
```

Load tests on USB storage with DELETE journal mode showed no benefit from concurrent readers. The single-user access pattern and fast queries (<50ms) don't benefit from reader concurrency. Each saved connection frees ~2MB of RSS (page cache + connection overhead).

Under WAL mode (ext4/btrfs), multiple readers would help, but the primary deployment target is USB/exFAT.

**File**: `replay-control-app/src/api/mod.rs`

### 5. Response cache (10s TTL)

```rust
// replay-control-app/src/api/cache/response.rs:7
const RESPONSE_TTL: Duration = Duration::from_secs(10);
```

Caches the fully assembled `RecommendationData` and `Vec<GameSection>` returned by the home page and favorites server functions. Designed for back-navigation: tapping into a game detail page and pressing back hits the cache instead of re-running all the DB queries and box art resolution.

Performance: ~19ms on cache hit vs ~136ms on cache miss for the home page.

Invalidated explicitly on any mutation (favorite add/remove, ROM delete, box art change, import, region preference change).

**File**: `replay-control-app/src/api/cache/response.rs`

### 6. Query cache (event-driven invalidation)

```rust
// replay-control-app/src/api/cache/query.rs
pub(crate) struct QueryCache {
    top_genres: RwLock<Option<Vec<String>>>,
    top_developers: RwLock<Option<Vec<String>>>,
    decades: RwLock<Option<Vec<u16>>>,
    active_systems: RwLock<Option<Vec<String>>>,
}
```

Pill data (genres, developers, decades, active systems) for the home page recommendations changes only when the game library changes. No TTL -- invalidated explicitly via `invalidate_all()` when library changes occur (import, rebuild, ROM add/delete, region preference change).

Saves ~50ms per home page load by skipping four aggregate queries.

**File**: `replay-control-app/src/api/cache/query.rs`

### 7. Streaming SSR with skeleton loaders

The home page uses `Resource::new` for slow data (recents, recommendations) and `Resource::new_blocking` for fast data (info, systems). `Resource::new` defers resolution, so the page shell streams immediately with skeleton placeholders, then content fills in as data arrives.

```rust
// replay-control-app/src/pages/home.rs
let info = Resource::new_blocking(|| (), |_| server_fns::get_info());    // blocks SSR
let recents = Resource::new(|| (), |_| server_fns::get_recents());        // streams later
let recommendations = Resource::new(|| (), |_| server_fns::get_recommendations(6)); // streams later
```

Each streamed section uses `<Suspense fallback=|| view! { <Skeleton /> }>` with a dedicated skeleton component. The user sees the page layout immediately; data fills in progressively.

**File**: `replay-control-app/src/pages/home.rs`

### 8. Suspense > ErrorBoundary nesting order

With Leptos 0.7 streaming SSR, `<Suspense>` must wrap `<ErrorBoundary>`, not the reverse. The correct nesting:

```rust
<Suspense fallback=move || view! { <Skeleton /> }>
    <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }>
        {move || Suspend::new(async move { ... })}
    </ErrorBoundary>
</Suspense>
```

Reversing the order (ErrorBoundary outside Suspense) breaks hydration in streaming SSR mode. This pattern is used consistently across all pages.

**Files**: `replay-control-app/src/pages/home.rs`, `replay-control-app/src/pages/favorites.rs`, `replay-control-app/src/pages/game_detail.rs`

### 9. WriteGate for exFAT

```rust
// replay-control-app/src/api/mod.rs
pub(crate) struct WriteGate(Arc<AtomicBool>);
```

An RAII guard that blocks all DB reads during heavy write operations (metadata import, thumbnail index rebuild, game library rebuild). While the WriteGate is held, `DbPool::read()` returns `None`, which the UI handles gracefully (skeleton/empty states).

This prevents SQLite corruption on exFAT with DELETE journal mode, where concurrent reads and writes through the same `nolock=1` connection can cause "database disk image is malformed" errors.

Activated in `import.rs` and `background.rs` before batch writes, dropped after each write phase.

**Files**: `replay-control-app/src/api/mod.rs`, `replay-control-app/src/api/import.rs`, `replay-control-app/src/api/background.rs`

### 10. Embedded game databases

No-Intro DATs (11 systems), TheGamesDB metadata, arcade databases (FBNeo, MAME 2003+, MAME 0.285, Flycast), Wikidata series data, and genre/category INI files are compiled into the binary at build time via `build.rs` using PHF (perfect hash function) maps.

```rust
// replay-control-core-server/src/game/arcade_db.rs
include!(concat!(env!("OUT_DIR"), "/arcade_db.rs"));

// replay-control-core-server/src/game/game_db.rs
include!(concat!(env!("OUT_DIR"), "/game_db.rs"));

// replay-control-core-server/src/game/series_db.rs
include!(concat!(env!("OUT_DIR"), "/series_db.rs"));
```

This adds ~34MB to the binary but avoids runtime file I/O for metadata lookups. PHF maps provide O(1) lookups from ROM filename stem or CRC32 hash to canonical game data (title, year, genre, developer, players).

**Files**: `replay-control-core/build.rs`, `replay-control-core-server/src/game/arcade_db.rs`, `replay-control-core-server/src/game/game_db.rs`, `replay-control-core-server/src/game/series_db.rs`

### 11. Enrichment as background pipeline

The background startup pipeline in `BackgroundManager` runs sequentially:
1. Auto-import LaunchBox XML (if present and DB is empty)
2. Cache verification: scan all systems, write to `game_library` table, enrich box art + ratings
3. Auto-rebuild thumbnail index (if data sources exist but index is empty)

After this, the DB is the source of truth. All request-time data comes from SQLite queries -- no filesystem access needed to serve pages.

```rust
// replay-control-app/src/api/background.rs
pub struct BackgroundManager;
// Pipeline phases: auto-import -> cache verification -> thumbnail rebuild
```

ROM directory watchers (inotify for local, 30-minute TTL for NFS) detect external changes and trigger re-scans for affected systems.

**File**: `replay-control-app/src/api/background.rs`

### 12. Single binary

The entire app compiles to one binary: Rust server (axum + Leptos SSR) + WASM hydration blob. No Node.js runtime, no separate build tools at deployment time.

Cross-compiled for `aarch64-unknown-linux-gnu` via `./build.sh aarch64`. Static assets (CSS, JS, icons) are either embedded via `include_str!` or served from `target/site/`.

**Files**: `build.sh`, `replay-control-app/Cargo.toml` (`cdylib` + `rlib` crate types)

### 13. No cargo-leptos

Custom `build.sh` handles the two-step compilation:
1. WASM hydrate: `cargo build --target wasm32-unknown-unknown --profile wasm-release --features hydrate`
2. wasm-bindgen + wasm-opt (`-Oz`)
3. Server SSR: `cargo build --release --features ssr`

This gives direct control over compilation profiles (separate optimization levels for WASM size vs server speed), wasm-opt flags, and cross-compilation without depending on cargo-leptos's assumptions.

Four Cargo profiles serve different goals:
- `dev` (SSR): opt-level 1, strip debuginfo
- `wasm-dev`: opt-level "s" (even dev WASM must be small enough to load)
- `release` (SSR): opt-level 3, thin LTO, strip symbols
- `wasm-release`: opt-level "z", fat LTO (best size reduction)

**Files**: `build.sh`, `Cargo.toml` (workspace profiles)

### 14. include_str! for static assets

CSS (compiled from partials at build time), service worker, manifest.json, and pull-to-refresh JS are embedded in the binary via `include_str!`:

```rust
// replay-control-app/src/main.rs
include_str!("../static/manifest.json")
include_str!("../static/sw.js")
include_str!("../static/ptr-init.js")
include_str!("../static/pulltorefresh.min.js")

// replay-control-app/src/api/mod.rs
include_str!(concat!(env!("OUT_DIR"), "/style.css"))
```

No disk reads for these assets at runtime. CSS partials (`style/_*.css`) are concatenated at build time by `replay-control-app/build.rs`.

WASM bundle and icons are served from disk (`target/site/pkg/`, `target/site/icons/`) via `tower_http::ServeDir` since they are larger binary files where embedding would bloat startup memory.

**Files**: `replay-control-app/src/main.rs`, `replay-control-app/src/api/mod.rs`, `replay-control-app/build.rs`

### Crate split: `replay-control-core` vs `replay-control-core-server`

Historically all shared library code lived in a single `replay-control-core` crate consumed by `replay-control-app`. Because `replay-control-app` is a Leptos full-stack crate that builds for both native (SSR) and `wasm32-unknown-unknown` (hydrate), `replay-control-core` had to compile for both targets. But it transitively pulled `rusqlite`, `deadpool-sqlite`, `tokio`, and `reqwest` — none of which link on wasm. The workaround was 89 `#[cfg(target_arch = "wasm32")]` attributes across 12 files, stubbing every DB/fs/HTTP function to return `None`/`HashMap::new()`/`vec![]` on wasm. A mirror layer in `replay-control-app/src/types.rs` duplicated ~17 serde wire types so hydrate-side code could name them without crossing the cfg boundary.

The split replaces that workaround with a crate-level firewall:

- **`replay-control-core`**: pure types, wire contracts, pure domain logic. Compiles for both targets. No `rusqlite`, `tokio`, `reqwest`, `std::fs`, `std::process`, `deadpool`, or `quick-xml`.
- **`replay-control-core-server`**: everything that touches those deps. Compiles for native only. Re-exports core's pure types at matching module paths so SSR callers find both type and native fn under `replay_control_core_server::<module>::`.
- **`replay-control-app`**: depends on `replay-control-core` unconditionally, on `replay-control-core-server` only under `feature = "ssr"`.

**Why not `#[cfg(feature = "server")]` on core instead?** It would rename the gates, not remove them. The goal was to stop branching in core, not relabel it. Two crates removes the cfgs by construction.

**Why not fold the native code into `replay-control-app`?** `metadata_report` (a CLI reporting bin) and `tools/build-catalog` consume the same logic. Moving it into the Leptos crate would force those consumers to either depend on `app` (wrong layering) or duplicate code.

**Orphan-rule note**: `DatePrecision` is in core but serialized to SQLite in core-server. A `DpSql` newtype scoped to `metadata_db` carries the `rusqlite::ToSql` / `FromSql` impls, sidestepping the orphan rule. Future foreign-trait-on-core-type impls should use the same pattern.

## What We Considered But Rejected

| Alternative | Why rejected |
|---|---|
| **mimalloc** | Tested; 155MB peak vs jemalloc's 120MB on the same workload |
| **Multiple DB reader connections** | No measurable benefit on USB/DELETE mode with single-user access |
| **FTS5 for search** | Larger schema change; current `search_text LIKE '%word%'` with indexed columns is fast enough (~51ms cross-system) |
| **Keep-alive routes in Leptos** | Not supported in Leptos 0.7; response cache (10s TTL) solves the main use case (back-navigation) |
| **L1 ROM cache** | Removed after search unification -- all game list queries go through the DB |
| **In-memory ImageIndex cache with filesystem change detection** | A per-system `ImageIndex` was cached in memory with mtime-based freshness detection. Every request that needed box art called `is_fresh()`, which did `read_dir` on the boxart folder — ~100ms per system on USB. With ~10 systems in recents, the home page cost 931ms cold, 248ms warm just for box art. The `ImageIndex` also consumed ~6-10MB of memory across all systems. Replaced with DB `box_art_url` field populated during enrichment — zero filesystem access at request time. Savings: ~360ms warm per request, ~10MB memory. |
| **mmap_size for SQLite** | Causes stale reads when heavy writes happen on a separate connection (thumbnail index rebuild writes 46K rows, read connections see corrupted mmap'd pages) |
