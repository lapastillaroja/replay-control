# Game Library

How the game library works: ROM scanning, caching, and the three-tier architecture.

## Architecture

The game library uses a three-tier cache to balance performance with freshness:

```
L1 (In-Memory)          L2 (SQLite)              L3 (Filesystem)
RwLock<HashMap>     game_library table          roms/ directory tree
  ~0ns lookup         ~1ms lookup                ~100ms full scan
  CacheEntry<Vec>     GameEntry rows             list_roms() + dedup
  TTL: 300s           mtime-based invalidation   source of truth
```

### L1: In-Memory Cache

`GameLibrary.roms` is a `RwLock<HashMap<String, CacheEntry<Vec<RomEntry>>>>` keyed by system folder name. Each `CacheEntry` stores the data, directory mtime at cache time, and a hard TTL (300 seconds).

On access via `get_roms()`, if the entry exists and the directory mtime matches and the TTL has not expired, the cached data is returned directly.

### L2: SQLite Persistent Cache

The `game_library` table in `metadata.db` stores one row per ROM with fields: `system`, `rom_filename`, `rom_path`, `display_name`, `size_bytes`, `is_m3u`, `box_art_url`, `driver_status`, `genre`, `genre_group`, `players`, `rating`, `rating_count`, `base_title`, `series_key`, `region`, `developer`, `is_clone`, `is_translation`, `is_hack`, `is_special`, `crc32`, `hash_mtime`, `hash_matched_name`.

The `game_library_meta` table tracks per-system metadata: `system`, `rom_count`, `total_size`, `dir_mtime_secs`.

On L1 miss, `load_roms_from_db()` checks the stored mtime against the current directory mtime. Match = serve from L2. Mismatch = fall through to L3.

### L3: Filesystem Scan

`list_roms()` in `replay-control-core/src/roms.rs` recursively walks the system directory, collects ROM files, applies M3U deduplication, and returns `Vec<RomEntry>`. Results are written through to both L2 and L1 via `save_roms_to_db()`.

## ROM Scanning

`collect_roms_recursive()` walks system directories collecting files that match the system's extension list. M3U files are always accepted regardless of extensions.

After collection, `apply_m3u_dedup()` parses M3U playlists, hides referenced disc files, and aggregates their sizes into the M3U entry.

## Display Name Resolution

- **Arcade systems**: `arcade_db::arcade_display_name(filename)` looks up by zip name
- **Non-arcade**: `game_db::game_display_name(system, filename)` with fallback to tag-stripped filename + `rom_tags::display_name_with_tags()` for region/revision suffixes

## Enrichment

`enrich_system_cache()` runs after cache population and after metadata imports. It:

1. Resolves box art URLs via the 5-tier resolution pipeline (see `docs/features/thumbnails.md`)
2. Loads ratings and rating counts from `game_metadata` (LaunchBox)
3. Fills empty genres from LaunchBox data
4. Fills empty developer from LaunchBox data
5. Auto-matches new ROMs to existing metadata by normalized title
6. Populates series data from Wikidata (see [Game Series](game-series.md))

## Unified GameListItem

The `GameListItem` component provides a consistent game rendering across all list views: system ROM lists, search results, developer pages, series siblings, and recommendation blocks. Props include system, display name, box art, genre/rating badges, favorite toggle, driver status badge, and an optional system badge for cross-system lists. This ensures a uniform look-and-feel across the app.

## Startup Pipeline

Server startup follows a sequenced pipeline to avoid race conditions between tasks that share the database:

1. **Auto-import**: run any pending metadata imports
2. **Populate**: scan all system directories and populate the game library cache
3. **Enrich**: resolve box art, ratings, developer, genre, and series data
4. **Watchers**: start filesystem and config watchers

The server responds immediately during warmup with a "Scanning game library..." banner, serving empty data until population completes (non-blocking startup).

## Filesystem Watching

On local storage (SD/USB/NVMe), a `notify` (inotify) watcher monitors the `roms/` directory with `RecursiveMode::Recursive`. Events are debounced (3 seconds) and trigger targeted cache invalidation + re-enrichment for affected systems.

On NFS storage, no watcher is set up (inotify does not detect remote changes). The user triggers updates manually from the metadata page.

## Cache Invalidation

- `invalidate()` clears all L1 and L2 data
- `invalidate_system(system)` clears one system from L1 and L2
- `invalidate_favorites()` and `invalidate_recents()` clear their respective caches
- Directory mtime changes trigger automatic L3 rescan on next access

## Key Source Files

| File | Role |
|------|------|
| `replay-control-app/src/api/cache.rs` | GameLibrary, get_roms, enrich_system_cache, resolve_box_art |
| `replay-control-app/src/api/cache/enrichment.rs` | Enrichment pipeline (ratings, developer, genre, series) |
| `replay-control-core/src/roms.rs` | ROM scanning, M3U dedup, collect_roms_recursive |
| `replay-control-core/src/metadata/metadata_db/game_library.rs` | game_library/game_library_meta tables, GameEntry |
| `replay-control-app/src/api/background.rs` | Startup pipeline, filesystem watcher, auto-import |
| `replay-control-app/src/components/game_list_item.rs` | Unified GameListItem component |
| `replay-control-core/src/platform/storage.rs` | StorageLocation, StorageKind |
