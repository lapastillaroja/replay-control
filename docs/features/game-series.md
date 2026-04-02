# Game Series and Franchises

How game series and franchise relationships are displayed.

## Overview

Replay Control identifies games that belong to the same series or franchise and surfaces this on the game detail page. This enables sequel/prequel navigation, cross-system series browsing, and helps you discover related games.

## Data Sources

### Wikidata (Primary)

The primary source is [Wikidata](https://www.wikidata.org/), with ~5,345 entries across 194+ series covering both console and arcade systems. The data includes franchise membership, sequel/prequel chains, and series ordinals.

### Algorithmic Fallback

When Wikidata has no series data for a game, the app groups games by title similarity. Games that share the same base title (e.g., "Sonic the Hedgehog", "Sonic the Hedgehog 2", "Sonic the Hedgehog 3") are grouped together automatically.

### Cross-Name Aliases

Regional title variants are also linked. For example, "Bare Knuckle" and "Streets of Rage" are recognized as the same franchise through alternate name databases.

## Game Detail Display

### Series Siblings

On the game detail page, series siblings appear as a horizontal scroll of game cards with box art under the series name heading (e.g., "Streets of Rage"). These include:

- Other games in the same series that exist in your library
- Cross-system ports (the same game on other systems)
- Cross-name aliases (regional title variants)

### Sequel/Prequel Navigation

A breadcrumb bar shows play order within the series:

```
< Prev Title  |  2 of 5  |  Next Title >
```

- Prev/Next links follow the series order
- Games in your library link to their detail page; games not in your library show the title without a link
- The "N of M" counter shows position within the series
