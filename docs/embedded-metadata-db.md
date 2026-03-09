# Embedded Metadata Database for All Systems

Design document analyzing the feasibility of extending the existing arcade DB approach (PHF map, build-time codegen) to cover all supported game systems: consoles, handhelds, and computers.

**Status:** Research / Proposal
**Date:** March 2026
**Related docs:** `arcade-db-design.md`, `rom-identification.md`, `game-metadata-sources.md`

---

## 1. Problem Statement

The Replay companion app has an embedded arcade metadata database (28,593 entries) that maps ROM zip filenames to display names and metadata. Non-arcade systems (consoles, handhelds, computers) have no equivalent -- they display raw filenames like `Super Mario World (USA).sfc` instead of clean titles like "Super Mario World."

While the ROM filename parser described in `rom-identification.md` can extract clean titles from No-Intro/GoodTools naming conventions, an embedded metadata database would provide:

- **Verified, canonical game titles** rather than filename-derived approximations
- **Additional metadata** (year, publisher, genre, player count) for filtering and display
- **Hash-based identification** for ROMs with non-standard filenames
- **Consistency** with the arcade DB approach already proven in production

The question is whether this approach scales from ~28K arcade entries to ~80-110K entries spanning all supported systems.

---

## 2. Data Sources Analysis

### 2.1 No-Intro DAT Files (Primary Source for Cartridge Systems)

**What:** ClrMamePro-format DAT files cataloging every known dump of cartridge-based games. Maintained by the No-Intro preservation group.

**Availability:** Freely downloadable. The libretro-database GitHub repository (`github.com/libretro/libretro-database`) mirrors No-Intro DATs in `metadat/no-intro/`, updated regularly. No account required for the libretro mirror (the official DAT-o-MATIC site requires a free account).

**Coverage:** 92 systems in the libretro mirror, covering every cartridge-based console and handheld relevant to RePlayOS.

**Format (ClrMamePro):**
```
game (
    name "Super Mario World (USA)"
    description "Super Mario World (USA)"
    region "USA"
    rom ( name "Super Mario World (USA).sfc" size 524288
          crc B19ED489 md5 CDD3C8C37322978CA8669B34BC89C804
          sha1 6B47BB75D16514B6A476AA0C73A683A2A4C18765 )
)
```

**Fields available:**
- `name` / `description` -- game title with region/revision tags (No-Intro naming convention)
- `region` -- region code (USA, Europe, Japan, etc.)
- `rom` -- filename, size, CRC32, MD5, SHA1
- `serial` -- disc serial number (Redump DATs only)

**Fields NOT available:** Year, publisher, developer, genre, player count, description text. No-Intro DATs are identification-only databases, not metadata databases.

**Key insight:** No-Intro DATs provide excellent hash-to-name mapping and canonical filename standards, but lack the rich metadata we want to display. They are essential as an **identification layer** but must be supplemented for display metadata.

### 2.2 Redump DAT Files (Primary Source for Disc Systems)

**What:** ClrMamePro-format DAT files for disc-based systems. Maintained by the Redump preservation project.

**Availability:** Freely downloadable from the libretro-database mirror at `metadat/redump/`. The official Redump site also offers DAT downloads.

**Coverage:** 22 systems in the libretro mirror, covering PlayStation, Saturn, Dreamcast, Sega CD, PC Engine CD, 3DO, Neo Geo CD, CD-i, and more.

**Format:** Same ClrMamePro format as No-Intro, with an additional `serial` field for disc serial numbers. Same metadata limitations -- no year, publisher, genre.

### 2.3 TheGamesDB JSON Dump (Primary Metadata Source)

**What:** A freely downloadable JSON dump of the entire TheGamesDB.net database, available at `https://cdn.thegamesdb.net/json/database-latest.json`. No API key required for the dump file.

**Coverage:** 120,067 game entries across all platforms (retro and modern).

**Fields per entry:**
```json
{
    "id": 4,
    "game_title": "Star Fox 64",
    "release_date": "1997-06-30",
    "platform": 3,
    "region_id": 1,
    "overview": "Star Fox 64 is a 1997 N64...",
    "youtube": "...",
    "players": 4,
    "coop": "No",
    "rating": "M - Mature 17+",
    "developers": [1389],
    "genres": [1, 8],
    "publishers": [1],
    "alternates": null,
    "hashes": null
}
```

**Strengths:**
- Free bulk download, no API key needed
- Rich metadata: title, release date, overview, players, genres, developers, publishers, rating
- 120K entries -- good coverage of retro titles
- Cross-platform (same game on multiple platforms = separate entries)

**Weaknesses:**
- English only
- Name-based matching required (no hash-based lookup in the dump)
- Overview/description text is large and unsuitable for embedding
- Developer and genre IDs reference separate lookup tables (included in the dump's `include` section)
- Platform IDs need mapping to RePlayOS system names

**Key insight:** TheGamesDB dump is the best freely available source for rich metadata (year, publisher, genre, players) that can be downloaded in bulk without API keys. However, it requires name-based matching against No-Intro titles.

### 2.4 OpenVGDB (Alternative / Supplementary)

**What:** A downloadable SQLite database for ROM identification and metadata. Used by OpenEmu on macOS.

**Availability:** Free, open source. Latest release v29.0 (November 2021). Database file ~9 MB compressed.

**Coverage:** Focused on cartridge-based consoles. Covers NES, SNES, N64, Game Boy/GBC/GBA, Mega Drive, Master System, Game Gear, Atari systems, and more. Less coverage for disc-based or computer platforms.

**Fields:** Game name, description, region, release date, publisher, developer, genre, ROM hashes (CRC32, MD5, SHA1), cover art URLs.

**Strengths:**
- Hash-based matching (CRC32, MD5, SHA1)
- Includes year, publisher, genre (unlike No-Intro)
- Single SQLite file, easy to parse

**Weaknesses:**
- Last updated November 2021 -- increasingly stale
- English only
- Limited system coverage (mainly cartridge-based)
- Smaller dataset than TheGamesDB

**Verdict:** Useful as a supplementary hash-based matching source, but TheGamesDB has broader and more current coverage.

### 2.5 Libretro Database

**What:** The libretro-database repository compiles No-Intro, Redump, and TOSEC DATs into RetroArch's .rdb format. It also mirrors the raw DAT files.

**Availability:** Free on GitHub. Updated frequently (last update: March 2026).

**Value for this project:** The primary value is as a **mirror for No-Intro and Redump DATs**, eliminating the need for a DAT-o-MATIC account. The .rdb files themselves use a RetroArch-specific binary format that we don't need.

### 2.6 LaunchBox Metadata XML

**What:** Downloadable XML containing 108,000+ game entries with rich metadata, available from `https://gamesdb.launchbox-app.com/Metadata.zip`.

**Fields:** Title, description (Notes), release date, developer, publisher, genre, max players, rating, video URL, Wikipedia URL.

**Strengths:** Large dataset, rich metadata, free download, no API key.

**Weaknesses:** English only, name-based matching only (no hashes), XML format (larger than JSON), licensing terms unclear for redistribution.

**Verdict:** Good alternative to TheGamesDB if TGDB data quality proves insufficient. The two databases overlap significantly.

### 2.7 TOSEC

**What:** "The Old School Emulation Center" -- DAT files for ROMs and disc images, with its own naming convention.

**Availability:** Freely downloadable from tosecdev.org and Internet Archive. Latest release March 2025.

**Coverage:** Comprehensive, especially strong for computer platforms (Amiga, Commodore 64, ZX Spectrum, MSX, DOS).

**Key difference from No-Intro:** TOSEC includes more variants (hacks, translations, homebrews, magazine coverdiscs) resulting in larger datasets. Uses a different naming convention from No-Intro.

**Verdict:** Lower priority than No-Intro/Redump. TOSEC is most useful for computer platforms where No-Intro coverage may be thinner, but adding a second naming convention parser increases complexity. Consider for Phase 2.

### 2.8 Shiragame

**What:** A monthly-updated SQLite database that compiles No-Intro, Redump, TOSEC, and MAME DATs into a unified format with hash-to-name mapping.

**Availability:** Free, open source. SQLite file ~90 MB compressed. Last release August 2022 (appears unmaintained).

**Verdict:** Interesting concept but appears abandoned. The approach of compiling multiple DAT sources into a single SQLite DB is exactly what we'd want, but we should build our own rather than depend on an unmaintained project.

### 2.9 Summary: Data Source Comparison

| Source | Type | Rich Metadata | Hash Matching | Free Bulk DL | Maintained | Coverage |
|--------|------|:---:|:---:|:---:|:---:|----------|
| No-Intro (libretro mirror) | DAT files | No | Yes (CRC/MD5/SHA1) | Yes | Yes | Cartridge systems |
| Redump (libretro mirror) | DAT files | No | Yes (CRC/MD5/SHA1) | Yes | Yes | Disc systems |
| TheGamesDB dump | JSON | Yes | No | Yes | Yes | 120K games, all platforms |
| OpenVGDB | SQLite | Partial | Yes | Yes | Stale (2021) | Cartridge systems |
| LaunchBox Metadata | XML | Yes | No | Yes | Yes | 108K+ games |
| TOSEC | DAT files | No | Yes | Yes | Yes | Computer systems esp. |
| Shiragame | SQLite | No | Yes | Yes | Stale (2022) | Multi-source compilation |

---

## 3. Scale Analysis: Games Per System

### 3.1 Estimated Entry Counts (from libretro-database DAT file sizes)

Calibration: Atari 5200 DAT = 29,083 bytes / 117 entries = ~249 bytes/entry (No-Intro). Sega CD DAT = 151,839 bytes / 544 entries = ~279 bytes/entry (Redump). Using 230 bytes/entry for No-Intro and 295 bytes/entry for Redump.

**Note:** These are FULL No-Intro/Redump counts including all regional variants, revisions, and alternate dumps. A single game like "Super Mario World" may have 5-10 entries (USA, Europe, Japan, Rev 1, etc.). The actual unique game count is typically 30-50% of the full entry count.

#### Cartridge Systems (No-Intro)

| System | RePlayOS Folder | Est. Full Entries | Est. Unique Games |
|--------|-----------------|------------------:|------------------:|
| NES | `nintendo_nes` | ~14,100 | ~4,500 |
| Nintendo DS | `nintendo_ds` | ~9,600 | ~4,000 |
| Game Boy Advance | `nintendo_gba` | ~4,300 | ~1,800 |
| SNES | `nintendo_snes` | ~4,300 | ~1,800 |
| Mega Drive / Genesis | `sega_smd` | ~3,900 | ~1,500 |
| Commodore 64 | `commodore_c64` | ~3,300 | ~2,500 |
| Commodore Amiga | `commodore_ami` | ~3,300 | ~2,500 |
| N64 | `nintendo_n64` | ~2,700 | ~400 |
| Game Boy Color | `nintendo_gbc` | ~2,700 | ~900 |
| Game Boy | `nintendo_gb` | ~2,200 | ~800 |
| Master System | `sega_sms` | ~1,100 | ~400 |
| Game Gear | `sega_gg` | ~920 | ~400 |
| Atari 2600 | `atari_2600` | ~900 | ~500 |
| MSX | `microsoft_msx` | ~820 | ~600 |
| Atari Lynx | `atari_lynx` | ~820 | ~80 |
| Atari Jaguar | `atari_jaguar` | ~640 | ~60 |
| PC Engine | `nec_pce` | ~510 | ~300 |
| Atari 7800 | `atari_7800` | ~420 | ~100 |
| Sega 32X | `sega_32x` | ~270 | ~40 |
| SG-1000 | `sega_sg` | ~210 | ~100 |
| Neo Geo Pocket Color | `snk_ngp` | ~180 | ~80 |
| Atari 5200 | `atari_5200` | ~120 | ~70 |
| ZX Spectrum | `sinclair_zx` | ~130 | ~80 |
| Neo Geo Pocket | `snk_ngp` | ~16 | ~10 |
| **Subtotal** | | **~57,500** | **~23,500** |

#### Disc Systems (Redump)

| System | RePlayOS Folder | Est. Full Entries | Est. Unique Games |
|--------|-----------------|------------------:|------------------:|
| PlayStation | `sony_psx` | ~13,200 | ~4,000 |
| CD-i | `philips_cdi` | ~2,800 | ~500 |
| Sega Saturn | `sega_st` | ~2,600 | ~1,100 |
| Sega Dreamcast | `sega_dc` | ~1,800 | ~700 |
| 3DO | `panasonic_3do` | ~630 | ~250 |
| Sega CD | `sega_cd` | ~510 | ~200 |
| PC Engine CD | `nec_pcecd` | ~480 | ~250 |
| Amiga CD | `commodore_amicd` | ~170 | ~100 |
| Neo Geo CD | `snk_ngcd` | ~150 | ~100 |
| **Subtotal** | | **~22,300** | **~7,200** |

#### Summary

| Category | Full Entries | Est. Unique Games |
|----------|------------:|-----------------:|
| Arcade (existing DB) | 28,593 | ~10,000 |
| Cartridge systems | ~57,500 | ~23,500 |
| Disc systems | ~22,300 | ~7,200 |
| **Total** | **~108,400** | **~40,700** |

### 3.2 What to Embed: Full Set vs. 1G1R

**Full No-Intro set (all entries):** Includes every regional variant, revision, and alternate dump. This is what you need for hash-based identification -- given any ROM file, you can match its hash against the full set.

**1G1R set (one game, one ROM):** Filtered to one "best" entry per unique game. This is what you'd use for display name resolution -- the user sees "Super Mario World" once, not 8 times.

**Recommendation:** Embed the **full set** for hash-based identification and filename matching. At display time, use the ROM identification parser (from `rom-identification.md`) to group variants. The embedded DB's role is to provide verified canonical names and metadata, not to deduplicate the user's ROM collection.

---

## 4. What Metadata to Embed

### 4.1 Per-Entry Fields

The metadata to embed depends on the data source. We have two layers:

**Layer 1: Identification (from No-Intro / Redump DATs)**
```
filename:    String   // canonical ROM filename (e.g., "Super Mario World (USA).sfc")
clean_title: String   // game title without tags (e.g., "Super Mario World")
region:      String   // region code (e.g., "USA", "Europe", "Japan")
crc32:       u32      // CRC32 checksum for hash-based matching
```

**Layer 2: Rich Metadata (from TheGamesDB dump, cross-referenced by title + platform)**
```
year:        u16      // release year (0 = unknown)
publisher:   String   // publisher name
genre:       String   // genre/category
players:     u8       // max players (0 = unknown)
```

### 4.2 Fields Deliberately Omitted

| Field | Reason for omission |
|-------|-------------------|
| Description/overview | Too large to embed (100-500 bytes per game x 80K games = 8-40 MB of text alone) |
| Box art URL/hash | URLs change; better to fetch at runtime via metadata APIs |
| MD5 / SHA1 hashes | CRC32 is sufficient for initial matching (4 bytes vs. 16+20 bytes per entry); SHA1 can be verified via on-demand API calls |
| Developer | Low display priority; can overlap with publisher; saves space |
| Rating (ESRB/PEGI) | Low priority for retro games |
| ROM file size | Available from the filesystem at scan time |

### 4.3 Per-Entry Size Estimate

| Field | Avg. Bytes | Notes |
|-------|----------:|-------|
| filename (key) | 40 | average No-Intro filename length |
| clean_title | 25 | game title without region/revision tags |
| region | 6 | "USA", "Europe", "Japan", etc. |
| crc32 | 4 | fixed-size integer |
| year | 2 | u16 |
| publisher | 15 | company name |
| genre | 15 | category string |
| players | 1 | u8 |
| PHF overhead | 3 | hash displacement table |
| **Total per entry** | **~111** | |

For 80,000 entries: ~8.7 MB binary. For 108,000 entries: ~11.7 MB binary.

---

## 5. Size Analysis

### 5.1 Current Arcade DB Baseline

| Metric | Value |
|--------|-------|
| Entries | 28,593 |
| Generated Rust source | 8.3 MB |
| Estimated binary contribution | ~2.2 MB |
| Binary total (release) | 35 MB |
| Arcade DB as % of binary | ~6.3% |
| Source bytes per entry | ~290 |
| Binary bytes per entry | ~77 |

### 5.2 Projected Sizes for All Systems

| Scenario | Entries | Gen. Source | Binary Impact | % of 35 MB Binary |
|----------|--------:|----------:|------------:|--------:|
| Arcade only (current) | 28,593 | 8.3 MB | ~2.2 MB | 6.3% |
| + Cartridge systems | 86,100 | ~25 MB | ~6.6 MB | 19% |
| + Disc systems | 108,400 | ~31 MB | ~8.3 MB | 24% |
| Lean schema (fewer fields) | 108,400 | ~20 MB | ~5.5 MB | 16% |

### 5.3 Raspberry Pi Constraints

| Resource | Pi 3 / Zero 2 | Pi 4 (4 GB) | Pi 5 |
|----------|--------------|-------------|------|
| RAM | 1 GB | 4 GB | 4-8 GB |
| Storage | SD card (8-128 GB) | SD/USB | SD/USB/NVMe |
| Binary + DB in RAM? | ~43 MB feasible | Easily feasible | Easily feasible |

A 43 MB binary (35 MB base + 8 MB metadata DB) is well within the capabilities of all Raspberry Pi models. For context, RetroArch itself is ~50-100 MB. The metadata DB would be memory-mapped as part of the binary's static data segment (it's `&'static str` data in a PHF map), so it doesn't require heap allocation.

**Verdict:** Feasible for Raspberry Pi deployment. Even the most generous estimate (8.3 MB) is modest compared to available RAM and storage.

---

## 6. Build-Time Feasibility

### 6.1 PHF Generation Performance

The `phf_codegen` library uses the CHD (Compress, Hash, and Displace) algorithm. Benchmarks:

- 28,593 entries (current arcade DB): sub-second
- 100,000 entries: ~0.4 seconds
- 500,000 entries: likely 2-5 seconds
- 1,000,000 entries: reportedly 20+ minutes (nonlinear scaling)

For ~108K entries, PHF generation itself should take **under 1 second**. This is well within acceptable build times.

### 6.2 Rust Compilation of Generated Code

The bigger concern is compiling the generated Rust source file. The current 8.3 MB `arcade_db.rs` compiles without issues. At ~31 MB for the full database, this pushes into territory where `rustc` may slow down significantly due to:

- Parsing a single very large file
- Processing a single enormous `static` initializer expression
- LLVM optimization passes on the data section

**Mitigation strategies:**
1. **Split into multiple PHF maps** -- one per system or system category. Each map is a separate `static` in the generated code, keeping individual expressions manageable.
2. **Reduce source verbosity** -- use shorter field names and compact struct layout to reduce generated source size.
3. **Conditional compilation** -- `cfg` features to include/exclude system databases (e.g., `--features db-nes,db-snes`).

### 6.3 Source Data Parsing at Build Time

Parsing ~30 MB of ClrMamePro DAT files plus cross-referencing against a ~200 MB JSON dump (TheGamesDB) is a non-trivial build.rs workload. Estimated:

- Parsing 80K entries from ~30 DAT files: 2-5 seconds (streaming XML/DAT parser)
- Parsing 120K entries from TheGamesDB JSON dump: 3-5 seconds
- Cross-referencing by title+platform: 1-2 seconds (hash map join)
- PHF generation: <1 second
- **Total build.rs time: ~10-15 seconds**

This is acceptable. The build.rs would only re-run when source data files change (tracked via `cargo::rerun-if-changed`).

### 6.4 Alternative: Pre-compiled Binary Blob

If PHF compilation proves too slow for 108K entries, an alternative is:

1. Build.rs parses all source data and writes a compact binary blob (custom format or MessagePack)
2. Embed via `include_bytes!`
3. At startup, deserialize into a `HashMap` or do binary search on the sorted blob

**Pros:** Faster compilation (no huge Rust source to parse), smaller binary (compact encoding).
**Cons:** Startup latency (deserialization), runtime allocation, more complex code.

**Verdict:** Try PHF first. Only switch to binary blob if compile times exceed 60 seconds. The `include_bytes!` approach in recent Rust (post PR #103812) handles large blobs much better than earlier versions.

### 6.5 Alternative: Embedded SQLite File

Another option is to build a SQLite database file at build time and embed it via `include_bytes!`. At runtime, open it as an in-memory database.

**Pros:** Flexible querying (SQL), standard format, good tooling, handles large datasets well.
**Cons:** Adds `rusqlite`/`libsqlite3-sys` dependency (~1.5 MB), startup deserialization, not zero-cost like PHF.

**Verdict:** Overkill for simple key-value lookups. Better suited if we need complex queries (e.g., "all USA RPGs from 1995"). Not recommended for Phase 1.

---

## 7. Identification Strategy

### 7.1 How Identification Works Per System Type

| System Type | Primary ID Method | Fallback | Example |
|-------------|-------------------|----------|---------|
| Arcade (zip) | Zip filename | None needed | `sf2.zip` -> lookup `sf2` |
| Cartridge (No-Intro named) | Filename match | CRC32 hash | `Super Mario World (USA).sfc` -> strip extension, look up |
| Cartridge (non-standard name) | CRC32 hash | Filename parse | `smw.sfc` -> hash -> CRC32 match |
| Disc (cue/chd) | Filename match | Serial number | `Resident Evil (USA).chd` -> strip ext, look up |

### 7.2 Filename-Based Matching (Fast, No I/O)

For ROMs following No-Intro naming conventions (the vast majority in curated collections):

```
Input:  "Legend of Zelda, The - A Link to the Past (USA).sfc"
Step 1: Strip extension -> "Legend of Zelda, The - A Link to the Past (USA)"
Step 2: Look up full filename in PHF map -> found!
Step 3: Return metadata: { clean_title: "The Legend of Zelda - A Link to the Past", year: 1991, ... }
```

This is identical to the arcade DB approach but using the full No-Intro filename (with region tags) as the key instead of the short ROM name.

**Key difference from arcade:** Arcade ROM names are short and unique (`sf2`, `mslug6`). No-Intro filenames are long and include region tags. The PHF key would be the full canonical filename (without extension), e.g., `"Super Mario World (USA)"`.

### 7.3 Hash-Based Matching (Accurate, Requires File I/O)

For ROMs with non-standard filenames, compute CRC32 and look up:

```
Input:  "smw_usa.sfc" (non-standard name)
Step 1: Filename not in PHF map
Step 2: Read file, compute CRC32 -> 0xB19ED489
Step 3: Look up CRC32 in a separate CRC->entry index
Step 4: Return metadata for "Super Mario World (USA)"
```

The CRC32 index would be a second PHF map (`Map<u32, &'static str>`) mapping CRC32 values to canonical filenames, which then resolve via the primary PHF map.

**Size impact of CRC32 index:** 80,000 entries x (4 bytes CRC + 4 bytes offset + 3 bytes PHF overhead) = ~880 KB additional binary size. Modest.

### 7.4 Title Extraction (Display)

For display purposes, derive a clean title from the canonical filename:

```rust
/// Extract clean title from a No-Intro filename.
/// "Legend of Zelda, The - A Link to the Past (USA)" -> "The Legend of Zelda - A Link to the Past"
fn clean_title(nointro_name: &str) -> String {
    // Strip everything from first '(' onward
    let title = nointro_name.split('(').next().unwrap_or(nointro_name).trim();
    // Normalize "Name, The" -> "The Name"
    normalize_article(title)
}
```

This can be done at build time and stored as a field, or at runtime (it's trivial string manipulation). Doing it at build time saves runtime work and ensures consistency.

---

## 8. Recommended Approach

### 8.1 Architecture Overview

```
Source Data (downloaded)          Build Script (build.rs)              Runtime
─────────────────────             ─────────────────────                ───────

No-Intro DATs (libretro) ──┐
                            ├─> Parse DATs ─┐
Redump DATs (libretro) ────┘                │
                                            ├─> Cross-ref ─> PHF codegen ─> include!()
TheGamesDB JSON dump ──────────> Parse JSON ┘                     │
                                                                   ▼
                                                     game_db.rs (per-system maps)
                                                           │
                                                           ├── lookup_game("nintendo_nes", "Super Mario Bros. (World).nes")
                                                           ├── lookup_by_crc("nintendo_nes", 0xABCD1234)
                                                           └── game_display_name("nintendo_nes", "Super Mario Bros. (World).nes")
```

### 8.2 Data Pipeline

**Step 1: Download source data** (one-time, script-assisted)

```
data/
├── no-intro/                      # Mirrored from libretro-database
│   ├── Nintendo - Nintendo Entertainment System.dat
│   ├── Nintendo - Super Nintendo Entertainment System.dat
│   ├── Sega - Mega Drive - Genesis.dat
│   └── ... (one per system)
├── redump/
│   ├── Sony - PlayStation.dat
│   ├── Sega - Saturn.dat
│   └── ... (one per system)
├── thegamesdb-latest.json         # TheGamesDB bulk dump
├── fbneo-arcade.dat               # (existing)
├── mame2003plus.xml               # (existing)
├── mame0285-arcade.xml            # (existing)
├── catver.ini                     # (existing)
└── catver-mame-current.ini        # (existing)
```

Download script fetches:
- No-Intro/Redump DATs: `git archive` or raw download from libretro-database
- TheGamesDB dump: single HTTP GET to `https://cdn.thegamesdb.net/json/database-latest.json`

**Step 2: Build-time processing** (build.rs)

1. Parse each No-Intro/Redump DAT file: extract filename, region, CRC32
2. Derive clean title from each filename (strip region/revision tags, normalize articles)
3. Parse TheGamesDB JSON: build a lookup table keyed by (normalized_title, platform_id)
4. Cross-reference: for each DAT entry, find a matching TheGamesDB entry by title+platform
5. Merge metadata: DAT provides filename, region, CRC32; TGDB provides year, publisher, genre, players
6. Generate PHF maps: one per system (or per system category)
7. Generate CRC32 index: maps CRC32 -> canonical filename for hash-based fallback

**Step 3: Runtime API**

```rust
/// Metadata for a game ROM.
pub struct GameInfo {
    /// Canonical filename from No-Intro/Redump (without extension).
    pub canonical_name: &'static str,
    /// Clean display title (articles normalized, tags stripped).
    pub display_name: &'static str,
    /// Region code.
    pub region: &'static str,
    /// Release year (0 = unknown).
    pub year: u16,
    /// Publisher name (empty = unknown).
    pub publisher: &'static str,
    /// Genre/category (empty = unknown).
    pub genre: &'static str,
    /// Max players (0 = unknown).
    pub players: u8,
    /// CRC32 of the ROM file.
    pub crc32: u32,
}

/// Look up game metadata by filename (without extension) for a given system.
pub fn lookup_game(system: &str, filename_stem: &str) -> Option<&'static GameInfo>;

/// Look up game metadata by CRC32 hash for a given system.
pub fn lookup_by_crc(system: &str, crc32: u32) -> Option<&'static GameInfo>;

/// Get display name for a ROM, falling back to filename parsing, then raw filename.
pub fn game_display_name(system: &str, filename: &str) -> String;
```

### 8.3 Data Source Selection

| Data Need | Source | Rationale |
|-----------|--------|-----------|
| Canonical filenames + hashes | No-Intro/Redump DATs (libretro mirror) | Authoritative, freely available, hash-based identification |
| Year, publisher, genre, players | TheGamesDB JSON dump | Free bulk download, 120K entries, good retro coverage |
| Arcade metadata | MAME/FBNeo DATs (existing) | Already implemented, keep as-is |
| Display name derivation | Filename parsing (build-time) | Deterministic, no external dependency |

### 8.4 Storage Format: PHF Maps, Split by System

Rather than one giant PHF map with 108K entries, use **one PHF map per system** (or per system category):

```rust
// Generated by build.rs -- one file per system
// replay-core/src/game_db_generated/nintendo_nes.rs
static NES_DB: phf::Map<&'static str, GameInfo> = ...;
static NES_CRC_INDEX: phf::Map<u32, &'static str> = ...;

// replay-core/src/game_db_generated/sega_smd.rs
static SMD_DB: phf::Map<&'static str, GameInfo> = ...;
static SMD_CRC_INDEX: phf::Map<u32, &'static str> = ...;
```

**Advantages of per-system splitting:**
- Each generated file is small enough for `rustc` to handle efficiently
- Systems can be enabled/disabled via cargo features
- Parallel compilation of per-system files
- Incremental builds: changing one DAT only recompiles that system's map
- Easier to test per-system

**Disadvantage:** More code to manage the dispatch layer. Mitigated by a macro or codegen.

### 8.5 Estimated Totals

| Metric | Value |
|--------|-------|
| Total entries (all systems) | ~108,000 |
| Binary size impact | ~8-9 MB |
| Generated Rust source | ~25-31 MB |
| Build.rs processing time | ~10-15 seconds |
| PHF generation time | <1 second |
| Rust compilation impact | ~30-60 seconds (split files) |
| TheGamesDB metadata match rate | ~60-70% (estimated) |

---

## 9. Phased Rollout Plan

### Phase 1: Identification Layer (No-Intro filename matching)

**Goal:** Embed canonical filenames from No-Intro DATs for the most popular cartridge systems. Provide `display_name` resolution for ROMs that follow No-Intro naming conventions.

**Systems:** NES, SNES, N64, Game Boy, GBC, GBA, Mega Drive/Genesis, Master System, Game Gear.

**Data source:** No-Intro DATs from libretro-database (9 files).

**Fields embedded:** canonical filename (key), clean_title (derived), region.

**Estimated entries:** ~45,000 (these 9 systems).

**Binary impact:** ~3.5 MB.

**What it enables:**
- `game_display_name("nintendo_nes", "Legend of Zelda, The (USA) (Rev A).nes")` returns `"The Legend of Zelda"`
- Verified canonical names for ROM files matching No-Intro naming
- Foundation for hash-based matching in Phase 2

**Excludes for now:** Year, publisher, genre (no TheGamesDB cross-referencing yet), CRC32 index, disc systems.

### Phase 2: Rich Metadata via TheGamesDB Cross-Reference

**Goal:** Add year, publisher, genre, and player count by cross-referencing No-Intro entries against the TheGamesDB dump.

**Data source:** TheGamesDB JSON dump (`database-latest.json`, ~200 MB).

**Processing:** Build.rs parses the JSON dump, builds a title+platform lookup table, and enriches existing entries.

**New fields:** year (u16), publisher (&'static str), genre (&'static str), players (u8).

**Binary impact:** +2-3 MB (additional string data for publisher/genre).

**Match rate:** Expect 60-70% of No-Intro entries to find a TheGamesDB match. Unmatched entries retain filename-derived clean_title with zero/empty metadata.

### Phase 3: CRC32 Hash Index

**Goal:** Enable hash-based ROM identification for files with non-standard filenames.

**Data:** CRC32 values from No-Intro DATs (already present but not used in Phase 1).

**Implementation:** Generate a second PHF map per system: `Map<u32, &'static str>` mapping CRC32 to canonical filename.

**Binary impact:** +880 KB.

**What it enables:**
- Identify renamed ROMs: `my_fav_game.sfc` -> CRC32 match -> "Super Mario World"
- Background hash scan to verify ROM collection integrity

### Phase 4: Disc Systems (Redump)

**Goal:** Extend to PlayStation, Saturn, Dreamcast, Sega CD, PC Engine CD, 3DO, Neo Geo CD, CD-i.

**Data source:** Redump DATs from libretro-database.

**Challenge:** Disc-based ROMs use CHD/CUE/ISO formats where CRC32 matching is less straightforward (you hash the extracted data, not the container). Filename matching is the practical primary method.

**Estimated entries:** ~22,000 additional.

**Binary impact:** +1.7 MB.

### Phase 5: Remaining Systems + Computer Platforms

**Goal:** Cover remaining RePlayOS systems (Commodore 64, Amiga, MSX, ZX Spectrum, Sharp X68000, Amstrad CPC).

**Challenge:** Computer platforms have large catalogs with many variants (especially C64 and Amiga). TOSEC DATs may be needed to supplement No-Intro coverage.

**Estimated entries:** ~10,000 additional.

---

## 10. Open Questions

1. **TheGamesDB title matching accuracy:** How reliably can we match No-Intro titles (e.g., "Legend of Zelda, The - A Link to the Past") to TheGamesDB titles (e.g., "The Legend of Zelda: A Link to the Past")? This needs fuzzy matching / normalization. A prototype should measure the match rate before committing to this source.

2. **Neo Geo (snk_ng):** Neo Geo games on RePlayOS use zip filenames like arcade ROMs. Should they use the arcade DB or the game DB? Since the `snk_ng` folder uses the same naming as FBNeo arcade ROMs, the existing arcade DB likely already covers them. Verify.

3. **ScummVM (scummvm):** ScummVM games use a `.scummvm` file that contains a game ID (e.g., `monkey2`). These IDs map to ScummVM's internal game database. This is a special case that needs its own handling -- not a DAT-file-based approach.

4. **IBM PC / DOS (ibm_pc):** DOS games are typically distributed as zip archives with executables inside. There is no standard naming convention and no hash-based identification method. This system may need to remain filename-only or use a curated list.

5. **Alpha Player (alpha_player):** This is a media player utility, not a game system. Skip metadata DB for this.

6. **Cargo feature flags:** Should each system's DB be behind a feature flag? This would allow users building for a specific system to exclude databases they don't need. Adds build complexity but enables smaller binaries.

7. **Update cadence:** No-Intro and Redump DATs are updated frequently (new dumps, corrections). How often should we refresh the embedded data? The arcade DB is pinned to MAME 0.285. Consider pinning to a specific No-Intro/Redump date and updating with each Replay release.

---

## 11. Conclusion

### Feasibility: Yes, with caveats

Extending the embedded database approach to all systems is feasible:

- **Scale is manageable:** ~108K entries is roughly 4x the current arcade DB. PHF handles this comfortably.
- **Binary size is acceptable:** ~8-9 MB additional binary size is well within Raspberry Pi constraints.
- **Build time is reasonable:** ~10-15 seconds for data processing, <1 second for PHF generation. Rust compilation of split files adds 30-60 seconds.
- **Data sources exist:** No-Intro/Redump DATs (free, via libretro mirror) provide identification. TheGamesDB dump (free) provides rich metadata.

### Key constraint: Metadata richness depends on cross-referencing

No-Intro/Redump DATs alone provide only filenames, regions, and hashes -- no year, publisher, or genre. Rich metadata requires cross-referencing against TheGamesDB or a similar source, with an estimated 60-70% match rate. Unmatched entries would have filename-derived titles with empty metadata, which is still an improvement over raw filenames.

### Recommended path forward

1. **Start with Phase 1** (No-Intro filename matching for 9 core systems) -- low risk, immediate value, validates the approach at scale.
2. **Measure TheGamesDB match rate** before committing to Phase 2 -- build a prototype that cross-references and reports match statistics.
3. **Split PHF maps per system** to manage compile times and enable incremental builds.
4. **Keep arcade DB separate** -- it works well and uses a different identification model (zip name vs. full filename).

### What NOT to do

- **Don't embed descriptions/overviews** -- too large (would add 10-40 MB), better fetched on demand via API.
- **Don't embed cover art** -- even URLs/hashes are too volatile; fetch at runtime.
- **Don't try to replace the metadata API approach** described in `game-metadata-sources.md` -- the embedded DB is for offline display names and basic metadata, while the API provides rich media (images, videos, detailed descriptions).
- **Don't use a single monolithic PHF map** for all 108K entries -- split by system.
