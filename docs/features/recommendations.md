# Recommendations

How the recommendation engine works across the home page, favorites page, and game detail page.

## Home Page

The home page shows several recommendation blocks, each tailored to your library:

### Rediscover Your Library

Genre-diverse random picks from your collection, shuffled on each visit.

### Top Rated

Highest-rated games weighted by community vote count. Games with few votes are penalized to avoid low-confidence entries dominating the list.

### Because You Love

Based on your favorited games' genres and systems. Finds other games in those categories that you have not favorited yet.

### Curated Spotlight (Rotating)

One section per page load, randomly picked from several types:

| Type | Example Title |
|------|---------------|
| Best by Genre | "Best Platformers" |
| Best of System | "Best of Mega Drive" |
| Games by Developer | "Games by Capcom" |
| Hidden Gems | Highly rated games you haven't played yet |

Requires a minimum of 6 games to show. Falls back to a global "Top Rated" section if the chosen type has too few results. Minimum rating threshold: 3.5.

### Discover Pills

A rotating set of 5 quick-link chips that let you browse by genre, system, developer, decade, or multiplayer mode.

## Favorites Page

### Because You Love [Game]

Picks a random favorite and finds similar games by genre across systems, supplemented by developer matches. Shows 6 games, excluding already-favorited titles.

### More from [Series]

Finds series siblings for all your favorited games that you have not yet favorited.

## Game Detail Page

### Related Games

Genre-based recommendations: games sharing the same genre as the current game.

## Filtering and Deduplication

All recommendations apply consistent filtering:

- **One ROM per game** -- when multiple region variants exist, only the preferred-region version is shown
- **Region preference** -- respects your configured region preference (Settings > Region)
- **Exclusions** -- clones, translations, hacks, and special ROMs (unlicensed, homebrew, pre-release, pirate) are excluded
- **No duplicates** -- a game never appears twice across recommendation sections
