# Global Search — Implementation Plan

## Overview

A search feature that spans all systems, returning results grouped by system, with filter toggles and persistence when navigating to per-system browse pages.

**Why it matters:** The current search is per-system only — the user must first navigate to `/games`, pick a system, then search within it. If a user wants to find "Sonic" across their entire library (Genesis, Game Gear, Saturn, Dreamcast, Arcade), they must search each system individually. Global search removes this friction and makes the app feel like a unified game library.

**Constraints:** Runs on Raspberry Pi hardware. ROM data comes from filesystem scans cached in memory with 30s TTL (`RomCache`). ~40 possible systems. Must remain snappy.

---

## UX Placement Options

### Option A: Search icon in the top bar

Add a magnifying glass icon in `.top-actions` (where the favorites star already sits).

- **Pros:** Always visible on every page. Familiar mobile pattern. Minimal layout change.
- **Cons:** Small tap target on mobile. Top bar is already narrow.

### Option B: Bottom nav tab (replace or add 5th)

Add "Search" as a bottom nav tab.

- **Pros:** Extremely discoverable — always one tap away. Consistent with apps like Spotify, YouTube.
- **Cons:** 5 tabs gets crowded on small phones. Replacing "Games" breaks muscle memory.

### Option C: Prominent search bar on the home page

Full-width search input directly on the home page, above "Last Played".

- **Pros:** Immediately visible on landing page. Google-style simplicity.
- **Cons:** Only accessible from the home page — user must navigate home first.

### Option D: Top bar icon + home page search bar (hybrid) — RECOMMENDED

Combine A and C. Home page has a prominent search bar for discoverability, top bar icon provides quick access from any page.

- **Pros:** Best of both worlds. Users discover search on home, use top bar icon from anywhere.
- **Cons:** Two entry points to maintain (minimal overhead).

### Option E: Floating action button (FAB)

Circular floating button in the bottom-right corner.

- **Pros:** Always visible. Does not interfere with existing layout.
- **Cons:** Can overlap content. Uncommon on web. May feel out of place.

### Recommendation: **Option D**

The home page search bar provides discoverability for new users and a natural starting point. The top bar search icon provides quick access from any page. The top bar already has a `.top-actions` container with the favorites star, so adding a search icon is trivial.

---

## Search Results Layout

### Route: `/search?q=<query>&hide_hacks=true&genre=Platform`

```
[Search input bar]                              <- full-width, autofocus
[Active filter chips: "Hide Hacks" x | "Platform" x]  <- dismissible chips
[Result count: "42 results across 7 systems"]

--- Super Nintendo (12 results) --- [See all ->]
  [thumb] Super Mario World (USA)           ★  Platform
  [thumb] Super Mario World 2 (USA)         ★  Platform
  [thumb] Super Mario All-Stars (USA)          Platform

--- Sega Mega Drive (8 results) --- [See all ->]
  [thumb] Sonic the Hedgehog (USA)          ★  Platform
  [thumb] Sonic the Hedgehog 2 (USA)           Platform
  [thumb] Sonic the Hedgehog 3 (USA)           Platform

--- Arcade (FBNeo) (5 results) --- [See all ->]
  [thumb] Sonic Wings                          Shooter
  [thumb] Sonic Boom                           Platform
  [thumb] Sonic the Fighters                   Fighting
```

### Per-result fields:
- Box art thumbnail (if available)
- Display name (from embedded DB or filename)
- Favorite star indicator
- Genre badge (from `normalized_genre` in arcade_db / game_db)
- System badge

### Top N per system:
- Show top **3** results per system by default
- "See all" link navigates to `/games/<system>?search=<query>&hide_hacks=true&genre=Platform` (carries filters)
- Systems sorted by number of matching results (descending)
- Systems with 0 results are hidden

---

## Filters

### Hack visibility toggle

**Default:** Show all (hacks visible). Some users have hack-heavy collections — hiding by default would confuse them.

**Filter:** "Hide Hacks" toggle filters out ROMs classified as `RomTier::Hack` by `rom_tags::classify()`.

**Extended toggles (phase 2):**
- "Hide Translations" (`RomTier::Translation`)
- "Hide Betas/Protos" (`RomTier::PreRelease`)
- "Hide Clones" (arcade only, `is_clone == true`)

**UX:** Filter chip buttons below the search bar. Active filters highlighted with accent color. Tapping deactivates. Each chip has an "x" dismiss button.

### Genre filter

**Data availability:** Both `arcade_db` and `game_db` have `normalized_genre` fields. Available genres:

Action, Adventure, Beat'em Up, Board & Card, Driving, Educational, Fighting, Maze, Music, Other, Pinball, Platform, Puzzle, Quiz, Role-Playing, Shooter, Simulation, Sports, Strategy

**UX:** Dropdown selector or scrollable horizontal chip row (~18 genres — too many to show all at once).

**Behavior:** Only ROMs matching `normalized_genre` are shown. ROMs without genre data are hidden when a genre filter is active.

### Filter state in URL

All filter state encoded in query params:
- `q` — search query
- `hide_hacks` — boolean (absent = false)
- `genre` — genre string (absent = no filter)

Makes filters bookmarkable and enables persistence.

---

## Filter Persistence

When clicking "See all" for a system, navigate to existing system browse page with filters:

```
/games/nintendo_snes?search=mario&hide_hacks=true&genre=Platform
```

**Changes in `RomList`:** Add `hide_hacks` and `genre` query param signals, pass to `get_roms_page`.

**Changes in `get_roms_page`:** Add `hide_hacks: bool` and `genre: String` parameters, apply before scoring/pagination.

**Backward compatibility:** Existing URLs without filter params work unchanged.

---

## User-Centric Ideas

### Search suggestions / autocomplete (Phase 2)
Compact dropdown with top 5-8 results as the user types. Lightweight server function returning display names + system + href, debounced at 200ms.

### Recent searches (Phase 2)
Last 10 queries stored in `localStorage` (client-side). Shown as suggestion chips when search input is focused and empty.

### Browse by genre (Phase 1)
Genre filter with empty search query = "browse by genre" — showing all platformers, all RPGs, etc. across the entire library.

### Random game / "I'm Feeling Lucky" (Phase 2)
Dice icon button picking a random game. Server function picks a random system (weighted by game count), then a random ROM.

### Quick actions from results (Phase 2+)
- Favorite toggle directly on search results
- Launch (depends on game-launching feature)
- Each result links to game detail page

### Keyboard shortcuts (Phase 2)
- `/` to focus search from anywhere
- `Escape` to clear search
- Arrow keys to navigate results
- `Enter` to open selected result

---

## Data & Performance

### How the cache works today

`RomCache` in `api/mod.rs` stores:
- `systems: RwLock<Option<CacheEntry<Vec<SystemSummary>>>>` — all systems with game counts
- `roms: RwLock<HashMap<String, CacheEntry<Vec<RomEntry>>>>` — per-system ROM lists

Cache TTL: 30 seconds. Cache miss triggers filesystem scan.

### Global search strategy

**Iterate all cached system ROM lists server-side:**

1. Get systems list from cache
2. For each system with `game_count > 0`, get ROM list from cache
3. Apply filters (hack/genre) before scoring to reduce work
4. Score each ROM against query using existing `search_score()`
5. Group by system, sort within each group by score
6. Return top N per system + total count per system

**Why this works on Pi:**
- ROM lists already in memory (cached from prior page visits)
- `search_score()` is a simple string operation — no regex, no heavy allocation
- `rom_tags::classify()` is O(filename length)
- Genre filtering is a string comparison on embedded DB data
- 15,000 ROMs (30 systems × 500 avg) scores in <10ms even on Pi

**Cold start mitigation:** First global search may trigger scans for all systems. Accept this with a loading indicator for Phase 1. Home page already warms the systems cache. Phase 2: background pre-warm on startup.

### Client-side debounce

400ms (slightly longer than the 300ms per-system search since global triggers more server work).

### Result size limits

Max payload: 3 results × ~30 systems = ~90 results. Well within acceptable size.

---

## Server Functions

### New: `global_search`

```rust
#[server(prefix = "/sfn")]
pub async fn global_search(
    query: String,
    hide_hacks: bool,
    genre: String,          // empty = no filter
    per_system_limit: usize,
) -> Result<GlobalSearchResults, ServerFnError>
```

### New: `get_all_genres`

```rust
#[server(prefix = "/sfn")]
pub async fn get_all_genres() -> Result<Vec<String>, ServerFnError>
```

Returns genres with actual ROMs in the library. Cache the result.

### Modified: `get_roms_page`

Add `hide_hacks: bool` and `genre: String` parameters for filter persistence.

### Response types:

```rust
pub struct GlobalSearchResults {
    pub groups: Vec<SystemSearchGroup>,
    pub total_results: usize,
    pub total_systems: usize,
}

pub struct SystemSearchGroup {
    pub system: String,
    pub system_display: String,
    pub total_matches: usize,
    pub top_results: Vec<GlobalSearchResult>,
}

pub struct GlobalSearchResult {
    pub rom_filename: String,
    pub display_name: String,
    pub system: String,
    pub genre: String,
    pub is_favorite: bool,
    pub box_art_url: Option<String>,
}
```

---

## New/Modified Files

### New files:
| File | Purpose |
|------|---------|
| `replay-control-app/src/pages/search.rs` | Global search page component |
| `replay-control-app/src/components/search_bar.rs` | Reusable search bar with filter chips |
| `replay-control-app/src/components/search_result.rs` | Individual search result row component |

### Modified files:
| File | Change |
|------|--------|
| `replay-control-app/src/pages/mod.rs` | Add `pub mod search;` |
| `replay-control-app/src/components/mod.rs` | Add `pub mod search_bar;` and `pub mod search_result;` |
| `replay-control-app/src/lib.rs` | Add route for `/search`, search icon in top bar `.top-actions` |
| `replay-control-app/src/server_fns.rs` | Add result types, `global_search()`, `get_all_genres()`. Modify `get_roms_page()` for filter params. |
| `replay-control-app/src/types.rs` | Add client-side mirror types |
| `replay-control-app/src/main.rs` | `register_explicit` for new server functions |
| `replay-control-app/src/components/rom_list.rs` | Read/pass `hide_hacks` and `genre` query params |
| `replay-control-app/src/pages/home.rs` | Add search bar to home page |
| `replay-control-app/src/i18n.rs` | Search-related translation keys |
| `replay-control-app/style/style.css` | Search page, filter chips, result group styles |

---

## i18n Keys

| Key | English |
|-----|---------|
| `search.title` | `"Search"` |
| `search.placeholder` | `"Search all games..."` |
| `search.no_results` | `"No results found"` |
| `search.no_results_with_filters` | `"No results. Try removing some filters."` |
| `search.results_summary` | `"results across"` |
| `search.systems` | `"systems"` |
| `search.see_all` | `"See all"` |
| `filter.hide_hacks` | `"Hide Hacks"` |
| `filter.hide_translations` | `"Hide Translations"` |
| `filter.hide_betas` | `"Hide Betas"` |
| `filter.genre` | `"Genre"` |
| `filter.genre_all` | `"All Genres"` |
| `filter.clear_filters` | `"Clear Filters"` |
| `search.random_game` | `"Random Game"` |

---

## Implementation Phases

### Phase 1 (core feature):
1. Add result types and mirror types
2. Implement `global_search()` server function
3. Modify `get_roms_page()` to accept filter params
4. Create `SearchPage` component with grouped results
5. Add route, top bar icon, home page search bar
6. Add filter chips (hide hacks, genre dropdown)
7. Implement filter persistence via query params on "See all" links
8. Update `RomList` to read and pass filter query params
9. Register new server functions in `main.rs`
10. Add i18n keys and CSS styles

### Phase 2 (enhancements):
- `get_all_genres()` for dynamic genre list
- Search suggestions / autocomplete dropdown
- Recent searches in localStorage
- Random game button
- Keyboard shortcut (`/` to focus search)

### Phase 3 (polish):
- Hide translations / hide betas filter toggles
- Hide clones toggle for arcade systems
- Favorite toggle directly in search results
- Background cache pre-warming on startup
- Search result animations / transitions
