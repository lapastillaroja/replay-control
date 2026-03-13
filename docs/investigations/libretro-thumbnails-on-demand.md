# On-Demand libretro-thumbnails: Manifest-Based Strategy

> **Status**: Implemented. The manifest-based approach (option C) was built in `replay-control-core/src/thumbnail_manifest.rs` with `data_sources` and `thumbnail_index` tables in `metadata_db.rs`. Bulk parallel downloads, symlink resolution, `ManifestFuzzyIndex`, and SSE progress are all operational.

> Investigation date: 2026-03-12
> Updated: 2026-03-12 (bulk download optimization research: section 15)
> Updated: 2026-03-12 (detailed fetch logic: sections 6-11)

## Problem

The current thumbnail import workflow requires a full `git clone --depth 1` of each system's libretro-thumbnails repo, then copies matching images to `.replay-control/media/`. This has significant costs on a Pi 4:

- **Download**: each repo is hundreds of MB to several GB (SNES is 3.2 GB, MAME is 12 GB)
- **Disk churn**: clones go to `tmp/`, images get copied to `media/`, then clones get deleted
- **Time**: cloning + matching a large system takes minutes
- **Wasted bandwidth**: we download every image in a repo but typically only need a fraction (the ones matching the user's ROMs)

Goal: fetch only the images the user actually needs, when they need them, while still supporting fuzzy matching.

---

## 1. libretro-thumbnails Repository Structure

### Organization

- **131 total repos** in the `libretro-thumbnails` GitHub organization
- One repo per system/console, named `Manufacturer_-_Console` (GitHub) / `Manufacturer - Console` (display)
- A meta-repo `libretro-thumbnails/libretro-thumbnails` contains all systems as git submodules
- **40 repos** are relevant to systems currently supported by RePlayOS

### Internal structure per repo

```
Named_Boxarts/
    Game Name (Region).png
    Game Name (Region) (Rev 1).png
    ...
Named_Snaps/
    Game Name (Region).png
    ...
Named_Titles/
    Game Name (Region).png
    ...
Named_Logos/
    Game Name (Region).png
    ...
.gitignore
```

RePlayOS only uses `Named_Boxarts` and `Named_Snaps`. Roughly 50% of files per repo are in the two categories we use (example: SNES has 3,768 boxarts + 3,742 snaps = 7,510 out of 14,857 total).

### File naming conventions

- Filenames use the game's **display name** (not ROM hash or codename)
- Special characters `&*/:\`<>?\\|"` are replaced with `_`
- Arcade games use the **display name** (not MAME codename): e.g., `Street Fighter II_ The World Warrior.png`
- Images are PNG, max 512px wide (per contributor guidelines)
- **Symlinks** are common: many region/revision variants point to the same image. These appear as small (< 100 byte) text files containing the relative path to the real image. On GitHub's tree API, they have `mode: "100644"` (not `120000` as might be expected) with a very small `size`.

### Branch naming

Most repos use `master` as default branch. Newer repos use `main`:
- `main`: Commodore - CD32, Commodore - CDTV, Sega - Naomi, Sega - Naomi 2, Philips - CD-i
- `master`: everything else

Any implementation must query the default branch via the API or try both.

### Scale (RePlayOS-relevant systems only)

| System | Files | Full Size |
|--------|------:|----------:|
| MAME | 35,075 | 11.7 GB |
| FBNeo - Arcade Games | 28,857 | 7.1 GB |
| Sony - PlayStation | 35,350 | 6.7 GB |
| Nintendo - NES | 48,167 | 3.7 GB |
| Nintendo - DS | 19,256 | 4.2 GB |
| Nintendo - SNES | 14,857 | 3.2 GB |
| Commodore - 64 | 58,758 | 1.6 GB |
| Sinclair - ZX Spectrum | 28,569 | 730 MB |
| DOS | 23,200 | 1.7 GB |
| Nintendo - GBA | 19,895 | 1.8 GB |
| Sega - Dreamcast | 3,984 | 1.7 GB |
| Sega - Saturn | 8,802 | 2.2 GB |
| *(27 more systems)* | ... | ... |
| **Total (40 repos)** | **416,243** | **57.2 GB** |

A typical RePlayOS user might have 50-500 ROMs per system, needing maybe 5-15% of available thumbnails.

---

## 2. Approaches Evaluated

### A. Blobless Partial Clone (`git clone --filter=blob:none`)

**How it works**: Clone only the git tree objects (directory listings) without any file content. Blobs (actual images) are fetched on demand when `git checkout` is used.

**Tested results**:

| Repo | Full Size | Blobless .git Size | File Count | Clone Time |
|------|----------:|-------------------:|-----------:|-----------:|
| Sega - Game Gear | 477 MB | 204 KB | 3,276 | 0.7s |
| Nintendo - SNES | 3.2 GB | 496 KB | 14,857 | 0.7s |
| Nintendo - NES | 3.7 GB | 976 KB | 48,167 | 0.9s |
| Sony - PlayStation | 6.7 GB | 904 KB | 35,350 | 0.8s |
| Commodore - 64 | 1.6 GB | 1.5 MB | 58,758 | 0.9s |
| MAME | 11.7 GB | 1.1 MB | 35,075 | 0.7s |

Key findings:
- **Blobless clone is 3,000x-16,000x smaller** than a full clone
- **Under 1 second** to clone, even for the largest repos
- `git ls-tree -r --name-only HEAD` gives the full file listing without fetching any blobs
- `git checkout HEAD -- "path/to/file.png"` fetches a single blob on demand (~0.5-1s per file)
- The .git directory grows as blobs are fetched (each blob is cached locally)
- Blobless clone with `--depth=1` combines both optimizations

**Pros**: Built-in git feature, works with any git hosting, gives you a real working tree for checked-out files.

**Cons**: Requires `git` on the device. Each on-demand fetch is a separate network round-trip (~0.5s each). The `.git/` directory grows unboundedly as blobs accumulate. Git's partial clone protocol may batch-fetch more than requested.

### B. GitHub Tree API + Raw URL Download

**How it works**: Use the GitHub REST API to get the file listing, then download images directly from `raw.githubusercontent.com`.

**File listing**: `GET /repos/libretro-thumbnails/{repo}/git/trees/{branch}?recursive=1`
- Returns all files with `path`, `sha`, `size`, `mode`, and `type`
- Even the largest repo (MAME, 35K files) returns without truncation
- Response is ~2-5 MB of JSON for large repos
- Single HTTP request, fast (~1-2s)

**Image download**: `https://raw.githubusercontent.com/libretro-thumbnails/{repo_url_name}/{branch}/Named_Boxarts/{url_encoded_filename}.png`
- Works for real files. Returns HTTP 200 with `image/png` content type.
- **Symlinks return the symlink text content** (the relative path), NOT the target image. This means we need to resolve symlinks ourselves by reading the text, finding the target filename, and fetching that instead.

**Identifying symlinks from the tree API**: Files with `size < 100` bytes that have a `.png` extension are symlinks. The tree API provides the `size` field, so we can flag them during manifest generation without downloading anything.

**Pros**: No git dependency. Precise control over what gets downloaded. Can parallelize downloads. Raw URLs are CDN-backed (fast globally).

**Cons**: Requires GitHub API access (rate limits: 5,000/hour authenticated, 60/hour unauthenticated). Symlink resolution adds complexity. URL encoding of filenames with special characters needs care.

### C. SQLite Manifest + On-Demand Fetch (Recommended Hybrid)

**How it works**: Combine approaches A/B into a two-tier system:

1. **Manifest generation** (user-triggered or periodic): Use the GitHub Tree API to build a local SQLite index of all available thumbnail filenames per system, stored in `metadata.db`.
2. **On-demand fetch**: When box art is needed for a ROM, look it up in the manifest (using the existing fuzzy matching logic), then download from `raw.githubusercontent.com`.
3. **Local cache**: Downloaded images go to `.replay-control/media/` as they do today. Once downloaded, they're served from disk forever.

This is the recommended approach. The schema design and UI integration are detailed in sections 3 and 4 below.

---

## 3. Database Schema Design

### Design Principles

The libretro thumbnail manifest import is structurally similar to the existing LaunchBox metadata import: download a data source from the internet, process it, and store results locally in `metadata.db`. The schema should reflect this parallel, and both data sources should be tracked with version/freshness metadata.

### Unified Table: `data_sources`

A single table tracks all imported data sources -- both LaunchBox metadata and individual libretro thumbnail repos. LaunchBox has one row; each libretro-thumbnails repo has its own row (e.g., `libretro:Nintendo_-_Super_Nintendo_Entertainment_System`). This eliminates the need for a separate `thumbnail_repo_status` table.

```sql
CREATE TABLE IF NOT EXISTS data_sources (
    source_name TEXT PRIMARY KEY,      -- e.g., "launchbox", "libretro:Nintendo_-_NES", "libretro:Sega_-_Game_Gear"
    source_type TEXT NOT NULL,         -- "launchbox" or "libretro-thumbnails"
    version_hash TEXT,                 -- SHA-256 for launchbox zip, commit SHA for libretro repos
    imported_at INTEGER NOT NULL,      -- Unix timestamp of last import
    entry_count INTEGER NOT NULL DEFAULT 0, -- Number of entries imported
    branch TEXT                        -- Git branch ("master"/"main"); NULL for non-git sources
);
```

**Usage by source type:**

| source_name | source_type | version_hash | branch |
|---|---|---|---|
| `launchbox` | `launchbox` | SHA-256 of downloaded zip | NULL |
| `libretro:Nintendo_-_Super_Nintendo_Entertainment_System` | `libretro-thumbnails` | Latest commit SHA | `master` |
| `libretro:Sega_-_Naomi` | `libretro-thumbnails` | Latest commit SHA | `main` |

**Querying aggregate stats for the UI** (the "Thumbnail Index: 208,121 images across 40 systems" line):

```sql
SELECT COUNT(*) AS repo_count, SUM(entry_count) AS total_entries
FROM data_sources
WHERE source_type = 'libretro-thumbnails';
```

**Querying per-repo freshness** (for incremental updates -- skip repos whose commit SHA hasn't changed):

```sql
SELECT source_name, version_hash FROM data_sources
WHERE source_type = 'libretro-thumbnails';
```

The `source_name` for libretro repos uses the `libretro:` prefix followed by the repo URL name. To extract the repo URL name for GitHub API calls and `thumbnail_index` lookups, strip the prefix: `source_name.strip_prefix("libretro:")`.

**Update flow**: Before re-fetching a repo's tree, check the latest commit SHA via `GET /repos/libretro-thumbnails/{repo}/commits?per_page=1`. If it matches `version_hash`, skip that repo entirely. This makes re-imports fast: only changed repos get re-fetched.

### New Table: `thumbnail_index`

Stores the file listing from libretro-thumbnails repos. This is the "manifest" -- the list of all available thumbnails that can be fetched on demand.

```sql
CREATE TABLE IF NOT EXISTS thumbnail_index (
    repo_name TEXT NOT NULL,       -- FK to data_sources, e.g., "libretro:Nintendo_-_Super_Nintendo_Entertainment_System"
    kind TEXT NOT NULL,            -- "Named_Boxarts" or "Named_Snaps"
    filename TEXT NOT NULL,        -- Filename without .png extension (the stem)
    symlink_target TEXT,           -- NULL if real file; target stem if symlink
    PRIMARY KEY (repo_name, kind, filename),
    FOREIGN KEY (repo_name) REFERENCES data_sources(source_name)
);
CREATE INDEX IF NOT EXISTS idx_thumbidx_repo ON thumbnail_index(repo_name);
```

**Normalization rationale (revised from earlier denormalized draft):**

The initial schema stored `system` (display name), `repo_name` (URL name), and `branch` on every row. For 208K entries across 40 systems, these per-row constants wasted significant space:

| Removed column | Avg bytes/row | Wasted across 208K rows |
|---|---:|---:|
| `system` (display name, redundant with `repo_name`) | ~40 | ~8.3 MB |
| `branch` (constant per repo) | ~6 | ~1.2 MB |
| **Total savings** | | **~9.5 MB** |

This reduces the estimated DB size from ~10-15 MB to ~4-7 MB -- a meaningful saving on a Pi SD card, and it halves the data that SQLite needs to read when loading a system's entries.

The normalized schema uses `repo_name` as the sole FK to `data_sources`, which already stores `branch`. The `system` column (libretro display name) is dropped entirely because:

1. **It's derivable from `repo_name`**: stripping the `libretro:` prefix and replacing underscores gives the display name (e.g., `"libretro:Nintendo_-_Super_Nintendo_Entertainment_System"` -> `"Nintendo - Super Nintendo Entertainment System"`).
2. **Queries already go through `thumbnail_repo_names()`**: The code translates a RePlayOS system folder name (e.g., `"nintendo_snes"`) to libretro display names (e.g., `"Nintendo - Super Nintendo Entertainment System"`), then converts to the `libretro:` prefixed URL name for the DB lookup. No display name column needed.
3. **Multi-repo systems work naturally**: `arcade_dc` maps to repos `["Atomiswave", "Sega_-_Naomi", "Sega_-_Naomi_2"]`. The query loads entries from all three `repo_name` values.

The `branch` column is dropped from `thumbnail_index` because it is constant per repo and already stored in `data_sources`. When constructing a download URL, the code looks up `branch` from `data_sources` -- or more practically, it loads `branch` alongside the repo name at the start of a download batch and passes it through.

**Other design choices carried forward:**

- **Dropped `sha` and `size`**: We don't need the git blob SHA or byte size for thumbnail resolution. If the file exists on disk, it's good enough. Removing these saves another ~8 bytes per row.
- **`filename` (not `stem`)**: Clearer intent.
- **Single-column index on `repo_name`**: The primary key `(repo_name, kind, filename)` covers exact lookups. The `repo_name` index speeds "load all entries for a repo" queries, which is the dominant access pattern.

**Trade-offs:**

- Constructing the download URL now requires a lookup into `data_sources` to get `branch`. In practice this is free: the `ManifestFuzzyIndex` builder already queries per-repo, so it fetches `branch` once per repo and stores it in the in-memory `ManifestMatch` struct. No per-row join needed.
- Foreign key constraint adds a dependency on `data_sources` being populated before `thumbnail_index`. This matches the natural import flow (create data source entry, then insert its entries).

### Integration with Existing `game_metadata` Table

No changes needed to `game_metadata`. The `thumbnail_index` table is a separate concern:
- `game_metadata` stores per-ROM enrichment data (description, rating, publisher, image paths)
- `thumbnail_index` stores the global catalog of available thumbnails

The connection between them happens at resolution time: when `resolve_box_art()` finds no local file, it queries `thumbnail_index` for a match, downloads the image, and then updates `game_metadata.box_art_path` as it does today.

### Manifest Size Estimate

For 40 RePlayOS systems, ~208K entries (only `Named_Boxarts` + `Named_Snaps`):
- SQLite with indexes: **~4-7 MB** (down from ~10-15 MB in the denormalized version)
- Rows are lightweight: repo_name (~45 chars, shared via PK prefix) + kind (~13 chars) + filename (~40 chars) + nullable symlink_target
- `repo_name` repetition across rows within the same repo is unavoidable (it's part of the PK), but SQLite's B-tree page compression handles prefix sharing efficiently for sorted keys with a common prefix

This fits easily in `metadata.db` alongside the existing tables.

---

## 4. UI Design: `/more/metadata` Page

### Design Decision: Unified System Table

**Recommendation: ONE unified system table, not two separate lists.**

The current page has two independent per-system displays: "Coverage" (metadata match rates under "Descriptions & Ratings") and per-system image rows (under "Images"). With thumbnail index data added, having three separate system lists would be overwhelming. A unified table gives users a single place to see the health of each system at a glance.

Rationale:

- Both metadata and thumbnails answer the same question: "how complete is the data for this system?"
- Users think in terms of systems ("How's my SNES collection doing?"), not data types.
- A unified view eliminates scrolling past one list to find the other.
- Different *actions* (re-import metadata vs. download thumbnails) are handled by *section-level* buttons in the Data Sources area, not per-row buttons. Per-row actions add clutter and rarely match how users actually work -- they import everything at once, not system by system.

The one exception: a per-system "Download thumbnails" button in the table. This is useful when a user has just added ROMs for a new system and wants images immediately without re-downloading everything. The button only appears when the thumbnail index has matches for that system that are not yet downloaded.

### Page Structure (Revised)

The page has four sections, top to bottom:

1. **Data Sources** -- freshness info + action buttons for each data source
2. **System Overview** -- unified per-system table
3. **Data Management** -- destructive actions (clear images, clear cache)
4. **Attribution** -- credits

The current separate "Descriptions & Ratings" and "Images" sections are merged into the "Data Sources" section (which holds action buttons for both) and the "System Overview" table (which shows the combined per-system view).

### Text Wireframe

```
 < Back                Game Data
 ___________________________________________________

 Data Sources
 ___________________________________________________

 Descriptions & Ratings (LaunchBox)
   12,000 entries -- last updated Mar 9
                                        [ Update ]

 Thumbnails (libretro)
   438 boxart, 312 snaps -- 84 MB on disk
   Index: 208,121 available across 40 systems -- last updated Feb 25
                                        [ Update ]  [ Stop ]

   [===========================........] 73%
   Phase 1: Fetching index... system 34/40 (52s)
   Phase 2: Downloading SNES: 45/62 images... (23s)

 ___________________________________________________

 System Overview
 ___________________________________________________

                        Games  Desc.  Thumb.
 Arcade (FBNeo)           58   72%    66%
 Atari - 2600             24   75%    92%
 Atari - 7800             12   67%    83%
 NEC - PC Engine          22   68%    91%
 Nintendo - NES          130   86%    75%
 Nintendo - SNES          95   94%    65%
 Sega - Genesis           78   86%    95%
 Sega - Master System     31   74%    90%
 Sony - PlayStation       52   87%    --
   ...

 (Systems with no data in either column are hidden)

 ___________________________________________________

 Data Management
 ___________________________________________________
 [ Clear Downloaded Images ]
 [ Clear Thumbnail Index ]
 [ Clear Metadata ]

 ___________________________________________________

 Attribution
 ___________________________________________________
 Game descriptions and ratings from LaunchBox.
 Box art and screenshots from libretro-thumbnails.
```

### Section Details

#### A. Data Sources

This section replaces the current separate "Descriptions & Ratings" stats + button and the "Images" stats + buttons. It groups all external data imports into a single area with a consistent layout.

Each data source is a card showing:
- **Name** (heading within the card)
- **Summary line**: entry count + relative date of last import (from `data_sources` table)
- **Action button**: "Update" to re-import/refresh

The three data sources displayed:

1. **Descriptions & Ratings (LaunchBox)** -- exactly what exists today. Stats come from `get_metadata_stats()`, action is `download_metadata()`. The summary replaces the current info-grid (total entries, with description, with rating, db size) with a single condensed line. The detailed breakdown (how many have ratings vs. descriptions) is unnecessary on a management page -- users care about "do I have data and how old is it?", not per-field breakdowns.

2. **Thumbnails (libretro)** -- new. Replaces the current git-clone-based image import. Displays two stats lines: downloaded image counts + disk size (from `get_image_stats()`), and index availability (aggregate query across `data_sources WHERE source_type = 'libretro-thumbnails'`). A single "Update" button triggers a two-phase pipeline: (1) fetch/refresh the thumbnail index from GitHub Tree API (~60s), then (2) automatically download all matched images (~30-90s depending on collection size). Progress bar and stop button appear inline. Total execution time on Pi 4 is ~90s-2min for a typical collection, making a single-button flow practical. On re-trigger, the index phase is fast (only changed repos are re-fetched via commit SHA comparison) and the download phase skips already-downloaded images.

#### B. System Overview (Unified Table)

A single table showing every system that has ROMs, with columns for:

| Column | Source | Notes |
|---|---|---|
| System name | ROM cache | Display name from system config |
| Games | ROM cache | Total ROM count for the system |
| Desc. | `get_system_coverage()` | Percentage of ROMs with metadata (description/rating) |
| Thumb. | Composite query | Percentage of ROMs with a downloaded thumbnail |

**Data flow for the Thumbnails column:**

The thumbnails column shows `downloaded / total_games` as a percentage. The count comes from `get_image_coverage()` (field `with_boxart`). When no thumbnail index exists yet, the column shows "--".

**Filtering and sorting:**

- Systems with zero ROMs are excluded (as today).
- Systems where both description and thumbnail columns would be empty (no metadata matched, no thumbnails downloaded or available) are excluded to prevent clutter.
- Sorted alphabetically by display name.

**No per-system download buttons.** The full update pipeline (index + download all) completes in ~90s-2min on a Pi 4, making per-system granularity unnecessary. A single "Update" button in the Data Sources section handles everything. This keeps the table clean and read-only — its purpose is visibility, not action.

#### C. Data Management

Three destructive actions with confirmation dialogs:

- **Clear Downloaded Images** — deletes `.replay-control/media/` (all boxart/snap PNGs). Same as today.
- **Clear Thumbnail Index** — deletes all `thumbnail_index` rows from the DB. Cheap to rebuild via "Update".
- **Clear Metadata** — deletes all `game_metadata` rows (LaunchBox descriptions/ratings). Also cheap to rebuild.

The current "Clear Image Cache" button (which deletes the git clone temp dir at `.replay-control/tmp/libretro-thumbnails/`) is no longer needed — the manifest-based approach eliminates git clones entirely. It should be removed.

#### D. Attribution

Updated to credit both data sources:
- "Game descriptions and ratings from LaunchBox."
- "Box art and screenshots from libretro-thumbnails."

### UX Flow: Step by Step

**First-time user:**

1. Navigate to `/more/metadata`
2. Page shows "Data Sources" section with both sources showing "No data" / "Not imported"
3. System Overview table shows systems with ROM counts but "--" in both Desc. and Thumb. columns
4. User clicks "Update" next to "Descriptions & Ratings" -- progress bar streams, completes in ~30-60s
5. Desc. column populates with match percentages
6. User clicks "Update" next to "Thumbnails" -- progress shows two phases:
   - Phase 1: "Fetching index... system 12/40" (~60s)
   - Phase 2: "Downloading images... SNES: 45/62" (~30-90s)
7. Thumb. column populates as images arrive; user can stop at any point

**Returning user (checking freshness):**

1. Page loads with data source summaries showing entry counts and relative dates
2. User sees everything is recent enough, no action needed
3. Glances at System Overview to confirm coverage is good

**User who added new ROMs:**

1. Page shows existing data. New ROMs appear in System Overview with updated counts.
2. If metadata is already imported, descriptions may already match (LaunchBox data is per-title, not per-ROM-file).
3. User clicks "Update" next to "Thumbnails" -- index phase is fast (only changed repos re-fetched), download phase fetches images for the new ROMs only (existing ones are skipped).

### Server Functions

New server functions needed:

| Server Function | Purpose |
|---|---|
| `UpdateThumbnails` | Trigger the two-phase pipeline: (1) refresh index from GitHub Tree API, (2) download all matched images |
| `GetThumbnailProgress` | SSE endpoint for combined index + download progress |
| `GetDataSourceInfo` | Return `data_sources` rows for freshness display |
| `GetUnifiedSystemStats` | Combined per-system stats (metadata + thumbnails) |

These follow the exact same pattern as the existing LaunchBox functions:
- `download_metadata()` triggers a background task via `tokio::task::spawn_blocking`
- Progress is stored in an `RwLock<Option<Progress>>` on `AppState`
- The UI polls via SSE (`/sse/thumbnail-index-progress`, `/sse/thumbnail-download-progress`)
- The `start_*` / `run_*_blocking` pattern from `import.rs` is reused

### Progress Tracking

Two new progress structs (mirroring existing `ImportProgress` and `ImageImportProgress`):

```rust
/// Progress for thumbnail index generation (fetching file listings from GitHub).
pub enum ThumbnailIndexState {
    Fetching,   // Downloading tree listings from GitHub API
    Processing, // Parsing JSON, resolving symlinks, inserting into DB
    Complete,
    Failed,
}

pub struct ThumbnailIndexProgress {
    pub state: ThumbnailIndexState,
    pub current_repo: String,    // Display name of the system being fetched
    pub repos_done: usize,
    pub repos_total: usize,
    pub entries_inserted: usize, // Running total across all repos
    pub elapsed_secs: u64,
    pub error: Option<String>,
}
```

The thumbnail *download* progress reuses `ImageImportState` and `ImageImportProgress` unchanged. The existing progress display component (`ImageProgressDisplay`) works as-is. Only the backend changes: instead of cloning a git repo and copying files, it queries `thumbnail_index` for matches, downloads from raw URLs, and saves to the same media directory.

### Data Source Freshness Display

The `data_sources` table provides the information shown in the Data Sources section. The `GetDataSourceInfo` server function queries by `source_type`:

- **LaunchBox**: single row where `source_name = 'launchbox'`, displays `entry_count` and `imported_at`.
- **Thumbnail Index**: aggregate query across all `source_type = 'libretro-thumbnails'` rows, displaying total `entry_count` and number of repos.

```
Descriptions & Ratings (LaunchBox)
  12,000 entries -- last updated {relative_date(imported_at)}

Thumbnail Index
  208,121 images across 40 systems -- last updated {relative_date(min_imported_at)}
```

The aggregate thumbnail stats are derived directly from SQL (`SUM(entry_count)`, `COUNT(*)`, `MIN(imported_at)`) with no extra columns needed.

**No "update available" indicator.** Checking for updates would require an API call on every page load (checking latest commit SHAs for 40 repos, or checking the LaunchBox download URL for a new file). This is unnecessary network traffic for a feature users trigger manually once every few weeks. The relative date ("last updated 3 days ago") gives users enough information to decide whether to refresh.

---

## 5. Recommended Architecture

### Phase 1: Manifest Generation (User-Triggered)

A server function that:
1. Iterates over all systems in `thumbnail_repo_names()`
2. For each system, calls the GitHub Tree API (1 request per repo)
3. Extracts `Named_Boxarts` and `Named_Snaps` entries
4. Identifies symlinks (size < 100 bytes) and resolves their targets from the tree listing
5. Inserts into `thumbnail_index` table in `metadata.db`
6. Records per-repo commit SHA and branch in `data_sources` (one row per repo, e.g., `libretro:Nintendo_-_NES`)
7. Updates aggregate stats queryable from `data_sources WHERE source_type = 'libretro-thumbnails'`

**API budget**: ~40 requests for all systems (within the 60/hour unauthenticated limit as a single batch; comfortably within 5,000/hour with a personal access token).

**Update strategy**: The manifest changes slowly (community contributions). On re-trigger, check each repo's latest commit SHA against `data_sources.version_hash` for that repo's row. Only re-fetch repos that have changed. This makes updates fast (seconds, not minutes) and further reduces API usage.

### Phase 2: On-Demand Image Resolution

Modify the existing `resolve_box_art()` code path:

```
resolve_box_art(system, rom_filename)
  1. Check local disk cache (.replay-control/media/{system}/boxart/)  -- existing logic
  2. If miss: look up in thumbnail_index using fuzzy matching
  3. If found in manifest:
     a. Construct raw.githubusercontent.com URL using repo_name (from row) and branch (from ManifestMatch, populated at index-build time via data_sources)
     b. If entry is a symlink, use the resolved target filename instead
     c. Download PNG to .replay-control/media/{system}/boxart/
     d. Return the local URL
  4. If not in manifest: return None (no thumbnail available)
```

The download can happen:
- **Synchronously** (blocks the page render ~0.5-1s per image, but only on first view)
- **Asynchronously** (return a placeholder, trigger background download, refresh on completion) -- better UX

### Phase 3: Background Pre-fetch (Optional)

When one image from a system is requested, pre-fetch siblings in the background:
- All ROMs in the same system that have a manifest match but no local file
- Throttled to avoid saturating the Pi's network (e.g., 2-4 concurrent downloads)
- Lower priority than the directly-requested image

### Integration Points

The manifest-based approach integrates cleanly with the existing code:

| Existing Code | Change Needed |
|---|---|
| `thumbnails::thumbnail_repo_names()` | No change -- still maps systems to repo names |
| `thumbnails::thumbnail_filename()` | No change -- still normalizes filenames |
| `thumbnails::find_thumbnail()` | New variant: look up in `thumbnail_index` instead of filesystem |
| `cache::ImageIndex` | Add manifest-backed fallback when disk entry is missing |
| `cache::resolve_box_art()` | Trigger on-demand download when manifest has a match but disk doesn't |
| `import.rs` (ImportSystemImages, ImportAllImages) | Could become "download matched images" using manifest instead of git clone |
| `metadata_db.rs` | Add `thumbnail_index` and `data_sources` tables |

### Raw URL Construction

```
https://raw.githubusercontent.com/libretro-thumbnails/{repo_name}/{branch}/{kind}/{filename}.png
```

Where:
- `repo_name`: from `thumbnail_index.repo_name` (already URL-safe with underscores)
- `branch`: from `data_sources.branch` (looked up once per repo at index-build time, carried in `ManifestMatch`)
- `kind`: `Named_Boxarts` or `Named_Snaps`
- `filename`: URL-encoded PNG filename (spaces become `%20`, parentheses `%28`/`%29`, etc.)

For symlinks, the raw URL returns the text content (not the target image), so the code must:
1. Detect the symlink from the manifest (`symlink_target IS NOT NULL`)
2. Use the target filename instead
3. Construct a new URL for the real file

---

## 6. Manifest Building: GitHub API to SQLite

This section details the full implementation of Phase 1 -- fetching the file listing from GitHub and storing it in the `thumbnail_index` table.

### 6.1 Collecting the repo list

The entry point is `thumbnail_repo_names()` in `thumbnails.rs`, which maps each RePlayOS system folder name to one or more libretro-thumbnails repo display names. Some systems map to multiple repos (e.g., `arcade_dc` maps to `["Atomiswave", "Sega - Naomi", "Sega - Naomi 2"]`, and `commodore_amicd` maps to `["Commodore - CD32", "Commodore - CDTV"]`).

To build the full list of unique repos to fetch, iterate all system entries and deduplicate:

```rust
struct RepoInfo {
    display_name: String,  // "Nintendo - Super Nintendo Entertainment System"
    url_name: String,      // "Nintendo_-_Super_Nintendo_Entertainment_System"
}

fn collect_all_repos() -> Vec<RepoInfo> {
    use replay_control_core::thumbnails::thumbnail_repo_names;
    use replay_control_core::systems;

    let mut repos: Vec<RepoInfo> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for system in systems::visible_systems() {
        if let Some(repo_names) = thumbnail_repo_names(system.folder_name) {
            for display_name in repo_names {
                let url_name = display_name.replace(' ', "_");
                if seen.insert(url_name.clone()) {
                    repos.push(RepoInfo {
                        display_name: display_name.to_string(),
                        url_name,
                    });
                }
            }
        }
    }
    repos
}
```

This deduplicates repos shared across systems (e.g., `MAME` is referenced by both `arcade_mame` and `arcade_mame_2k3p`). The total is ~40 unique repos.

### 6.2 Branch detection

Most repos use `master`; a few use `main`. Two strategies:

**Option A -- Hardcode the known exceptions**: The investigation already identified the `main` repos (Commodore - CD32, Commodore - CDTV, Sega - Naomi, Sega - Naomi 2, Philips - CDi). Store this in a function:

```rust
fn default_branch(repo_display_name: &str) -> &'static str {
    match repo_display_name {
        "Commodore - CD32" | "Commodore - CDTV"
        | "Sega - Naomi" | "Sega - Naomi 2"
        | "Philips - CDi" => "main",
        _ => "master",
    }
}
```

**Option B -- Query the GitHub API**: `GET /repos/libretro-thumbnails/{url_name}` returns `default_branch` in the response. This costs 1 extra API call per repo (80 total instead of 40) but is future-proof. Given we are already well within rate limits, this is acceptable.

**Recommendation**: Start with Option A (hardcoded). If a tree API call fails with 404 on `master`, retry with `main` as a fallback. This handles future branch renames without extra API calls.

### 6.3 Fetching the tree listing

For each repo, call the GitHub Tree API:

```
GET https://api.github.com/repos/libretro-thumbnails/{url_name}/git/trees/{branch}?recursive=1
```

Headers:
- `User-Agent: RePlayOS-Companion/1.0` (required by GitHub)
- `Accept: application/vnd.github+json`

Using `curl` via `std::process::Command` (consistent with the existing LaunchBox download in `launchbox.rs`):

```rust
/// Fetch the full tree listing for a libretro-thumbnails repo.
/// Returns the raw JSON response body.
fn fetch_repo_tree(url_name: &str, branch: &str) -> Result<String> {
    let url = format!(
        "https://api.github.com/repos/libretro-thumbnails/{url_name}/git/trees/{branch}?recursive=1"
    );

    let output = std::process::Command::new("curl")
        .args([
            "-fsSL",
            "--max-time", "30",
            "-H", "User-Agent: RePlayOS-Companion/1.0",
            "-H", "Accept: application/vnd.github+json",
            &url,
        ])
        .output()
        .map_err(|e| Error::Other(format!("Failed to run curl: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Other(format!(
            "GitHub API request failed for {url_name}: {stderr}"
        )));
    }

    String::from_utf8(output.stdout)
        .map_err(|e| Error::Other(format!("Invalid UTF-8 in API response: {e}")))
}
```

The response JSON looks like:

```json
{
  "sha": "abc123...",
  "tree": [
    {
      "path": "Named_Boxarts/Super Mario World (USA).png",
      "mode": "100644",
      "type": "blob",
      "sha": "def456...",
      "size": 45231
    },
    {
      "path": "Named_Boxarts/Super Mario World (Europe).png",
      "mode": "100644",
      "type": "blob",
      "sha": "...",
      "size": 42
    }
  ],
  "truncated": false
}
```

### 6.4 Parsing the response: extracting entries and detecting symlinks

Parse the JSON tree, filtering for `Named_Boxarts/` and `Named_Snaps/` entries:

```rust
use serde::Deserialize;

#[derive(Deserialize)]
struct TreeResponse {
    sha: String,
    tree: Vec<TreeEntry>,
    truncated: bool,
}

#[derive(Deserialize)]
struct TreeEntry {
    path: String,
    #[serde(default)]
    size: u64,
    #[serde(rename = "type")]
    entry_type: String,
}

struct ThumbnailEntry {
    kind: String,                     // "Named_Boxarts" or "Named_Snaps"
    filename: String,                 // stem without .png
    is_symlink: bool,                 // true if size < 100 bytes
}

fn parse_tree_entries(json: &str) -> Result<(String, Vec<ThumbnailEntry>)> {
    let resp: TreeResponse = serde_json::from_str(json)
        .map_err(|e| Error::Other(format!("Failed to parse tree JSON: {e}")))?;

    if resp.truncated {
        tracing::warn!("Tree response was truncated -- some entries may be missing");
    }

    let mut entries = Vec::new();

    for entry in &resp.tree {
        if entry.entry_type != "blob" {
            continue;
        }

        // Filter to Named_Boxarts/ and Named_Snaps/ only.
        let (kind, rest) = if let Some(rest) = entry.path.strip_prefix("Named_Boxarts/") {
            ("Named_Boxarts", rest)
        } else if let Some(rest) = entry.path.strip_prefix("Named_Snaps/") {
            ("Named_Snaps", rest)
        } else {
            continue;
        };

        // Extract the filename stem (strip .png extension).
        let stem = match rest.strip_suffix(".png") {
            Some(s) => s.to_string(),
            None => continue,
        };

        // Detect symlinks: real PNGs are almost always > 100 bytes.
        // Symlinks are small text files containing a relative path.
        let is_symlink = entry.size < 100;

        entries.push(ThumbnailEntry {
            kind: kind.to_string(),
            filename: stem,
            is_symlink,
        });
    }

    Ok((resp.sha, entries))
}
```

**Important design note on symlink resolution**: The tree API gives us `size` but not the file content. We know an entry is a symlink (size < 100), but we do not know its target without fetching the blob. There are two viable strategies:

1. **Resolve at download time** (recommended): When downloading an image for a ROM that matched a symlink entry, first fetch the raw URL. If the response is < 200 bytes and doesn't start with PNG magic bytes (`\x89PNG`), it's the symlink text content. Read the target path from it, construct a new URL, and fetch the real image. This adds one extra HTTP round-trip for symlink entries only (~0.5s), but avoids thousands of blob API calls during manifest import.

2. **Batch-resolve during manifest import**: For each symlink entry, call `GET /repos/libretro-thumbnails/{repo}/git/blobs/{sha}` (returns base64-encoded content). This would resolve all symlinks upfront but costs one API call per symlink. For SNES with ~2,000 symlinks, that's 2,000 extra API calls -- hitting rate limits.

Strategy 1 is strongly preferred. Store `symlink_target` as an empty string (placeholder) in the DB for symlink entries. At download time, detect and resolve. This is consistent with how `resolve_fake_symlink()` in the current `thumbnails.rs` already works at the filesystem level -- it reads the small file, detects it's not PNG, interprets it as a path, and follows it.

### 6.5 Inserting into SQLite

After parsing each repo's tree, bulk-insert into `thumbnail_index`:

```rust
fn insert_thumbnail_entries(
    db: &mut MetadataDb,
    source_name: &str,  // e.g., "libretro:Nintendo_-_NES"
    entries: &[ThumbnailEntry],
) -> Result<usize> {
    // Delete existing entries for this repo before inserting
    // (handles re-imports cleanly).
    db.conn.execute(
        "DELETE FROM thumbnail_index WHERE repo_name = ?1",
        params![source_name],
    )?;

    let tx = db.conn.transaction()?;
    let mut count = 0;
    {
        let mut stmt = tx.prepare_cached(
            "INSERT OR REPLACE INTO thumbnail_index
             (repo_name, kind, filename, symlink_target)
             VALUES (?1, ?2, ?3, ?4)"
        )?;

        for entry in entries {
            let symlink_target = if entry.is_symlink {
                Some(String::new())  // Placeholder -- resolved at download time
            } else {
                None
            };

            stmt.execute(params![
                source_name,
                entry.kind,
                entry.filename,
                symlink_target,
            ])?;
            count += 1;
        }
    }
    tx.commit()?;
    Ok(count)
}
```

### 6.6 Rate limiting and error handling

**GitHub API rate limits**:
- Unauthenticated: 60 requests/hour (tight for all 40 repos in one pass, but sufficient since a full import is a single batch of 40 requests)
- Authenticated (personal access token): 5,000 requests/hour

For unauthenticated use, fetch repos sequentially with a small delay between requests. 40 repos at ~1-2s per request = ~60-80s total. This is within the 60/hour limit as a single batch, but repeated triggers within the same hour would exhaust it.

**Mitigation**: The `data_sources` table enables incremental updates. After the first full import, re-triggers only fetch repos whose `version_hash` (commit SHA) has changed (typically 0-3 repos), staying well within limits.

**Error handling per repo**:
- HTTP 404: branch name wrong -- retry with `main` if tried `master`, or vice versa
- HTTP 403 (rate limited): stop the entire import, report error with retry-after time
- HTTP 5xx or network timeout: skip this repo, continue with next, report partial failure
- JSON parse error: skip this repo, log warning

```rust
fn import_all_manifests(
    db: &mut MetadataDb,
    on_progress: impl Fn(usize, usize, &str),
) -> Result<ManifestImportStats> {
    let repos = collect_all_repos();
    let total = repos.len();
    let mut total_entries = 0usize;
    let mut repos_fetched = 0usize;
    let mut errors: Vec<String> = Vec::new();

    for (i, repo) in repos.iter().enumerate() {
        on_progress(i, total, &repo.display_name);

        // Check if repo has changed since last import.
        let source_name = format!("libretro:{}", repo.url_name);
        if let Some(status) = db.get_data_source(&source_name)? {
            match check_repo_freshness(&repo.url_name, status.version_hash.as_deref().unwrap_or("")) {
                Ok(false) => {
                    // Repo unchanged -- skip.
                    total_entries += status.entry_count as usize;
                    repos_fetched += 1;
                    continue;
                }
                Ok(true) => { /* Repo changed, re-fetch below. */ }
                Err(e) => {
                    tracing::warn!("Freshness check failed for {}: {e}", repo.display_name);
                    // Can't tell -- re-fetch to be safe.
                }
            }
        }

        let branch = default_branch(&repo.display_name);
        let json = match fetch_repo_tree(&repo.url_name, branch) {
            Ok(j) => j,
            Err(_) => {
                // Try the other branch before giving up.
                let alt = if branch == "master" { "main" } else { "master" };
                match fetch_repo_tree(&repo.url_name, alt) {
                    Ok(j) => j,
                    Err(e) => {
                        errors.push(format!("{}: {e}", repo.display_name));
                        continue;
                    }
                }
            }
        };

        let (commit_sha, entries) = parse_tree_entries(&json)?;
        let count = insert_thumbnail_entries(
            db, &source_name, &entries,
        )?;

        db.upsert_data_source(&source_name, "libretro-thumbnails", &commit_sha, branch, count)?;
        total_entries += count;
        repos_fetched += 1;
    }

    Ok(ManifestImportStats { repos_fetched, total_entries, errors })
}
```

### 6.7 System-to-repo mapping during queries

The `thumbnail_index` uses `repo_name` (the `libretro:`-prefixed source name) as its grouping key, not the RePlayOS folder name. When querying for a specific RePlayOS system, use `thumbnail_repo_names()` to get display names, then convert to source names:

```rust
fn query_manifest_for_system(
    db: &MetadataDb,
    replayos_system: &str,
    kind: &str,
) -> Result<Vec<ManifestEntry>> {
    let repo_names = thumbnail_repo_names(replayos_system)
        .ok_or_else(|| Error::Other(format!("No thumbnail repo for {replayos_system}")))?;

    let mut results = Vec::new();
    for display_name in repo_names {
        let source_name = format!("libretro:{}", display_name.replace(' ', "_"));
        let entries = db.query_thumbnail_index(&source_name, kind)?;
        results.extend(entries);
    }
    Ok(results)
}
```

This correctly handles multi-repo systems like `arcade_dc` which spans 3 repos (`libretro:Atomiswave`, `libretro:Sega_-_Naomi`, `libretro:Sega_-_Naomi_2`).

---

## 7. Image Matching: ROM Filename to Thumbnail Entry

This section describes how to match a user's ROM file against the `thumbnail_index` table using the same 3-tier fuzzy matching logic that already exists in `thumbnails.rs`.

### 7.1 Existing matching tiers (from `thumbnails.rs`)

The current filesystem-based matching in `find_thumbnail()` works in three tiers:

1. **Exact match**: `thumbnail_filename(rom_stem)` == `index_entry_stem`. The `thumbnail_filename()` function normalizes special characters (`&*/:\`<>?\\|"` become `_`), so `"Game: The Sequel"` becomes `"Game_ The Sequel"`.

2. **Strip-tags match**: Strip parenthesized/bracketed tags from both the ROM stem and the index entries, then compare case-insensitively. `strip_tags("Indiana Jones (Spanish)")` yields `"Indiana Jones"`. This catches region/revision variants.

3. **Version-stripped match**: Further strip TOSEC/GDI version strings. `strip_version("Sonic Adventure 2 v1.008")` yields `"Sonic Adventure 2"`. Handles Dreamcast GDI dumps and TOSEC-named ROMs.

Additionally, for arcade systems, the ROM filename (a MAME codename like `sf2`) is translated to a display name via `arcade_db::lookup_arcade_game()` before matching.

There is also a **colon-variant fallback**: since `thumbnail_filename()` replaces `:` with `_`, but some libretro contributors used ` -` or dropped the colon, the current code tries alternative normalizations (e.g., `"Title: Subtitle"` -> `"Title - Subtitle"` and `"Title Subtitle"`).

### 7.2 Matching against the `thumbnail_index` table

The same logic applies, but instead of scanning a filesystem directory, we query SQLite. The key design question: **should matching be done in SQL or in Rust after loading the index?**

**Recommendation: Load into Rust, match in-memory** (same approach as the existing `FuzzyIndex` / `ImageIndex`).

Reasons:
- The strip-tags, version-stripping, and colon-variant logic is already written in Rust and is non-trivial to express in SQL
- Per-system index sizes are small (MAME has ~17K boxart entries; most systems have 1-5K)
- Loading all `filename` values for one system + kind is a single indexed query, returning ~5K-17K strings
- Building a `HashMap` in Rust for O(1) lookups is fast and mirrors the existing pattern

```rust
/// A fuzzy index built from thumbnail_index DB entries instead of filesystem.
struct ManifestFuzzyIndex {
    /// exact thumbnail_filename stem -> ManifestMatch
    exact: HashMap<String, ManifestMatch>,
    /// lowercase(strip_tags(stem)) -> ManifestMatch
    by_tags: HashMap<String, ManifestMatch>,
    /// lowercase(strip_version(strip_tags(stem))) -> ManifestMatch
    by_version: HashMap<String, ManifestMatch>,
}

#[derive(Clone)]
struct ManifestMatch {
    filename: String,           // The stem as stored in thumbnail_index
    is_symlink: bool,           // Whether symlink_target is set
    repo_url_name: String,      // URL name (strip "libretro:" prefix from thumbnail_index.repo_name)
    branch: String,             // Looked up from data_sources at index-build time
}

fn build_manifest_fuzzy_index(
    db: &MetadataDb,
    repo_display_names: &[&str],
    kind: &str,
) -> ManifestFuzzyIndex {
    // NOTE: strip_tags is currently private in thumbnails.rs -- it will need
    // to be made `pub` (or the logic inlined here) before this code compiles.
    use replay_control_core::thumbnails::{strip_version, strip_tags};

    let mut exact = HashMap::new();
    let mut by_tags = HashMap::new();
    let mut by_version = HashMap::new();

    for display_name in repo_display_names {
        let url_name = display_name.replace(' ', "_");

        // Look up branch from data_sources (one lookup per repo).
        let source_name = format!("libretro:{url_name}");
        let branch = db.get_data_source(&source_name)
            .ok().flatten()
            .and_then(|s| s.branch)
            .unwrap_or_else(|| "master".to_string());

        let entries = db.query_thumbnail_index(&source_name, kind)
            .unwrap_or_default();

        for entry in entries {
            let m = ManifestMatch {
                filename: entry.filename.clone(),
                is_symlink: entry.symlink_target.is_some(),
                repo_url_name: url_name.clone(),
                branch: branch.clone(),
            };

            // Tier 1: exact
            exact.entry(entry.filename.clone()).or_insert_with(|| m.clone());

            // Tier 2: strip tags
            let stripped = strip_tags(&entry.filename);
            let key = stripped.to_lowercase();
            by_tags.entry(key.clone()).or_insert_with(|| m.clone());

            // Tier 3: version-stripped
            let version_key = strip_version(&key);
            if version_key.len() < key.len() {
                by_version.entry(version_key.to_string()).or_insert(m);
            }
        }
    }

    ManifestFuzzyIndex { exact, by_tags, by_version }
}
```

### 7.3 The matching function

```rust
/// Look up a ROM in the manifest fuzzy index.
/// Returns the matching manifest entry, or None.
fn find_in_manifest(
    index: &ManifestFuzzyIndex,
    rom_filename: &str,
    system: &str,
) -> Option<&ManifestMatch> {
    // NOTE: strip_tags is currently private -- see note in build_manifest_fuzzy_index.
    use replay_control_core::thumbnails::{thumbnail_filename, strip_version, strip_tags};
    use replay_control_core::arcade_db;

    let stem = rom_filename.rfind('.')
        .map(|i| &rom_filename[..i])
        .unwrap_or(rom_filename);

    let is_arcade = matches!(
        system,
        "arcade_mame" | "arcade_fbneo" | "arcade_mame_2k3p" | "arcade_dc"
    );

    // For arcade ROMs, translate MAME codename to display name.
    let display_name = if is_arcade {
        arcade_db::lookup_arcade_game(stem).map(|info| info.display_name)
    } else {
        None
    };
    let thumb_name = thumbnail_filename(display_name.unwrap_or(stem));

    // Tier 1: exact match.
    if let Some(m) = index.exact.get(&thumb_name) {
        return Some(m);
    }

    // Colon variants (same logic as import_system_thumbnails in thumbnails.rs).
    let source = display_name.unwrap_or(stem);
    if source.contains(':') {
        let dash_variant = thumbnail_filename(
            &source.replace(": ", " - ").replace(':', " -"),
        );
        if let Some(m) = index.exact.get(&dash_variant) {
            return Some(m);
        }
        let drop_variant = thumbnail_filename(
            &source.replace(": ", " ").replace(':', ""),
        );
        if let Some(m) = index.exact.get(&drop_variant) {
            return Some(m);
        }
    }

    // Tier 2: strip tags.
    let key = strip_tags(&thumb_name).to_lowercase();
    if let Some(m) = index.by_tags.get(&key) {
        return Some(m);
    }

    // Tier 3: version-stripped.
    let version_key = strip_version(&key);
    if version_key.len() < key.len() {
        if let Some(m) = index.by_tags.get(version_key)
            .or_else(|| index.by_version.get(version_key))
        {
            return Some(m);
        }
    }

    None
}
```

### 7.4 SQL query for loading the index

The query to load all entries for a repo + kind uses the `idx_thumbidx_repo` index:

```sql
SELECT filename, symlink_target
FROM thumbnail_index
WHERE repo_name = ?1 AND kind = ?2
```

The `branch` is fetched separately from `data_sources` (one query per repo, done once at the start of `build_manifest_fuzzy_index`). No join needed per row.

For a system like SNES with ~3,700 boxart entries, this returns ~3,700 rows with short strings. Loading time is negligible (< 10ms on Pi 4).

---

## 8. Image Download: Thumbnail Entry to Local File

### 8.1 URL construction

Given a `ManifestMatch`, the download URL is:

```
https://raw.githubusercontent.com/libretro-thumbnails/{repo_url_name}/{branch}/{kind}/{url_encoded_filename}.png
```

The `repo_url_name` already uses underscores for spaces (e.g., `Nintendo_-_Super_Nintendo_Entertainment_System`), but the filename within the path contains spaces, parentheses, and other characters that need percent-encoding.

```rust
/// Construct the raw.githubusercontent.com URL for a thumbnail.
fn thumbnail_download_url(m: &ManifestMatch, kind: &str) -> String {
    let encoded = url_encode_path_component(&format!("{}.png", m.filename));
    format!(
        "https://raw.githubusercontent.com/libretro-thumbnails/{}/{}/{}/{}",
        m.repo_url_name, m.branch, kind, encoded,
    )
}

/// Percent-encode a single path component for a URL.
/// Encodes everything except unreserved characters (RFC 3986).
fn url_encode_path_component(s: &str) -> String {
    let mut result = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9'
            | b'-' | b'_' | b'.' | b'~' => {
                result.push(b as char);
            }
            _ => {
                result.push_str(&format!("%{:02X}", b));
            }
        }
    }
    result
}
```

Example URL for `Super Mario World (USA).png` in the SNES repo:
```
https://raw.githubusercontent.com/libretro-thumbnails/
    Nintendo_-_Super_Nintendo_Entertainment_System/master/
    Named_Boxarts/Super%20Mario%20World%20%28USA%29.png
```

### 8.2 Symlink handling at download time

If `m.is_symlink` is true, the entry in the libretro repo is a git symlink. Fetching its raw URL returns the symlink text content (a relative path like `Super Mario World (USA).png`), NOT the actual image.

The detection and resolution happens at download time:

```rust
const PNG_MAGIC: [u8; 4] = [0x89, b'P', b'N', b'G'];

/// Download a thumbnail image, handling symlink resolution transparently.
/// Returns the raw PNG bytes on success.
fn download_thumbnail(m: &ManifestMatch, kind: &str) -> Result<Vec<u8>> {
    let url = thumbnail_download_url(m, kind);
    let bytes = curl_download_bytes(&url)?;

    // Check if this is a symlink (text content instead of PNG).
    if bytes.len() < 200 && !bytes.starts_with(&PNG_MAGIC) {
        // The content is the relative target path, e.g., "Super Mario World (USA).png"
        // or "../Named_Snaps/foo.png" (rare cross-directory symlink).
        let target_path = std::str::from_utf8(&bytes)
            .map_err(|e| Error::Other(format!("Invalid symlink content: {e}")))?
            .trim();

        // Extract just the filename from the relative path.
        let target_filename = target_path
            .rsplit('/')
            .next()
            .unwrap_or(target_path);

        let encoded = url_encode_path_component(target_filename);
        let target_url = format!(
            "https://raw.githubusercontent.com/libretro-thumbnails/{}/{}/{}/{}",
            m.repo_url_name, m.branch, kind, encoded,
        );

        let real_bytes = curl_download_bytes(&target_url)?;

        if real_bytes.len() < 200 && !real_bytes.starts_with(&PNG_MAGIC) {
            return Err(Error::Other(format!(
                "Symlink chain: {} -> {} did not resolve to a valid PNG",
                m.filename, target_path,
            )));
        }

        return Ok(real_bytes);
    }

    Ok(bytes)
}
```

This approach mirrors the existing `resolve_fake_symlink()` in `thumbnails.rs` which does the same detection at the filesystem level: read the file, check for PNG magic bytes, and if missing, interpret the content as a relative path.

### 8.3 Downloading with curl

Using `curl` via `std::process::Command`, consistent with the LaunchBox download in `launchbox.rs`:

```rust
/// Download raw bytes from a URL using curl (blocking).
fn curl_download_bytes(url: &str) -> Result<Vec<u8>> {
    let output = std::process::Command::new("curl")
        .args([
            "-fsSL",
            "--max-time", "15",
            "--retry", "2",
            "--retry-delay", "1",
            url,
        ])
        .output()
        .map_err(|e| Error::Other(format!("Failed to run curl: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Other(format!("Download failed for {url}: {stderr}")));
    }

    Ok(output.stdout)
}
```

For the async variant (used in on-demand single-image fetch within a tokio context):

```rust
/// Download raw bytes from a URL using curl (async, non-blocking).
async fn curl_download_bytes_async(url: &str) -> Result<Vec<u8>> {
    let output = tokio::process::Command::new("curl")
        .args([
            "-fsSL",
            "--max-time", "15",
            "--retry", "2",
            "--retry-delay", "1",
            url,
        ])
        .output()
        .await
        .map_err(|e| Error::Other(format!("Failed to run curl: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Other(format!("Download failed for {url}: {stderr}")));
    }

    Ok(output.stdout)
}
```

### 8.4 Local file storage

Downloaded images are saved to the same location the current git-clone import uses:

```
{storage_root}/.replay-control/media/{replayos_system}/boxart/{matched_stem}.png
{storage_root}/.replay-control/media/{replayos_system}/snap/{matched_stem}.png
```

Where `{matched_stem}` is the `filename` from the manifest match (the libretro-thumbnails stem, not the ROM stem). This matches the existing convention -- `import_system_thumbnails()` in `thumbnails.rs` already saves files as `boxart/{matched_stem}.png`.

The `ThumbnailKind` enum already maps between repo directory names and local directory names:
- `ThumbnailKind::Boxart`: repo dir `Named_Boxarts`, local dir `boxart`
- `ThumbnailKind::Snap`: repo dir `Named_Snaps`, local dir `snap`

```rust
fn save_thumbnail(
    storage_root: &Path,
    system: &str,
    kind: ThumbnailKind,
    matched_stem: &str,
    png_bytes: &[u8],
) -> Result<std::path::PathBuf> {
    let media_dir = storage_root
        .join(replay_control_core::storage::RC_DIR)  // ".replay-control"
        .join("media")
        .join(system)
        .join(kind.media_dir());  // "boxart" or "snap"

    std::fs::create_dir_all(&media_dir)
        .map_err(|e| Error::io(&media_dir, e))?;

    let dest = media_dir.join(format!("{matched_stem}.png"));
    std::fs::write(&dest, png_bytes)
        .map_err(|e| Error::io(&dest, e))?;

    Ok(dest)
}
```

### 8.5 Error handling for downloads

| Error | Action |
|---|---|
| HTTP 404 | Image does not exist at that URL. Log a debug-level warning, skip. Do not retry. |
| HTTP 403 (rate limited) | `raw.githubusercontent.com` rarely rate-limits, but if it does, back off for 60s then retry once. |
| HTTP 5xx | Transient server error. Retry up to 2 times (handled by curl's `--retry 2`). |
| Network timeout | Handled by curl's `--max-time 15`. Log and skip. |
| Symlink chain error | The symlink target is also a symlink or does not exist. Log and skip. |
| Disk write failure | Fatal for this image. Log error, continue with next image. |

---

## 9. Bulk Download Flow

### 9.1 Per-system download

The bulk download processes one system at a time. For each system:

1. Load the manifest fuzzy index from `thumbnail_index` for this system's repo(s)
2. List all ROM filenames for the system (existing `list_rom_filenames()`)
3. For each ROM, check if local boxart already exists on disk
4. If not, attempt fuzzy match against the manifest index
5. If matched, download the image and save it

```rust
struct DownloadStats {
    total: usize,
    downloaded: usize,
    skipped: usize,
    failed: usize,
}

async fn download_system_thumbnails(
    db: &MetadataDb,
    storage_root: &Path,
    system: &str,
    kind: ThumbnailKind,
    on_progress: impl Fn(usize, usize, usize),  // (processed, total, downloaded)
    cancel: Arc<std::sync::atomic::AtomicBool>,
) -> Result<DownloadStats> {
    let repo_names = thumbnail_repo_names(system)
        .ok_or_else(|| Error::Other(format!("No thumbnail repo for {system}")))?;

    let display_names: Vec<&str> = repo_names.to_vec();

    // Build the fuzzy index from the manifest.
    let manifest_index = build_manifest_fuzzy_index(db, &display_names, kind.repo_dir());

    let rom_filenames = list_rom_filenames(storage_root, system);
    let total = rom_filenames.len();

    let media_dir = storage_root
        .join(crate::storage::RC_DIR)
        .join("media")
        .join(system)
        .join(kind.media_dir());

    // Phase 1: Collect work items (ROMs that need a download).
    let mut work: Vec<(String, ManifestMatch)> = Vec::new();
    let mut skipped = 0usize;
    for rom_filename in &rom_filenames {
        if let Some(m) = find_in_manifest(&manifest_index, rom_filename, system) {
            let local_path = media_dir.join(format!("{}.png", m.filename));
            if local_path.exists() {
                skipped += 1;  // Already downloaded.
            } else {
                work.push((rom_filename.clone(), m.clone()));
            }
        }
        // ROMs with no manifest match are silently ignored.
    }

    // Phase 2: Download in parallel (see section 15 for benchmarks).
    let downloaded = Arc::new(AtomicUsize::new(0));
    let failed = Arc::new(AtomicUsize::new(0));
    let processed = Arc::new(AtomicUsize::new(0));
    let concurrency = 10; // 10-16 recommended for Pi 4

    let semaphore = Arc::new(tokio::sync::Semaphore::new(concurrency));
    let mut handles = Vec::new();

    for (_rom_filename, m) in work {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }

        let sem = semaphore.clone();
        let kind_dir = kind.repo_dir().to_string();
        let root = storage_root.to_path_buf();
        let sys = system.to_string();
        let dl = downloaded.clone();
        let fl = failed.clone();
        let pr = processed.clone();
        let cancel = cancel.clone();

        let handle = tokio::spawn(async move {
            let _permit = sem.acquire().await.unwrap();
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return;
            }

            match download_thumbnail(&m, &kind_dir) {
                Ok(bytes) => {
                    match save_thumbnail(&root, &sys, kind, &m.filename, &bytes) {
                        Ok(_) => { dl.fetch_add(1, std::sync::atomic::Ordering::Relaxed); }
                        Err(e) => {
                            tracing::warn!("Failed to save {}: {e}", m.filename);
                            fl.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                }
                Err(e) => {
                    tracing::debug!("Failed to download {}: {e}", m.filename);
                    fl.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            }
            pr.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        });
        handles.push(handle);
    }

    // Await all downloads, reporting progress periodically.
    for handle in handles {
        let _ = handle.await;
        let done = processed.load(std::sync::atomic::Ordering::Relaxed);
        on_progress(skipped + done, total, downloaded.load(std::sync::atomic::Ordering::Relaxed));
    }

    let downloaded = downloaded.load(std::sync::atomic::Ordering::Relaxed);
    let failed = failed.load(std::sync::atomic::Ordering::Relaxed);
    on_progress(total, total, downloaded);
    Ok(DownloadStats { total, downloaded, skipped, failed })
}
```

### 9.2 Progress reporting via SSE

The bulk download reuses the existing `ImageImportProgress` struct and SSE pattern. The states change from clone-based to download-based:

| Old State | New Equivalent | Description |
|---|---|---|
| `Cloning` | `Indexing` | Building the manifest fuzzy index for the system |
| `Copying` | `Downloading` | Fetching images from raw.githubusercontent.com |
| `Complete` | `Complete` | Unchanged |
| `Failed` | `Failed` | Unchanged |
| `Cancelled` | `Cancelled` | Unchanged |

The progress fields map naturally:
- `processed`: number of ROMs checked so far
- `total`: total ROM count for the system
- `boxart_copied` -> `downloaded`: number of images fetched so far
- `current_system` / `total_systems`: for "Download All" multi-system runs

### 9.3 Throttling and concurrency

**Parallel downloads are recommended.** See section 15 for detailed benchmarks.

Testing shows that `raw.githubusercontent.com` (Fastly CDN) handles high concurrency without rate limiting -- 1056 concurrent image downloads returned all HTTP 200 with zero failures. Sequential downloads waste time on per-request latency (~0.3-0.5s per image); parallel downloads amortize this across all images.

**Recommended concurrency**: 10-16 parallel downloads on a Pi 4. This keeps CPU and memory overhead reasonable while dramatically reducing wall-clock time. On a 50 Mbps connection, 200 matched images download in ~10s with concurrency 10, versus ~60-100s sequential.

**Implementation**: Use `tokio::sync::Semaphore` with a configurable permit count. Spawn all downloads as `tokio::spawn` tasks, each acquiring a permit before starting the HTTP request. Progress reporting aggregates across all in-flight tasks.

```rust
let semaphore = Arc::new(Semaphore::new(concurrency));
let mut handles = Vec::new();

for (_rom, m) in work {
    let sem = semaphore.clone();
    let handle = tokio::spawn(async move {
        let _permit = sem.acquire().await.unwrap();
        download_thumbnail(&m, kind.repo_dir())
    });
    handles.push(handle);
}
```

**Per-system throttling**: Between systems in a multi-system download, no delay is needed -- just proceed to the next system immediately.

### 9.4 Resume capability

The download is inherently resumable:
- Before downloading each image, check if the local file already exists (`local_path.exists()`)
- If it exists, skip it (already downloaded in a previous run)
- If interrupted and re-run, the loop skips all previously downloaded images and continues from where it left off
- No bookkeeping file or DB flag needed -- the filesystem IS the state

This matches the existing behavior of `import_system_thumbnails()` in `thumbnails.rs`, which checks `if !dst.exists()` before copying.

### 9.5 DB updates after download

After downloading images for a system, update the `game_metadata` table's `box_art_path` column, just as the current `import_system_thumbnails()` does via `bulk_update_image_paths()`:

```rust
// After downloading all images for a system, update DB paths.
let mut db_updates: Vec<(String, String, Option<String>, Option<String>)> = Vec::new();

for (rom_filename, manifest_match) in &successful_downloads {
    db_updates.push((
        system.to_string(),
        rom_filename.to_string(),
        Some(format!("boxart/{}.png", manifest_match.filename)),
        None,  // snap_path updated separately if downloading snaps
    ));
}

if !db_updates.is_empty() {
    db.bulk_update_image_paths(&db_updates)?;
}
```

---

## 10. On-Demand Single Image Fetch

### 10.1 When it triggers

The current `resolve_box_art()` in `cache.rs` follows this lookup chain:

1. DB path from `game_metadata.box_art_path` (validated against disk via `ImageIndex.exact`)
2. Exact `thumbnail_filename(stem)` match against disk `ImageIndex.exact`
3. Fuzzy match (strip tags) against disk `ImageIndex.fuzzy`
4. Version-stripped match against disk `ImageIndex.version`
5. If all miss: return `None`

With the manifest, a new step is inserted before returning `None`:

```
5. If no local file: look up in thumbnail_index via ManifestFuzzyIndex
6. If found in manifest: trigger background download, return None for now
7. If not in manifest: return None (no thumbnail available anywhere)
```

### 10.2 Synchronous vs. asynchronous download

**Option A -- Synchronous** (block the page render):
- `resolve_box_art()` calls `download_thumbnail()` inline
- Page load is delayed ~0.5-1s per missing image
- Simple to implement, but bad for system listing pages that show 20-50 ROMs simultaneously

**Option B -- Async with placeholder** (recommended):
- `resolve_box_art()` returns `None` for undownloaded manifest matches
- A spawned task downloads the image in the background
- On the next page load (or after navigating away and back), the image appears
- The UI already handles missing box art with placeholder styling, so no visual breakage

**Option C -- Hybrid streaming**:
- A special server route `/media/{system}/boxart/_pending/{stem}` downloads-and-streams on the fly
- First load gets the image (with ~0.5-1s delay per image), subsequent loads are instant from disk
- Most complex to implement, best single-image UX, but poor for list pages (serial per-image delays)

**Recommendation**: Start with Option B (async). Implementation:

```rust
// In resolve_box_art(), after all disk lookups miss (step 4):

// Step 5: Check manifest for an available thumbnail.
if let Some(manifest_match) = find_in_manifest(&manifest_index, rom_filename, system) {
    // Queue a background download (fire-and-forget).
    let m = manifest_match.clone();
    let root = storage_root.clone();
    let sys = system.to_string();
    tokio::spawn(async move {
        match curl_download_bytes_async(&thumbnail_download_url(&m, "Named_Boxarts")).await {
            Ok(bytes) => {
                if let Err(e) = save_thumbnail(&root, &sys, ThumbnailKind::Boxart, &m.filename, &bytes) {
                    tracing::debug!("On-demand save failed for {}: {e}", m.filename);
                }
            }
            Err(e) => {
                tracing::debug!("On-demand download failed for {}: {e}", m.filename);
            }
        }
    });
    return None;  // Image will appear on next page load.
}
```

The UX implication: on the first visit to a system page after building the manifest, box art slots are empty for images not yet downloaded. As background downloads complete (each taking ~0.5-1s), refreshing the page progressively reveals them. This is the same UX users already experience when they haven't run the image import.

### 10.3 Avoiding duplicate downloads

When a system page loads with 50 ROMs, `resolve_box_art()` is called 50 times, potentially spawning 50 concurrent background downloads. Use an in-memory `HashSet` on `AppState` to deduplicate:

```rust
// Add to AppState:
pending_thumbnail_downloads: std::sync::RwLock<HashSet<String>>,

// In resolve_box_art(), before spawning a download:
let download_key = format!("{system}/{}", manifest_match.filename);
{
    let mut pending = state.pending_thumbnail_downloads.write().unwrap();
    if !pending.insert(download_key.clone()) {
        return None;  // Already queued by a concurrent call.
    }
}

// In the spawned download task, after completion (success or failure):
{
    let mut pending = state.pending_thumbnail_downloads.write().unwrap();
    pending.remove(&download_key);
}
```

This ensures each image is downloaded at most once, even when multiple page loads trigger `resolve_box_art()` for the same ROM before the download completes.

### 10.4 When to build the manifest fuzzy index

Building the `ManifestFuzzyIndex` from SQLite takes ~5-10ms per system. It should NOT be rebuilt on every `resolve_box_art()` call. Instead, cache it alongside the existing `ImageIndex`:

```rust
// Extend ImageIndex with an optional manifest fallback:
pub struct ImageIndex {
    // ... existing fields (exact, fuzzy, version, db_paths, dir_mtime, expires) ...

    /// Manifest-backed fallback for images not yet downloaded.
    /// None if the thumbnail_index table has no entries for this system.
    pub manifest: Option<ManifestFuzzyIndex>,
}
```

The manifest index is built once when `get_image_index()` constructs the `ImageIndex`, and is reused across all `resolve_box_art()` calls for that system within the same cache cycle (same mtime-based + 300s hard TTL invalidation as the disk-based index).

---

## 11. Cache Integration

### 11.1 How downloaded thumbnails interact with L1/L2/L3

The existing cache hierarchy in `cache.rs`:
- **L1**: In-memory `ImageIndex` (per-system, mtime-based invalidation, 300s hard TTL)
- **L2**: SQLite `game_library` table (per-ROM `box_art_url` column, updated by `enrich_system_cache()`)
- **L3**: Filesystem scan of `.replay-control/media/{system}/boxart/`

When a new thumbnail is downloaded to disk:
- **L3** is automatically up-to-date (the file now exists on disk)
- **L1** becomes stale because `dir_mtime(boxart_dir)` changes when a new file is written. The next call to `get_image_index()` detects the mtime change via `ImageIndex::is_fresh()` and rebuilds from disk, picking up the new file. No explicit invalidation call is needed.
- **L2** needs an explicit update to the `box_art_url` column in `game_library` for the affected ROM(s).

### 11.2 The `enrich_system_cache()` flow

`enrich_system_cache()` in `cache.rs` updates L2 (game_library) with box art URLs and ratings. It:

1. Gets the `ImageIndex` for the system via `get_image_index()` (L1 or rebuild from L3)
2. Reads all ROM filenames from the L1 rom cache
3. For each ROM, calls `resolve_box_art()` against the `ImageIndex`
4. Collects `(filename, box_art_url, rating)` tuples for ROMs that have matches
5. Calls `db.update_box_art_and_rating()` to batch-update L2
6. Also updates the L1 in-memory rom entries with the resolved URLs

**After bulk thumbnail downloads**: Call `enrich_system_cache()` once per system after all images for that system are downloaded. This is exactly what the existing background verification flow does -- `spawn_cache_verification()` calls `enrich_system_cache()` for every stale system on startup.

```rust
// In the bulk download completion handler, after all images
// for a system are downloaded:
fn after_system_download(state: &AppState, system: &str) {
    // Force L1 image index rebuild (picks up new files).
    if let Ok(mut guard) = state.cache.images.write() {
        guard.remove(system);
    }

    // Propagate new box art paths to L2 (game_library).
    state.cache.enrich_system_cache(state, system);
}
```

### 11.3 On-demand downloads and cache coherence

When a single image is downloaded on-demand (section 10), the cache update path is:

1. Image saved to disk --> L3 is current
2. L1 `ImageIndex` detects `boxart_dir` mtime change on next `get_image_index()` call --> rebuilds automatically, includes the new file
3. L2 `box_art_url` is NOT updated until `enrich_system_cache()` runs

This means L2 may lag behind for on-demand downloads. This is acceptable because:
- `resolve_box_art()` checks L1 (disk-based index) before falling back to L2. Once the image is on disk and L1 is rebuilt, it's found immediately.
- L2 is only used as a warm-start cache for page loads; a missing `box_art_url` just means `resolve_box_art()` falls through to the L1 disk check, which is fast.
- `enrich_system_cache()` runs on startup via `spawn_cache_verification()` and after bulk operations, so L2 catches up on the next app restart at worst.

For immediate L2 coherence after a single on-demand download, the spawned download task could do a targeted DB update:

```rust
// In the on-demand download task, after saving successfully:
if let Some(guard) = state.metadata_db() {
    if let Some(db) = guard.as_ref() {
        let rel_path = format!("boxart/{}.png", matched_stem);
        let _ = db.bulk_update_image_paths(&[(
            system.to_string(),
            rom_filename.to_string(),
            Some(rel_path),
            None,
        )]);
    }
}
```

This is optional -- the lazy convergence via startup enrichment is sufficient for most use cases.

### 11.4 Should thumbnail downloads trigger a full cache enrichment pass?

**No, not for on-demand downloads.** A full `enrich_system_cache()` iterates every ROM in the system and resolves box art for each one. For a 500-ROM system, running this after every single image download is wasteful.

The correct approach by download type:

| Download Type | Cache Update Strategy |
|---|---|
| **Bulk download** (Download All / per-system) | Run `enrich_system_cache()` once after all images for the system are downloaded. Same as current behavior after `import_system_thumbnails()`. |
| **On-demand single download** | Either skip L2 update (let startup enrichment catch it) or do a targeted single-ROM update as shown above. Do NOT run full enrichment. |
| **Background pre-fetch** (Phase 3, future) | Run `enrich_system_cache()` once after the pre-fetch batch completes for a system. |

---

## 12. Data Source Versioning

### Motivation

Currently, users have no visibility into what metadata they have or how fresh it is. The LaunchBox import just shows a count of entries, but not when the data was downloaded or whether a newer version is available. The same problem would apply to the thumbnail index.

### `data_sources` Table (Detailed Design)

The `data_sources` table (defined in section 3) serves as a unified registry of all external data imports. LaunchBox has a single row; each libretro-thumbnails repo has its own row. It's populated/updated as part of each import's completion step.

**LaunchBox integration** (retrofit into existing import):

After `run_import_blocking` completes successfully, upsert into `data_sources`:
```sql
INSERT INTO data_sources (source_name, source_type, version_hash, imported_at, entry_count, branch)
VALUES ('launchbox', 'launchbox', <zip_sha256>, <unix_now>, <stats.inserted>, NULL)
ON CONFLICT(source_name) DO UPDATE SET
    version_hash = excluded.version_hash,
    imported_at = excluded.imported_at,
    entry_count = excluded.entry_count;
```

The `version_hash` for LaunchBox is the SHA-256 of the downloaded Metadata.zip file. This can be computed during the download step (streaming hash) with minimal overhead.

**Libretro thumbnails integration**:

After fetching and inserting entries for each repo, upsert that repo's row:
```sql
INSERT INTO data_sources (source_name, source_type, version_hash, imported_at, entry_count, branch)
VALUES ('libretro:Nintendo_-_NES', 'libretro-thumbnails', <commit_sha>, <unix_now>, <file_count>, 'master')
ON CONFLICT(source_name) DO UPDATE SET
    version_hash = excluded.version_hash,
    imported_at = excluded.imported_at,
    entry_count = excluded.entry_count,
    branch = excluded.branch;
```

Each repo's `version_hash` is its latest commit SHA. There is no need for a composite hash -- aggregate freshness is computed by querying all `libretro-thumbnails` rows.

### UI Display

The `GetDataSourceInfo` server function queries `data_sources` by `source_type`, aggregating the libretro rows. The UI renders them as an info grid:

```
LaunchBox Descriptions & Ratings
  Last updated: March 9, 2026 (3 days ago)
  Entries: 12,000

Thumbnail Index
  Last updated: February 25, 2026 (15 days ago)
  Available thumbnails: 208,121 across 40 systems
```

This section would appear at the top of `/more/metadata` or within each respective section, giving users at-a-glance freshness information.

---

## 13. Storage and Network Budget

### Manifest-only (Phase 1)
- **Disk**: ~4-7 MB for the thumbnail_index in metadata.db (all 40 systems, normalized schema -- see section 3)
- **Network**: ~40 API calls to generate (~5 MB total response data)
- **Time**: ~30-60 seconds to generate (API latency)

### On-demand images (Phase 2)
- **Disk**: only images actually needed. Typical user with 200 ROMs across 10 systems: ~50-200 MB
- **Network**: average image is ~288 KB (measured across 1056 Game Gear boxarts). 200 images = ~56 MB total
- **Latency**: with parallel downloads (concurrency 10-16), the bottleneck is bandwidth, not per-request latency
- **No rate limiting**: `raw.githubusercontent.com` does not rate-limit bulk downloads (tested 1056 files at concurrency 200 with zero failures)
- **No API quota consumed**: raw URLs bypass the GitHub API rate limit entirely
- See section 15 for detailed benchmarks and Pi 4 time estimates

### Compared to current approach
- Current full clone for SNES alone: 3.2 GB download, ~3.2 GB temp disk, ~200 MB final media
- Manifest approach for SNES: 0.5 MB manifest data, ~200 MB final media, zero temp disk overhead

**Savings for a 10-system import**: ~20-30 GB download avoided, ~20-30 GB temp disk avoided.

---

## 14. RePlayOS-Specific Considerations

### Pi 4 Constraints
- **RAM**: 1-4 GB. The manifest fits comfortably in memory if needed, but SQLite avoids that requirement.
- **SD card wear**: Eliminates the massive temp clone write/delete cycle, significantly reducing wear.
- **CPU**: No heavy `git` operations (packfile decompression was the main CPU cost of cloning).
- **Network**: Pi 4 has gigabit ethernet and WiFi. Individual image fetches are small; latency is the bottleneck, not bandwidth.

### USB / SD / NFS Storage
- Manifest lives in `metadata.db` (same NFS `nolock` considerations apply).
- Downloaded images go to `.replay-control/media/` as today -- no change.
- On NFS, concurrent downloads from multiple Pis sharing the same storage would be fine (each Pi fetches its own needed images; duplicates are harmless).

### Offline Operation
- If no network: resolve_box_art falls through to "no thumbnail" for uncached images. Already-downloaded images continue to work.
- The manifest itself is offline-usable once generated. Only the image download step requires network.
- The existing "Re-match All" feature (offline rematch from cloned repos) remains available for users who manually place repos.

### Migration Path
- The new system is purely additive. Existing imported images remain in place.
- The "Download Images" UI replaces the git-clone workflow with manifest-based downloads.
- The legacy git-clone path can be kept as a fallback for users without internet access to GitHub's API (unlikely edge case).

---

## 15. Bulk Download Optimization: Strategy Research

> Tested 2026-03-12 from a fast dev machine (fiber). All timings represent the CDN/protocol behavior; Pi 4 estimates are derived from bandwidth scaling.

Section 9 uses parallel HTTP downloads from `raw.githubusercontent.com`. This section documents the benchmarks and alternative strategies that informed that design decision. A naive sequential approach for 500 ROMs would be 500 HTTP requests at ~0.5s each = ~250 seconds; the parallel approach brings this down to ~23s at concurrency 10.

### 15.1 Strategies Evaluated

#### A. Git blobless clone + per-file checkout

Already documented in section 2A. When checking out specific files from a `--filter=blob:none` clone, git spawns a **separate `git fetch` subprocess for each missing blob**. There is no batching -- each blob is an independent HTTP round-trip to GitHub.

**Measured timings** (Sega Game Gear repo, 1056 boxarts):

| Files checked out | Round-trips | Wall time |
|------------------:|------------:|----------:|
| 5 | 5 | 2.9s |
| 50 | ~50 | 14.0s |
| 500 (extrapolated) | ~500 | ~140s |

Each round-trip takes 0.3-0.6s due to HTTP/TLS handshake + git protocol negotiation. This is **slower than sequential raw URL downloads** because of the per-request git protocol overhead.

**Verdict**: Not viable for bulk downloads. Worse than the baseline in every dimension.

#### B. Git backfill + sparse checkout (git 2.40+)

The `git backfill` command (added in git 2.40, available in git 2.53) fetches missing blobs in **batched packfile requests**, respecting sparse-checkout patterns.

**Test procedure**: `git clone --filter=blob:none --depth=1 --no-checkout --sparse`, then `git sparse-checkout set Named_Boxarts`, then `git backfill --sparse`.

**Measured timings** (Sega Game Gear):

| Scope | Batch requests | Wall time |
|-------|---------------:|----------:|
| 3 individual files (sparse set) | 1 | 0.85s |
| All 1056 Named_Boxarts files | 2 | 21.6s |

After backfill, `git checkout` is instant (blobs already local). The `--min-batch-size` flag controls how many blobs are requested per round-trip (default 50,000 -- effectively "all at once" for our repos).

**Pros**: True batched fetch, minimal round-trips (1-2 for an entire directory). Respects sparse checkout, so you can limit to `Named_Boxarts/` without fetching `Named_Snaps/`, `Named_Titles/`, etc.

**Cons**:
- Requires `git` on the device (Pi 4 Buildroot image may not have it)
- Cannot filter to specific files within `Named_Boxarts/` -- fetches ALL files in the sparse set, including symlinks and images the user does not need
- The `.git/` directory grows with cached blobs (~340 MB for Game Gear boxarts alone)
- Slower than parallel HTTP for the same data volume (21.6s vs 4.9s for 1056 files)
- No fine-grained progress reporting during the packfile download

**Verdict**: Viable as a fallback but inferior to parallel HTTP in both speed and selectivity.

#### C. Parallel HTTP downloads from raw.githubusercontent.com

**The clear winner.** Testing reveals that `raw.githubusercontent.com` is served by Fastly CDN with no observable rate limiting, even at extreme concurrency levels.

**CDN properties** (from response headers):
- CDN: Fastly/Varnish
- Protocol: HTTP/2 (multiplexing supported)
- Cache TTL: 300 seconds (`max-age=300`)
- CORS: `access-control-allow-origin: *`
- No `x-ratelimit-*` headers (unlike the GitHub API)
- No GitHub API quota consumed

**Measured timings** (from dev machine with ~500 Mbps effective throughput):

| Images | Concurrency | Wall time | All HTTP 200? |
|-------:|------------:|----------:|--------------:|
| 20 | 10 | 0.67s | Yes |
| 50 | 10 | 1.80s | Yes |
| 50 | 20 | 0.89s | Yes |
| 50 | 50 | 0.14s | Yes |
| 200 | 10 | 4.85s | Yes |
| 200 | 20 | 2.73s | Yes |
| 200 | 50 | 0.35s | Yes |
| 500 | 20 | 4.57s | Yes |
| 500 | 50 | 2.85s | Yes |
| 1056 | 50 | 4.92s | Yes |
| 1056 | 100 | 1.94s | Yes |
| 1056 | 200 | 1.61s | Yes |

Cross-repo test (500 SNES images, cold CDN cache): 4.07s at concurrency 50, all HTTP 200.

**Average image size**: 288 KB (measured across 1056 Game Gear boxarts; includes both real images and symlink text files at <100 bytes).

**Key observations**:
1. **No rate limiting** at any concurrency level tested (up to 200 concurrent connections, 1056 total requests)
2. **HTTP/2 multiplexing** slightly reduces overhead vs HTTP/1.1 (~10% faster at the same concurrency)
3. At high concurrency (50+), the bottleneck is **bandwidth**, not latency
4. Downloads are **perfectly selective** -- only the exact files needed are fetched
5. Symlink entries return their text content (<100 bytes) instantly; a second request fetches the real target

#### D. GitHub tarball (archive) download

Download the entire repo as a `.tar.gz` via `https://github.com/{owner}/{repo}/archive/refs/heads/{branch}.tar.gz`.

**Measured sizes and timings**:

| Repo | Full clone size | Tarball size | Download time |
|------|----------------:|------------:|----------:|
| Sega Game Gear | 477 MB | 380 MB | 15.2s |
| Nintendo SNES | 3.2 GB | 1.14 GB | 48.3s |

Streaming extraction with a wildcard filter (`tar -xz --wildcards '*/Named_Boxarts/*'`) works but still downloads the **entire** tarball before any extraction can begin (gzip is not seekable).

**Pros**: Single HTTP request. No API quota consumed (the `github.com` direct URL does not require authentication; the API endpoint at `api.github.com/repos/.../tarball` consumes 1 API call and redirects to `codeload.github.com`). No range requests supported.

**Cons**:
- Downloads ALL data (all 4 thumbnail directories), even if only `Named_Boxarts/` is needed
- For selective downloads (user needs 50 out of 1056 images), this fetches 380 MB instead of ~14 MB
- Decompression is CPU-intensive on a Pi 4 (gzip of hundreds of MB)
- Requires ~2x the tarball size in temp disk (compressed + extracted)
- No progress reporting at the per-image level
- No resume capability (must re-download the entire tarball on failure)

**Verdict**: Only viable if downloading nearly ALL images for a system (>80% coverage). For the typical case of 50-200 matched ROMs out of 1000+ available, the tarball wastes 5-20x the bandwidth.

#### E. GitHub Contents API

`GET /repos/{owner}/{repo}/contents/{path}` returns a directory listing with `download_url` for each file.

**Critical limitation**: hard-capped at **1000 entries** per directory. Many `Named_Boxarts/` directories exceed this (Game Gear: 1056, SNES: 3768, NES: far more). The API returns exactly 1000 entries with no pagination -- there is no way to fetch the remaining files.

The Tree API (`GET /repos/{owner}/{repo}/git/trees/{branch}?recursive=1`) does NOT have this limit and is already used for manifest generation (section 6). There is no reason to use the Contents API for directory listings.

**Verdict**: Unsuitable. The 1000-file cap is a hard blocker.

#### F. Pre-packaged release assets

Theoretical approach: publish per-system zip files as GitHub release assets (e.g., `Sega_-_Game_Gear-boxarts.zip`). Users download a single zip per system.

This is a **distribution strategy**, not a fetch strategy -- someone would need to build and publish these archives. The libretro-thumbnails project does not do this. Building it ourselves would require infrastructure (CI to rebuild on changes) and storage (hosting ~50 GB of zips). Not practical.

**Verdict**: Not viable without significant infrastructure investment.

### 15.2 Pi 4 Estimated Download Times

The Pi 4 has Gigabit Ethernet and 802.11ac WiFi. Realistic throughput on home internet:
- Wired: 50-100 Mbps typical (ISP-limited, not Pi-limited)
- WiFi: 30-50 Mbps typical

With the measured average image size of 288 KB:

| Matched ROMs | Data size | @ 30 Mbps | @ 50 Mbps | @ 100 Mbps |
|-------------:|----------:|----------:|----------:|-----------:|
| 10 | 2.9 MB | 0.8s | 0.5s | 0.2s |
| 50 | 14.4 MB | 3.8s | 2.3s | 1.2s |
| 100 | 28.8 MB | 7.7s | 4.6s | 2.3s |
| 200 | 57.6 MB | 15.4s | 9.2s | 4.6s |
| 500 | 144 MB | 38.4s | 23.0s | 11.5s |
| 1000 | 288 MB | 76.8s | 46.1s | 23.0s |

These estimates assume bandwidth-limited downloads with sufficient concurrency (10-16 parallel connections). With sequential downloads, per-request latency (~300-500ms) dominates, making the same workloads 3-10x slower.

**Concurrency recommendation for Pi 4**: Start with 10 concurrent downloads. This provides good throughput without overwhelming the Pi's CPU (each `curl` process or HTTP connection has some overhead). If the Pi is on WiFi, reduce to 6-8 to avoid saturating the wireless link.

### 15.3 Comparison Summary

| Strategy | 50 images | 200 images | 500 images | API calls | Selective? |
|----------|----------:|-----------:|-----------:|----------:|:----------:|
| Sequential curl (baseline) | ~25s | ~100s | ~250s | 0 | Yes |
| **Parallel curl (concurrency 10)** | **~2.3s** | **~9.2s** | **~23s** | **0** | **Yes** |
| Git checkout (blobless) | ~25s | ~100s | ~250s | 0 | Yes |
| Git backfill + sparse | ~5s | ~21s | ~21s* | 0 | No** |
| Full tarball | ~15s | ~15s | ~15s | 0-1 | No |

\*Backfill downloads all files in the sparse set (entire `Named_Boxarts/`), regardless of how many the user needs.
\*\*Sparse checkout can only filter by directory, not individual files.

Times shown are for the Sega Game Gear repo (small system) on a fast connection. For larger repos (SNES, NES) on a Pi 4, multiply tarball and backfill times by 3-5x.

### 15.4 Recommendation

**Use parallel HTTP downloads from `raw.githubusercontent.com` with a semaphore-controlled concurrency of 10-16.**

This is the best approach across all dimensions:

1. **Fastest** for selective downloads (the typical case: 50-200 images needed out of 1000+ available)
2. **Most bandwidth-efficient**: downloads only the exact images matched to the user's ROMs
3. **Zero API quota consumed**: raw URLs bypass GitHub API rate limiting entirely
4. **No rate limiting observed**: Fastly CDN serves unlimited concurrent requests without throttling
5. **No git dependency**: the Pi does not need git installed
6. **Trivially resumable**: skip files that already exist on disk
7. **Fine-grained progress**: each completed download can update the UI immediately
8. **Simple implementation**: `tokio::spawn` + `Semaphore` in Rust, or parallel `curl` processes

The only scenario where the tarball approach might win is downloading an entire system's boxart set (1000+ images, >80% coverage). Even then, the parallel HTTP approach is competitive in time and uses less temp disk. The tarball has no resume capability and requires full re-download on failure.

Git backfill is technically interesting but impractical: it requires git on the device, fetches more data than needed (all files in the sparse set, not just matched ones), and is slower than parallel HTTP for equivalent data volumes.

### 15.5 Implementation notes for section 9

The code in section 9.1 (`download_system_thumbnails`) uses parallel downloads with a `Semaphore`-controlled concurrency. Key design points:

1. **Phase 2 is concurrent**: All download work items are spawned as `tokio::spawn` tasks with a `Semaphore` controlling concurrency (default 10).

2. **Progress reporting**: `AtomicUsize` counters are incremented by each completed task. The progress callback reads the counters as handles are awaited.

3. **Error handling**: Failed downloads do not block other downloads. Errors are counted and reported at the end.

4. **Cancel support**: The `AtomicBool` cancel flag is checked by each task before starting its download. When set, no new downloads start, but in-flight downloads complete normally.

5. **Symlink optimization**: When the manifest indicates a file is a symlink, skip the first HTTP request and go directly to the resolved target URL. This saves one round-trip per symlink file (~40-50% of entries are symlinks in typical repos).

---

## 16. Summary of Recommendations

1. **Short term**: Add the `data_sources` table and retrofit it into the existing LaunchBox import. This is a small, self-contained change that immediately adds value (freshness visibility) and establishes the pattern for future data sources.

2. **Medium term**: Implement the thumbnail manifest (Phase 1 + 2). Add the `thumbnail_index` table (per-repo entries tracked via `data_sources`). Build the "Update Thumbnail Index" server function and UI on the `/more/metadata` page, mirroring the LaunchBox download pattern. Replace the git-clone image download with manifest-based on-demand fetch. **Use parallel downloads (concurrency 10-16)** -- see section 15.

3. **Long term**: Add background pre-fetch (Phase 3) so that browsing a system gradually fills in all thumbnails without explicit user action.

4. **Nice to have**: Index and fetch `Named_Titles/` from libretro-thumbnails repos. Title screens could be shown alongside boxart/snaps in a game detail view or as an alternative thumbnail style. The schema already supports this — just add `"Named_Titles"` as a third `kind` value in `thumbnail_index`. Download budget increases ~50% (three image types instead of two per matched ROM).

The blobless partial clone approach (option A) is technically elegant but adds complexity (git dependency, growing .git directory, batch fetch unpredictability). The manifest + raw URL approach is simpler, more predictable, and better suited to the Pi's constraints. Parallel HTTP downloads from the Fastly CDN are fast, unthrottled, and consume zero GitHub API quota.
