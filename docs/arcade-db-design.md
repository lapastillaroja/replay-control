# Arcade Game Metadata Database Design

## Problem

Arcade ROM files are stored as zip archives with short, cryptic filenames (e.g., `mslug6.zip`, `sf2.zip`, `ffight.zip`). RePlayOS internally maps these to human-readable names using its own database built from FBNeo/MAME DAT files plus arcadeitalia.net, but that database is not accessible to us. Replay Control currently displays these raw filenames, which is a poor user experience.

We need an embedded database that maps arcade ROM zip names to display names and metadata, covering the four arcade system folders: `arcade_fbneo`, `arcade_mame`, `arcade_mame_2k3p`, and `arcade_dc`.

## RePlayOS Arcade Core Versions

Source: [RePlayOS changelog](https://www.replayos.com/changelog/) and [systems page](https://www.replayos.com/systems/).

As of **RePlayOS v1.4.0** (latest public release):

| System Folder      | Display Name               | Libretro Core        | Core/ROM Set Version     | RPi 3/Zero 2 | RPi 4 | RPi 5 |
|--------------------|----------------------------|----------------------|--------------------------|---------------|-------|-------|
| `arcade_fbneo`     | Arcade (FBNeo)             | fbneo_libretro       | Current FBNeo (latest)   | Partial       | Yes   | Yes   |
| `arcade_mame`      | Arcade (MAME)              | mamearcade_libretro  | MAME 0.284+              | No            | Partial | Yes |
| `arcade_mame_2k3p` | Arcade (MAME 2K3+)        | mame2003_plus_libretro | MAME 0.78 (backported) | Partial       | Partial | No  |
| `arcade_dc`        | Arcade SEGA Naomi/Atomis   | flycast_libretro     | Flycast v2.4+            | No            | Partial | Yes |

### Version History (from changelog)

- **v1.4.0**: Internal arcade database updated to **MAME 0.285**
- **v1.2.0**: Updated FBNeo and MAME mainline arcade cores (**MAME 0.284**); internal arcade DB to MAME 0.284
- **v0.52.0**: Internal arcade naming database based on **MAME 0.276**
- **v0.50.0**: Updated MAME core to **v0.275**, Flycast core to **v2.4**

### Key Notes

- RePlayOS generates its internal arcade naming database using a **Python utility** that parses **FBNeo + MAME full Arcade DAT** files (added in v0.49.0, expanded in v0.49.17).
- The internal arcade DB is **not accessible** to external apps — it's compiled into the `replay` C binary.
- `arcade_mame_2k3p` is **not supported on RPi 5** (the core is too old / RPi 5 uses newer cores).
- `arcade_dc` uses Flycast for Naomi/Atomiswave boards (SEGA arcade hardware based on Dreamcast).
- FBNeo uses MVS BIOS and SNK NEO-GEO AES BIOS (changed in v1.2.0).
- Naomi/Atomiswave requires `naomi.zip` and `naomi2.zip` BIOS files in the `dc/` BIOS subfolder.
- ROM format for all arcade systems: **zip**.

## Data Sources Evaluation

### 1. MAME -listxml XML (Current MAME — RePlayOS uses v0.284+, DB up to v0.285)

**What it is:** The canonical, machine-readable output from `mame -listxml`. An XML file (~285 MB uncompressed) containing every machine MAME supports (~49,000+ entries including non-arcade devices, BIOS, mechanical).

**Fields available per machine:**
- `name` (ROM zip name), `cloneof`, `romof`, `isbios`, `isdevice`, `ismechanical`, `runnable`
- `description` (display name), `year`, `manufacturer`
- `display` element: `type` (raster/vector/lcd/svg), `rotate` (0/90/180/270)
- `input` element: `players`, `buttons`, `coins`, `control` (joy8way, trackball, etc.)
- `driver` element: `status` (good/imperfect/preliminary), `emulation`, `color`, `sound`

**Pros:**
- Authoritative and comprehensive -- the single source of truth for MAME
- Includes orientation, player count, driver status, and screen type natively
- Actively maintained with monthly releases
- Includes parent/clone relationships

**Cons:**
- Enormous file size (~285 MB XML, ~20 MB compressed)
- Requires filtering out ~35,000 non-arcade entries (devices, BIOS, software lists, mechanical)
- Version-specific: ROM names can change between MAME versions
- Cannot easily be downloaded without running a MAME binary or using third-party mirrors
- RePlayOS currently uses MAME 0.284+ core with internal DB up to 0.285. We should match this version.

**Best source for:** Current MAME (`arcade_mame`) entries. Download MAME 0.285 DAT from progettosnaps.net mirrors.

### 2. MAME 2003-Plus XML (libretro)

**What it is:** The XML DAT file for the MAME 2003+ libretro core, hosted on GitHub at `libretro/mame2003-plus-libretro`. Based on MAME 0.78 with backported game support.

**URL:** `https://raw.githubusercontent.com/libretro/mame2003-plus-libretro/master/metadata/mame2003-plus.xml`

**Stats:** 5,256 game entries (2,917 parents + 2,339 clones), ~22 MB XML

**Fields available:** Same structure as MAME listxml but older DTD variant:
- `name`, `cloneof`, `romof`, `runnable`
- `description`, `year`, `manufacturer`
- `video`: `screen` (raster/vector), `orientation` (horizontal/vertical), `width`, `height`
- `input`: `players`, `buttons`, `coins`, `control`
- `driver`: `status` (good/preliminary/protection)

**Pros:**
- Directly matches the `arcade_mame_2k3p` folder on RePlayOS
- Every entry has video, input, and driver elements (100% coverage)
- Hosted on GitHub -- easy to download in build scripts
- Manageable size
- Also has `catver.ini` at `libretro/mame2003-plus-libretro/master/metadata/catver.ini`

**Cons:**
- Based on old MAME (0.78) -- doesn't cover newer games
- Only relevant for the MAME 2003+ core

**Best source for:** `arcade_mame_2k3p` entries.

### 3. FBNeo DAT File (libretro)

**What it is:** ClrMame Pro XML DAT files for FinalBurn Neo, hosted on GitHub at `libretro/FBNeo`.

**URL:** `https://raw.githubusercontent.com/libretro/FBNeo/master/dats/FinalBurn%20Neo%20(ClrMame%20Pro%20XML%2C%20Arcade%20only).dat`

**Stats:** 8,095 arcade game entries (2,538 parents + 5,557 clones), ~13 MB XML

**Fields available per game:**
- `name`, `cloneof`, `romof`
- `description` (display name), `year`, `manufacturer`
- ROM file listings (name, size, crc)

**Does NOT include:** orientation, player count, screen type, driver status, category/genre.

**Pros:**
- Directly matches the `arcade_fbneo` folder on RePlayOS
- Hosted on GitHub -- reliable, version-controlled
- Arcade-only file available (no filtering needed)
- Game names and manufacturer data are accurate

**Cons:**
- Missing orientation, player count, and driver status
- These fields would need to be sourced from MAME data or nplayers.ini separately

**Best source for:** `arcade_fbneo` entries (name, year, manufacturer, parent/clone).

**Also available:** `gamelist.txt` at `https://raw.githubusercontent.com/libretro/FBNeo/master/gamelist.txt` -- a plain-text table with all 24,527 entries (arcade + non-arcade) including name, status, full name, parent, year, company, hardware, and remarks. This is parseable but includes non-arcade entries that need filtering.

### 4. Progetto-SNAPS catver.ini / Genre Files

**What it is:** Community-maintained category/genre classification files for MAME games.

**URL:** `https://www.progettosnaps.net/catver/`
**Also at:** `https://raw.githubusercontent.com/libretro/mame2003-plus-libretro/master/metadata/catver.ini`

**Format:** Simple INI file. `[Category]` section with `romname=Category / Subcategory` entries.

**Example categories:** `Shooter / Flying Vertical`, `Sports / Baseball`, `Fighter / 2D`, `Puzzle / Drop`, `Platform / Run Jump`.

**Pros:**
- Clean, parseable format
- Widely used in frontends (MAME, Attract-Mode, LaunchBox)
- Provides genre/category data missing from DAT files

**Cons:**
- Separate file that must be joined with the main data
- May not cover all FBNeo-specific entries

**Best source for:** Category/genre classification.

### 5. nplayers.ini

**What it is:** Community-maintained file mapping ROM names to player count information.

**URL:** `https://nplayers.arcadebelgium.be/`

**Pros:** Detailed player count info (simultaneous vs. alternating). Covers MAME games.

**Cons:** Redundant -- MAME listxml and MAME 2003+ XML already include player counts in `<input>` elements. Only useful to supplement FBNeo data.

### 6. Arcade Database (adb.arcadeitalia.net)

**What it is:** A web-based arcade database with game information, screenshots, and videos. Updated to MAME 0.284. Supports CSV/XML export.

**URL:** `https://adb.arcadeitalia.net/`

**Pros:**
- Rich metadata including genre, players, orientation
- Export in CSV format
- 49,538 entries

**Cons:**
- Web scraping / manual export required (no stable API for bulk download)
- Licensing unclear for redistribution
- Relies on being online during build
- Overkill for our needs when we already have the DAT files

**Not recommended** as a primary source. Useful as a reference.

## Recommended Approach

### Strategy: Multiple source files, unified at build time

Use the **version-matched DAT/XML files** from the libretro GitHub repos as primary sources, since they directly correspond to the emulator cores running on RePlayOS:

| RePlayOS Folder    | Emulator Core           | ROM Set Version       | Primary Data Source                     |
|--------------------|-------------------------|-----------------------|-----------------------------------------|
| `arcade_fbneo`     | fbneo_libretro          | Current FBNeo         | FBNeo Arcade-only DAT (GitHub)          |
| `arcade_mame`      | mamearcade_libretro     | MAME 0.284+           | MAME 0.285 listxml (progettosnaps)      |
| `arcade_mame_2k3p` | mame2003_plus_libretro  | MAME 0.78 (backported)| MAME 2003+ XML (GitHub)                 |
| `arcade_dc`        | flycast_libretro        | Flycast v2.4+         | Hardcoded list (small, ~50 games)       |

Supplement with `catver.ini` for genre/category data.

For the initial implementation, **start with FBNeo + MAME 2003+ only**. These are fully available on GitHub, cover the two most commonly used arcade cores, and between them represent ~10,000 unique arcade games. MAME current can be added later.

### Why not a single unified list?

ROM names are not globally unique across emulators. The same zip name may exist in both FBNeo and MAME with different ROM contents. However, ROM names to display-name mappings are almost always identical across emulators (e.g., `sf2` is always "Street Fighter II" regardless of the emulator). The build process should deduplicate by ROM name, preferring the richer metadata source (MAME 2003+ has orientation/players, FBNeo does not).

## Schema

### Minimal viable schema

```
ArcadeGame {
    rom_name:      String   // Primary key. Zip filename without extension. e.g., "sf2"
    display_name:  String   // Human-readable. e.g., "Street Fighter II - The World Warrior"
    year:          String   // Release year. e.g., "1991" (string because some are "198?" or empty)
    manufacturer:  String   // e.g., "Capcom"
    players:       u8       // Max simultaneous players. 0 = unknown
    rotation:      u8       // 0 = horizontal, 1 = vertical, 2 = unknown
    status:        u8       // 0 = working, 1 = imperfect, 2 = preliminary/broken, 3 = unknown
    is_clone:      bool     // Whether this is a clone of another game
    parent:        String   // Parent ROM name if is_clone, empty otherwise
    category:      String   // Genre from catver.ini. e.g., "Shooter / Flying Vertical"
}
```

### Fields rationale

| Field          | Why                                                                    |
|----------------|------------------------------------------------------------------------|
| `rom_name`     | Lookup key. Must match what's on disk.                                 |
| `display_name` | The whole point -- show "Metal Slug 6" instead of "mslug6"            |
| `year`         | Useful for sorting/filtering in the UI                                 |
| `manufacturer` | Useful for browsing/filtering                                          |
| `players`      | RePlayOS config has `view_players` filter -- we should support this    |
| `rotation`     | RePlayOS config has `view_rotation` filter -- we should support this   |
| `status`       | Hide/warn about non-working games                                      |
| `is_clone`     | Allow hiding clones to show only unique games                          |
| `parent`       | Navigate from clone to parent                                          |
| `category`     | Genre filtering/display                                                |

### Deliberately omitted

- **ROM file checksums/sizes:** Not our job (ROM managers handle this)
- **Control type:** Nice-to-have but not essential for display
- **Screen resolution:** Not useful for Replay Control
- **BIOS requirements:** Not our concern

## Build Pipeline

### Overview

```
Source Files (downloaded)      Build Script (build.rs)           Compiled Binary
────────────────────────       ─────────────────────────         ─────────────────
FBNeo arcade DAT ──────┐
                        ├──> Parse XML ──> Normalize ──> PHF codegen ──> include!()
MAME 2003+ XML ────────┤                                    │
                        │                                    │
MAME 0.285 compact XML ┤                                    │
                        │                                    │
catver.ini (2003+) ─────┤                                    │
                        │                                    │
catver.ini (current) ───┘                                    │
                                                             ▼
                                                  arcade_db.rs (~2.2 MB)
```

### Step 1: Download sources

Source files live in `data/` at the project root (gitignored -- not checked into the repo). A download script fetches them from GitHub:

```sh
./scripts/download-arcade-data.sh
```

This creates and populates:

```
data/
├── README.md                 # Checked into git (explains the folder)
├── fbneo-arcade.dat          # FBNeo ClrMame Pro XML, Arcade only
├── mame2003plus.xml          # MAME 2003+ listxml
├── mame0285-arcade.xml       # MAME 0.285 compact arcade XML (preprocessed)
├── catver.ini                # Category/version file (MAME 2003+)
└── catver-mame-current.ini   # Category/version file (current MAME)
```

The script downloads from the libretro GitHub repos (see URLs in "Source Data URLs" below), is idempotent, and safe to re-run. Developers run it once after cloning, and again when they want to refresh the data.

**Why not checked into git?** The source files total ~35 MB and come from upstream repos. Keeping them out of git avoids bloating the repository. The download script makes setup a single command.

**Why not downloaded at build time?** Build reproducibility, offline builds, CI reliability. The `build.rs` gracefully handles missing source files (it only processes files that exist), so the build succeeds even without running the download script -- you just get fewer entries in the arcade DB.

Note: `replay-core/data/arcade/flycast_games.csv` (hand-curated, small) remains checked into git separately from the downloaded sources.

### Step 2: Parse and merge (build.rs)

The `build.rs` script:

1. Parses Flycast CSV -- hand-curated Naomi/Atomiswave entries (always preserved)
2. Parses FBNeo DAT (ClrMame Pro XML format) -- extracts name, description, year, manufacturer, cloneof
3. Parses MAME 2003+ XML -- extracts all FBNeo fields plus orientation, players, driver status
4. Parses MAME 0.285 compact XML -- richest/most up-to-date data, overrides FBNeo and MAME 2003+ entries
5. Parses catver.ini (MAME 2003+) -- overlays category data on entries
6. Parses catver-mame-current.ini -- overlays categories on remaining entries
7. Generates a PHF map via `phf_codegen`

Merge strategy:
- Flycast entries are always preserved (hand-curated, tracked by rom name)
- FBNeo entries fill gaps (only insert if rom_name is new)
- MAME 2003+ overrides FBNeo-sourced entries (which lack players/rotation/status)
- MAME current overrides FBNeo and MAME 2003+ entries (more accurate/updated metadata)
- catver.ini categories are overlaid on entries that lack a category

### Step 3: Binary format

Two options, in order of recommendation:

#### Option A: Generated Rust code with `phf_codegen` (recommended)

Use `phf_codegen` in `build.rs` to generate a perfect hash map. The build script writes a `.rs` file that is `include!`'d into the library.

```rust
// Generated by build.rs
static ARCADE_DB: phf::Map<&'static str, ArcadeGameEntry> = phf::Map { ... };
```

**Pros:** Zero-cost lookup at runtime (O(1), no parsing), type-safe, no deserialization step.
**Cons:** Increases compile time slightly. At ~10,000 entries, PHF generation takes <1 second.

#### Option B: Embedded binary blob with `include_bytes!`

Serialize the database to a custom compact binary format, embed with `include_bytes!`, and deserialize on first access (lazy_static or OnceLock).

Binary format:
```
Header:
  magic: [u8; 4]        = b"ARDB"
  version: u16           = 1
  entry_count: u32
  string_pool_offset: u32

Entry table (fixed-size records):
  rom_name_offset: u32       // into string pool
  rom_name_len: u16
  display_name_offset: u32
  display_name_len: u16
  year_offset: u32
  year_len: u8
  manufacturer_offset: u32
  manufacturer_len: u8
  parent_offset: u32
  parent_len: u8
  category_offset: u32
  category_len: u8
  players: u8
  rotation: u8
  status: u8
  flags: u8              // bit 0: is_clone

String pool:
  Concatenated UTF-8 strings, referenced by offset+length
```

**Pros:** Very compact, fast to parse.
**Cons:** Requires a deserialization step at startup, custom binary format to maintain.

#### Recommendation: Option A (phf_codegen)

For ~10,000 entries, PHF is the right choice. It trades ~800 KB of binary size for zero runtime overhead. The compiled binary runs on a Raspberry Pi where we want minimal startup latency. There is no deserialization, no allocation, no parsing -- just a static hash table baked into the binary.

### Step 4: Compile-time embedding

The generated code lives at `replay-core/src/arcade_db_generated.rs` (gitignored) and is included via:

```rust
// In replay-core/src/arcade_db.rs
include!(concat!(env!("OUT_DIR"), "/arcade_db.rs"));
```

## Integration Plan

### API surface (replay-core)

```rust
// replay-core/src/arcade_db.rs

/// Metadata for an arcade game ROM.
pub struct ArcadeGameInfo {
    pub display_name: &'static str,
    pub year: &'static str,
    pub manufacturer: &'static str,
    pub players: u8,
    pub rotation: Rotation,
    pub status: DriverStatus,
    pub is_clone: bool,
    pub parent: &'static str,
    pub category: &'static str,
}

pub enum Rotation { Horizontal, Vertical, Unknown }
pub enum DriverStatus { Working, Imperfect, Preliminary, Unknown }

/// Look up arcade game metadata by ROM filename (without extension).
pub fn lookup_arcade_game(rom_name: &str) -> Option<&'static ArcadeGameInfo> {
    ARCADE_DB.get(rom_name)
}

/// Get display name for a ROM, falling back to the filename.
pub fn arcade_display_name(filename: &str) -> &str {
    let rom_name = filename.strip_suffix(".zip").unwrap_or(filename);
    lookup_arcade_game(rom_name)
        .map(|info| info.display_name)
        .unwrap_or(filename)
}
```

### Integration with ROM listing

In `replay-core/src/roms.rs`, the `RomEntry` struct currently has a `filename` field. Two options:

**Option 1 (minimal change):** Add a `display_name` field to `RomEntry`, populated during `collect_roms_recursive` for arcade systems:

```rust
pub struct RomEntry {
    // ... existing fields ...
    /// Human-readable display name (resolved from arcade DB for arcade systems)
    pub display_name: Option<String>,
}
```

When building a `RomEntry` for an arcade system (`SystemCategory::Arcade`), call `arcade_display_name()` and store the result.

**Option 2 (lazy resolution):** Don't change `RomEntry`. Instead, resolve display names in the UI layer (replay-app) when rendering the game list. This keeps replay-core agnostic about display concerns.

**Recommendation:** Option 1. The display name is a property of the ROM, not a UI concern. It should be available to any consumer of replay-core.

### Sorting with display names

Currently ROMs sort by filename. With display names, sort by display name instead:

```rust
roms.sort_by(|a, b| {
    let a_name = a.display_name.as_deref().unwrap_or(&a.filename);
    let b_name = b.display_name.as_deref().unwrap_or(&b.filename);
    a_name.to_lowercase().cmp(&b_name.to_lowercase())
});
```

## Size Estimates

| Component                       | Entries | Est. Binary Size |
|---------------------------------|---------|------------------|
| FBNeo arcade games              | 8,108   | ~500 KB          |
| MAME 2003+ games                | 5,272   | ~350 KB          |
| MAME current (0.285) arcade     | 26,777  | ~1.5 MB          |
| Combined (deduplicated)         | 28,593  | ~2.0 MB          |
| With PHF overhead               | 28,593  | ~2.2 MB          |

These are conservative estimates. The actual binary impact includes the PHF hash displacement table (~2-3 bytes per entry) plus all string data. At ~800 KB for the initial version, this is well within acceptable limits for a Raspberry Pi binary.

For comparison, a single arcade ROM zip is typically 1-50 MB. The entire metadata DB is smaller than most individual ROMs.

## Source Data URLs

### Primary (downloaded via `./scripts/download-arcade-data.sh`)

| File | URL | Update Frequency |
|------|-----|------------------|
| FBNeo Arcade DAT | `https://raw.githubusercontent.com/libretro/FBNeo/master/dats/FinalBurn%20Neo%20(ClrMame%20Pro%20XML%2C%20Arcade%20only).dat` | ~Monthly |
| MAME 2003+ XML | `https://raw.githubusercontent.com/libretro/mame2003-plus-libretro/master/metadata/mame2003-plus.xml` | Rarely |
| MAME 2003+ catver.ini | `https://raw.githubusercontent.com/libretro/mame2003-plus-libretro/master/metadata/catver.ini` | Rarely |
| MAME 0.285 DAT pack | `https://www.progettosnaps.net/download/?tipo=dat_mame&file=/dats/MAME/packs/MAME_Dats_285.7z` | Per MAME release |
| catver.ini (current MAME) | `https://raw.githubusercontent.com/AntoPISA/MAME_SupportFiles/refs/heads/main/catver.ini/catver.ini` | ~Monthly |

### Future / supplementary

| File | URL | Notes |
|------|-----|-------|
| FBNeo gamelist.txt | `https://raw.githubusercontent.com/libretro/FBNeo/master/gamelist.txt` | Plain-text, all platforms, includes hardware column |
| MAME current DATs | `https://www.progettosnaps.net/dats/MAME/` | Mirrors of MAME listxml, updated per release |
| catver.ini (latest) | `https://www.progettosnaps.net/catver/` | Category files for latest MAME |
| nplayers.ini | `https://nplayers.arcadebelgium.be/` | Detailed player count info |
| Arcade Italia | `https://adb.arcadeitalia.net/download.php` | CSV/XML export, manual download |
| MAME Support Files (GitHub) | `https://github.com/AntoPISA/MAME_SupportFiles` | Category, catver, series, etc. |

## Implementation Task List

### Phase 0: Flycast / Naomi / Atomiswave (DONE)

**Completed.** The `arcade_dc` system is implemented as the first arcade DB, proving the build pipeline works.

- **Data source:** Game list extracted from Flycast source code (`naomi_roms.cpp`) cross-referenced with MAME driver files (`naomi.cpp`, `dc_atomiswave.cpp`, `segasp.cpp`) for year/manufacturer metadata.
- **Data file:** `replay-core/data/arcade/flycast_games.csv` -- 301 entries (212 parents + 89 clones) covering Naomi, Naomi 2, Naomi GD-ROM, Atomiswave, and System SP platforms.
- **Build pipeline:** `replay-core/build.rs` parses the CSV and generates a `phf::Map` via `phf_codegen`. The generated code is `include!`'d into `replay-core/src/arcade_db.rs`.
- **Public API:** `lookup_arcade_game(rom_name)` and `arcade_display_name(filename)` in `replay_core::arcade_db`.
- **Tests:** 8 unit tests covering lookups, clones, rotation, display name fallback.
- **Dependencies:** `phf` (runtime), `phf_codegen` + `csv` (build).

### Phase 1: Core database (FBNeo + MAME 2003+) (DONE)

**Completed.** The arcade DB now includes FBNeo and MAME 2003+ data, expanding from 301 to 10,375 unique entries.

1. **Source data files** -- downloaded via `./scripts/download-arcade-data.sh` into `data/` at the project root (gitignored):
   - `fbneo-arcade.dat` -- FBNeo ClrMame Pro XML, arcade only (8,108 entries, ~13 MB)
   - `mame2003plus.xml` -- MAME 2003+ XML (5,272 entries, ~22 MB)
   - `catver.ini` -- category/genre mappings (5,258 entries)

2. **Extended `replay-core/build.rs`**:
   - Added `quick-xml = "0.37"` to `[build-dependencies]` for streaming SAX XML parsing
   - `parse_fbneo_dat()` -- streaming parser for FBNeo ClrMame Pro XML; extracts name, description, year, manufacturer, cloneof
   - `parse_mame2003plus_xml()` -- streaming parser for MAME 2003+ XML; extracts all FBNeo fields plus video orientation, input players, driver status
   - `parse_catver_ini()` -- INI parser for `[Category]` section; builds rom_name-to-category mapping
   - Merge strategy: Flycast CSV loaded first (hand-curated, preserved), then FBNeo (fills gaps), then MAME 2003+ (overwrites entries lacking players/rotation/status), then catver.ini category overlay
   - Build emits `cargo:warning` counts per source for visibility
   - All source files tracked via `cargo::rerun-if-changed`

3. **Integrate with ROM listing** (DONE -- completed as part of Phase 0 integration)
   - `display_name: Option<String>` added to `RomEntry`, `RecentEntry`, and `Favorite`
   - `collect_roms_recursive` calls `arcade_display_name()` for arcade systems
   - Sorting uses display name when available
   - Search matches on both filename and display name
   - Home page, favorites page, and game detail page all show display names

4. **Testing** -- 31 tests total (5 new):
   - `lookup_sf2_from_mame` -- verifies MAME 2003+ metadata (players, rotation, status, category)
   - `lookup_pacman_clone` -- verifies clone/parent relationship and category overlay
   - `lookup_dkong_vertical` -- verifies vertical rotation detection
   - `lookup_fbneo_only_game` -- verifies FBNeo-only entry with unknown rotation/status
   - `total_entry_count` -- asserts 10,000+ unique entries after deduplication
   - All 26 existing tests continue to pass

### Phase 2: Enhanced metadata usage (future)

5. Add filtering by category, players, orientation in the UI
6. Show metadata in game detail view (year, manufacturer, category)
7. Add "hide clones" toggle
8. Add "hide non-working" toggle

### Phase 3: Current MAME support (DONE)

**Completed.** The arcade DB now includes MAME 0.285 data, expanding from 10,375 to 28,593 unique entries.

1. **Data source:** The full MAME 0.285 listxml (~285 MB XML) is downloaded from Progetto-SNAPS as a 7z archive (~40 MB), then preprocessed by `scripts/extract-mame-arcade.py` into a compact ~3.6 MB XML file containing only arcade entries with the metadata fields we need (name, description, year, manufacturer, cloneof, rotation, players, driver status). Non-arcade entries (BIOS, devices, mechanical, non-runnable) are filtered out during preprocessing.

2. **Category data:** `catver-mame-current.ini` from the MAME_SupportFiles GitHub repo (v0.274, 47,853 mappings) supplements the MAME 2003+ catver.ini with categories for newer games.

3. **Data files** (downloaded via `./scripts/download-arcade-data.sh` into `data/`, gitignored):
   - `mame0285-arcade.xml` -- preprocessed MAME 0.285 arcade entries (26,777 entries, ~3.6 MB)
   - `catver-mame-current.ini` -- category/genre mappings for current MAME (47,853 entries, ~2.3 MB)

4. **Preprocessing pipeline** (in download script, requires `7z` and `python3`):
   - Download `MAME_Dats_285.7z` from Progetto-SNAPS
   - Extract full listxml from archive
   - Run `scripts/extract-mame-arcade.py` to produce compact XML
   - Clean up large temporary files (7z archive and full XML are not kept)

5. **Extended `replay-core/build.rs`**:
   - Added `parse_mame_current_xml()` -- streaming parser for the compact format (`<m>` elements with `<d>`, `<y>`, `<f>` children)
   - Merge step 4: MAME current entries override FBNeo and MAME 2003+ entries (which may have outdated metadata), but preserve Flycast hand-curated entries
   - Merge step 6: catver-mame-current.ini overlays categories on entries that still lack them after the MAME 2003+ catver overlay
   - Build stats: 26,777 MAME current entries loaded, 18,218 new + 8,258 overrides, 21,078 additional category overlays

6. **Testing** -- 35 tests total (4 new):
   - `lookup_mame_current_only_game` -- verifies Time Crisis metadata (MAME current only, not in FBNeo/MAME 2003+)
   - `lookup_mame_current_overrides_mame2003` -- verifies MAME current overrides FBNeo entries with rotation/status data
   - `lookup_mame_current_preserves_flycast` -- verifies Flycast hand-curated entries are preserved
   - `mame_current_category_overlay` -- verifies category applied from current MAME catver.ini
   - Updated `lookup_fbneo_only_game` to use `3countba` (a game truly only in FBNeo)
   - Updated `lookup_sf2_from_mame` to expect MAME current description format
   - Updated `total_entry_count` threshold from 10,000 to 25,000

## Open Questions

1. ~~**Should we ship the source data files in git?**~~ **Resolved:** No. Source files (~35 MB) live in `data/` at the project root, which is gitignored. A download script (`./scripts/download-arcade-data.sh`) fetches them. This keeps the repo small while making setup a single command. `build.rs` gracefully handles missing files.

2. **Version pinning:** ~~Should we pin to a specific FBNeo/MAME version?~~ **Resolved:** Yes -- pin to MAME 0.285 (matching RePlayOS v1.4.0 internal DB). Use current FBNeo DAT from GitHub (RePlayOS tracks latest). Update when new RePlayOS versions ship with updated cores.

3. ~~**Atomiswave/Naomi (`arcade_dc`):** This is a small set (~50 games) running on the Flycast core (v2.4+). Requires `naomi.zip` and `naomi2.zip` BIOS. We could hardcode these or find a Flycast-specific game list. Lower priority.~~ **Resolved:** Implemented in Phase 0 with 197 entries sourced from Flycast + MAME. Covers Naomi, Naomi 2, Atomiswave, and System SP.
