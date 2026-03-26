# Features

Replay Control is a web-based companion app for [RePlayOS](https://www.replayos.com/), a retro gaming operating system for the Raspberry Pi. It runs as a local server on the Pi and provides a browser UI for managing your game library, launching games, and enriching your collection with metadata and artwork — all from your phone, tablet, or desktop.

**Offline-first by design.** Embedded databases compiled into the binary provide display names, genres, player counts, developer, and year data for ~34K console ROMs and ~15K playable arcade games out of the box — no internet required. Online sources (LaunchBox, libretro-thumbnails, Wikidata) can optionally enrich the library with descriptions, ratings, series data, and box art when connected.

---

## Home Page

- **Last Played** hero card with the most recently launched game
- **Recently Played** horizontal scroll of the last ~11 played games (query limited to 15 entries for performance)
- **Library stats** — total games, systems with games, total favorites, disk usage
- **Systems overview** — grid of systems that have games, with game count and link to ROM list
- **Recommendations** — curated blocks including random picks (genre-diverse), top-rated (weighted by vote count), multiplayer, favorites-based suggestions, and top genres. Recommendations are deduplicated by game title, respect region preference, and exclude clones, hacks, translations, and special ROMs. [Detail](recommendations.md)

## Game Library

- **Systems grid** — all known systems with display name, manufacturer, game count, and total size; empty systems shown dimmed
- **System ROM list** with search (debounced), infinite scroll, and SQL-level pagination (100 per page via `LIMIT`/`OFFSET`)
- **Per-ROM actions** — favorite toggle, inline rename, delete with confirmation
- **ROM metadata** — filename, path, file size (Mbit/Kbit for cartridge systems, MB/GB for disc-based), extension badge, box art thumbnail
- **Arcade display names** — embedded database of ~15K playable arcade entries (MAME, FBNeo, Flycast/Naomi/Atomiswave) maps codenames to human-readable titles across the entire app. Non-playable machines (slot machines, gambling, etc.) filtered at build time.
- **M3U multi-disc handling** — individual disc files are hidden when an M3U playlist exists; sizes are aggregated into the playlist entry. Auto-generates M3U playlists for TOSEC-named multi-part games (Side A/B, Disk N of M) at scan time
- **Filesystem watching** — on local storage (SD/USB/NVMe), inotify detects new, changed, or deleted ROMs and updates the library automatically
- **Unified GameListItem** — consistent game rendering component across all list views (ROM lists, search results, developer pages, series siblings, recommendations) with box art, badges, and favorite toggle
- **Sequenced startup** — server responds immediately during warmup with empty data and a "Scanning game library..." banner; background pipeline runs auto-import, populate, enrich, and watchers in order. [Detail](game-library.md)

## Game Detail

- **Box art** with fallback placeholder when no image is available
- **Screenshot gallery** — title screen (`Named_Titles`) and in-game screenshot (`Named_Snaps`) displayed as labeled gallery items, with arcade MAME codename translation handled automatically via `resolve_image_on_disk`
- **Game info card** — system, filename, file size, format, developer. Arcade games additionally show year, manufacturer, players, rotation, category, driver status, and parent ROM
- **Launch on TV** — launch the game on the RePlayOS device with visual feedback (launching, success, error states). Creates a recents entry on successful launch
- **User screenshots** — displays screenshots captured on RePlayOS, matched by ROM filename. Gallery view with fullscreen lightbox and keyboard navigation
- **Videos** — paste YouTube/Twitch/Vimeo/Dailymotion URLs or search for trailers, gameplay, and 1CC videos via Invidious/Piped. Pin results to saved videos. Privacy-respecting embeds (`youtube-nocookie.com`). Videos are shared across regional variants via `base_title`, with alias resolution for cross-name sharing
- **Box art swap** — pick alternate region-variant cover art from the full libretro-thumbnails catalog
- **Game series** — series name heading with horizontal scroll of series siblings (cross-system). Sequel/prequel breadcrumb navigation (`< Prev | 2 of 5 | Next >`) using Wikidata P155/P156 chains with bidirectional link filling at build time. Clone ROMs used as fallback when non-clone targets are unavailable. [Detail](game-series.md)
- **Alternate versions** — other versions of the same game shown as chip links (clones, region variants with different tags)
- **Also available on** — cross-system section showing the same game (`base_title` match) on other systems in the library
- **Related games** — genre-based recommendations shown on the detail page
- **Game manuals** — in-folder document detection (PDF, TXT, HTML) and on-demand download from archive.org via RetroKit TSV. Language preferences for manual search. Inline delete for downloaded manuals
- **Actions** — favorite/unfavorite toggle, inline rename (with extension protection), delete with multi-file confirmation
- **ROM management** — multi-file delete handles M3U + disc files, CUE + BIN, ScummVM data directories, SBI companions. Rename restrictions prevent broken games (CUE, ScummVM, binary M3U). Delete confirmation shows file count and total size for multi-file ROMs
- **Variant sections** — regional variants, translations, hacks, specials, arcade versions, and cross-name aliases shown in dedicated collapsible sections

## Favorites

- **Featured card** showing the most recently added favorite
- **Recently Added** horizontal scroll
- **Stats** — total favorites and number of systems represented
- **By System** cards with per-system count and latest favorite
- **All Favorites** with flat list and grouped-by-system views (toggle)
- **Remove confirmation** — star click shows "Remove?" before acting; optimistic UI
- **Organize by developer** — favorites can be organized into subfolders by developer/manufacturer, with algorithmic normalization of MAME manufacturer strings (licensing info, regional suffixes, corporate names, joint ventures)
- **Sorted by date added** — favorites list sorted by newest first, consistent across subfolders
- **Recursive unfavorite** — removing a favorite searches all subfolders, not just the root

## Global Search

- Cross-system search accessible from the nav bar, home page, or `/` keyboard shortcut
- **Parallel search** across systems via `tokio::spawn` for faster results
- **SQL pre-filtered search** — `search_text` column enables database-level filtering before in-memory scoring (220ms to 16ms)
- Word-level fuzzy matching against both filenames and display names
- Region preference bonus, hack/translation penalties in scoring
- Filters: genre, driver status (arcade), favorites only, minimum rating
- URL-persisted query parameters
- **Developer search** — searching a developer/manufacturer name shows a "Games by Developer" horizontal scroll block above regular results, with up to 2 additional matching developers as tappable links
- **Developer game list** — `/developer/:name` page with system filter chips, content filters, and infinite scroll
- Recent searches and "Random Game" button. [Detail](search.md)

## Metadata Management

Accessible from More > Game Data.

**Text metadata (LaunchBox):**
- One-click download of LaunchBox XML (~460 MB), with single-pass streaming parse (~6s on Pi) and normalized title matching against the ROM library
- Real-time progress via Server-Sent Events (downloading, parsing, matching)
- Per-system coverage stats showing matched/unmatched games
- Parses description, rating, rating count, publisher, developer, genre, max players, release date, cooperative flag

**Image metadata (libretro-thumbnails):**
- Per-system or batch "Download All" image import from libretro-thumbnails GitHub repos
- Three image types: `Named_Boxarts` (cover art), `Named_Snaps` (in-game screenshots), `Named_Titles` (title screens)
- Cancellable imports with real-time SSE progress
- Manifest-based thumbnail index using GitHub REST API — enables on-demand single-image downloads without cloning entire repos
- Fuzzy image matching with multi-tier fallback (exact, tag-stripped, version-stripped)
- Auto-deletes cloned repos after matching to save disk space

**Series data (Wikidata):**
- ~5,345 embedded game series entries across 194+ franchises, with sequel/prequel chains and ordinals
- Wikidata attribution shown on the metadata page. [Detail](game-series.md)

**Cache management:**
- Clear metadata, clear images, orphaned image cleanup — each with confirmation
- Orphaned image cleanup with 80% safety net per system. [Detail](metadata.md), [Thumbnails detail](thumbnails.md)

## Settings and System Configuration

Accessible from the More page, organized into Preferences, Game Data, and System sections.

- **Region preference** — primary and secondary preferred ROM region (USA, Europe, Japan, World); affects sort order, search scoring, and recommendation dedup. Default: World
- **Text size** — normal/large toggle with rem-based scaling
- **Skin/theme sync** — browse and apply RePlayOS skins; optionally sync the app's color scheme to the active skin. Skin and storage changes push instantly to all connected browsers via broadcast SSE (`/sse/config`)
- **Hostname** — view and change the Pi's hostname and mDNS address
- **Wi-Fi** — view and edit Wi-Fi settings (SSID, password, country, mode)
- **NFS share** — view and edit NFS v4 share configuration
- **System Info** — storage type and path, disk usage, network addresses
- **System Logs** — view RePlayOS system logs with source filter and refresh

## Storage

- Auto-detects storage mode from RePlayOS config: SD card, USB, NVMe (Pi 5), or NFS
- Config file watcher with automatic cache invalidation on storage changes; storage changes push via broadcast SSE (`/sse/config`) to trigger client reload
- Filesystem-aware SQLite journal mode: WAL on POSIX-capable filesystems (ext4, btrfs, xfs), DELETE on exFAT/FAT32/NFS
- Runtime corruption detection — `sqlite3_errcode` checked after every query; `SQLITE_CORRUPT` triggers a recovery banner with phase, system name, and progress count. metadata.db can be rebuilt, user_data.db can be restored from automatic startup backup
- **Cache-Control headers** for static assets — `pkg/` assets use 1-hour cache (no content hash in filenames); other static assets use standard caching
- App data stored in `.replay-control/` on the ROM storage device, separate from RePlayOS config. [Detail](storage.md)

## Libretro Core — Recently Played Viewer

A libretro core (.so) loaded by the RePlayOS frontend on the TV:

- Displays recently played games and favorites with box art, navigable via gamepad
- Shows game metadata (year, developer, genre, players, rating, description)
- Adapts layout for CRT (320x240) and HDMI (720p)
- Communicates via REST API (`/api/core/recents`, `/api/core/favorites`, `/api/core/game/:system/:filename`). [Detail](libretro-core.md)

## Technical Foundation

- **Leptos 0.7 SSR** with WASM hydration — server-rendered HTML with client-side interactivity
- **PWA** — installable as a home screen app on mobile devices
- **Responsive design** — mobile-first with breakpoints at 600px (small tablet), 768px (tablet landscape), 900px (medium tablet), and 1024px (desktop). Grids, hero cards, screenshots, and navigation adapt at each breakpoint.
- **Three-tier game library cache** — in-memory (L1), SQLite (L2), filesystem (L3) for fast page loads with automatic freshness. [Detail](game-library.md)
- **Embedded game databases** — ~34K console ROMs (No-Intro + TheGamesDB + libretro-database) and ~15K playable arcade entries (MAME + FBNeo + Flycast) compiled via PHF maps for zero-cost lookups
- **Embedded series database** — ~5,345 Wikidata series entries compiled at build time for game franchise identification. [Detail](game-series.md)
- **ROM filename parser** — extracts title, region, revision, and classification (hack, translation, special) from No-Intro, GoodTools, and TOSEC naming conventions. TOSEC support includes structured tag parsing (year, publisher, side/disk), 17 country code mappings, bracket flag classification ([a] Alternate, [h] Hack, [cr] Cracked, etc.) with display labels, language codes, and format suffix disambiguation. [Detail](rom-organization.md), [Detail](metadata.md)
- **CRC32 ROM identification** — hash-based ROM identification for 9 cartridge systems using No-Intro DATs
- **deadpool-sqlite connection pool** — async `pool.get().await` + `conn.interact().await` API prevents tokio worker starvation; 3 concurrent read connections + 1 write per DB (both WAL and DELETE modes), 10-second pool wait timeout
- **Broadcast SSE** — `/sse/config` pushes skin and storage changes to all connected browsers; activity SSE uses broadcast instead of polling
- **Cross-compilation** — `./build.sh aarch64` produces an ARM binary for Raspberry Pi deployment
- **REST API** — `/api/core/` endpoints for the libretro core. [Detail](libretro-core.md)
- **Internationalization** — i18n infrastructure in place with English as the default language

---

## Feature Documentation

| Document | Coverage |
|----------|----------|
| [Game Library](game-library.md) | Three-tier cache, ROM scanning, enrichment, startup pipeline, unified GameListItem |
| [Game Series](game-series.md) | Wikidata series data, sequel/prequel navigation, cross-system matching |
| [Metadata](metadata.md) | Embedded databases, LaunchBox import, Wikidata, GameInfo API |
| [Recommendations](recommendations.md) | Home page recommendation engine, weighted rating, deduplication |
| [ROM Organization](rom-organization.md) | Favorites, recents, region preferences |
| [Search](search.md) | Global search, developer search, developer game list page |
| [Storage](storage.md) | Storage detection, filesystem watching, config boundary |
| [Thumbnails](thumbnails.md) | Box art, screenshots, title screens, matching pipeline, arcade image resolution |
| [Libretro Core](libretro-core.md) | Recently played viewer, REST API, CRT/HDMI layout |
| [Game Launching](game-launching.md) | Implementation guide for autostart-based game launching |
