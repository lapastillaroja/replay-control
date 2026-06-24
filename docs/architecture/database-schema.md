# Database Schema

Four SQLite databases:

- **catalog.sqlite** — read-only, bundled with the binary; built from upstream DATs/XMLs at compile time (No-Intro, MAME, FBNeo, Flycast, Wikidata, etc.).
- **library.db** — rebuildable cache at `/var/lib/replay-control/storages/<storage-id>/library.db` on the host SD. Centralised + keyed by a stable per-storage id so it stays on ext4/WAL and survives storage swaps. See [Design Decision #15](design-decisions.md#15-library-db-centralised-on-the-host-sd-keyed-by-storage-id).
- **external_metadata.db** — host-global at `/var/lib/replay-control/external_metadata.db`. Holds source-derived data that's identical across storages (provider metadata/resources, libretro thumbnail manifests, source-version stamps). Read **only by enrichment** — never at request time.
- **user_data.db** — persistent user customizations at `<storage>/.replay-control/user_data.db`. Stays on the ROM storage so it travels with the ROMs; never auto-deleted.

Schema defined in `tools/build-catalog/src/main.rs` (catalog), `replay-control-core-server/src/library/db/mod.rs` (library), `replay-control-core-server/src/external_metadata.rs` (external metadata), and `replay-control-core-server/src/user_data/db.rs` (user data).

## catalog.sqlite

Read-only, mounted via the catalog pool (`replay-control-core-server/src/catalog_pool.rs`). Lives next to the binary on disk; auto-update swaps it atomically alongside the binary on each release.

### arcade_game

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
| `arcade_stv` | MAME → MAME 2003+ → FBNeo |
| (any other) | empty — uses deterministic fallback |

After the priority list is exhausted, the merge walks any remaining sources (`ArcadeSource::ALL`) so that a ROM present *only* in (e.g.) the Naomi catalog still resolves on `arcade_mame`.

For each field the first source with a non-default value wins. Booleans (`is_clone`, `is_bios`) take the value from the first source that has the row at all, since `false` is a valid value rather than "missing".

### canonical_game

One row per canonical console game identity. ROM filename variants in `rom_entry` point at this table.

| Column | Type | Purpose |
|--------|------|---------|
| id | INTEGER | Surrogate primary key |
| system | TEXT | RePlay system folder, e.g. `"nintendo_snes"` |
| display_name | TEXT | Human-readable game name |
| year | INTEGER | Release year when known, `0` when unset |
| genre | TEXT | Source genre text |
| developer | TEXT | Developer name |
| publisher | TEXT | Publisher name |
| players | INTEGER | Max player count, `0` when unset |
| coop | INTEGER | Co-op support flag, nullable when unknown |
| rating | TEXT | Source rating text |
| normalized_genre | TEXT | Canonical genre group |
| description | TEXT | Catalog-backed long-form description, currently from community metadata |
| source | TEXT | Source tag for the canonical row, e.g. `"no-intro"` or `"community"` |

**PRIMARY KEY**: `id`

**Index**: `idx_cg_system ON canonical_game(system)` — supports system-scoped catalog scans and stats.

### rom_entry

One row per known No-Intro/libretro ROM filename stem. Maps concrete filenames and CRC32 values to a canonical game.

| Column | Type | Purpose |
|--------|------|---------|
| id | INTEGER | Surrogate primary key |
| system | TEXT | RePlay system folder |
| filename_stem | TEXT | ROM filename without extension |
| region | TEXT | Parsed region tag |
| crc32 | INTEGER | No-Intro CRC32, `0` when unavailable |
| canonical_game_id | INTEGER | FK to `canonical_game.id` |
| normalized_title | TEXT | Normalized title for fuzzy lookup |

**PRIMARY KEY**: `id`

**Indexes**:

| Index | Columns | Covers |
|-------|---------|--------|
| `idx_re_stem` | `(system, filename_stem)` | Exact filename-stem lookup |
| `idx_re_crc` | `(system, crc32)` | Hash-based ROM identification |
| `idx_re_norm` | `(system, normalized_title)` | Normalized-title fallback lookup |
| `idx_re_cgid` | `(canonical_game_id)` | Joins from canonical games to their ROM entries |

### rom_alternate

Alternate names for canonical console games. Used to seed `library.db.game_alias` during enrichment.

| Column | Type | Purpose |
|--------|------|---------|
| canonical_game_id | INTEGER | FK to `canonical_game.id` |
| system | TEXT | RePlay system folder |
| alternate_name | TEXT | Alternate title/name |

**PRIMARY KEY**: none. Rows are source-derived aliases; duplicates are tolerated by downstream `INSERT OR IGNORE` / de-duplication paths.

**Index**: `idx_ra_game ON rom_alternate(canonical_game_id, system)` — covers alternate lookup for matched canonical games.

**Sample usage**: catalog matching loads aliases for a matched `canonical_game_id` and inserts them into `library.db.game_alias` for search/detail enrichment.

### series_entry

Wikidata-derived series/franchise relationships.

| Column | Type | Purpose |
|--------|------|---------|
| id | INTEGER | Surrogate primary key |
| game_title | TEXT | Source game title |
| series_name | TEXT | Series/franchise name |
| system | TEXT | RePlay system folder |
| series_order | INTEGER | Position in series, nullable |
| follows | TEXT | Previous game title, if known |
| followed_by | TEXT | Next game title, if known |
| normalized_title | TEXT | Normalized title for matching |

**PRIMARY KEY**: `id`

**Index**: `idx_se_system ON series_entry(system, normalized_title)` — supports per-system series lookup for a matched game.

### arcade_release_date

Per-source arcade release year attribution. Seeded into `library.db.game_release_date` for arcade systems.

| Column | Type | Purpose |
|--------|------|---------|
| rom_name | TEXT | Arcade ROM short name |
| year | TEXT | Release year |
| source | TEXT | Source tag, default `"mame"` |

**PRIMARY KEY**: none.

**Sample usage**: `arcade_db::arcade_release_dates()` reads all rows ordered by `rom_name`; the resolver merges them with matched arcade ROMs during scan/enrichment.

### console_release_date

Per-region console release dates, sourced from TGDB during catalog build.

| Column | Type | Purpose |
|--------|------|---------|
| system | TEXT | RePlay system folder (PK part 1) |
| base_title | TEXT | Canonical base title (PK part 2) |
| region | TEXT | Region key, e.g. `"usa"`, `"japan"`, `"europe"` (PK part 3) |
| release_date | TEXT | ISO 8601 partial/full date |
| precision | TEXT | `"day"`, `"month"`, or `"year"` |
| source | TEXT | Source tag, default `"tgdb"` |

**PRIMARY KEY**: `(system, base_title, region)`

**Sample usage**: `game_db::console_release_dates()` streams rows into `library.db.game_release_date`; the per-storage resolver mirrors the preferred row into `game_library.release_date`.

### catalog_game_resource

Catalog-bundled resources linked to normalized game titles. Currently used for manual URL indexes from [MiSTer Manual Downloader](https://github.com/antiKk/MiSTer_ManualDownloader) and the [Retrokit manuals Archive.org collection](https://archive.org/download/retrokit-manuals), plus strategy guide and video-index links from Shmups Wiki. Only URLs are bundled in `catalog.sqlite`; PDFs are downloaded later only when a user saves a manual. These rows are copied into `library.db.library_game_resource` during scan/enrichment so game-detail reads stay on the library DB.

| Column | Type | Purpose |
|--------|------|---------|
| system | TEXT | RePlay system folder, or `"*"` for a global title resource that applies to every system (PK part 1) |
| normalized_title | TEXT | Normalized game title (PK part 2) |
| resource_type | TEXT | Resource kind, e.g. `"manual"`, `"strategy_guide"`, or `"video_index"` (PK part 3) |
| source | TEXT | Catalog source, e.g. `"mister_manuals"`, `"retrokit"`, or `"shmups_wiki"` (PK part 4) |
| resource_id | TEXT | Source-stable resource identifier or URL hash (PK part 5) |
| url | TEXT | Download URL |
| title | TEXT | Human-readable resource title |
| languages | TEXT | Comma-separated BCP-47-ish language tags, e.g. `"en"` or `"en,es,it"` |
| mime_type | TEXT | Expected content type such as `"application/pdf"` |

**PRIMARY KEY**: `(system, normalized_title, resource_type, source, resource_id)`

**Index**: `catalog_game_resource_idx_lookup ON catalog_game_resource(system, normalized_title, resource_type)` — supports enrichment lookups for all known titles in one system. Global (`"*"`) resources are intentionally small and may be loaded as a whole for the requested resource types.

### db_meta

Build metadata for the bundled catalog.

| Column | Type | Purpose |
|--------|------|---------|
| key | TEXT | Metadata key (PK) |
| value | TEXT | Metadata value |

**PRIMARY KEY**: `key`

Known keys include `mame_version`, `generated_at`, `is_stub`, and `catalog_enrichment_inputs_version` (a content hash over bundled resource rows and catalog-backed detail metadata; startup compares it with `library_meta.enrichment_inputs_version` to decide whether existing libraries need re-enrichment).

## library.db

Per-storage rebuildable cache. Lives at `/var/lib/replay-control/storages/<id>/library.db` on the host SD.

Schema is built by `init_tables()` (creates the current v6 shape on a fresh DB) and patched by `run_migrations()` (drops v1 tables on existing DBs from older binaries and applies additive migrations).

### Write-isolation rule

Writes to `library.db` are restricted to **scan / rebuild / enrichment / watcher / explicit-user-action** paths — never request-time SSR or HTTP read handlers. The `system_summaries` derived view and `LibraryService::load_roms_from_db` reader entry point intentionally do **not** fall through to a filesystem scan; population is the job of `BackgroundManager::populate_all_systems`, which iterates `visible_systems()` and calls `scan_and_cache_system` (strict reconcile) per system. `system_summaries` is not a cache: static system fields come from the core `SYSTEMS` catalog, while counts come from `game_library_meta`.

Rationale: an earlier read-time L3 fallback wrote the result of a silent walker straight back to `game_library_meta`. On a partially-mounted NFS the walk returned 41 zero-rom rows that no recovery path could undo (mtime was stamped, `rom_count > 0` guard skipped). Removing the read-time write closes the vector at its source.

The strict reconcile rule (`scan_and_cache_system`) closes the matching writer-side vector: a successful filesystem read replaces L2 for that system, but a failed read returns `Err` and preserves cached state. Rebuild and watcher paths additionally **do not pre-clear L2** — strict reconcile is only safe when there are cached rows to fall back on. The SQL-level zero-overwrite guard in `save_system_meta` is belt-and-suspenders.

The type-level split between `LibraryReadPool` and `LibraryWritePool` makes the rule a compile-time invariant for SSR/HTTP handlers (they only see the read pool). The regression suite at `replay-control-app/tests/cold_nfs_tests.rs` plus the per-system reconcile tests in `replay-control-app/src/api/library/mod.rs` lock in the runtime invariants.

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
| hash_size_bytes | INTEGER | ROM file size when CRC32 was computed (cache key) |
| hash_matched_name | TEXT | No-Intro canonical name if CRC32 matched |
| scan_token | INTEGER | Discovery reconcile token for the latest successful per-system scan attempt |
| identity_state | INTEGER | Row-level hash identity state (`pending`, `running`, matched/unmatched complete, failed, or not applicable) |
| release_date | TEXT | ISO 8601 partial/full date, mirror from `game_release_date` resolver |
| release_precision | TEXT | `"day"` / `"month"` / `"year"` |
| release_region_used | TEXT | Region the resolver picked for this row |
| cooperative | INTEGER | Co-op support flag |
| normalized_title | TEXT | Scan-time normalized title for enrichment matching |
| normalized_title_alt | TEXT | Alternate normalized title for enrichment matching |

**PRIMARY KEY**: `(system, rom_filename)`

**Indexes**:

| Index | Columns | Covers |
|-------|---------|--------|
| `idx_game_library_genre` | `(system, genre) WHERE genre IS NOT NULL AND genre != ''` | similar_by_genre, system_genre_groups |
| `idx_game_library_genre_group` | `(system, genre_group) WHERE genre_group != ''` | Genre group filtering |
| `idx_game_library_series_key` | `(series_key) WHERE series_key != ''` | series_siblings |
| `idx_game_library_developer_title` | `(developer, base_title) WHERE developer != ''` | find_developer_matches, games_by_developer, top_developers |
| `idx_game_library_base_title` | `(system, base_title) WHERE base_title != ''` | regional_variants, translations, hacks, specials, find_best_rom |
| `idx_game_library_identity_pending` | `(system, identity_state) WHERE identity_state IN (2, 3, 6)` | pending/running/failed identity recovery |
| `idx_game_library_cooperative` | `(system, cooperative) WHERE cooperative = 1` | coop_only filter, random_coop_games |

### game_library_meta

Per-system scan metadata. Used by `system_summaries` to derive UI counts and by aggregate info/coverage endpoints that only need per-system counts. Startup does not use `dir_mtime_secs` as its correctness boundary; it performs a full visible-system reconciliation so nested-folder changes made while the device was off are not missed.

| Column | Type | Purpose |
|--------|------|---------|
| system | TEXT | System folder name (PK) |
| dir_mtime_secs | INTEGER | Directory mtime at last scan |
| scanned_at | INTEGER | Unix timestamp of last scan |
| rom_count | INTEGER | Number of ROMs found |
| total_size_bytes | INTEGER | Total size of all ROMs |
| discovery_state | INTEGER | Per-system discovery state (`pending`, `running`, complete, failed) |
| enrichment_state | INTEGER | Per-system enrichment state (`pending`, `running`, complete, failed) |
| thumbnail_state | INTEGER | Per-system thumbnail planning/download state |

**PRIMARY KEY**: `system`

### game_library_system_stats

Rebuildable per-system romset facts for metadata/coverage pages. Most columns are a materialized view over `game_library`, `game_detail_metadata`, and `library_game_resource`; thumbnail media columns are refreshed from the downloaded media folders after scan/rebuild/startup verification and thumbnail update maintenance. Missing rows are backfilled when `library.db` opens so upgraded installs do not need a manual rescan before coverage appears. Request-time metadata pages read library summary, matched artwork coverage, downloaded artwork totals, and per-system coverage from this table and do not keep an additional app-local snapshot cache.

| Column | Type | Purpose |
|--------|------|---------|
| system | TEXT | System folder name (PK) |
| rom_count | INTEGER | Total discovered ROM rows |
| total_size_bytes | INTEGER | Total discovered ROM bytes |
| clone_count | INTEGER | ROMs classified as clones |
| hack_count | INTEGER | ROMs classified as hacks |
| translation_count | INTEGER | ROMs classified as translations |
| homebrew_count | INTEGER | Reserved for homebrew/aftermarket classification when stored separately |
| unlicensed_count | INTEGER | Reserved for unlicensed classification when stored separately |
| special_count | INTEGER | ROMs excluded from recommendation-style surfaces |
| region_counts_json | TEXT | Display-only region distribution |
| release_year_min | INTEGER | Earliest known release year |
| release_year_max | INTEGER | Latest known release year |
| release_date_known_count | INTEGER | ROMs with known release date/year |
| genre_counts_json | TEXT | Display-only genre distribution |
| genre_group_counts_json | TEXT | Display-only normalized genre distribution |
| developer_known_count | INTEGER | ROMs with developer data |
| publisher_known_count | INTEGER | ROMs with publisher data from detail metadata |
| player_count_distribution_json | TEXT | Display-only player-count distribution |
| rating_known_count | INTEGER | ROMs with rating data |
| description_count | INTEGER | ROMs with long-form description data |
| boxart_count | INTEGER | ROMs with box art URL coverage |
| snap_count | INTEGER | Reserved for screenshot coverage |
| title_screen_count | INTEGER | Reserved for title-screen coverage |
| thumbnail_total_size_bytes | INTEGER | Total size of valid downloaded thumbnail media for this system |
| thumbnail_file_count | INTEGER | Total valid downloaded thumbnail files for this system |
| thumbnail_boxart_file_count | INTEGER | Valid downloaded box art files for this system |
| thumbnail_snap_file_count | INTEGER | Valid downloaded screenshot files for this system |
| thumbnail_title_file_count | INTEGER | Valid downloaded title-screen files for this system |
| manual_count | INTEGER | ROMs with manual resource suggestions |
| video_count | INTEGER | ROMs with video resource suggestions |
| resource_count | INTEGER | Total rebuildable resource rows |
| coop_count | INTEGER | ROMs marked as cooperative |
| verified_count | INTEGER | ROMs matched by CRC identity |
| driver_status_json | TEXT | Arcade driver-status distribution |
| refresh_state | INTEGER | Stats state (`unknown`, `fresh`, `stale`, `refreshing`, failed) |
| updated_at | INTEGER | Unix timestamp of last refresh |

**PRIMARY KEY**: `system`

### game_detail_metadata

Long-form description + publisher per ROM, denormalized so the game-detail server fn stays on the library pool (no cross-pool acquire to `external_metadata.db`). One row per matched ROM; rebuilt at every enrichment pass.

| Column | Type | Purpose |
|--------|------|---------|
| system | TEXT | PK part 1 |
| rom_filename | TEXT | PK part 2 |
| description | TEXT | Long-form description (nullable) |
| publisher | TEXT | Publisher name (nullable) |

**PRIMARY KEY**: `(system, rom_filename)`

**Foreign key**: `(system, rom_filename) REFERENCES game_library(system, rom_filename) ON DELETE CASCADE`

### library_game_resource

Per-ROM resource suggestions copied from provider and catalog sources during enrichment. Game-detail request paths read this table only; they do not query `external_metadata.db` or `catalog.sqlite`.

| Column | Type | Purpose |
|--------|------|---------|
| system | TEXT | System folder name (PK part 1, FK to `game_library`) |
| rom_filename | TEXT | ROM filename (PK part 2, FK to `game_library`) |
| source | TEXT | Source tag, e.g. `"launchbox"`, `"mister_manuals"`, `"retrokit"` (PK part 3) |
| resource_type | TEXT | Resource kind, e.g. `"manual"` or `"video"` (PK part 4) |
| resource_id | TEXT | Source-stable resource ID (PK part 5) |
| url | TEXT | External URL |
| title | TEXT | Display title |
| languages | TEXT | Comma-separated language tags |
| platform | TEXT | Platform hint for video resources, e.g. `"youtube"` |
| mime_type | TEXT | Expected content type for downloadable resources |

**PRIMARY KEY**: `(system, rom_filename, source, resource_type, resource_id)`

**Foreign key**: `(system, rom_filename) REFERENCES game_library(system, rom_filename) ON DELETE CASCADE`

**Index**: `library_game_resource_idx_rom_type ON library_game_resource(system, rom_filename, resource_type)` — supports game-detail manual/video suggestions.

### game_detail_metadata_stage

Temporary staging table for chunked enrichment. Detail rows are written here before the live `game_detail_metadata` rows are replaced, so cancelled or failed enrichment keeps the previous live detail data.

| Column | Type | Purpose |
|--------|------|---------|
| system | TEXT | System folder name (PK part 1) |
| stage_token | INTEGER | Per-run staging token (PK part 2) |
| rom_filename | TEXT | ROM filename (PK part 3) |
| description | TEXT | Long-form description (nullable) |
| publisher | TEXT | Publisher name (nullable) |

**PRIMARY KEY**: `(system, stage_token, rom_filename)`

### library_game_resource_stage

Temporary staging table for chunked enrichment. Resource rows are written here before the live `library_game_resource` rows are replaced, matching the detail staging lifecycle.

| Column | Type | Purpose |
|--------|------|---------|
| system | TEXT | System folder name (PK part 1) |
| stage_token | INTEGER | Per-run staging token (PK part 2) |
| rom_filename | TEXT | ROM filename (PK part 3) |
| source | TEXT | Source tag, e.g. `"launchbox"`, `"mister_manuals"`, `"retrokit"` (PK part 4) |
| resource_type | TEXT | Resource kind, e.g. `"manual"` or `"video"` (PK part 5) |
| resource_id | TEXT | Source-stable resource ID (PK part 6) |
| url | TEXT | External URL |
| title | TEXT | Display title |
| languages | TEXT | Comma-separated language tags |
| platform | TEXT | Platform hint for video resources, e.g. `"youtube"` |
| mime_type | TEXT | Expected content type for downloadable resources |

**PRIMARY KEY**: `(system, stage_token, rom_filename, source, resource_type, resource_id)`

### library_thumbnail_job

Durable per-storage artwork download queue. Rebuild/rescan queues missing artwork here; downloads run after the library is usable.

| Column | Type | Purpose |
|--------|------|---------|
| system | TEXT | System folder name (PK part 1, FK to `game_library`) |
| rom_filename | TEXT | ROM filename (PK part 2, FK to `game_library`) |
| kind | TEXT | Libretro image kind (`Named_Boxarts`, `Named_Titles`, `Named_Snaps`) |
| filename | TEXT | Manifest filename stem |
| repo_url_name | TEXT | URL-safe libretro-thumbnails repository name |
| branch | TEXT | Repository branch |
| is_symlink | INTEGER | Whether the manifest entry is a symlink |
| state | INTEGER | Queue state (`queued`, `running`, `failed`) |
| attempts | INTEGER | Number of failed runtime attempts recorded for the job |
| priority | INTEGER | Download priority: box art first, then titles, then screenshots |
| updated_at | INTEGER | Unix timestamp of last queue update |

**PRIMARY KEY**: `(system, rom_filename, kind, filename)`

**Foreign key**: `(system, rom_filename) REFERENCES game_library(system, rom_filename) ON DELETE CASCADE`

**Index**: `idx_library_thumbnail_job_system_priority_status ON library_thumbnail_job(system, priority, state)` — supports priority queue fetches.

Failed rows are retained, but the pending-job loader skips rows once their
`attempts` value reaches the runtime cap. A later manifest change for the same
primary key resets the attempt count and makes the job eligible again.

### library_build_sequence

Small rebuildable counter table used by library build phases.

| Column | Type | Purpose |
|--------|------|---------|
| name | TEXT | Counter name; currently `scan_token` |
| next_value | INTEGER | Next monotonic value to allocate |

**PRIMARY KEY**: `name`

### game_release_date

Multi-region, full-precision release dates. Seeded from the bundled catalog (`console_release_date` / `arcade_release_date`) at scan time; resolver picks the user's preferred region and mirrors into `game_library.release_date` / `release_precision` / `release_region_used`.

| Column | Type | Purpose |
|--------|------|---------|
| system | TEXT | PK part 1 |
| base_title | TEXT | PK part 2 |
| region | TEXT | PK part 3 (`"usa"`, `"japan"`, `"europe"`, `"world"`, …) |
| release_date | TEXT | ISO 8601 partial/full |
| precision | TEXT | `"day"` / `"month"` / `"year"` |
| source | TEXT | Data origin tag |

**PRIMARY KEY**: `(system, base_title, region)`

**Indexes**:

| Index | Columns | Covers |
|-------|---------|--------|
| `idx_release_date_lookup` | `(system, base_title)` | Resolver lookups |
| `idx_release_date_chrono` | `(release_date)` | Chronological scans |

### game_alias

Alternative names for games. Populated by enrichment from `external_metadata.db.provider_alternate` UNION the catalog `rom_alternate`.

| Column | Type | Purpose |
|--------|------|---------|
| system | TEXT | PK part 1 |
| base_title | TEXT | PK part 2 |
| alias_name | TEXT | Alternative name (PK part 3) |
| alias_region | TEXT | Region for this alias |
| source | TEXT | Data source tag |

**PRIMARY KEY**: `(system, base_title, alias_name)`

**Indexes**:

| Index | Columns | Covers |
|-------|---------|--------|
| `idx_game_alias_name` | `(alias_name COLLATE NOCASE)` | search_aliases (LIKE queries) |
| `idx_game_alias_system_alias` | `(system, alias_name)` | alias_variants, alias_base_titles |

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

**PRIMARY KEY**: `(system, base_title, series_name)`

**Indexes**:

| Index | Columns | Covers |
|-------|---------|--------|
| `idx_game_series_name` | `(series_name COLLATE NOCASE)` | Series name lookups |
| `idx_game_series_system` | `(system, series_name)` | System-scoped series queries |
| `idx_game_series_order` | `(series_name, series_order) WHERE series_order IS NOT NULL` | Neighbor lookups, max order queries |

### library_meta

Per-storage key/value metadata that does not deserve a dedicated table.

| Column | Type | Purpose |
|--------|------|---------|
| key | TEXT | Metadata key (PK) |
| value | TEXT | Metadata value |

**PRIMARY KEY**: `key`

Known keys include:

| Key | Purpose |
|-----|---------|
| `title_norm_version` | `replay_control_core::title_utils::TITLE_NORM_VERSION` used when `game_library.normalized_title` / `normalized_title_alt` were last reconciled |
| `enrichment_inputs_version` | Bundled catalog enrichment-input hash that was last flushed into `library_game_resource` and `game_detail_metadata` |
| `discovery_fingerprint:<system>` | Last clean recursive startup-scan fingerprint for one system; startup uses it to skip unchanged systems after a full file walk |

### schema_version

Records the applied schema version. Used by the downgrade guard: `LibraryDb::open` refuses to open a DB whose stamped version is greater than the binary's `SCHEMA_VERSION`, since silently treating new-shape rows as old-shape would corrupt them on subsequent writes.

| Column | Type | Purpose |
|--------|------|---------|
| version | INTEGER | Applied version (PK) |
| applied_at | INTEGER | Unix timestamp |

**PRIMARY KEY**: `version`

## external_metadata.db

Host-global. Lives at `/var/lib/replay-control/external_metadata.db`. Holds source-derived metadata that doesn't depend on which storage is mounted (provider metadata/resources + libretro thumbnail manifests + source-version stamps). Schema in `replay-control-core-server/src/external_metadata.rs`.

Read mostly by enrichment, metadata maintenance, thumbnail planning, and box-art variant lookups. Normal game-detail/list request paths read `library.db`. Exposed on `AppState::external_metadata_pool` with a 2-reader / 1-writer pool.

### provider_game

Per-system provider entries, keyed by normalized title and provider (not ROM filename — the same DB serves every storage, so the row exists once regardless of how many storages have a matching ROM). LaunchBox currently writes provider `"launchbox"`.

| Column | Type | Purpose |
|--------|------|---------|
| provider | TEXT | Provider tag, currently `"launchbox"` (PK part 3) |
| system | TEXT | RePlay system folder (PK part 1) |
| normalized_title | TEXT | Normalized title (PK part 2) |
| description | TEXT | Long-form description |
| genre | TEXT | Provider genre |
| developer | TEXT | Provider developer |
| publisher | TEXT | Provider publisher |
| release_date | TEXT | ISO 8601 partial/full |
| release_precision | TEXT | `"day"` / `"month"` / `"year"` |
| rating | REAL | Community rating |
| rating_count | INTEGER | Number of ratings |
| cooperative | INTEGER | Co-op flag |
| players | INTEGER | Max players |

**PRIMARY KEY**: `(system, normalized_title, provider)`

### provider_alternate

Per-system alternate names from provider data. LaunchBox populates this from `<GameAlternateName>` entries.

| Column | Type | Purpose |
|--------|------|---------|
| provider | TEXT | Provider tag (PK part 4) |
| system | TEXT | RePlay system folder (PK part 1) |
| normalized_title | TEXT | Normalized primary title (PK part 2, matches `provider_game.normalized_title`) |
| alternate_name | TEXT | PK part 3 |
| normalized_alternate | TEXT | Normalized alternate title used by enrichment matching |

**PRIMARY KEY**: `(system, normalized_title, alternate_name, provider)`

### provider_resource

Provider-supplied links attached to a normalized game title. LaunchBox currently writes video URLs as `"video"` resources.

| Column | Type | Purpose |
|--------|------|---------|
| provider | TEXT | Provider tag (PK part 4) |
| system | TEXT | RePlay system folder (PK part 1) |
| normalized_title | TEXT | Normalized game title (PK part 2) |
| resource_type | TEXT | Resource kind, e.g. `"video"` (PK part 3) |
| resource_id | TEXT | Source-stable ID or URL hash (PK part 5) |
| url | TEXT | External URL |
| title | TEXT | Display title |
| languages | TEXT | Comma-separated language tags |
| platform | TEXT | Platform hint such as `"youtube"` |
| mime_type | TEXT | Expected content type for downloadable resources |

**PRIMARY KEY**: `(system, normalized_title, resource_type, provider, resource_id)`

### thumbnail_manifest

Index of available thumbnails from libretro-thumbnails repos. Populated by the thumbnail update pipeline (GitHub API) or rebuilt from disk by `phase_auto_rebuild_thumbnail_index`.

| Column | Type | Purpose |
|--------|------|---------|
| repo_name | TEXT | Source repo identifier (PK part 1) |
| kind | TEXT | Image kind: `"Named_Boxarts"`, `"Named_Snaps"`, etc. (PK part 2) |
| filename | TEXT | Image filename stem (PK part 3) |
| symlink_target | TEXT | Symlink target if the repo entry was a symlink |

**PRIMARY KEY**: `(repo_name, kind, filename)`

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

**PRIMARY KEY**: `source_name`

**Index**: `idx_data_source_type ON data_source(source_type)` — supports source-family stats and cleanup.

### external_meta

Key-value blob for DB-level metadata.

| Column | Type | Purpose |
|--------|------|---------|
| key | TEXT | Metadata key (PK) |
| value | TEXT | Metadata value |

**PRIMARY KEY**: `key`

Known keys:

| Key | Purpose |
|-----|---------|
| `launchbox_xml_crc32` | CRC32 of the last-parsed LaunchBox XML — content-derived freshness check at boot |
| `launchbox_upstream_etag` | ETag from the last successful upstream LaunchBox `Metadata.zip` download |
| `thumbnail_manifest_fetched_at` | Unix timestamp for the last successful libretro manifest fetch; short TTL for repeated update clicks |
| `title_norm_version` | Title normalizer version used for `provider_alternate.normalized_alternate` |

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

**PRIMARY KEY**: `(system, rom_filename)`

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

**PRIMARY KEY**: `(system, rom_filename, video_id)`

**Index**: `idx_game_videos_base_title ON (system, base_title)` — enables sharing videos across ROMs of the same game.

### game_manual_resource

User-saved manual resources for a game. Downloaded manuals are stored under `<storage>/.replay-control/manuals/` and tracked here; uploaded manuals use the same table. Legacy manuals found in `<storage>/manuals` remain read-only and are not inserted.

| Column | Type | Purpose |
|--------|------|---------|
| system | TEXT | System folder name (PK part 1) |
| base_title | TEXT | Base title for cross-ROM manual sharing |
| rom_filename | TEXT | ROM filename (PK part 2) |
| manual_id | TEXT | Stable saved-manual ID (PK part 3) |
| resource_key | TEXT | Original source/resource key for duplicate suppression |
| title | TEXT | Display title |
| origin | TEXT | `"downloaded"` or `"upload"` |
| provider | TEXT | Optional source/provider attribution for downloaded manuals, e.g. `"retrokit"` or `"mister_manuals"` |
| url | TEXT | Source URL for downloaded manuals |
| storage_path | TEXT | Relative path under `.replay-control/manuals` |
| original_filename | TEXT | Original upload/download filename when known |
| languages | TEXT | Comma-separated language tags |
| mime_type | TEXT | Validated content type (`application/pdf` or `text/plain`) |
| size_bytes | INTEGER | Saved file size |
| added_at | INTEGER | Unix timestamp |

**PRIMARY KEY**: `(system, rom_filename, manual_id)`

**Indexes**:

| Index | Columns | Covers |
|-------|---------|--------|
| `game_manual_resource_idx_base_title` | `(system, base_title)` | Sharing saved manuals across ROM variants of the same game |
| `game_manual_resource_idx_resource_key` | `(system, rom_filename, resource_key)` | Suppressing duplicate suggestions after a manual is saved |

### game_resource_link

User-saved external links for a game. These are non-downloadable resources such as strategy guides, wiki pages, and non-video links pinned from suggested resources or added by the user.

| Column | Type | Purpose |
|--------|------|---------|
| system | TEXT | System folder name (PK part 1) |
| base_title | TEXT | Base title for cross-ROM link sharing |
| rom_filename | TEXT | ROM filename that originally saved the link (PK part 2) |
| link_id | TEXT | Stable saved-link ID, currently a URL hash (PK part 3) |
| url | TEXT | Link URL |
| title | TEXT | Display title |
| source | TEXT | Optional source/provider attribution, e.g. `"shmups_wiki"` |
| resource_type | TEXT | Resource kind, e.g. `"strategy_guide"`, `"video_index"`, or `"link"` |
| added_at | INTEGER | Unix timestamp |

**PRIMARY KEY**: `(system, rom_filename, link_id)`

**Indexes**:

| Index | Columns | Covers |
|-------|---------|--------|
| `game_resource_link_idx_base_title` | `(system, base_title)` | Sharing saved links across ROM variants of the same game |
| `game_resource_link_idx_url` | `(system, rom_filename, url)` | Suppressing duplicate suggestions after a link is saved |

## Schema Migrations

`library.db` has a versioned migration handler in `LibraryDb::run_migrations`. The current `SCHEMA_VERSION` is **9**. Recent library-build pipeline shape changes also use open-time schema validation: if rebuildable `library.db` tables are missing required columns or indexes, the table is recreated and startup discovery repopulates it.

History:
- **v1**: original shape (`game_library`, `game_metadata`, `thumbnail_index`, `data_sources`, `game_release_date`, `game_alias`, `game_series`).
- **v2**: external_metadata.db redesign — drops `game_metadata`, `thumbnail_index`, and `data_sources`. Their content moves to `external_metadata.db` (LaunchBox text + libretro manifests + source version stamps).
- **v3**: adds `game_description` (description + publisher denormalized from external metadata so the game-detail page stays on the library pool).
- **v4**: adds `game_library.normalized_title` and `normalized_title_alt`, populated at scan time for faster enrichment matching and reconciled via `library_meta.title_norm_version`.
- **v5**: adds `game_library.hash_size_bytes`, allowing CRC32 cache validation by mtime + size without a full post-upgrade rehash storm.
- **v6**: renames `game_description` to `game_detail_metadata` and adds `library_game_resource` for manual/video suggestions copied from provider/catalog sources during enrichment.
- **v7**: adds `game_library.identity_state` for resumable hash matching.
- **v8**: adds per-system discovery, enrichment, and thumbnail state to `game_library_meta`.
- **v9**: adds durable thumbnail download jobs. Newer rebuildable columns/tables such as `scan_token`, thumbnail priority, detail/resource staging, and `game_library_system_stats`, plus rebuildable `library_meta` stamps such as per-system discovery fingerprints, are validated or regenerated by the library pipeline instead of a formal migration.

`run_migrations` reads the stored version, applies each `if current < N` step in order, then stamps `SCHEMA_VERSION`. Each step's destructive SQL (`DROP TABLE`) is logged at info above the SQL. For rebuildable library-cache tables, column drift is also treated as cache drift: the app recreates the table rather than carrying long migration code for data that can be rebuilt from ROM storage and metadata sources.

A **downgrade guard** at the top of `run_migrations` refuses to open a DB stamped with a version newer than the binary — silently treating new-shape rows as old-shape would corrupt them on subsequent writes.

`external_metadata.db` uses a different pattern: column-set drift triggers drop-and-recreate via `crate::sqlite::table_columns_diverge`. Its content is reproducible (LaunchBox XML, libretro repos, on-disk image scan) so a destructive rebuild costs only the next refresh cycle. `user_data.db` is treated as user-owned data and is not auto-dropped for schema drift.

## Corruption Handling

Two layers of detection run at open time:

1. **Magic-header pre-flight** — `sqlite::has_invalid_sqlite_header` reads the first 16 bytes of the file. If they're non-empty but don't match the SQLite magic string, the file has been clobbered by a torn write or partial overwrite. SQLite itself would refuse to open the file with a generic `SQLITE_NOTADB`, which previously crash-looped the systemd service before any recovery code could run. The pre-flight short-circuits to recovery instead.
2. **Table probe** — `probe_tables()` issues a row-scan against every known table. Catches logical/index corruption that the file-level magic check can't see.

Both layers feed the same recovery model. For `library.db` and `external_metadata.db`, corruption triggers automatic delete-and-recreate (both are rebuildable). For `user_data.db`, corruption is flagged via `DbPool::new_corrupt` but the DB is **not** destroyed — the user gets a banner with a one-click **Reset** action (user data is irreplaceable, so the choice belongs to the user). The banner is delivered via the `/sse/config` push channel, so it appears immediately on every connected tab without polling.

Both `SQLITE_CORRUPT (11)` and `SQLITE_NOTADB (26)` route through the same `check_for_corruption` path, so runtime queries that fail either way trigger the same recovery flow.
