# Game List UI/UX Patterns Analysis

Comprehensive analysis of every place in the RePlayOS Companion App where games
are displayed as lists, scrolls, grids, cards, or items.

**Last updated:** March 2026 (post-refactor)

Source files analyzed:

- `replay-control-app/src/pages/home.rs`
- `replay-control-app/src/pages/favorites.rs`
- `replay-control-app/src/pages/search.rs`
- `replay-control-app/src/pages/game_detail.rs`
- `replay-control-app/src/components/hero_card.rs`
- `replay-control-app/src/components/rom_list.rs`
- `replay-control-app/src/components/genre_dropdown.rs`
- `replay-control-app/src/components/system_card.rs`
- `replay-control-app/src/server_fns.rs`
- `replay-control-app/src/types.rs`
- `replay-control-app/style/style.css`

---

## 1. Inventory of Every Game List

| # | Location | Component | Layout | Data Source |
|---|----------|-----------|--------|-------------|
| 1 | Home: Last Played | `HeroCard` (shared) | Hero card | `RecentWithArt` (first item) |
| 2 | Home: Recently Played | `GameScrollCard` (shared) | Horizontal scroll | `RecentWithArt` (items 2-11) |
| 3 | Favorites: Latest Added | `HeroCard` (shared) | Hero card | `FavoriteWithArt` (newest) |
| 4 | Favorites: Recently Added | `GameScrollCard` (shared) | Horizontal scroll | `FavoriteWithArt` (items 2-11) |
| 5 | Favorites: All (flat) | `FlatFavorites` / `FavItem` | Vertical list | `FavoriteWithArt` |
| 6 | Favorites: All (grouped) | `GroupedFavorites` / `FavItem` | Grouped vertical list | `FavoriteWithArt` |
| 7 | Favorites: By System cards | inline in `FavoritesContent` | Grid (2-col) | Derived from `FavoriteWithArt` |
| 8 | System Favorites page | `SystemFavoritesContent` / `FavItem` | Vertical list | `FavoriteWithArt` |
| 9 | System ROM list | `RomList` / `RomItem` | Vertical list + infinite scroll | `RomEntry` (paginated) |
| 10 | Global search results | `SearchResults` / `SystemGroup` / `SearchResultItem` | Grouped vertical list | `GlobalSearchResult` |

---

## 2. Detailed Pattern Documentation

### Pattern 1: Home Page -- Last Played Hero Card

**Component:** `HeroCard` (components/hero_card.rs), invoked from `HomePage` (home.rs)
**Data:** `RecentWithArt` -- first item from `get_recents()`
**CSS classes:** `.hero-card`, `.hero-thumb`, `.hero-thumb-placeholder`, `.hero-info`, `.hero-title`, `.hero-system`

```
+--------------------------------------------------------------+
| .hero-card (link to /games/{system}/{rom})                   |
|                                                              |
|  +----------+  +------------------------------------------+ |
|  |          |  | .hero-title                               | |
|  |  .hero-  |  |   "Super Mario World"                    | |
|  |  thumb   |  |                                           | |
|  | 56px h   |  | .hero-system                              | |
|  | <=80px w |  |   "Super Nintendo"                        | |
|  +----------+  +------------------------------------------+ |
|                                                              |
+--------------------------------------------------------------+
```

**Data shown:** Display name (or filename fallback), system display name, box art thumbnail
**Box art:** YES -- `hero-thumb` (56px height, max 80px width) or `.hero-thumb-placeholder` (56x56px gray box)
**Actions:** None (entire card is a link)
**Layout:** Horizontal flex, surface background, 20px padding, 12px border-radius

---

### Pattern 2: Home Page -- Recently Played Horizontal Scroll

**Component:** `GameScrollCard` (components/hero_card.rs), invoked from `HomePage` (home.rs)
**Data:** `RecentWithArt` -- items 2-11 from `get_recents()` (skip first, take 10)
**CSS classes:** `.recent-scroll`, `.recent-item`, `.recent-thumb`, `.recent-thumb-placeholder`, `.recent-name`, `.recent-system`

```
.recent-scroll (overflow-x: auto, flex row, gap 12px)
+------------------+  +------------------+  +------------------+
| .recent-item     |  | .recent-item     |  | .recent-item     |  ...
| 130px wide       |  | 130px wide       |  | 130px wide       |
|                  |  |                  |  |                  |
| +------+         |  | +------+         |  | [placeholder]    |
| | .re- |         |  | | .re- |         |  |  56x80px         |
| | cent-|         |  | | cent-|         |  |  gray box        |
| | thumb|         |  | | thumb|         |  |                  |
| | 80px |         |  | | 80px |         |  |                  |
| +------+         |  | +------+         |  |                  |
|                  |  |                  |  |                  |
| .recent-name     |  | .recent-name     |  | .recent-name     |
| (0.8rem, center) |  | (ellipsis)       |  | (ellipsis)       |
|                  |  |                  |  |                  |
| .recent-system   |  | .recent-system   |  | .recent-system   |
| (0.7rem, muted)  |  | (0.7rem, muted)  |  | (0.7rem, muted)  |
+------------------+  +------------------+  +------------------+
```

**Data shown:** Display name (or filename), system display name, box art thumbnail
**Box art:** YES -- `recent-thumb` (80px height, max 100px width) or `.recent-thumb-placeholder` (56x80px gray box)
**Actions:** None (entire card is a link)
**Layout:** Horizontal scroll, flex-shrink: 0 cards, 130px fixed width, centered text

---

### Pattern 3: Favorites Page -- Latest Added Hero Card

**Component:** `HeroCard` (shared, same as Pattern 1)
**Data:** `FavoriteWithArt` -- newest by `date_added`
**CSS classes:** Same as Pattern 1

**Data shown:** Display name (or filename), system display name, box art thumbnail
**Box art:** YES -- identical to Pattern 1
**Actions:** None (entire card is a link)
**Layout:** Identical to Pattern 1 (shared component)

---

### Pattern 4: Favorites Page -- Recently Added Horizontal Scroll

**Component:** `GameScrollCard` (shared, same as Pattern 2)
**Data:** `FavoriteWithArt` -- items 2-11 sorted by `date_added` descending
**CSS classes:** Same as Pattern 2

**Data shown:** Display name (or filename), system display name, box art thumbnail
**Box art:** YES -- identical to Pattern 2
**Actions:** None (entire card is a link)
**Layout:** Identical to Pattern 2 (shared component)

---

### Pattern 5: Favorites Page -- All Favorites (Flat List)

**Component:** `FlatFavorites` + `FavItem` (favorites.rs)
**Data:** `FavoriteWithArt` -- `FavItem` receives `fav: Favorite` + `box_art_url: Option<String>`
**CSS classes:** `.fav-list`, `.fav-item`, `.fav-info`, `.fav-name`, `.fav-system`, `.fav-star-btn`, `.fav-confirm-actions`, `.rom-thumb-link`, `.rom-thumb`

```
.fav-list (flex column)
+--------------------------------------------------------------+
| .fav-item                                                    |
|                                                              |
|  .rom-thumb-link  .fav-info (flex: 1)    .fav-star-btn      |
|  +------+        +---------------------+  +-----+           |
|  | .rom-|        | .fav-name (link)     |  |     |           |
|  | thumb|        |   "Super Mario Wrld" |  | (*) | gold star |
|  | 40px |        | .fav-system          |  |     |           |
|  +------+        |   "Super Nintendo"   |  +-----+           |
|                  +---------------------+                     |
|                                                              |
+---------- border-bottom: 1px solid var(--border) ------------+
| .fav-item                                                    |
|  ...                                                         |
+--------------------------------------------------------------+
```

When star is clicked, confirmation replaces it:

```
+--------------------------------------------------------------+
| .fav-item                                                    |
|                                                              |
|  [thumb] .fav-info            .fav-confirm-actions           |
|  +------+ +------------------+ +-------------------------+  |
|  | 40px | | "Super Mario..." | | [Remove?]  [x]          |  |
|  +------+ | "Super Nintendo" | +-------------------------+  |
|           +------------------+                               |
+--------------------------------------------------------------+
```

**Data shown:** Display name (or filename), system display name (when `show_system=true`), box art thumbnail
**Box art:** YES (conditional) -- `.rom-thumb` (40px height, max 56px width) shown when `box_art_url` is present. No placeholder when absent (thumbnail area is simply omitted).
**Actions:** Remove from favorites (star button -> confirm -> remove)
**Layout:** Vertical list, flex row per item, 12px padding, border-bottom separators

---

### Pattern 6: Favorites Page -- All Favorites (Grouped by System)

**Component:** `GroupedFavorites` + `FavItem` (favorites.rs)
**Data:** Same as Pattern 5
**CSS classes:** `.fav-grouped`, `.fav-group`, `.fav-group-title`, `.fav-group-count`, plus same `.fav-item` classes

```
.fav-grouped (flex column)
+--------------------------------------------------------------+
| .fav-group                                                   |
|                                                              |
| .fav-group-title                                             |
|   "Super Nintendo (3)"                                       |
| ========================== border-bottom: 2px accent         |
|                                                              |
| .fav-item (show_system=false)                                |
| +----------------------------------------------------+       |
| | [thumb] .fav-name                    .fav-star      |       |
| |         "Super Mario World"             (*)         |       |
| +----------------------------------------------------+       |
| .fav-item                                                    |
| | [thumb] "Chrono Trigger"                (*)         |       |
| +----------------------------------------------------+       |
| .fav-item                                                    |
| | [thumb] "Donkey Kong Country"           (*)         |       |
| +----------------------------------------------------+       |
+--------------------------------------------------------------+
| .fav-group                                                   |
| "PlayStation (2)"                                            |
| ==========================                                   |
| ...                                                          |
+--------------------------------------------------------------+
```

**Data shown:** Display name (or filename), box art thumbnail -- NO system name (grouped already)
**Box art:** YES (conditional, same as Pattern 5)
**Actions:** Remove from favorites (same as Pattern 5)
**Layout:** Grouped vertical list, group headers with accent border

---

### Pattern 7: Favorites Page -- By System Cards Grid

**Component:** Inline in `FavoritesContent` (favorites.rs)
**Data:** Derived aggregation from `FavoriteWithArt` -- system name, count, latest added name
**CSS classes:** `.systems-grid`, `.system-card`, `.system-card-name`, `.system-card-count`, `.system-card-size`

```
.systems-grid (2-col grid, 3 on tablet, 4 on desktop)
+---------------------------+  +---------------------------+
| .system-card (link)       |  | .system-card (link)       |
|                           |  |                           |
| .system-card-name         |  | .system-card-name         |
|   "Super Nintendo"        |  |   "PlayStation"           |
|                           |  |                           |
| .system-card-count        |  | .system-card-count        |
|   "5 favorites"           |  |   "3 favorites"           |
|                           |  |                           |
| .system-card-size         |  | .system-card-size         |
|   "Chrono Trigger"        |  |   "Castlevania: SOTN"     |
| (latest added name)       |  | (latest added name)       |
+---------------------------+  +---------------------------+
```

Note: The `.system-card-size` class is reused here to show the "latest added" game name,
but in the Home page systems grid it shows the actual disk size. This semantic mismatch remains.

**Data shown:** System display name, favorite count, latest added game name
**Box art:** NO
**Actions:** None (entire card is a link to `/favorites/{system}`)
**Layout:** 2-column grid (responsive to 3/4 columns)

---

### Pattern 7b: Home Page -- Systems Grid

**Component:** `SystemCard` (components/system_card.rs) + `EmptySystemCard` (home.rs)
**Data:** `SystemSummary` from `get_systems()`
**CSS classes:** `.systems-grid`, `.system-card`, `.system-card-name`, `.system-card-manufacturer`, `.system-card-count`, `.system-card-size`

```
.systems-grid (2-col grid, 3 on tablet, 4 on desktop)
+---------------------------+  +---------------------------+
| .system-card (link)       |  | .system-card.empty        |
|                           |  | (not clickable)           |
| .system-card-name         |  | .system-card-name         |
|   "Super Nintendo"        |  |   "Virtual Boy"           |
| .system-card-manufacturer |  | .system-card-manufacturer |
|   "Nintendo"              |  |   "Nintendo"              |
| .system-card-count        |  | .system-card-count        |
|   "234 games"             |  |   "No games"              |
| .system-card-size         |  |                           |
|   "1.2 GB"                |  |                           |
+---------------------------+  +---------------------------+
```

**Data shown:** System display name, manufacturer, game count, total size (for non-empty systems)
**Box art:** NO
**Actions:** None (entire card is a link to `/games/{system}`; empty cards are inert divs)
**Layout:** Same 2-column grid as Pattern 7

---

### Pattern 8: System Favorites Page

**Component:** `SystemFavoritesContent` + `FavItem` (favorites.rs)
**Data:** `FavoriteWithArt` filtered to one system
**CSS classes:** Same as Pattern 5: `.fav-list`, `.fav-item`, etc. Plus header: `.rom-header`, `.back-btn`, `.page-title`, `.rom-count`

```
+--------------------------------------------------------------+
| .rom-header                                                  |
|  [< Back]   .page-title: "Super Nintendo"                   |
+--------------------------------------------------------------+
| .rom-count: "5 favorites"                                    |
+--------------------------------------------------------------+
| .fav-list                                                    |
| +----------------------------------------------------------+ |
| | [thumb] GameName                              [star]      | |
| +----------------------------------------------------------+ |
| | [thumb] GameName                              [star]      | |
| +----------------------------------------------------------+ |
+--------------------------------------------------------------+
```

**Data shown:** Display name, box art thumbnail (when available) -- no system name (already scoped)
**Box art:** YES (conditional, same as Pattern 5)
**Actions:** Remove from favorites
**Layout:** Same as Pattern 5 but with `show_system=false`

---

### Pattern 9: System ROM List Page

**Component:** `RomList` / `RomItem` (components/rom_list.rs)
**Data:** `RomEntry` -- paginated via `get_roms_page()`, PAGE_SIZE=100
**CSS classes:** `.rom-list`, `.rom-item`, `.rom-fav-btn`, `.rom-thumb-link`, `.rom-thumb`, `.rom-thumb-placeholder`, `.rom-info`, `.rom-name`, `.rom-path`, `.rom-meta`, `.rom-size`, `.rom-ext`, `.rom-actions`, `.rom-action-btn`

```
+--------------------------------------------------------------+
| .rom-header                                                  |
|  [< Back]   .page-title: "Super Nintendo"                   |
+--------------------------------------------------------------+
| .search-bar                                                  |
|  +--------------------------------------------------------+  |
|  | .search-input: "search within system..."               |  |
|  +--------------------------------------------------------+  |
+--------------------------------------------------------------+
| .search-filters .rom-list-filters                            |
|  [Hide Hacks] [Hide Translations] [Hide Betas] [Genre v]    |
|  [Hide Clones] (arcade only)                                |
+--------------------------------------------------------------+
| .rom-count: "50 / 234 games"                                 |
+--------------------------------------------------------------+
| .rom-list (flex column)                                      |
|                                                              |
| .rom-item                                                    |
| +----------------------------------------------------------+ |
| | (*)  +------+ .rom-info          .rom-meta   .rom-actions| |
| | fav  | .rom-| .rom-name (link)   .rom-size   [Ren] [Del] | |
| | btn  | thumb| "Super Mario Wld"  "1.2 MB"               | |
| |      | 40px | .rom-path          .rom-ext                | |
| |      +------+ "smw.sfc"          ".sfc"                  | |
| +----------------------------------------------------------+ |
|                                                              |
| .rom-item (no art available)                                 |
| +----------------------------------------------------------+ |
| | (*) +------+ .rom-info           .rom-meta   .rom-acts   | |
| |     | plc- | .rom-name           "3.1 MB"   [Ren][Del]  | |
| |     | hldr | .rom-path           ".sfc"                  | |
| |     | 40px |                                              | |
| |     +------+                                              | |
| +----------------------------------------------------------+ |
|                                                              |
| .load-more-sentinel (infinite scroll trigger)                |
| +----------------------------------------------------------+ |
| |              [Load More / Loading...]                    | |
| +----------------------------------------------------------+ |
+--------------------------------------------------------------+
```

Full `.rom-item` detail:

```
+--------------------------------------------------------------+
|  .rom-item                                                   |
|                                                              |
|  .rom-fav-btn   .rom-thumb-link  .rom-info       .rom-meta  |
|  +---+          +--------+      +------------+   +--------+ |
|  |   |          |        |      | .rom-name   |  | .rom-  | |
|  | * |          | [img]  |      |  (link to   |  | size   | |
|  | or|          | 40px h |      |  detail pg) |  | "1.2MB"| |
|  | o |          | <=56px |      |             |  |        | |
|  |   |          |   w    |      | .rom-path   |  | .rom-  | |
|  +---+          |  -or-  |      |  "file.sfc" |  | ext    | |
|                 | [plc-  |      +------------+   | ".sfc" | |
|                 | holder]|                        +--------+ |
|                 | 40x40  |                                   |
|                 +--------+                                   |
|                                                              |
|  .rom-actions (hover-reveal on desktop, always on touch)     |
|  +-----+-----+                                              |
|  | [P] | [X] |  <-- Rename, Delete                          |
|  +-----+-----+                                              |
|                                                              |
+--------------------------------------------------------------+
```

**Data shown:** Display name (or filename), file path, file size, file extension, favorite status, box art
**Box art:** YES -- `.rom-thumb` (40px height, max 56px width) when available, or `.rom-thumb-placeholder` (40x40px gray box with 3px border-radius) when not
**Actions:** Favorite toggle (star), rename (pencil icon), delete (X icon)
**Layout:** Vertical list, flex row per item, infinite scroll with IntersectionObserver

---

### Pattern 10: Global Search Results

**Component:** `SearchResults` / `SystemGroup` / `SearchResultItem` (pages/search.rs)
**Data:** `GlobalSearchResults` -> `SystemSearchGroup` -> `GlobalSearchResult`
**CSS classes:** `.search-groups`, `.search-group`, `.search-group-header`, `.search-group-title`, `.search-see-all`, `.search-group-results`, `.search-result-item`, `.rom-fav-btn`, `.search-result-thumb-link`, `.search-result-thumb`, `.search-result-thumb-placeholder`, `.search-result-info`, `.search-result-name`, `.search-result-badges`, `.search-badge`, `.search-badge-genre`

```
.search-page
+--------------------------------------------------------------+
| .search-page-bar                                             |
|  +--------------------------------------------------------+  |
|  | .search-page-input: "mario" (larger, 2px border)       |  |
|  +--------------------------------------------------------+  |
+--------------------------------------------------------------+
| (empty state when no query: recent searches + random game)   |
+--------------------------------------------------------------+
| .search-filters                                              |
|  [Hide Hacks] [Hide Trans.] [Hide Betas] [Hide Clones]      |
|  [Genre v]                                                   |
+--------------------------------------------------------------+
| .search-summary: "42 results in 5 systems"                   |
+--------------------------------------------------------------+
| .search-groups (flex column, gap 20px)                       |
|                                                              |
| .search-group (card with border-radius)                      |
| +----------------------------------------------------------+ |
| | .search-group-header                                     | |
| |  "Super Nintendo (12)"              "See all ->"         | |
| +-- border-bottom ---------------------------------------- + |
| | .search-group-results                                    | |
| |                                                          | |
| | .search-result-item                                      | |
| | +------------------------------------------------------+ | |
| | | (*) +------+ .search-result-info                     | | |
| | | fav | .se- | .search-result-name (link)              | | |
| | | btn | arch-|   "Super Mario World"                   | | |
| | |     | resu-| .search-result-badges                   | | |
| | |     | lt-  |   [Platformer]                          | | |
| | |     | thumb|   genre badge                           | | |
| | |     | 40px |                                         | | |
| | |     +------+                                         | | |
| | +------------------------------------------------------+ | |
| |                                                          | |
| | .search-result-item (no art)                             | |
| | +------------------------------------------------------+ | |
| | | (*) +------+ .search-result-info                     | | |
| | |     | plc- | .search-result-name: "Super Mario RPG"  | | |
| | |     | hldr | .search-result-badges: [RPG]            | | |
| | |     | 40px |                                         | | |
| | |     +------+                                         | | |
| | +------------------------------------------------------+ | |
| +----------------------------------------------------------+ |
|                                                              |
| .search-group                                                |
| +----------------------------------------------------------+ |
| | "Arcade (8)"                        "See all ->"         | |
| | ...                                                      | |
| +----------------------------------------------------------+ |
+--------------------------------------------------------------+
```

**Data shown:** Display name, genre (as badge), box art, favorite status (interactive star)
**Box art:** YES -- `.search-result-thumb` (40px height, max 56px width) when available, or `.search-result-thumb-placeholder` (40x40px gray box) when not
**Actions:** Favorite toggle (star button with optimistic toggle), link to detail page, "See all" link to system ROM list with filters preserved
**Layout:** Grouped by system in cards, top 3 results per system, vertical list within each group

---

## 3. Data Structures Comparison

| Field | RecentWithArt | FavoriteWithArt | RomEntry | GlobalSearchResult | GameInfo (detail) |
|-------|:---:|:---:|:---:|:---:|:---:|
| display_name | via GameRef | via GameRef | via GameRef | direct | direct |
| rom_filename | via GameRef | via GameRef | via GameRef | direct | direct |
| system | via GameRef | via GameRef | via GameRef | direct | direct |
| system_display | via GameRef | via GameRef | via GameRef | - | direct |
| rom_path | via GameRef | via GameRef | via GameRef | direct | direct |
| box_art_url | direct | direct | direct | direct | direct |
| size_bytes | - | - | direct | - | direct |
| is_favorite | - | - | direct | direct | direct |
| genre | - | - | - | direct | direct |
| year | - | - | - | - | direct |
| developer | - | - | - | - | direct |
| publisher | - | - | - | - | direct (external) |
| players | - | - | - | - | direct |
| rating | - | - | - | - | direct (external) |
| rotation | - | - | - | - | direct (arcade) |
| driver_status | - | - | - | - | direct (arcade) |
| is_clone | - | - | - | - | direct (arcade) |
| region | - | - | - | - | direct (console) |
| description | - | - | - | - | direct (external) |
| is_m3u | - | - | direct | - | direct |
| date_added | via Favorite | via Favorite | - | - | - |
| last_played | via RecentEntry | - | - | - | - |
| marker_filename | via RecentEntry | via Favorite | - | - | - |
| subfolder | - | via Favorite | - | - | - |
| screenshot_url | - | - | - | - | direct |

---

## 4. Inconsistencies and Issues

### Issue A: Box Art Dropped in FavItem -- RESOLVED

~~`FavItem` receives `fav: Favorite` (not `FavoriteWithArt`), so the `box_art_url`
is stripped before the component sees it.~~

**Status:** FIXED. `FavItem` now accepts a separate `box_art_url: Option<String>` prop.
All call sites (`FlatFavorites`, `GroupedFavorites`, `SystemFavoritesContent`) pass
`f.box_art_url`. Thumbnails render using `.rom-thumb-link` / `.rom-thumb` with
`<Show when=move || has_box_art>`. Items without box art omit the thumbnail space
entirely (no placeholder in favorites lists).

### Issue B: No Placeholder in ROM List and Search Results -- RESOLVED

~~When box art is not available, the thumbnail is simply omitted, causing inconsistent
row heights.~~

**Status:** FIXED. Both `RomItem` and `SearchResultItem` now always render the
thumb-link area: an `<img>` when box art exists, or a placeholder div when not.

- `RomItem` uses `.rom-thumb-placeholder` (40x40px, `var(--border)` background, 3px radius)
- `SearchResultItem` uses `.search-result-thumb-placeholder` (40x40px, same styling)

All items in the ROM list and search results now have consistent row heights regardless
of box art availability.

### Issue C: Semantic Class Name Reuse -- REMAINS (LOW)

In the favorites "By System" cards (Pattern 7), the class `.system-card-size` is reused
to display the "latest added game name" rather than a disk size. On the Home page's
systems grid (Pattern 7b), the same class shows actual disk size via the `SystemCard`
component. Semantically confusing, though visually it works fine since both are small
muted text.

### Issue D: Duplicated Hero Card Rendering Logic -- RESOLVED

~~The hero card pattern is copy-pasted identically between `home.rs` and `favorites.rs`.~~

**Status:** FIXED. Extracted to `HeroCard` component in `src/components/hero_card.rs`.
Both pages now use `<HeroCard href name system box_art_url />`.

### Issue E: Duplicated Horizontal Scroll Item Logic -- RESOLVED

~~The recent-scroll item pattern is copy-pasted identically between `home.rs` and `favorites.rs`.~~

**Status:** FIXED. Extracted to `GameScrollCard` component in `src/components/hero_card.rs`.
Both pages now use `<GameScrollCard href name system box_art_url />`.

### Issue F: Partially Duplicated Filter Bar Logic -- PARTIALLY RESOLVED

The `GenreDropdown` component was extracted to `src/components/genre_dropdown.rs` and
is shared between `rom_list.rs` and `search.rs`.

**Remaining:** The filter chips themselves (Hide Hacks, Hide Translations, Hide Betas,
Hide Clones) are still implemented independently in both `rom_list.rs` and `search.rs`.
Each file defines its own set of `RwSignal<bool>` signals and renders its own set of
`<button class="filter-chip">` elements with identical structure and styling. Extracting
a shared `FilterChips` component would further reduce duplication but is low priority
since the logic is straightforward and context-dependent (the ROM list conditionally
shows "Hide Clones" only for arcade systems via `<Show when=move || is_arcade.get()>`,
while search always shows it).

### Issue G: Actions Consistency -- PARTIALLY RESOLVED

| View | Favorite Toggle | Delete | Rename | Launch | Link to Detail |
|------|:---:|:---:|:---:|:---:|:---:|
| Hero card (`HeroCard`) | - | - | - | - | YES (entire card) |
| Scroll card (`GameScrollCard`) | - | - | - | - | YES (entire card) |
| FavItem (all fav lists) | Unfavorite | - | - | - | YES (name + thumb link) |
| RomItem | Toggle | Delete | Rename | - | YES (name + thumb link) |
| SearchResultItem | Toggle | - | - | - | YES (name + thumb link) |
| GameDetailContent | Toggle | Delete | Rename | Launch | N/A (is the detail) |

**Changes from previous analysis:**
- Search results now have an interactive favorite toggle (was read-only badge before)
- FavItem now has clickable thumbnails linking to game detail (previously text-only)

**Remaining observations:**
- Hero cards and scroll cards still have no quick-favorite action (appropriate for their
  compact layouts)
- FavItem has no delete or rename -- only unfavorite (appropriate for favorites context)
- Only the game detail page has the Launch action

### Issue H: Different Item Heights and Structures -- IMPROVED

| View | Row Height (approx) | Thumb Size | Placeholder | Name Font | Path Shown |
|------|---------------------|-----------|-------------|-----------|-----------|
| Hero card | ~96px | 56px h | 56x56px gray | 1.1rem bold | No |
| Scroll card | ~160px | 80px h | 56x80px gray | 0.8rem | No |
| FavItem (with art) | ~60px | 40px h | None | 0.85rem | No |
| FavItem (no art) | ~44px | N/A | N/A | 0.85rem | No |
| RomItem | ~60px | 40px h | 40x40px gray | 0.85rem | Yes |
| SearchResultItem | ~60px | 40px h | 40x40px gray | 0.85rem | No |

**Changes from previous:** FavItem with box art now has a similar row height to RomItem and
SearchResultItem. FavItem without box art is still shorter (no placeholder), creating some
height inconsistency within the favorites list when box art coverage is mixed.

---

## 5. Visual Summary: All Patterns Side by Side

```
HOME PAGE                          FAVORITES PAGE
+----------------------------+     +----------------------------+
| [Search link (fake bar)]   |     |                            |
|                            |     | LATEST ADDED               |
| LAST PLAYED                |     | +------------------------+ |
| +------------------------+ |     | | [art] Title            | |
| | [art] Title            | |     | |        System          | |
| |        System          | |     | +------------------------+ |
| +------------------------+ |     |                            |
|  (HeroCard component)      |     | RECENTLY ADDED             |
|                            |     | +----+ +----+ +----+      |
| RECENTLY PLAYED            |     | |art | |art | |art | -->  |
| +----+ +----+ +----+      |     | |name| |name| |name|      |
| |art | |art | |art | -->  |     | |sys | |sys | |sys |      |
| |name| |name| |name|      |     | +----+ +----+ +----+      |
| |sys | |sys | |sys |      |     |  (GameScrollCard component) |
| +----+ +----+ +----+      |     |                            |
|  (GameScrollCard comp.)    |     | STATS: [N favs] [N sys]   |
|                            |     |                            |
| LIBRARY                   |     | [> Organize favorites]     |
| [Games] [Systems] [Favs]  |     |                            |
| [Storage bar]              |     | BY SYSTEM (grid)           |
|                            |     | +--------+ +--------+     |
| SYSTEMS (grid)             |     | |SysName | |SysName |     |
| +--------+ +--------+     |     | |N favs  | |N favs  |     |
| |SysName | |SysName |     |     | |Latest  | |Latest  |     |
| |Maker   | |Maker   |     |     | +--------+ +--------+     |
| |N games | |N games |     |     |                            |
| |Size    | |Size    |     |     | ALL FAVORITES [Flat|Group] |
| +--------+ +--------+     |     | +------------------------+ |
| (SystemCard component)     |     | |[t] GameName     [star] | |
| ...                        |     | |    System              | |
+----------------------------+     | +------------------------+ |
                                   | |[t] GameName     [star] | |
                                   | |    System              | |
                                   | +------------------------+ |
                                   |  (FavItem component)       |
                                   +----------------------------+

ROM LIST PAGE                      SEARCH PAGE
+----------------------------+     +----------------------------+
| [< Back]  System Name     |     | [Search input, larger]     |
| [Search within system]     |     |                            |
| [Hacks][Trans][Betas]      |     | (empty: recent searches    |
| [Clones*][Genre v]         |     |  + random game button)     |
| "50 / 234 games"           |     |                            |
|                            |     | [Hacks][Trans][Betas]      |
| +------------------------+ |     | [Clones][Genre v]          |
| |(*) [t] GameName  1.2MB | |     |                            |
| |        path.sfc  .sfc  | |     | "42 results in 5 systems"  |
| |              [Ren][Del] | |     |                            |
| +------------------------+ |     | +------------------------+ |
| |(*) [p] GameName    3MB | |     | | SystemA (12)  See all >| |
| |        path.sfc  .sfc  | |     | +------------------------+ |
| |              [Ren][Del] | |     | |(*) [t] GameName        | |
| +------------------------+ |     | |         [Genre]         | |
| |(*) [t] GameName  800KB | |     | +------------------------+ |
| |        path.sfc  .sfc  | |     | |(*) [p] GameName        | |
| |              [Ren][Del] | |     | |         [Genre]         | |
| +------------------------+ |     | +------------------------+ |
| ...                        |     |                            |
| [Load more / auto-scroll]  |     | +------------------------+ |
+----------------------------+     | | SystemB (8)   See all >| |
                                   | | ...                      | |
                                   | +------------------------+ |
                                   +----------------------------+

(*) = fav star (interactive)   [t] = thumbnail (when art exists)
[p] = placeholder (40x40 gray)  [Ren] = rename    [Del] = delete
* = Clones filter shown only for arcade systems
(GenreDropdown shared component)
```

---

## 6. Completed Improvements

### 6.1 Add Box Art Thumbnails to FavItem -- DONE

**Problem:** The all-favorites list was the most text-heavy, least visual part of the app.

**What changed:** Added `box_art_url: Option<String>` prop to `FavItem`. All call sites
(`FlatFavorites`, `GroupedFavorites`, `SystemFavoritesContent`) now pass `f.box_art_url`.
Thumbnail uses `.rom-thumb-link` / `.rom-thumb` classes with `<Show>` for conditional
rendering. Items with box art now show a 40px thumbnail before the game name.

### 6.2 Extract Shared HeroCard Component -- DONE

**Problem:** Hero card logic was copy-pasted in `home.rs` and `favorites.rs`.

**What changed:** Created `src/components/hero_card.rs` with `HeroCard` component.
Updated `home.rs` and `favorites.rs` to use `<HeroCard>` instead of inline rendering.
Added `pub mod hero_card;` to `src/components/mod.rs`.

### 6.3 Extract Shared GameScrollCard Component -- DONE

**Problem:** Horizontal scroll item logic was copy-pasted in `home.rs` and `favorites.rs`.

**What changed:** Created `GameScrollCard` component in `hero_card.rs` alongside `HeroCard`.
Updated `home.rs` and `favorites.rs` to use `<GameScrollCard>` instead of inline rendering.

### 6.4 Add Placeholder Thumbnails in ROM List and Search -- DONE

**Problem:** Items without box art had no thumbnail space, causing inconsistent row alignment.

**What changed:** `RomItem` now always renders the thumb-link area with either an image
or a `.rom-thumb-placeholder` div. `SearchResultItem` does the same with
`.search-result-thumb-placeholder`. Added CSS for both placeholder classes (40x40px
gray box with 3px border-radius).

### 6.5 Deduplicate GenreDropdown Component -- DONE

**Problem:** `GenreDropdown` was defined identically in both `rom_list.rs` and `search.rs`.

**What changed:** Created `src/components/genre_dropdown.rs` with the shared `GenreDropdown`
component. Removed local definitions from both `rom_list.rs` and `search.rs`. Both now
import from `crate::components::genre_dropdown::GenreDropdown`. Added `pub mod genre_dropdown;`
to `src/components/mod.rs`.

### 6.6 Add Quick-Favorite to Search Results -- DONE

**Problem:** Search results showed the favorite status as a read-only badge with no way
to toggle it.

**What changed:** Added `rom_path` field to `GlobalSearchResult` struct and populated it
in the server-side search function. `SearchResultItem` now has a `.rom-fav-btn` button
with optimistic toggle using the same pattern as `RomItem` (calls `add_favorite` /
`remove_favorite` server functions). The read-only star badge was removed.

---

## 7. Information Fields Analysis by User Persona

This section analyzes what additional information fields would add value in game lists,
cross-referenced with the five user personas defined in the user analysis.

### Available Data Fields

The following metadata fields exist in the data model but are shown in varying degrees
across views:

| Field | Currently in detail | In ROM list | In search | In favorites | In recents |
|-------|:---:|:---:|:---:|:---:|:---:|
| Display name | YES | YES | YES | YES | YES |
| System name | YES | implicit (page) | YES (group header) | YES/implicit | YES |
| Box art | YES (full) | YES (40px) | YES (40px) | YES (40px) | YES (56-80px) |
| File size | YES | YES | - | - | - |
| File extension | YES | YES | - | - | - |
| File path | YES | YES | - | - | - |
| Genre | YES | - | YES (badge) | - | - |
| Year | YES | - | - | - | - |
| Developer | YES | - | - | - | - |
| Publisher | YES (ext.) | - | - | - | - |
| Players | YES | - | - | - | - |
| Rating | YES (ext.) | - | - | - | - |
| Description | YES (ext.) | - | - | - | - |
| Region | YES (console) | - | - | - | - |
| Rotation | YES (arcade) | - | - | - | - |
| Driver status | YES (arcade) | - | - | - | - |
| Clone/parent | YES (arcade) | - | - | - | - |
| Arcade category | YES (arcade) | - | - | - | - |
| Favorite status | YES | YES (star) | YES (star) | implicit | - |
| Last played | - | - | - | - | implicit (order) |
| Date added (fav) | - | - | - | implicit (order) | - |

### Field-by-Field Analysis

#### Play Time

**Currently available:** No. RePlayOS does not track play time. The `last_played`
timestamp from recents is the epoch of the last launch, not cumulative duration.

**Persona value:**
- A (Casual): Medium -- would like knowing "I've played this for 12 hours"
- B (Collector): High -- would help curate and identify favorites by actual usage
- C (Parent): Medium -- would help monitor kids' gaming habits
- D (Arcade): Low -- arcade sessions are short; less meaningful
- E (Technical): Low -- tangential to system management

**Recommended contexts:** Game detail page (if ever tracked). Not suitable for list views
since the data does not currently exist at any level. Would require RePlayOS core changes
to log session durations.

**Verdict:** Not actionable without upstream RePlayOS support. Worth considering if
RePlayOS adds session tracking.

#### Last Played Date

**Currently available:** Yes, in `RecentEntry.last_played` (epoch timestamp). Currently
used only for sort order on the home page; the actual date/time is never displayed.

**Persona value:**
- A (Casual): Medium -- "when did I last play this?" helps rediscovery
- B (Collector): Medium -- tracking engagement over time
- C (Parent): High -- "what did the kids play today?"
- D (Arcade): Low -- less relevant for arcade browsing
- E (Technical): Low

**Recommended contexts:**
- Game detail page: Show "Last played: 3 days ago" or similar relative date. Low effort,
  high value for Persona A and C. The data already exists in the recents file.
- Home page recently played cards: A subtle "2h ago" or "Yesterday" label would add
  context without cluttering the compact card. Would need to propagate `last_played`
  to the view layer.
- ROM list / Search: Not recommended. Too much visual noise for a browsing context.
- Favorites: Could show "Last played: March 5" in the favorites detail, but favorites
  data does not carry `last_played` (would need a join with recents data).

**Verdict:** Show on game detail page (easy, high value). Consider adding to recently
played cards as a subtle subtitle.

#### Genre

**Currently available:** Yes, from arcade_db and game_db. Currently shown as badge in
search results and as a metadata field on the game detail page. The genre dropdown
filter exists for both ROM list and search.

**Persona value:**
- A (Casual): Medium -- helps browse by mood ("I want a platformer")
- B (Collector): High -- essential for organization and curation
- C (Parent): Medium -- can filter for appropriate genres
- D (Arcade): High -- critical for cabinet game selection (fighters, shooters, etc.)
- E (Technical): Low

**Recommended contexts:**
- ROM list: Show genre as a subtle badge or secondary text on each `RomItem`. The data
  is available in `GameInfo` via `resolve_game_info()` but is not propagated to `RomEntry`.
  Would require adding a `genre` field to `RomEntry` / `GameRef`.
- Favorites list: Show genre as a secondary badge on `FavItem`. Would require adding
  `genre` to the favorites data path (`FavoriteWithArt`).
- Search results: Already shown as badge -- good.
- Game detail: Already shown -- good.

**Verdict:** Adding genre to `RomEntry` and showing it in ROM list items would benefit
Personas B and D. Medium effort (schema change + view update). High priority for
improving browse-by-genre workflows.

#### Developer / Publisher

**Currently available:** Yes. Developer from arcade_db (as `manufacturer`) and game_db.
Publisher from external LaunchBox metadata (optional import).

**Persona value:**
- A (Casual): Low -- rarely cares about developer names
- B (Collector): Medium -- interesting for organization but not a primary need
- C (Parent): Low
- D (Arcade): Medium -- manufacturer is meaningful for arcade (Capcom, SNK, etc.)
- E (Technical): Low

**Recommended contexts:**
- Game detail page: Already shown -- good.
- ROM list / Search: Not recommended for list views. Too many text fields would
  clutter compact rows. If shown, it should be a very subtle secondary text.
- Favorites: Not recommended.

**Verdict:** Keep on detail page only. Not worth the visual clutter in list views.

#### Rating (LaunchBox)

**Currently available:** Yes, from LaunchBox external metadata (0.0-5.0 scale). Requires
metadata import. Shown on game detail page as "X.X / 5.0".

**Persona value:**
- A (Casual): High -- "is this game any good?" is a fundamental question
- B (Collector): High -- helps prioritize what to play from a large collection
- C (Parent): High -- helps pick quality games for kids
- D (Arcade): Medium -- community ratings may not reflect cabinet experience
- E (Technical): Low

**Recommended contexts:**
- ROM list: A small star rating or numeric badge next to the game name would help with
  discovery. Would require adding `rating` to `RomEntry`.
- Search results: A rating badge alongside genre would add discovery value. Would
  require adding `rating` to `GlobalSearchResult`.
- Favorites list: Less useful (you already chose to favorite it).
- Game detail: Already shown -- good.
- Home page hero card / recents: Not recommended (too compact).

**Verdict:** High value for Personas A, B, and C. Adding a compact rating indicator
to ROM list items and search results would significantly improve browse-and-discover
workflows. Medium effort (schema changes + view updates). Depends on metadata being
imported, so should degrade gracefully (only show when rating exists).

#### Release Year

**Currently available:** Yes, from arcade_db and game_db. Shown on game detail page.

**Persona value:**
- A (Casual): Medium -- nostalgic value ("games from my childhood in 1994")
- B (Collector): High -- essential for browsing chronologically, identifying eras
- C (Parent): Low
- D (Arcade): Medium -- useful for identifying game generations
- E (Technical): Low

**Recommended contexts:**
- ROM list: A subtle year badge could enable chronological browsing. The data exists
  in `GameInfo` but is not propagated to `RomEntry`. Would pair well with a "sort by year"
  feature (currently missing).
- Search results: Potentially useful as a badge alongside genre.
- Game detail: Already shown -- good.

**Verdict:** Adding year to list views is lower priority than genre or rating, but
would become high value if sort-by-year is implemented. Keep on detail page for now.

#### File Size

**Currently available:** Yes, in `RomEntry.size_bytes`. Shown in ROM list items and
game detail page.

**Persona value:**
- A (Casual): Low -- irrelevant to gameplay
- B (Collector): Medium -- helps manage storage, identify corrupt/incomplete files
- C (Parent): Low
- D (Arcade): Low
- E (Technical): Medium -- useful for storage management

**Recommended contexts:**
- ROM list: Already shown -- appropriate for this file-management context.
- Search results: Not shown, not needed (search is about finding, not managing).
- Favorites: Not shown, not needed (favorites is about curation, not file management).
- Game detail: Already shown -- appropriate.

**Verdict:** Current placement is correct. No changes needed.

#### Save State Availability

**Currently available:** No. RePlayOS save states are managed by the emulator cores
(RetroArch), and the companion app has no visibility into which games have save states.

**Persona value:**
- A (Casual): High -- "can I resume where I left off?" is critical
- B (Collector): Medium
- C (Parent): Medium -- kids care about progress
- D (Arcade): Low -- arcade games rarely use save states
- E (Technical): Low

**Recommended contexts:** Would be useful as a small icon/badge in ROM list items
and on the game detail page, but requires filesystem scanning for RetroArch save files
(`.state`, `.srm` files in the saves directory).

**Verdict:** High value but requires significant backend work to detect save files.
Not currently feasible without understanding the RetroArch save file layout on RePlayOS.

#### Compatibility Status (Arcade Driver Status)

**Currently available:** Yes, for arcade systems only. `driver_status` field in
`GameInfo` with values "Working", "Imperfect", "Preliminary", "Unknown".

**Persona value:**
- A (Casual): Low -- does not know what driver status means
- B (Collector): Medium -- wants to know if a game works
- C (Parent): Low
- D (Arcade): Very High -- critical for curating a playable cabinet set
- E (Technical): Medium -- useful for debugging

**Recommended contexts:**
- ROM list (arcade systems): A small colored badge (green/yellow/red) for Working/
  Imperfect/Preliminary would be extremely valuable for Persona D, who needs to
  quickly identify playable games from thousands of arcade ROMs. The data exists in
  `GameInfo` but is not propagated to `RomEntry`.
- Search results: A similar badge for arcade results would help.
- Game detail: Already shown as text -- good, but could be enhanced with color.
- Non-arcade systems: N/A.

**Verdict:** High priority specifically for arcade users. Adding a driver status badge
to ROM list items for arcade systems would be a significant UX improvement for Persona D.
Medium effort (add to `RomEntry` for arcade, conditional rendering).

#### Number of Players

**Currently available:** Yes, from arcade_db and game_db. Shown on game detail page.

**Persona value:**
- A (Casual): Medium -- "is this a 2-player game?"
- B (Collector): Low
- C (Parent): High -- "can the kids play together?"
- D (Arcade): High -- critical for cabinet setup (2P fighters, 4P beat-em-ups)
- E (Technical): Low

**Recommended contexts:**
- ROM list: A small "2P" or "4P" badge would be useful but adds visual clutter.
  Best as a filter option rather than a displayed field.
- Search results: Could be a badge alongside genre for multiplayer games.
- Game detail: Already shown -- good.

**Verdict:** More valuable as a filter than as a displayed field. A "Multiplayer only"
filter chip on the ROM list page would serve Personas C and D well. Medium effort.

### Summary: Recommended Additions by Priority

| Priority | Field | Where to Add | Effort | Primary Personas |
|----------|-------|-------------|--------|-----------------|
| 1 | Genre | ROM list items | Medium (schema + view) | B, D |
| 2 | Rating | ROM list items, search results | Medium (schema + view) | A, B, C |
| 3 | Driver status badge | ROM list items (arcade only) | Medium | D |
| 4 | Last played date | Game detail page | Small | A, C |
| 5 | Year | ROM list items (subtle) | Medium (schema + view) | B |
| 6 | Players filter | ROM list filter bar | Medium | C, D |
| 7 | Save state indicator | ROM list items, detail | Large (backend) | A, C |

Note: Priorities 1-3 all require adding fields to `RomEntry` (or `GameRef`), which
means a schema change that flows through both SSR and WASM targets. These could be
batched into a single change to minimize the structural disruption.

---

## 8. Remaining Inconsistencies and Future Improvements

### 8.1 FavItem Thumbnail Inconsistency (LOW)

`FavItem` shows thumbnails when box art exists but has no placeholder when it does not.
This means favorites lists have mixed-height rows: ~60px for items with art, ~44px for
items without. In contrast, `RomItem` and `SearchResultItem` always allocate thumbnail
space (either image or placeholder), giving them consistent row heights.

Adding a `.fav-thumb-placeholder` (or reusing `.rom-thumb-placeholder`) to `FavItem`
would align it with the ROM list behavior. Low priority since favorites tend to have
higher box art coverage (users favorite games they care about, which are more likely
to have metadata imported).

### 8.2 Filter Chip Duplication (LOW)

The filter chip buttons (Hide Hacks, Hide Translations, Hide Betas, Hide Clones) are
still independently implemented in `rom_list.rs` and `search.rs`. Extracting a shared
`FilterChips` component would reduce duplication, but the conditional logic differs
slightly between the two contexts (arcade-only clone filter in ROM list), so the
extraction is not entirely trivial.

### 8.3 No Search in Favorites (MEDIUM)

The favorites page has no search functionality. With hundreds of favorites, finding a
specific game requires scrolling through the flat/grouped list. Adding a client-side
filter (since all favorites are already loaded into the `RwSignal<Vec<FavoriteWithArt>>`)
would be straightforward and benefit all personas.

### 8.4 No Sort Options in ROM Lists (MEDIUM)

Games in ROM lists are always sorted alphabetically. There is no option to sort by year,
size, genre, or other metadata fields. The data exists in the embedded databases but
is not exposed as sort options. This is a significant gap for discovery workflows
(Personas A, B, D).

### 8.5 Semantic Mismatch in System Cards (LOW)

The `.system-card-size` class is used for disk size on the home page systems grid
(`SystemCard` component) and for the "latest added game name" on the favorites page
systems grid (inline rendering). The same CSS class serves two different semantic purposes.
A rename or a dedicated class for the favorites variant would be cleaner.

### 8.6 No Genre/Rating in ROM List Items

As detailed in Section 7, the ROM list (`RomItem`) shows only file-centric metadata
(size, extension, path) but no game-centric metadata (genre, year, rating). This
information exists in the detail view but is absent from the browse view where it
would be most useful for discovery.

---

## 9. Completed Improvements Summary

| # | Improvement | Priority | Effort | Visual Impact | Status |
|---|-------------|----------|--------|---------------|--------|
| 6.1 | Box art in FavItem | HIGH | Small | Large -- transforms favorites list | DONE |
| 6.2 | Extract HeroCard | MEDIUM | Small | None (refactor) | DONE |
| 6.3 | Extract GameScrollCard | MEDIUM | Small | None (refactor) | DONE |
| 6.4 | Placeholders in ROM/search lists | LOW | Small | Medium -- consistent alignment | DONE |
| 6.5 | Deduplicate GenreDropdown | LOW | Trivial | None (refactor) | DONE |
| 6.6 | Quick-favorite in search | LOW | Medium | Small -- convenience feature | DONE |

All six originally proposed improvements have been implemented.
