# Replay Control -- UX/Feature Feedback Analysis

A detailed review of the Replay Control companion app from the perspective of a retro gaming enthusiast who just discovered it and wants to rediscover old games.

**Context:** Leptos 0.7 SSR web app running on a Raspberry Pi, accessed via mobile or desktop browser. Manages retro game ROMs on a RePlayOS system.

**Date:** March 2026

---

## 1. First Impressions -- The Home Page

The home page gets the essentials right for a retro gaming dashboard. It follows a clean information hierarchy:

1. **Last Played** hero card (the most recent game, prominently displayed)
2. **Recently Played** horizontal scroll (last ~10 games)
3. **Library stats** grid (total games, systems, favorites, disk used, storage type)
4. **Systems** grid (systems with games, with counts)

**What works well:**
- The "Last Played" hero card is a strong focal point. Tapping it takes you directly to the game detail page. This mirrors the "continue where you left off" pattern that Netflix, Steam, and EmulationStation all use.
- Library stats give an immediate sense of scale -- seeing "3,368 Games" and "147 GB" is gratifying for a collector.
- The systems grid at the bottom provides a clear path forward: pick a system, start browsing.

**What could be better:**
- The hero card shows only the game name and system. Compare this to the UX.md wireframe which envisions box art, year, genre, and player count right on the hero card. A retro gamer looking at "Sonic The Hedgehog 2 / Sega Mega Drive" gets less excitement than seeing the box art, "1992 / Platformer / 1-2 Players" alongside it. When box art is available from the metadata system, surfacing it here would dramatically improve the emotional pull.
- The "Recently Played" items are 140px-wide text cards. Without thumbnails, they feel like a spreadsheet more than a game library. Even small box art thumbnails (48x48) would transform this section.
- There is no global search accessible from the home page. The top bar has a star icon for favorites, but no search icon. The UX.md design calls for a search button in the top bar. A retro gamer with 3,000+ games needs to find "that one game" quickly -- the current flow requires navigating to a specific system first, then using the per-system search.
- The "No games played yet" and "No recent games" empty states are plain text. For a first-time user, this is the first thing they see. It could be more welcoming -- perhaps a brief "Welcome to Replay Control" message with a clear call-to-action pointing to the Games tab.
- The stats grid shows "Storage" with the value "USB" or "NFS" in uppercase. This is technical information that most users do not care about on the home page. Consider replacing it with something more engaging like "Most Played System" or "Latest Addition."

---

## 2. Game Discovery -- Browsing, Searching, and Finding Games

### Systems Grid (`/games`)

The systems grid shows all known systems -- including those with zero games (shown at 40% opacity). This is a deliberate design choice.

**What works well:**
- Each system card shows: display name, manufacturer, game count, and total size. This is useful information density.
- The grid is responsive: 2 columns on mobile, 3 on tablet, 4 on desktop.
- Empty systems being visible is actually good -- it tells the user "RePlayOS supports this, go find some ROMs."

**What could be better:**
- There are no system icons or logos. Every card looks the same structurally. Compare EmulationStation's system view where each system has a distinctive logo -- a retro gamer can visually scan for "the SNES one" without reading. Even simple colored accents per manufacturer (Nintendo = red, Sega = blue, etc.) would help.
- No filtering or sorting. With 30+ systems, the grid can be large. Filters by category (Arcade / Console / Handheld / Computer) or manufacturer would help. The data model already has `SystemCategory` and `manufacturer` fields -- they just are not exposed as filters.
- Systems are shown in alphabetical order by folder name (e.g., `arcade_dc`, `arcade_fbneo`, `atari_2600`...). This means all arcade variants cluster at the top and all Atari systems come next. Sorting by game count (most games first) or by manufacturer would be more useful for discovery.

### ROM List (`/games/:system`)

This is the workhorse page -- where you browse a system's games.

**What works well:**
- **Search with debounce** is implemented well. 300ms debounce, synced to URL query param (so browser back works), and the search bar stays focused while results update (Transition instead of Suspense). This is technically excellent.
- **Infinite scroll** with IntersectionObserver + manual "Load more" fallback is the right choice for large lists. 100 ROMs per page is a reasonable batch size.
- **Favorite toggle** is inline -- the star button on each ROM row provides immediate access.
- **ROM count** shows "loaded / total" during pagination, which manages expectations.
- Each ROM shows: display name, relative path, file size, and extension badge. The path is useful for identifying subfolder organization.

**What could be better:**
- **No filtering by genre, players, year, or region.** This is the single biggest gap for game discovery. A retro gamer browsing 1,200 arcade games wants to say "show me 2-player fighting games." The data exists in the arcade DB and game DB (genre, players, year, region) -- it just is not exposed as filters on the ROM list page. The `get_roms_page` server function only supports text search, not structured filtering.
- **No sort options.** Games are sorted alphabetically by default (from the cache). There is no way to sort by year, size, genre, or "most recently added." For rediscovery, sorting by year is powerful -- "show me everything from 1993."
- **No thumbnails in the ROM list.** This is text-only. Compare LaunchBox's list view where each game has a small thumbnail. Even when box art is available via the metadata system, the ROM list does not use it.
- **Display names do not consistently show metadata.** For non-arcade systems, the display name comes from either the game DB or filename parsing. But additional metadata (year, genre, players) is only visible on the game detail page, not the list. Adding "(1992, Platformer)" after "Sonic The Hedgehog 2" in the list would aid browsing.
- **Rename and Delete are in the ROM list.** While power-user-friendly, having destructive actions (rename, delete) visible in the browse view is unusual. Most game library apps treat the list as read-only and put management actions on the detail page only. The hover-reveal on desktop is a nice touch, but on mobile these buttons are always visible.
- **The search only matches game name and filename.** It does not search by genre, year, developer, or other metadata. Searching "Capcom" does not find all Capcom games; searching "platformer" does not find platformers.

---

## 3. Game Detail Pages

The game detail page (`/games/:system/:filename`) is surprisingly rich in structure. It has sections for:

1. Cover art (box art or game name placeholder)
2. Game info grid (system, filename, file size, format, year, developer, publisher, genre, players, rating, region, plus arcade-specific fields)
3. Description
4. Screenshots
5. Videos (placeholder)
6. Music / Soundtrack (placeholder)
7. Manual (placeholder)
8. Actions (favorite, rename, delete)

**What works well:**
- **The metadata grid is comprehensive.** When metadata is available, you get year, developer, publisher, genre, players, rating, region -- all the fields that help decide "should I play this?" The grid layout (2 columns mobile, 3 tablet, 4 desktop) is clean.
- **Box art integration works.** When images have been downloaded via the metadata page, the cover art section shows actual box art. The fallback (game name as text in a 4:3 box) is reasonable.
- **Descriptions from LaunchBox** give genuine context. Having a paragraph about the game's story or gameplay is exactly what helps with rediscovery.
- **Rating display** ("3.8 / 5.0") from LaunchBox is useful for prioritizing what to play.
- **Arcade-specific fields** (rotation, driver status, clone parent, category) are genuinely useful for arcade enthusiasts. Knowing a game is "Vertical" helps when picking games for a CRT setup.
- **Favorite toggle** with optimistic UI is responsive and satisfying.
- **Rename with inline editing** and keyboard shortcuts (Enter/Escape) is well designed for power users.
- **Delete with two-step confirmation** prevents accidents.

**What could be better:**
- **The placeholder sections are distracting.** Videos, Music/Soundtrack, and Manual always show "No videos available," "No soundtrack available," "No manual available." These are permanent dead-ends for nearly every game. Showing empty sections with "not available" messages clutters the page. Consider hiding sections entirely when there is no content, and only showing the section headers when content exists (or when the feature is implemented). Right now they create a "half-finished" impression.
- **No "Related Games" or "More by this developer/genre."** After reading about a game, a natural next step is "show me similar games." A simple "More [Genre] games" or "More by [Developer]" link would dramatically improve discovery flow.
- **The screenshot section shows only one screenshot.** The `screenshot_url` field holds a single image. A gallery of multiple screenshots would be more engaging.
- **No way to launch the game from this page.** This is noted in the features.md as planned (via `_autostart` folder manipulation), but it is the most conspicuous gap for a retro gamer. The detail page tells you everything about the game but cannot help you play it.
- **Back button only goes to the system's ROM list.** If you arrived from the home page's "Recently Played" section or from Favorites, the back button says "Back" but takes you to `/games/:system`, not where you came from. This breaks the navigation mental model.
- **Cover art aspect ratio is fixed at 4:3.** Many console game covers are portrait-oriented (roughly 2:3 or 3:4). The 4:3 container uses `object-fit: contain` which works, but portrait covers end up quite small within the horizontal frame. A responsive aspect ratio (or letting the image dictate the container height) would better showcase the art.

---

## 4. Favorites System

The favorites page is one of the most thoughtfully designed parts of the app. It mirrors the home page structure with its own hierarchy:

1. **Latest Added** hero card
2. **Recently Added** horizontal scroll
3. **Stats** (total favorites, number of systems)
4. **Organize Favorites** collapsible panel
5. **By System** cards
6. **All Favorites** list (flat or grouped)

**What works well:**
- **The dual-view toggle** (flat vs. grouped) is genuinely useful. Flat for quick scanning, grouped for organized browsing.
- **Organize Favorites** is a power feature. Primary/secondary criteria (Genre, System, Players, Rating, Alphabetical) with actual filesystem reorganization. The "Keep originals at root" checkbox for RePlayOS compatibility shows attention to the real-world constraint. This is something LaunchBox and EmulationStation do not offer.
- **Optimistic removal** with confirmation prevents accidents without feeling sluggish.
- **System cards** in the "By System" section show the latest favorite per system, which is a nice touch.
- **System-specific favorites page** (`/favorites/:system`) provides focused browsing.

**What could be better:**
- **No search within favorites.** With 50+ favorites, finding a specific game requires scrolling. The ROM list has search; favorites should too.
- **No sorting options.** Favorites are shown in the order they were added (by `date_added`). Being able to sort alphabetically, by system, or by date would help.
- **The "Organize Favorites" panel is hidden by default.** This is appropriate since it is an advanced feature, but the collapse toggle (a small triangle + "Organize Favorites" text) is easy to miss. A brief description above it explaining what it does would help discoverability.
- **No box art in favorites list.** Same issue as the ROM list -- it is all text. When the user has carefully curated a favorites list, seeing the box art would make it feel like a personal collection rather than a file listing.
- **The "Remove?" confirmation text is hardcoded in English** (`"Remove?"` on line 334 of favorites.rs) instead of going through the i18n system. This would break in a translated UI.

---

## 5. Metadata Completeness

The metadata system is impressively engineered for a companion app:

- **LaunchBox XML** for descriptions, ratings, publishers (downloadable from internet)
- **libretro-thumbnails** for box art and screenshots (per-system Git repos)
- **Embedded game databases** (No-Intro DATs, TheGamesDB, arcade DB) for basic metadata (year, genre, developer, players)
- **SQLite cache** for persistence
- **Fuzzy matching** for ROM-to-metadata linking

**What works well:**
- **One-button metadata download.** "Download / Update" fetches the LaunchBox metadata XML (~460 MB), extracts and parses it, and matches entries to the user's ROMs. Progress is shown in real-time (state, items processed, items matched, elapsed time). This is significantly easier than ScreenScraper's per-ROM API approach.
- **Per-system image download.** Each system's images can be downloaded independently, and there is also a "Download All" button for bulk import. Progress tracking shows system-by-system status for multi-system downloads.
- **Coverage visibility.** The metadata page shows per-system coverage (e.g., "142/203 (70%)"), giving users a clear picture of what is and is not covered.
- **Attribution section.** Properly credits LaunchBox and libretro-thumbnails. Respects data licensing.
- **Image stats** show box art count, screenshot count, and total media size on disk.

**What could be better:**
- **No indication of metadata status from the game list or favorites.** A user has no way to know which games have descriptions/images without clicking into each one. Even a subtle indicator (a small icon or colored dot) on ROM list items would help. The features.md mentions this as a future idea.
- **Coverage percentages can be misleading.** The matching uses normalized title comparison, which means some matches may be incorrect (different games with similar names). There is no way to see or fix mismatches.
- **No per-game metadata refresh.** If a game's metadata is wrong or missing, the only option is to re-run the entire import. A "Refresh metadata" button on the game detail page would be useful.
- **Images are not visible in lists.** Even after downloading hundreds of megabytes of box art, the only place images appear is on the game detail page. The home page, ROM lists, and favorites are all text-only. This undermines the value of the image download feature.
- **The metadata page is buried under More > Game Metadata.** A first-time user might not discover it. Consider adding a prompt or banner on the home page when metadata has not been downloaded yet ("Enhance your library with game descriptions and box art").

---

## 6. Navigation

### Bottom Tab Bar

Four tabs: Home, Games, Favs, More. Uses emoji icons (house, gamepad, star, hamburger). Active tab highlighted with accent color.

**What works well:**
- The tab bar is fixed at the bottom, always accessible. This is the standard mobile pattern.
- Tab labels are short and clear.
- Active state detection uses path-based matching (`/favorites/*` highlights the Favs tab), which works correctly for nested pages.
- Safe area insets are handled for iPhone notch/home indicator.

**What could be better:**
- **Emoji icons vary across platforms.** The house, gamepad, star, and hamburger emoji render differently on iOS, Android, Windows, and Linux. Some may be monochrome, others colorful. Using SVG icons would ensure visual consistency.
- **No active indicator animation.** The active tab just changes color. A subtle transition or indicator bar would improve perceived polish.
- **The "Favs" label is abbreviated** while "Home," "Games," and "More" are not. "Favorites" would be more consistent (it fits on most screens).
- **No desktop sidebar layout.** The UX.md mentions that "on desktop, bottom tabs can become a left sidebar," but this is not implemented. On a desktop browser, the bottom tab bar works but feels mobile-first.

### Top Bar

Shows "Replay Control" on the left and a star icon (link to favorites) on the right.

**What could be better:**
- **Missing global search.** The UX.md wireframe shows a search icon in the top bar. This is one of the most important missing navigation elements.
- **The star icon duplicates the Favs tab.** Having the same destination (favorites) accessible from both the top bar and bottom tab is redundant.
- **No settings/gear icon in the top bar.** The UX.md shows one, but it is not implemented.

### Back Navigation

Back buttons use "\u{2190} Back" text with a border-styled button. They link to a hardcoded parent route.

**What could be better:**
- **Back buttons do not respect navigation history.** The game detail page always goes back to `/games/:system`, even if the user arrived from home or favorites. Using `history.back()` or tracking the referrer would be more intuitive.
- **Inconsistent back button placement.** The game detail page, system favorites page, and settings pages all have back buttons, but they are styled slightly differently and placed in different positions relative to the page title.

---

## 7. Missing Features -- What a Retro Gamer Would Expect

In rough priority order:

1. **Global search.** The ability to search across all systems from anywhere in the app. "I know I have Castlevania somewhere but I don't remember which system." This is table stakes for any game library app.

2. **Game launching.** Being able to tap "Play" from the app and have the game start on the TV. This is noted as planned but is the feature that would transform the app from a "library manager" into a "game launcher." EmulationStation, LaunchBox, and Playnite all center around this.

3. **Filters (genre, players, year, region).** The data exists in the databases. Exposing it as filters on the ROM list would unlock genuine game discovery. "Show me all 2-player games from the 90s" is a common retro gaming use case.

4. **Box art in lists.** The infrastructure is there (metadata system, image downloads), but the visual payoff is not delivered in the browse experience. Every modern game library app shows thumbnails in the list view.

5. **Random game suggestion.** A "I'm feeling lucky" or "Surprise me" button that picks a random game. This is uniquely valuable for retro gaming where the library is large and the user may not know what they want to play.

6. **Sort options.** Sort by name, year, size, rating, recently added. Different contexts call for different ordering.

7. **Collections/Playlists.** Beyond favorites, the ability to create named collections ("Couch Co-op Night," "Beat 'em ups," "Games to finish"). LaunchBox and Playnite both support this.

8. **Play statistics.** Play count, total play time, last played date per game. RePlayOS tracks "recently played" via marker files, but richer statistics would help with rediscovery ("I played this for 10 hours" vs. "I opened this once").

9. **RetroAchievements integration.** Already documented as planned. Showing achievement progress on game detail pages would add significant depth.

10. **Upload UI.** The upload API exists (`/upload/:system` endpoint), but there is no UI page for it. The More menu shows "Upload ROMs" in the UX.md wireframe but it is not implemented as a page. Users currently need to use SFTP or direct SD card access.

---

## 8. Pain Points

### Confusing UI Patterns

- **Destructive actions in the browse list.** Rename and Delete buttons on every ROM in the list view are unexpected. Most users are browsing, not managing files. Consider moving these to the game detail page only (where they already exist), or hiding them behind a long-press/right-click/swipe gesture.

- **Three different "back" patterns.** Some pages use a styled back button in the page header. Some pages rely on the bottom tab bar. Settings pages have back buttons that go to `/more`. This inconsistency requires the user to think about "how do I go back from here?"

- **The "Organize Favorites" concept is complex.** Primary/secondary criteria, keep-originals toggle, organize vs. flatten -- this is powerful but the UI does not explain what it actually does to the filesystem. A brief explanation or preview ("This will create subfolders like `_favorites/Platformer/` and move your .fav files into them") would prevent confusion.

### Missing Feedback

- **No toast/notification for favorite toggle.** When you star or unstar a game in the ROM list, there is no visual confirmation beyond the star icon changing. A brief toast ("Added to favorites") would confirm the action, especially on mobile where the star is small.

- **No loading indicator for favorite toggle.** The optimistic UI updates the star immediately, but if the server call fails silently, the user's expectation and reality diverge. There is no error handling for failed favorite operations.

- **Delete and rename in the ROM list give no success feedback.** The version counter increments and triggers a re-fetch, but there is no toast or message confirming "ROM deleted" or "ROM renamed."

### Dead Ends

- **Videos, Music, Manual sections on every game detail page.** These always show "not available." They create the impression of a broken or incomplete feature. Until content exists, these sections should be hidden.

- **The "More" page menu item "Upload ROMs" is listed in UX.md but not implemented.** The upload API exists but has no UI surface.

- **System cards for empty systems go nowhere useful.** On the Games page, tapping a system with 0 games shows an empty ROM list with just a search bar and "0 games." There is no guidance ("Add ROMs via SFTP or USB").

### Mobile-Specific Issues

- **The ROM list action buttons (rename, delete) are always visible on touch devices.** The CSS uses `@media (hover: hover)` to hide them on hover-capable devices, but on touch devices they are always present, adding visual noise to every row.

- **No pull-to-refresh.** On mobile, swiping down to refresh is expected. Currently the only way to refresh data is to navigate away and back.

- **The search bar on the ROM list does not auto-focus.** When you open a system's ROM list, you have to manually tap the search bar. On mobile, where the keyboard is a significant action, auto-focus after navigation might be too aggressive, but at least a prominent search icon could indicate its availability.

---

## 9. Delight Moments -- What Is Surprisingly Good

### Skin Synchronization

The skin system is genuinely delightful. The app reads RePlayOS's active skin from `replay.cfg`, extracts colors from the skin PNG, and applies them as CSS custom properties. There are 11 built-in skins (REPLAY, MEGA TECH, PLAY CHOICE, ASTRO, etc.) with names that evoke classic arcade and console aesthetics. The skin page shows visual previews of each palette. The "Sync with RePlayOS" toggle means the web app automatically matches whatever theme is running on the TV.

This is the kind of detail that makes a retro gamer smile. It shows the developer understands the aesthetic side of retro gaming, not just the technical side.

### Arcade Metadata Depth

The arcade database with 28,593 entries is serious. For arcade games, the detail page shows rotation (Horizontal/Vertical), driver status (Working/Imperfect/Preliminary), clone relationships, and the original MAME category. This is information that matters to arcade enthusiasts -- knowing a game is "Preliminary" saves you from trying to play a broken ROM, and knowing it is "Vertical" matters for CRT orientation.

### The Organize Favorites Feature

Being able to organize favorites by genre, players, or rating into actual filesystem subfolders -- and having those subfolders recognized by RePlayOS's native UI -- is a uniquely thoughtful feature. This bridges the web companion app and the native frontend in a way that other companion apps do not attempt.

### Display Name Resolution

The multi-layered name resolution (arcade DB -> game DB -> No-Intro DAT -> filename parsing -> tag extraction) means that most games show human-readable names instead of raw filenames. "Sonic The Hedgehog 2 (World)" instead of "Sonic The Hedgehog 2 (World) (Rev A).md". The tag system intelligently preserves useful suffixes like region codes while stripping noise. This is the kind of invisible polish that users benefit from without noticing.

### The PWA Foundation

The app registers a service worker, has a manifest.json, and sets apple-mobile-web-app meta tags. Adding it to a phone's home screen gives it a native-app feel. For a companion app that lives on the local network, this is exactly the right approach.

### Infinite Scroll Implementation

The IntersectionObserver-based infinite scroll with a manual "Load more" fallback, URL-synced debounced search, and version-based cache invalidation after mutations (delete/rename) is technically excellent. It handles edge cases (timer cleanup on unmount, URL back-button sync) that many implementations miss.

### i18n Infrastructure

While only English is currently supported, the translation system is clean and extensible. Every user-facing string goes through `t(locale, key)`. Adding Spanish, Portuguese, French, or Japanese would require only adding match arms in the `t` function. For a European retro gaming community this is forward-thinking.

---

## 10. Comparison to Other Retro Gaming Apps

| Feature | EmulationStation | LaunchBox | Replay Control |
|---|---|---|---|
| Game launching | Yes (core feature) | Yes (core feature) | Not yet (planned) |
| Box art in lists | Yes | Yes | No (detail page only) |
| Global search | Yes | Yes | No (per-system only) |
| Filter by genre/players | No | Yes | No (data exists, not exposed) |
| Sort options | Limited | Yes | No |
| Favorites | Yes (simple) | Yes (collections) | Yes (with organize) |
| Metadata scraping | Yes (ScreenScraper) | Built-in | Yes (LaunchBox + libretro) |
| Skin/theme | Yes (extensive) | Yes | Yes (syncs with OS) |
| Web-based | No | No | Yes (unique advantage) |
| Mobile access | No | No | Yes (unique advantage) |
| ROM management | No | Limited | Yes (rename, delete, upload API) |
| Multi-device | No | No | Yes (any browser) |

Replay Control's unique advantage is that it runs on the Pi itself and is accessible from any device. No other companion app offers this for RePlayOS. The ROM management features (rename, delete, organize favorites) are also distinctive -- EmulationStation and LaunchBox do not let you manage files from their UI.

The main gap relative to established tools is the visual richness. EmulationStation and LaunchBox are visually immersive -- box art everywhere, animated backgrounds, game video previews. Replay Control is clean and functional but text-heavy. The metadata infrastructure to close this gap exists; it just needs to be surfaced in the browse experience.

---

## 11. Summary of Recommendations

### High Impact, Lower Effort
1. **Hide empty placeholder sections** (Videos, Music, Manual) on game detail page when no content exists
2. **Add global search** -- a search input in the top bar that searches across all systems
3. **Show box art thumbnails** in the ROM list and favorites list (the data and serving infrastructure already exist)
4. **Add sort options** to the ROM list (alphabetical, year, size)
5. **Add search to favorites** page
6. **Fix back navigation** to respect browser history instead of hardcoded parent routes
7. **Move the i18n-missing "Remove?" string** through the translation system

### High Impact, Higher Effort
8. **Add genre/players/year filters** to the ROM list (requires extending `get_roms_page` server function)
9. **Show box art on home page** hero card and recent items
10. **Add a "Random Game" button** to the home page or per-system ROM list
11. **Build the Upload ROMs UI** page (API already exists)
12. **Add metadata status indicators** to ROM list items (icon showing which games have descriptions/images)

### Polish
13. Replace emoji icons in the bottom nav with SVG icons for cross-platform consistency
14. Add toast notifications for favorite toggle, delete, and rename actions
15. Add empty-state guidance for systems with no games ("Add ROMs via SFTP or USB")
16. Hide rename/delete from ROM list on mobile; keep them on game detail page only
17. Consider a "Welcome" first-run experience when the library is empty or metadata has not been downloaded
