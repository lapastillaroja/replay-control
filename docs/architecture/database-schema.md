# Database Schema

Three SQLite databases:

- **catalog.sqlite** — read-only, bundled with the binary; built from upstream DATs/XMLs at compile time (No-Intro, MAME, FBNeo, Flycast, Wikidata, etc.).
- **library.db** — rebuildable cache at `/var/lib/replay-control/storages/<storage-id>/library.db` on the host SD. Centralised + keyed by a stable per-storage id so it stays on ext4/WAL and survives storage swaps. See [Design Decision #15](design-decisions.md#15-library-db-centralised-on-the-host-sd-keyed-by-storage-id).
- **external_metadata.db** — host-global at `/var/lib/replay-control/external_metadata.db`. Holds source-derived data that's identical across storages (LaunchBox text, libretro thumbnail manifests, source-version stamps). Read **only by enrichment** — never at request time.
- **user_data.db** — persistent user customizations at `<storage>/.replay-control/user_data.db`. Stays on the ROM storage so it travels with the ROMs; never auto-deleted.

Schema defined in `tools/build-catalog/src/main.rs` (catalog), `replay-control-core-server/src/library/db/mod.rs` (library), `replay-control-core-server/src/external_metadata.rs` (external metadata), and `replay-control-core-server/src/user_data/db.rs` (user data).

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

For each field the first source with a non-default value wins. Booleans (`is_clone`, `is_bios`) take the value from the first source that has the row at all, since `false` is a valid value rather than "missing".

### Other catalog tables

The catalog also holds `canonical_games` (console games + metadata), `rom_entries` (No-Intro filename → canonical_games mapping), `rom_alternates` (alternate region/version names), `series_entries` (Wikidata series), `console_release_dates` (year attribution for console ROMs), `arcade_release_dates` (per-source year attribution for arcade ROMs), and `db_meta` (build metadata). All written by `tools/build-catalog/src/main.rs` from raw upstream files.

## library.db

Per-storage rebuildable cache. Lives at `/var/lib/replay-control/storages/<id>/library.db` on the host SD.

Schema is built by `init_tables()` (creates v3 shape on a fresh DB) and patched by `run_migrations()` (drops v1 tables on existing DBs from older binaries).

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
| release_date | TEXT | ISO 8601 partial/full date, mirror from `game_release_date` resolver |
| release_precision | TEXT | `"day"` / `"month"` / `"year"` |
| release_region_used | TEXT | Region the resolver picked for this row |
| cooperative | INTEGER | Co-op support flag |

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

### game_description

Long-form description + publisher per ROM, denormalized so the game-detail server fn stays on the library pool (no cross-pool acquire to `external_metadata.db`). One row per matched ROM; rebuilt at every enrichment pass.

| Column | Type | Purpose |
|--------|------|---------|
| system | TEXT | PK part 1 |
| rom_filename | TEXT | PK part 2 |
| description | TEXT | Long-form description (nullable) |
| publisher | TEXT | Publisher name (nullable) |

### game_release_date

Multi-region, full-precision release dates. Seeded from the bundled catalog (`console_release_dates` / `arcade_release_dates`) at scan time; resolver picks the user's preferred region and mirrors into `game_library.release_date` / `release_precision` / `release_region_used`.

| Column | Type | Purpose |
|--------|------|---------|
| system | TEXT | PK part 1 |
| base_title | TEXT | PK part 2 |
| region | TEXT | PK part 3 (`"usa"`, `"japan"`, `"europe"`, `"world"`, …) |
| release_date | TEXT | ISO 8601 partial/full |
| precision | TEXT | `"day"` / `"month"` / `"year"` |
| source | TEXT | Data origin tag |

### game_alias

Alternative names for games. Populated by enrichment from `external_metadata.db.launchbox_alternate` UNION the catalog `rom_alternates`.

| Column | Type | Purpose |
|--------|------|---------|
| system | TEXT | PK part 1 |
| base_title | TEXT | PK part 2 |
| alias_name | TEXT | Alternative name (PK part 3) |
| alias_region | TEXT | Region for this alias |
| source | TEXT | Data source tag |

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

### schema_version

Records the applied schema version. Used by the downgrade guard: `LibraryDb::open` refuses to open a DB whose stamped version is greater than the binary's `SCHEMA_VERSION`, since silently treating new-shape rows as old-shape would corrupt them on subsequent writes.

| Column | Type | Purpose |
|--------|------|---------|
| version | INTEGER | Applied version (PK) |
| applied_at | INTEGER | Unix timestamp |

### Indexes

| Index | Columns | Covers |
|-------|---------|--------|
| `idx_game_library_genre` | `(system, genre) WHERE genre IS NOT NULL AND genre != ''` | similar_by_genre, system_genre_groups |
| `idx_game_library_genre_group` | `(system, genre_group) WHERE genre_group != ''` | Genre group filtering |
| `idx_game_library_series_key` | `(series_key) WHERE series_key != ''` | series_siblings |
| `idx_game_library_developer_title` | `(developer, base_title) WHERE developer != ''` | find_developer_matches, games_by_developer, top_developers |
| `idx_game_library_base_title` | `(system, base_title) WHERE base_title != ''` | regional_variants, translations, hacks, specials, find_best_rom |
| `idx_game_library_cooperative` | `(system, cooperative) WHERE cooperative = 1` | coop_only filter, random_coop_games |
| `idx_game_alias_name` | `(alias_name COLLATE NOCASE)` | search_aliases (LIKE queries) |
| `idx_game_alias_system_alias` | `(system, alias_name)` | alias_variants, alias_base_titles |
| `idx_game_series_name` | `(series_name COLLATE NOCASE)` | Series name lookups |
| `idx_game_series_system` | `(system, series_name)` | System-scoped series queries |
| `idx_game_series_order` | `(series_name, series_order) WHERE series_order IS NOT NULL` | Neighbor lookups, max order queries |
| `idx_release_date_lookup` | `(system, base_title)` on `game_release_date` | Resolver lookups |
| `idx_release_date_chrono` | `(release_date)` on `game_release_date` | Chronological scans |

## external_metadata.db

Host-global. Lives at `/var/lib/replay-control/external_metadata.db`. Holds source-derived metadata that doesn't depend on which storage is mounted (LaunchBox text + libretro thumbnail manifests + source-version stamps). Schema in `replay-control-core-server/src/external_metadata.rs`.

Read mostly by enrichment, metadata maintenance, thumbnail planning, and box-art variant lookups. Normal game-detail/list request paths read `library.db`. Exposed on `AppState::external_metadata_pool` with a 2-reader / 1-writer pool.

### launchbox_game

Per-system LaunchBox entries, keyed by normalized title (not ROM filename — the same DB serves every storage, so the row exists once regardless of how many storages have a matching ROM).

| Column | Type | Purpose |
|--------|------|---------|
| system | TEXT | PK part 1 |
| normalized_title | TEXT | PK part 2 (output of `replay_control_core::title_utils::normalize_title_for_metadata`) |
| description | TEXT | Long-form description |
| genre | TEXT | LaunchBox genre |
| developer | TEXT | LaunchBox developer |
| publisher | TEXT | LaunchBox publisher |
| release_date | TEXT | ISO 8601 partial/full |
| release_precision | TEXT | `"day"` / `"month"` / `"year"` |
| rating | REAL | Community rating |
| rating_count | INTEGER | Number of ratings |
| cooperative | INTEGER | Co-op flag |
| players | INTEGER | Max players |

### launchbox_alternate

Per-system alternate names from the LaunchBox `<GameAlternateName>` entries.

| Column | Type | Purpose |
|--------|------|---------|
| system | TEXT | PK part 1 |
| normalized_title | TEXT | PK part 2 (matches `launchbox_game.normalized_title`) |
| alternate_name | TEXT | PK part 3 |

### thumbnail_manifest

Index of available thumbnails from libretro-thumbnails repos. Populated by the thumbnail update pipeline (GitHub API) or rebuilt from disk by `phase_auto_rebuild_thumbnail_index`.

| Column | Type | Purpose |
|--------|------|---------|
| repo_name | TEXT | Source repo identifier (PK part 1) |
| kind | TEXT | Image kind: `"Named_Boxarts"`, `"Named_Snaps"`, etc. (PK part 2) |
| filename | TEXT | Image filename stem (PK part 3) |
| symlink_target | TEXT | Symlink target if the repo entry was a symlink |

### data_source

Tracks imported data sources and their versions (libretro repo commit shas, future external sources). One row per source.

| Column | Type | Purpose |
|--------|------|---------|
| source_name | TEXT | Unique identifier (PK) |
| source_type | TEXT | Category (e.g., `"libretro-thumbnails"`) |
| version_hash | TEXT | Content hash for change detection (e.g. git commit sha) |
| imported_at | INTEGER | Unix timestamp |
| entry_count | INTEGER | Number of entries imported |
| branch | TEXT | Git branch name (libretro repos use `master` or `main`) |

### external_meta

Key-value blob for DB-level metadata.

| Key | Purpose |
|-----|---------|
| `launchbox_xml_crc32` | CRC32 of the last-parsed LaunchBox XML — content-derived freshness check at boot |

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
| platform | TEXT | e.g., `"youtube"` |
| platform_video_id | TEXT | Platform-specific ID |
| title | TEXT | Human-readable title |
| added_at | INTEGER | Unix timestamp |
| from_recommendation | INTEGER | Whether pinned from search |
| tag | TEXT | Category: `"trailer"`, `"gameplay"`, `"1cc"`, or NULL |

**Index**: `idx_game_videos_base_title ON (system, base_title)` — enables sharing videos across ROMs of the same game.

## Schema Migrations

`library.db` has a versioned migration handler in `LibraryDb::run_migrations`. The current `SCHEMA_VERSION` is **3**.

History:
- **v1**: original shape (`game_library`, `game_metadata`, `thumbnail_index`, `data_sources`, `game_release_date`, `game_alias`, `game_series`).
- **v2**: external_metadata.db redesign — drops `game_metadata`, `thumbnail_index`, and `data_sources`. Their content moves to `external_metadata.db` (LaunchBox text + libretro manifests + source version stamps).
- **v3**: adds `game_description` (description + publisher denormalized from `external_metadata.launchbox_game` so the game-detail page stays on the library pool).

`run_migrations` reads the stored version, applies each `if current < N` step in order, then stamps `SCHEMA_VERSION`. Each step's destructive SQL (`DROP TABLE`) is logged at info above the SQL.

A **downgrade guard** at the top of `run_migrations` refuses to open a DB stamped with a version newer than the binary — silently treating new-shape rows as old-shape would corrupt them on subsequent writes.

`external_metadata.db` and `user_data.db` use a different pattern: column-set drift triggers drop-and-recreate via `crate::sqlite::table_columns_diverge`. Their content is reproducible (LaunchBox XML, libretro repos, on-disk image scan) so a destructive rebuild costs only the next refresh cycle.

## Corruption Handling

Two layers of detection run at open time:

1. **Magic-header pre-flight** — `sqlite::has_invalid_sqlite_header` reads the first 16 bytes of the file. If they're non-empty but don't match the SQLite magic string, the file has been clobbered by a torn write or partial overwrite. SQLite itself would refuse to open the file with a generic `SQLITE_NOTADB`, which previously crash-looped the systemd service before any recovery code could run. The pre-flight short-circuits to recovery instead.
2. **Table probe** — `probe_tables()` issues a row-scan against every known table. Catches logical/index corruption that the file-level magic check can't see.

Both layers feed the same recovery model. For `library.db` and `external_metadata.db`, corruption triggers automatic delete-and-recreate (both are rebuildable). For `user_data.db`, corruption is flagged via `DbPool::new_corrupt` but the DB is **not** destroyed — the user gets a banner with a one-click **Reset** action (user data is irreplaceable, so the choice belongs to the user). The banner is delivered via the `/sse/config` push channel, so it appears immediately on every connected tab without polling.

Both `SQLITE_CORRUPT (11)` and `SQLITE_NOTADB (26)` route through the same `check_for_corruption` path, so runtime queries that fail either way trigger the same recovery flow.
