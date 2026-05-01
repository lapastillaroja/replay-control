# Database Schema

Three SQLite databases:

- **catalog.sqlite** -- read-only, bundled with the binary; built from upstream DATs/XMLs at compile time (No-Intro, MAME, FBNeo, Flycast, Wikidata, etc.)
- **library.db** -- rebuildable cache at `/var/lib/replay-control/storages/<storage-id>/library.db` on the host SD. Centralised + keyed by a stable per-storage id so it stays on ext4/WAL and survives storage swaps. See [Design Decision #15](design-decisions.md#15-library-db-centralised-on-the-host-sd-keyed-by-storage-id).
- **user_data.db** -- persistent user customizations at `<storage>/.replay-control/user_data.db`. Stays on the ROM storage so it travels with the ROMs; never auto-deleted.

Schema defined in `tools/build-catalog/src/main.rs` (catalog), `replay-control-core-server/src/library/db/mod.rs` (library), and `replay-control-core-server/src/user_data/db.rs` (user data).

## catalog.sqlite

Read-only, mounted via the catalog pool (`replay-control-core-server/src/catalog_pool.rs`). Lives next to the binary on disk; auto-update swaps it atomically alongside the binary on each release.

### arcade_games

One row per `(rom_name, source)`. Each upstream curates ROMs in its own style — for example, MAME current ships `display_name="Galaga88"` for `rom_name="galaga88"` while FBNeo ships `"Galaga '88"`. Storing one row per source preserves each upstream's data; the runtime [merges fields by per-system priority](#per-system-arcade-merge) so each system shows its own upstream's curated values, with field-level fallback to other sources for fields the primary doesn't fill.

| Column | Type | Purpose |
|--------|------|---------|
| rom_name | TEXT | MAME-style ROM short name (PK part 1, e.g. `"sf2"`, `"galaga88"`) |
| source | TEXT | Upstream tag: `"fbneo"`, `"mame"`, `"mame_2k3p"`, `"naomi"` (PK part 2) |
| display_name | TEXT | Human-readable name *as the source wrote it* |
| year | TEXT | Release year (4-digit string or empty) |
| manufacturer | TEXT | Hardware manufacturer / publisher |
| players | INTEGER | Max player count (0 = unset) |
| rotation | TEXT | `"horizontal"`, `"vertical"`, or `"unknown"` |
| status | TEXT | Driver status: `"working"`, `"imperfect"`, `"preliminary"`, `"unknown"` |
| is_clone | INTEGER | Whether this ROM is a clone of another |
| is_bios | INTEGER | Whether this is a BIOS entry (filtered from playable lists) |
| parent | TEXT | Parent ROM short name if this is a clone |
| category | TEXT | Detail genre, e.g. `"Shooter / Gallery"` |
| normalized_genre | TEXT | Canonicalized genre group, e.g. `"Shooter"` |

**PRIMARY KEY**: `(rom_name, source)` — covers `WHERE rom_name = ?` lookups via leading-prefix scan, so no separate index is needed.

**Coverage** (~15.4K distinct ROMs, ~27.3K rows): MAME current 88%, FBNeo 53%, MAME 2003+ 34%, Naomi 2%. Production catalog is ~14.8 MB.

#### Per-system arcade merge

`replay-control-core-server/src/game/arcade_db.rs` exposes `lookup_arcade_game(system, rom_name)` and `lookup_arcade_games_batch(system, &[rom_names])`. Both return a single merged `ArcadeGameInfo` per ROM, built from the up-to-four source rows for that ROM.

The priority order per system lives in `replay_control_core::systems::arcade_source_priority`:

| System | Priority order |
|--------|----------------|
| `arcade_fbneo` | FBNeo → MAME → MAME 2003+ |
| `arcade_mame` | MAME → MAME 2003+ → FBNeo |
| `arcade_mame_2k3p` | MAME 2003+ → MAME → FBNeo |
| `arcade_dc` | Naomi → MAME → MAME 2003+ → FBNeo |
| (any other) | empty — uses deterministic fallback |

After the priority list is exhausted, the merge walks any remaining sources (`ArcadeSource::ALL`) so that a ROM present *only* in (e.g.) the Naomi catalog still resolves on `arcade_mame`.

For each field the first source with a non-default value wins. Booleans (`is_clone`, `is_bios`) take the value from the first source that has the row at all, since `false` is a valid value rather than "missing". Concrete consequence for `galaga88` on `arcade_fbneo`: `display_name="Galaga '88"` from FBNeo + `rotation=vertical` from MAME (FBNeo's row has `rotation=unknown`, so the merge falls through field-by-field).

### arcade_release_dates

Per-source year attribution for arcade ROMs. Used by the year-precision resolver in `library_db::resolve_release_date_for_library`.

| Column | Type | Purpose |
|--------|------|---------|
| rom_name | TEXT | MAME-style short name |
| year | TEXT | 4-digit year |
| source | TEXT | Same source tags as `arcade_games.source` |

One row per source per ROM (BIOS entries excluded). Aligned with the `arcade_games` row-per-source layout.

### Other catalog tables

The catalog also holds `canonical_games` (console games + metadata), `rom_entries` (No-Intro filename → canonical_games mapping), `rom_alternates` (alternate region/version names), `series_entries` (Wikidata series), `console_release_dates` (year attribution for console ROMs), and `db_meta` (build metadata). All written by `tools/build-catalog/src/main.rs` from raw upstream files.

## library.db

### game_library

Primary game catalog. One row per ROM file. Populated by the scan pipeline, enriched by the enrichment pipeline.

| Column | Type | Purpose |
|--------|------|---------|
| system | TEXT | System folder name (PK part 1) |
| rom_filename | TEXT | ROM filename (PK part 2) |
| rom_path | TEXT | Full path to ROM file |
| display_name | TEXT | Human-readable name (nullable) |
| base_title | TEXT | Tags stripped, used for grouping variants |
| series_key | TEXT | Algorithmic franchise key (base_title minus trailing numbers/roman numerals) |
| region | TEXT | Detected region string |
| developer | TEXT | From arcade_db at scan time or LaunchBox via enrichment |
| search_text | TEXT | Pre-computed search string |
| genre | TEXT | Detail genre (e.g., "Maze / Shooter") |
| genre_group | TEXT | Normalized genre for filtering (e.g., "Shooter") |
| rating | REAL | LaunchBox community rating |
| rating_count | INTEGER | Number of ratings |
| players | INTEGER | Max player count |
| is_clone | INTEGER | Whether this is a variant of another ROM |
| is_m3u | INTEGER | Whether this is an M3U playlist entry |
| is_translation | INTEGER | Translation patch detected |
| is_hack | INTEGER | ROM hack detected |
| is_special | INTEGER | Excluded from recommendations (FastROM, 60Hz, unlicensed, etc.) |
| box_art_url | TEXT | Resolved box art URL (e.g., `/media/snes/boxart/Name.png`) |
| driver_status | TEXT | Arcade driver status (good/imperfect/preliminary) |
| size_bytes | INTEGER | ROM file size |
| crc32 | INTEGER | CRC32 hash (NULL for CD/computer/arcade) |
| hash_mtime | INTEGER | File mtime when CRC32 was computed (cache key) |
| hash_matched_name | TEXT | No-Intro canonical name if CRC32 matched |
| release_year | INTEGER | From TOSEC tags or LaunchBox enrichment |

**PRIMARY KEY**: `(system, rom_filename)`

### game_library_meta

Per-system scan metadata. Used by the startup pipeline for mtime-based cache verification.

| Column | Type | Purpose |
|--------|------|---------|
| system | TEXT | System folder name (PK) |
| dir_mtime_secs | INTEGER | Directory mtime at last scan |
| scanned_at | INTEGER | Unix timestamp of last scan |
| rom_count | INTEGER | Number of ROMs found |
| total_size_bytes | INTEGER | Total size of all ROMs |

### game_metadata

External metadata from LaunchBox import. One row per ROM that has been matched.

| Column | Type | Purpose |
|--------|------|---------|
| system | TEXT | System folder name (PK part 1) |
| rom_filename | TEXT | ROM filename (PK part 2) |
| description | TEXT | Game description |
| genre | TEXT | Genre string |
| developer | TEXT | Developer name |
| publisher | TEXT | Publisher name |
| release_year | INTEGER | Release year |
| rating | REAL | Community rating |
| rating_count | INTEGER | Number of ratings |
| cooperative | INTEGER | Supports co-op (boolean) |
| players | INTEGER | Max players |
| box_art_path | TEXT | Image path from import |
| screenshot_path | TEXT | Screenshot path |
| title_path | TEXT | Title screen path |
| source | TEXT | Data source identifier |
| fetched_at | INTEGER | Unix timestamp of import |

### game_alias

Alternative names for games (from Wikidata or other sources). Used for search and variant resolution.

| Column | Type | Purpose |
|--------|------|---------|
| system | TEXT | PK part 1 |
| base_title | TEXT | PK part 2 |
| alias_name | TEXT | Alternative name (PK part 3) |
| alias_region | TEXT | Region for this alias |
| source | TEXT | Data source |

### game_series

Franchise/series relationships (from Wikidata). Links games that belong to the same series.

| Column | Type | Purpose |
|--------|------|---------|
| system | TEXT | PK part 1 |
| base_title | TEXT | PK part 2 |
| series_name | TEXT | Series identifier (PK part 3) |
| series_order | INTEGER | Position in series (nullable) |
| source | TEXT | Data source |
| follows_base_title | TEXT | Previous game in chain |
| followed_by_base_title | TEXT | Next game in chain |

### thumbnail_index

Index of available thumbnails from libretro-thumbnails repos. Populated by the thumbnail update pipeline (GitHub API) or rebuilt from disk.

| Column | Type | Purpose |
|--------|------|---------|
| repo_name | TEXT | Source repo identifier (PK part 1) |
| kind | TEXT | Image kind: "Named_Boxarts", "Named_Snaps", etc. (PK part 2) |
| filename | TEXT | Image filename (PK part 3) |
| symlink_target | TEXT | Target if symlinked in the repo |

**FK**: `repo_name` references `data_sources(source_name)`

### data_sources

Tracks imported data sources and their versions.

| Column | Type | Purpose |
|--------|------|---------|
| source_name | TEXT | Unique identifier (PK) |
| source_type | TEXT | Category (e.g., "libretro-thumbnails") |
| version_hash | TEXT | Content hash for change detection |
| imported_at | INTEGER | Unix timestamp |
| entry_count | INTEGER | Number of entries imported |
| branch | TEXT | Git branch name |

## Indexes

Each index is designed for specific query patterns (comments from the source):

| Index | Columns | Covers |
|-------|---------|--------|
| `idx_game_library_genre` | `(system, genre) WHERE genre IS NOT NULL AND genre != ''` | similar_by_genre, system_genre_groups |
| `idx_game_library_genre_group` | `(system, genre_group) WHERE genre_group != ''` | Genre group filtering |
| `idx_game_library_series_key` | `(series_key) WHERE series_key != ''` | series_siblings |
| `idx_game_library_developer_title` | `(developer, base_title) WHERE developer != ''` | find_developer_matches, games_by_developer, top_developers |
| `idx_game_library_base_title` | `(system, base_title) WHERE base_title != ''` | regional_variants, translations, hacks, specials, find_best_rom |
| `idx_data_sources_type` | `(source_type)` | get_data_source_stats, clear_thumbnail_index |
| `idx_game_alias_name` | `(alias_name COLLATE NOCASE)` | search_aliases (LIKE queries) |
| `idx_game_alias_system_alias` | `(system, alias_name)` | alias_variants, alias_base_titles |
| `idx_game_series_name` | `(series_name COLLATE NOCASE)` | Series name lookups |
| `idx_game_series_system` | `(system, series_name)` | System-scoped series queries |
| `idx_game_series_order` | `(series_name, series_order) WHERE series_order IS NOT NULL` | Neighbor lookups, max order queries |

The `thumbnail_index` PK `(repo_name, kind, filename)` covers repo_name-only prefix lookups, so no separate index is needed.

## user_data.db

Defined in `replay-control-core-server/src/user_data/db.rs`. Separate from library.db so user choices survive metadata clears and rebuilds.

### box_art_overrides

User-selected box art for specific ROMs.

| Column | Type | Purpose |
|--------|------|---------|
| system | TEXT | PK part 1 |
| rom_filename | TEXT | PK part 2 |
| override_path | TEXT | Path to selected image |
| set_at | INTEGER | Unix timestamp |

### game_videos

User-saved video links for games.

| Column | Type | Purpose |
|--------|------|---------|
| system | TEXT | PK part 1 |
| base_title | TEXT | For cross-ROM video sharing |
| rom_filename | TEXT | PK part 2 |
| video_id | TEXT | Unique video identifier (PK part 3) |
| url | TEXT | Canonical video URL |
| platform | TEXT | e.g., "youtube" |
| platform_video_id | TEXT | Platform-specific ID |
| title | TEXT | Human-readable title |
| added_at | INTEGER | Unix timestamp |
| from_recommendation | INTEGER | Whether pinned from search |
| tag | TEXT | Category: "trailer", "gameplay", "1cc", or NULL |

**Index**: `idx_game_videos_base_title ON (system, base_title)` -- enables sharing videos across ROMs of the same game.

## Schema Migrations

Handled inline in `init_tables()` with idempotent `ALTER TABLE ... ADD COLUMN` statements (errors silently ignored if column already exists). Example: `release_year` column added to `game_library` for TOSEC tag parsing.

## Corruption Handling

Two layers of detection run at open time:

1. **Magic-header pre-flight** -- `sqlite::has_invalid_sqlite_header` reads the first 16 bytes of the file. If they're non-empty but don't match the SQLite magic string, the file has been clobbered by a torn write or partial overwrite. SQLite itself would refuse to open the file with a generic `SQLITE_NOTADB`, which previously crash-looped the systemd service before any recovery code could run. The pre-flight short-circuits to recovery instead.
2. **Table probe** -- `probe_tables()` issues a row-scan against every known table. Catches logical/index corruption that the file-level magic check can't see.

Both layers feed the same recovery model. For `library.db`, corruption triggers automatic delete-and-recreate (it's a rebuildable cache). For `user_data.db`, corruption is flagged via `DbPool::new_corrupt` but the DB is **not** destroyed -- the user gets a banner with a one-click **Reset** action (user data is irreplaceable, so the choice belongs to the user). The banner is delivered via the `/sse/config` push channel, so it appears immediately on every connected tab without polling.

Both `SQLITE_CORRUPT (11)` and `SQLITE_NOTADB (26)` route through the same `check_for_corruption` path, so runtime queries that fail either way trigger the same recovery flow.
