# Enrichment Pipeline

Pure data logic in `replay-control-core-server/src/library/enrichment.rs` plus image resolution in `replay-control-core-server/src/library/thumbnails/resolution.rs`.
App orchestration in `replay-control-app/src/api/library/enrichment.rs`.

## Purpose

Enrichment populates derived fields in `game_library` that the ROM scan doesn't set: `box_art_url`, `genre`, `players`, `rating`, `rating_count`, `developer`, `release_year`, `cooperative`. It also populates `game_detail_metadata` (description + publisher) and `library_game_resource` (manual/video suggestions) so the game-detail page can serve rich metadata from a single library-pool read.

## Architecture: Core-server / App Split

### Core-server (`replay_control_core_server::enrichment` + `image_resolution`)

Pure data + native I/O — no web server state, no broadcast channels:

- **`enrich_system()`** — takes `&Connection` (library), `system`, `&ImageIndex`, `&ArcadeInfoLookup`, pre-loaded provider metadata/resources, and pre-loaded catalog resources. Returns `EnrichmentResult` with all updates (developer, cooperative, year, description rows, resource rows, box-art/genre/players/rating, on-demand manifest downloads).
- **`build_image_index()`** — pure filesystem walk + libretro manifest fold. Takes pre-loaded `Vec<(url_name, branch, Vec<ThumbnailManifestEntry>)>`; doesn't acquire any pool.
- **`format_box_art_url()`** — converts a relative path to a URL path (`/media/{system}/...`).

### App (`replay-control-app/src/api/library/`)

Thin orchestration wrapper that handles AppState, pool access, and side effects:

- **`enrichment.rs`** — `enrich_system_library()` parallel-loads provider rows/resources, catalog manual resources, libretro repo data, and arcade info via `tokio::join!`, then calls core-server's `enrich_system()` inside one library-pool read. All writes (developer, cooperative, year, release-date resolver, `game_detail_metadata`, `library_game_resource`, box art / genre / rating) happen inside a **single** library-pool write closure so a Pi's per-commit fsync only fires once per system.

## When It Runs

Enrichment runs per-system in these contexts:

1. **Startup** (Phase 2) and **post-rebuild/rescan**: `populate_all_systems()` runs `scan_and_reconcile_system()` followed immediately by `enrich_system_library()` for the same system before moving to the next — inline, not a fleet-wide second pass.
2. **Post-import**: `reenrich_all_systems()` iterates every stored system in the library so newly-imported provider data flows into `game_library`, `game_detail_metadata`, and `library_game_resource`. (Enrichment-only — populate is not re-run.)
3. **ROM watcher**: when inotify detects new files in a system directory (debounced, then strict scan + enrich).

## Flow: `enrich_system_library()` (app side)

For each system:

1. **Load visible filenames** — single `library_pool.read`. Used by all subsequent steps.
2. **Parallel setup** (`tokio::join!`):
   - `build_image_index` — also pre-loads libretro repo data via `external_metadata_pool.read` and walks `<storage>/.replay-control/media/<system>/boxart/`.
   - `load_provider_rows` — `external_metadata_pool.read` of LaunchBox provider rows and alternates, keyed by normalized title.
   - `load_provider_resources` — `external_metadata_pool.read` of provider resource links such as LaunchBox videos.
   - `load_catalog_resources` — `catalog_pool.read` of bundled manual links for the system and the current library's normalized titles.
   - `ArcadeInfoLookup::build` — catalog batch lookup keyed by ROM filename stem.
3. **`enrich_system()` core call** inside `library_pool.read` — pure data merge producing `EnrichmentResult`.
4. **Queue on-demand manifest downloads** — for ROMs with no local art but a manifest match.
5. **Single transactional write** to `library.db`:
   - `update_developers` (gap-fill from launchbox)
   - `update_cooperative` (OR-merge)
   - `update_release_years` (gap-fill)
   - `resolve_release_date_for_library` (re-runs the release-date resolver from catalog seed)
   - `replace_detail_metadata_and_resources_for_system` (truncate + repopulate `game_detail_metadata` and `library_game_resource`)
   - `update_box_art_genre_rating` (box art URL + LaunchBox-derived gap-fills)

## ROM filename → normalized title resolution

`provider_game` is keyed by `(system, normalized_title, provider)` because the host-global DB doesn't know which ROM filenames each storage has. Per-ROM lookup goes through a small helper in `library/enrichment.rs`:

- **Console**: `normalize_title_for_metadata(filename_stem(rom_filename))`.
- **Arcade**: `normalize_title_for_metadata(arcade_lookup.display_name)`. For clones, also try the parent's display name (mirrors the original import-time index logic).

Each ROM may map to one or two normalized-title candidates; `match_launchbox_rows` returns the first LaunchBox provider hit.

## Merge semantics

**Field-level fill-empty.** Scan-time sources (`arcade_db`, `canonical_game`, TOSEC tag parsing) populate first; LaunchBox enrichment fills the gaps. Already-set values are preserved — re-enrichment never overwrites a non-empty `developer` / `genre` / `players` / `release_year` / `cooperative=1`.

`description` and `publisher` are not gap-filled — they're rebuilt every pass from `provider_game.description` / `publisher` into `game_detail_metadata`. Resource suggestions are also rebuilt every pass into `library_game_resource`, so removed ROMs lose stale suggestions through the same truncate-and-replace system path.

## Image Index

`ImageIndex` (in `image_resolution.rs`) wraps:

- **`DirIndex`** — `<storage>/.replay-control/media/<system>/boxart/` directory walk with four matching tiers (exact stem, case-insensitive, fuzzy `base_title`, version-stripped) plus an aggressive-normalization tier and an aggressive-compact tier (last-resort for spaces-vs-no-spaces mismatches like `"Galaga '88"` ↔ `"Galaga88"`).
- **`db_paths`** — user box-art overrides from `user_data.db.box_art_overrides` (highest priority).
- **`manifest`** — `ManifestFuzzyIndex` built from pre-loaded libretro repo data, used for on-demand image downloads when no local image exists.

Built fresh per enrichment pass; not cached across requests.

## Box Art Resolution at Request Time

`box_art_url` in `game_library` stores a URL path like `/media/snes/boxart/Name.png`. This is a pre-resolved path set during enrichment — there is no filesystem lookup at request time.

If the enrichment pipeline finds no local image but the thumbnail manifest (from libretro) has a match, it queues a background download via `queue_on_demand_download()`. The download runs via the `ThumbnailDownloadOrchestrator`, saves the image to disk, updates `box_art_url` in the DB, and invalidates the response cache. The art appears on the next page load.

## Data Sources

Enrichment draws from these sources:

1. **Bundled `catalog.sqlite` — game_db** (`replay-control-core-server::game_db`): No-Intro / TheGamesDB derived. Provides genre, players, and rating for known console ROMs. Applied at scan time.
2. **External metadata `external_metadata.db` — provider tables** (`replay_control_core_server::external_metadata::system_provider_game_rows`, `system_provider_alternates`, `system_provider_resources`): host-global tables populated from `launchbox-metadata.xml`. Provides description, genre, developer, publisher, rating, rating_count, players, release_date, release_precision, cooperative, and provider resource links such as videos. Applied during enrichment via per-system batched HashMaps.
3. **Bundled `catalog.sqlite` — arcade_db** (`replay-control-core-server::arcade_db`): MAME / FBNeo / Flycast derived, with one row per upstream source. The runtime merges fields per arcade system's source priority (see [Database Schema](database-schema.md#per-system-arcade-merge)) so `arcade_fbneo` shows FBNeo's curated names and `arcade_mame` shows MAME's, with field-level fallback when the primary source lacks data. Developer/manufacturer is applied at scan time, with display name and box art picked from the per-system merged result.
4. **Bundled `catalog.sqlite` — catalog_game_resource**: MiSTer Manual Downloader and Retrokit manual links, plus Shmups Wiki strategy guide and video-index links. Matching resources are copied into `library_game_resource` during enrichment; saving a manual downloads the file into user-owned storage.
