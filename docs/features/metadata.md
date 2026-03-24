# Metadata

How game metadata is sourced, imported, and used.

## Design Principle: Offline-First

Replay Control is designed to work fully offline from the first install. The embedded databases (game_db, arcade_db) are compiled into the binary and provide genre, players, year, and display names for ~34K console ROMs and ~15K playable arcade games without any network access.

When connected to the internet, users can optionally enrich their library further: downloading LaunchBox metadata (descriptions, ratings) and libretro-thumbnails (box art, screenshots). These online sources fill gaps that the baked-in data doesn't cover but are never required.

## Embedded Databases (Compile-Time)

Two PHF maps are compiled into the binary via `build.rs`:

### arcade_db (~15,440 entries, 15,414 playable)

Covers all four arcade system folders: `arcade_mame`, `arcade_fbneo`, `arcade_mame_2k3p`, `arcade_dc`.

Fields: `display_name`, `year`, `manufacturer`, `players`, `rotation`, `driver_status`, `is_clone`, `is_bios`, `parent`, `category`, `normalized_genre`.

Build-time merge order (later overrides earlier, except Flycast which is always preserved):
1. Flycast CSV (hand-curated Naomi/Atomiswave, ~300 entries)
2. FBNeo DAT (~8K entries)
3. MAME 2003+ XML (~5K entries, adds players/rotation/status)
4. MAME 0.285 XML (~27K entries, most complete)
5. catver.ini v0.285 merged with category.ini v0.285 (~49,801 category entries)
6. nplayers.ini v0.278 (~427 player count fills for entries missing players from XML)

Non-playable entries are filtered at build time: 13,153 non-game machines (slot machines, gambling, computers, electromechanical, etc.) are excluded by category prefix matching. 26 BIOS entries are preserved with `is_bios` flag for future system info pages but filtered from game lists at display time.

Source data is downloaded via `./scripts/download-arcade-data.sh` into the gitignored `data/` directory.

### game_db (~34K ROM entries, ~15K canonical games)

Covers 20+ non-arcade systems. Two-level model: `CanonicalGame` (shared per game) + `GameEntry` (per ROM variant).

Fields: `canonical_name`, `year`, `genre`, `developer`, `players`, `region`, `crc32`, `normalized_genre`.

Lookup chain: exact filename stem (O(1) PHF) -> CRC32 hash-based fallback (for 9 cartridge systems with No-Intro DATs) -> normalized title fallback.

Sources: No-Intro DATs (ROM identification), TheGamesDB JSON (metadata enrichment), libretro-database DATs (genre/players).

### Shared Genre Taxonomy

Both databases map to ~18 normalized genres at build time: Action, Adventure, Beat'em Up, Board & Card, Driving, Educational, Fighting, Maze, Music, Pinball, Platform, Puzzle, Quiz, Role-Playing, Shooter, Simulation, Sports, Strategy, Other.

## External Metadata (Runtime)

### LaunchBox XML Import

The user downloads a ~460 MB XML file from LaunchBox containing ~108K game entries. The import pipeline:

1. **Build ROM index**: Scan all ROM directories, translate arcade codenames to display names via `arcade_db`, normalize filenames
2. **Stream-parse XML**: Process each `<Game>` element, map platform to system folder(s) via `platform_map()` (~45 mappings)
3. **Match and insert**: Normalized title matching against the ROM index, batch upsert to `game_metadata` table (batches of 500)

Matching uses `normalize_title()`: strip parenthetical/bracket tags, reorder articles ("Title, The" -> "The Title"), keep only lowercase alphanumeric.

Data stored: description, rating, rating count (from `<CommunityRatingCount>`), publisher, developer (from `<Developer>`), genre, max players (from `<MaxPlayers>`), release date (from `<ReleaseDate>`), cooperative flag (from `<Cooperative>`).

### Genre Fallback

When the baked-in database has no genre for a ROM, `enrich_system_cache()` fills it from LaunchBox's `game_metadata.genre`. This happens automatically after import. The baked-in genre always takes priority (LaunchBox only fills gaps).

### Player Count Fallback

Similarly, when a ROM has no player count from the baked-in database, enrichment falls back to `game_metadata.players` (parsed from LaunchBox's `<MaxPlayers>` field). This is critical for 11 systems that have 0% baked-in player coverage (amstrad_cpc, sharp_x68k, sega_sg, sega_32x, sega_st, sega_cd, sega_dc, sony_psx, ibm_pc, scummvm, commodore_ami).

### Orphaned Image Cleanup

The metadata page provides a "Cleanup Orphaned Images" button that removes downloaded images no longer associated with any game in the library. The cleanup:
- Scans `boxart/` directories for each system (snap has no URL column to cross-reference)
- Compares files on disk against active `game_library.box_art_url` entries
- Applies an 80% safety net per system (refuses to delete if >80% would be removed)
- Skips systems where no box art URLs have been enriched yet
- Protected by `metadata_operation_in_progress` guard to prevent races with other operations

### Wikidata Series Data

Embedded at build time from Wikidata SPARQL extracts. Provides game series/franchise relationships using P179 (part of the series), P155/P156 (follows/followed by) for sequel chains, and P1545 ordinals for series ordering. ~5,345 entries across 194+ series covering both console and arcade systems.

At scan time, entries are matched to library games by normalized title (with roman numeral normalization, e.g., "II" matches "2") and cross-system matching (a game's Wikidata entry may list a different platform than the ROM's system folder). See [Game Series](game-series.md) for details.

## Unified GameInfo API

Server functions return a single `GameInfo` struct regardless of data source. `resolve_game_info()` is the only place that branches on arcade vs. non-arcade:

- Always available (from embedded DB): display_name, year, genre, developer, players
- Available after import (from metadata_db): description, rating, rating_count, publisher, developer, box_art_url, screenshot_url, title_url
- Arcade-specific: rotation, driver_status, is_clone, parent_rom, arcade_category
- Console-specific: region

## ROM Tag Parsing

`rom_tags.rs` classifies ROMs by parsing filename tags. Supports No-Intro, GoodTools, and TOSEC naming conventions. TOSEC version strings and country codes are recognized for display name improvement and thumbnail matching.

| Tier | Examples | Effect |
|------|----------|--------|
| Original | No special tags | Included in recommendations |
| Translation | `(Traducido Es)`, `[T+Spa]` | Separate section, excluded from recommendations |
| Hack | `(Hack)`, `[h1]` | Separate section, excluded from recommendations |
| Special | `(FastRom)`, `(Unl)`, `(Homebrew)`, `(Beta)`, `(Pirate)` | Excluded from recommendations |
| Revision | `(Rev 1)`, `(Rev A)` | Shown as variant, included in recommendations |

## Key Source Files

| File | Role |
|------|------|
| `replay-control-core/src/game/arcade_db.rs` | Arcade PHF map + lookup |
| `replay-control-core/src/game/game_db.rs` | Console PHF maps + lookup chain |
| `replay-control-core/src/metadata/launchbox.rs` | LaunchBox XML import, ROM index, title normalization |
| `replay-control-core/src/metadata/metadata_db/` | SQLite schema, game_metadata, game_library, aliases/series tables |
| `replay-control-core/src/game/series_db.rs` | Embedded Wikidata series database |
| `replay-control-core/src/game/rom_tags.rs` | ROM filename classification and tag extraction |
| `replay-control-core/build.rs` | Build-time database generation |
| `replay-control-app/src/server_fns/mod.rs` | `resolve_game_info()` |

## Related Documentation

- `research/investigations/arcade-db-design.md` -- Full design doc for the arcade database build pipeline
- `docs/reference/game-metadata.md` -- Comprehensive metadata source evaluation and storage design
- `docs/reference/rom-matching.md` -- Matching pipeline details and coverage results
- `docs/reference/rom-identification.md` -- ROM filename parsing specification
