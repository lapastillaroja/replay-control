# Enrichment Pipeline

Pure data logic in `replay-control-core-server/src/library/enrichment.rs` plus image resolution in `replay-control-core-server/src/library/thumbnails/resolution.rs`.
App orchestration in `replay-control-app/src/api/library/enrichment.rs`.

## Architecture: Core-server / App Split

### Core-server (`replay_control_core_server::enrichment` + `image_resolution`)

Pure data + native I/O — no web server state, no broadcast channels:

- **`build_image_index()`** — takes `&Connection`, system, storage_root, user_overrides → `ImageIndex`
- **`resolve_box_art()`** — takes `&ImageIndex`, system, rom_filename → `BoxArtResult` (Found / ManifestHit / NotFound)
- **`enrich_system()`** — takes `&Connection`, system, `&ImageIndex`, auto_matched_ratings → `EnrichmentResult`
- **`format_box_art_url()`** — converts a relative path to a URL path (`/media/{system}/...`)

### App (`replay-control-app/src/api/library/`)

Thin orchestration wrapper that handles AppState, pool access, and side effects:

- **`enrichment.rs`** — `enrich_system_cache()` coordinates the pipeline: fetches user overrides from `user_data_pool`, builds the image index via core-server, runs auto-matching, calls core-server's `enrich_system()` inside a DB read, then writes results and queues manifest downloads via the on-demand download path.

## Purpose

Enrichment populates derived fields in `game_library` that the ROM scan doesn't set: `box_art_url`, `genre`, `players`, `rating`, `rating_count`, `developer`, and `release_year`. These fields power the UI's box art display, genre filtering, and recommendation engine.

## When It Runs

Enrichment runs per-system in these contexts:

1. **Startup** (Phase 2): after scanning or re-scanning a system via `populate_all_systems()` or `phase_cache_verification()`
2. **Post-import**: after LaunchBox metadata import completes (`spawn_cache_enrichment`)
3. **Post-rebuild**: after "Rebuild Game Library" user action (`spawn_rebuild_enrichment`)
4. **ROM watcher**: when inotify detects new files in a system directory (debounced, then scan + enrich)

## Flow: `enrich_system_cache()`

For each system:

1. **Build image index** (`build_image_index`) -- temporary, not cached across requests
2. **Auto-match new ROMs** -- ROMs added after the last import get matched to existing `game_metadata` entries by normalized title (delegated to `replay_control_core::metadata_matching`)
3. **Run core enrichment** (`enrich_system`) -- a single synchronous DB read that:
   - Loads LaunchBox metadata (ratings, genres, players, rating_counts, developers, release_years)
   - Loads existing game_library values (to avoid overwriting scan-time values)
   - Merges auto-matched ratings
   - Reads visible ROM filenames
   - Resolves box art for each ROM via the image index
   - Returns `EnrichmentResult` with all updates
4. **Queue manifest downloads** -- for ROMs with no local art but a manifest match
5. **Write updates** -- separate bulk writes for developer, release_year, and box_art/genre/rating

Fields are only filled when the game_library row doesn't already have a value. Scan-time values (e.g., arcade developer from `arcade_db`, genre from embedded `game_db`) are preserved.

## Image Index (DirIndex)

Defined in `replay-control-core-server/src/metadata/image_matching.rs`. Built from a single `readdir` scan of `<storage>/.replay-control/media/<system>/boxart/`.

Four matching tiers, tried in order:

| Tier | Key | Example |
|------|-----|---------|
| Exact | thumbnail_filename stem | `"Super Mario World (USA)"` |
| Case-insensitive | lowercase stem | `"super mario world (usa)"` |
| Fuzzy (base_title) | tags stripped | `"Super Mario World"` |
| Version-stripped | trailing version removed | `"Super Mario"` (from `"Super Mario v1.1"`) |

Additional tiers in `find_best_match()`:
- DB path lookup (from `game_metadata.box_art_path`, highest priority after user overrides)
- Colon variants for arcade (`: ` -> ` - ` and `: ` -> ` `)
- Tilde dual-title split (for `Name A ~ Name B` titles, try each half)

The `ImageIndex` struct in `enrichment.rs` (core) extends `DirIndex` with:
- `db_paths`: ROM filename -> box_art_path from the `game_metadata` table
- `manifest`: `ManifestFuzzyIndex` for on-demand downloads of images not yet on disk
- User box art overrides from `user_data.db` (highest priority, merged into `db_paths`)

## Box Art Resolution at Request Time

`box_art_url` in `game_library` stores a URL path like `/media/snes/boxart/Name.png`. This is a pre-resolved path set during enrichment -- there is no filesystem lookup at request time.

If the enrichment pipeline finds no local image but the thumbnail manifest (from GitHub API) has a match, it queues a background download via `queue_on_demand_download()`. The download runs in a `std::thread::spawn`, saves the image to disk, updates `box_art_url` in the DB, and invalidates the response cache. The art appears on the next page load.

## Data Sources

Enrichment draws from three sources:

1. **Bundled `catalog.sqlite` — game_db** (`replay-control-core-server::game_db`): No-Intro / TheGamesDB derived. Provides genre, players, and rating for known console ROMs. Applied at scan time.
2. **LaunchBox metadata** (`game_metadata` table): imported from XML. Provides description, genre, developer, publisher, rating, rating_count, players, release_year. Applied during enrichment.
3. **Bundled `catalog.sqlite` — arcade_db** (`replay-control-core-server::arcade_db`): MAME / FBNeo / Flycast derived, with one row per upstream source. The runtime merges fields per arcade system's source priority (see [Database Schema](database-schema.md#per-system-arcade-merge)) so `arcade_fbneo` shows FBNeo's curated names and `arcade_mame` shows MAME's, with field-level fallback when the primary source lacks data. Developer/manufacturer is applied at scan time, with display name and box art picked from the per-system merged result.

## Auto-Matching

When new ROMs are added (e.g., via upload or USB copy) after a LaunchBox import, `auto_match_metadata()` matches them against existing `game_metadata` entries using normalized title matching (delegated to `replay_control_core::metadata_matching`). Matched metadata is persisted to `game_metadata` so future enrichment runs hit directly.
