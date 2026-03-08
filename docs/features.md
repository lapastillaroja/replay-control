# Features

Tracking document for the Replay companion app. Organized by page/area.

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

---

## Game Detail (`/games/:system/:filename`)

### Implemented
- **Header** with back button (returns to system ROM list) and game title (display name or filename)
- **Cover art placeholder** — styled 4:3 aspect ratio box with game name centered; CSS class ready for future image integration
- **Game info card** — metadata grid showing system, filename, file size, and format/extension
- **Arcade metadata** — for arcade games, shows year, manufacturer, players, rotation, category, and parent ROM (if clone)
- **Placeholder sections** for future content:
  - Description ("No description available")
  - Screenshots gallery ("No screenshots available")
  - Videos ("No videos available")
  - Music / Soundtrack ("No soundtrack available")
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
- **Game metadata integration** — cover images, descriptions, screenshots from external metadata providers
- **Video search** — link to gameplay videos on YouTube and other sources
- **Manual viewer** — display game manuals in-browser
- **Soundtrack player** — play game music tracks

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
  - Only one delete confirmation or rename operation active at a time
- **Arcade display names** (Phase 0) — Flycast/Naomi/Atomiswave games show human-readable names (e.g., "Metal Slug 6" instead of `mslug6.zip`) via embedded PHF database (301 entries). Display names appear in ROM lists, home page recents, favorites, and game detail pages.
- **`display_name` propagation** — `RomEntry`, `RecentEntry`, and `Favorite` all carry an optional `display_name` populated from the arcade DB for arcade systems. Search matches on both filename and display name.

### Planned
- **Arcade DB expansion** — FBNeo (~8K entries) and MAME 2003+ (~5K entries) game databases (see `docs/arcade-db-design.md`, Phase 1)
- **ROM filename parsing** — parse No-Intro and GoodTools naming conventions to extract clean title, region, revision, translation info, and hack markers (see `docs/rom-identification.md`)
- **Game grouping** — group regional variants, revisions, hacks, and translations of the same game into a single collapsible row
- **Grouped view toggle** — switch between flat file list and grouped game list per system
- **Duplicate detection** — identify and flag duplicate ROMs across regions/dumps

### Future ideas
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

### Planned
- **Favorite organization** — group/flatten favorites by system subfolder (`_favorites/sega_smd/`) with server-side file reorganization (server functions `group_favorites` and `flatten_favorites` exist)
- **Date added** tracking — add a timestamp to favorites to enable true "recently added" ordering instead of relying on list position

### Future ideas
- Search within favorites
- Quick-launch favorites (depends on game launching — see `docs/game-launching.md`)
- Drag-to-reorder favorites
- Export/import favorites list
- Per-favorite notes or tags

---

## More / Settings (`/more`)

### Implemented
- **Menu items** for: Backup & Restore, Wi-Fi Configuration, NFS Share Settings (UI only, not functional)
- **System Info** section showing: storage type, storage path, disk total, disk used, disk available

### Planned
- **Background task system** — task manager with progress reporting, cancellation, and polling-based UI updates (see `docs/background-tasks.md`). Includes library scan trigger and task status display.
- **Wi-Fi configuration** — configure Wi-Fi networks from the web UI (currently a placeholder menu item)
- **NFS share settings** — configure NFS v4 share from the web UI (currently a placeholder menu item)

### Future ideas
- RePlayOS config editor (replay.cfg settings)
- Theme/appearance settings
- **User language preference** — allow the user to choose their preferred language; the app will honor this setting for UI text and when building/fetching game databases. i18n infrastructure is in place (only English currently).
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
- **Two-crate architecture**: `replay-core` (business logic, native only) and `replay-app` (SSR + hydration)
- **Build system** — `build.sh` script building WASM hydrate + server SSR binary (no cargo-leptos)
- **Arcade metadata database** — embedded PHF map for Flycast/Naomi/Atomiswave games (301 entries), generated at build time from CSV data via `phf_codegen`
- **Storage abstraction** — supports local filesystem and USB-mounted storage, auto-detects storage root
- **Caching layer** — in-memory cache for system summaries with TTL-based expiration
- **Mirror types** — client-side type definitions matching server-side `replay-core` types for serialization

### Planned
- **Arcade DB expansion** — add FBNeo (~8K entries) and MAME 2003+ (~5K entries) game databases, merged and deduplicated at build time (see `docs/arcade-db-design.md`)
- **ROM filename parser** — regex-based parser for No-Intro and GoodTools naming conventions, extracting title, region, revision, flags (see `docs/rom-identification.md`)
- **Background task manager** — `TaskManager` with `DashMap`, progress via `AtomicU32`, cancellation via `CancellationToken`, polling-based UI (see `docs/background-tasks.md`)
- **Game metadata integration** — pluggable metadata providers for box art, descriptions, ratings. ScreenScraper recommended as primary source for console games, Arcade Italia for arcade (see `docs/game-metadata-sources.md`)
- **Server function registration** — explicit registration for library-crate server functions to prevent linker stripping

### Planned
- **Game launching** — launch games on RePlayOS from the web UI. Recommended approach: `_autostart` folder manipulation + process restart. Needs testing on real hardware. See `docs/game-launching.md`.

### Future ideas
- **SQLite cache layer** — replace filesystem scanning with indexed database, populated by background scan, updated via inotify
- **ROM hash computation** — MD5/SHA1/CRC32 for hash-based identification and metadata matching
- **RetroAchievements integration** — connect user's RA account, show earned achievements per game
- **Authentication** — pairing token or password-based auth (middleware designed but not implemented)
- **Remote control** — trigger actions on RePlayOS from the web UI
- **Backup & sync** — backup ROM library, save states, configuration
- **Game recommendations** — suggest games based on library, favorites, play history
- **Non-installed game search** — discover games not in the library
- **Game videos** — search for related gameplay videos from YouTube or other sources
- **Game manuals viewer** — read game manuals from the web UI
- **Cross-compilation** — ARM (aarch64) binary for Raspberry Pi deployment
- **systemd integration** — run as a system service on RePlayOS
- **mDNS/Avahi** — auto-discovery via `replaypi.local`
- **CLI mode** — command-line interface for scripting and power users (same binary)
