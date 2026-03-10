# Source Code Analysis

Comprehensive analysis of the RePlayOS companion app codebase. Based on reading every source file in both crates.

**Codebase snapshot**: March 2026

---

## 1. Architecture Overview

### Workspace Structure

```
replay/
  Cargo.toml                  # Workspace root (resolver v2)
  replay-control-core/        # Library crate — pure logic, no web framework
  replay-control-app/          # Binary + library crate — Leptos 0.7 SSR app
```

### Two-Crate Design

**replay-control-core** is a pure Rust library with no web dependencies. It handles:
- System definitions (41 systems across 5 categories)
- ROM scanning, listing, sorting (region/quality tiers)
- Favorites CRUD with deep nesting and organization by criteria
- Recents parsing from `.rec` files
- Config file reading/writing (`replay.cfg` key=value format)
- Storage detection (SD/USB/NFS) with disk usage via `df`
- ROM tag parsing (No-Intro/GoodTools naming conventions)
- Game launching via autostart file + systemctl
- Screenshot matching by ROM filename prefix
- Video URL parsing (YouTube/Twitch/Vimeo/Dailymotion)
- Video storage as JSON files
- Embedded game databases: arcade (~28K entries via PHF) and non-arcade (~34K entries via PHF)
- Skin/theme definitions (11 built-in palettes)
- Thumbnail matching with 3-tier fuzzy fallback
- SQLite metadata cache with NFS-safe nolock fallback (feature-gated behind `metadata`)
- LaunchBox XML streaming import with normalized title matching

**replay-control-app** is a dual-target crate (cdylib for WASM, rlib+bin for server):
- `ssr` feature: Axum server, server functions, REST API, SSE, static file serving
- `hydrate` feature: WASM client-side hydration, IntersectionObserver, EventSource, localStorage
- Pages: Home, Games (system view), Game Detail, Favorites, Search, More (settings menu), WiFi, NFS, Hostname, Metadata, Skin, Logs
- Components: BottomNav, SystemCard, RomList
- Server functions: 50+ `#[server]` functions in a single file
- REST API: parallel set of endpoints for external access (system info, ROMs, favorites, upload, recents)
- i18n: manual match-based translation (~200 keys, 3 locales: en/es/pt)
- Mirror types: client-side type mirrors in `types.rs` for WASM (core types only available under `ssr`)

### Data Flow

```
Browser → WASM hydrate → Server Functions (POST /sfn/*) → AppState → Core Library → Filesystem/SQLite
                                                                    ↕
Browser → REST API (GET/POST /api/*) → AppState → Core Library → Filesystem/SQLite
```

Server functions and REST API are **redundant** — both paths exist. Server functions are used by the Leptos UI; REST API is kept for external access (e.g., curl, automation).

### Key Design Decisions

1. **No cargo-leptos**: Custom `build.sh` handles WASM compilation + server build. `dev.sh` adds file watching and auto-reload.
2. **Manual server function registration**: `inventory` auto-registration gets stripped by the linker when server functions are in a library crate. Every server function needs `register_explicit::<T>()` in `main.rs`.
3. **`any_spawner::Executor::init_tokio()`**: Must be called before SSR — Leptos 0.7's `render_app_to_stream_with_context` does not initialize it automatically.
4. **Mirror types pattern**: Core types are only available under `#[cfg(feature = "ssr")]`. The app defines parallel structs in `types.rs` with `#[cfg(not(feature = "ssr"))]` for WASM.
5. **Config boundary**: `replay.cfg` belongs to RePlayOS. The app reads it freely but only writes parameters that RePlayOS has no UI for (e.g., skin). App-specific settings go in `.replay-control/config.cfg`.
6. **PHF embedded databases**: Build scripts generate Perfect Hash Function maps from CSV/XML data files at compile time, embedding ~62K game entries directly into the binary.

---

## 2. Code Quality Assessment

### Strengths

- **Clean separation of concerns**: Core library has zero web dependencies. All filesystem, config, and game logic is testable in isolation.
- **Extensive test coverage in core**: `rom_tags.rs` (1136 lines with tests), `favorites.rs` (863 lines with extensive tests), `config.rs`, `recents.rs`, `video_url.rs`, `game_db.rs`, and `arcade_db.rs` all have thorough unit tests.
- **Good use of Leptos 0.7 patterns**: Components consistently use `Suspend` + `ErrorBoundary`, `StoredValue` for non-Copy data, `Show` for conditionals, `bind:value` for two-way binding.
- **Robust NFS support**: SQLite opens with `nolock` VFS fallback when standard locking fails. Storage detection handles USB/NFS/SD transparently.
- **Smart caching**: `RomCache` with TTL avoids repeated filesystem traversals on NFS.
- **Well-structured CSS**: Single file (~2237 lines) using CSS custom properties for theming. Responsive breakpoints at 600px/768px/1024px. Clean BEM-ish naming.
- **Comprehensive i18n**: All user-visible strings are translated (en/es/pt).

### Weaknesses

- **`server_fns.rs` monolith**: ~1700+ lines, 50+ server functions, 20+ struct definitions. This is the single biggest code quality issue.
- **Code duplication**: Multiple instances across components (detailed in Section 12).
- **Inconsistent error handling**: Some server functions use `?` propagation, others use manual `map_err`. Some swallow errors silently (e.g., `let _ = db.clear()`).
- **`style.css` is a single 2237-line file**: No modular CSS, no CSS modules or scoping. Risk of selector collisions in a growing app.
- **`game_detail.rs` complexity**: ~1196 lines for a single page with 8 sub-components. The `GameVideoSection` alone is ~300 lines.
- **No automated integration tests**: Core library has unit tests; the app has none.

### Adherence to CLAUDE.md Conventions

The codebase generally follows the coding rules defined in `CLAUDE.md`:
- Setup-above-view-below pattern is consistently applied
- `StoredValue` is used throughout instead of clone explosions
- `Suspend` + `ErrorBoundary` for async data
- `Show` for conditionals
- Signals treated as Copy

**Deviations**: The `on_search_keydown` handler in `home.rs` duplicates the form submit logic (lines 33-43 vs 21-31) instead of extracting a shared function. Several pages (search, rom_list) have inline `.map()` chains that could be extracted into components per CLAUDE.md guidance.

---

## 3. `server_fns.rs` Analysis

### Size and Scope

This file contains approximately 1700 lines with:
- 50+ `#[server]` functions
- 20+ struct/enum definitions (`GameInfo`, `SystemInfo`, `RomPage`, `RomDetail`, `WifiConfig`, `NfsConfig`, `SkinInfo`, `ImageImportProgress`, `ImageImportState`, `GlobalSearchResults`, `GlobalSearchResult`, `VideoEntry`, `VideoRecommendation`, `ScreenshotUrl`, etc.)
- Helper functions (`resolve_game_info`, `enrich_from_metadata_cache`, `find_image_on_disk`, `search_score`, `lookup_genre`)

### Functional Groups

The functions naturally cluster into these domains:

| Group | Functions | ~Lines |
|-------|-----------|--------|
| System info | `get_info`, `get_systems` | ~80 |
| ROM listing/detail | `get_roms_page`, `get_rom_detail`, `delete_rom`, `rename_rom`, `launch_game` | ~200 |
| Favorites | `get_favorites`, `add_favorite`, `remove_favorite`, `group_favorites`, `flatten_favorites`, `get_system_favorites`, `organize_favorites` | ~150 |
| Recents | `get_recents` | ~40 |
| Search | `global_search`, `get_all_genres`, `get_system_genres`, `random_game` | ~200 |
| Config (WiFi/NFS/Hostname) | `get_wifi_config`, `save_wifi_config`, `get_nfs_config`, `save_nfs_config`, `get_hostname`, `save_hostname` | ~100 |
| Metadata | `get_metadata_stats`, `import_launchbox_metadata`, `clear_metadata`, `regenerate_metadata`, `download_metadata`, `get_import_progress`, `get_system_coverage` | ~150 |
| Images | `import_system_images`, `import_all_images`, `get_image_import_progress`, `get_image_coverage`, `get_image_stats`, `clear_images`, `cancel_image_import` | ~200 |
| Skins | `get_skins`, `set_skin`, `set_skin_sync` | ~80 |
| System ops | `restart_replay_ui`, `reboot_system`, `refresh_storage`, `get_system_logs` | ~60 |
| Videos | `get_game_videos`, `add_game_video`, `remove_game_video`, `search_game_videos` | ~150 |

### Key Helper Functions

- **`resolve_game_info()`**: Builds a `GameInfo` struct from a `RomEntry` by combining embedded DB lookups (arcade_db/game_db), ROM tag parsing, and metadata cache. This is the central game data resolution function.
- **`enrich_from_metadata_cache()`**: Adds description, rating, publisher, and image URLs from SQLite metadata DB.
- **`find_image_on_disk()`**: Scans the media directory for boxart/snap images. **Performance concern**: does filesystem listing per ROM.
- **`search_score()`**: Multi-factor relevance scoring for global search (name match, favorite bonus, metadata bonus, tag penalties).
- **`lookup_genre()`**: Resolves normalized genre from arcade_db, game_db, or metadata DB.

### Splitting Recommendation

This file should be split into modules matching the functional groups above. Each module would be a file under `src/server_fns/` with a `mod.rs` re-exporting everything. Shared types (`GameInfo`, `SystemInfo`, etc.) would go in a `types.rs` submodule.

---

## 4. Component Analysis

### Pages (12 total)

| Page | File | Lines | Complexity |
|------|------|-------|------------|
| Home | `home.rs` | 238 | Medium — 3 Resources, search bar, hero card, recent scroll, stats, systems grid |
| Games | `games.rs` | 36 | Low — thin wrapper delegating to RomList |
| Game Detail | `game_detail.rs` | 1196 | **High** — 8 sub-components (Content, LaunchAction, RenameAction, DeleteAction, CapturesLightbox, VideoSection, VideoEmbed, VideoRecommendations, RecommendationItem) |
| Favorites | `favorites.rs` | 625 | High — hero card, stats, organize panel with criteria dropdowns, grouped/flat views |
| Search | `search.rs` | 686 | High — debounced input, URL param sync, filter chips, genre dropdown, recent searches (localStorage), random game, grouped results |
| More | `more.rs` | 92 | Low — settings menu with MenuItem links |
| WiFi | `wifi.rs` | 188 | Medium — form with validation, RebootButton |
| NFS | `nfs.rs` | 154 | Medium — form with validation, RebootButton (duplicated from wifi.rs) |
| Hostname | `hostname.rs` | 92 | Low — simple form |
| Metadata | `metadata.rs` | 736 | **High** — description stats, download/import with polling progress, per-system coverage, image section with SSE progress, clear images, attribution |
| Skin | `skin.rs` | 166 | Medium — grid with preview cards, sync toggle |
| Logs | `logs.rs` | 73 | Low — source filter dropdown, refresh button, pre-formatted output |

### Shared Components

| Component | File | Lines | Notes |
|-----------|------|-------|-------|
| BottomNav | `nav.rs` | 41 | 3 tabs (Home/Favorites/More), active state from URL |
| SystemCard | `system_card.rs` | 41 | Card for systems grid, shows name/manufacturer/count |
| RomList | `rom_list.rs` | 676 | **Complex** — search, filters, pagination, infinite scroll (IntersectionObserver), debounce, URL sync, genre dropdown, RomItem with fav toggle/rename/delete |

### Component Quality Notes

- **GameDetailContent** (game_detail.rs): Good use of `StoredValue` for non-Copy data, `Show` for conditionals. The video section is self-contained with its own state management. The lightbox has keyboard navigation. Overall well-structured despite its size.
- **RomList** (rom_list.rs): The IntersectionObserver-based infinite scroll is correctly implemented with cleanup. The debounce logic uses `gloo_timers` futures. However, the genre dropdown is duplicated from search.rs.
- **Search** (search.rs): URL param sync via `window().location()` is well-implemented. Recent searches use localStorage correctly. The filter chip UI pattern is duplicated from rom_list.rs.

---

## 5. CSS Analysis

### Structure

Single file: `style/style.css` — 2237 lines. Included via `include_str!()` in main.rs and served at `/style.css`.

### Design System

CSS custom properties defined in `:root`:
```css
--bg, --surface, --surface-hover, --border
--text, --text-secondary
--accent, --accent-hover
--star, --error, --success
--radius (12px), --radius-sm (8px)
--nav-height (64px), --top-bar-height (56px)
```

Skin theming overrides these via dynamically generated `<style>` blocks from `skins.rs::theme_css()`.

### Responsive Breakpoints

- `min-width: 600px` — tablet adjustments (skin grid 3-col, manage actions row, metadata import form row)
- `min-width: 768px` — systems grid 3-col, stats grid 4-col, game meta 3-col, game actions row
- `min-width: 900px` — skin grid 4-col
- `min-width: 1024px` — systems grid 4-col, game meta 4-col

### Key Patterns

- **Safe area handling**: Uses `env(safe-area-inset-*)` for PWA/mobile notch avoidance
- **Dark-only theme**: No light mode support. All colors are dark-palette.
- **Hover detection**: Uses `@media (hover: hover)` to hide ROM action buttons on desktop, showing them only on hover
- **Video embeds**: 16:9 responsive container via `padding-bottom: 56.25%` trick
- **Lightbox**: Fixed overlay with `z-index: 1000`, pixelated image rendering for retro screenshots

### Issues

1. **No modular CSS**: Everything in one file. No CSS modules, no scoping. Selector names are globally unique but this is fragile.
2. **Duplicated input styles**: `.search-input`, `.home-search-input`, `.search-page-input`, `.form-input`, `.rename-input` — similar but slightly different styles for text inputs.
3. **No animation system**: Only one `@keyframes` definition (`pulse-opacity`). All transitions are inline `transition:` properties.
4. **Missing dark mode toggle**: The app is dark-only. A `prefers-color-scheme` media query for system theme integration is absent.

---

## 6. Core Library Analysis

### Module Breakdown

| Module | Lines | Test Coverage | Notes |
|--------|-------|---------------|-------|
| `systems.rs` | 373 | Yes | 41 static system definitions, `find_system()`, `system_from_fav_filename()` |
| `roms.rs` | ~320 | Yes | Recursive scanning, region/quality tier sorting, favorite marking, delete, rename, duplicate detection |
| `favorites.rs` | 863 | **Extensive** | CRUD, organize by 5 criteria (system/genre/players/rating/alpha), flatten, deep nesting, deduplication |
| `recents.rs` | 174 | Yes | `.rec` file parsing, deduplication of favorite/non-favorite entries |
| `config.rs` | 214 | Yes | `key="value"` parser, write-back preserving comments and unknown keys |
| `storage.rs` | 166 | No | Storage detection via config + filesystem probing, `df` command for disk usage |
| `rom_tags.rs` | 1136 | **Extensive** | No-Intro/GoodTools tag parser: regions, revisions, translations, hacks, betas, etc. |
| `game_ref.rs` | 83 | No | `GameRef` with display name resolution from arcade_db/game_db |
| `screenshots.rs` | 146 | Yes | Screenshot matching by ROM filename prefix, timestamp parsing |
| `launch.rs` | 143 | No | Autostart file creation + `systemctl restart`, health check recovery |
| `video_url.rs` | 321 | Yes | YouTube/Twitch/Vimeo/Dailymotion URL parsing, canonical/embed URL generation |
| `videos.rs` | 99 | No | Video storage as JSON in `.replay-control/videos/` |
| `skins.rs` | 260 | Yes | 11 skin palettes, CSS variable generation, theme color extraction |
| `arcade_db.rs` | 240 | Yes | PHF map of ~28K arcade games (FBNeo + MAME 2003+ + MAME current + Flycast) |
| `game_db.rs` | 471 | Yes | PHF maps of ~34K non-arcade games (No-Intro DATs + TheGamesDB + libretro metadata), normalized title fallback, CRC32 lookup |
| `thumbnails.rs` | 567 | No | 3-tier fuzzy thumbnail matching, git clone with cancel, fake symlink resolution |
| `metadata_db.rs` | 461 | No | SQLite cache with NFS nolock fallback, bulk upsert/update, per-system stats |
| `launchbox.rs` | 507 | No | Streaming XML parser, normalized title matching with article reordering, download via curl |
| `error.rs` | 46 | No | `thiserror`-based error enum |

### Build-Time Code Generation

The core crate has build scripts that:
1. Parse MAME XML files and FBNeo CSV for arcade game data → generates `arcade_db.rs` with a PHF map
2. Parse No-Intro DAT files, cross-reference with TheGamesDB JSON and libretro-database metadata → generates `game_db.rs` with per-system PHF maps, canonical game tables, CRC32 indexes, and normalized title indexes

This approach embeds ~62K game entries into the binary with O(1) lookup time and zero runtime parsing.

### Core Design Quality

- **Title normalization** is thorough: handles `"Title, The"` ↔ `"The Title"` reordering, strips parenthesized tags, removes punctuation, collapses whitespace. Used in both game_db (build-time) and launchbox (runtime).
- **ROM tag parsing** (`rom_tags.rs`) is impressively comprehensive: handles regions, revisions, translations, patches, hacks, betas, prototypes, alternate dumps, BIOS, unlicensed, with classification into quality tiers for sorting.
- **Favorites organization** supports 5 criteria (system, genre, players, rating, alphabetical) with primary+secondary grouping and deduplication. Tested with 20+ test cases.
- **Config writer** preserves comments and unknown keys when writing back — respectful of shared config ownership with RePlayOS.

---

## 7. Performance Concerns

### High Priority

1. **`find_image_on_disk()` does filesystem scanning per ROM**: In `server_fns.rs`, this function reads the media directory listing for every ROM to check for boxart/snap images. For a system with 1000 ROMs, this means 1000 directory listings. Should build an index once and cache it.

2. **`get_all_genres()` iterates all ROMs across all systems**: This server function scans every ROM directory, builds full ROM lists, and extracts genres. Called from the search page genre dropdown. Should be cached or pre-computed.

3. **`global_search()` scans all systems on every query**: Loads ROM lists for every system, filters, scores, and sorts. No caching of the combined index. With 40+ systems and thousands of ROMs, this is expensive on NFS.

4. **`RomCache` clones entire Vec<RomEntry> on cache hit**: The cache stores `Vec<RomEntry>` and `.clone()` on every read. For systems with thousands of ROMs, this is a non-trivial allocation. Consider `Arc<Vec<RomEntry>>` for shared ownership.

### Medium Priority

5. **Metadata DB opened lazily but held behind Mutex**: The `MetadataDb` is wrapped in `Arc<Mutex<Option<MetadataDb>>>`. Every metadata lookup acquires the mutex. Under concurrent requests, this serializes all metadata access. Consider a connection pool or per-request connections.

6. **Image import clones git repos one at a time**: For multi-repo systems like `arcade_dc` (3 repos), repos are cloned sequentially. Could parallelize repo cloning while remaining single-threaded for image copying.

7. **No pagination for favorites**: `get_favorites()` returns all favorites at once. For users with hundreds of favorites, this loads everything into memory.

### Low Priority

8. **CSS served via `include_str!()`**: The entire CSS file is compiled into the binary. Hot-reloading CSS during development requires a full recompile. (Mitigated by `dev.sh` auto-rebuild.)

9. **Skin CSS injected as inline `<style>`**: Works fine for 11 skins but would not scale to user-defined themes without a different approach.

---

## 8. Security Review

### Path Traversal

The media and captures handlers in `main.rs` check for `..` in paths:
```rust
if path.contains("..") {
    return StatusCode::BAD_REQUEST.into_response();
}
```

This is the **minimum viable check**. It blocks the most common traversal attack but does not handle:
- URL-encoded sequences (`%2e%2e`)
- Symlink following (a symlink inside the media dir could point outside it)
- Absolute paths (a path starting with `/` would be rejected by the route pattern anyway)

The URL-encoded case is actually safe because Axum's `Path` extractor decodes the URL before passing the string, so `%2e%2e` becomes `..` and is caught. Symlink following is a theoretical concern but low-risk since the media directory is app-controlled.

### ROM Operations

- **Delete**: `delete_rom()` takes a relative path and delegates to `core::roms::delete_rom()`, which joins it with the storage root. The core function does not validate that the path stays within the roms directory. A malicious relative path like `../../etc/passwd` would be caught by the filesystem layout (roms are under `<storage>/roms/`) but there is no explicit validation.
- **Rename**: `rename_rom()` takes a relative path and new filename. The core function renames within the same directory. No validation that the new name does not contain path separators.
- **Upload**: `upload.rs` uses multipart parsing and writes to `<storage>/roms/<system>/<filename>`. The filename comes from the multipart `filename` header. No sanitization of the filename.

### Recommendations

1. Add explicit path canonicalization and containment checks for ROM operations.
2. Sanitize uploaded filenames (strip path separators, null bytes, control characters).
3. Consider rate limiting for operations that trigger filesystem scans.

---

## 9. Error Handling

### Patterns Used

1. **`thiserror` enum in core**: `Error` enum with `Io`, `Config`, `NotFound`, `InvalidInput`, `Other` variants. Used consistently in core functions.

2. **`ServerFnError` in server functions**: Server functions return `Result<T, ServerFnError>`. Most convert core errors via `.map_err(|e| ServerFnError::new(e.to_string()))`.

3. **`ErrorBoundary` in pages**: All pages wrap their async content in `<ErrorBoundary>` + `<Suspense>` + `Suspend::new()`. The `ErrorDisplay` component renders caught errors.

### Issues

1. **Silent error swallowing**: Several places use `let _ = ...` to discard errors:
   - `let _ = db.clear()` in `regenerate_metadata()`
   - `let _ = std::fs::remove_file(&zip_path)` in download cleanup
   - `let _ = child.kill()` in thumbnail clone cancellation

   Some of these are intentional (cleanup best-effort), but `db.clear()` failure is significant.

2. **Error message quality**: Server function errors are wrapped with `.map_err(|e| ServerFnError::new(e.to_string()))`, which loses error type information. The client sees generic string messages.

3. **No structured error logging**: Errors are logged via `tracing::warn!` or `tracing::debug!` with ad-hoc formatting. No structured fields for filtering or alerting.

4. **Panic on lock poisoning**: All `RwLock`/`Mutex` accesses use `.expect("lock poisoned")`, which panics. In a web server, a panic in one request handler could crash the process.

---

## 10. Testing

### Current State

The core library has substantial unit tests:
- `rom_tags.rs`: 35+ tests covering regions, revisions, translations, hacks, classification
- `favorites.rs`: 20+ tests for CRUD, organization, flattening, deduplication
- `config.rs`: Tests for parsing, writing, comment preservation
- `recents.rs`: Tests for parsing, deduplication
- `video_url.rs`: Tests for all 4 platforms, edge cases
- `game_db.rs`: 20+ tests for lookup, CRC, normalized title, fuzzy matching
- `arcade_db.rs`: 15+ tests for lookup, clones, rotation, categories, total entry count
- `skins.rs`: Tests for CSS generation, color validation, name count matching
- `systems.rs`: Tests for lookup, fav filename parsing
- `util.rs`: Tests for `format_size_short()`
- `screenshots.rs`: Tests for filename matching, timestamp parsing

### Missing Tests

1. **No app-layer tests**: Zero tests in `replay-control-app`. No component tests, no server function tests, no integration tests.
2. **No tests for `api/mod.rs`**: `AppState`, `RomCache`, storage watcher, import orchestration — all untested.
3. **No tests for `thumbnails.rs`**: The 3-tier fuzzy matching, fake symlink resolution, and git clone logic have no tests.
4. **No tests for `metadata_db.rs`**: SQLite operations, migrations, nolock fallback — all untested.
5. **No tests for `launchbox.rs`**: XML parsing, title normalization, ROM index building — untested at unit level (though there is an integration path via metadata import).
6. **No tests for `storage.rs`**: Detection logic depends on filesystem state, making it hard to unit test, but the `df` parsing could be tested.
7. **No end-to-end tests**: No test harness that starts the server and makes HTTP requests.

### Test Quality

The existing tests are well-written: they test edge cases, use descriptive names, and cover both positive and negative paths. The game_db tests verify cross-region canonical sharing, which is a subtle correctness property.

---

## 11. Technical Debt

### Critical

1. **`server_fns.rs` monolith (1700+ lines)**: The largest file in the codebase. Contains all server function definitions, response types, and helper functions. Hard to navigate, review, and maintain. Every new feature adds to this file.

2. **Duplicated code patterns across pages/components**:
   - `RebootButton` component duplicated in `wifi.rs` and `nfs.rs`
   - Genre dropdown duplicated in `rom_list.rs` and `search.rs`
   - Filter chips UI duplicated in `rom_list.rs` and `search.rs`
   - Debounce logic duplicated (different implementations in rom_list.rs and search.rs)
   - `update_url_params` helper duplicated in rom_list.rs and search.rs

3. **REST API + Server Functions redundancy**: The REST API (`api/` module) and server functions (`server_fns.rs`) provide overlapping functionality. Both paths go through `AppState` to core. The REST API was presumably the original interface; server functions were added for Leptos SSR. Maintaining both doubles the surface area.

### Significant

4. **`api/mod.rs` complexity (500+ lines)**: `AppState` has 8 fields (all `Arc<RwLock<...>>` or `Arc<Mutex<...>>`). Import orchestration, storage watching, and config management are all in this file alongside the state definition.

5. **CSS in single file (2237 lines)**: No modular organization. Finding/modifying styles for a specific component requires searching through the entire file.

6. **Mirror types maintenance burden**: Every struct shared between server and client must be defined twice (`types.rs` for WASM, source struct in `server_fns.rs`). Adding or modifying a field requires updating both.

7. **Manual server function registration in `main.rs`**: Every new server function requires adding a `register_explicit::<T>()` call. Forgetting this causes silent failures (the function appears to exist but returns errors at runtime).

### Minor

8. **i18n as match arms**: The `t()` function is a 200+ arm match. Adding translations requires editing a single function. No compile-time exhaustiveness checking. No tooling for missing translations.

9. **No pagination for favorites/recents**: These endpoints return all data at once. Fine for current usage but will not scale.

10. **`game_detail.rs` size (1196 lines)**: The video section alone could be its own module.

---

## 12. Proposed Changes

### 12.1 Split `server_fns.rs` into modules

**What**: Refactor the monolithic `server_fns.rs` into `src/server_fns/mod.rs` with submodules: `types.rs`, `system.rs`, `roms.rs`, `favorites.rs`, `search.rs`, `config.rs`, `metadata.rs`, `images.rs`, `skins.rs`, `videos.rs`, `system_ops.rs`.

**Why**: The file is 1700+ lines with 50+ functions. It is the primary bottleneck for code navigation and review. Every feature change touches this file, increasing merge conflict risk.

**Effort**: Medium. The functions have few interdependencies. `resolve_game_info()` and `enrich_from_metadata_cache()` are shared helpers that would go in a `helpers.rs` submodule.

**Priority**: **High**. This is the single highest-impact refactor for developer velocity.

---

### 12.2 Extract shared components to eliminate duplication

**What**: Create shared components for:
- `RebootButton` (currently duplicated in wifi.rs and nfs.rs)
- `GenreDropdown` (duplicated in rom_list.rs and search.rs)
- `FilterChips` (duplicated in rom_list.rs and search.rs)
- `DebouncedInput` (duplicated debounce logic in rom_list.rs and search.rs)
- `update_url_params` utility (duplicated in rom_list.rs and search.rs)

**Why**: DRY violation. Bug fixes or UX changes must be applied in multiple places. Risk of divergence.

**Effort**: Low-Medium. Each extraction is straightforward. `RebootButton` is the simplest (copy one version to `components/`). `GenreDropdown` requires parameterizing the data source.

**Priority**: **High**. Low effort, immediate payoff.

---

### 12.3 Cache `find_image_on_disk()` results

**What**: Build an image path index (HashMap of `(system, rom_filename) -> (boxart_path, snap_path)`) once per cache TTL, instead of scanning the media directory per ROM.

**Why**: Current implementation does a directory listing per ROM. For 1000 ROMs, that is 1000 readdir syscalls, magnified on NFS.

**Effort**: Low. Add an image path cache to `RomCache`, populate on first access, invalidate on image import completion.

**Priority**: **High**. Direct performance impact on ROM listing and game detail pages.

---

### 12.4 Cache global search index

**What**: Build a combined search index (all systems, all ROMs with metadata) cached in `RomCache` with TTL. Use it for `global_search()`, `get_all_genres()`, and `random_game()`.

**Why**: These functions currently scan all systems on every call. With 40+ systems and NFS latency, this is the slowest server function path.

**Effort**: Medium. Requires defining a combined index structure and invalidation strategy.

**Priority**: **Medium-High**. Significantly improves search page responsiveness.

---

### 12.5 Use `Arc<Vec<RomEntry>>` in `RomCache`

**What**: Change `RomCache` to store `Arc<Vec<RomEntry>>` instead of `Vec<RomEntry>`. Return `Arc<Vec<RomEntry>>` from `get_roms()` instead of cloning.

**Why**: Currently every cache hit clones the entire Vec. For systems with thousands of ROMs, this is unnecessary allocation.

**Effort**: Low. Change the cache entry type and update callsites to work with `Arc<Vec<>>`.

**Priority**: **Medium**. Reduces allocations but may not be user-visible on most systems.

---

### 12.6 Split CSS into per-component files

**What**: Split `style.css` into modular files (e.g., `base.css`, `layout.css`, `home.css`, `game-detail.css`, `rom-list.css`, `search.css`, `settings.css`, `video.css`, `lightbox.css`) and concatenate them at build time.

**Why**: 2237 lines in one file is difficult to navigate. Finding styles for a specific component requires searching.

**Effort**: Medium. Requires a build step to concatenate CSS files (or just use `@import` with a simple concatenation script).

**Priority**: **Medium**. Improves developer experience but does not affect functionality.

---

### 12.7 Extract `GameVideoSection` into its own module

**What**: Move `GameVideoSection`, `VideoEmbed`, `VideoRecommendations`, and `RecommendationItem` from `game_detail.rs` into `src/components/game_videos.rs` (or `src/pages/game_detail/videos.rs`).

**Why**: These 4 components total ~500 lines and are self-contained. Extracting them would reduce `game_detail.rs` from 1196 to ~700 lines.

**Effort**: Low. The components are already self-contained with clear prop boundaries.

**Priority**: **Medium**. Reduces the largest page file to a manageable size.

---

### 12.8 Add path sanitization for ROM operations

**What**: Add explicit path validation in `delete_rom()`, `rename_rom()`, and the upload handler: reject filenames containing `/`, `\`, null bytes, or `..` sequences. Canonicalize paths and verify they stay within the roms directory.

**Why**: Current code relies on the filesystem layout for safety. Explicit validation is defense-in-depth against path traversal.

**Effort**: Low. Add a `sanitize_filename()` function and call it in the three entry points.

**Priority**: **Medium**. Security improvement with minimal effort.

---

### 12.9 Replace `Arc<Mutex<Option<MetadataDb>>>` with per-request connections

**What**: Instead of holding a single `MetadataDb` behind a Mutex, open a new SQLite connection per request (or use a connection pool like `r2d2`).

**Why**: The current Mutex serializes all metadata access. Under concurrent load (e.g., SSR rendering multiple pages), this is a bottleneck. SQLite supports multiple concurrent readers with WAL mode.

**Effort**: Medium. Requires changing `AppState` and all callsites that use `metadata_db()`.

**Priority**: **Low-Medium**. Only matters under concurrent load, which is rare for a single-user companion app.

---

### 12.10 Add integration tests for server functions

**What**: Create an integration test module that:
1. Sets up a temporary storage directory with test ROM files
2. Initializes `AppState`
3. Calls server functions directly (bypassing HTTP)
4. Verifies correct behavior

**Why**: Server functions are the primary interface between the UI and core logic. They are currently untested. Bugs in data resolution, caching, or error handling are caught only manually.

**Effort**: Medium-High. Requires test fixtures (mock ROM directory, test Metadata.xml) and test infrastructure.

**Priority**: **Low-Medium**. Valuable for long-term maintainability but not blocking current development.

---

### 12.11 Consider removing the REST API

**What**: Evaluate whether the REST API (`api/` module: system_info, roms, favorites, upload, recents) is still needed. If not used externally, remove it to reduce maintenance burden.

**Why**: The REST API and server functions are redundant. Both go through `AppState` to core. Server functions are used by the Leptos UI; the REST API was the original interface. If nothing external uses the REST API, it is dead code.

**Effort**: Low (if removing) or None (if keeping for external use).

**Priority**: **Low**. Requires checking if any external tools (scripts, automation, RePlayOS itself) use the REST endpoints.

---

### 12.12 Add `thumbnails.rs` and `metadata_db.rs` tests

**What**: Add unit tests for:
- `strip_tags()`, `strip_version()`, `thumbnail_filename()`, `find_thumbnail()` in thumbnails.rs
- `MetadataDb::lookup()`, `upsert()`, `bulk_upsert()`, `stats()`, `image_stats()` in metadata_db.rs
- `normalize_title()` in launchbox.rs (especially article reordering edge cases)

**Why**: These modules handle complex matching logic (fuzzy thumbnail matching, title normalization) that is prone to subtle bugs. The matching logic has already been iterated on (3-tier fallback, colon variants) which suggests edge cases have been found empirically.

**Effort**: Low-Medium. Thumbnail matching tests need a test directory with sample PNG files. Metadata DB tests need an in-memory SQLite database.

**Priority**: **Low-Medium**. Prevents regressions in matching accuracy.

---

### Summary Table

| # | Change | Effort | Priority |
|---|--------|--------|----------|
| 12.1 | Split server_fns.rs | Medium | High |
| 12.2 | Extract shared components | Low-Medium | High |
| 12.3 | Cache image paths | Low | High |
| 12.4 | Cache search index | Medium | Medium-High |
| 12.5 | Arc\<Vec\> in RomCache | Low | Medium |
| 12.6 | Split CSS | Medium | Medium |
| 12.7 | Extract video components | Low | Medium |
| 12.8 | Path sanitization | Low | Medium |
| 12.9 | MetadataDb connection pool | Medium | Low-Medium |
| 12.10 | Server function integration tests | Medium-High | Low-Medium |
| 12.11 | Evaluate REST API removal | Low | Low |
| 12.12 | Add matching logic tests | Low-Medium | Low-Medium |
