# Game List UI/UX Patterns Analysis

Comprehensive analysis of every place in the RePlayOS Companion App where games
are displayed as lists, scrolls, grids, cards, or items.

Source files analyzed:

- `replay-control-app/src/pages/home.rs`
- `replay-control-app/src/pages/favorites.rs`
- `replay-control-app/src/pages/search.rs`
- `replay-control-app/src/pages/game_detail.rs`
- `replay-control-app/src/components/rom_list.rs`
- `replay-control-app/src/server_fns.rs`
- `replay-control-app/src/types.rs`
- `replay-control-app/style/style.css`

---

## 1. Inventory of Every Game List

| # | Location | Component | Layout | Data Source |
|---|----------|-----------|--------|-------------|
| 1 | Home: Last Played | inline in `HomePage` | Hero card | `RecentWithArt` (first item) |
| 2 | Home: Recently Played | inline in `HomePage` | Horizontal scroll | `RecentWithArt` (items 2-11) |
| 3 | Favorites: Latest Added | inline in `FavoritesContent` | Hero card | `FavoriteWithArt` (newest) |
| 4 | Favorites: Recently Added | inline in `FavoritesContent` | Horizontal scroll | `FavoriteWithArt` (items 2-11) |
| 5 | Favorites: All (flat) | `FlatFavorites` / `FavItem` | Vertical list | `FavoriteWithArt` |
| 6 | Favorites: All (grouped) | `GroupedFavorites` / `FavItem` | Grouped vertical list | `FavoriteWithArt` |
| 7 | Favorites: By System cards | inline in `FavoritesContent` | Grid (2-col) | Derived from `FavoriteWithArt` |
| 8 | System Favorites page | `SystemFavoritesContent` / `FavItem` | Vertical list | `FavoriteWithArt` |
| 9 | System ROM list | `RomList` / `RomItem` | Vertical list + infinite scroll | `RomEntry` (paginated) |
| 10 | Global search results | `SearchResults` / `SystemGroup` / `SearchResultItem` | Grouped vertical list | `GlobalSearchResult` |

---

## 2. Detailed Pattern Documentation

### Pattern 1: Home Page -- Last Played Hero Card

**Component:** Inline in `HomePage` (home.rs, lines 70-101)
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
**Box art:** YES -- `hero-thumb` (56px height, max 80px width) or placeholder
**Actions:** None (entire card is a link)
**Layout:** Horizontal flex, surface background, 20px padding, 12px border-radius

---

### Pattern 2: Home Page -- Recently Played Horizontal Scroll

**Component:** Inline in `HomePage` (home.rs, lines 104-139)
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
| | cent-|         |  | | cent-|         |  |                  |
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
**Box art:** YES -- `recent-thumb` (80px height, max 100px width) or placeholder
**Actions:** None (entire card is a link)
**Layout:** Horizontal scroll, flex-shrink: 0 cards, 130px fixed width, centered text

---

### Pattern 3: Favorites Page -- Latest Added Hero Card

**Component:** Inline in `FavoritesContent` (favorites.rs, lines 138-158)
**Data:** `FavoriteWithArt` -- newest by `date_added`
**CSS classes:** Same as Pattern 1: `.hero-card`, `.hero-thumb`, etc.

```
+--------------------------------------------------------------+
| .hero-card (link to /games/{system}/{rom})                   |
|                                                              |
|  +----------+  +------------------------------------------+ |
|  |  .hero-  |  | .hero-title                               | |
|  |  thumb   |  |   "Castlevania: SOTN"                    | |
|  | 56px h   |  | .hero-system                              | |
|  | <=80px w |  |   "PlayStation"                           | |
|  +----------+  +------------------------------------------+ |
|                                                              |
+--------------------------------------------------------------+
```

**Data shown:** Display name (or filename), system display name, box art thumbnail
**Box art:** YES -- identical to Pattern 1
**Actions:** None (entire card is a link)
**Layout:** Identical to Pattern 1 (reuses same CSS classes)

---

### Pattern 4: Favorites Page -- Recently Added Horizontal Scroll

**Component:** Inline in `FavoritesContent` (favorites.rs, lines 161-183)
**Data:** `FavoriteWithArt` -- items 2-11 sorted by `date_added` descending
**CSS classes:** Same as Pattern 2: `.recent-scroll`, `.recent-item`, etc.

```
(Identical layout to Pattern 2)
```

**Data shown:** Display name (or filename), system display name, box art thumbnail
**Box art:** YES -- identical to Pattern 2
**Actions:** None (entire card is a link)
**Layout:** Identical to Pattern 2 (reuses same CSS classes)

---

### Pattern 5: Favorites Page -- All Favorites (Flat List)

**Component:** `FlatFavorites` + `FavItem` (favorites.rs, lines 244-380)
**Data:** `FavoriteWithArt` but **only `Favorite` is passed to `FavItem`** (box_art_url is dropped!)
**CSS classes:** `.fav-list`, `.fav-item`, `.fav-info`, `.fav-name`, `.fav-system`, `.fav-star-btn`, `.fav-confirm-actions`

```
.fav-list (flex column)
+--------------------------------------------------------------+
| .fav-item                                                    |
|                                                              |
|  .fav-info (flex: 1)             .fav-star-btn              |
|  +------------------------------+  +-----+                  |
|  | .fav-name (link)             |  |     |                  |
|  |   "Super Mario World"        |  | (*) |  <-- gold star   |
|  |                              |  |     |                  |
|  | .fav-system                  |  +-----+                  |
|  |   "Super Nintendo"           |                            |
|  +------------------------------+                            |
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
|  .fav-info                 .fav-confirm-actions              |
|  +---------------------+  +---------------------------+     |
|  | "Super Mario World" |  | [Remove?]  [x]            |     |
|  | "Super Nintendo"    |  +---------------------------+     |
|  +---------------------+                                     |
+--------------------------------------------------------------+
```

**Data shown:** Display name (or filename), system display name (when `show_system=true`)
**Box art:** NO -- box art is **not shown** despite being available in `FavoriteWithArt`
**Actions:** Remove from favorites (star button -> confirm -> remove)
**Layout:** Vertical list, flex row per item, 12px padding, border-bottom separators

---

### Pattern 6: Favorites Page -- All Favorites (Grouped by System)

**Component:** `GroupedFavorites` + `FavItem` (favorites.rs, lines 266-313)
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
| | .fav-name                              .fav-star   |       |
| |   "Super Mario World"                     (*)      |       |
| +----------------------------------------------------+       |
| .fav-item                                                    |
| | "Chrono Trigger"                           (*)      |       |
| +----------------------------------------------------+       |
| .fav-item                                                    |
| | "Donkey Kong Country"                      (*)      |       |
| +----------------------------------------------------+       |
+--------------------------------------------------------------+
| .fav-group                                                   |
| "PlayStation (2)"                                            |
| ==========================                                   |
| ...                                                          |
+--------------------------------------------------------------+
```

**Data shown:** Display name (or filename) -- NO system name (grouped already)
**Box art:** NO -- same as Pattern 5
**Actions:** Remove from favorites (same as Pattern 5)
**Layout:** Grouped vertical list, group headers with accent border

---

### Pattern 7: Favorites Page -- By System Cards Grid

**Component:** Inline in `FavoritesContent` (favorites.rs, lines 203-223)
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
but in the Home page systems grid it shows the actual disk size. This is a semantic mismatch.

**Data shown:** System display name, favorite count, latest added game name
**Box art:** NO
**Actions:** None (entire card is a link to `/favorites/{system}`)
**Layout:** 2-column grid (responsive to 3/4 columns)

---

### Pattern 8: System Favorites Page

**Component:** `SystemFavoritesContent` (favorites.rs, lines 583-639)
**Data:** `FavoriteWithArt` filtered to one system, but again only `Favorite` is passed to `FavItem`
**CSS classes:** Same as Pattern 5: `.fav-list`, `.fav-item`, etc. Plus header: `.rom-header`, `.back-btn`, `.page-title`, `.rom-count`

```
+--------------------------------------------------------------+
| .rom-header                                                  |
|  [< Back]   .page-title: "Super Nintendo"                   |
+--------------------------------------------------------------+
| .rom-count: "5 favorites"                                    |
+--------------------------------------------------------------+
| .fav-list                                                    |
| (same layout as Pattern 5 but with show_system=false)        |
+--------------------------------------------------------------+
```

**Data shown:** Display name only (no system -- already scoped)
**Box art:** NO
**Actions:** Remove from favorites
**Layout:** Same as Pattern 5

---

### Pattern 9: System ROM List Page

**Component:** `RomList` / `RomItem` (rom_list.rs)
**Data:** `RomEntry` -- paginated via `get_roms_page()`, PAGE_SIZE=100
**CSS classes:** `.rom-list`, `.rom-item`, `.rom-fav-btn`, `.rom-thumb-link`, `.rom-thumb`, `.rom-info`, `.rom-name`, `.rom-path`, `.rom-meta`, `.rom-size`, `.rom-ext`, `.rom-actions`, `.rom-action-btn`

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
+--------------------------------------------------------------+
| .rom-count: "50 / 234 games"                                 |
+--------------------------------------------------------------+
| .rom-list (flex column)                                      |
|                                                              |
| .rom-item                                                    |
| +----------------------------------------------------------+ |
| | (*)  +------+ .rom-info          .rom-meta   .rom-actions| |
| | fav  | .rom-| .rom-name (link)   .rom-size   [R] [X]    | |
| | btn  | thumb| "Super Mario Wld"  "1.2 MB"               | |
| |      | 40px | .rom-path          .rom-ext                | |
| |      +------+ "smw.sfc"          ".sfc"                  | |
| +----------------------------------------------------------+ |
|                                                              |
| .rom-item                                                    |
| +----------------------------------------------------------+ |
| | (*) .rom-info (no thumb if no art)  .rom-meta .rom-acts  | |
| |     .rom-name: "Chrono Trigger"     "3.1 MB"  [R] [X]   | |
| |     .rom-path: "chrono_trigger.sfc" ".sfc"               | |
| +----------------------------------------------------------+ |
|                                                              |
| .load-more-sentinel (infinite scroll trigger)                |
| +----------------------------------------------------------+ |
| |              [Load More / Loading...]                    | |
| +----------------------------------------------------------+ |
+--------------------------------------------------------------+
```

Full `.rom-item` detail view:

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
|  +---+          +--------+      |  "file.sfc" |  | ext    | |
|                                 +------------+   | ".sfc" | |
|                                                  +--------+ |
|                                                              |
|  .rom-actions (hover-reveal on desktop, always on touch)     |
|  +-----+-----+                                              |
|  | [P] | [X] |  <-- Rename, Delete                          |
|  +-----+-----+                                              |
|                                                              |
+--------------------------------------------------------------+
```

**Data shown:** Display name (or filename), file path, file size, file extension, favorite status, box art
**Box art:** YES -- `.rom-thumb` (40px height, max 56px width) but **only if available** (no placeholder shown)
**Actions:** Favorite toggle (star), rename (pencil icon), delete (X icon)
**Layout:** Vertical list, flex row per item, infinite scroll with IntersectionObserver

---

### Pattern 10: Global Search Results

**Component:** `SearchResults` / `SystemGroup` / `SearchResultItem` (search.rs)
**Data:** `GlobalSearchResults` -> `SystemSearchGroup` -> `GlobalSearchResult`
**CSS classes:** `.search-groups`, `.search-group`, `.search-group-header`, `.search-group-title`, `.search-see-all`, `.search-group-results`, `.search-result-item`, `.search-result-thumb-link`, `.search-result-thumb`, `.search-result-info`, `.search-result-name`, `.search-result-badges`, `.search-badge`, `.search-badge-genre`, `.search-badge-fav`

```
.search-page
+--------------------------------------------------------------+
| .search-page-bar                                             |
|  +--------------------------------------------------------+  |
|  | .search-page-input: "mario" (larger, 2px border)       |  |
|  +--------------------------------------------------------+  |
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
| | | +------+ .search-result-info                         | | |
| | | | .se- | .search-result-name (link)                  | | |
| | | | arch-|   "Super Mario World"                       | | |
| | | | resu-| .search-result-badges                       | | |
| | | | lt-  |   [Platformer]  *                           | | |
| | | | thumb|   genre badge    fav star                   | | |
| | | | 40px |                                             | | |
| | | +------+                                             | | |
| | +------------------------------------------------------+ | |
| |                                                          | |
| | .search-result-item                                      | |
| | +------------------------------------------------------+ | |
| | |  .search-result-info (no thumb if no art)            | | |
| | |  .search-result-name: "Super Mario RPG"             | | |
| | |  .search-result-badges: [RPG]                       | | |
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

**Data shown:** Display name, genre (as badge), favorite indicator (star badge), box art
**Box art:** YES -- `.search-result-thumb` (40px height, max 56px width) but **only when available** (no placeholder; the `<Show>` conditional hides it entirely)
**Actions:** None (name is a link to detail page; "See all" link goes to system ROM list)
**Layout:** Grouped by system in cards, top 3 results per system, vertical list within each group

---

## 3. Data Structures Comparison

| Field | RecentWithArt | FavoriteWithArt | RomEntry | GlobalSearchResult | GameInfo (detail) |
|-------|:---:|:---:|:---:|:---:|:---:|
| display_name | via GameRef | via GameRef | via GameRef | direct | direct |
| rom_filename | via GameRef | via GameRef | via GameRef | direct | direct |
| system | via GameRef | via GameRef | via GameRef | direct | direct |
| system_display | via GameRef | via GameRef | via GameRef | - | direct |
| rom_path | via GameRef | via GameRef | via GameRef | - | direct |
| box_art_url | direct | direct | direct | direct | direct |
| size_bytes | - | - | direct | - | direct |
| is_favorite | - | - | direct | direct | direct |
| genre | - | - | - | direct | direct |
| is_m3u | - | - | direct | - | direct |
| date_added | via Favorite | via Favorite | - | - | - |
| last_played | via RecentEntry | - | - | - | - |
| marker_filename | via RecentEntry | via Favorite | - | - | - |
| subfolder | - | via Favorite | - | - | - |

---

## 4. Inconsistencies and Issues

### Issue A: Box Art Dropped in FavItem (HIGH)

`FavItem` receives `fav: Favorite` (not `FavoriteWithArt`), so the `box_art_url`
is stripped before the component sees it. This means the all-favorites list, grouped
favorites list, and system-specific favorites page all display **text-only items with
no thumbnails**, even though:

- The hero card and horizontal scroll on the same page DO show box art
- The data (`FavoriteWithArt`) already contains `box_art_url`
- The ROM list (`RomItem`) shows thumbnails

**Location:** `FlatFavorites`, `GroupedFavorites`, `SystemFavoritesContent` -- all pass
`f.fav` to `FavItem` instead of the full `FavoriteWithArt`.

### Issue B: No Placeholder in ROM List and Search Results (MEDIUM)

In the ROM list (`RomItem`) and search results (`SearchResultItem`), when box art is
not available, the thumbnail is simply omitted. This causes inconsistent row heights
and shifting content:

- ROM list items with art: `[star] [thumb 40px] [name/path] [meta] [actions]`
- ROM list items without art: `[star] [name/path] [meta] [actions]`

The hero card and horizontal scroll correctly show a placeholder rectangle when no art
is available. The ROM list and search results do not.

### Issue C: Semantic Class Name Reuse (LOW)

In the favorites "By System" cards (Pattern 7), the class `.system-card-size` is reused
to display the "latest added game name" rather than a disk size. On the Home page's
systems grid, the same class shows actual disk size. Semantically confusing, though
visually it works fine since both are small muted text.

### Issue D: Duplicated Hero Card Rendering Logic (MEDIUM)

The hero card pattern (box art + title + system) is copy-pasted identically between:
- `home.rs` lines 83-95 (Last Played)
- `favorites.rs` lines 144-156 (Latest Added)

Both use the exact same CSS classes, same conditional `has_art` branching, same
`into_any()` pattern. This is a prime candidate for extraction.

### Issue E: Duplicated Horizontal Scroll Item Logic (MEDIUM)

The recent-scroll item pattern is copy-pasted identically between:
- `home.rs` lines 117-132 (Recently Played)
- `favorites.rs` lines 165-180 (Recently Added)

Same CSS classes, same art/placeholder logic, same structure.

### Issue F: Duplicated Filter Bar Logic (LOW)

The filter chips (Hide Hacks, Hide Translations, Hide Betas, Hide Clones, Genre
dropdown) are implemented independently in three places:
- `rom_list.rs` lines 271-339
- `search.rs` lines 259-321

And the `GenreDropdown` component is defined separately in both `rom_list.rs` and
`search.rs` with identical implementations.

### Issue G: Missing Actions in Some Views (MEDIUM)

| View | Favorite Toggle | Delete | Rename | Launch | Link to Detail |
|------|:---:|:---:|:---:|:---:|:---:|
| Hero card | - | - | - | - | YES (entire card) |
| Recent scroll item | - | - | - | - | YES (entire card) |
| FavItem (all lists) | Unfavorite | - | - | - | YES (name link) |
| RomItem | Toggle | Delete | Rename | - | YES (name + thumb link) |
| SearchResultItem | - | - | - | - | YES (name + thumb link) |
| GameDetailContent | Toggle | Delete | Rename | Launch | N/A (is the detail) |

Observations:
- Search results show `is_favorite` as a badge but have **no action** to toggle it
- Hero cards and recent scroll items have **no quick-favorite** action
- FavItem has no delete or rename -- only unfavorite (appropriate)
- Only the game detail page has the Launch action

### Issue H: Different Item Heights and Structures (LOW)

| View | Row Height (approx) | Thumb Size | Name Font | Path Shown |
|------|---------------------|-----------|-----------|-----------|
| Hero card | ~96px | 56px h | 1.1rem bold | No |
| Recent scroll | ~160px | 80px h | 0.8rem | No |
| FavItem | ~44px | None | 0.85rem | No |
| RomItem | ~60px (with art) | 40px h | 0.85rem | Yes |
| SearchResultItem | ~60px (with art) | 40px h | 0.85rem | No |

FavItem is noticeably smaller and plainer than RomItem and SearchResultItem for the
same conceptual purpose (a game in a list).

---

## 5. Visual Summary: All Patterns Side by Side

```
HOME PAGE                          FAVORITES PAGE
+----------------------------+     +----------------------------+
| [Search bar]               |     |                            |
|                            |     | LATEST ADDED               |
| LAST PLAYED                |     | +------------------------+ |
| +------------------------+ |     | | [art] Title            | |
| | [art] Title            | |     | |        System          | |
| |        System          | |     | +------------------------+ |
| +------------------------+ |     |                            |
|                            |     | RECENTLY ADDED             |
| RECENTLY PLAYED            |     | +----+ +----+ +----+      |
| +----+ +----+ +----+      |     | |art | |art | |art | -->  |
| |art | |art | |art | -->  |     | |name| |name| |name|      |
| |name| |name| |name|      |     | |sys | |sys | |sys |      |
| |sys | |sys | |sys |      |     | +----+ +----+ +----+      |
| +----+ +----+ +----+      |     |                            |
|                            |     | STATS: [N favs] [N sys]   |
| LIBRARY                   |     |                            |
| [Games] [Systems] [Favs]  |     | [> Organize favorites]     |
| [Storage bar]              |     |                            |
|                            |     | BY SYSTEM (grid)           |
| SYSTEMS (grid)             |     | +--------+ +--------+     |
| +--------+ +--------+     |     | |SysName | |SysName |     |
| |SysName | |SysName |     |     | |N favs  | |N favs  |     |
| |Maker   | |Maker   |     |     | |Latest  | |Latest  |     |
| |N games | |N games |     |     | +--------+ +--------+     |
| |Size    | |Size    |     |     |                            |
| +--------+ +--------+     |     | ALL FAVORITES [Flat|Group] |
| ...                        |     | +------------------------+ |
+----------------------------+     | | GameName       [star]  | |
                                   | |  System                | |
                                   | +------------------------+ |
                                   | | GameName       [star]  | |
                                   | |  System                | |
                                   | +------------------------+ |
                                   +----------------------------+

ROM LIST PAGE                      SEARCH PAGE
+----------------------------+     +----------------------------+
| [< Back]  System Name     |     | [Search input, larger]     |
| [Search within system]     |     | [Hacks][Trans][Betas]      |
| [Hacks][Trans][Betas]      |     | [Clones][Genre v]          |
| [Genre v]                  |     |                            |
| "50 / 234 games"           |     | "42 results in 5 systems"  |
|                            |     |                            |
| +------------------------+ |     | +------------------------+ |
| |(*) [t] GameName  1.2MB | |     | | SystemA (12)  See all >| |
| |        path.sfc  .sfc  | |     | +------------------------+ |
| |              [Ren][Del] | |     | | [t] GameName           | |
| +------------------------+ |     | |     [Genre] [*]         | |
| |(*) GameName       3MB  | |     | +------------------------+ |
| |    path.sfc      .sfc  | |     | | [t] GameName           | |
| |              [Ren][Del] | |     | |     [Genre]            | |
| +------------------------+ |     | +------------------------+ |
| |(*) [t] GameName  800KB | |     |                            |
| |        path.sfc  .sfc  | |     | +------------------------+ |
| |              [Ren][Del] | |     | | SystemB (8)   See all >| |
| +------------------------+ |     | | ...                      | |
| ...                        |     | +------------------------+ |
| [Load more / auto-scroll]  |     +----------------------------+
+----------------------------+

(*) = fav star    [t] = thumbnail (only when art exists)
[Ren] = rename    [Del] = delete
```

---

## 6. Proposed Improvements

### 6.1 Add Box Art Thumbnails to FavItem (HIGH PRIORITY) -- DONE

**Problem:** The all-favorites list is the most text-heavy, least visual part of the app.
Every other game list shows images.

**Solution:** Pass the full `FavoriteWithArt` to `FavItem` (or add a `box_art_url: Option<String>` prop).
Add an optional thumbnail before the name, matching the `rom-thumb` pattern from `RomItem`.

```
BEFORE:
| GameName             [star] |
|  System                     |

AFTER:
| [thumb] GameName     [star] |
|         System              |
```

**Estimated effort:** Small -- change `FavItem` props from `fav: Favorite` to include
`box_art_url: Option<String>`, add conditional thumbnail rendering, copy the
`.rom-thumb-link` / `.rom-thumb` pattern.

**What changed:** Added `box_art_url: Option<String>` prop to `FavItem`. All call sites
(`FlatFavorites`, `GroupedFavorites`, `SystemFavoritesContent`) now pass `f.box_art_url`.
Thumbnail uses `.rom-thumb-link` / `.rom-thumb` classes with `<Show>` for conditional rendering.

### 6.2 Extract Shared HeroCard Component (MEDIUM PRIORITY) -- DONE

**Problem:** Hero card logic is copy-pasted in `home.rs` and `favorites.rs`.

**Solution:** Create `src/components/hero_card.rs`:

```rust
#[component]
pub fn HeroCard(
    href: String,
    name: String,
    system: String,
    box_art_url: Option<String>,
) -> impl IntoView { ... }
```

Both pages would call `<HeroCard href name system box_art_url />`.

**Estimated effort:** Small refactor, no visual change.

**What changed:** Created `src/components/hero_card.rs` with `HeroCard` component.
Updated `home.rs` and `favorites.rs` to use `<HeroCard>` instead of inline rendering.
Added `pub mod hero_card;` to `src/components/mod.rs`.

### 6.3 Extract Shared RecentScrollItem Component (MEDIUM PRIORITY) -- DONE

**Problem:** Horizontal scroll item logic is copy-pasted in `home.rs` and `favorites.rs`.

**Solution:** Create `GameScrollCard` in `src/components/hero_card.rs` (same file as `HeroCard`):

```rust
#[component]
pub fn GameScrollCard(
    href: String,
    name: String,
    system: String,
    box_art_url: Option<String>,
) -> impl IntoView { ... }
```

**Estimated effort:** Small refactor, no visual change.

**What changed:** Created `GameScrollCard` component in `hero_card.rs` alongside `HeroCard`.
Updated `home.rs` and `favorites.rs` to use `<GameScrollCard>` instead of inline rendering.

### 6.4 Add Placeholder Thumbnails in ROM List and Search (LOW PRIORITY) -- DONE

**Problem:** Items without box art in the ROM list and search results have no thumbnail
space, causing inconsistent row alignment.

**Solution:** Add the same placeholder pattern used in the hero card and recent scroll:

```rust
// In RomItem, change:
<Show when=move || has_box_art>
    <A href=... attr:class="rom-thumb-link">
        <img class="rom-thumb" ... />
    </A>
</Show>

// To:
<A href=... attr:class="rom-thumb-link">
    {if has_box_art {
        view! { <img class="rom-thumb" src=... /> }.into_any()
    } else {
        view! { <div class="rom-thumb-placeholder"></div> }.into_any()
    }}
</A>
```

Add CSS:
```css
.rom-thumb-placeholder {
    width: 40px;
    height: 40px;
    background: var(--border);
    border-radius: 3px;
}
```

**Estimated effort:** Small. Same for `SearchResultItem`.

**Trade-off:** On systems with very few images this would add many grey rectangles.
A possible middle ground: only show placeholders if >50% of visible items have art,
but that's complex. The simpler approach (always show placeholder) is fine for now.

**What changed:** `RomItem` now always renders the thumb-link area with either an image
or a `.rom-thumb-placeholder` div. `SearchResultItem` does the same with
`.search-result-thumb-placeholder`. Added CSS for both placeholder classes (40x40px
gray box with 3px border-radius).

### 6.5 Deduplicate GenreDropdown Component (LOW PRIORITY) -- DONE

**Problem:** `GenreDropdown` is defined identically in both `rom_list.rs` and `search.rs`.

**Solution:** Move to `src/components/genre_dropdown.rs` and import in both pages.

**Estimated effort:** Trivial.

**What changed:** Created `src/components/genre_dropdown.rs` with the shared `GenreDropdown`
component. Removed local definitions from both `rom_list.rs` and `search.rs`. Both now
import from `crate::components::genre_dropdown::GenreDropdown`. Added `pub mod genre_dropdown;`
to `src/components/mod.rs`.

### 6.6 Add Quick-Favorite to Search Results (LOW PRIORITY) -- DONE

**Problem:** Search results show the favorite status as a read-only badge but have no
way to toggle it. The user must navigate to the game detail page to favorite/unfavorite.

**Solution:** Replace the star badge with a clickable star button (like `RomItem`'s
`.rom-fav-btn`). Would require passing the system/filename data into
`SearchResultItem` and calling the same `add_favorite` / `remove_favorite` server
functions.

**Estimated effort:** Medium -- need to add state management (optimistic toggle) and
ensure the server function call has the needed data (system + rom_path, not just
system + rom_filename).

**What changed:** Added `rom_path` field to `GlobalSearchResult` struct and populated it
in the server-side search function. `SearchResultItem` now has a `.rom-fav-btn` button
with optimistic toggle using the same pattern as `RomItem` (calls `add_favorite` /
`remove_favorite` server functions). The read-only star badge was removed.

---

## 7. Priority Summary

| # | Improvement | Priority | Effort | Visual Impact | Status |
|---|-------------|----------|--------|---------------|--------|
| 6.1 | Box art in FavItem | HIGH | Small | Large -- transforms the most-used list | DONE |
| 6.2 | Extract HeroCard | MEDIUM | Small | None (refactor) | DONE |
| 6.3 | Extract GameScrollCard | MEDIUM | Small | None (refactor) | DONE |
| 6.4 | Placeholders in ROM/search lists | LOW | Small | Medium -- consistent alignment | DONE |
| 6.5 | Deduplicate GenreDropdown | LOW | Trivial | None (refactor) | DONE |
| 6.6 | Quick-favorite in search | LOW | Medium | Small -- convenience feature | DONE |
