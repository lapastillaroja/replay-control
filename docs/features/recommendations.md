# Recommendations

How the home page recommendation engine works.

## Overview

The home page shows several recommendation blocks, all powered by SQL queries against the `game_library` table in `metadata.db`. No per-ROM filesystem I/O is needed at render time.

## Home Page

### Rediscover Your Library (Random Picks)
`random_cached_roms_diverse()` selects random games with genre diversity. Uses a dedup CTE that partitions by `(system, base_title)` and picks one ROM per game (preferring the user's region). Results are shuffled and genre-balanced.

### Because You Love (Favorites-Based)
`system_roms_excluding()` takes the user's favorited systems and genres, then finds other games in those categories. Excludes already-favorited games.

### Curated Spotlight (Rotating)
One section per page load, randomly picked from 5 types:

| Type | Title | Query |
|------|-------|-------|
| Global Top Rated | "Top Rated" | `top_rated_filtered(None, None, None)` |
| Best by Genre | "Best Platformers" | `top_rated_filtered(None, Some(genre), None)` |
| Best of System | "Best of Mega Drive" | `top_rated_filtered(Some(system), None, None)` |
| Games by Developer | "Games by Capcom" | `top_rated_filtered(None, None, Some(developer))` |
| Hidden Gems | "Hidden Gems" | `top_rated_filtered` excluding recents + favorites, prefer low rating_count |

Uses `top_rated_filtered()` — a generic rated-games query with optional system/genre/developer filters. Minimum 6 games per spotlight; falls back to global Top Rated if insufficient. Rating threshold: 3.5+.

Weighted scoring penalizes games with few votes:
- 10+ votes: full rating
- 3-9 votes: 90% of rating
- 0-2 votes: 70% of rating

### Discover Pills
Rotating set of 5 pills linking to filtered search/browse pages: genre, system, developer, decade, multiplayer, 4-player.

## Favorites Page

### Because You Love [Game]
Picks a random favorite, finds similar games by genre (cross-system), fills with developer matches. Excludes already-favorited games. Shows 6 games. Section title uses `strip_tags()` to remove region/revision tags.

### More from [Series]
Looks up `series_key` for all favorites, finds series siblings not yet favorited. Uses proper display name from `game_series.series_name` table.

## Game Detail Page

### Related Games (Genre Similarity)
`similar_by_genre()` finds games sharing the same normalized genre, excluding the current game.

## Deduplication

All recommendation queries use a common dedup CTE pattern:

```sql
WITH deduped AS (
    SELECT *, ROW_NUMBER() OVER (
        PARTITION BY system, base_title
        ORDER BY CASE WHEN region = ?pref THEN 0 WHEN region = 'world' THEN 1 ELSE 2 END
    ) AS rn
    FROM game_library
    WHERE is_clone = 0 AND is_translation = 0 AND is_hack = 0 AND is_special = 0
)
SELECT ... FROM deduped WHERE rn = 1
```

This ensures:
- One ROM per game (by `base_title` within a system)
- Region preference respected (USA > World > others, configurable)
- Clones, translations, hacks, and special ROMs excluded

## What Gets Filtered

| Category | Flag | Excluded From |
|----------|------|---------------|
| Arcade clones | `is_clone = 1` | All recommendations |
| Translations | `is_translation = 1` | All recommendations |
| Hacks | `is_hack = 1` | All recommendations |
| FastROM/60Hz patches, unlicensed, homebrew, pre-release, pirate | `is_special = 1` | All recommendations |
| Regional variants | Dedup CTE (one per base_title) | Keeps only preferred region |

## Box Art Resolution

Each recommended game's box art URL is resolved via the same 5-tier pipeline used in ROM lists (see `docs/features/thumbnails.md`). The URL is stored in `game_library.box_art_url` after enrichment.

## Key Source Files

| File | Role |
|------|------|
| `replay-control-core/src/metadata/metadata_db/recommendations.rs` | `top_rated_filtered`, `random_cached_roms_diverse`, `top_developers`, etc. |
| `replay-control-app/src/server_fns/recommendations.rs` | Home page: `get_recommendations()`, spotlight rotation, `GameSection` struct |
| `replay-control-app/src/server_fns/favorites.rs` | Favorites page: `get_favorites_recommendations()` |
| `replay-control-app/src/server_fns/related.rs` | Game detail: related games, regional variants |
| `replay-control-app/src/api/cache/enrichment.rs` | `enrich_system_cache` populates box_art_url and rating |
