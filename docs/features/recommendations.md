# Recommendations

How the home page recommendation engine works.

## Overview

The home page shows several recommendation blocks, all powered by SQL queries against the `game_library` table in `metadata.db`. No per-ROM filesystem I/O is needed at render time.

## Recommendation Blocks

### Random Picks (Diverse)
`random_cached_roms_diverse()` selects random games with genre diversity. Uses a dedup CTE that partitions by `(system, base_title)` and picks one ROM per game (preferring the user's region). Results are shuffled and genre-balanced.

### Top Rated
`top_rated_cached_roms()` returns the highest-rated games by LaunchBox community rating. Same dedup CTE to avoid showing multiple variants of the same game. Results are randomized within top-N to avoid a static list.

### Multiplayer
Filters for games with `players >= 2`. Random selection with dedup.

### Because You Love (Favorites-Based)
`system_roms_excluding()` takes the user's favorited systems and genres, then finds other games in those categories. Excludes already-favorited games.

### Related Games (Genre Similarity)
`similar_by_genre()` on the game detail page finds games sharing the same normalized genre, excluding the current game.

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
| `replay-control-core/src/metadata/metadata_db.rs` | All recommendation SQL queries |
| `replay-control-app/src/server_fns/search.rs` | `lookup_genre()` with LaunchBox fallback |
| `replay-control-app/src/server_fns/related.rs` | Related games, regional variants |
| `replay-control-app/src/api/cache.rs` | `enrich_system_cache` populates box_art_url and rating |
