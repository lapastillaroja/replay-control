# Game Series and Franchises

How game series/franchise data is sourced, matched, and displayed.

## Overview

Replay Control identifies games that belong to the same series or franchise and surfaces this information on the game detail page. This enables sequel/prequel navigation, cross-system series browsing, and helps users discover related games they may want to play next.

## Data Sources

### Wikidata (Primary)

The primary source is Wikidata, queried via SPARQL at build time and embedded into the binary. The extract uses:

- **P179** (part of the series) -- links a game to its franchise
- **P155** (follows) / **P156** (followed by) -- sequel/prequel chains
- **P1545** (series ordinal) -- numeric position within the series

The embedded database contains ~5,345 entries across 194+ series, covering both console and arcade systems. Platform QIDs map to RePlayOS system folder names.

### Algorithmic Fallback

When Wikidata has no series data for a game, the system uses `series_key` -- an algorithmically computed grouping key derived from the game's base title with trailing numbers removed. This catches obvious sequels (e.g., "Sonic the Hedgehog", "Sonic the Hedgehog 2", "Sonic the Hedgehog 3" all share the same series key) but cannot identify non-sequential series members.

### Alias Resolution

Cross-name variants are also linked as series siblings. The alias system (TGDB alternate names, LaunchBox alternate names, bidirectional fuzzy matching for colon/dash variants) identifies games known under different names across regions, such as "Bare Knuckle" / "Streets of Rage".

## Matching Pipeline

At scan time (during enrichment), Wikidata entries are matched to library ROMs through:

1. **Normalized title matching** -- both the Wikidata entry title and the ROM display name are normalized (lowercase, strip non-alphanumeric, collapse whitespace)
2. **Roman numeral normalization** -- "streets of rage ii" matches "streets of rage 2"
3. **Cross-system matching** -- a game's Wikidata entry may list a different platform (e.g., Metal Slug X is listed under sony_psx in Wikidata but may exist in the library as arcade_fbneo). All Wikidata entries are checked regardless of platform.
4. **Subtitle-stripped fallback** -- strips text after colons/dashes for looser matching (catches games like "DonPachi II")

## Game Detail Display

### Series Siblings

On the game detail page, series siblings appear as a horizontal scroll of game cards with box art under the series name heading (e.g., "Streets of Rage"). These include:

- Other games in the same Wikidata series that exist in the user's library
- Cross-system ports (the same game on other systems)
- Cross-name aliases (regional title variants)

The current game is excluded from the siblings list, but cross-system ports of the current game are included.

### Sequel/Prequel Navigation

A breadcrumb-style navigation bar shows the play order within the series:

```
< Prev Title  |  2 of 5  |  Next Title >
```

- **Prev/Next** links use Wikidata P155/P156 chains when available, with P1545 ordinal fallback
- **Bidirectional link filling** at build time: `build.rs` runs a reverse-link pass that ensures if game A has P156 (followed by B), then B gets P155 (follows A) even if Wikidata only has one direction
- **Clone ROM fallback**: sequel link targets prefer non-clone ROMs, but fall back to clone entries when the non-clone version is not in the library
- Games in the user's library are linked to their detail page; games not in the library show the title without a link
- The "N of M" counter shows position using Wikidata ordinals

## Key Source Files

| File | Role |
|------|------|
| `replay-control-core/src/game/series_db.rs` | Embedded Wikidata series database, lookup functions |
| `replay-control-core/src/metadata/metadata_db/aliases_series.rs` | game_series and game_aliases tables, alias resolution |
| `replay-control-core/src/metadata/alias_matching.rs` | Alias matching pipeline (TGDB, LaunchBox, fuzzy) |
| `replay-control-core/src/game/title_utils.rs` | Title normalization, roman numeral conversion |
| `replay-control-app/src/server_fns/related.rs` | RelatedGamesData, SequelLink, series siblings resolution |
| `replay-control-app/src/api/cache/aliases.rs` | Series data population during enrichment |
| `replay-control-core/build.rs` | Build-time Wikidata series database generation |
