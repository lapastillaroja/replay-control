# Known Issues

## Alpha Player Hidden from UI

The "Alpha Player" system (`alpha_player`) is a libretro video player core
whose "ROMs" are video files (mkv, avi, mp4, etc.), not games. The game-centric
UI does not make sense for video content, so it is hidden from the system list.

**Priority:** Low — Alpha Player is a niche feature. Revisit when media features are planned.

## New ROMs Added Externally — Partial Metadata

When ROMs are added via SCP, NFS copy, or any mechanism outside the companion app:

| Feature | Auto? | Details |
|---------|-------|---------|
| Game list | Yes | mtime-based cache + inotify detect new files |
| Built-in metadata (names, genres, players) | Yes | Compile-time DB lookup by filename |
| LaunchBox metadata (descriptions, ratings) | Partial | Auto-matched during enrichment if LaunchBox was previously imported |
| Thumbnails (box art / snaps) | Partial | On-demand download if thumbnail index exists; image appears on 2nd view |

**Workaround:** Manually trigger "Regenerate" or "Download & Import" from the Metadata page for full coverage.

## Scroll Position Lost on Back Navigation

Navigating from a game list to a game detail page and pressing Back resets
the scroll position to the top.

**Status:** Won't fix — the complexity of a scroll restoration solution
doesn't justify the minor UX inconvenience.
