# Search

How global search, developer search, and the developer game list work.

## Global Search

Cross-system search accessible from the bottom nav Search tab, the home page search bar, or the `/` keyboard shortcut.

### How It Works

- Type a query (results appear after a short debounce)
- Search matches against both ROM filenames and display names using word-level fuzzy matching
- Results are ranked with region preference bonuses and hack/translation penalties
- Near-instant results across 23K+ games

### Filters

All filters are persisted in the URL, so you can share or bookmark filtered searches:

| Filter | Options |
|--------|---------|
| Genre | Any normalized genre (Action, Platform, Shooter, etc.) |
| Driver Status | Working, Imperfect, Preliminary (arcade only) |
| Favorites Only | Show only favorited games |
| Min Rating | Minimum community rating threshold |

### Recent Searches

Your last search queries are stored and displayed as quick-access chips on the search page.

### Random Game

A "Random Game" button picks a random ROM from the library and navigates to its detail page.

## Developer Search

When your search query matches a developer or manufacturer name, a "Games by Developer" block appears above the regular search results. This shows:

- The top-matched developer's games in a horizontal scroll with box art
- Up to 2 additional matching developers as tappable links with game counts

## Developer Game List

Each developer has a dedicated page (`/developer/:name`) with:

- **System filter chips** across the top (all systems the developer has games on, with counts)
- **Content filters** -- hide hacks, hide translations, hide clones, multiplayer only, genre filter, minimum rating
- **Infinite scroll** with pagination
- **Cross-system game list** with system badges on each card
