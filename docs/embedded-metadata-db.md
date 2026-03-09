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

---

## Appendix A: Rich Metadata Fields -- Size and Build Impact Analysis

This appendix analyzes the impact of adding five additional metadata fields to the embedded database, beyond the core fields already planned (filename, clean_title, region, crc32, year, publisher, genre, players). The analysis covers fields that were either omitted in section 4.2 or mentioned only briefly, and reconsiders them with concrete numbers.

The fields are analyzed in priority order, with cumulative totals showing the cost of including each one.

### A.1 Players (u8)

#### Data sourcing

- **TheGamesDB JSON dump** (`database-latest.json`, ~40 MB): The `players` field is a direct integer in each game entry. Coverage is good for popular retro titles but spotty for obscure releases -- estimated 60-70% of retro game entries have a non-null `players` value.
- **Libretro-database**: The `metadat/maxusers/` directory contains per-system DAT files that map ROM filenames to max player counts. This is compiled from multiple sources and is CRC-matched, so coverage is high for games present in the No-Intro sets. Estimated 50-60% coverage across all systems.
- **LaunchBox Metadata XML** (108K+ entries): The `MaxPlayers` field is present on most entries. Estimated 70-80% coverage.
- **Cross-referencing**: Best strategy is to use libretro-database `maxusers` DATs as primary (hash-matched, authoritative) and fall back to TheGamesDB/LaunchBox for gaps.
- **Quality**: Highly consistent -- player count is a simple integer (1-8 typically). Minimal data quality issues.

#### Size impact

- **Per entry**: 1 byte (u8). Zero for unknown (already in the schema).
- **Total raw**: 108,000 x 1 = **108 KB**.
- **No compression needed** -- it's a single byte.
- **Cumulative**: 108 KB.

#### Build impact

- Negligible. Parsing maxusers DATs is trivial (simple key-value format). The u8 field adds virtually nothing to PHF map generation or compile time.

#### Storage strategy

- Inline in the PHF map entry struct. A u8 field is the most compact possible representation.

#### Recommendation

**Include in Phase 2.** Already planned in the schema (section 4.1). Cost is negligible (108 KB). The players field is valuable for filtering (multiplayer games) and display. libretro-database maxusers DATs are the best source -- free, hash-matched, maintained.

---

### A.2 Genres

#### Data sourcing

- **TheGamesDB JSON dump**: Genres are stored as arrays of integer IDs referencing a separate lookup table in the dump's `include.genres` section. Each game may have 1-3 genre IDs. The lookup table has ~30 genres (Action, Adventure, Fighting, Platform, Puzzle, Racing, RPG, Shooter, Sports, Strategy, etc.). Coverage: estimated 65-75% of retro game entries have at least one genre.
- **Libretro-database**: The `metadat/genre/` directory contains per-system DAT files mapping ROM filenames (via CRC) to genre strings. These use their own genre taxonomy. Coverage varies by system -- popular systems like NES and SNES have good coverage (60-70%), less common systems may be 30-40%.
- **LaunchBox Metadata XML**: The `Genres` field uses semicolon-separated genre strings. Coverage is high (80%+) with a rich taxonomy.
- **Quality**: Genre taxonomies are inconsistent across sources. TheGamesDB uses broad categories ("Action"), libretro uses more specific categories ("Platform / Run Jump" similar to MAME catver.ini), and LaunchBox uses its own set. Normalization is needed.

#### Size impact

- **Per entry (string)**: Average genre string is 10-20 bytes (e.g., "Action", "Role-Playing", "Platform / Action"). With multiple genres, average ~20 bytes.
- **Total raw**: 108,000 x 20 = **2.1 MB**.
- **With string interning**: There are only ~30-50 unique genre values (or ~100-150 if using compound genres like "Action / Platform"). A string interning pool of genre values would be ~2-4 KB. Each entry stores a u8 or u16 index instead of a string pointer. Per entry: 1-2 bytes. Total: 108,000 x 2 = **216 KB** interned.
- **Cumulative**: 108 KB (players) + 216 KB (genres) = **324 KB**.

#### Build impact

- Parsing genre DATs from libretro-database or extracting from TheGamesDB JSON: 1-2 seconds additional.
- String interning at build time: trivial.
- PHF impact: minimal -- replacing a `&'static str` with a u8/u16 index reduces generated code size.

#### Storage strategy

- **String interning pool**: Build a static array of genre strings at compile time. Each entry stores a `u8` index into this array (supports up to 255 genres, more than enough). This is far more compact than embedding genre strings per-entry.

```rust
static GENRE_POOL: &[&str] = &["Action", "Adventure", "Fighting", "Platform", ...];

pub struct GameInfo {
    // ...
    pub genre_index: u8,  // index into GENRE_POOL, 255 = unknown
}
```

- For games with multiple genres, either pick the primary genre (most common in practice) or use a `u16` bitfield (supports up to 16 genres per game with 16 possible genre values, or more with wider types).

#### Recommendation

**Include in Phase 2.** With string interning, the cost is ~216 KB -- trivial. Genre data is highly valuable for filtering and browsing. Use libretro-database `metadat/genre/` as the primary source (CRC-matched), supplemented by TheGamesDB. Normalize to a unified taxonomy of ~30-50 genres at build time.

---

### A.3 Developer

#### Data sourcing

- **TheGamesDB JSON dump**: Developers are stored as arrays of integer IDs referencing `include.developers` lookup table. The lookup table contains ~9,000 developer entries. Each game typically has 1-2 developer IDs. Coverage for retro games: estimated 55-65%.
- **Libretro-database**: The `metadat/developer/` directory contains per-system DAT files. These map CRC-matched ROMs to developer strings. Coverage varies -- major systems have reasonable coverage (40-60%), but many entries are missing or attributed to the publisher rather than the actual developer.
- **LaunchBox Metadata XML**: The `Developer` field is a string. Coverage: ~70% for retro platforms.
- **Quality**: Developer data is messy for retro games. Many NES/SNES games were developed by small studios that were renamed, merged, or dissolved. Spellings vary across sources (e.g., "Konami" vs "Konami Computer Entertainment Tokyo" vs "KCET"). Normalization is harder than for genres.

#### Size impact

- **Per entry (raw string)**: Average developer name is 12-20 bytes (e.g., "Capcom", "Nintendo R&D1", "Square", "Konami Computer Entertainment Tokyo").
- **Total raw**: 108,000 x 16 = **1.7 MB**.
- **With string interning**: There are roughly 2,000-4,000 unique developer names across the full dataset. An interning pool of developer strings would be ~40-80 KB. Each entry stores a u16 index. Per entry: 2 bytes. Total entries: 108,000 x 2 = **216 KB** for indices + ~60 KB pool = **276 KB** interned.
- **Cumulative**: 324 KB + 276 KB = **600 KB**.

#### Build impact

- Parsing developer DATs or extracting from TheGamesDB JSON: 1-2 seconds additional.
- Developer name normalization (deduplication of variant spellings): adds complexity to build.rs but doesn't significantly impact build time.
- PHF impact: minimal with interning.

#### Storage strategy

- **String interning pool** (same approach as genres): Build a static array of developer strings. Each entry stores a `u16` index (supports up to 65,535 developers). The pool itself is ~40-80 KB.

```rust
static DEVELOPER_POOL: &[&str] = &["Capcom", "Nintendo", "Konami", "Square", "Sega", ...];

pub struct GameInfo {
    // ...
    pub developer_index: u16,  // index into DEVELOPER_POOL, 0xFFFF = unknown
}
```

#### Recommendation

**Include in Phase 2, but lower priority than genres.** Cost is ~276 KB with interning -- still modest. The value is moderate: developer info is nice-to-have for display but rarely used for filtering. The main challenge is data quality -- developer names need normalization, which adds build.rs complexity. Consider deferring to Phase 3 if the normalization effort is too high.

---

### A.4 Publisher

#### Data sourcing

- **TheGamesDB JSON dump**: Publishers are stored as arrays of integer IDs referencing `include.publishers` lookup table. The lookup table contains ~4,000 publisher entries. Coverage: estimated 60-70% for retro games.
- **Libretro-database**: The `metadat/publisher/` directory contains per-system DAT files. CRC-matched. Coverage similar to developer data (40-60%).
- **LaunchBox Metadata XML**: The `Publisher` field is a string. Coverage: ~75%.
- **Quality**: Better than developer data -- publishers are more standardized. "Nintendo", "Sega", "Capcom", "Konami" are unambiguous. Some variant spellings exist ("THQ" vs "THQ Inc.") but fewer than developers.

#### Size impact

- **Per entry (raw string)**: Average publisher name is 10-15 bytes.
- **Total raw**: 108,000 x 12 = **1.3 MB**.
- **With string interning**: There are roughly 1,000-2,500 unique publisher names. An interning pool would be ~20-50 KB. Each entry stores a u16 index. Per entry: 2 bytes. Total: 108,000 x 2 = **216 KB** indices + ~35 KB pool = **251 KB** interned.
- **Cumulative**: 600 KB + 251 KB = **851 KB**.

#### Build impact

- Same as developer: 1-2 seconds parsing, trivial interning.
- Publisher name normalization is simpler than developer normalization.

#### Storage strategy

- **String interning pool**, identical approach to developer. Publisher is already in the planned schema (section 4.1), so this confirms the interning approach is optimal.

#### Recommendation

**Include in Phase 2** (already planned). Cost is ~251 KB with interning. Publisher is more useful than developer for browsing ("show me all Capcom games") and has better data quality. Use TheGamesDB as primary source, supplemented by libretro-database `metadat/publisher/`.

---

### A.5 Description/Overview

#### Data sourcing

- **TheGamesDB JSON dump**: The `overview` field contains a prose description/synopsis of the game. This is the most substantial text field in any game metadata source. Skyscraper (a popular ROM scraper) defaults to truncating TheGamesDB descriptions at 2,500 characters. In practice, retro game descriptions in TheGamesDB range from 50 to 2,000 characters, with an estimated average of 400-600 characters (200-300 words). Coverage: estimated 50-65% of retro game entries have a non-empty overview.
- **LaunchBox Metadata XML**: The `Notes` field contains similar description text, sourced from MobyGames, Wikipedia, and community contributions. Average length is comparable to TheGamesDB. Coverage: ~60-70%.
- **ScreenScraper API**: Provides multi-language descriptions (French, English, Spanish, etc.). Not available in bulk download -- requires per-game API calls.
- **Quality**: Descriptions vary wildly in quality. Some are one-sentence stubs ("Pac-Man is a classic arcade game."), others are detailed multi-paragraph summaries. Retro games (pre-2000) tend to have shorter descriptions than modern games. English-only from TheGamesDB and LaunchBox.

#### Size impact

This is the most impactful field by far. The analysis considers several scenarios:

**Scenario 1: Full descriptions, uncompressed**

- Estimated average: 500 bytes per entry (considering that ~40% of entries have no description).
- For entries with descriptions (~65K entries): average 750 bytes each.
- **Total raw**: ~65,000 x 750 + 43,000 x 0 = **~48 MB**.
- This is larger than the entire current binary (35 MB). Clearly not viable as-is.

**Scenario 2: Truncated descriptions (first 200 characters)**

- Average after truncation: ~150 bytes per entry (with empty entries).
- **Total raw**: 108,000 x 150 = **~16 MB**.
- Still very large -- nearly doubles the binary.

**Scenario 3: Full descriptions, zstd-compressed blob**

- Raw text of ~48 MB of English prose compresses extremely well with zstd. English text typically achieves 3:1 to 5:1 compression ratios. Game descriptions are repetitive (similar vocabulary, structure) which helps compression.
- With zstd dictionary compression (trained on the corpus): estimated 5:1 to 8:1 ratio.
- **Compressed size**: 48 MB / 6 = **~8 MB** (zstd with dictionary).
- This would be embedded as a single `include_bytes!` blob. At runtime, individual descriptions are decompressed on demand using an offset table.
- Decompression speed: zstd decompresses at 1-2 GB/s, so a single description (750 bytes) decompresses in microseconds.

**Scenario 4: Separate runtime file (not compiled in)**

- Ship descriptions as a separate `.zst` or `.sqlite` file alongside the binary.
- File size: ~8-10 MB compressed.
- Not part of the binary at all -- loaded on demand.
- **Binary impact: 0 MB** (plus ~100 KB for the offset index).

**Scenario 5: Fetched on-demand via API**

- No embedded data at all. Use ScreenScraper, TheGamesDB API, or IGDB API to fetch descriptions when the user views a game detail page.
- **Binary impact: 0 MB**.
- Requires network access. Already planned as the primary approach in `game-metadata-sources.md`.

#### Build impact

- **Scenarios 1-3**: Parsing TheGamesDB's ~40 MB JSON dump to extract descriptions: 3-5 seconds. Compressing the blob with zstd: 1-2 seconds. The descriptions would NOT go through PHF -- they'd be a separate compressed blob with an offset table.
- **Scenario 4**: Build.rs generates the compressed file as a build artifact. No compile-time impact on the Rust binary itself.
- **Scenario 5**: No build impact.

#### Storage strategy analysis

| Strategy | Binary Size | Startup Cost | Lookup Cost | Complexity | Offline? |
|----------|------------|-------------|-------------|------------|----------|
| Inline in PHF map (raw strings) | +48 MB | None | O(1) | Low | Yes |
| Truncated (200 chars) in PHF | +16 MB | None | O(1) | Low | Yes |
| Compressed blob (zstd) via `include_bytes!` | +8 MB | ~50 ms decompress index | O(1) + decompress | Medium | Yes |
| Separate .zst file at runtime | +0 MB binary | File open + index load | O(1) + decompress | Medium | Yes |
| Separate SQLite file at runtime | +0 MB binary | DB open (~5 ms) | SQL query | Medium | Yes |
| Fetched on-demand via API | +0 MB | None | Network latency | Low | No |

**Compressed blob details**: The embedded blob approach would work as follows:
1. Build.rs generates a compressed blob: `[offset_table][zstd_compressed_descriptions]`
2. The offset table maps game IDs to (offset, length) pairs within the compressed data.
3. At runtime, to read a description: look up the offset, decompress that slice with zstd.
4. With zstd's seekable format or per-entry compression with a shared dictionary, individual entries can be decompressed without decompressing the entire blob.

**SQLite file details**: An alternative to the compressed blob is a SQLite database file:
1. Build.rs generates a SQLite file with a single table: `descriptions(game_id TEXT PRIMARY KEY, text TEXT)`.
2. Embedded via `include_bytes!` or shipped as a separate file.
3. At runtime, open as an in-memory database (or memory-mapped file).
4. SQLite's built-in page-level compression (via extensions) or external zstd compression can reduce size.
5. Adds `rusqlite`/`libsqlite3-sys` dependency (~1.5 MB binary overhead).

#### Recommendation

**Defer descriptions from the embedded database. Continue with the API-based approach from `game-metadata-sources.md`.**

Rationale:
- Even compressed, descriptions add 8 MB to the binary -- nearly matching the entire metadata DB.
- The value proposition is lower than other fields: descriptions are only shown on the game detail page (not in lists), so the user sees them one at a time.
- API-based fetching (ScreenScraper, TheGamesDB) provides the same data with zero binary cost, and supports multiple languages.
- If offline descriptions are needed in the future, the best approach is **Scenario 4** (separate compressed file) rather than embedding in the binary. This keeps the binary lean while allowing optional description data to be shipped alongside it.

If offline descriptions become a requirement, the recommended approach is:
1. Generate a zstd-compressed descriptions file at build time (~8 MB).
2. Ship it alongside the binary as an optional data file.
3. Load and decompress on demand using an offset index.
4. Fall back to API-based fetching if the file is not present.

---

### A.6 Cumulative Size Summary

| Fields Included | Per-Entry Bytes | Total Size (108K entries) | Notes |
|----------------|---------------:|-------------------------:|-------|
| Baseline (filename, clean_title, region, crc32, year) | ~77 | ~8.3 MB | Current plan (section 5.2) |
| + Players (u8) | +1 | +108 KB | Negligible |
| + Genre (u8 interned index) | +2 | +216 KB + ~3 KB pool | Trivial |
| + Publisher (u16 interned index) | +2 | +216 KB + ~35 KB pool | Already planned |
| + Developer (u16 interned index) | +2 | +216 KB + ~60 KB pool | Moderate value |
| **Total (all four)** | **+7** | **+~850 KB** | **~9.2 MB total binary impact** |
| + Descriptions (compressed blob) | +0 per entry | +8 MB blob | Deferred -- use API instead |
| **Total with descriptions** | | **~17 MB** | Not recommended |

The four structured fields (players, genre, publisher, developer) add only ~850 KB total with string interning. This is a 10% increase over the baseline metadata DB size and well within acceptable limits. Descriptions, by contrast, would nearly double the total size and are better served by the API-based approach.

### A.7 How Other Projects Handle Embedded Game Metadata

- **RetroArch/libretro**: Compiles metadata from multiple DAT sources into .rdb (RetroArch Database) binary files. These are per-system files loaded at runtime, not embedded in the binary. Fields include name, description, genre, developer, publisher, release year, players, CRC/MD5/SHA1. Descriptions are included in the RDB files but RetroArch loads them from disk, not from compiled-in data.

- **OpenEmu (macOS)**: Uses OpenVGDB, a SQLite database (~9 MB compressed) downloaded at first launch. Contains game names, descriptions, genres, publishers, developers, and ROM hashes. Not compiled into the binary.

- **EmulationStation**: Uses gamelist.xml files generated by scrapers (Skyscraper, Selph's scraper). These are per-system XML files on disk containing name, description, developer, publisher, genre, players, release date, and image paths. Not embedded in the binary.

- **Pegasus Frontend**: Uses metadata files (metafile.txt or gamelist.xml) generated by scrapers. Similar approach to EmulationStation -- per-system files on disk.

**Common pattern**: No major emulator frontend embeds game descriptions in the binary. They all use either runtime files (SQLite, XML, custom binary) or API-based fetching. The Replay approach of embedding identification + basic metadata (name, year, publisher, genre, players) in the binary while fetching rich content (descriptions, media) via API is consistent with industry practice, but goes further by using PHF for zero-cost lookups rather than runtime-loaded databases.

---

## Appendix B: Multi-ROM Game Grouping Strategy

This appendix analyzes how the embedded metadata database should handle the relationship between multiple ROM files (regional variants, revisions, translations) and a single canonical "game" entity. This is about the data model in the embedded DB -- the UI grouping behavior is described in `rom-identification.md` section 4 and `features.md`.

### B.1 The Problem

A single game like "Super Mario World" exists as multiple ROM files in a No-Intro set:

```
Super Mario World (USA).sfc                         CRC: B19ED489
Super Mario World (Europe).sfc                       CRC: 6B47BB75
Super Mario World (Europe) (Rev 1).sfc               CRC: A1B0E19C
Super Mario World (Japan).sfc                        CRC: 47DC3788
Super Mario World (USA) (Virtual Console).sfc        CRC: ...
```

Each of these is a separate entry in the No-Intro DAT file with a distinct CRC32 hash. In the current metadata DB design (section 4), each gets its own entry in the PHF map with its own copy of metadata:

```
"Super Mario World (USA)"          -> { clean_title: "Super Mario World", year: 1992, publisher: "Nintendo", genre: "Platform", players: 2 }
"Super Mario World (Europe)"       -> { clean_title: "Super Mario World", year: 1992, publisher: "Nintendo", genre: "Platform", players: 2 }
"Super Mario World (Europe) (Rev 1)" -> { clean_title: "Super Mario World", year: 1992, publisher: "Nintendo", genre: "Platform", players: 2 }
"Super Mario World (Japan)"        -> { clean_title: "Super Mario World", year: 1992, publisher: "Nintendo", genre: "Platform", players: 2 }
```

The metadata (year, publisher, genre, players) is identical across all variants. This is wasteful -- the same strings (or interned indices) are repeated for every variant.

More importantly, when cross-referencing against TheGamesDB to obtain metadata, TheGamesDB has **one entry** for "Super Mario World" on SNES (possibly with separate entries per platform, but not per regional variant). We need to match multiple No-Intro filenames to a single TheGamesDB entry.

### B.2 Scale of the Problem

Based on the data from section 3.1 of this document:

| Category | Full Entries (all variants) | Est. Unique Games | Variant Ratio |
|----------|---------------------------:|------------------:|--------------:|
| Cartridge systems | ~57,500 | ~23,500 | 2.4:1 |
| Disc systems | ~22,300 | ~7,200 | 3.1:1 |
| **Total (non-arcade)** | **~79,800** | **~30,700** | **2.6:1** |
| Arcade (existing DB) | 28,593 | ~10,000 | 2.9:1 |
| **Grand total** | **~108,400** | **~40,700** | **2.7:1** |

On average, each unique game has ~2.7 ROM entries in the full No-Intro/Redump sets. Some games have far more: popular titles like "Tetris" or "Super Mario Bros." may have 10-20 variants (multiple regions, revisions, special editions, Virtual Console re-releases).

The ratios vary significantly by system:

| System | Full Entries | Est. Unique | Ratio | Notes |
|--------|------------:|------------:|------:|-------|
| NES | ~14,100 | ~4,500 | 3.1:1 | Many unlicensed/bootleg/region variants |
| SNES | ~4,300 | ~1,800 | 2.4:1 | Typical ratio |
| N64 | ~2,700 | ~400 | 6.8:1 | Small library, many regional variants |
| Game Boy | ~2,200 | ~800 | 2.8:1 | |
| PlayStation | ~13,200 | ~4,000 | 3.3:1 | Disc systems have more variants |
| Mega Drive | ~3,900 | ~1,500 | 2.6:1 | |

**Key insight**: Deduplicating metadata by storing it once per unique game rather than once per ROM variant would save roughly 60% of the metadata storage for string fields like clean_title, publisher, genre, and developer.

### B.3 No-Intro Parent/Clone Relationships

No-Intro DATs support a `cloneof` attribute in XML-format parent/clone DATs, directly analogous to MAME's parent/clone system:

```xml
<game name="Super Mario World (Europe)">
  <description>Super Mario World (Europe)</description>
  <rom name="Super Mario World (Europe).sfc" size="524288" crc="6B47BB75" ... />
</game>

<game name="Super Mario World (USA)" cloneof="Super Mario World (Europe)">
  <description>Super Mario World (USA)</description>
  <rom name="Super Mario World (USA).sfc" size="524288" crc="B19ED489" ... />
</game>
```

**Availability**: No-Intro parent/clone DATs are available from DAT-o-MATIC (requires a free account) in XML format. The standard DATs mirrored in libretro-database (`metadat/no-intro/`) use ClrMamePro format which does NOT include parent/clone information. To get parent/clone data, you must download the XML-format P/C DATs from DAT-o-MATIC directly.

**How parent selection works in No-Intro**: Unlike MAME (where the parent is typically the World or most recent version), No-Intro's parent designation is somewhat arbitrary -- it indicates a grouping relationship, not a preferred version. The Retool project explicitly ignores No-Intro's parent/clone assignments and instead does its own title-based grouping, because the parent/clone assignments in No-Intro DATs can be inconsistent or incomplete.

**Coverage**: Parent/clone DATs are not available for all systems. The major systems (NES, SNES, Mega Drive, Game Boy, etc.) have them, but coverage is not universal.

**Verdict**: No-Intro P/C DATs are useful as a supplementary grouping signal, but cannot be the sole grouping mechanism. Title-based normalization is more reliable and universally available.

### B.4 Grouping Approaches Analyzed

#### Approach 1: Title Normalization (Recommended Primary Method)

Strip region tags, revision markers, and flags from No-Intro filenames to derive a base title, then use that as the grouping key.

**Algorithm** (already described in `rom-identification.md` section 4):
```
"Super Mario World (USA)"              -> "super mario world"
"Super Mario World (Europe) (Rev 1)"   -> "super mario world"
"Super Mario World (Japan)"            -> "super mario world"
"Sonic the Hedgehog (USA, Europe)"     -> "sonic hedgehog"
"Sonic the Hedgehog (Japan)"           -> "sonic hedgehog"
```

**Reliability**: Very high for No-Intro-named files. No-Intro's naming convention ensures that regional variants of the same game have identical titles before the first `(`. The `rom-identification.md` parser handles this correctly.

**Edge cases**:
- **Subtitle differences**: "Contra (USA)" vs "Probotector (Europe)" -- different titles for the same game in different regions. Title normalization alone won't group these. Requires a manual override list or cross-referencing against TheGamesDB.
- **Numbering conventions**: "Final Fantasy III (USA)" is actually "Final Fantasy VI (Japan)". Again, requires manual mapping.
- **"Name, The" normalization**: "Legend of Zelda, The (USA)" and "Zelda no Densetsu (Japan)" -- different titles entirely. Cannot be grouped by title alone.

**Estimated accuracy**: ~85-90% of No-Intro entries correctly group by title normalization alone. The remaining 10-15% are regional title differences that require supplementary data.

#### Approach 2: No-Intro Parent/Clone DATs (Supplementary)

Use the `cloneof` attribute from No-Intro XML P/C DATs to establish groupings.

**Pros**: Authoritative groupings maintained by the No-Intro community. Handles regional title differences (e.g., "Contra" / "Probotector" will share a parent).

**Cons**: Requires downloading separate DAT files from DAT-o-MATIC (not available in the libretro mirror). Not available for all systems. Parent designation is somewhat arbitrary.

**Recommended use**: Parse P/C DATs at build time as a supplementary grouping signal. When title normalization fails to group variants that the P/C DAT says belong together, use the P/C relationship.

#### Approach 3: TheGamesDB Cross-Reference (Supplementary)

TheGamesDB entries are per-game, not per-ROM. When the build.rs cross-references No-Intro entries against TheGamesDB by title+platform, multiple No-Intro entries will match the same TheGamesDB entry. The TheGamesDB ID becomes a natural grouping key.

**Pros**: Provides a canonical game ID from an external source. Handles some regional title differences if TheGamesDB has alternate titles.

**Cons**: Only works for entries that match TheGamesDB (~60-70%). Name matching is fuzzy and may produce false positives (e.g., "Mega Man" matching "Mega Man 2").

**Recommended use**: When a TheGamesDB match is found, record the TGDB ID as the canonical game ID. Multiple ROM entries sharing the same TGDB ID are grouped together.

#### Approach 4: Hash-Based Grouping (Implicit)

No-Intro DATs list all known dumps per game. Different hashes = different ROMs, but the DAT groups them under the same `game` element (each `game` element can have multiple `rom` elements for multi-file games, though this is rare for cartridge systems).

**Relevance**: This is already handled by the DAT parsing -- each `game` element produces one DB entry. The "multiple ROMs per game" problem is actually "multiple `game` elements per logical game" (due to regional variants being separate `game` entries in the DAT).

#### Approach 5: Retool's Clone Lists (Supplementary Reference)

The Retool project (a 1G1R tool for No-Intro/Redump) maintains JSON-based clone lists that manually define groupings for titles that automatic title matching misses. These clone lists are open source and cover the most problematic cases (regional title differences, compilations, supersets).

**Value**: Retool's clone lists are a curated, high-quality source of grouping overrides. They can be parsed at build time to supplement title-based normalization.

**Location**: `https://github.com/unexpectedpanda/retool` -- clone lists are in the `clonelists/` directory.

### B.5 Recommended Data Model

#### Two-Level Structure: Game ID + ROM Entries

Instead of storing full metadata per ROM filename, use a normalized two-level model:

**Level 1: Canonical Games** (one per unique game per system)

```rust
pub struct CanonicalGame {
    /// Unique game ID (u32, assigned at build time).
    pub game_id: u32,
    /// Clean display title.
    pub display_name: &'static str,
    /// Release year (0 = unknown).
    pub year: u16,
    /// Publisher index into interning pool (0xFFFF = unknown).
    pub publisher: u16,
    /// Developer index into interning pool (0xFFFF = unknown).
    pub developer: u16,
    /// Genre index into interning pool (0xFF = unknown).
    pub genre: u8,
    /// Max players (0 = unknown).
    pub players: u8,
}
```

**Per-entry size**: 4 (id) + 8 (ptr+len for display_name on 32-bit, or just an offset) + 2 (year) + 2 (publisher) + 2 (developer) + 1 (genre) + 1 (players) = **~12-20 bytes** per canonical game.

**Count**: ~30,700 unique games (non-arcade) or ~40,700 including arcade.

**Total**: 30,700 x 16 = **~490 KB** for the canonical game table.

**Level 2: ROM Entries** (one per ROM filename in the No-Intro/Redump sets)

```rust
pub struct RomEntry {
    /// Game ID linking to CanonicalGame.
    pub game_id: u32,
    /// Region code.
    pub region: &'static str,
    /// CRC32 for hash-based matching.
    pub crc32: u32,
}
```

**Per-entry size**: 4 (game_id) + 6 (region string avg) + 4 (crc32) + 3 (PHF overhead) = **~17 bytes** per ROM entry.

**Count**: ~79,800 ROM entries (non-arcade).

**Total**: 79,800 x 17 = **~1.35 MB** for ROM entry maps.

#### PHF Map Structure

```rust
// Per-system generated code

/// Canonical game table: game_id -> metadata
static NES_GAMES: &[CanonicalGame] = &[ ... ];  // ~4,500 entries for NES

/// ROM filename -> (game_id, region, crc32)
static NES_ROM_DB: phf::Map<&'static str, RomEntry> = ...;  // ~14,100 entries for NES

/// CRC32 -> canonical filename (for hash-based fallback)
static NES_CRC_INDEX: phf::Map<u32, &'static str> = ...;  // ~14,100 entries
```

The canonical game table is a simple array (not a PHF map) indexed by a per-system sequential `game_id`. The ROM-to-game mapping uses PHF for O(1) filename lookup.

#### Lookup Flow

```
1. User has: "Super Mario World (USA).sfc"
2. Strip extension: "Super Mario World (USA)"
3. PHF lookup in NES_ROM_DB: -> RomEntry { game_id: 1234, region: "USA", crc32: 0xB19ED489 }
4. Index into NES_GAMES[1234]: -> CanonicalGame { display_name: "Super Mario World", year: 1992, ... }
5. Return combined info
```

### B.6 Size Impact of Deduplication

#### Current approach (denormalized): metadata repeated per ROM entry

Per the existing plan (section 4.3), each of ~108K entries carries ~111 bytes including all metadata strings. This includes the clean_title string repeated across all variants of the same game.

- **Total (denormalized)**: 108,000 x 111 = **~11.7 MB**

#### Proposed approach (normalized): metadata stored once per canonical game

- **Canonical game table**: 40,700 x 16 = **~650 KB** (with interned strings for publisher/developer/genre)
- **ROM entry maps**: 108,000 x 17 = **~1.84 MB** (filename key + game_id + region + crc32)
- **CRC index maps**: 108,000 x 11 = **~1.19 MB** (crc32 key + canonical filename reference)
- **String interning pools**: ~100 KB (genres + publishers + developers)
- **Display name strings**: 40,700 x 25 = **~1.02 MB** (one copy per game, not per variant)
- **Canonical name strings**: 108,000 x 40 = **~4.32 MB** (still needed for filename-based lookup keys, cannot be deduplicated)
- **Region strings**: with interning (~20 unique regions), negligible
- **PHF overhead**: 108,000 x 3 = **~324 KB** (ROM maps) + 108,000 x 3 = **~324 KB** (CRC maps) + 40,700 x 0 = 0 (canonical game table is array, not PHF)

**Total (normalized)**: 650 KB + 1.84 MB + 1.19 MB + 100 KB + 1.02 MB + 4.32 MB + 648 KB = **~9.8 MB**

#### Comparison

| Approach | Binary Size | Metadata per entry | Unique metadata copies |
|----------|------------|-------------------|----------------------|
| Denormalized (current plan) | ~11.7 MB | ~111 bytes | 108,000 |
| Normalized (two-level) | ~9.8 MB | ~17 bytes + shared game | 40,700 |
| **Savings** | **~1.9 MB (16%)** | | |

The savings from normalization are moderate (~16%) because the dominant cost is the PHF map keys (canonical filenames), which cannot be deduplicated -- each ROM filename is unique. The savings come primarily from not repeating display_name, publisher, genre, developer, and players across regional variants.

**Verdict**: The 16% size reduction is welcome but not transformative. The stronger argument for normalization is **data quality**: storing metadata once per game ensures consistency (you can't have "Super Mario World (USA)" say publisher "Nintendo" while "Super Mario World (Europe)" says publisher "Nintendo EAD" due to a cross-referencing mismatch).

### B.7 Grouping Algorithm for Build-Time Processing

The build.rs pipeline should assign canonical game IDs using this algorithm:

```
1. Parse all No-Intro/Redump DAT entries for a system.
2. For each entry, derive a group_key by:
   a. Strip everything from the first '(' onward.
   b. Trim whitespace.
   c. Normalize articles ("Name, The" -> "The Name").
   d. Lowercase.
   e. Remove punctuation.
   f. Collapse whitespace.
3. Group entries by (system, group_key).
4. OPTIONAL: Parse No-Intro P/C DATs. For entries grouped differently
   by P/C data vs. title normalization, prefer P/C grouping.
5. OPTIONAL: Parse Retool clone lists for override cases
   (regional title differences like Contra/Probotector).
6. Assign a sequential game_id to each group.
7. Cross-reference each group against TheGamesDB by title + platform
   to obtain metadata (year, publisher, genre, players).
8. Store: one CanonicalGame per group, one RomEntry per DAT entry.
```

**Build time impact**: Steps 2-3 are trivial string manipulation. Step 4 adds P/C DAT parsing (~1-2 seconds). Step 5 adds clone list JSON parsing (<1 second). Steps 6-7 are already planned. Total additional build time: **~2-3 seconds**.

### B.8 Handling Non-Standard Filenames

The two-level model handles non-standard filenames naturally:

1. **Standard No-Intro filename**: Direct PHF lookup in ROM_DB succeeds. Get game_id, look up canonical game.
2. **Non-standard filename** (e.g., `smw.sfc`): PHF lookup fails. Fall back to CRC32 hash computation.
3. **CRC32 hash match**: Look up CRC in CRC_INDEX. Get canonical filename, then PHF lookup, then game_id.
4. **No match**: Fall back to filename parser from `rom-identification.md`. The parser extracts a clean title, which can be fuzzy-matched against canonical game display names.

This is the same fallback chain described in section 7 of the main document, but now each step resolves to a canonical `game_id` rather than duplicated metadata.

### B.9 Impact on the Phased Rollout Plan

The two-level normalized model can be adopted incrementally:

**Phase 1** (identification layer): No change needed. Phase 1 only embeds filenames and derived clean_titles -- there's no rich metadata to deduplicate yet. Title normalization already happens at this phase, so group_keys are available.

**Phase 2** (TheGamesDB cross-reference): This is where normalization pays off. Instead of cross-referencing each ROM filename against TheGamesDB individually (and potentially getting different matches for variants of the same game), cross-reference once per group_key. Store the result in the CanonicalGame table. This improves match rates (grouping variants increases the chance that at least one title matches TheGamesDB) and ensures consistency.

**Phase 3** (CRC32 hash index): No change. The CRC index maps to canonical filenames, which resolve to game_ids via the ROM_DB.

**Phase 4-5** (disc systems, remaining systems): The normalized model scales naturally. Adding new systems means adding new per-system canonical game tables and ROM maps.

### B.10 Recommendation

**Adopt the two-level normalized model from Phase 2 onward.**

- **Phase 1**: Keep the simpler denormalized model (filename -> clean_title + region). The overhead of normalization isn't justified when there's no rich metadata to deduplicate.
- **Phase 2**: When adding TheGamesDB metadata, switch to the two-level model. Use title normalization as the primary grouping algorithm, supplemented by No-Intro P/C DATs where available. Store metadata once per canonical game.
- **Grouping accuracy**: Title normalization handles ~85-90% of cases. No-Intro P/C DATs handle most of the remaining cases. A small manual override list (inspired by Retool's clone lists) handles the remaining edge cases (regional title differences).
- **Size savings**: ~1.9 MB (16%) compared to the denormalized approach. More importantly, metadata consistency is guaranteed.
- **Implementation complexity**: Moderate. The build.rs pipeline needs a grouping step before cross-referencing, and the runtime API adds an indirection (ROM -> game_id -> metadata). But the actual code is straightforward -- a HashMap for grouping at build time, and an array index at runtime.
