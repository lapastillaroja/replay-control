# Related Games Feature Plan

## Overview

Add two "related games" sections to the game detail page (`/games/:system/:filename`):

1. **Regional Variants** -- other versions of the same game (USA, Europe, Japan, etc.)
2. **More Like This** -- games from the same system sharing the same genre

Both sections load lazily via a separate `Resource` so they never block the main game detail render.

## Text Wireframes (Mobile-First)

### Regional Variants (below Info card)

When variants exist (2+ regions for the same base_title):

```
+------------------------------------------+
| Regional Variants                        |
|                                          |
|  [USA]  [Europe]  [Japan]  [Brazil]      |
|                                          |
+------------------------------------------+
```

Each chip is a link to that variant's detail page. The current game's region is visually distinct (filled/active style). Chips show the region string as-is from the DB (e.g., "USA", "Europe", "Japan").

When only one region exists: section is hidden entirely (no empty state).

### More Like This (below Videos section)

When similar games exist:

```
+------------------------------------------+
| More Like This                           |
|                                          |
| [card] [card] [card] [card] -->          |
|                                          |
| Each card:                               |
| +--------+                               |
| | boxart |                               |
| +--------+                               |
| Game Name                                |
+------------------------------------------+
```

Horizontal scrollable row using the existing `recent-scroll` + `GameScrollCard` pattern from the home page. No system subtitle needed since all games are from the same system.

When no similar games found (no genre, or only game of that genre): section is hidden entirely.

## Data Model

### Existing Schema (no changes needed)

The `game_library` table already has the columns we need:

```sql
-- PRIMARY KEY (system, rom_filename)
-- base_title TEXT NOT NULL DEFAULT ''
-- region TEXT NOT NULL DEFAULT ''
-- genre TEXT
-- is_clone INTEGER NOT NULL DEFAULT 0
```

### New Index

Add one composite index to make the "More Like This" query fast:

```sql
CREATE INDEX IF NOT EXISTS idx_game_library_genre
  ON game_library (system, genre)
  WHERE genre IS NOT NULL AND genre != '';
```

The regional variants query uses `(system, base_title)` which already performs well via the primary key prefix scan on `system`. No extra index needed since the result set per system+base_title is tiny (typically 1-5 rows).

## Server Function Design

### Response Struct

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedGamesData {
    /// Other regions of the same game. Empty if only one region exists.
    pub regional_variants: Vec<RegionalVariant>,
    /// Games from the same system + genre. Empty if no genre or no matches.
    pub similar_games: Vec<RecommendedGame>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionalVariant {
    pub rom_filename: String,
    pub region: String,
    pub href: String,
    /// True if this is the current game (for active chip styling).
    pub is_current: bool,
}
```

Reuses the existing `RecommendedGame` struct for similar games (it already has `system`, `display_name`, `rom_filename`, `box_art_url`, `href`).

### Server Function

```rust
#[server(prefix = "/sfn")]
pub async fn get_related_games(
    system: String,
    filename: String,
) -> Result<RelatedGamesData, ServerFnError>
```

Implementation outline:

1. Get `AppState` from context
2. Use `cache.with_db_read()` to run both queries in a single DB lock
3. For regional variants: query by `(system, base_title)` where `base_title` matches the current game's
4. For similar games: query by `(system, genre)`, exclude current game, randomize, limit 8
5. Resolve box art URLs for similar games using `resolve_box_art_for_picks()`
6. Return `RelatedGamesData`

## SQL Queries

### Regional Variants

```sql
-- Step 1: get current game's base_title
SELECT base_title FROM game_library
WHERE system = ?1 AND rom_filename = ?2;

-- Step 2: find all games with same base_title (only if base_title is non-empty)
SELECT rom_filename, region FROM game_library
WHERE system = ?1 AND base_title = ?2 AND base_title != ''
ORDER BY
  CASE region
    WHEN 'USA' THEN 1
    WHEN 'Europe' THEN 2
    WHEN 'Japan' THEN 3
    ELSE 4
  END,
  region;
```

Both can be combined into a single subquery:

```sql
SELECT rom_filename, region FROM game_library
WHERE system = ?1
  AND base_title != ''
  AND base_title = (
    SELECT base_title FROM game_library
    WHERE system = ?1 AND rom_filename = ?2
  )
ORDER BY
  CASE region
    WHEN 'USA' THEN 1
    WHEN 'Europe' THEN 2
    WHEN 'Japan' THEN 3
    ELSE 4
  END,
  region;
```

### More Like This (Console)

```sql
SELECT system, rom_filename, rom_path, display_name, size_bytes,
       is_m3u, box_art_url, driver_status, genre, players, rating,
       is_clone, base_title, region
FROM game_library
WHERE system = ?1
  AND genre = ?2
  AND genre != ''
  AND rom_filename != ?3
ORDER BY RANDOM()
LIMIT 8;
```

### More Like This (Arcade -- prefer same category)

For arcade systems, first try matching by `arcade_category` (looked up from `arcade_db`), then fall back to genre:

```sql
-- The arcade_category is not in game_library, so we filter in Rust after querying by genre.
-- Query a larger pool by genre, then prefer games whose arcade_category matches.
SELECT system, rom_filename, rom_path, display_name, size_bytes,
       is_m3u, box_art_url, driver_status, genre, players, rating,
       is_clone, base_title, region
FROM game_library
WHERE system = ?1
  AND genre = ?2
  AND genre != ''
  AND rom_filename != ?3
  AND is_clone = 0
ORDER BY RANDOM()
LIMIT 24;
```

Then in Rust: partition by matching `arcade_category` (via `arcade_db::lookup_arcade_game`), take up to 8 preferring same-category games, fill remainder from the rest.

## Component Design

### New Components

**`RelatedGamesSection`** -- top-level component placed in `GameDetailContent`

Props: `system: StoredValue<String>`, `rom_filename: StoredValue<String>`, `genre: StoredValue<String>`, `arcade_category: StoredValue<Option<String>>`

- Creates a `Resource` that calls `get_related_games(system, filename)`
- Renders `RegionalVariantsChips` and `SimilarGamesRow` conditionally
- Uses `Transition` (not `Suspense`) to avoid flickering since it's a secondary section

**`RegionalVariantsChips`** -- renders the chip row

Props: `variants: Vec<RegionalVariant>`

- Only rendered when `variants.len() > 1`
- Each chip is an `<A>` link to the variant's game detail page
- Current game's chip gets `.active` class

**`SimilarGamesRow`** -- renders the horizontal scroll row

Props: `games: Vec<RecommendedGame>`

- Only rendered when `games` is non-empty
- Reuses `GameScrollCard` from `components/hero_card.rs` inside a `div.recent-scroll`
- No system subtitle on cards since all are same system

### Component Tree

```
GameDetailContent
  ...existing sections...
  RelatedGamesSection          <-- new, after Videos, before Manual
    RegionalVariantsChips      <-- conditionally rendered
    SimilarGamesRow            <-- conditionally rendered
```

### Placement in game_detail.rs

Insert between the Videos section and the Manual section:

```rust
// Related Games (lazy-loaded)
<RelatedGamesSection
    system=system_sv
    rom_filename=filename_sv
    genre=genre
    arcade_category=arcade_category
/>

// Manual
<section class="section game-section">
    ...
</section>
```

## Files to Modify

| File | Change |
|------|--------|
| `replay-control-core/src/metadata/metadata_db.rs` | Add `regional_variants()` and `similar_games()` methods on `MetadataDb`. Add `idx_game_library_genre` index in `create_game_library_tables()`. |
| `replay-control-app/src/server_fns/mod.rs` | Add `mod related;` and `pub use related::*;`. |
| `replay-control-app/src/server_fns/related.rs` | **New file.** `RelatedGamesData`, `RegionalVariant` structs, `get_related_games` server function. |
| `replay-control-app/src/pages/game_detail.rs` | Import and render `RelatedGamesSection`. Add `RelatedGamesSection`, `RegionalVariantsChips`, `SimilarGamesRow` components (or put them in a separate file under `components/`). |
| `replay-control-app/src/main.rs` | Add `register_explicit::<GetRelatedGames>()`. |
| `replay-control-app/style/_07-game-detail.css` | Add styles for `.regional-variants`, `.region-chip`, `.region-chip.active`. The `SimilarGamesRow` reuses existing `.recent-scroll` and `GameScrollCard` styles. |
| `replay-control-app/src/i18n/*.ftl` | Add keys: `game_detail-regional-variants`, `game_detail-more-like-this`. |

## Edge Cases

| Case | Handling |
|------|----------|
| **No genre** | `similar_games` query returns empty; "More Like This" section hidden |
| **Single-region game** | `regional_variants` has 1 entry; section hidden (only show when >1) |
| **Empty `base_title`** | Variants query filters `base_title != ''`; section hidden |
| **Arcade games** | No `region` field -- regional variants section always hidden. "More Like This" prefers same `arcade_category`, excludes clones |
| **Arcade clones** | Excluded from "More Like This" results via `is_clone = 0` |
| **Current game is the only one of its genre** | Query returns empty; section hidden |
| **Game has genre but < 8 matches** | Show however many exist (no minimum) |
| **NFS storage** | DB already uses `nolock` VFS fallback; no special handling needed |
| **game_library not yet populated** | `with_db_read()` returns `None`; both sections hidden |
