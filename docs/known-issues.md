# Known Issues & TODOs

## ROM Rename Side Effects

When a ROM file is renamed via the companion app, several data sources that
reference the original filename become orphaned:

| Data | Location | Impact |
|------|----------|--------|
| User screenshots | `captures/{system}/{old_filename}_*.png` | Screenshots no longer match the renamed ROM |
| Pinned videos | `.replay-control/videos.json` (key `"{system}/{old_filename}"` becomes orphaned) | Video links lost for the renamed ROM |
| Favorites | `_favorites/*/{system}@{old_filename}.fav` | RePlayOS manages these — .fav file content has the old path |
| Recent entries | `_recent/{system}@{old_filename}.rec` | Old path in .rec file |
| Metadata DB | `.replay-control/metadata.db` | Cached metadata keyed by old filename |

### Current behavior
- **Favorites**: The `rename_rom` function in `roms.rs` already updates `.fav`
  files when renaming (renames the .fav file and updates its content).
- **Recent entries**: Not updated — old entries become orphaned (acceptable,
  they expire naturally).
- **User screenshots**: Not updated — orphaned after rename.
- **Pinned videos**: Not updated — orphaned after rename.
- **Metadata DB**: Not updated — stale cache entry (re-import would fix).

### Proposed solution
When renaming a ROM, cascade the rename to related data:
1. Rename matching screenshot files in `captures/{system}/`
2. Update video key in `.replay-control/videos.json` from old to new filename
3. Update metadata DB entry (or invalidate cache for that ROM)
4. Recent entries: skip (they expire naturally)

This should be a single `rename_rom_cascade()` function that wraps the existing
`rename_rom()` and handles all side effects. Failures in side effects should be
logged but not block the rename.

### Priority
Medium — affects users who rename ROMs and have screenshots/videos for them.
The workaround is to re-take screenshots or re-pin videos after renaming.

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
`docs/reference/alpha-player-analysis.md` for a full analysis.

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
- No background ROM rescan runs while the server is up — the filesystem watcher
  only monitors `replay.cfg`, not ROM directories.

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

See `docs/reference/thumbnail-new-roms-behavior.md` for full analysis.
