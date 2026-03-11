# Features

Tracking document for Replay Control. Organized by page/area.

---

## Home (`/`)

### Implemented
- **Last Played** hero card showing the most recently played game (name + system)
- **Recently Played** horizontal scroll showing the last ~10 played games (skipping the featured one)
- **Library stats** grid: total games, systems with games, total favorites, disk used
- **Systems overview** grid of systems that have games, with game count and link to system ROM list

### Planned
- None currently

### Future ideas
- **Last Seen** games section — track which games the user has browsed in the web UI (separate from play history)
- Quick-launch favorite games from the home page
- Disk usage breakdown by system (visual chart)
- **RetroAchievements summary** — if RA is configured, show user's total points, rank, and recently earned achievements

---

## Game Detail (`/games/:system/:filename`)

### Implemented
- **Header** with back button (browser history back navigation) and game title (display name or filename)
- **Box art display** — shows imported box art thumbnail when available; falls back to styled placeholder with game name
- **Game info card** — metadata grid showing system, filename, file size, and format/extension
- **Arcade metadata** — for arcade games, shows year, manufacturer, players, rotation, category, and parent ROM (if clone)
- **Videos section** — search and save game-related videos:
  - **Saved videos** (My Videos) — paste YouTube/Twitch/Vimeo/Dailymotion URLs, embedded inline with responsive 16:9 iframes
  - **Find Trailers** / **Find Gameplay** / **Find 1CC** buttons — on-demand search via Invidious/Piped APIs (multi-instance fallback)
  - **Inline preview** — click thumbnail/title in search results to watch without pinning; play icon overlay on thumbnails
  - **Pin to save** — pin search results to My Videos with tag (trailer/gameplay/1cc)
  - **Remove** — remove saved videos with overlay button
  - **Show all** — initial display limited to 3 videos, expandable
  - URL sanitization — strips tracking params, builds canonical URLs, uses `youtube-nocookie.com` for privacy
  - Separate `videos.json` storage (survives metadata clears)
  - Duplicate detection — prevents saving the same video twice
- **Placeholder sections** for future content:
  - Description ("No description available")
  - Screenshots gallery ("No screenshots available")
  - Manual ("No manual available")
- **Actions section** with:
  - Favorite/Unfavorite toggle with optimistic UI
  - Rename (inline input with Enter/Escape, navigates to renamed game on success)
  - Delete with confirmation step (navigates back to system page on success)
- **Navigation links** — game names are clickable throughout the app:
  - ROM list items link to game detail
  - Home page hero card (last played) links to game detail
  - Home page recent items link to game detail
  - Favorites hero card, recently added items, and all favorites list items link to game detail
- URL-encoded filenames in links to handle spaces, parentheses, etc.

### Planned
- **Screenshots gallery** — display screenshots taken on RePlayOS for the current game. Screenshots are discovered from the `captures/` directory by matching the ROM filename prefix. Shown as a horizontal scrollable gallery below the metadata section. See `docs/reference/screenshots-analysis.md`.
- **Game metadata integration** — cover images, descriptions, screenshots from external metadata providers
- **RetroAchievements integration** — show achievements for the current game if the user has configured their RA account. Displays: achievement list with icons, earned/locked status, earn dates, total points, and completion percentage. Data fetched from RA API and cached locally. See details in the RetroAchievements section below.
- **Manual viewer** — display game manuals in-browser

---

## Screenshots (`/screenshots`)

### Planned
- **Browse all screenshots** — dedicated page accessible from the More section listing all screenshots taken on RePlayOS
- **Grouped display** — screenshots grouped by system, with game name and timestamp for each
- **Game navigation** — each screenshot links to its game detail page
- **Delete screenshots** — delete individual screenshots with confirmation step
- **Pagination** — paginated or infinite-scroll loading for large collections

### Future ideas
- Bulk delete (multi-select)
- Filter by system or date range
- Fullscreen lightbox viewer

---

## RetroAchievements Integration (nice to have)

Integration with [RetroAchievements](https://retroachievements.org/) to show achievement data per game.

### Configuration

- **Username** — the user's RetroAchievements username (public, required)
- **Web API Key** — obtained from the user's RA control panel at retroachievements.org (required, stored securely in app config)
- **No password needed** — the RA API authenticates with username + API key only

Settings page gets a "RetroAchievements" section where the user enters their username and API key. A "Test Connection" button verifies the credentials are valid.

### Game Detail Integration

When RA is configured, the game detail page shows an "Achievements" section:
- **Achievement list** — each achievement with icon, title, description, points, and earned/locked status
- **Completion bar** — visual progress (e.g., "12/47 achievements — 25%")
- **Total points** — points earned for this game vs. total available
- **Earn dates** — when each achievement was unlocked (for earned ones)
- **Hardcore vs. softcore** — distinguish between hardcore and casual completions

### API Details

- **Auth:** username + web API key (base64 encoded). No OAuth, no password
- **Key endpoints:**
  - `getGameInfoAndUserProgress` — game metadata + user's achievement progress for a specific game
  - `getUserProgress` — batch progress check across multiple games
  - `getGame` — game metadata and achievement list (without user progress)
- **Matching:** RA uses its own game IDs. Match via hash-based lookup (RA supports MD5) or game title + platform search
- **Rate limits:** reasonable for individual use; cache responses locally to minimize calls
- **Caching:** store RA responses in the metadata SQLite DB (`metadata.db`), refresh on game detail page visit if data is older than configurable TTL (default: 1 hour)

### Future ideas
- Achievement notifications — poll for newly earned achievements
- Leaderboard display — show user's ranking on RA leaderboards per game
- RA user profile summary on home page (total points, rank, recent unlocks)
- Badge/mastery indicators on game list items

---

## Metadata Management (`/more/metadata`)

Dedicated page for managing external game metadata (descriptions, images, ratings). Accessible from the More section. Needed because of data licensing restrictions — metadata can't be bundled with the app and must be fetched/cached per device.

### Implemented

**Text metadata (LaunchBox):**
- **Auto-download** — "Download Metadata" button fetches LaunchBox XML from the internet, extracts, and imports into SQLite. Background task with real-time progress (downloading, parsing, matching)
- **Coverage stats** — per-system breakdown of matched/unmatched games, sorted alphabetically by system name
- **Clear metadata** — delete the SQLite cache and start fresh

**Image metadata (libretro-thumbnails):**
- **Per-system download** — download box art and screenshots for a specific system from libretro-thumbnails GitHub repos
- **Download All** — batch download images for all supported systems
- **Stop/Cancel button** — cancel in-progress image imports immediately (kills git clone subprocess, stops copy loop). "Cancelling..." feedback on the button
- **Real-time SSE progress** — `/sse/image-progress` endpoint streams progress every 200ms via Server-Sent Events. Client uses `EventSource` instead of polling for near-instant UI updates
- **Optimistic UI** — progress bar appears instantly on click, before the server responds
- **Per-system coverage grid** — shows box art count vs total games for each system, sorted alphabetically, with per-system download/update buttons
- **Image stats** — total box art count, screenshot count, and media disk usage
- **Pulsing animation** — disabled download buttons pulse to signal activity
- **exFAT symlink resolution** — handles fake symlinks on exFAT filesystems (git writes symlink targets as text files)
- **Fuzzy image matching** — normalized filename matching with tilde dual-name handling

**Cache management:**
- **Clear Images** — removes all imported images with confirmation step

**Attribution:**
- Source credits line: "Game descriptions and ratings provided by LaunchBox. Box art and screenshots from libretro-thumbnails."

### Future ideas
- Per-source enable/disable toggles (e.g., disable ScreenScraper, use only LaunchBox)
- Auto-sync on new ROM detection (fetch metadata automatically when new games appear)
- Metadata export/import (backup metadata cache to a file for transfer between devices)
- Per-game manual metadata refresh from the game detail page
- Metadata quality indicators on game list (icon showing which games have descriptions/images)
- ScreenScraper API integration for richer media (screenshots, videos)

---

## Games (`/games`, `/games/:system`)

### Implemented
- **Systems grid** showing all known systems with display name, manufacturer, game count, and total size
- Empty systems shown with reduced opacity and non-clickable
- **System ROM list** (`/games/:system`) with:
  - Search bar with debounced input (300ms)
  - ROM count display (loaded / total)
  - Infinite scroll with IntersectionObserver sentinel and manual "Load more" fallback
  - Pagination via server function (100 ROMs per page)
  - Per-ROM favorite toggle (star button) with optimistic UI update
  - Per-ROM rename (inline text input, Enter to confirm, Escape to cancel)
  - Per-ROM delete with confirmation step (delete button swaps to confirm/cancel)
  - ROM metadata display: filename, relative path, file size, file extension badge
  - Box art thumbnails in ROM list items (when imported images are available)
  - Only one delete confirmation or rename operation active at a time
- **Arcade display names** — full arcade DB with 28,593 unique entries covering Flycast/Naomi/Atomiswave (301), FBNeo (8,108), MAME 2003+ (5,272), and MAME current (26,777), deduplicated at build time via embedded PHF database. Display names appear in ROM lists, home page recents, favorites, and game detail pages. See `docs/arcade-db-design.md`.
- **`display_name` propagation** — `RomEntry`, `RecentEntry`, and `Favorite` all carry an optional `display_name` populated from the arcade DB for arcade systems. Search matches on both filename and display name.
- **`is_favorite` on RomEntry** — ROM entries carry a boolean `is_favorite` flag, enabling favorite state display directly in the ROM list without separate lookups
- **System display name in ROM list header** — system ROM list page shows the human-readable system name (e.g., "Sega Mega Drive") in the header instead of the system ID
- **Add favorite from game list** — favorite toggle in ROM list works correctly (was previously broken)

### Planned
- **ROM filename parsing** — parse No-Intro and GoodTools naming conventions to extract clean title, region, revision, translation info, and hack markers (see `docs/rom-identification.md`)
- **Game grouping** — group regional variants, revisions, hacks, and translations of the same game into a single collapsible row
- **Grouped view toggle** — switch between flat file list and grouped game list per system
- **Duplicate detection** — identify and flag duplicate ROMs across regions/dumps

### Future ideas
- **Preferred region** — user selects a preferred region (e.g., USA, Europe, Japan); games matching the preferred region sort to the top of the list, and in grouped view the preferred region variant is shown as the primary entry
- M3U multi-disc management (create, edit, reorder disc entries)
- ROM upload from browser (nice-to-have)
- Batch operations (multi-select delete, move)
- Filter by region, format, file type
- Sort by name, size, date
- Hide clones toggle (for arcade systems)
- Hide non-working games toggle (for arcade systems)

---

## Favorites (`/favorites`)

### Implemented
- **Featured / Latest Added** hero card showing the most recently added favorite (name + system)
- **Recently Added** horizontal scroll of the last ~10 favorites (newest first, excluding the featured one)
- **Stats** grid showing total favorites count and number of systems
- **By System** cards for each system that has favorites, showing count and latest favorite per system; links to the system's game list
- **All Favorites** section with:
  - Flat list view (all favorites with system badge)
  - Grouped view (favorites organized under system headers with count)
  - Toggle between flat and grouped views
- **Remove confirmation** — clicking the star shows a "Remove?" confirmation button instead of immediately removing; cancel button or clicking another item dismisses
- Only one confirmation active at a time across all favorites
- **Optimistic UI** — favorite removal updates the list immediately, server call happens in background
- **`date_added` on Favorite** — favorites carry a timestamp, enabling true "recently added" ordering

### Planned
- **Favorite organization** — group/flatten favorites by system subfolder (`_favorites/sega_smd/`) with server-side file reorganization (server functions `group_favorites` and `flatten_favorites` exist)

### Future ideas
- Search within favorites
- Quick-launch favorites (depends on game launching — see `docs/reference/game-launching.md`)
- Drag-to-reorder favorites
- Export/import favorites list
- Per-favorite notes or tags

---

## More / Settings (`/more`)

### Implemented
- **Menu items** linking to: Skin/Theme, Wi-Fi Configuration, NFS Share Settings, Hostname, Metadata, System Logs
- **System Info** section showing: storage type, storage path, disk total, disk used, disk available, ethernet IP, Wi-Fi IP
- **System Logs page** (`/more/logs`) — view RePlayOS system logs (journalctl) with source filter (all, companion app, RePlayOS) and refresh button. Useful for troubleshooting.
- **Skin/theme sync** — browse and apply RePlayOS skins from the web UI; optionally sync the app's color scheme to the active skin (see `docs/skin-theming-analysis.md`)
- **Hostname configuration** — view and change the Pi's hostname from the web UI; updates mDNS address
- **Wi-Fi configuration** — view and edit Wi-Fi settings (SSID, password, country, mode) from the web UI
- **NFS share settings** — view and edit NFS v4 share configuration (server IP, share path, version) from the web UI
- **Metadata management** — dedicated page at `/more/metadata` for importing LaunchBox metadata (auto-download + parse), viewing per-system coverage stats, importing libretro-thumbnails box art, clearing metadata/images, and regenerating the metadata DB (see `docs/game-metadata.md`)

### Planned
- **Screenshots browser** — menu item linking to `/screenshots` page for browsing and managing all RePlayOS screenshots (see `docs/reference/screenshots-analysis.md`)

### Future ideas
- RePlayOS config editor (replay.cfg settings)
- Theme/appearance settings
- **User language preference** — allow the user to choose their preferred language; the app will honor this setting for UI text and when building/fetching game databases. i18n infrastructure is in place (only English currently).
- **Preferred region** — user selects a preferred region (USA, Europe, Japan, etc.); honored by game list sorting and grouped view default variant selection
- About page with version info and links

---

## Infrastructure

### Implemented
- **Leptos 0.7 SSR** with WASM hydration — server pre-renders HTML, client hydrates for interactivity
- **Axum web server** serving SSR pages, REST API, and static assets
- **Server functions** (`#[server]`) for data fetching — direct calls on server, HTTP on client
- **Client-side routing** via `leptos_router` with proper browser history
- **PWA support** — manifest.json, service worker registration, apple-mobile-web-app meta tags
- **Internationalization (i18n)** — lightweight manual approach with `t(locale, "key")`, English default, extensible to additional languages
- **Responsive design** — mobile-first CSS with breakpoints for tablet (768px) and desktop (1024px)
- **Two-crate architecture**: `replay-control-core` (business logic, native only) and `replay-control-app` (SSR + hydration)
- **Build system** — `build.sh` script building WASM hydrate + server SSR binary (no cargo-leptos)
- **Arcade metadata database** — embedded PHF map with 28,593 unique entries (Flycast 301, FBNeo 8,108, MAME 2003+ 5,272, MAME current 26,777), generated at build time from XML/CSV data via `phf_codegen`. See `docs/arcade-db-design.md`.
- **GameRef unification** — unified game reference type used across ROM lists, favorites, recents, and game detail pages for consistent game identification
- **Storage abstraction** — supports local filesystem and USB-mounted storage, auto-detects storage root
- **Caching layer** — in-memory cache for system summaries with TTL-based expiration
- **Mirror types** — client-side type definitions matching server-side `replay-control-core` types for serialization
- **Server function registration** — explicit registration for library-crate server functions to prevent linker stripping
- **ROM filename parser** — regex-based parser (`rom_tags` module) for No-Intro and GoodTools naming conventions, extracting title, region, revision, flags (see `docs/rom-identification.md`)
- **Game metadata system** — SQLite metadata cache (`metadata_db`) with LaunchBox XML import (`launchbox` module) and libretro-thumbnails image import (`thumbnails` module). Coverage stats, per-system breakdown, auto-download with progress tracking. Cancellable git clone (subprocess kill), cooperative cancellation via `AtomicBool`, SSE real-time progress streaming. See `docs/game-metadata.md`.
- **Non-arcade game database** — embedded PHF maps for ~34K ROM entries across 20+ systems (`game_db` module). Two-level model: `CanonicalGame` + `GameEntry`. Sources: No-Intro DATs, TheGamesDB JSON, libretro-database DATs.
- **Skin sync** — read RePlayOS skin index from `replay.cfg`, extract dominant colors from skin PNG images, apply as CSS custom properties for theme synchronization (`skins` module)
- **Cross-compilation** — `./build.sh aarch64` for ARM (aarch64) Raspberry Pi binary
- **Install/deployment script** — `install.sh` supporting SSH deployment to Pi (see `docs/reference/deployment.md`)
- **SSE (Server-Sent Events)** — `/sse/image-progress` endpoint for real-time progress streaming (200ms interval). Used by the metadata page for image import progress instead of polling.

### Planned
- **Game launching** — launch games on RePlayOS from the web UI. Recommended approach: `_autostart` folder manipulation + process restart. Needs testing on real hardware. See `docs/reference/game-launching.md`.

### Future ideas
- **SQLite cache layer** — replace filesystem scanning with indexed database, populated by background scan, updated via inotify
- **ROM hash computation** — MD5/SHA1/CRC32 for hash-based identification and metadata matching
- **RetroAchievements integration** — connect user's RA account, show earned achievements per game (see dedicated section above for full details)
- **Authentication** — pairing token or password-based auth (middleware designed but not implemented)
- **Remote control** — trigger actions on RePlayOS from the web UI
- **Backup & sync** — backup ROM library, save states, configuration
- **Game recommendations** — suggest games based on library, favorites, play history
- **Non-installed game search** — discover games not in the library
- **Game manuals viewer** — read game manuals from the web UI
- **systemd integration** — run as a system service on RePlayOS
- **mDNS/Avahi** — auto-discovery via `replaypi.local`
- **CLI mode** — command-line interface for scripting and power users (same binary)
- **CI/CD pipeline** — automated builds and GitHub Releases (see `docs/reference/binary-distribution.md`)
- **App-specific configuration file** — Replay Control should NOT write to `replay.cfg`, which is reserved for official RePlayOS system configurations (Wi-Fi, NFS, video output, etc.) and lives on the SD card at `/media/sd/config/replay.cfg` (not on ROM storage). Instead, the app uses `.replay-control/settings.cfg` on the ROM storage device for storing user preferences such as preferred region, language, theme, and other app-level settings. The format is plain text and user-editable, using the same `key = "value"` syntax as `replay.cfg`.
