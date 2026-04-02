# Game Library

How the game library works from a user perspective: browsing systems, managing ROMs, and keeping your library up to date.

## Systems Grid

The main library view shows all known systems as cards, each with the system display name, manufacturer, game count, and total size. Systems with no ROMs appear dimmed.

Tap a system to view its games.

## Game List

Each system page shows its ROMs with:

- **Box art thumbnails** for every game that has images available
- **Search** within the system (debounced so it doesn't lag while typing)
- **Infinite scroll** with fast pagination (100 games per page)
- **ROM details** -- filename, file size (Mbit/Kbit for cartridge systems, MB/GB for disc-based), file format badge
- **Consistent game cards** -- the same card layout (box art, badges, favorite toggle) is used across all views: ROM lists, search results, developer pages, series siblings, and recommendations

## Per-Game Actions

Each game supports:

- **Favorite toggle** -- add or remove from favorites
- **Inline rename** -- rename the ROM file (with extension protection to prevent breaking file associations)
- **Delete with confirmation** -- shows file count and total size before deleting

**Smart multi-file management:** Delete handles related files together -- M3U + disc files, CUE + BIN, ScummVM data directories, SBI companions. Rename is restricted for formats where renaming would break the game (CUE sheets, ScummVM, binary-referenced M3U playlists).

## Multi-Disc Handling (M3U)

Games that span multiple discs (common for PlayStation, Sega CD, etc.) are handled automatically:

- When an M3U playlist exists, individual disc files are hidden from the game list
- Sizes from all disc files are aggregated into the playlist entry
- M3U playlists are auto-generated at scan time for multi-part games (Side A/B, Disk 1 of N)

From a user perspective, a 3-disc game appears as a single entry with the combined size.

## Arcade Display Names

Arcade ROMs use internal codenames (e.g., `sf2.zip`). The app automatically shows human-readable titles ("Street Fighter II") for ~15K playable arcade entries across MAME, FBNeo, and Flycast (Naomi/Atomiswave). Non-playable machines (slot machines, gambling, etc.) are filtered out.

## Favorites

- **Add/remove favorites** from any game card or the game detail page
- **Favorites page** with featured card, recently added scroll, per-system cards, and flat/grouped views
- **Organize by criteria** -- group favorites into subfolders by developer, genre, system, players, or alphabetically (up to 2 levels of nesting)
- **Remove confirmation** -- tapping the star shows "Remove?" before acting
- **Sorted by date added** -- newest first, consistent across subfolders
- **Favorites-based recommendations** on the favorites page:
  - "Because You Love [Game]" -- similar games by genre and developer
  - "More from [Series]" -- unfavorited series siblings across all your favorites

## Recents

Recently played games are tracked automatically when you launch a game from the app. The home page shows:

- **Last Played** hero card with the most recently launched game
- **Recently Played** horizontal scroll of recent games

## Region Preference

Set your preferred ROM region (USA, Europe, Japan, World) in Settings. This affects:

- **Sort order** -- preferred region variants appear first in game lists
- **Search scoring** -- preferred region gets a boost in search results
- **Recommendation dedup** -- when multiple region variants exist, the preferred one is shown

A secondary region preference is also supported for a two-tier sort: Primary > Secondary > World > others.

## Automatic Library Updates

On local storage (SD, USB, NVMe), the app watches the `roms/` directory for changes. New, modified, or deleted ROMs are detected automatically -- no manual refresh needed. Changes are debounced (3 seconds) to handle bulk file copies smoothly.

On NFS storage, automatic detection is not possible (inotify does not work across network mounts). Use the "Rebuild Game Library" button in the metadata page to pick up changes.

## Startup Behavior

On first launch or after a rebuild, the app scans all system directories to index your ROMs. During this process:

- The server responds immediately with a "Scanning game library..." banner showing progress (current system and game count)
- Pages are fully usable while scanning runs in the background
- If no storage is connected, a waiting page is shown until storage becomes available
- Interrupted scans are detected and resumed automatically

For architecture details on the cache tiers, scan pipeline, and enrichment process, see the [Architecture](../architecture/index.md) section.
