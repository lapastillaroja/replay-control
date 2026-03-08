# Arcade Game Metadata Database Design

## Problem

Arcade ROM files are stored as zip archives with short, cryptic filenames (e.g., `mslug6.zip`, `sf2.zip`, `ffight.zip`). RePlayOS internally maps these to human-readable names using its own database built from FBNeo/MAME DAT files plus arcadeitalia.net, but that database is not accessible to us. The Replay companion app currently displays these raw filenames, which is a poor user experience.

We need an embedded database that maps arcade ROM zip names to display names and metadata, covering the four arcade system folders: `arcade_fbneo`, `arcade_mame`, `arcade_mame_2k3p`, and `arcade_dc`.

## Data Sources Evaluation

### 1. MAME -listxml XML (Current MAME, v0.286+)

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
- RePlayOS uses a specific MAME version (lr-mame), not necessarily the latest

**Best source for:** Current MAME (`arcade_mame`) entries. Download from progettosnaps.net mirrors.

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

| RePlayOS Folder    | Emulator Core           | Primary Data Source                     |
|--------------------|-------------------------|-----------------------------------------|
| `arcade_fbneo`     | fbneo_libretro.so       | FBNeo Arcade-only DAT (GitHub)          |
| `arcade_mame`      | mamearcade_libretro.so  | Current MAME listxml (progettosnaps)    |
| `arcade_mame_2k3p` | mame2003_plus_libretro  | MAME 2003+ XML (GitHub)                 |
| `arcade_dc`        | flycast_libretro.so     | Hardcoded list (small, ~50 games)       |

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
- **Screen resolution:** Not useful for a web companion app
- **BIOS requirements:** Not our concern

## Build Pipeline

### Overview

```
Source Files (GitHub)          Build Script (build.rs)           Compiled Binary
────────────────────          ─────────────────────────         ─────────────────
FBNeo arcade DAT ──┐
                    ├──> Parse XML ──> Normalize ──> Serialize ──> include_bytes!()
MAME 2003+ XML ────┤                                    │
                    │                                    │
catver.ini ─────────┘                                    │
                                                         ▼
                                              arcade_db.bin (~500 KB)
```

### Step 1: Download sources (offline-first)

Keep cached copies of the source files in the repository under `replay-core/data/sources/`:

```
replay-core/data/sources/
├── fbneo-arcade.dat          # FBNeo ClrMame Pro XML, Arcade only
├── mame2003plus.xml          # MAME 2003+ listxml
├── catver.ini                # Category/version file
└── UPDATE.md                 # Instructions + URLs for refreshing
```

These files are checked into git. A developer updates them manually when a new emulator version ships. This avoids network dependencies during build.

**Why not download at build time?** Build reproducibility, offline builds, CI reliability. The source files change infrequently (emulator core updates are rare on RePlayOS).

### Step 2: Parse and merge (build.rs)

The `build.rs` script:

1. Parses FBNeo DAT (ClrMame Pro XML format) -- extracts name, description, year, manufacturer, cloneof
2. Parses MAME 2003+ XML -- extracts name, description, year, manufacturer, cloneof, orientation, players, driver status
3. Parses catver.ini -- builds a `HashMap<rom_name, category>`
4. Merges into a unified map keyed by `rom_name`:
   - If a game exists in both, prefer MAME 2003+ metadata (has orientation/players/status)
   - Always overlay category from catver.ini
   - FBNeo-only games get `players=0, rotation=unknown, status=unknown`
5. Serializes to a compact binary format

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
| FBNeo arcade games              | 8,095   | ~500 KB          |
| MAME 2003+ games                | 5,256   | ~350 KB          |
| Combined (deduplicated)         | ~10,000 | ~700 KB          |
| With PHF overhead               | ~10,000 | ~800 KB          |
| Future: add current MAME arcade | ~15,000 | ~1.2 MB          |

These are conservative estimates. The actual binary impact includes the PHF hash displacement table (~2-3 bytes per entry) plus all string data. At ~800 KB for the initial version, this is well within acceptable limits for a Raspberry Pi binary.

For comparison, a single arcade ROM zip is typically 1-50 MB. The entire metadata DB is smaller than most individual ROMs.

## Source Data URLs

### Primary (checked into repo)

| File | URL | Update Frequency |
|------|-----|------------------|
| FBNeo Arcade DAT | `https://raw.githubusercontent.com/libretro/FBNeo/master/dats/FinalBurn%20Neo%20(ClrMame%20Pro%20XML%2C%20Arcade%20only).dat` | ~Monthly |
| MAME 2003+ XML | `https://raw.githubusercontent.com/libretro/mame2003-plus-libretro/master/metadata/mame2003-plus.xml` | Rarely |
| MAME 2003+ catver.ini | `https://raw.githubusercontent.com/libretro/mame2003-plus-libretro/master/metadata/catver.ini` | Rarely |

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

### Phase 1: Core database (FBNeo + MAME 2003+)

1. **Download and commit source data files**
   - Fetch FBNeo arcade-only DAT, MAME 2003+ XML, and catver.ini
   - Store in `replay-core/data/sources/`
   - Create `UPDATE.md` with download URLs and refresh instructions

2. **Create `replay-core/build.rs`**
   - Add `phf_codegen`, `quick-xml` (or `roxmltree`) to `[build-dependencies]`
   - Parse FBNeo DAT: extract game name, description, year, manufacturer, cloneof
   - Parse MAME 2003+ XML: extract same fields plus video orientation, input players, driver status
   - Parse catver.ini: extract rom_name -> category mapping
   - Merge into unified map, deduplicate by rom_name
   - Generate `arcade_db.rs` using `phf_codegen`

3. **Create `replay-core/src/arcade_db.rs`**
   - Define `ArcadeGameInfo`, `Rotation`, `DriverStatus` types
   - `include!` the generated code
   - Public API: `lookup_arcade_game()`, `arcade_display_name()`
   - Add to `lib.rs` module list

4. **Integrate with ROM listing**
   - Add `display_name: Option<String>` to `RomEntry`
   - In `collect_roms_recursive`, call `arcade_display_name()` for arcade systems
   - Update sorting to use display name
   - Update tests

5. **Testing**
   - Unit tests for `lookup_arcade_game` with known ROM names
   - Test fallback behavior for unknown ROMs
   - Test that all source entries parse without errors
   - Verify binary size increase is within estimates

### Phase 2: Enhanced metadata usage (future)

6. Add filtering by category, players, orientation in the UI
7. Show metadata in game detail view (year, manufacturer, category)
8. Add "hide clones" toggle
9. Add "hide non-working" toggle

### Phase 3: Current MAME support (future)

10. Download and process current MAME listxml
11. Filter out non-arcade entries (isbios, isdevice, ismechanical, !runnable)
12. Add to the merged database
13. Handle `arcade_dc` (Atomiswave/Naomi) -- small enough to hardcode or pull from Flycast metadata

## Open Questions

1. **Should we ship the source data files in git?** The FBNeo DAT is 13 MB and MAME 2003+ XML is 22 MB. Together ~35 MB in the repo. This is acceptable for a one-time cost, and avoids network dependencies in builds. We could also use git-lfs if size becomes a concern.

2. **Version pinning:** Should we pin to a specific FBNeo/MAME version that matches the RePlayOS release? Ideally yes, but in practice ROM names are highly stable across versions. A newer DAT will be a strict superset of what the user has.

3. **Atomiswave/Naomi (`arcade_dc`):** This is a small set (~50 games). Flycast uses a different ROM format. We could hardcode these or find a Flycast-specific game list. Lower priority.
