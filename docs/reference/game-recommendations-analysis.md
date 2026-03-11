# Game Recommendations Feature -- Analysis & Design

Comprehensive design analysis for adding game recommendation blocks to the Home page of the Replay Control companion app. The goal is to make the Home page more dynamic, encouraging discovery and rediscovery of games within the user's existing library.

**Date:** March 2026

---

## 1. Available Data Sources

Understanding what data exists is the foundation. Each recommendation type depends on one or more of these sources.

### 1.1 Embedded Game Database (`game_db`)

- ~34K ROM entries across all non-arcade systems
- Per game: `display_name`, `year` (u16), `genre` (string), `developer` (string), `players` (u8), `normalized_genre` (shared taxonomy), `region`, `crc32`
- Lookup: by exact filename stem, by CRC32, by normalized title (fuzzy)
- **Always available** -- compiled into the binary, no downloads needed

### 1.2 Embedded Arcade Database (`arcade_db`)

- ~29K entries (MAME/FBNeo)
- Per game: `display_name`, `year`, `manufacturer`, `players`, `rotation`, `driver_status`, `is_clone`, `parent`, `category`, `normalized_genre`
- **Always available** -- compiled into the binary

### 1.3 Metadata SQLite Cache (`metadata_db`)

- User-downloaded LaunchBox data: `description`, `rating` (f64, 0-5 scale), `publisher`
- Image paths: `box_art_path`, `screenshot_path`
- Queries available: `lookup`, `lookup_ratings` (batch per system), `system_ratings` (all ratings for a system), `all_ratings`, `entries_per_system`, `image_stats`
- **Only available after user downloads metadata** (More > Game Metadata)

### 1.4 Favorites (filesystem)

- `.fav` marker files in `_favorites/` directory
- Per favorite: `system`, `rom_filename`, `rom_path`, `subfolder`, `date_added` (mtime)
- Can derive: favorite count per system, most recently favorited games
- **Always available** (though may be empty)

### 1.5 Recently Played (filesystem)

- `.rec` marker files in `_recent/` directory
- Per entry: `system`, `rom_filename`, `last_played` (mtime)
- Can derive: most-played systems, genre distribution of played games
- **Always available** (though may be empty if no games have been played yet)

### 1.6 ROM Cache (in-memory, per system)

- Full list of ROMs per system with file sizes, display names, genre/players via DB lookups
- `get_roms_page` already supports: text search, genre filter, multiplayer filter, min_rating filter, hide hacks/translations/betas/clones
- **Always available**

### 1.7 System Definitions

- Static system metadata: `folder_name`, `display_name`, `manufacturer`, `category` (Arcade/Console/Handheld/Computer)
- **Always available**

---

## 2. Recommendation Types

### 2.1 Direct Game Recommendations (Box Art Cards)

These show a small horizontal scroll of game cards (box art + name + system). Tapping a card navigates to the game detail page. Visually similar to the existing "Recently Played" scroll but with different selection logic.

#### A. "Rediscover" -- Random Picks

**What it shows:** 4-6 randomly selected games from the user's library, refreshed on each page load.

**Data sources:** ROM cache (all systems). Optional: filter to games that have box art (metadata_db) for visual appeal.

**Algorithm:**
1. Pick a random system weighted by game count (reuse `random_game` logic)
2. Pick a random ROM from that system
3. Repeat 4-6 times, deduplicating systems if possible (spread across systems)
4. Optional: bias toward games with box art if metadata is available
5. Optional: exclude recently played games (to surface truly forgotten games)

**Works without metadata:** Yes. Games without box art show the placeholder card. But the feature is significantly more appealing with box art downloaded.

**Complexity:** Simple. The `random_game` server function already exists and does weighted random selection. Extending it to return multiple games with box art URLs is straightforward.

#### B. "Because You Like [System]" -- Favorites-Based System Recommendations

**What it shows:** 4-6 games from the user's most-favorited system(s) that are NOT already favorites.

**Data sources:** Favorites list (count per system) + ROM cache + game_db/arcade_db (genre) + metadata_db (box art, rating).

**Algorithm:**
1. Count favorites per system; pick the top 1-2 systems
2. For each system, load all ROMs, exclude favorites
3. If metadata available: sort by rating descending, pick top 4-6
4. If no metadata: pick random games from the same genre as the user's favorites in that system
5. Fallback: pick random non-favorited games from the system

**Works without metadata:** Partially. Without ratings, falls back to genre matching or random selection. Without box art, shows placeholders.

**Complexity:** Medium. Requires a new server function that cross-references favorites with ROM lists and metadata ratings.

#### C. "More Like Your Favorites" -- Genre-Based Recommendations

**What it shows:** 4-6 games that share the genre of the user's favorites but are from different systems.

**Data sources:** Favorites list + game_db/arcade_db (genre lookup) + ROM cache + metadata_db (box art, rating).

**Algorithm:**
1. Analyze the user's favorites: count genre occurrences across all favorites
2. Pick the top 1-2 genres (e.g., "Platform", "Fighting")
3. For each genre, find games across ALL systems that match, excluding favorites
4. Rank by rating (if available), then randomize within the top candidates
5. Prefer games from systems the user hasn't explored much

**Works without metadata:** Partially. Genre is available from embedded DBs (always). Ratings and box art need metadata downloaded.

**Complexity:** Medium. Genre aggregation across favorites is new logic, but all the building blocks (genre lookup, ROM iteration, favorites check) exist.

#### D. "Top Rated" -- Curated Best-Of

**What it shows:** 4-6 highest-rated games from the user's library.

**Data sources:** metadata_db (ratings), ROM cache, box art.

**Algorithm:**
1. Query `all_ratings()` from metadata_db
2. Sort by rating descending
3. Filter to games that exist in the user's ROM cache
4. Optionally filter to games with box art for visual appeal
5. Pick top 4-6, preferring diversity across systems

**Works without metadata:** No. This recommendation type requires downloaded metadata (ratings). Should be hidden or replaced with a fallback when metadata is not available.

**Complexity:** Simple. `all_ratings()` already exists. The filtering and sorting are straightforward.

### 2.2 Search Recommendation Blocks (Quick Filter Links)

These are styled cards/buttons that link to the search page (`/search`) with pre-filled query parameters. They function as curated entry points into the existing search and filtering infrastructure.

#### E. "Best Rated [System] Games"

**What it shows:** A card per system (top 2-3 systems by game count, or user's most-played systems). Example: "Best rated Mega Drive games" linking to `/search?genre=&min_rating=4&system=sega_smd` (or the system ROM list with rating filter).

**Data source for card selection:** System list + favorites count (or recently played systems). The actual results are handled by existing `get_roms_page` which already supports `min_rating`.

**Algorithm for choosing which systems to feature:**
1. If user has favorites: use top-favorited systems
2. If user has recents: use recently played systems
3. Fallback: top systems by game count

**Works without metadata:** Partially. The link itself works, but the `min_rating` filter yields zero results without metadata. Could fall back to linking without the rating filter (just "Mega Drive games").

**Complexity:** Simple. These are just styled `<A>` links with constructed URLs. No new server logic.

#### F. "Best [Genre] Games"

**What it shows:** Cards for the most common genres in the library. Example: "Best Platformers" linking to `/search?genre=Platform`.

**Data source:** `get_all_genres()` already returns all available genres. Pick the top 3-4 most populated genres.

**Algorithm:**
1. Call `get_all_genres()` (or a new function that returns genre + count)
2. Pick the top 3-4 by game count
3. If user has favorites, bias toward genres represented in favorites

**Works without metadata:** Yes. Genre data comes from the embedded game DBs (always available). The search page with genre filter works without metadata.

**Complexity:** Simple. Minor enhancement to `get_all_genres()` to return counts, then styled links.

#### G. "Multiplayer Games"

**What it shows:** A single prominent card: "2+ Player Games" linking to `/search?multiplayer=true`.

**Data source:** Players data from game_db/arcade_db. The `multiplayer_only` filter already exists on both `get_roms_page` and `global_search`.

**Algorithm:** Static link. Optionally: show a count ("127 multiplayer games in your library").

**Works without metadata:** Yes. Player count comes from embedded DBs.

**Complexity:** Simple. Just a styled link with an optional count derived from existing data.

#### H. "Games from [Year/Decade]"

**What it shows:** Nostalgia-targeted cards: "Games from the 90s", "1992 Classics". Could align with the user's age/preferences.

**Data source:** Year data from game_db/arcade_db. Currently `get_roms_page` does NOT support a year filter, so this would need either a new filter parameter or a creative use of search.

**Works without metadata:** Yes. Year comes from embedded DBs.

**Complexity:** Medium. The year data exists in the DBs but is not currently exposed as a filter on the ROM list or search. Adding a `min_year`/`max_year` parameter to `get_roms_page` and `global_search` would be needed.

---

## 3. Home Page Placement

### Current Home Page Layout

```
1. [Search bar]                      (link to /search)
2. [Last Played hero card]           (most recent game, box art)
3. [Recently Played scroll]          (horizontal scroll, last 10)
4. [Library Stats grid]              (games, systems, favorites, storage)
5. [All Systems grid]                (system cards, 2-4 columns)
```

### Proposed Layout with Recommendations

```
1. [Search bar]                      (unchanged)
2. [Last Played hero card]           (unchanged)
3. [Recently Played scroll]          (unchanged)
4. [Recommendations section]         (NEW -- 1 or 2 recommendation blocks)
5. [Quick Discover links]            (NEW -- search recommendation cards)
6. [Library Stats grid]              (unchanged, moved down slightly)
7. [All Systems grid]                (unchanged)
```

### Design Principles for Placement

- **Do not push the Systems grid too far down.** The user analysis (section 4.1) identifies the systems grid as the primary browsing entry point. Recommendations should complement, not displace.
- **Keep it compact.** One horizontal scroll of game cards + one row of quick-filter links. No more than ~200px vertical space on mobile.
- **Progressive disclosure.** If the user has no favorites and no play history, show only the "Rediscover" (random) block + genre/multiplayer links. As user data grows, swap in more personalized recommendations.
- **Responsive.** On mobile (< 600px), recommendation cards are a horizontal scroll. On desktop (1024px+), they can be a grid row.

---

## 4. UI Mockup Descriptions

### 4.1 Direct Recommendation Scroll

```
┌──────────────────────────────────────────────────────────┐
│  Rediscover Your Library                          ↻ Refresh │
│  ┌──────┐  ┌──────┐  ┌──────┐  ┌──────┐  ┌──────┐       │
│  │ Box  │  │ Box  │  │ Box  │  │ Box  │  │ Box  │       │
│  │ Art  │  │ Art  │  │ Art  │  │ Art  │  │ Art  │       │
│  │      │  │      │  │      │  │      │  │      │       │
│  └──────┘  └──────┘  └──────┘  └──────┘  └──────┘       │
│  Sonic 2    Mario W   Zelda     Castlev   Street F       │
│  Mega Drive SNES      NES       GBA       Arcade         │
└──────────────────────────────────────────────────────────┘
```

- Reuses the existing `GameScrollCard` component (already has box art, name, system)
- Section title changes based on recommendation type: "Rediscover Your Library", "Because You Play [System]", "More Like Your Favorites", "Top Rated"
- Optional refresh button (re-rolls random picks without page reload)
- Cards are 120-140px wide, same as "Recently Played" scroll

### 4.2 Quick Discover Links (Search Recommendation Cards)

```
┌──────────────────────────────────────────────────────────┐
│  Discover                                                │
│  ┌─────────────────┐  ┌─────────────────┐               │
│  │ 🎮 Multiplayer  │  │ ⭐ Best Rated    │               │
│  │   127 games     │  │   Mega Drive     │               │
│  └─────────────────┘  └─────────────────┘               │
│  ┌─────────────────┐  ┌─────────────────┐               │
│  │ 🏃 Platformers  │  │ 👊 Fighting     │               │
│  │   342 games     │  │   198 games     │               │
│  └─────────────────┘  └─────────────────┘               │
└──────────────────────────────────────────────────────────┘
```

- 2x2 grid on mobile, 4-column row on desktop
- Each card is a styled `<A>` link to `/search?...` with pre-filled filters
- Subtle accent color from the current skin theme
- Small game count shown to indicate how many results to expect
- Cards vary based on available data:
  - Always: genre cards, multiplayer card
  - With favorites: "Best rated [favorite system]"
  - With metadata: rating-filtered cards

### 4.3 Personalized Variant (for users with play history)

```
┌──────────────────────────────────────────────────────────┐
│  Because You Love Mega Drive                    See all → │
│  ┌──────┐  ┌──────┐  ┌──────┐  ┌──────┐                 │
│  │      │  │      │  │      │  │      │                 │
│  │ Box  │  │ Box  │  │ Box  │  │ Box  │                 │
│  │      │  │      │  │      │  │      │                 │
│  └──────┘  └──────┘  └──────┘  └──────┘                 │
│  Shining F  Gunstar   Comix Z   Thunder                  │
│  ★ 4.8      ★ 4.7     ★ 4.5    ★ 4.3                   │
└──────────────────────────────────────────────────────────┘
```

- Appears when user has favorites or play history
- Title is dynamic ("Because You Love [System]" or "More [Genre] Games")
- "See all" links to the system ROM list or a pre-filtered search
- Shows rating badges when available
- Uses `GameScrollCard` with an optional rating overlay

---

## 5. Implementation Complexity

| Recommendation | Server Logic | UI Components | New Endpoints | Complexity |
|---|---|---|---|---|
| A. Random Picks | Extend `random_game` to return N results + box art | Reuse `GameScrollCard` | 1 new server fn | **Simple** |
| B. System-based (favorites) | Cross-reference favorites + ROMs + ratings | Reuse `GameScrollCard` | 1 new server fn | **Medium** |
| C. Genre-based (favorites) | Genre aggregation + cross-system search | Reuse `GameScrollCard` | 1 new server fn | **Medium** |
| D. Top Rated | Sort all_ratings + filter + box art | Reuse `GameScrollCard` | 1 new server fn | **Simple** |
| E. Best Rated System | Build URL from favorites/recents data | New `DiscoverCard` component | None (URL construction) | **Simple** |
| F. Best Genre | Extend `get_all_genres` with counts | New `DiscoverCard` component | Minor extension | **Simple** |
| G. Multiplayer | Static link + optional count | New `DiscoverCard` component | None | **Simple** |
| H. Year/Decade | Need year filter on search | New `DiscoverCard` component | 1 filter param addition | **Medium** |

---

## 6. Metadata Dependency Matrix

| Recommendation | Without Metadata | With Metadata |
|---|---|---|
| A. Random Picks | Works (placeholder art) | Full experience (box art) |
| B. System-based | Genre matching or random | Rating-sorted, box art |
| C. Genre-based | Genre matching (from embedded DB) | Rating-sorted, box art |
| D. Top Rated | **Not available** -- hide this block | Full experience |
| E. Best Rated System | Link works, but rating filter yields 0 results -- use unfiltered link | Rating filter works |
| F. Best Genre | Full experience (genre from embedded DB) | Same + box art in results |
| G. Multiplayer | Full experience (players from embedded DB) | Same + box art in results |
| H. Year/Decade | Full experience (year from embedded DB) | Same + box art in results |

**Graceful degradation strategy:**
- Detect whether metadata is downloaded (`metadata_db.is_empty()`)
- If no metadata: show types A, F, G (always work). Hide D, adjust E.
- If metadata present: show all types, with rating-based recommendations.
- If no favorites and no recents: show only A (random), F (genre), G (multiplayer).
- If favorites exist: add B and C.
- If both favorites and metadata exist: show the full set.

---

## 7. Proposed Server Functions

### `get_recommendations`

A single server function that returns all recommendation data in one call, adapting to what data is available.

```
Input: (none -- uses server-side state to determine what's available)

Output: RecommendationData {
    // Direct game recommendations (box art cards)
    random_picks: Vec<RecommendedGame>,      // always populated
    system_picks: Option<SystemPicks>,        // only if user has favorites
    genre_picks: Option<GenrePicks>,          // only if user has favorites
    top_rated: Option<Vec<RecommendedGame>>,  // only if metadata available

    // Search recommendation metadata (for building links)
    top_genres: Vec<(String, usize)>,         // genre name + count
    multiplayer_count: usize,                 // total multiplayer games
    favorite_systems: Vec<String>,            // top favorited system folders
    has_metadata: bool,                       // whether rating-based links are useful
}
```

**Why a single function:** The home page already fires 3 concurrent server function calls (`get_info`, `get_recents`, `get_systems`). Adding 4-5 more for individual recommendation types would strain the Pi's limited resources. A single call that computes all recommendations server-side is more efficient.

### `get_random_picks`

A simpler standalone function if recommendations are implemented incrementally:

```
Input: count: usize (default 6)
Output: Vec<RecommendedGame> {
    system: String,
    rom_filename: String,
    display_name: String,
    system_display: String,
    box_art_url: Option<String>,
    genre: String,
    rating: Option<f32>,
    href: String,
}
```

---

## 8. Phased Implementation Plan

### Phase 1: Random Discovery + Quick Links (Simple)

**Scope:** Add a "Rediscover" section with random game picks and a "Discover" section with genre/multiplayer quick links.

**Server changes:**
- New server function `get_random_picks(count)` -- returns N random games with box art URLs
- Extend `get_all_genres` to return `Vec<(String, usize)>` (genre + count) -- or a new `get_genre_counts` function

**UI changes:**
- New section in `home.rs` between "Recently Played" and "Library Stats"
- Reuse `GameScrollCard` for the random picks scroll
- New `DiscoverCard` component for the quick-filter links
- New `DiscoverGrid` component to lay out 2x2 / 4-column cards

**Effort:** ~1-2 sessions. Mostly UI layout work; server logic is trivial.

**Value:** Immediately makes the Home page more dynamic. Works without any metadata downloaded. The random picks change on every page load, giving a sense of freshness.

### Phase 2: Favorites-Based Recommendations (Medium)

**Scope:** When the user has favorites, replace or supplement the random picks with personalized recommendations.

**Server changes:**
- New server function `get_personalized_recommendations()` that:
  1. Loads favorites, counts per system and per genre
  2. For top system: picks highly-rated non-favorited games (or random if no ratings)
  3. For top genre: finds matching games across other systems
  4. Returns structured data for both recommendation blocks

**UI changes:**
- Dynamic section title ("Because You Love Mega Drive" / "More Like Your Favorites")
- "See all" link on each recommendation block
- Optional rating badge on game cards

**Effort:** ~2-3 sessions. The cross-referencing logic is the main work.

**Value:** Makes the app feel personal. Directly addresses the user analysis finding that "there is no way to discover games within the existing library."

### Phase 3: Rating-Based Recommendations (Simple, metadata-dependent)

**Scope:** When metadata is downloaded, add "Top Rated" recommendations and rating-filtered quick links.

**Server changes:**
- Use existing `all_ratings()` + ROM cache to find top-rated games
- Adjust quick links to include `min_rating=4` parameter

**UI changes:**
- New recommendation block "Top Rated in Your Library"
- Quick links update: "Best rated Mega Drive games" with rating filter

**Effort:** ~1 session. All the data access functions already exist.

**Value:** Leverages the metadata the user invested time in downloading. Creates a reward loop: download metadata -> see curated recommendations.

### Phase 4: Year-Based Discovery (Medium)

**Scope:** Add decade-based discovery links ("90s Classics", "Games from 1992").

**Server changes:**
- Add `min_year` / `max_year` parameters to `get_roms_page` and `global_search`
- New helper to compute year distribution across the library

**UI changes:**
- Decade cards in the Discover section
- Possibly a "This day in gaming history" widget for extra delight

**Effort:** ~1-2 sessions. The year data exists; exposing it as a filter requires touching the existing server functions.

**Value:** Nostalgia-driven discovery. Particularly appealing for Persona A (casual retro gamer).

---

## 9. Inspiration from User Analysis

The user analysis document (`user-analysis.md`) and UX feedback analysis (`ux-feedback-analysis.md`) identify several points that this feature directly addresses:

### From User Analysis

> **Section 5, #7:** "Related games on game detail page. 'More by this developer' or 'More in this genre' links from the game detail page would improve discovery flow."

The genre-based recommendations (type C) and quick-filter links (type F) bring this concept to the home page level.

> **Section 5, #5:** "Random game suggestion. A 'Surprise me' button that picks a random game."

The random picks recommendation (type A) surfaces this at the home page level, more prominently than the existing "Random Game" button buried on the search page's empty state.

> **Section 4.5:** "No sorting on ROM lists. For discovery ('show me games from 1993'), this is a significant gap."

The year-based discovery links (type H) address this without requiring sort implementation.

### From UX Feedback Analysis

> **Section 1:** "There is no global search accessible from the home page."

While global search has since been added, the search recommendation links (types E-H) extend this by providing curated entry points -- you do not even need to know what to search for.

> **Section 2:** "No filtering by genre, players, year, or region."

The genre and multiplayer filters have been implemented on the ROM list and search pages. The recommendation links expose these powerful filters through easily tappable cards, lowering the discovery barrier.

> **Section 7, #5:** "Random game suggestion [...] uniquely valuable for retro gaming where the library is large and the user may not know what they want to play."

Type A (random picks) puts this front and center.

### Persona Alignment

| Persona | Most Relevant Recommendations |
|---|---|
| A. Casual Retro Gamer | Random picks (A), Top Rated (D), Year-based (H) |
| B. Collector / Curator | Genre-based (C), System-based (B), Top Rated (D) |
| C. Parent | Top Rated (D), Multiplayer (G), Genre links (F) |
| D. Arcade Cabinet Builder | Genre links (F -- "Best Fighting games"), Multiplayer (G) |
| E. Technical User | Less relevant, but appreciates the dynamic content |

---

## 10. Edge Cases & Considerations

### Empty library
When total_games == 0, show no recommendations. The existing "No games" states handle this.

### Very small library (< 20 games)
Random picks might repeat across page loads. Consider deduplicating against recently shown picks (stored in localStorage or session).

### All games already favorited
The "non-favorited games from your favorite system" logic in type B would return empty. Fall back to type A (random) or type D (top rated).

### Single-system library
System-based recommendations become trivial ("Because you love Mega Drive" when it is the only system). In this case, skip system-based (B) and focus on genre-based (C) and rating-based (D) within that system.

### Performance on Raspberry Pi
The home page already makes 3 server calls. Adding a 4th for recommendations is acceptable if the function is efficient. The key constraint: do not iterate all ROMs for all systems. Use the in-memory cache (already populated for system counts) and keep genre/favorites analysis lightweight. Target < 50ms for the recommendation server function.

### Cache invalidation
Random picks should change on each page load (no caching). Personalized recommendations should update when favorites change but can be cached for 5-10 minutes otherwise (they depend on favorites + ratings which change infrequently).

### Refresh mechanism
The random picks scroll should have a small refresh icon that re-rolls without a full page reload. On click, invalidate just the recommendation Resource. Leptos makes this easy with `Resource::refetch()`.

---

## 11. Summary

| Phase | Recommendations Added | Complexity | Metadata Required |
|---|---|---|---|
| Phase 1 | Random picks (A) + Genre links (F) + Multiplayer link (G) | Simple | No |
| Phase 2 | System-based (B) + Genre-based (C) favorites recommendations | Medium | No (enhanced with) |
| Phase 3 | Top Rated (D) + Rating-filtered quick links (E) | Simple | Yes |
| Phase 4 | Year/Decade discovery (H) | Medium | No |

Phase 1 delivers the highest value-to-effort ratio: the Home page goes from static (always showing the same content in the same order) to dynamic (different random games each visit, curated entry points into the library). It works for all users, including first-time users with no metadata, and it requires only one new server function plus two small UI components.
