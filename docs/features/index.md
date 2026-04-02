# Features

Replay Control is a web-based companion app for [RePlayOS](https://www.replayos.com/), a retro gaming operating system for the Raspberry Pi. It runs as a local server on the Pi and provides a browser UI for managing your game library, launching games, and enriching your collection with metadata and artwork — all from your phone, tablet, or desktop.

**Offline-first by design.** Display names, genres, player counts, developer, and year data for ~34K console ROMs and ~15K playable arcade games are available out of the box — no internet required. Online sources (LaunchBox, libretro-thumbnails, Wikidata) can optionally enrich the library with descriptions, ratings, series data, and box art when connected.

> **A personal project.** Replay Control is built by one person who loves retro gaming and wanted a better companion app for RePlayOS. Feature requests and ideas are always welcome, but the roadmap follows personal preferences and what makes the app better for its core use case.

---

## Home Page

- **Last Played** hero card showing the most recently launched game
- **Recently Played** horizontal scroll of recent games
- **Library stats** — total games, systems with games, total favorites, disk usage
- **Systems overview** — grid of systems that have games, with game count and link to ROM list
- **Personalized recommendations** — curated blocks tailored to your library:
  - **Rediscover Your Library** — genre-diverse random picks from your collection
  - **Top Rated** — highest-rated games weighted by community vote count
  - **Multiplayer** — games supporting 2+ players
  - **Because You Love** — suggestions based on your favorited games' genres and systems
  - **Top Genres** — your most-played genre categories
  - **Curated Spotlight** — rotating section that highlights a different angle each visit:
    - Best by Genre (e.g., "Best Platformers")
    - Best of System (e.g., "Best of Mega Drive")
    - Games by Developer (e.g., "Games by Capcom")
    - Hidden Gems — highly rated games you haven't played yet
  - **Discover pills** — rotating quick-links to browse by genre, system, developer, decade, or multiplayer mode
- Smart recommendations — no duplicate games, region-aware, excludes clones, hacks, translations, and special ROMs. [Detail](recommendations.md)

## Game Library

- **Systems grid** — all known systems with display name, manufacturer, game count, and total size; empty systems shown dimmed
- **System ROM list** with debounced search, infinite scroll, and fast pagination (100 per page)
- **Per-ROM actions** — favorite toggle, inline rename, delete with confirmation
- **ROM details** — filename, path, file size (Mbit/Kbit for cartridge systems, MB/GB for disc-based), extension badge, box art thumbnail
- **Arcade display names** — ~15K playable arcade entries (MAME, FBNeo, Flycast/Naomi/Atomiswave) show human-readable titles instead of codenames across the entire app. Non-playable machines filtered out
- **M3U multi-disc handling** — individual disc files are hidden when an M3U playlist exists; sizes are aggregated into the playlist entry. Auto-generates M3U playlists for multi-part games (Side A/B, Disk N of M) at scan time
- **Automatic library updates** — on local storage (SD/USB/NVMe), new, changed, or deleted ROMs are detected and the library updates automatically
- **Consistent game cards** — uniform game rendering with box art, badges, and favorite toggle across all views (ROM lists, search, developer pages, series siblings, recommendations)
- **Organize preview** — nested folder structure display for organized favorites and developer subfolders
- **Instant startup** — the server responds immediately during warmup with a "Scanning game library..." banner; the library populates in the background. Starts gracefully when no storage is connected, showing a waiting page. Detects and resumes incomplete scans. [Detail](game-library.md)

## Game Detail

- **Box art** with fallback placeholder when no image is available
- **Screenshot gallery** — title screen and in-game screenshot displayed as labeled gallery items, with arcade codename translation handled automatically
- **Game info card** — system, filename, file size, format, developer. Arcade games additionally show year, manufacturer, players, rotation, category, driver status, and parent ROM
- **Launch on TV** — launch the game on the RePlayOS device with visual feedback (launching, success, error states). Creates a recents entry on successful launch
- **User screenshots** — displays screenshots captured on RePlayOS, matched by ROM filename. Gallery view with fullscreen lightbox and keyboard navigation
- **Videos** — paste YouTube/Twitch/Vimeo/Dailymotion URLs or search for trailers, gameplay, and 1CC videos via Invidious/Piped. Pin results to saved videos. Privacy-respecting embeds. Videos are shared across regional variants, with alias resolution for cross-name sharing
- **Box art swap** — pick alternate region-variant cover art from the full libretro-thumbnails catalog
- **Game series** — series name heading with horizontal scroll of series siblings across systems. Sequel/prequel breadcrumb navigation (< Prev | 2 of 5 | Next >) with bidirectional link filling. Clone ROMs used as fallback when non-clone targets are unavailable. [Detail](game-series.md)
- **Alternate versions** — other versions of the same game shown as chip links (clones, region variants with different tags)
- **Also available on** — cross-system section showing the same game on other systems in the library
- **Related games** — genre-based recommendations on the detail page
- **Game manuals** — in-folder document detection (PDF, TXT, HTML) and on-demand download from archive.org via RetroKit TSV. Language preferences for manual search. Inline delete for downloaded manuals
- **Actions** — favorite/unfavorite toggle, inline rename (with extension protection), delete with multi-file confirmation
- **Smart multi-file management** — delete handles M3U + disc files, CUE + BIN, ScummVM data directories, SBI companions. Rename restrictions prevent broken games (CUE, ScummVM, binary M3U). Delete confirmation shows file count and total size
- **Variant sections** — regional variants, translations, hacks, specials, arcade versions, and cross-name aliases shown in dedicated collapsible sections
- **Distribution channel tags** — SegaNet, BS (Satellaview), Sega Channel, and Sufami Turbo labels displayed on applicable ROMs

## Favorites

- **Featured card** showing the most recently added favorite
- **Recently Added** horizontal scroll
- **Stats** — total favorites and number of systems represented
- **By System** cards with per-system count and latest favorite
- **All Favorites** with flat list and grouped-by-system views (toggle)
- **Remove confirmation** — star click shows "Remove?" before acting; optimistic UI
- **Organize by developer** — favorites can be organized into subfolders by developer/manufacturer, with smart normalization of MAME manufacturer strings (licensing info, regional suffixes, corporate names, joint ventures)
- **Sorted by date added** — newest first, consistent across subfolders
- **Recursive unfavorite** — removing a favorite searches all subfolders, not just the root
- **Favorites-based recommendations:**
  - **Because You Love [Game]** — picks a random favorite, finds similar games by genre across systems, fills with developer matches
  - **More from [Series]** — finds unfavorited series siblings for all your favorited games

## Global Search

- Cross-system search accessible from the bottom nav Search tab, home page, or `/` keyboard shortcut
- **Fast cross-system search** across 23K+ games with near-instant results
- Word-level fuzzy matching against both filenames and display names
- Region preference bonus, hack/translation penalties in scoring
- Filters: genre, driver status (arcade), favorites only, minimum rating
- URL-persisted query parameters
- **Developer search** — searching a developer/manufacturer name shows a "Games by Developer" horizontal scroll block above regular results, with up to 2 additional matching developers as tappable links
- **Developer game list** — dedicated page per developer with system filter chips, content filters, and infinite scroll
- Recent searches and "Random Game" button. [Detail](search.md)

## Metadata Management

Accessible from More > Game Data.

**Text metadata (LaunchBox):**
- One-click download of LaunchBox XML (~460 MB), with fast streaming parse (~6s on Pi) and automatic title matching against the ROM library
- Real-time progress updates (downloading, parsing, matching)
- Per-system coverage stats showing matched/unmatched games
- Parses description, rating, rating count, publisher, developer, genre, max players, release date, cooperative flag

**Image metadata (libretro-thumbnails):**
- Per-system or batch "Download All" image import from libretro-thumbnails GitHub repos
- Three image types: box art (cover art), in-game screenshots, title screens
- Cancellable imports with real-time progress
- Smart image matching with multi-tier fallback (exact, tag-stripped, version-stripped)
- On-demand single-image downloads without cloning entire repos
- Auto-deletes cloned repos after matching to save disk space

**Series data (Wikidata):**
- ~5,345 embedded game series entries across 194+ franchises, with sequel/prequel chains and ordinals
- Wikidata attribution shown on the metadata page. [Detail](game-series.md)

**Cache management:**
- Clear metadata, clear images, orphaned image cleanup — each with confirmation
- Orphaned image cleanup with safety net per system. [Detail](metadata.md), [Thumbnails detail](thumbnails.md)

## Settings and System Configuration

Accessible from the More page, organized into Preferences, Game Data, and System sections.

- **Region preference** — primary and secondary preferred ROM region (USA, Europe, Japan, World); affects sort order, search scoring, and recommendation dedup. Default: World
- **Text size** — normal/large toggle
- **Skin/theme sync** — browse and apply RePlayOS skins; optionally sync the app's color scheme to the active skin. Skin and storage changes push instantly to all connected browsers
- **Hostname** — view and change the Pi's hostname and mDNS address
- **Change Password** — change the Pi's root SSH password from the web UI, with current-password verification
- **Wi-Fi** — view and edit Wi-Fi settings (SSID, password, country, mode)
- **NFS share** — view and edit NFS v4 share configuration
- **Version display** — app version and git hash shown in the More page footer; API endpoint for programmatic access
- **System Info** — storage type and path, disk usage, network addresses
- **System Logs** — view RePlayOS system logs with source filter and refresh

## Storage

- Auto-detects storage mode from RePlayOS config: SD card, USB, NVMe (Pi 5), or NFS
- Automatic library refresh when storage configuration changes; storage changes push to all connected browsers to trigger reload
- Smart filesystem detection — automatically chooses the best database configuration for the underlying filesystem (ext4, exFAT, NFS)
- Runtime corruption detection with recovery banners — metadata.db can be rebuilt, user_data.db can be restored from automatic backups
- App data stored in `.replay-control/` on the ROM storage device, separate from RePlayOS config. [Detail](storage.md)

## User Experience

- **Streaming SSR** — pages load progressively: the layout appears instantly with skeleton placeholders, then content fills in as data arrives. No blank screens while waiting for data
- **Skeleton loaders** — smooth loading animations for every data section across home, favorites, game detail, and search pages
- **Instant page loads** with smart multi-layer caching — back-navigation and rapid reloads feel instant
- **PWA** — installable as a home screen app on mobile devices; service worker precaches the app shell for offline loading with a fallback page when the device is unreachable. Pull-to-refresh on iOS standalone mode
- **Responsive design** — mobile-first layout that adapts to phones, tablets, and desktops. Grids, hero cards, screenshots, and navigation adjust at each breakpoint
- **Internationalization** — i18n infrastructure with English as the default language

## Libretro Core (Proof of Concept)

> **Note:** The libretro core is a technical experiment — a proof of concept for displaying game library data on the TV via the RePlayOS frontend. It is not a production feature.

- Displays recently played games and favorites with box art, navigable via gamepad
- Adapts layout for CRT (320x240) and HDMI (720p)
- Communicates with the companion app via REST API. [Detail](libretro-core.md)

---

## Feature Documentation

| Document | Coverage |
|----------|----------|
| [Getting Started](getting-started.md) | Prerequisites, quick install, first launch, adding ROMs |
| [Installation](install.md) | All install methods, update, uninstall, environment configuration |
| [Game Library](game-library.md) | System browsing, game actions, favorites, recents, region preference, automatic updates |
| [Game Series](game-series.md) | Wikidata series data, sequel/prequel navigation, cross-system matching |
| [Metadata](metadata.md) | Embedded databases, LaunchBox import, ROM classification |
| [Recommendations](recommendations.md) | Home page and favorites recommendation engine, spotlight rotation, deduplication |
| [Search](search.md) | Global search, developer search, developer game list page |
| [Storage](storage.md) | Storage detection, automatic updates, config boundary |
| [Thumbnails](thumbnails.md) | Box art, screenshots, title screens, image matching, box art swap |
| [Settings](settings.md) | System configuration, user preferences |
| [Libretro Core](libretro-core.md) | Recently played viewer, TV display |
| [Benchmarks](benchmarks.md) | Performance measurements on Raspberry Pi 5 |

## Architecture Documentation

For implementation details, database schemas, caching strategies, and design decisions, see the [Architecture](../architecture/index.md) section:

| Document | Coverage |
|----------|----------|
| [Architecture Overview](../architecture/index.md) | Crate structure, key file paths |
| [Technical Foundation](../architecture/technical-foundation.md) | Stack, embedded databases, ROM identification, cross-compilation |
| [Design Decisions](../architecture/design-decisions.md) | Performance design decisions, memory budget, rejected alternatives |
| [Database Schema](../architecture/database-schema.md) | SQLite tables, indexes, migrations |
| [Connection Pooling](../architecture/connection-pooling.md) | Database pool setup and filesystem safety |
| [Server Functions](../architecture/server-functions.md) | SSR, streaming, resource patterns, response cache |
| [Startup Pipeline](../architecture/startup-pipeline.md) | Background initialization phases |
| [Enrichment](../architecture/enrichment.md) | Box art, genre, rating population pipeline |
| [ROM Classification](../architecture/rom-classification.md) | Filename parsing and tier assignment |
| [Activity System](../architecture/activity-system.md) | Mutual exclusion and progress broadcasting |
