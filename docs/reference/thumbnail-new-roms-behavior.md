# Behavior When New ROMs Are Added Externally

This document describes what happens when a user adds new ROMs via SCP, NFS copy,
or any other external mechanism (i.e., outside of the app's upload feature).

## Summary

New ROMs will eventually appear in the game list without user action, but
**metadata (descriptions, ratings) and thumbnail images require either an
explicit user action or a page reload**, depending on which subsystem is
involved. The app does not use inotify or any filesystem watcher for ROM
directories.

## Detailed Behavior by Subsystem

### 1. Game List (ROM Scanning)

**Automatic, with a delay of up to 5 minutes.**

The ROM cache uses a two-tier invalidation strategy:

- **Directory mtime check**: A single `stat()` call on the system directory.
  Adding a file to a directory updates its mtime, which immediately invalidates
  the in-memory (L1) and SQLite (L2) caches for that system.
- **Hard TTL fallback**: Even if mtime cannot be read (e.g., NFS flake), the
  cache expires after 300 seconds (`CACHE_HARD_TTL`).

When the user navigates to a system page after the cache is stale, the app
performs an L3 filesystem scan, discovers the new ROMs, and writes the results
back to L1 and L2. The new ROMs appear in the list.

**At startup**, `spawn_cache_verification()` runs a background task that checks
all cached systems' mtimes against the filesystem and re-scans any stale ones.
This means that if the server was already running when new ROMs were added, the
new ROMs will be detected on the next page load (once the 5-minute TTL or mtime
change triggers a re-scan). If the server is restarted, the background
verification catches stale systems within a few seconds of startup.

There is **no periodic background rescan** of ROM directories while the server
is running. The 60-second poll in `spawn_storage_watcher` only watches
`replay.cfg` for storage-level changes (e.g., switching from USB to NFS), not
individual ROM file additions.

**What the user sees**: The new ROMs show up in the game list the next time they
visit the system page, as long as enough time has passed for the cache to
expire. In practice, the mtime change means the cache is invalidated
immediately, and the next page load triggers a fresh scan. The maximum wait is
5 minutes (hard TTL) if mtime detection fails.

### 2. Metadata (LaunchBox Descriptions, Ratings, Publishers)

**Not automatic. Requires explicit user action.**

The metadata import (`import_launchbox`) builds a ROM index from the filesystem
at import time and matches LaunchBox XML entries against that snapshot. New ROMs
added after the import will not have metadata entries in the SQLite
`game_metadata` table.

Specifically:
- `build_rom_index()` scans all ROM directories and builds a `(system, normalized_title) -> [filenames]` map.
- `import_launchbox()` streams the XML and matches entries against this map.
- The match is done at import time only; there is no lazy matching or deferred lookup.

To get metadata for new ROMs, the user must go to the Metadata page and either:
- Click "Regenerate" (clears the DB and re-imports from `launchbox-metadata.xml`)
- Click "Download & Import" (re-downloads the XML from the internet and re-imports)

**Auto-import on startup**: `spawn_auto_import()` checks if `launchbox-metadata.xml`
exists and the DB is empty, then triggers an import. This only runs when the DB
is empty (fresh install or after a clear), not when new ROMs are added.

**What the user sees**: New ROMs appear in the list without descriptions, ratings,
or publisher info until the user manually triggers a metadata re-import.

### 3. Thumbnail Images (Box Art)

**Partially automatic via on-demand download; otherwise requires explicit action.**

There are two paths for thumbnail resolution:

#### Path A: Bulk Download (explicit action)

The "Update Images" button on the Metadata page runs a two-phase pipeline:
1. **Index phase**: Fetches file listings from all libretro-thumbnails GitHub repos
   and stores them in the `thumbnail_index` SQLite table.
2. **Download phase**: For each system with ROMs, scans the filesystem for ROM
   filenames, looks them up in the manifest fuzzy index, and downloads missing
   images from `raw.githubusercontent.com`.

This uses `list_rom_filenames()` which scans the filesystem live, so it will
find new ROMs. However, the user must explicitly trigger this operation.

#### Path B: On-Demand Download (automatic, requires manifest index)

When a user views a system page, `resolve_box_art()` is called for each visible
ROM. If no local image exists but the `thumbnail_index` has a match, the app
queues a background download via `queue_on_demand_download()`:

1. The download runs in a `tokio::task::spawn_blocking` thread.
2. On success, the image is saved to disk and the system's image cache is
   invalidated (`invalidate_system_images`).
3. The `pending_downloads` set prevents duplicate downloads for the same image.
4. The image appears on the **next** page load (not the current one, since the
   download is asynchronous).

**Critical prerequisite**: On-demand download only works if the thumbnail
manifest index has already been populated (i.e., the user has run "Update
Images" at least once). If the `thumbnail_index` table is empty, the manifest
field in `ImageIndex` will be `None` and no on-demand downloads are attempted.

**What the user sees**:
- If the thumbnail index exists (user has run "Update Images" before): New ROMs
  get box art automatically after two page loads -- the first load triggers the
  background download, the second load displays the downloaded image. This works
  because the manifest index is built from the libretro-thumbnails repos (not
  from the local ROM list), so it already contains entries for games the user
  hasn't added yet.
- If the thumbnail index does not exist: No images appear until the user runs
  "Update Images".

### 4. Image Index Cache

The per-system `ImageIndex` uses the same mtime + hard TTL invalidation as ROM
caches, but it watches the `boxart/` directory rather than the ROM directory.
When an on-demand download saves a new image, the code explicitly calls
`invalidate_system_images()`, so the next request rebuilds the index and picks
up the new file.

### 5. Baked-In Game Database (game_db, arcade_db)

**Automatic, no delay.**

The compile-time baked-in game databases (`game_db` for consoles, `arcade_db`
for arcade) provide display names, genres, player counts, and other basic
metadata. These are looked up by ROM filename on every request and require no
import step. If the new ROM's filename matches an entry in the baked-in database,
the display name, genre, year, developer, and player count are available
immediately.

## End-to-End Scenario

User adds `Super Mario World (USA).sfc` to `/roms/nintendo_snes/` via SCP:

| Feature              | Available? | When?                        | User Action Needed? |
|----------------------|-----------|------------------------------|---------------------|
| Appears in game list | Yes       | Next page load (mtime)       | No                  |
| Display name         | Yes       | Immediately (baked-in DB)    | No                  |
| Genre, year, players | Yes       | Immediately (baked-in DB)    | No                  |
| Box art              | Depends   | 2nd page load (on-demand)    | Only if index empty |
| Description, rating  | No        | After re-import              | Yes (Regenerate)    |
| Screenshot (snap)    | Depends   | After "Update Images"        | Yes                 |

## Recommendations for Improving the Experience

1. **Periodic background ROM rescan**: Currently the cache only invalidates when
   a user visits a page. A periodic background task (e.g., every 60 seconds)
   could re-scan systems whose directory mtime has changed, keeping the L2 cache
   warm and ensuring the home page game counts are always current.

2. **Incremental metadata matching**: When new ROMs are detected during a cache
   rescan, attempt to match them against existing `game_metadata` entries in the
   DB. The LaunchBox import stores metadata keyed by `(system, rom_filename)`,
   but many entries could be pre-matched by normalized title, avoiding a full
   re-import.

3. **Automatic thumbnail download for new ROMs**: When the ROM cache detects new
   files (mtime change -> L3 rescan), diff the new ROM list against the previous
   one and trigger on-demand downloads for the additions. This would make box art
   appear without requiring the user to visit the system page.

4. **Startup enrichment for uncached ROMs**: The `spawn_cache_verification` task
   already re-scans stale systems and calls `enrich_system_cache`. This path
   naturally handles new ROMs added while the server was stopped. No change
   needed here -- it already works correctly.

## Source Files

- Cache invalidation logic: `replay-control-app/src/api/cache.rs`
- Background startup tasks: `replay-control-app/src/api/background.rs`
- ROM scanning: `replay-control-core/src/roms.rs`
- Thumbnail matching: `replay-control-core/src/thumbnails.rs`
- Manifest index and download: `replay-control-core/src/thumbnail_manifest.rs`
- On-demand download queue: `replay-control-app/src/api/cache.rs` (`queue_on_demand_download`)
- Metadata import: `replay-control-app/src/api/import.rs`
- LaunchBox XML parser: `replay-control-core/src/launchbox.rs`
- Server function (ROM list): `replay-control-app/src/server_fns/roms.rs`
- Config file watcher: `replay-control-app/src/api/background.rs` (`spawn_storage_watcher`)
