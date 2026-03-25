# Known Issues & TODOs

## ~~Tokio Worker Starvation on Game Detail~~ (RESOLVED)

**Fixed in `cf96bf5`, `6f9df97`, `c5d6797` (2026-03-24).** Opening game detail pages
for large systems (e.g., Amstrad CPC with 4213 ROMs) caused a permanent app hang.
The `DbPool` synchronous API (`block_in_place` + `block_on`) pinned tokio worker
threads while waiting for connections, and 10+ SSR resources competing for 1 DELETE
mode connection exhausted all workers. Fixed by switching to the proper async API
(`pool.get().await` + `conn.interact().await`), increasing DELETE mode readers from
1 to 3, adding a 10-second pool timeout, and replacing full-system L2 loads with
single-row primary key lookups for game detail. See
`research/investigations/amstrad-cpc-hang.md` for the full analysis.

## ~~ROM Rename Side Effects~~ (RESOLVED)

**Fixed in `445abc9` (2026-03-23).** ROM rename and delete now cascade to all
associated data: favorites, screenshots, user_data.db (videos, box art overrides),
and metadata.db game_library entries. See ROM management Phase 3 in
`research/investigations/rom-management-analysis.md`.

Remaining limitation: recent entries (`.rec` files) are not updated on rename
(acceptable — they expire naturally).

## Alpha Player Hidden from UI

The "Alpha Player" system (`alpha_player`) is a libretro video player core
whose "ROMs" are video files (mkv, avi, mp4, mp3, flac, ogg), not games. The
current game-centric UI — metadata fields, box art, "games"/"ROMs" labels —
does not make sense for video content.

### Current behavior
Alpha Player is listed in `HIDDEN_SYSTEMS` in `systems.rs` and filtered out by
`visible_systems()`, which is used by `scan_systems()` and `find_duplicates()`
in `roms.rs`. It will not appear in the systems list, global search results, or
duplicate detection. The system definition is still present in `SYSTEMS` and
`find_system()` still resolves it (needed if RePlayOS references it in
favorites/recents).

### Recommended future work
Build a dedicated "Media" section with video-appropriate UI. See
`research/investigations/alpha-player-analysis.md` for a full analysis.

### Priority
Low — Alpha Player is a niche feature and hiding it has no user-facing
downside. Revisit when media features are planned.

## New ROMs Added Externally Don't Get LaunchBox Metadata

When ROMs are added via SCP, NFS copy, or any mechanism outside the companion
app, some data sources pick them up automatically and others do not.

| Feature | Auto? | Details |
|---------|-------|---------|
| Game list | Yes | mtime-based cache detects new files on next page load |
| Built-in metadata (names, genres, players) | Yes | Compile-time DB lookup by filename |
| LaunchBox metadata (descriptions, ratings) | No | ROM index is a snapshot from import time |
| Thumbnails (box art / snaps) | Partial | On-demand download works if thumbnail index exists; image appears on 2nd view |

### Current behavior
- The ROM cache uses directory mtime to detect new files — game lists update
  immediately on next page load.
- LaunchBox import builds a ROM-to-metadata index at import time. New ROMs have
  no descriptions or ratings until the user manually triggers "Regenerate" or
  "Download & Import" from the Metadata page.
- If the thumbnail index has been built, on-demand downloads are triggered via
  `queue_on_demand_download()` when viewing a ROM with no local image. The image
  appears on the second page load. If the index was never built, no images
  appear until the user runs "Update Images".
- A filesystem watcher (inotify) monitors ROM directories on local storage
  (SD/USB/NVMe) and triggers automatic rescans within 3 seconds of changes.
  NFS mounts have no watcher; changes are detected via mtime on next access
  (30-minute TTL as safety net).

### Proposed solution
1. **LaunchBox metadata**: On ROM cache miss (new file detected), queue a
   lightweight re-match against the existing LaunchBox DB entries. No need to
   re-parse the full XML — just match the new filename against already-imported
   game titles.
2. **Thumbnail index**: Already handled by on-demand download for individual
   ROMs. Consider a periodic lightweight check (e.g., on system page load) to
   batch-queue missing thumbnails.

### Priority
Medium — affects any user who adds ROMs outside the companion app (common for
power users using SCP/NFS). Workaround: manually trigger re-import/update from
the Metadata page.

See `research/plans/thumbnail-new-roms-behavior.md` for full analysis.

## Scroll Position Lost on Back Navigation

When navigating from a game list (system page, search results, developer page) to a
game detail page and then pressing Back, the scroll position resets to the top of
the list. The user loses their place in long game lists.

### Current behavior
Leptos's `<Suspense>` re-renders the list from scratch on navigation. The browser's
native scroll restoration doesn't work because the content is async-loaded.

### Possible approaches
1. **`scroll-restoration` CSS** — `overflow-anchor: auto` might help in some cases
2. **Save scroll position in session storage** — store `window.scrollY` on navigate,
   restore after the list re-renders
3. **Keep-alive / caching** — cache the rendered list component so it doesn't
   re-fetch on back navigation
4. **Virtualized list** — only render visible items (overkill for current scale)

### Decision
Won't fix. The complexity of a PageCache solution doesn't justify the minor UX
inconvenience. Users can scroll back quickly. See
`research/investigations/scroll-restoration-analysis.md`.
