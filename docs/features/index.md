# Features

Replay Control is a web-based companion app for [RePlayOS](https://www.replayos.com/), a retro gaming operating system for the Raspberry Pi. It runs as a local server on the Pi and provides a browser UI for managing your game library, launching games, and enriching your collection with metadata and artwork — all from your phone, tablet, or desktop.

**Offline-first by design.** Embedded databases compiled into the binary provide display names, genres, player counts, and year data for ~34K console ROMs and ~28K arcade games out of the box — no internet required. Online sources (LaunchBox, libretro-thumbnails) can optionally enrich the library with descriptions, ratings, and box art when connected.

---

## Home Page

- **Last Played** hero card with the most recently launched game
- **Recently Played** horizontal scroll of the last ~10 played games
- **Library stats** — total games, systems with games, total favorites, disk usage
- **Systems overview** — grid of systems that have games, with game count and link to ROM list
- **Recommendations** — curated blocks including random picks (genre-diverse), top-rated, multiplayer, favorites-based suggestions, and top genres. Recommendations are deduplicated by game title, respect region preference, and exclude clones, hacks, translations, and special ROMs. [Detail](features/recommendations.md)

## Game Library

- **Systems grid** — all known systems with display name, manufacturer, game count, and total size; empty systems shown dimmed
- **System ROM list** with search (debounced), infinite scroll, and pagination (100 per page)
- **Per-ROM actions** — favorite toggle, inline rename, delete with confirmation
- **ROM metadata** — filename, path, file size (Mbit/Kbit for cartridge systems, MB/GB for disc-based), extension badge, box art thumbnail
- **Arcade display names** — embedded database of ~28K arcade entries (MAME, FBNeo, Flycast/Naomi/Atomiswave) maps codenames to human-readable titles across the entire app
- **M3U multi-disc handling** — individual disc files are hidden when an M3U playlist exists; sizes are aggregated into the playlist entry
- **Filesystem watching** — on local storage (SD/USB/NVMe), inotify detects new, changed, or deleted ROMs and updates the library automatically. [Detail](features/game-library.md)

## Game Detail

- **Box art** with fallback placeholder when no image is available
- **Game info card** — system, filename, file size, format. Arcade games additionally show year, manufacturer, players, rotation, category, driver status, and parent ROM
- **Launch on TV** — launch the game on the RePlayOS device with visual feedback (launching, success, error states). Creates a recents entry on successful launch
- **User screenshots** — displays screenshots captured on RePlayOS, matched by ROM filename. Gallery view with fullscreen lightbox and keyboard navigation
- **Videos** — paste YouTube/Twitch/Vimeo/Dailymotion URLs or search for trailers, gameplay, and 1CC videos via Invidious/Piped. Pin results to saved videos. Privacy-respecting embeds (`youtube-nocookie.com`)
- **Box art swap** — pick alternate region-variant cover art from the full libretro-thumbnails catalog
- **Related games** — genre-based recommendations shown on the detail page
- **Actions** — favorite/unfavorite toggle, inline rename, delete with confirmation
- **Variant sections** — regional variants, translations, and hacks of the same game shown in dedicated collapsible sections

## Favorites

- **Featured card** showing the most recently added favorite
- **Recently Added** horizontal scroll
- **Stats** — total favorites and number of systems represented
- **By System** cards with per-system count and latest favorite
- **All Favorites** with flat list and grouped-by-system views (toggle)
- **Remove confirmation** — star click shows "Remove?" before acting; optimistic UI

## Global Search

- Cross-system search accessible from the nav bar, home page, or `/` keyboard shortcut
- Word-level fuzzy matching against both filenames and display names
- Region preference bonus, hack/translation penalties in scoring
- Filters: genre, driver status (arcade), favorites only, minimum rating
- URL-persisted query parameters
- Recent searches and "Random Game" button. [Detail](features/search.md)

## Metadata Management

Accessible from More > Metadata.

**Text metadata (LaunchBox):**
- One-click download of LaunchBox XML (~460 MB), with streaming parse and normalized title matching against the ROM library
- Real-time progress via Server-Sent Events (downloading, parsing, matching)
- Per-system coverage stats showing matched/unmatched games

**Image metadata (libretro-thumbnails):**
- Per-system or batch "Download All" image import from libretro-thumbnails GitHub repos
- Cancellable imports with real-time SSE progress
- Manifest-based thumbnail index using GitHub REST API — enables on-demand single-image downloads without cloning entire repos
- Fuzzy image matching with multi-tier fallback (exact, tag-stripped, version-stripped)
- Auto-deletes cloned repos after matching to save disk space

**Cache management:**
- Clear metadata, clear images, clear repo cache — each with confirmation. [Detail](features/metadata.md), [Thumbnails detail](features/thumbnails.md)

## Settings and System Configuration

Accessible from the More page.

- **Region preference** — select preferred ROM region (USA, Europe, Japan, World); affects sort order, search scoring, and recommendation dedup
- **Skin/theme sync** — browse and apply RePlayOS skins; optionally sync the app's color scheme to the active skin
- **Hostname** — view and change the Pi's hostname and mDNS address
- **Wi-Fi** — view and edit Wi-Fi settings (SSID, password, country, mode)
- **NFS share** — view and edit NFS v4 share configuration
- **System Info** — storage type and path, disk usage, network addresses
- **System Logs** — view RePlayOS system logs with source filter and refresh

## Storage

- Auto-detects storage mode from RePlayOS config: SD card, USB, NVMe (Pi 5), or NFS
- Config file watcher with automatic cache invalidation on storage changes
- App data stored in `.replay-control/` on the ROM storage device, separate from RePlayOS config. [Detail](features/storage.md)

## Technical Foundation

- **Leptos 0.7 SSR** with WASM hydration — server-rendered HTML with client-side interactivity
- **PWA** — installable as a home screen app on mobile devices
- **Responsive design** — mobile-first with tablet and desktop breakpoints
- **Three-tier game library cache** — in-memory (L1), SQLite (L2), filesystem (L3) for fast page loads with automatic freshness. [Detail](features/game-library.md)
- **Embedded game databases** — ~34K console ROMs (No-Intro + TheGamesDB + libretro-database) and ~28K arcade entries (MAME + FBNeo + Flycast) compiled via PHF maps for zero-cost lookups
- **ROM filename parser** — extracts title, region, revision, and classification (hack, translation, special) from No-Intro and GoodTools naming conventions. [Detail](features/rom-organization.md)
- **Cross-compilation** — `./build.sh aarch64` produces an ARM binary for Raspberry Pi deployment
- **Internationalization** — i18n infrastructure in place with English as the default language
