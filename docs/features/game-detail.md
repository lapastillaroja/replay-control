# Game Detail

Everything on the game detail page: info, media, navigation, and actions.

{{< screenshot "detail-sonic2-mobile.png" "Game detail page" >}}

## Box Art and Screenshots

The top of the page shows box art with a screenshots gallery. Tap the box art to browse screenshots and title screens in a lightbox carousel.

## Game Info Card

A summary card displays:

- System and filename
- File size (Mbit/Kbit for cartridge systems, MB/GB for disc-based)
- Developer, release year, genre
- Player count and co-op support

## Launch on TV

A "Play" button sends the game to RePlayOS for immediate launch on the connected TV. Available when the Pi is running RePlayOS with a supported core.

## User Screenshots

Screenshots captured on RePlayOS are matched to games by filename and displayed in a dedicated section. Tap any screenshot to view it in a fullscreen lightbox.

## Videos

Paste a video URL directly, or search for gameplay videos using privacy-respecting Invidious/Piped instances (no YouTube API or tracking). Pin a result to save it for the game. Pinned videos appear on future visits without re-searching.

## Game Manuals

Two sources for manuals:

- **In-folder detection** -- PDF or image files in the same ROM directory are detected and offered for viewing
- **Internet Archive download** -- search and download manuals from the Internet Archive directly from the detail page

Language preferences from Settings are respected when multiple manual languages are available.

## Box Art Swap

Browse alternate box art from libretro-thumbnails, typically region variants (US, EU, JP covers). Select any alternate to replace the current box art display.

## Game Series

Sequel/prequel breadcrumb navigation with play order position (e.g., "2 of 5"). Series siblings appear as a horizontal scroll of game cards. See [Game Series](game-series.md) for details on data sources and matching.

## Alternate Versions

Games identified as clones or region variants of the current game, grouped and displayed as cards. Useful for finding a specific region release or revision.

## Also Available On

Cross-system availability: the same game on other systems in your library. Tap to jump to that version's detail page.

## Variant Sections

Collapsible sections grouping related ROMs by type:

- **Regional variants** -- different region releases (USA, Europe, Japan)
- **Translations** -- fan translations and language patches
- **Hacks** -- ROM hacks and modifications
- **Specials** -- unlicensed, homebrew, beta, prototype, and other special releases

Each section is collapsed by default and shows the count of variants.

## Distribution Channel Tags

Games distributed through special channels display a tag: SegaNet, Satellaview, Sega Channel, or Sufami Turbo. These tags appear as badges on the info card.

## Related Games

Genre-based recommendations: other games in your library that share the same genre. Displayed as a horizontal scroll of game cards at the bottom of the page.

## Actions

Available from the detail page:

- **Favorite** -- toggle favorite status
- **Rename** -- inline rename with extension protection (restricted for formats where renaming would break the game)
- **Delete** -- confirmation dialog showing file count and total size, with smart multi-file handling (M3U + disc files, CUE + BIN, ScummVM data directories, SBI companions)
