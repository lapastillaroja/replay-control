# Search

How global search works: matching, scoring, and filtering.

## Overview

Global search is a cross-system search accessible from the top bar icon, the home page search bar, and the `/` keyboard shortcut. Results are served via the `global_search()` server function.

## Search Flow

1. User types a query (debounced 300ms)
2. Server loads all ROMs from L1/L2 cache across all systems
3. Each ROM is scored against the query
4. Results are filtered by optional criteria (genre, driver status, favorites-only)
5. Top results are returned sorted by score, paginated

## Scoring Algorithm

Word-level fuzzy matching in `server_fns/search.rs`:

1. **Tokenize** query and candidate name into words
2. **Word boundary matching**: bonus for matches at word boundaries (e.g., "mario" matches "Super Mario World" better than "mariobros")
3. **Substring matching**: each query word is checked against each name word
4. **Region preference bonus**: ROMs in the user's preferred region score higher
5. **Penalties**: Hacks and translations receive score penalties to rank below originals
6. **Display name matching**: Both filename and display name (from arcade_db/game_db) are checked; best score wins

## Filters

URL-persisted query parameters on `/search`:

| Filter | Parameter | Options |
|--------|-----------|---------|
| Genre | `genre` | Any normalized genre from the taxonomy |
| Driver Status | `status` | Working, Imperfect, Preliminary (arcade only) |
| Favorites Only | `fav` | Boolean |
| Min Rating | `rating` | Minimum LaunchBox community rating |

Genre resolution uses `lookup_genre()` which falls back to LaunchBox data when the baked-in database has no genre.

## Recent Searches

The last N search queries are stored client-side and displayed as quick-access chips on the search page.

## Random Game

A "Random Game" button picks a random ROM from the library and navigates to its game detail page.

## Key Source Files

| File | Role |
|------|------|
| `replay-control-app/src/server_fns/search.rs` | `global_search()`, scoring, `lookup_genre()` |
| `replay-control-app/src/pages/search.rs` | Search page UI, URL param persistence |
| `replay-control-app/src/components/search_bar.rs` | Top bar search icon, input handling |
