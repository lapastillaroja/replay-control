# Source Code Analysis

Comprehensive analysis of the RePlayOS companion app codebase. Based on reading every source file in both crates.

**Codebase snapshot**: March 2026

---

## 1. Architecture Overview

### Workspace Structure

```
replay/
  Cargo.toml                  # Workspace root (resolver v2)
  replay-control-core/        # Library crate -- pure logic, no web framework
  replay-control-app/         # Binary + library crate -- Leptos 0.7 SSR app
```

### Two-Crate Design

**replay-control-core** is a pure Rust library with no web dependencies. It handles:
- System definitions (41 visible systems across 5 categories, 1 hidden)
- ROM scanning, listing, sorting (region/quality tiers)
- Favorites CRUD with deep nesting and organization by 5 criteria
- Recents parsing from `.rec` files
- Config file reading/writing (`replay.cfg` key=value format)
- Storage detection (SD/USB/NFS) with disk usage via `df`
- ROM tag parsing (No-Intro/GoodTools naming conventions)
- Game launching via autostart file + systemctl
- Embedded game databases via PHF (Perfect Hash Function):
  - `arcade_db`: ~28K arcade game entries with driver status, rotation, category, players
  - `game_db`: ~34K non-arcade game entries with genre, developer, year, players, CRC32
- LaunchBox XML metadata import (streaming parser)
- SQLite metadata cache with NFS nolock VFS fallback
- Thumbnail matching from libretro-thumbnails repos (3-tier fuzzy matching)
- Screenshot discovery by ROM filename prefix
- Video URL parsing (YouTube, Twitch, Vimeo, Dailymotion)
- Video storage as JSON
- Skin palette definitions (11 built-in themes via CSS custom properties)

**replay-control-app** is a Leptos 0.7 application that compiles in two modes:
- `--features ssr`: Server binary (Axum web server with SSR rendering)
- `--features hydrate`: WASM client (hydrates server-rendered HTML in the browser)

The app layer provides:
- 12 page routes with SSR + client-side hydration
- 51 server functions across 8 domain modules (RPC calls from client to server)
- REST API endpoints for external access
- SSE (Server-Sent Events) for real-time progress streaming
- IntersectionObserver-based infinite scroll
- Service worker for PWA installability
- Keyboard shortcut ("/" for search)
- Mirror types pattern for WASM (core types unavailable under `hydrate` feature)

### Request Flow

```
Browser request
  --> Axum Router
    --> Static files (/pkg, /icons, /style.css, /manifest.json, /sw.js)
    --> REST API (/api/*)
    --> Server functions (/sfn/*)
    --> SSE (/sse/image-progress)
    --> Media files (/media/*)
    --> User captures (/captures/*)
    --> SSR fallback (all other routes --> Leptos SSR --> HTML with hydration scripts)
```

After initial page load, the WASM bundle hydrates and all subsequent navigation + data fetching happens client-side via server functions.

### Server Function Registration

Because server functions are defined in a library crate, Rust's linker strips the `inventory` auto-registration entries. All 51 server functions require explicit `register_explicit::<T>()` calls in `main.rs`. This is a known Leptos limitation with the library crate pattern.

### State Management

`AppState` is the central server-side state, held in `Arc`-wrapped fields:
- `storage: Arc<RwLock<StorageLocation>>` -- hot-swappable storage location
- `config: Arc<RwLock<ReplayConfig>>` -- replay.cfg contents, re-read on refresh
- `cache: Arc<RomCache>` -- TTL-based filesystem scan cache (30s expiry)
- `metadata_db: Arc<Mutex<Option<MetadataDb>>>` -- lazily-opened SQLite handle
- `import_progress / image_import_progress` -- shared progress for background tasks
- `image_import_cancel: Arc<AtomicBool>` -- cooperative cancellation flag
- `skin_override: Arc<RwLock<Option<u32>>>` -- app-specific skin override
- `storage_path_override: Option<PathBuf>` -- CLI override disables auto-detection

### Background Tasks

Two background tasks run at startup:
1. **Storage watcher** (`spawn_storage_watcher`): Re-detects storage location every 60 seconds (skipped if `--storage-path` CLI override is set)
2. **Auto-import** (`spawn_auto_import`): If `Metadata.xml` exists in the expected location and the metadata DB is empty, automatically triggers a metadata import

### Config Boundary

Two separate config files serve different purposes:
- `replay.cfg`: Belongs to RePlayOS. Lives on the SD card at `/media/sd/config/replay.cfg` -- always, regardless of whether ROM storage is on SD, USB, or NFS. The app reads freely and writes only parameters that RePlayOS does not provide its own UI for (e.g., skin index)
- `.replay-control/config.cfg`: App-specific settings (same key=value format), stored on the ROM storage device. For settings that do not belong in the RePlayOS config

---

## 2. Full Feature Inventory

### Pages (12 routes)

| Route | Page Component | Description |
|-------|---------------|-------------|
| `/` | `HomePage` | Dashboard with last played hero card, recent games scroll, stats grid (games/systems/favorites/storage), systems grid |
| `/games/:system` | `SystemRomView` | ROM list for a system with search, filters, infinite scroll, inline rename/delete |
| `/games/:system/:filename` | `GameDetailPage` | Full game detail with metadata, screenshots, videos, captures, launch, favorite, rename, delete |
| `/favorites` | `FavoritesPage` | Favorites with grouped/flat view toggle, organize panel, filter bar |
| `/favorites/:system` | `SystemFavoritesPage` | System-scoped favorites view |
| `/search` | `SearchPage` | Global search with debounce, genre filter, hide hacks/translations/betas/clones, multiplayer filter, recent searches, random game |
| `/more` | `MorePage` | Settings menu with system info display |
| `/more/wifi` | `WifiPage` | Wi-Fi SSID/password/country/security config with reboot button |
| `/more/nfs` | `NfsPage` | NFS server/share/version config with reboot button |
| `/more/hostname` | `HostnamePage` | Network hostname editor |
| `/more/metadata` | `MetadataPage` | Metadata import (descriptions/ratings), image import with per-system progress, coverage stats, re-match, clear |
| `/more/skin` | `SkinPage` | 11 skin palettes with preview cards, sync-with-ReplayOS toggle |
| `/more/logs` | `LogsPage` | System log viewer (all/replay-control/replay sources) |

### Shared Components (7)

| Component | File | Description |
|-----------|------|-------------|
| `BottomNav` | `nav.rs` | 3-tab navigation (Games/Favs/More) with active state from URL |
| `RomList` | `rom_list.rs` | Reusable ROM list with search, 6 filter types, infinite scroll, inline actions |
| `SystemCard` | `system_card.rs` | Card for systems grid (name/manufacturer/count/size) |
| `HeroCard` / `GameScrollCard` | `hero_card.rs` | Reusable cards for home and favorites pages |
| `GenreDropdown` | `genre_dropdown.rs` | Shared genre filter dropdown, used by rom_list and search |
| `RebootButton` | `reboot_button.rs` | Shared reboot button with loading state and result display, used by wifi and nfs pages |
| `SearchShortcut` | `lib.rs` | Invisible component installing "/" keyboard shortcut for search navigation |

### Server Functions (51 public async functions)

Split across 8 domain modules in `server_fns/`:

**System & Storage (5)** -- `system.rs`:
`get_info`, `get_systems`, `get_recents`, `get_system_logs`, `refresh_storage`

**ROMs & Game Launch (5)** -- `roms.rs`:
`get_roms_page`, `get_rom_detail`, `delete_rom`, `rename_rom`, `launch_game`

**Favorites (7)** -- `favorites.rs`:
`get_favorites`, `get_system_favorites`, `add_favorite`, `remove_favorite`, `organize_favorites`, `group_favorites`, `flatten_favorites`

**Search & Filters (4)** -- `search.rs`:
`global_search`, `get_all_genres`, `get_system_genres`, `random_game`

**Settings (11)** -- `settings.rs`:
`get_wifi_config`, `save_wifi_config`, `get_nfs_config`, `save_nfs_config`, `get_hostname`, `save_hostname`, `get_skins`, `set_skin`, `set_skin_sync`, `restart_replay_ui`, `reboot_system`

**Metadata (7)** -- `metadata.rs`:
`get_metadata_stats`, `import_launchbox_metadata`, `download_metadata`, `get_import_progress`, `get_system_coverage`, `clear_metadata`, `regenerate_metadata`

**Images (8)** -- `images.rs`:
`import_system_images`, `import_all_images`, `rematch_all_images`, `cancel_image_import`, `get_image_import_progress`, `get_image_coverage`, `get_image_stats`, `clear_images`

**Videos (4)** -- `videos.rs`:
`get_game_videos`, `add_game_video`, `remove_game_video`, `search_game_videos`

### REST API Endpoints (5 route groups)

Kept for external tool access (curl, scripts):
- `system_info::routes()` -- system info queries
- `roms::routes()` -- ROM listing/actions
- `favorites::routes()` -- favorites CRUD
- `upload::routes()` -- ROM upload
- `recents::routes()` -- recent games

### Embedded Databases

| Database | Entries | Data Source | Lookup Method |
|----------|---------|-------------|---------------|
| `arcade_db` | ~28K | MAME | PHF by ROM stem (e.g., "pacman") |
| `game_db` | ~34K | Screenscraper | PHF by `system~stem`, normalized title fallback, CRC32 fallback, tilde-split fallback |

### Core Library Modules (21 source files)

| Module | Lines | Purpose |
|--------|-------|---------|
| `rom_tags.rs` | 1,135 | No-Intro/GoodTools tag parser, tier classification, region detection |
| `favorites.rs` | 862 | CRUD, organize by 5 criteria, flatten, deduplication, sanitize folder names |
| `thumbnails.rs` | 765 | libretro-thumbnails import, 3-tier fuzzy matching, fake symlink resolution, staleness check |
| `metadata_db.rs` | 511 | SQLite cache with NFS nolock fallback, WAL mode, batch operations |
| `launchbox.rs` | 506 | Streaming XML parser for LaunchBox Metadata.xml |
| `game_db.rs` | 470 | PHF map for ~34K games, normalized title fallback, CRC32 lookup |
| `bin/metadata_report.rs` | 458 | CLI tool for metadata coverage reporting |
| `systems.rs` | 392 | 42 system definitions (41 visible), categories, extensions |
| `roms.rs` | 332 | ROM scanning, listing, sorting, delete, rename, duplicate detection |
| `video_url.rs` | 320 | YouTube/Twitch/Vimeo/Dailymotion URL parsing and embed generation |
| `skins.rs` | 259 | 11 skin palettes, CSS variable generation |
| `arcade_db.rs` | 239 | PHF map for ~28K arcade games, DriverStatus, Rotation enums |
| `config.rs` | 213 | replay.cfg parser with comment-preserving write-back |
| `recents.rs` | 173 | `.rec` file parsing, deduplication |
| `storage.rs` | 165 | Storage detection (SD/USB/NFS), disk usage via `df` |
| `screenshots.rs` | 145 | Screenshot matching by ROM filename prefix |
| `launch.rs` | 143 | Game launching via autostart + systemctl with health check |
| `videos.rs` | 98 | Video storage as JSON in `.replay-control/videos.json` |
| `game_ref.rs` | 82 | Display name resolution from arcade_db/game_db with tag enrichment |
| `error.rs` | 45 | `thiserror`-based error enum |
| `lib.rs` | 22 | Module declarations, `metadata` feature gate |

### Systems Supported (42 total, 41 visible)

**Arcade (4)**: arcade, arcade_dc (Atomiswave/Naomi/Naomi2), neo_geo, neo_geo_pocket_color

**Console (18)**: Atari 2600/5200/7800, ColecoVision, Intellivision, NES, SNES, N64, N64DD, GameCube, Genesis, Master System, Saturn, Dreamcast, PS1, TG-16, TG-CD, Vectrex, 3DO, Jaguar

**Computer (8)**: Amiga, Amstrad CPC, Atari ST, C64, DOS, MSX, ZX Spectrum, Apple II

**Handheld (6)**: Game Boy, GBC, GBA, Game Gear, Lynx, WonderSwan

**Utility (1)**: alpha_player (hidden)

---

## 3. Code Quality Assessment

### Patterns Used

**Leptos Conventions**:
- `Resource` for async data loading with `Transition` boundaries (avoids content flash on reload)
- `Signal`/`RwSignal` for reactive state, leveraging `Copy` semantics
- `StoredValue` for non-reactive data in closures (avoiding clone explosion)
- `Effect` for side effects (debounce timers, DOM interactions)
- `#[component]` functions with setup-above/view-below pattern
- `Show` for conditional rendering

**Data Architecture**:
- Mirror types pattern (`types.rs`) bridges the SSR/hydrate feature boundary
- `resolve_game_info()` is the single function bridging arcade_db and game_db
- `enrich_from_metadata_cache()` augments game info with external metadata
- TTL-based caching (`RomCache`) avoids repeated filesystem scans
- Cooperative cancellation via `AtomicBool` for long-running imports

**Error Handling**:
- `thiserror` in core crate, `ServerFnError` wrapping at the app boundary
- Core functions return `Result<T, Error>` consistently
- Server functions map core errors via `ServerFnError::new()`

**Testing**:
- 155 `#[test]` functions in the core crate across 11 modules
- 6 `#[test]` functions in the app crate (`util.rs` only)
- No integration tests or end-to-end tests
- Core crate has good unit test coverage for data processing logic
- App crate (components, server functions) has no test coverage

### Code Consistency

**Strengths**:
- Consistent use of Leptos idioms across all 12 pages
- i18n keys used consistently (never hardcoded strings in components)
- Server functions follow a uniform pattern: extract `AppState`, call core, map errors
- CSS follows a systematic naming convention (`.page-*`, `.filter-*`, `.game-detail-*`)
- ROM tag parsing is thorough with 60 test cases

**Inconsistencies**:
- Filter state management varies: some pages use URL params, others use local signals
- Error display varies: some pages show inline errors, others use `ErrorBoundary`

### Areas of Concern

1. **api/mod.rs at 1,439 lines**: Contains `AppState`, `RomCache`, background task spawning, storage watcher, auto-import, and the full image import orchestration pipeline. It handles too many concerns.

2. **game_detail.rs at 1,195 lines**: 8+ sub-components for a single page. The video section alone is substantial. Some of these could be extracted to their own files.

3. **No app-layer tests**: The 6 tests in `util.rs` are the only app crate tests. Server functions, components, and the API layer are untested.

---

## 4. Data Flow

### ROM Discovery to UI

```
Filesystem scan (core/roms.rs)
  --> RomEntry { game: GameRef, size_bytes, is_m3u, is_favorite, ... }
  --> mark_favorites() enriches is_favorite flag
  --> RomCache (api/mod.rs) caches results for 30s
  --> get_roms_page() server function:
      - Paginates via offset/PAGE_SIZE (100 items)
      - Calls resolve_game_info() per ROM for display names
      - Calls enrich_from_metadata_cache() for descriptions/ratings/images
      - Returns enriched RomPage { roms, has_more, total }
  --> RomList component:
      - Creates Resource with system + search + filters + page params
      - Renders with Transition (no flicker on reload)
      - Appends pages via IntersectionObserver infinite scroll
      - Each RomItem shows box art, display name, rating badge, driver status badge
```

### Game Detail Pipeline

```
URL params (:system, :filename)
  --> get_rom_detail() server function:
      - resolve_game_info(system, filename, rom_path)
      - enrich_from_metadata_cache() for description/rating/images
      - screenshots::find_screenshots() for user captures
      - Return RomDetail { game: GameInfo, file_size, screenshots }
  --> GameDetailPage component:
      - Hero section with box art + launch button
      - Info grid (system, year, genre, developer, players, etc.)
      - Screenshots carousel
      - Videos section (user-saved + recommendations)
      - Captures lightbox for user screenshots
      - Actions (favorite, rename, delete)
```

### Search Pipeline

```
User types in search input
  --> 300ms debounce (Effect with set_timeout)
  --> URL param sync (?q=..., ?genre=..., etc.)
  --> global_search() server function:
      - Scans ALL systems via RomCache
      - For each ROM: resolve_game_info() + search_score()
      - search_score() ranks: exact match > starts-with > contains > display name match
      - Applies filters: hide_hacks, hide_translations, hide_betas, hide_clones, multiplayer, genre
      - Sorts by relevance score (descending)
      - Returns GlobalSearchResults { results: Vec<(system, Vec<SearchResult>)>, total }
  --> SearchPage component:
      - Filter chips bar
      - Results grouped by system
      - Recent searches (localStorage)
      - Random game button
```

### Metadata Import Pipeline

```
User clicks "Download / Update" on MetadataPage
  --> download_metadata() server function:
      - Downloads Metadata.xml from launchbox-metadata repo
      - Stores at <storage>/.replay-control/Metadata.xml
  --> import_launchbox_metadata() server function:
      - AppState.start_import() spawns background task
      - Streams XML with launchbox.rs parser
      - Builds ROM index from all systems
      - Matches LaunchBox entries to ROM filenames (normalized title matching)
      - Updates metadata_db SQLite cache
      - Progress tracked via import_progress RwLock
  --> MetadataPage polls get_import_progress() for live updates
```

### Image Import Pipeline

```
User clicks "Download" for a system (or "Download All")
  --> import_system_images() / import_all_images() server function:
      - AppState orchestrates: reuse or clone libretro-thumbnails repo
      - Staleness check (is_repo_stale) compares local vs remote HEAD
      - resolve_fake_symlinks_in_dir() for FAT32/exFAT compatibility (fresh clones only)
      - 3-tier fuzzy matching: exact --> strip-tags --> version-stripped
      - Copies matched images to <storage>/.replay-control/media/<system>/
      - Updates metadata_db with image paths (bulk_update_image_paths)
      - Repos kept on disk in tmp/ for reuse in subsequent imports
      - Progress streamed via SSE at /sse/image-progress (200ms interval, auto-closes when idle)
  --> MetadataPage connects to SSE, displays per-system progress
  --> Cancel button sets image_import_cancel AtomicBool
```

### Favorites Organization Pipeline

```
User selects criteria on OrganizePanel
  --> organize_favorites() server function:
      - Reads all favorites for all systems
      - Groups by primary criteria (system/genre/players/rating/alphabetical)
      - Optionally groups by secondary criteria
      - Creates folder structure on disk
      - Option to keep originals at root (ReplayOS compatibility)
  --> flatten_favorites() reverses: moves all favorites to root level
```

---

## 5. Changes Since Original Analysis

This section documents features and changes added after the initial analysis was written.

### New Features

**Multiplayer Filter**: ROM list and search pages gained a "Multiplayer" filter chip. Uses `players` field from game_db/arcade_db to filter games with 2+ players.

**Rating Display in ROM List**: `RomEntry` gained `rating: Option<f32>` and `players: Option<u8>` fields. Server functions enrich ROM entries with ratings from the metadata cache. ROM list items show star-based rating badges.

**Driver Status Badges**: Arcade ROM entries display colored badges (Working/Imperfect/Preliminary) based on MAME driver emulation status from arcade_db.

**Re-match All Images**: New `rematch_all_images()` server function and "Re-match All" button on MetadataPage. Re-runs image matching using already-downloaded repos without re-downloading. Useful when matching algorithm improves.

**Random Game**: New `random_game()` server function and button on SearchPage. Selects a random ROM across all systems and navigates to its detail page.

**Favorites Filter Bar**: FavoritesPage gained a search/filter bar for filtering favorites by name.

**Recent Searches**: SearchPage stores recent search queries in localStorage and displays them as quick-access chips.

**Genre Browsing**: Search results include genre chips that link to filtered search views ("Browsing all [genre]").

**Video Management**: Full video section on GameDetailPage with YouTube/Twitch/Vimeo URL parsing, embed rendering, video recommendations (find trailers/gameplay/1CC), pin/unpin, user captures display.

**Captures Lightbox**: GameDetailPage shows user screenshots taken on RePlayOS with a lightbox viewer.

**Metadata Download**: `download_metadata()` server function downloads LaunchBox Metadata.xml directly (previously required manual placement). Includes `regenerate_metadata()` for rebuilding from existing XML.

### Core Library Improvements

**Thumbnail Matching Enhancements**:
- `strip_version()`: Strips GDI/TOSEC version strings from filenames for better matching
- `is_repo_stale()`: Compares local vs remote HEAD to determine if re-download is needed
- `resolve_fake_symlinks_in_dir()`: Post-clone resolution of text-file symlinks created by git on FAT32/exFAT filesystems that don't support real symlinks
- Colon variant fallback: Tries both "Game: Subtitle" and "Game - Subtitle" forms
- N64DD prefix handling for combined N64/N64DD repos

**Multi-repo Support**: `arcade_dc` system maps to 3 libretro-thumbnails repos (Atomiswave, Naomi, Naomi 2), with sequential search across all.

**Batch Rating Lookups**: `metadata_db.lookup_ratings()` fetches ratings for multiple ROMs efficiently. `all_ratings()` returns all ratings for organize-by-rating functionality.

**Image Path Bulk Updates**: `metadata_db.bulk_update_image_paths()` uses transactions for efficient batch writes during image import.

### Architectural Changes

**Server Functions Split**: The monolithic `server_fns.rs` (2,322 lines) was split into 8 domain modules under `server_fns/`: `system.rs`, `roms.rs`, `favorites.rs`, `search.rs`, `settings.rs`, `metadata.rs`, `images.rs`, `videos.rs`. Shared types (`GameInfo`, `SystemInfo`) and helpers (`resolve_game_info`, `enrich_from_metadata_cache`) remain in `server_fns/mod.rs`.

**RebootButton Extraction**: The `RebootButton` component, previously duplicated identically in `wifi.rs` and `nfs.rs`, was extracted to a shared `components/reboot_button.rs`. Both pages now import from the shared component.

**Transition Unified**: All pages now consistently use `Transition` instead of `Suspense` for async data loading boundaries. This eliminates content flash when data reloads.

**SSE Auto-Close**: The image progress SSE endpoint now auto-closes after 5 consecutive idle ticks (1 second of no active import) instead of running indefinitely until client disconnect.

**Image Repos Kept on Disk**: Cloned libretro-thumbnails repos in `tmp/` are now kept on disk between imports instead of being deleted after each import. On subsequent imports, repos are reused if not stale. A staleness check (`is_repo_stale`) compares local vs remote HEAD and triggers a fresh clone only when upstream has new images.

**Log Prefix**: The systemd unit name and log source filter changed from `replay-companion` to `replay-control`.

**Genre Dropdown Extraction**: Previously duplicated genre filter logic in rom_list.rs and search.rs was extracted to a shared `GenreDropdown` component (`genre_dropdown.rs`).

**Hero Card Extraction**: `HeroCard` and `GameScrollCard` components were extracted to `hero_card.rs` for reuse across home and favorites pages.

**i18n Simplified**: The locale system was simplified to English-only (single `En` variant). The multi-locale infrastructure (match on locale+key) remains for future expansion but currently has no runtime overhead.

**SSE Progress Streaming**: Image import progress switched from polling to Server-Sent Events at `/sse/image-progress` with 200ms update interval. This provides smoother progress updates compared to the polling approach still used for metadata import.

**App-Specific Config**: Introduction of `.replay-control/config.cfg` for app-specific settings, keeping the `replay.cfg` boundary clean.

---

## 6. Proposed Improvements

### High Priority

**1. Split api/mod.rs into focused modules**

At 1,439 lines, this file handles AppState, RomCache, background tasks, config management, storage detection, metadata DB, and image import orchestration. The image import orchestration alone is hundreds of lines. Proposed split:

```
api/
  mod.rs          # AppState definition + new/storage/config methods
  cache.rs        # RomCache with TTL logic
  background.rs   # spawn_storage_watcher, spawn_auto_import
  import.rs       # start_import, start_image_import, image import orchestration
```

### Medium Priority

**2. Extract game_detail.rs sub-components**

The file has 1,195 lines with 8+ sub-components. The video section (`GameVideoSection`, `VideoEmbed`, `VideoRecommendations`, `RecommendationItem`) could be extracted to `components/video_section.rs`. The captures lightbox could move to `components/captures.rs`.

**3. Unify metadata import progress to SSE**

Metadata import still uses polling (`get_import_progress()` server function called on a timer) while image import uses SSE. Both should use SSE for consistency and efficiency.

**4. Add app-layer tests**

The entire app crate (10,483 lines of Rust) has only 6 tests in `util.rs`. Server function logic -- especially `search_score()`, `resolve_game_info()`, and filter application -- should have unit tests.

**5. Introduce typed filter state**

Filter state (hide_hacks, hide_translations, hide_betas, hide_clones, multiplayer, genre, search query) is managed as individual signals in both rom_list.rs and search.rs. A shared `FilterState` struct would reduce duplication and make filter logic testable.

### Low Priority

**6. CSS organization**

The single `style.css` (2,356 lines) could benefit from being split by page/component for maintainability. However, the current approach avoids CSS module complexity and keeps the build simple.

**7. Lazy-load embedded databases**

The `arcade_db` and `game_db` PHF maps are compiled into the binary (~62K entries total). This increases binary size but provides zero-cost lookups. The tradeoff is acceptable for the deployment target (Raspberry Pi with sufficient RAM), but lazy initialization via `once_cell` could reduce startup memory if needed.

**8. Search performance**

`global_search()` iterates over all ROMs across all systems on every search request. For large collections (10K+ ROMs), this could benefit from a pre-built search index. However, the RomCache already avoids repeated filesystem scans, and the 30s TTL keeps results fresh.

---

## 7. Technical Debt and Known Issues

### Code Duplication

1. **Filter logic** in `rom_list.rs` and `search.rs` -- similar filter chip rendering and state management
2. **Box art URL resolution** appears in multiple server functions (`get_roms_page`, `get_favorites`, `get_recents`, `get_system_favorites`) with slightly different patterns

### Architectural Issues

3. **api/mod.rs monolith** (1,439 lines) -- AppState, caching, background tasks, and import orchestration in one file
4. **51 register_explicit calls in main.rs** -- Brittle: adding a server function requires remembering to add the registration. Forgetting causes silent runtime failures (function returns 404)
5. **Mirror types in types.rs** -- Every core type used in server function signatures must be duplicated. Adding a field to a core type requires updating the mirror. The compiler does not enforce parity.

### Missing Features

6. **No authentication/authorization** -- Any network client can access all functionality including delete, rename, reboot, and config changes. Acceptable for a local-network appliance but limits deployment scenarios.
7. **No rate limiting** -- No protection against rapid repeated requests to expensive operations (ROM scanning, metadata import)
8. **No request validation middleware** -- Path traversal checks are done ad-hoc in individual handlers (media, captures) rather than via middleware

### Known Limitations

9. **Metadata import blocks the DB mutex** -- During metadata import, the `metadata_db` Mutex is held by the import task. Other requests needing metadata (ROM list enrichment, search) must wait or get stale data.
10. **RomCache clones entire ROM lists** -- `get_roms()` returns `Vec<RomEntry>` by cloning. For systems with thousands of ROMs, this creates significant allocation pressure on every cache hit.
11. **SearchShortcut leaks event listener** -- The `Closure::forget()` call in `SearchShortcut` leaks the keydown listener. In a SPA with long sessions, this is a minor memory leak. In practice, only one listener is ever created.
12. **No offline support** -- Despite having a service worker registered, the `sw.js` is minimal and does not implement caching strategies. The PWA works only when connected to the server.

### Testing Gaps

13. **App crate almost untested** -- 10,483 lines of Rust with only 6 tests (`format_size` and `format_size_short`). Server functions, components, and API layer have zero test coverage.
14. **No integration tests** -- No tests verify the full request flow (HTTP request --> server function --> core crate --> response)
15. **No WASM tests** -- Client-side behavior (hydration, infinite scroll, debounce, keyboard shortcuts) is untested

---

## 8. Lines of Code and Complexity Metrics

### Summary

| Component | Lines | Percentage |
|-----------|-------|------------|
| App crate (Rust) | 10,483 | 52.0% |
| Core crate (Rust) | 7,335 | 36.4% |
| CSS | 2,356 | 11.7% |
| **Total** | **20,174** | **100%** |

### App Crate Breakdown (10,483 lines Rust)

**Server-side infrastructure:**

| File | Lines | Description |
|------|-------|-------------|
| `server_fns/mod.rs` | 502 | Shared types (GameInfo, SystemInfo), resolve_game_info, enrich_from_metadata_cache |
| `server_fns/search.rs` | 442 | Global search, genre queries, search_score, random game |
| `server_fns/videos.rs` | 315 | Video CRUD and search |
| `server_fns/roms.rs` | 266 | ROM list/detail, delete, rename, launch |
| `server_fns/settings.rs` | 261 | Wi-Fi, NFS, hostname, skins, restart, reboot |
| `server_fns/images.rs` | 167 | Image import, rematch, cancel, coverage, stats, clear |
| `server_fns/system.rs` | 145 | System info, systems, recents, logs, refresh storage |
| `server_fns/metadata.rs` | 127 | Metadata import, download, coverage, clear, regenerate |
| `server_fns/favorites.rs` | 113 | Favorites CRUD, organize, group, flatten |
| `api/mod.rs` | 1,439 | AppState, RomCache, background tasks, import orchestration |
| `main.rs` | 358 | CLI args, 51 register_explicit calls, Axum router setup |
| `api/favorites.rs` | 104 | REST API favorites routes |
| `api/roms.rs` | 97 | REST API ROM routes |
| `api/upload.rs` | 68 | REST API upload handler |
| `api/system_info.rs` | 52 | REST API system info |
| `api/recents.rs` | 30 | REST API recents |

**Pages:**

| File | Lines | Description |
|------|-------|-------------|
| `pages/game_detail.rs` | 1,195 | Game detail with 8+ sub-components |
| `pages/metadata.rs` | 777 | Metadata + image import management |
| `pages/search.rs` | 735 | Global search with filters |
| `pages/favorites.rs` | 696 | Favorites with organize panel |
| `pages/home.rs` | 187 | Dashboard |
| `pages/wifi.rs` | 149 | Wi-Fi settings |
| `pages/skin.rs` | 165 | Skin selector |
| `pages/nfs.rs` | 115 | NFS settings |
| `pages/hostname.rs` | 91 | Hostname editor |
| `pages/more.rs` | 91 | Settings menu |
| `pages/logs.rs` | 72 | Log viewer |
| `pages/games.rs` | 35 | Thin wrapper for RomList |

**Shared components:**

| File | Lines | Description |
|------|-------|-------------|
| `components/rom_list.rs` | 712 | ROM list with filters + infinite scroll |
| `components/hero_card.rs` | 52 | Reusable game cards |
| `components/reboot_button.rs` | 43 | Shared reboot button with loading state |
| `components/system_card.rs` | 40 | System grid cards |
| `components/nav.rs` | 40 | Bottom navigation |
| `components/genre_dropdown.rs` | 26 | Genre filter dropdown |

**Framework & support:**

| File | Lines | Description |
|------|-------|-------------|
| `i18n.rs` | 350 | ~120 translation keys |
| `lib.rs` | 198 | App root, Shell, Router, routes |
| `types.rs` | 128 | Mirror types for WASM |
| `util.rs` | 79 | Size formatting |

### Core Crate Breakdown (7,335 lines Rust)

| File | Lines | Description |
|------|-------|-------------|
| `rom_tags.rs` | 1,135 | Tag parsing + 60 tests |
| `favorites.rs` | 862 | Favorites CRUD + organize + 11 tests |
| `thumbnails.rs` | 765 | Image matching + fake symlink resolution |
| `metadata_db.rs` | 511 | SQLite metadata cache |
| `launchbox.rs` | 506 | LaunchBox XML parser |
| `game_db.rs` | 470 | ~34K game PHF + 29 tests |
| `bin/metadata_report.rs` | 458 | Metadata coverage CLI tool |
| `systems.rs` | 392 | 42 system definitions + 4 tests |
| `roms.rs` | 332 | ROM scan/list/sort + 3 tests |
| `video_url.rs` | 320 | Video URL parsing + 10 tests |
| `skins.rs` | 259 | 11 skin palettes + 6 tests |
| `arcade_db.rs` | 239 | ~28K arcade PHF + 19 tests |
| `config.rs` | 213 | Config parser + 6 tests |
| `recents.rs` | 173 | Recents parser + 4 tests |
| `storage.rs` | 165 | Storage detection |
| `screenshots.rs` | 145 | Screenshot discovery + 3 tests |
| `launch.rs` | 143 | Game launching |
| `videos.rs` | 98 | Video JSON storage |
| `game_ref.rs` | 82 | GameRef with display name resolution |
| `error.rs` | 45 | Error types |
| `lib.rs` | 22 | Module declarations |

### Test Coverage

| Crate | Test Functions | Lines with Tests | Notes |
|-------|---------------|-----------------|-------|
| Core | 155 | All major modules | Good coverage of data processing logic |
| App | 6 | `util.rs` only | No tests for server functions, components, or API |
| **Total** | **161** | | |

**Core crate test distribution:**
- `rom_tags.rs`: 60 tests (tag parsing edge cases)
- `game_db.rs`: 29 tests (lookup methods, normalization)
- `arcade_db.rs`: 19 tests (arcade lookups)
- `favorites.rs`: 11 tests (CRUD, organize, deduplicate)
- `video_url.rs`: 10 tests (URL parsing for 4 platforms)
- `config.rs`: 6 tests (parse, write, preserve comments)
- `skins.rs`: 6 tests (palette generation)
- `recents.rs`: 4 tests (parsing, deduplication)
- `systems.rs`: 4 tests (lookup, extensions)
- `roms.rs`: 3 tests (scan, extensions)
- `screenshots.rs`: 3 tests (matching, timestamps)

### Complexity Hotspots

Files over 500 lines that warrant attention:

| File | Lines | Concern |
|------|-------|---------|
| `api/mod.rs` | 1,439 | Too many responsibilities |
| `game_detail.rs` | 1,195 | Many sub-components in one file |
| `rom_tags.rs` | 1,135 | Inherently complex but well-tested (60 tests) |
| `favorites.rs` | 862 | Complex but well-tested (11 tests) |
| `metadata.rs` | 777 | Complex page but self-contained |
| `thumbnails.rs` | 765 | Multi-repo matching, fake symlinks, staleness check |
| `search.rs` | 735 | Filter logic partially duplicated with rom_list |
| `rom_list.rs` | 712 | Filter logic partially duplicated with search |
| `favorites.rs` (page) | 696 | Multiple views (grouped/flat/system) |

### Build Artifacts

The project produces:
- Server binary (SSR): standard Rust binary for the target architecture
- WASM bundle (hydrate): `replay_control_app.wasm` + `replay_control_app.js` in `target/site/pkg/`
- CSS: included via `include_str!` at compile time (no separate file serving needed beyond initial load)
- Static assets: `manifest.json`, `sw.js`, app icons in `target/site/`
