# Known Issues

## Alpha Player Hidden from UI

The "Alpha Player" system (`alpha_player`) is a libretro video player core
whose "ROMs" are video files (mkv, avi, mp4, etc.), not games. The game-centric
UI does not make sense for video content, so it is hidden from the system list.

**Priority:** Low — Alpha Player is a niche feature. Revisit when media features are planned.

## Triforce Artwork Unavailable

Triforce games currently have no supported source for box art or in-game
screenshots in Replay Control. Games can still be listed and launched, but these
images will remain unavailable until a suitable source is supported.

## Jaguar CD CHD Does Not Launch

If a Jaguar CD `.chd` does not launch, it may have been made with a `chdman`
build that lost the disc's session information. Recreate it from the original
CUE/BIN files using the `chdman` included with the
[latest Virtual Jaguar release](https://github.com/libretro/virtualjaguar-libretro/releases/latest)
(at least version 3.7.0),
or use the CUE/BIN files directly. Recompressing the existing CHD cannot
restore information that was lost during its first conversion.

## New ROMs Added Externally — Partial Metadata

When ROMs are added via SCP, NFS copy, or any mechanism outside the companion app:

| Feature | Auto? | Details |
|---------|-------|---------|
| Game list | Yes | stored scan state + inotify detect new files |
| Built-in metadata (names, genres, players) | Yes | Compile-time DB lookup by filename |
| LaunchBox metadata (descriptions, ratings) | Partial | Auto-matched during enrichment if LaunchBox was previously imported |
| Thumbnails (box art / snaps) | Partial | On-demand download if thumbnail index exists; image appears on 2nd view |

**Workaround:** Manually trigger "Regenerate" or "Download & Import" from the Metadata page for full coverage.

## Scroll Position Lost on Back Navigation

Navigating from a game list to a game detail page and pressing Back resets
the scroll position to the top.

**Status:** Won't fix — the complexity of a scroll restoration solution
doesn't justify the minor UX inconvenience.
