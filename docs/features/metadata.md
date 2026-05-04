# Metadata

How game metadata is sourced and used.

## Offline-First Design

Replay Control works fully offline from the first install. Built-in databases provide genre, player count, year, and display names for ~34K console ROMs and ~15K playable arcade games without any network access.

When connected to the internet, you can optionally enrich your library with additional data from external sources. These fill gaps that the built-in data does not cover but are never required.

## Built-In Data

### Console Games (~34K ROMs)

Display names, year, genre, developer, player count, and region for games across 20+ systems. Data is sourced from [No-Intro](https://datomatic.no-intro.org/), [TheGamesDB](https://thegamesdb.net/), and libretro-database at build time.

Games are identified by filename, with a CRC32 hash fallback for 9 cartridge systems when the filename does not match the database.

### Arcade Games (~15K playable entries)

Covers [MAME](https://www.mamedev.org/), [FBNeo](https://github.com/finalburnneo/FBNeo), and [Flycast](https://github.com/flyinghead/flycast) (Naomi/Atomiswave) arcade systems. Each entry includes display name, year, manufacturer, player count, rotation, driver status, clone/parent relationships, and category.

Non-playable machines (slot machines, gambling, etc.) are filtered out.

### Genre Taxonomy

Both console and arcade databases map to a shared set of ~18 normalized genres: Action, Adventure, Beat'em Up, Board & Card, Driving, Educational, Fighting, Maze, Music, Pinball, Platform, Puzzle, Quiz, Role-Playing, Shooter, Simulation, Sports, Strategy, and Other.

### Series Data

~5,345 game series entries across 194+ franchises from [Wikidata](https://www.wikidata.org/), with sequel/prequel chains and ordinals. See [Game Series](game-series.md) for details.

## External Metadata (Optional)

### LaunchBox Refresh

Download the [LaunchBox](https://gamesdb.launchbox-app.com/) XML file (~460 MB) from the metadata page. One-button "Refresh metadata" handles download → parse → match → enrich:

- Real-time progress updates (downloading, parsing, enriching) via the activity SSE stream
- The XML is parsed once into the host-global `external_metadata.db` (`/var/lib/replay-control/external_metadata.db`); per-storage caches no longer hold a copy
- Boot-time freshness is content-derived (CRC32 of the XML vs. the last-parsed stamp) — newly added ROMs after a refresh pick up metadata automatically on the next enrichment
- Per-system coverage stats refresh after enrichment completes

Data imported: description, rating, rating count, publisher, developer, genre, max players, release date, and cooperative flag.

Where the built-in database already has a value (e.g., genre), it takes priority. LaunchBox data only fills gaps. Description and publisher are denormalized into a per-storage `game_description` cache so the game-detail page reads them from a single pool.

### Box Art and Screenshots

See [Thumbnails](thumbnails.md) for image downloads from libretro-thumbnails.

## ROM Tag Parsing

ROM filenames are parsed to extract region, revision, and classification tags. Supported naming conventions:

- **[No-Intro](https://datomatic.no-intro.org/)** -- parenthesized tags: `(USA)`, `(Rev 1)`, `(Hack)`, `(Beta)`, etc.
- **GoodTools** -- bracket flags: `[!]` verified, `[h]` hack, `[T-Spa]` translation, etc.
- **[TOSEC](https://www.tosecdev.org/)** -- structured tags: year, publisher, side/disk, country codes, language codes, format suffix

### ROM Classification

ROMs are classified into tiers that affect their visibility in recommendations and variant sections:

| Category | Examples | Effect |
|----------|----------|--------|
| Original | No special tags | Included in recommendations |
| Revision | `(Rev 1)`, `(Rev A)` | Shown as variant, included in recommendations |
| Translation | `(Traducido Es)`, `[T+Spa]` | Separate section, excluded from recommendations |
| Hack | `(Hack)`, `[h1]` | Separate section, excluded from recommendations |
| Special | `(Unl)`, `(Homebrew)`, `(Beta)`, `(Pirate)` | Excluded from recommendations |

## Cache Management

The metadata page provides tools to manage stored data:

- **Clear metadata** -- wipes `launchbox_game` + `launchbox_alternate` and resets the XML hash stamp so the next refresh re-parses from disk
- **Clear images** -- removes downloaded box art and screenshots from disk + clears `box_art_url` from `game_library`
- **Cleanup orphaned images** -- removes downloaded images no longer associated with any game in the library, with a safety threshold per system to prevent accidental mass deletion
