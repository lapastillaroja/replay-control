# Technical Foundation

The core technical stack and infrastructure that powers Replay Control. For performance-specific design decisions, see [Design Decisions](design-decisions.md).

## Crates

The codebase is split into four crates inside a Cargo workspace.

### replay-control-core (pure library)

Pure Rust library compiled for both native (SSR) and `wasm32-unknown-unknown` (hydrate). Zero I/O dependencies — no `rusqlite`, `tokio`, `reqwest`, `std::fs`, `std::process`, `deadpool`, or `quick-xml`. Contains:

- **Pure domain types**: ROM filename parsing (`rom_tags`), title normalization (`title_utils`), developer/genre canonicalization, search scoring, semver-based update detection, locale
- **Pure reference data**: systems catalog (`platform::systems`), skin palettes, `DatePrecision` enum
- **Error types**: shared error enum (`error::Error`)

This crate is the default home for new code. If code touches SQLite, fs, HTTP, or a process, it belongs in `replay-control-core-server` instead.

### replay-control-core-server (native library)

Server-only native implementation. Compiled for native targets only (never wasm). Pulls `rusqlite`, `deadpool-sqlite`, `tokio`, `reqwest` (optional), `quick-xml` (optional). Contains:

- **DbPool**: generic `deadpool-sqlite` wrapper with read/write pools, journal mode detection, `WriteGate` RAII guard for exFAT safety, and corruption auto-close. App constructs `library_pool` and `user_data_pool` instances on top.
- **Catalog pool**: async read-only `deadpool-sqlite` pool for the bundled `catalog.sqlite` (game databases, arcade DB, series DB)
- **Game DB queries**: native SQL lookups for arcade/console metadata, display name resolution, release dates
- **Library scanning + uploads**: ROM discovery, favorites/recents I/O, hashing, disc-group detection, `roms::write_rom` for upload writes
- **Metadata pipeline**: LaunchBox XML parsing (`launchbox::run_bulk_import` bridges sync XML parsing to the async pool, `import_launchbox_aliases` writes the alias table), thumbnail manifest download, image resolution, library DB writes, `thumbnails::update_image_paths_from_disk`
- **Update I/O**: GitHub release polling (`update::check_github_update`), asset download (`update::download_asset`), and `available.json` filesystem helpers — gated on the `http` feature
- **Platform adapters**: `/proc/mounts` filesystem detection, `df` disk usage, storage location detection
- **HTTP client**: `reqwest`-backed helpers (feature-gated)
- **Settings store**: `replay.cfg` / `settings.cfg` reader+writer
- **Launch**: spawns `replay` process for game playback
- **Test utilities**: `test_utils` module with shared fixtures (`build_library_pool`, `insert_game_library_row`) used by both inline `#[cfg(test)]` modules and any `tests/*.rs` integration tests

Re-exports `replay-control-core`'s pure types at each matching module level (e.g. `replay_control_core_server::arcade_db::ArcadeGameInfo` resolves via `pub use replay_control_core::arcade_db::*`), so SSR callers have a single import path for both type and native fn.

Feature-gated: `metadata` enables `quick-xml`; `http` enables `reqwest`. The `metadata_report` bin requires `metadata`.

### replay-control-app (web application)

Leptos 0.7 SSR + WASM hydration app built on Axum. Depends on `replay-control-core` unconditionally (both SSR and hydrate builds) and on `replay-control-core-server` only when the `ssr` feature is active. Contains:

- **Server functions**: ~70 registered server functions for all UI data needs
- **API layer** (`src/api/`): `AppState` (owns the two `DbPool` instances + activity broadcast), background pipeline + filesystem watchers, activity system, L1 game library cache (`api/library/`), thin Axum handlers (upload, recents, favorites, etc.). Pure I/O — pool wrappers, update HTTP, ROM writes, pipeline cores — lives in core-server.
- **Pages** (`src/pages/`): home, system browser, game detail, favorites, settings, metadata management, search
- **Components** (`src/components/`): reusable UI components (hero cards, game rows, skeleton loaders, modals)
- **Internationalization**: runtime i18n with locale-keyed translation strings
- **App-only types** (`src/types.rs`): `Activity`, phase/progress types for the SSE stream. Wire types that cross server-function boundaries are imported directly from `replay-control-core` — no mirror layer.

### replay-control-libretro (TV display core)

Standalone cdylib (not in the workspace) that implements the libretro API. Runs as a RetroArch core on the TV, fetching game detail data from the companion app's HTTP API via `minreq`. Renders box art using the `png` crate. Lightweight by design -- no web framework, no SQLite.

## Key File Paths

| Concern | Path |
|---------|------|
| App entry point | `replay-control-app/src/main.rs` |
| AppState (owns pool instances) | `replay-control-app/src/api/mod.rs` |
| DbPool / SqliteManager / WriteGate | `replay-control-core-server/src/db_pool.rs` |
| Background pipeline + watchers | `replay-control-app/src/api/background.rs` |
| Update polling + asset download (HTTP/fs) | `replay-control-core-server/src/update.rs` |
| Activity system | `replay-control-app/src/api/activity.rs` |
| App-side enrichment orchestration | `replay-control-app/src/api/library/enrichment.rs` |
| Image resolution | `replay-control-core-server/src/library/thumbnails/resolution.rs` |
| DB schema | `replay-control-core-server/src/library/db/mod.rs` |
| User data DB | `replay-control-core-server/src/user_data/db.rs` |
| Catalog pool | `replay-control-core-server/src/catalog_pool.rs` |
| ROM tag parsing | `replay-control-core/src/game/rom_tags.rs` |
| Image matching | `replay-control-core-server/src/library/thumbnails/matching.rs` |
| HTTP client | `replay-control-core-server/src/http.rs` |
| Shared test fixtures | `replay-control-core-server/src/test_utils.rs` |

## Stack

**Leptos 0.7 SSR with WASM hydration** — the server renders full HTML pages on the Pi, then the browser hydrates with a lightweight WASM bundle for client-side interactivity. Four compilation profiles handle the dual-target build:

| Environment | SSR Server | WASM Client |
|-------------|-----------|-------------|
| Dev | `dev` (opt 1) | `wasm-dev` (opt "s") |
| Prod | `release` (opt 3, thin LTO) | `wasm-release` (opt "z", fat LTO) |

The entire app compiles to a single binary — no Node.js runtime, no separate build tools at deployment time. Static assets (CSS, service worker, manifest) are embedded in the binary via `include_str!`; larger assets (WASM bundle, icons) are served from disk.

**Axum** serves HTTP, SSE, and the REST API. ~70 server functions are registered explicitly (see [Server Functions](server-functions.md) for why).

## Streaming SSR and Skeleton Loaders

Pages use Leptos `Resource::new_blocking` for critical-path data (page structure loads immediately) and `Resource::new` for slower data (recommendations, recents). Non-blocking resources render with `<Suspense>` skeleton fallbacks — the page shell streams immediately, then content fills in progressively. See [Server Functions](server-functions.md) for the resource patterns and nesting rules.

## Embedded Game Databases

~34K console ROMs across 20+ systems (No-Intro + TheGamesDB + libretro-database) and ~15K playable arcade entries (MAME 0.285 + MAME 2003+ + FBNeo + Flycast/Naomi/Atomiswave) are compiled into the binary at build time via PHF (perfect hash function) maps. This provides O(1) lookups from ROM filename stem or CRC32 hash to canonical game data (title, year, genre, developer, players) with zero runtime file I/O.

Non-playable arcade machines (slot machines, gambling, etc.) are filtered at build time.

Systems with embedded data include SG-1000, 32X, and all major consoles from the No-Intro catalog.

See [Design Decisions #10](design-decisions.md) for the trade-offs.

**Files**: `tools/build-catalog/src/main.rs`, `replay-control-core-server/src/game/arcade_db.rs`, `replay-control-core-server/src/game/game_db.rs`

## Embedded Series Database

~5,345 Wikidata series entries across 194+ franchises compiled at build time. Provides game franchise identification, sequel/prequel chains (P155/P156), and ordinals. Bidirectional links are filled at build time so both forward and backward navigation work even when Wikidata only has one direction.

**Files**: `replay-control-core-server/src/game/series_db.rs`

## CRC32 ROM Identification

Hash-based ROM identification for 9 cartridge systems using No-Intro DATs. When a ROM filename doesn't match any database entry, CRC32 hashing provides a second-chance identification path. Hashes are computed lazily and cached in the `game_library` table (`crc32`, `hash_mtime`, `hash_matched_name` columns) to avoid re-hashing unchanged files.

## ROM Filename Parser

Extracts title, region, revision, and classification (hack, translation, special) from No-Intro, GoodTools, and TOSEC naming conventions.

- **No-Intro**: Parenthesized tags — `(USA)`, `(Rev 1)`, `(Hack)`, `(Beta)`, etc.
- **GoodTools**: Bracket flags — `[!]` verified, `[h]` hack, `[cr]` cracked, `[T-Spa]` translation, etc.
- **TOSEC**: Structured tag parsing (year, publisher, side/disk), 17 country code mappings, bracket flag classification with display labels, language codes, and format suffix disambiguation.

See [ROM Classification](rom-classification.md) for the full tier system and tag details.

## Connection Pooling

`deadpool-sqlite` connection pool with separate read/write pools per database. Async API (`pool.get().await` + `conn.interact().await`) prevents tokio worker starvation. Pool sizes tuned for single-user Pi deployment (1 reader + 1 writer per DB). Filesystem-aware journal mode selection (WAL on POSIX, DELETE on exFAT/NFS). WriteGate RAII guard prevents corruption on exFAT during bulk writes.

See [Connection Pooling](connection-pooling.md) for the full architecture.

## Three-Tier Game Library Cache

The game library uses a layered cache architecture:

| Tier | Storage | Lookup Speed | Role |
|------|---------|-------------|------|
| L1 | In-memory (`RwLock<HashMap>`) | ~0ns | Hot cache with mtime-based freshness |
| L2 | SQLite (`game_library` table) | ~1ms | Persistent cache surviving restarts |
| L3 | Filesystem (`roms/` directory) | ~100ms | Source of truth (full scan) |

NFS storage uses a 30-minute hard TTL on L1 as a safety net since inotify cannot detect remote changes.

See [Game Library](../features/game-library.md) for the cache invalidation rules and startup pipeline.

## Broadcast SSE

Two SSE endpoints provide real-time push notifications:

- **`/sse/config`** — pushes skin changes and storage changes to all connected browsers. Skin changes update the app's color scheme instantly; storage changes trigger a full client reload. Initial state snapshot on connect, event-driven updates, 30-second keep-alive.
- **Activity SSE** — background operations (scanning, importing, thumbnail downloads) push progress updates to connected clients instead of clients polling for status.

See [Activity System](activity-system.md) for the mutual exclusion and progress broadcasting design.

## Shared HTTP Client

All outbound HTTP requests use a shared `reqwest` client (`replay-control-core-server/src/http.rs`, `shared_client()`). The client is initialized once with sensible defaults (timeouts, connection pooling) and reused across the app. This replaced earlier curl subprocess calls, reducing overhead and enabling connection reuse for GitHub API, LaunchBox downloads, and thumbnail fetches.

## Analytics Infrastructure

Optional anonymous usage analytics. When the user opts in via Settings, the app collects anonymous usage data (feature usage, library stats) to help improve the product. No personal information or game library contents are transmitted.

## Cross-Compilation

`./build.sh aarch64` produces an ARM binary for Raspberry Pi deployment. The build is a two-step process:

1. WASM hydrate: `cargo build --target wasm32-unknown-unknown --profile wasm-release --features hydrate`
2. wasm-bindgen + wasm-opt (`-Oz`)
3. Server SSR: `cargo build --release --features ssr` (with `aarch64-unknown-linux-gnu` target for Pi)

See [Design Decisions #13](design-decisions.md) for why the project uses a custom build script instead of cargo-leptos.

## REST API

`/api/core/` endpoints serve the libretro core running on the TV. Lightweight JSON responses for recents, favorites, and game detail data (box art, metadata, screenshot paths).

See [Libretro Core](../features/libretro-core.md) for the API contract.

## Auto-Update System

The app checks GitHub releases for new versions and handles the full download-install-restart cycle from the web UI.

**Update check**: A background task runs 60 seconds after startup and every 24 hours. It queries the GitHub releases API, comparing against the current version. The update channel (stable or beta) determines whether prereleases are considered. Results are broadcast to all connected browsers via the `/sse/config` SSE endpoint as `UpdateAvailable` events.

**Update state**: The `UpdateState` enum (`None` → `Available` → `Restarting`) is provided as app-level context and drives the update banner on the Settings page. The banner shows "Update Now", "View on GitHub", and "Skip this version" actions.

**Install flow**: Clicking "Update Now" navigates to `/updating`, which triggers `start_update()`. This downloads the binary and site tarballs from the GitHub release, verifies them, writes a shell script (`/var/tmp/replay-control-do-update.sh`) that replaces the binary and restarts the service, then executes it. The updating page shows a countdown and auto-reloads when the new version responds. Rollback is supported via `.bak` of the previous binary.

**Configuration**: `UpdateChannel` (stable/beta) is stored in `AppSettings`. An optional GitHub API key raises the rate limit from 60 to 5,000 requests/hour.

Key types: `UpdateState`, `AvailableUpdate`, `UpdateChannel` in `replay-control-core/src/update.rs`. HTTP polling, asset download, and `available.json` fs helpers in `replay-control-core-server/src/update.rs` (gated on `http`). Server functions in `replay-control-app/src/server_fns/`. App-side orchestration (24h timer, SSE broadcast, systemctl restart of the running service) in `replay-control-app/src/api/background.rs`. UI in `replay-control-app/src/pages/updating.rs` and `replay-control-app/src/pages/settings.rs`.

## Internationalization

Full UI available in English, Spanish, and Japanese. Translation keys are defined as an enum in `replay-control-app/src/i18n/keys.rs` with per-language match arms. Locale is auto-detected from the browser or manually selected in Settings. SSR renders in the correct language from the first byte — the `<html lang>` attribute is set server-side.

## PWA and Service Worker

Installable as a home screen app on mobile devices. The service worker precaches the app shell (CSS, WASM, JS, icons) for offline loading. When the Pi is unreachable, a fallback page is shown instead of a browser error. Pull-to-refresh support on iOS standalone mode.

Static assets under `pkg/` use 1-hour cache-control headers; other static assets use standard caching.

## Responsive Design

Mobile-first with breakpoints at 600px (small tablet), 768px (tablet landscape), 900px (medium tablet), and 1024px (desktop). Grids, hero cards, screenshots, and navigation adapt at each breakpoint. CSS is compiled from partials at build time and embedded in the binary.
