# Metadata

How game metadata is sourced and used.

{{< screenshot "metadata-mobile.png" "Game metadata settings" >}}

## Offline-First Design

Replay Control works fully offline from the first install. Built-in databases provide genre, player count, year, and display names for ~34K console ROMs and broad arcade metadata without any network access.

When connected to the internet, you can optionally enrich your library with additional data from external sources. These fill gaps that the built-in data does not cover but are never required.

## Built-In Data

### Console Games (~34K ROMs)

Display names, year, genre, developer, player count, and region for games across 20+ systems. Data is sourced from [No-Intro](https://datomatic.no-intro.org/), [TheGamesDB](https://thegamesdb.net/), and libretro-database at build time.

Games are identified by the strongest available signal:

- CRC32 hash matches for cartridge systems covered by No-Intro data
- RetroAchievements runtime hashes for headered cartridge systems and supported disc systems
- Filename/title matching as the fallback for systems or files without a hash identity path

Hash-identified rows are marked as verified. Title fallback still gives useful metadata coverage for hacks, translations, regional variants, and systems without a hash source, but it is not treated as a verified dump match.

Commodore Amiga and Amiga CD32 titles are identified across their common naming conventions (WHDLoad, ADF, IPF) and given clean display names — for example "SuperFrog" instead of "SuperFrog_v1.1_0485 (Europe)" — while regional variants you own as distinct games keep a clean region suffix so they stay distinguishable.

### Arcade Games

Covers [MAME](https://www.mamedev.org/), [FBNeo](https://github.com/finalburnneo/FBNeo), and [Flycast](https://github.com/flyinghead/flycast) (Naomi/Atomiswave) arcade systems. Each entry includes display name, year, manufacturer, player count, rotation, driver status, clone/parent relationships, category, and the hardware **board** it ran on (CPS-2, Neo Geo MVS, Taito F3, …).

Entries from the source metadata are retained, including categories such as gambling, slot machine, computer, handheld, and electromechanical, so ROMs from full MAME sets can still be identified.

The board powers a dedicated browse-by-board experience — board pages, board search, and board recommendations. See [Arcade Boards](arcade-boards.md).

### Genre Taxonomy

Both console and arcade databases map to a shared set of ~18 normalized genres: Action, Adventure, Beat'em Up, Board & Card, Driving, Educational, Fighting, Maze, Music, Pinball, Platform, Puzzle, Quiz, Role-Playing, Shooter, Simulation, Sports, Strategy, and Other.

### Series Data

~5,345 game series entries across 194+ franchises from [Wikidata](https://www.wikidata.org/) (CC0), with sequel/prequel chains and ordinals. The repository carries a generated `data/wikidata/series.json` snapshot so release builds do not depend on live Wikidata SPARQL availability. See [Game Series](game-series.md) for details.

### Community Metadata

Curated entries for ROMs not covered by upstream sources — for example **AmigaVision** (a single boot file that bundles ~3,000 Amiga games), aftermarket cartridges, and homebrew compilations. Stored as JSON in `data/community/<system>.json` and baked into `catalog.sqlite` at build time. Anyone can submit a PR adding new entries; no Rust code changes are required. See [Contributing community metadata](../contributing/community-metadata.md) for the schema and submission flow.

## External Metadata (Optional)

### LaunchBox Refresh

Download the [LaunchBox](https://gamesdb.launchbox-app.com/) XML file (~460 MB) from the metadata page. One-button "Refresh metadata" handles download → parse → match → enrich:

- Real-time progress updates (downloading, parsing, enriching) via the activity SSE stream
- The XML is parsed once into the host-global `external_metadata.db` (`/var/lib/replay-control/external_metadata.db`); per-storage caches no longer hold a copy
- Boot-time freshness is content-derived (CRC32 of the XML vs. the last-parsed stamp) — newly added ROMs after a refresh pick up metadata automatically on the next enrichment
- Per-system coverage, romset composition, release-date and publisher coverage, media suggestions, and downloaded artwork totals are stored in `library.db` and refreshed during scan/rebuild and thumbnail update maintenance, so the metadata page can show library stats without walking the media folders while the page loads
- On a fresh install, local library discovery can start without waiting for optional network metadata. If metadata cannot be downloaded, the scan still completes and enrichment catches up when sources are available later.

Data imported: description, rating, rating count, publisher, developer, genre, max players, release date, cooperative flag, and LaunchBox video links.

Where the built-in database already has a value (e.g., genre), it takes priority. LaunchBox data only fills gaps. Description, publisher, and resource suggestions are denormalized into per-storage `library.db` tables so the game-detail page reads them from a single pool.

### Bundled Resource Links

Release builds also bundle manual links from [MiSTer Manual Downloader](https://github.com/antiKk/MiSTer_ManualDownloader) and the [Retrokit manuals Archive.org collection](https://archive.org/download/retrokit-manuals), plus Shmups Wiki strategy guide and video-index links, into `catalog.sqlite`. Only URL indexes are bundled; the manual PDF/text file itself is downloaded and validated only when the user saves it from a game detail page.

During scan/enrichment, matching resources are copied into the per-storage library cache. Game detail pages can then show manual suggestions and guide links offline without fetching source indexes at request time; saving a manual downloads and validates the file so it remains available later if the appliance is offline. The metadata page shows bundled manual-link and guide-link counts in the built-in data section.

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

## Library Coverage

The metadata page includes a per-system breakdown. Expand any system to see how complete its metadata is — genre, developer, publisher, release date, rating, description, box art, manuals, and videos — alongside romset composition (unique, clones, hacks, …), region and genre distribution, and downloaded media counts.

Two identity-related rows appear where relevant:

- **Verified** — how many games are identified by an exact content match (cartridge CRC or RetroAchievements disc/cartridge hash) rather than by filename.
- **RetroAchievements** — how many games have a known achievement set, shown for every system RetroAchievements supports.

Systems RetroAchievements doesn't cover at all (Amiga, C64, DOS, …) show a "not supported" note instead of a bar. Systems it does cover but whose RePlay emulator can't award achievements — for example PlayStation, PC Engine CD, and arcade MAME — keep the coverage bar, since the matches are real, but add a note that they aren't earnable on RePlay today.

## Export Metadata (CSV)

The **Advanced** section of the metadata page offers an **Export metadata (CSV)** download — a per-ROM metadata report aimed at people who maintain ROM packs or upstream metadata sources. Pick a system from the selector (or leave it on **All systems**, the default) and download.

Each row is one ROM. Most columns are filled in where data exists and left blank where it's missing, so the blanks are the gap report; a `missing_fields` column summarises which key fields are absent for quick sorting. Alongside identity columns (system, filename, title, region, content hash, verified name, and tags such as hack/translation/clone), every metadata field is split into two columns — one for the built-in catalog and one for the imported LaunchBox data. That split shows not just *that* a field is missing but *which* source is missing it, so the gap can be fixed in the right place.

Box-art, screenshot, and title-image columns report what is actually present in this device's media folders, and the RetroAchievements columns show the matched game id and its achievement count. The export reflects the live library on the device, including hacks and translations (flagged, not hidden).

For source-coverage analysis, each row also carries a `classification` (original, revision, hack, translation, unlicensed, homebrew, prerelease, pirate, region_variant — finer than the hack/translation flags), the catalog provenance (`source_catalog`: no-intro / community / wikidata), and grouping keys (`genre_group`, `series_key`). To compare original releases only, drop the rows whose `classification` is hack, translation, unlicensed, homebrew, or pirate.

## Cache Management

The metadata page provides tools to manage stored data:

- **Clear metadata** -- wipes provider metadata/resources and resets the XML hash stamp so the next refresh re-parses from disk
- **Clear images** -- removes downloaded box art and screenshots from disk + clears `box_art_url` from `game_library`
- **Cleanup orphaned images** -- removes downloaded images no longer associated with any game in the library, with a safety threshold per system to prevent accidental mass deletion
