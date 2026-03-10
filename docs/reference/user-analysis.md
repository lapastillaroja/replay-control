# Replay Control -- User Analysis

Comprehensive analysis of the Replay Control companion app from a user perspective, based on a full audit of the codebase (March 2026).

**App:** Leptos 0.7 SSR web app running on a Raspberry Pi alongside RePlayOS. Accessed via phone, tablet, or desktop browser on the same local network. Manages a retro game library (ROMs, favorites, metadata, system settings).

---

## 1. User Personas

### Persona A: The Casual Retro Gamer

**Profile:** 25--45, grew up with 8/16-bit consoles, has a modest collection (50--500 ROMs across 3--8 systems). Uses RePlayOS to relive childhood games on the TV. Interacts with the companion app occasionally from a phone on the couch.

**Goals:** Find a specific game quickly, launch it on the TV, maybe favorite it for next time. Browse what is available without remembering file names.

**Pain tolerance:** Low. Expects the app to "just work" like Netflix or a streaming service. Will not read documentation or configure metadata imports unless guided.

**Key flows:** Home page -> recently played -> tap to launch. Or: search bar -> type game name -> tap result -> launch.

### Persona B: The Collector / Curator

**Profile:** 30--50, maintains a large ROM library (2,000--20,000+ ROMs across 15--30 systems). Cares about organization, clean file names, complete metadata. Willing to spend time setting up the system properly.

**Goals:** Import metadata and box art for full coverage. Organize favorites into curated lists. Rename ROMs to follow conventions. Browse per-system collections with filtering. Track which games have metadata and fill gaps.

**Pain tolerance:** Medium to high. Will navigate the metadata page, configure NFS, and use power-user features. Wants detailed statistics and control.

**Key flows:** Metadata page -> download all metadata -> download all images -> verify per-system coverage. ROM list -> filter by genre -> browse -> add favorites -> organize favorites by genre/system.

### Persona C: The Parent / Family Setup Person

**Profile:** 30--45, setting up RePlayOS for kids or family use. Wants a curated, safe, and simple experience. May pre-select favorites and hide the rest.

**Goals:** Build a favorites list of age-appropriate games. Make it easy for kids to find and launch games. Keep settings locked down. Possibly rename ROMs to friendly names the kids recognize.

**Pain tolerance:** Very low for complexity. Wants a clear, clean interface. Will not tolerate cryptic file names or technical jargon.

**Key flows:** Browse systems -> add kid-friendly games to favorites -> organize favorites by system or genre -> hand the controller to the kids (they use the RePlayOS TV UI, not this app).

### Persona D: The Arcade Cabinet Builder

**Profile:** 25--55, building a dedicated arcade cabinet or bartop. Arcade-focused library (MAME, FBNeo, Flycast). Cares about driver status, rotation, clone/parent relationships, player count. May have 5,000+ arcade ROMs.

**Goals:** Filter arcade ROMs by working status, hide clones, find 2-player fighting games for the cabinet, check rotation for vertical monitor setups. Curate a playable subset from thousands of ROMs.

**Pain tolerance:** High for arcade-specific features. Expects detailed arcade metadata (the app delivers this well). Frustrated by generic game-library UIs that ignore arcade specifics.

**Key flows:** Arcade system page -> hide clones -> filter by genre "Fighter" -> browse -> check game detail for rotation and status -> add to favorites.

### Persona E: The Technical User / Developer

**Profile:** Any age. Comfortable with SSH, config files, system logs. May be contributing to RePlayOS or just likes to tinker.

**Goals:** Check system logs, configure Wi-Fi and NFS from the browser (instead of SSH), monitor disk usage, change hostname. Debug issues remotely.

**Pain tolerance:** Very high. Appreciates the logs page and system info. Wants raw data and control.

**Key flows:** More page -> system info -> check IPs -> Wi-Fi config -> change settings -> reboot. Or: More -> Logs -> filter by service -> diagnose issue.

---

## 2. User Journeys

### Journey 1: First Visit (New User)

1. User opens the app in a browser (e.g., `http://replay.local:8091/`).
2. **Home page** loads. If no games have been played yet, the hero card shows "No games played yet" and the recently played section shows "No recent games." The library stats show total game count, system count, and storage usage.
3. User sees the systems grid at the bottom and taps a system card (e.g., "Sega Mega Drive -- 120 games").
4. The ROM list page loads showing all ROMs for that system with a search bar, filter chips, and paginated list (100 per page, infinite scroll).
5. User taps a game name and arrives at the game detail page showing metadata, cover art (if imported), and actions.

**Friction points:**
- No onboarding or welcome message for first-time users. The empty hero card and "no games played" text give no guidance.
- The user does not know that metadata and box art exist until they discover the "More -> Game Metadata" page. There is no prompt or banner suggesting they download metadata.
- The "More" tab label is vague -- it hides important features (Wi-Fi, metadata, skins) behind a generic label.

### Journey 2: Finding and Launching a Game

1. User is on the home page and wants to play "Street Fighter II."
2. User taps the search bar on the home page (or types "/" on keyboard, which navigates to the search page).
3. Types "street fighter" -- results appear after 400ms debounce, grouped by system.
4. Results show top 3 matches per system with box art thumbnails, genre badges, and favorite stars. A summary line shows total matches across systems.
5. User taps a result, arrives at the game detail page.
6. User taps "Launch on TV" -- the game launches on RePlayOS's display.
7. After 3 seconds, the button shows "Launched!" feedback, then resets.

**Friction points:**
- The search page is not immediately obvious from the home page. The home page has a search bar, but pressing Enter there navigates to `/search?q=...` (a separate page). This two-search-bar situation (home page bar + dedicated search page) may confuse users.
- On the search page, if the search field is empty, the user sees "Recent Searches" chips and a "Random Game" button. This is good empty-state design.
- Search results show only the top 3 per system. The "See all" link goes to the system's ROM list with the search term pre-filled. This is reasonable but requires an extra click to see all matches within a system.

### Journey 3: Managing Favorites

1. User browses games and taps the star icon on ROM list items to mark favorites. Optimistic UI -- the star fills immediately.
2. User taps the "Favs" tab in the bottom nav.
3. Favorites page shows: hero card for the latest-added favorite, recently added horizontal scroll, stats (total favorites + system count), by-system cards, and a full list with flat/grouped toggle.
4. User can toggle between flat view (all favorites, system badges) and grouped view (favorites organized under system headers).
5. The "Organize Favorites" collapsible panel lets the user reorganize favorites into subfolders on disk by genre, system, players, rating, or alphabetically. A "Keep originals at root" checkbox maintains RePlayOS compatibility. "Flatten All" reverses the organization.

**Friction points:**
- The favorites page has no search functionality. With hundreds of favorites, finding a specific one requires scrolling.
- Removing a favorite requires tapping the star, then confirming with a "Remove?" button. This two-step process is good for preventing accidents but feels slow for bulk cleanup.
- The "Organize Favorites" feature reorganizes files on disk. This is a powerful but potentially confusing concept -- the user may not realize that organizing favorites creates actual filesystem subfolders.

### Journey 4: Browsing User Captures

1. User takes screenshots during gameplay on RePlayOS (via the hotkey on the controller).
2. Later, the user opens the companion app and navigates to a game's detail page.
3. The "Your Captures" section shows thumbnails of screenshots matching that game.
4. Thumbnails are shown in a wrapping grid. If there are more than 12, a "View all" button reveals the rest.
5. Tapping a thumbnail opens a fullscreen lightbox with prev/next navigation and keyboard support (arrow keys, Escape).

**Friction points:**
- There is no way to browse all captures across all games in one place. The captures are only visible per-game on the detail page. A dedicated "Captures" or "Screenshots" page is planned but not implemented.
- There is no way to delete captures from the companion app.

### Journey 5: Configuring the System

1. User taps the "More" tab.
2. Menu items: Skin, Wi-Fi, NFS, Hostname, Game Metadata, System Logs.
3. System info section shows storage type, path, disk usage, and network IPs.

**Sub-journeys:**
- **Wi-Fi:** Enter SSID, password, country code, security mode, hidden toggle. Save, then Reboot.
- **NFS:** Enter server address, share path, NFS version. Save, then Reboot.
- **Hostname:** Change the Pi's hostname for mDNS.
- **Skin:** Grid of color theme cards. Tap to apply. Optional "Sync with RePlayOS" toggle to follow the TV UI's skin setting.
- **Metadata:** Download LaunchBox descriptions/ratings (one-click), download libretro-thumbnails box art (per-system or all), view coverage stats, clear data.
- **Logs:** View journalctl output, filter by service (All / Replay Control / RePlayOS UI), refresh button.

---

## 3. Feature Inventory

### Complete Feature List

| Feature | Page/Location | Status |
|---------|--------------|--------|
| Last Played hero card | Home | Implemented |
| Recently Played horizontal scroll | Home | Implemented |
| Library statistics (games, systems, favorites, storage) | Home | Implemented |
| Systems grid with game counts | Home | Implemented |
| Home page search bar (navigates to search page) | Home | Implemented |
| System ROM list with pagination (100/page) | /games/:system | Implemented |
| Infinite scroll with IntersectionObserver | /games/:system | Implemented |
| Per-system search with debounce (300ms) | /games/:system | Implemented |
| ROM filter: hide hacks | /games/:system, /search | Implemented |
| ROM filter: hide translations | /games/:system, /search | Implemented |
| ROM filter: hide betas/protos | /games/:system, /search | Implemented |
| ROM filter: hide clones (arcade only) | /games/:system, /search | Implemented |
| ROM filter: genre dropdown | /games/:system, /search | Implemented |
| Box art thumbnails in ROM list | /games/:system | Implemented |
| Favorite toggle on ROM list items | /games/:system | Implemented |
| Inline ROM rename (Enter/Escape) | /games/:system | Implemented |
| ROM delete with confirmation | /games/:system | Implemented |
| Display names from arcade DB + game DB | Everywhere | Implemented |
| Global search across all systems | /search | Implemented |
| Search results grouped by system (top 3 per system) | /search | Implemented |
| "See all" links from search results to system ROM list | /search | Implemented |
| Search with box art thumbnails and genre/fav badges | /search | Implemented |
| Recent searches (localStorage, 8 max) | /search | Implemented |
| Random Game button | /search | Implemented |
| Genre filter dropdown (global and per-system) | /search, /games/:system | Implemented |
| URL state sync for search params and filters | /search, /games/:system | Implemented |
| Keyboard shortcut: "/" navigates to search | Global | Implemented |
| Game detail page with metadata grid | /games/:system/:filename | Implemented |
| Box art cover image | /games/:system/:filename | Implemented |
| Arcade-specific metadata (rotation, status, clone/parent, category) | /games/:system/:filename | Implemented |
| Console-specific metadata (region) | /games/:system/:filename | Implemented |
| External metadata (description, rating, publisher) from LaunchBox | /games/:system/:filename | Implemented |
| Screenshots section (single imported screenshot) | /games/:system/:filename | Implemented |
| User captures gallery with lightbox | /games/:system/:filename | Implemented |
| Game launch ("Launch on TV") | /games/:system/:filename | Implemented |
| Favorite/unfavorite toggle | /games/:system/:filename | Implemented |
| Game rename with navigation to renamed game | /games/:system/:filename | Implemented |
| Game delete with navigation back to system | /games/:system/:filename | Implemented |
| Saved video URLs (YouTube/Twitch/Vimeo/Dailymotion) | /games/:system/:filename | Implemented |
| Video search (trailers/gameplay/1CC via Piped/Invidious) | /games/:system/:filename | Implemented |
| Pin search results to saved videos | /games/:system/:filename | Implemented |
| Inline video preview from search results | /games/:system/:filename | Implemented |
| Favorites page with hero, recents, stats, by-system, full list | /favorites | Implemented |
| Favorites flat/grouped view toggle | /favorites | Implemented |
| Favorites organize into subfolders (genre/system/players/rating/alpha) | /favorites | Implemented |
| Favorites flatten (undo organization) | /favorites | Implemented |
| System-specific favorites page | /favorites/:system | Implemented |
| Remove favorite with confirmation | /favorites | Implemented |
| More menu page with system info | /more | Implemented |
| Skin/theme selection grid | /more/skin | Implemented |
| Skin sync with RePlayOS toggle | /more/skin | Implemented |
| Wi-Fi configuration (SSID, password, country, mode, hidden) | /more/wifi | Implemented |
| NFS share settings (server, share, version) | /more/nfs | Implemented |
| Hostname configuration | /more/hostname | Implemented |
| Metadata download (LaunchBox auto-download + import) | /more/metadata | Implemented |
| Metadata import progress (polling) | /more/metadata | Implemented |
| Per-system metadata coverage stats | /more/metadata | Implemented |
| Image download (libretro-thumbnails, per-system + all) | /more/metadata | Implemented |
| Image import progress (SSE real-time) | /more/metadata | Implemented |
| Image import cancel | /more/metadata | Implemented |
| Clear images with confirmation | /more/metadata | Implemented |
| Image stats (boxart count, screenshot count, media size) | /more/metadata | Implemented |
| System logs viewer with source filter | /more/logs | Implemented |
| System reboot from Wi-Fi and NFS pages | /more/wifi, /more/nfs | Implemented |
| PWA support (manifest, service worker, home screen) | Infrastructure | Implemented |
| Responsive design (mobile-first, tablet at 768px, desktop at 1024px) | Infrastructure | Implemented |
| Dark theme with CSS custom properties | Infrastructure | Implemented |
| Bottom navigation (Games, Favs, More) | Infrastructure | Implemented |
| Top bar with search + favorites quick access | Infrastructure | Implemented |
| safe-area-inset support (iPhone notch) | Infrastructure | Implemented |
| ROM upload API (multipart POST endpoint) | API only (no UI) | Partial |
| Screenshots browser page | /screenshots | Not implemented |
| RetroAchievements integration | -- | Not implemented |
| Multi-language support | -- | English only |
| Authentication | -- | Not implemented |
| Sort options for ROM lists | -- | Not implemented |
| Game grouping (variants/regions under one entry) | -- | Not implemented |

---

## 4. UX Pain Points

Based on code analysis, these are the issues a real user would encounter:

### 4.1 Navigation

**Back button behavior on game detail is inconsistent.** The back button uses `history.back()` when browser history length > 1, otherwise falls back to `/games/:system`. This means:
- Coming from home page's "Recently Played" -> game detail -> back = correct (goes to home).
- Coming from favorites -> game detail -> back = correct (goes to favorites).
- Opening a direct link to a game detail page -> back = goes to the system's ROM list (may not be where the user expects).

This is a reasonable implementation but the fallback destination (`/games/:system`) may surprise users who arrived via a shared link.

**Two search bars create confusion.** The home page has a search bar that navigates to `/search?q=...` on Enter. The dedicated search page at `/search` has its own search bar. They serve the same purpose but are separate components. A user might type in the home search bar and expect results to appear inline.

**Bottom nav has only 3 tabs.** The "More" tab consolidates Wi-Fi, NFS, hostname, metadata, logs, and skins. This is a lot of functionality behind a single generic "More" label. Important first-time-setup features (metadata import) are buried two levels deep.

### 4.2 Data Loading

**No offline indication.** The app has PWA support (service worker registration), but there is no indication when the Pi is unreachable or the connection drops. Server function errors display as raw error text. The service worker is registered but its caching strategy is not clear from the codebase (the `sw.js` file is in static assets, not examined here).

**ROM list data is re-fetched on every navigation.** The `Resource` for the first page of ROMs is created fresh each time the system ROM list component mounts. For large libraries, this means waiting for the server to scan and sort ROMs on every navigation. The server has an in-memory cache with TTL, so this is fast after the first load, but there is no client-side caching across navigations.

**Metadata page fires multiple Resources on load.** `MetadataPage` creates three Resources (`stats`, `coverage`, `image_coverage`) plus `image_stats`. All fire on mount. On a slow Pi, this means four concurrent server function calls. The page handles this with independent loading spinners per section, which is correct.

### 4.3 Interaction Design

**ROM actions (rename/delete) are always visible on mobile.** On desktop, the rename and delete buttons on ROM list items are hidden until hover (`@media (hover: hover) { .rom-actions { opacity: 0 } }`). On mobile (no hover), they are always visible, creating visual noise for every ROM in a potentially long list.

**No undo for destructive actions.** Deleting a ROM or removing a favorite is confirmed with a single extra click, but there is no undo. Once a ROM is deleted, it is gone from the filesystem. The two-step confirmation (click delete -> click "confirm delete") helps, but an undo toast would be safer.

**Rename navigates away.** When renaming a game from the game detail page, successful rename navigates to the new URL (`/games/:system/:new_filename`). This is correct, but the user sees a brief loading state as the page re-fetches. On the ROM list page, rename triggers a version bump that re-fetches the entire page, losing scroll position.

**Skin change requires page reload.** The skin page says "Skin saved. Reload to see the new skin." The CSS custom properties are injected server-side via an inline `<style>` tag, so the client cannot update them dynamically. The user must manually reload the page.

### 4.4 Content Gaps

**Empty sections clutter game detail pages.** The game detail page always renders sections for Description, Screenshots, Videos, and Manual. When empty, these show "No description available," "No screenshots available," etc. For the vast majority of games (those without imported metadata), every section shows its empty state, making the page feel barren.

**Manual section is permanently empty.** The Manual section always shows "No manual available" with no path to getting manuals. This is a placeholder that will never have content unless the feature is built. It creates a "half-finished" impression.

**Screenshot section shows at most one image.** The "Screenshots" section (imported from libretro-thumbnails) shows a single screenshot. The "User Captures" section below it can show multiple captures from gameplay. These two concepts (imported screenshot vs. user capture) serve different purposes but their proximity may confuse users.

### 4.5 Information Architecture

**System cards have no visual differentiation.** All system cards look identical structurally -- text only, no logos, no color coding. With 20+ systems visible, scanning for a specific system requires reading each card's text.

**No sorting on ROM lists.** Games are always alphabetically sorted. There is no option to sort by year, size, genre, or date added. For discovery ("show me games from 1993"), this is a significant gap since the metadata exists in the embedded databases.

**Favorites page lacks search.** With hundreds of favorites, the flat/grouped list can be very long. There is no search or filter within favorites.

---

## 5. Missing Features

Features that users would naturally expect but do not exist yet, ordered by expected user impact:

### High Impact

1. **Sort options for game lists.** Sort by name, year, size, genre, or region. The data is available in arcade_db and game_db but not exposed as sort options.

2. **Browse all captures/screenshots page.** Users take screenshots during gameplay and can see them per-game, but there is no centralized gallery. Planned at `/screenshots` but not implemented.

3. **Onboarding / first-run guidance.** No welcome message, no prompt to download metadata. A first-time user sees an empty home page with no direction.

4. **Search within favorites.** The favorites page has no search functionality.

5. **ROM upload from browser.** The API endpoint exists (`POST /api/:system/upload`) but there is no UI for it. The "More" page has an "Upload ROMs" label key in i18n but no menu item pointing to it.

### Medium Impact

6. **System filtering on home page.** Filter systems by category (Arcade/Console/Handheld/Computer) or manufacturer. The data exists in the system definitions but is not exposed as filters.

7. **Related games on game detail page.** "More by this developer" or "More in this genre" links from the game detail page would improve discovery flow.

8. **Batch operations.** Multi-select for bulk delete, bulk favorite, or bulk unfavorite on ROM lists.

9. **Game grouping.** Regional variants, revisions, and hacks of the same game shown as a single expandable row instead of separate list items.

10. **Light theme / high-contrast mode.** The app is dark-theme only (the skin system changes accent colors but the base dark palette is hardcoded). No light theme option, no high-contrast mode.

### Lower Impact

11. **Multiple languages.** The i18n infrastructure exists with `t(locale, "key")` and a `Locale` enum, but only English is implemented. Spanish, French, German, Portuguese, and Japanese would cover the retro gaming community.

12. **Export/import favorites.** No way to backup or transfer a favorites list.

13. **About page.** No version info, no links to RePlayOS website or documentation.

14. **Notification/toast system.** Status messages appear inline (green/red text) but there is no global toast/snackbar system. Messages can be missed if they appear off-screen.

15. **Persistent filter preferences.** Filter states (hide hacks, hide translations, etc.) reset on every page navigation. Users who always want hacks hidden must re-enable the filter each time.

---

## 6. Accessibility

### Current State

The app has minimal accessibility support. Here is what exists and what is missing:

#### What exists

- **Semantic HTML structure.** The app uses `<nav>`, `<main>`, `<header>`, `<section>`, `<h2>`, `<h3>`, `<button>`, `<input>`, `<select>`, `<label>` elements appropriately. This provides a reasonable document outline for screen readers.
- **Focus styles on inputs.** Search inputs and form inputs have `:focus` styles with accent-colored borders and subtle box-shadow. This provides visual focus indication for keyboard users.
- **Keyboard shortcut.** The "/" key navigates to the search page (skipping inputs/textareas). This is a power-user accessibility feature.
- **Lightbox keyboard navigation.** The captures lightbox supports arrow keys and Escape. This is good keyboard accessibility.
- **`viewport-fit=cover`** and `safe-area-inset` handling for iPhone notch areas.

#### What is missing

- **No ARIA attributes anywhere.** No `aria-label`, `aria-live`, `aria-describedby`, `role` attributes on interactive elements. The star buttons for favorites have `title` attributes ("Remove from favorites") but no `aria-label`.
- **No `aria-live` regions for dynamic updates.** Status messages after saving settings, import progress updates, and optimistic UI changes are not announced to screen readers.
- **No skip-to-content link.** There is no way to skip past the top bar and bottom nav to the main content.
- **No focus management after navigation.** When navigating between pages via the client-side router, focus is not moved to the new content. Screen reader users may not realize the page has changed.
- **No `prefers-reduced-motion` support.** Transitions and animations (hover effects, opacity changes, import progress pulse) do not respect the reduced motion preference.
- **No `prefers-color-scheme` support.** The dark theme is always active. Users with system-level light mode get no accommodation.
- **Icon buttons lack text labels.** The top-bar search icon (SVG magnifying glass) and favorites star icon are icon-only with no visible text label and no `aria-label`. Screen readers would announce these as empty links.
- **Emoji as icons.** Navigation icons, action buttons, and status indicators use Unicode emoji (controller, star, pencil, cross mark). These are readable by screen readers but may be announced verbosely (e.g., "Video Game" instead of "Games"). Custom SVG icons with `aria-hidden` and visually-hidden text would be better.
- **Color contrast.** The secondary text color (`#8b8f96` on `#0f1115`) has a contrast ratio of approximately 5.2:1 against the background, which meets WCAG AA for normal text but may be challenging for users with low vision. The border color (`#2a2e36` on `#0f1115`) has very low contrast and is purely decorative, which is acceptable.
- **No visible focus indicators on custom buttons.** Filter chips, skin cards, and action buttons have no `:focus-visible` or `:focus` styles (only hover styles). Keyboard users cannot tell which element is focused.
- **Touch targets.** Most buttons and interactive elements have adequate size (44px+ touch targets per Apple guidelines). The ROM list's rename and delete buttons (`rom-action-btn`) are smaller and closer together, which may cause mis-taps on mobile.

### Summary

The app is functional for sighted mouse/touch users but has significant accessibility gaps for keyboard-only users and screen reader users. The semantic HTML provides a reasonable foundation that could be improved incrementally.

---

## 7. Mobile vs Desktop

### Responsive Design Approach

The app uses a **mobile-first** CSS design with three breakpoints:

| Breakpoint | Target | Changes |
|-----------|--------|---------|
| Default (< 600px) | Phone | 2-column system grid, stacked actions, full-width buttons |
| 600px | Small tablet | Skin grid 3 columns, metadata form layout |
| 768px | Tablet | System grid 3 columns, stats grid 4 columns, game meta 3 columns, action buttons inline, launch button auto-width |
| 1024px | Desktop | System grid 4 columns, game meta 4 columns |

**Max content width:** `1200px` (via `.page { max-width: 1200px; margin: 0 auto; }`).

### Mobile Strengths

- **Bottom navigation** with 3 tabs (Games, Favs, More) is well-placed for thumb reach on phones.
- **Safe area insets** are handled for iPhone notch (`env(safe-area-inset-top)`, `env(safe-area-inset-bottom)`).
- **PWA manifest** with `"display": "standalone"` enables home screen installation. Apple-specific meta tags (`apple-mobile-web-app-capable`, `apple-mobile-web-app-status-bar-style`) are present.
- **Touch-friendly interactions.** Most interactive elements are full-width on mobile, providing large touch targets.
- **Horizontal scroll sections** (recently played, recently added favorites) use `overflow-x: auto` with `-webkit-overflow-scrolling: touch`.
- **Sticky top bar** (`position: sticky; top: 0`) keeps the app title and quick actions always visible.

### Mobile Issues

- **ROM list action buttons (rename/delete) are always visible on mobile.** The hover-reveal behavior (`@media (hover: hover)`) correctly detects touch devices and shows buttons permanently. However, this means every ROM row has two small buttons visible, adding visual clutter. A swipe-to-reveal pattern or a context menu triggered by long-press would be more mobile-native.
- **Filter chips wrap on narrow screens.** With 4 filter chips + a genre dropdown, the filter bar wraps to multiple lines on phones. This can push the ROM list content below the fold, especially when all filters are active.
- **No pull-to-refresh.** The app does not implement pull-to-refresh, which is a standard mobile pattern. Users must navigate away and back to refresh data.
- **Landscape orientation** is not specially handled. On a phone in landscape, the bottom nav takes up proportionally more vertical space. The `"orientation": "any"` in the manifest allows both orientations, which is correct.
- **Game detail page is long on mobile.** With all sections (cover art, launch button, info grid, description, screenshots, user captures, videos, manual, actions), the page can require significant scrolling on a phone. No section anchoring or tab-based layout is used.

### Desktop Strengths

- **Max-width container** prevents content from stretching too wide on large screens.
- **Hover effects** on ROM items, system cards, and buttons provide good interactive feedback.
- **ROM actions hidden until hover** (`@media (hover: hover)`) keeps the list clean on desktop.
- **Keyboard shortcut** ("/" for search) is a desktop-oriented productivity feature.
- **Multi-column grids** (4-column systems grid, 4-column metadata grid) use desktop screen real estate well.

### Desktop Issues

- **No sidebar navigation on desktop.** The bottom nav pattern works well on mobile but is suboptimal on desktop. On wide screens, a sidebar nav would use horizontal space better and avoid the "thumb reach" rationale that justifies bottom nav on phones.
- **Content is narrow on ultra-wide screens.** The 1200px max-width means a 2560px-wide monitor has >50% of the screen as empty space. This is a reasonable tradeoff for readability, but a two-pane layout (system list + ROM list side-by-side) would be more efficient.
- **No multi-pane views.** On desktop, clicking a system card navigates to a new page. A master-detail layout (system list on left, ROM list on right) would reduce navigation.

---

## 8. Recommendations

Prioritized by user impact and implementation effort.

### Priority 1: Quick wins (small effort, high impact)

1. **Add `aria-label` to icon-only buttons.** The top-bar search icon and star icon, ROM list star buttons, rename/delete buttons, and lightbox navigation buttons all lack accessible labels. Adding `aria-label` attributes is a one-line change per button.

2. **Hide empty game detail sections.** Instead of showing "No description available" / "No screenshots available" / "No manual available" for every game, hide these sections entirely when no content exists. Only show section headers when there is content to display (or when the user can take action, like adding a video).

3. **Add focus-visible styles.** Add `:focus-visible` CSS rules to interactive elements (buttons, links, filter chips, cards) so keyboard users can see which element is focused. A simple outline or border-color change on `:focus-visible` would suffice.

4. **Persist filter preferences in localStorage.** Save the "hide hacks," "hide translations," and "hide betas" filter states to localStorage so they persist across page navigations and sessions.

5. **Show metadata download prompt to new users.** When the home page loads with no metadata (no box art, no descriptions), show a subtle banner: "Download game metadata for cover art and descriptions" linking to `/more/metadata`. Dismiss permanently after first metadata import.

### Priority 2: Moderate effort, high impact

6. **Add sort options to ROM lists.** Add a sort dropdown to the ROM list page: alphabetical (default), by year, by genre, by file size. The metadata is already available from the game databases; only the UI and server-side sorting are needed.

7. **Search within favorites.** Add a search bar to the favorites page that filters the favorites list client-side (no server call needed since all favorites are already loaded).

8. **ROM upload UI.** The API endpoint already exists. Add a page at `/more/upload` with a system selector and file drop zone / file picker. This addresses Persona B (collector) and Persona C (parent setting up the system).

9. **Improve system cards with category badges or color coding.** Add a small badge ("Arcade" / "Console" / "Handheld" / "Computer") or a colored accent strip to system cards for visual differentiation. The `SystemCategory` enum already exists in the core crate.

10. **Add `prefers-reduced-motion` media query.** Wrap all CSS transitions and animations in a `@media (prefers-reduced-motion: no-preference)` block. For users who prefer reduced motion, set `transition: none` and disable the pulsing animation on the metadata download button.

### Priority 3: Larger effort, medium-to-high impact

11. **Dedicated screenshots/captures browser page.** Implement the planned `/screenshots` page. Group captures by system, show game name and timestamp, link each to its game detail page. The server-side `find_screenshots_for_rom` function already exists; a broader `find_all_screenshots` variant is needed.

12. **Multi-language support.** Add at least Spanish and Portuguese as a second and third language. The i18n infrastructure is ready -- it requires adding match arms to `t()` and a locale selector in the More page.

13. **Related games on game detail.** After the metadata grid, show a "More [Genre] Games" link that navigates to `/search?genre=[genre]`. Minimal effort, high discovery value. Could also add "More by [Developer]" if the developer is known.

14. **Light theme support.** Add a `@media (prefers-color-scheme: light)` CSS block that overrides the `:root` custom properties with light colors. The skin system already uses CSS custom properties, so this would layer naturally.

15. **Sidebar navigation on desktop.** At the 1024px breakpoint, switch from bottom nav to a left sidebar. The bottom nav component already checks the current pathname for active state, so the logic is reusable.

---

## Appendix: Page Map

```
/                          Home (last played, recents, stats, systems grid)
/games/:system             System ROM list (search, filters, infinite scroll)
/games/:system/:filename   Game detail (metadata, art, captures, videos, actions)
/favorites                 All favorites (hero, recents, stats, by-system, full list)
/favorites/:system         System-specific favorites
/search                    Global search (filters, grouped results, recent searches)
/more                      Settings menu + system info
/more/skin                 Skin/theme selection
/more/wifi                 Wi-Fi configuration
/more/nfs                  NFS share settings
/more/hostname             Hostname configuration
/more/metadata             Metadata management (descriptions, images, coverage)
/more/logs                 System logs viewer
```

## Appendix: Server Function Inventory

| Function | Purpose |
|----------|---------|
| `get_info` | System info (storage, disk, IPs, counts) |
| `get_systems` | All system summaries |
| `get_recents` | Recently played games with box art |
| `get_roms_page` | Paginated ROM list with search/filter |
| `get_rom_detail` | Full game detail with metadata |
| `get_favorites` | All favorites |
| `get_system_favorites` | Favorites for one system |
| `add_favorite` | Mark a game as favorite |
| `remove_favorite` | Unmark a favorite |
| `group_favorites` | Group favorites by system subfolder |
| `flatten_favorites` | Undo favorite grouping |
| `organize_favorites` | Organize favorites by criteria |
| `delete_rom` | Delete a ROM file |
| `rename_rom` | Rename a ROM file |
| `launch_game` | Launch game on RePlayOS TV |
| `global_search` | Search across all systems |
| `get_all_genres` | All available genres |
| `get_system_genres` | Genres for one system |
| `random_game` | Pick a random game |
| `get_wifi_config` / `save_wifi_config` | Wi-Fi settings |
| `get_nfs_config` / `save_nfs_config` | NFS settings |
| `get_hostname` / `save_hostname` | Hostname |
| `get_skins` / `set_skin` / `set_skin_sync` | Skin management |
| `restart_replay_ui` | Restart RePlayOS UI process |
| `reboot_system` | Reboot the Pi |
| `refresh_storage` | Re-detect storage |
| `get_metadata_stats` | Metadata DB statistics |
| `get_system_coverage` | Per-system metadata coverage |
| `download_metadata` | Auto-download LaunchBox metadata |
| `import_launchbox_metadata` | Import from local XML |
| `get_import_progress` | Metadata import progress |
| `clear_metadata` | Delete metadata cache |
| `regenerate_metadata` | Clear + re-import metadata |
| `get_image_coverage` | Per-system image coverage |
| `get_image_stats` | Image totals and media size |
| `import_system_images` | Download images for one system |
| `import_all_images` | Download images for all systems |
| `cancel_image_import` | Cancel image download |
| `get_image_import_progress` | Image import progress |
| `clear_images` | Delete all imported images |
| `get_system_logs` | Read journalctl output |
| `get_game_videos` | Saved videos for a game |
| `add_game_video` | Save a video URL |
| `remove_game_video` | Remove a saved video |
| `search_game_videos` | Search Piped/Invidious for videos |

## Appendix: Technology Stack

- **Frontend:** Leptos 0.7, compiled to WASM (hydration) + native (SSR)
- **Server:** Axum, serving SSR HTML + REST API + static files + SSE
- **Styling:** Single CSS file (2,236 lines), CSS custom properties for theming, no preprocessor
- **State:** Leptos signals (reactive), server-side in-memory cache with TTL
- **Data:** Embedded PHF databases (arcade_db ~29K entries, game_db ~34K entries), SQLite for metadata cache, filesystem for ROM storage/favorites/captures/videos
- **Build:** Custom `build.sh` (WASM + native), cross-compile via `./build.sh aarch64`
- **Deployment:** `install.sh` for SSH deploy to Pi, systemd service on RePlayOS
