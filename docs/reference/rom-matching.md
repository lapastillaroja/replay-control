# ROM Matching

> **Note**: See also `docs/features/metadata.md` and `docs/features/thumbnails.md` for current-state documentation of matching and metadata resolution.

How Replay identifies games, resolves display names, and matches ROMs to external metadata. This documents the **implemented** logic as of March 2026.

---

## Overview

Replay uses a multi-layer matching system to turn raw ROM files into identified games with rich metadata. The layers, from fastest to richest:

1. **Embedded databases** (compiled into the binary, always available, nanosecond lookup)
2. **External metadata cache** (SQLite on disk, microsecond lookup, populated by user-triggered downloads)

Each layer is independent — the app works fine with only embedded data, and external metadata enriches it when available.

```
ROM file on disk
  │
  ├── Arcade? ──→ arcade_db (PHF map, 28K+ entries)
  │                 └── ArcadeGameInfo: display_name, year, manufacturer, players, ...
  │
  └── Console? ──→ game_db (PHF map, 34K+ entries across 20+ systems)
                    ├── Exact filename stem match → GameEntry
                    ├── CRC32 fallback → GameEntry
                    └── Normalized title fallback → CanonicalGame
  │
  └── External cache ──→ metadata_db (SQLite)
                          └── GameMetadata: description, rating, publisher, images
```

---

## Layer 1: Embedded Databases

### arcade_db (arcade systems)

**Scope:** `arcade_mame`, `arcade_fbneo`, `arcade_mame_2k3p`, `arcade_dc`

**Key insight:** Arcade ROMs use MAME codenames as filenames (`sf2.zip`, `mslug6.zip`). These are stable identifiers — the same codename means the same game across all MAME-based emulators.

**Lookup:** `arcade_db::lookup_arcade_game(rom_name)` — O(1) PHF map keyed by ROM name without `.zip` extension.

**Data model:**
```rust
pub struct ArcadeGameInfo {
    pub rom_name: &'static str,       // "sf2"
    pub display_name: &'static str,   // "Street Fighter II: The World Warrior"
    pub year: &'static str,           // "1991"
    pub manufacturer: &'static str,   // "Capcom"
    pub players: u8,                  // 2
    pub rotation: Rotation,           // Horizontal | Vertical
    pub status: DriverStatus,         // Working | Imperfect | Preliminary
    pub is_clone: bool,               // false
    pub parent: &'static str,         // "" (empty for non-clones)
    pub category: &'static str,       // "Fighter / 2D"
    pub normalized_genre: &'static str, // "Fighting"
}
```

**Build-time data sources** (merged in this order, later sources override earlier ones):

| Source | Entries | Priority | Notes |
|--------|---------|----------|-------|
| Flycast CSV | ~300 | Highest (never overridden) | Hand-curated Naomi/Atomiswave data |
| FBNeo DAT | ~8K | Base | Name, year, manufacturer, parent/clone |
| MAME 2003+ XML | ~5K | Overrides FBNeo | Adds players, rotation, driver status |
| MAME current (0.285) XML | ~27K | Overrides 2003+ (not Flycast) | Most complete and up-to-date |
| catver.ini (2003+) | ~5K | Category overlay | Applied only to entries without category |
| catver-mame-current.ini | ~48K | Category overlay | Supplements 2003+ categories |

**Total:** ~28,593 unique entries after merge.

### game_db (non-arcade systems)

**Scope:** 20+ systems including `nintendo_nes`, `nintendo_snes`, `nintendo_gb`, `nintendo_gbc`, `nintendo_gba`, `nintendo_n64`, `nintendo_ds`, `sega_sms`, `sega_smd`, `sega_gg`, `sega_32x`, `nec_pce`, `atari_2600`, `atari_7800`, `atari_lynx`, `snk_ng`, `snk_ngp`, and more.

**Key insight:** Non-arcade ROMs use human-readable filenames following the No-Intro naming convention: `Legend of Zelda, The - A Link to the Past (USA).sfc`. The filename itself encodes the game title, region, revision, and more.

**Lookup chain** (three-step fallback):

1. **Exact filename stem** — `game_db::lookup_game(system, stem)` — O(1) PHF map. The stem is the filename without extension (e.g., `Super Mario World (USA)`). Returns a `GameEntry` with region, CRC32, and a reference to the canonical game.

2. **CRC32 fallback** — `game_db::lookup_by_crc(system, crc32)` — for files whose names don't match No-Intro convention but whose contents do. Requires reading the file to compute the hash.

3. **Normalized title fallback** — `game_db::lookup_by_normalized_title(system, normalized)` — strips parenthetical tags, lowercases, removes punctuation. Matches across naming variants (e.g., renamed ROMs, different conventions).

**Data model:**
```rust
// Shared metadata for one unique game per system
pub struct CanonicalGame {
    pub canonical_name: &'static str,  // "Super Mario World"
    pub year: u16,                      // 1990
    pub genre: &'static str,           // "Platform"
    pub developer: &'static str,       // "Nintendo"
    pub players: u8,                   // 2
    pub normalized_genre: &'static str, // "Platform"
}

// One entry per ROM filename variant
pub struct GameEntry {
    pub game: &'static CanonicalGame,
    pub region: &'static str,          // "USA"
    pub crc32: u32,
}
```

**Build-time data sources:**

| Source | Role |
|--------|------|
| No-Intro DATs | ROM identification: filename stems, regions, CRC32s |
| TheGamesDB JSON dump | Enrichment: year, genre, developer, players |
| libretro-database DATs | Supplementary: genre, players |

**Total:** ~34,064 ROM entries, ~15,767 canonical games across 9 systems.

### Filename normalization (game_db)

`game_db::normalize_filename(stem)`:

1. Strip everything from first `(` or `[` onward
2. Keep only lowercase alphanumeric + spaces
3. Collapse whitespace

```
"Super Mario World (USA)"           → "super mario world"
"Legend of Zelda, The (USA) (Rev 1)" → "legend of zelda the"
```

---

## Layer 2: External Metadata (LaunchBox Import)

External metadata provides descriptions, ratings, and publisher info not available in the embedded databases. It's stored in a SQLite cache at `<storage_root>/.replay-control/metadata.db`.

### Data model

```rust
pub struct GameMetadata {
    pub description: Option<String>,    // Game synopsis
    pub rating: Option<f64>,            // Community rating
    pub publisher: Option<String>,
    pub source: String,                 // "launchbox"
    pub fetched_at: i64,                // Unix timestamp
    pub box_art_path: Option<String>,   // Relative path to image
    pub screenshot_path: Option<String>,
}
```

**Schema:** Primary key is `(system, rom_filename)`. Uses WAL mode with `nolock` VFS fallback for NFS mounts.

### The matching problem

LaunchBox provides metadata keyed by **human-readable game title + platform name** (e.g., "Street Fighter II: The World Warrior" on "Arcade"). ROMs on disk are identified by **filenames** which may be:

- **MAME codenames** (arcade): `sf2.zip` — not human-readable
- **No-Intro names** (console): `Legend of Zelda, The - A Link to the Past (USA).sfc` — human-readable but with tags and reordered articles

The import process bridges this gap using a **ROM index** and **title normalization**.

### Import pipeline

```
Step 1: Build ROM Index (scan filesystem)
  For each ROM file on disk:
    ├── Arcade system? → Look up arcade_db display name → normalize → index
    └── Console system? → Strip extension → normalize filename stem → index
  Result: HashMap<(system_folder, normalized_title), Vec<rom_filename>>

Step 2: Parse LaunchBox XML (stream ~460 MB)
  For each <Game> entry:
    ├── Map platform name → system folder(s) via platform_map()
    ├── Normalize game title
    └── Look up in ROM index → if found, batch insert into metadata_db

Step 3: Bulk upsert to SQLite (batches of 500)
```

### Step 1: Building the ROM index

`launchbox::build_rom_index(storage_root)` scans all system directories under `<storage_root>/roms/` and builds a lookup table.

**Non-arcade systems** — the filename stem is normalized directly:

```
roms/nintendo_snes/Legend of Zelda, The - A Link to the Past (USA).sfc
  stem: "Legend of Zelda, The - A Link to the Past (USA)"
  normalized: "thelegendofzeldaalinktothepast"
  index key: ("nintendo_snes", "thelegendofzeldaalinktothepast")
  value: ["Legend of Zelda, The - A Link to the Past (USA).sfc"]
```

**Arcade systems** — MAME codenames must be translated to display names first:

```
roms/arcade_mame/Horizontal/00 Clean Romset/sf2.zip
  stem: "sf2"
  arcade_db::lookup_arcade_game("sf2") → "Street Fighter II: The World Warrior"
  normalized: "streetfighteriitheworldwarrior"
  index key: ("arcade_mame", "streetfighteriitheworldwarrior")
  value: ["sf2.zip"]
```

Without this translation, `sf2` would never match the LaunchBox entry for "Street Fighter II: The World Warrior".

**Clone fallback** — arcade clones are also indexed under their parent's display name:

```
roms/arcade_fbneo/Horizontal/00 Clean Romset/sf2ce.zip
  stem: "sf2ce"
  arcade_db → display_name: "Street Fighter II': Champion Edition"
  parent: "sf2" → parent display_name: "Street Fighter II: The World Warrior"

  Indexed under BOTH:
    ("arcade_fbneo", "streetfighteriiichampionedition") → ["sf2ce.zip"]
    ("arcade_fbneo", "streetfighteriitheworldwarrior")  → ["sf2ce.zip"]
```

This ensures clones get metadata even when LaunchBox only has an entry for the parent game.

Arcade system detection uses a hardcoded list: `arcade_mame`, `arcade_fbneo`, `arcade_mame_2k3p`, `arcade_dc`.

### Title normalization (LaunchBox matching)

`launchbox::normalize_title(name)` — used for both ROM index keys and LaunchBox entry titles:

1. **Strip parenthetical/bracket tags** — remove everything in `(...)` and `[...]`, including nested groups
2. **Reorder articles** — handle No-Intro's `"Title, The"` → `"The Title"` convention (also `A`, `An`)
3. **Keep only lowercase alphanumeric** — strip all punctuation, spaces, and special characters

```
"Street Fighter II: The World Warrior"  → "streetfighteriitheworldwarrior"
"Legend of Zelda, The (USA)"            → "thelegendofzelda"
"Super Mario Bros. 3 (USA) (Rev A)"     → "supermariobros3"
"Metal Slug 6"                          → "metalslug6"
```

Both sides (ROM filenames and LaunchBox titles) pass through the same normalization, so minor differences in punctuation, spacing, or article placement are handled.

### Step 2: Platform mapping

`launchbox::platform_map()` maps LaunchBox platform names to system folder names. A single platform can map to multiple folders:

| LaunchBox Platform | System Folder(s) |
|--------------------|-------------------|
| `"Arcade"` | `arcade_mame`, `arcade_fbneo`, `arcade_mame_2k3p` |
| `"Sammy Atomiswave"` | `arcade_dc` |
| `"Sega Naomi"` / `"Sega Naomi 2"` | `arcade_dc` |
| `"Nintendo Entertainment System"` | `nintendo_nes` |
| `"Super Nintendo Entertainment System"` | `nintendo_snes` |
| `"Sega Genesis"` / `"Sega Mega Drive"` | `sega_smd` |
| ... | (~45 mappings total) |

When a LaunchBox entry matches a platform with multiple folders (e.g., "Arcade"), the matching callback fires once per folder, so the entry is matched against ROMs in all three arcade folders.

### Step 3: Matching and insertion

For each LaunchBox `<Game>` entry with a recognized platform:
1. Skip if no useful data (empty overview AND no rating)
2. Normalize the game's `<Name>`
3. Look up `(system_folder, normalized_title)` in the ROM index
4. If found, create a `GameMetadata` for each matching ROM filename
5. Batch upsert to SQLite (every 500 entries)

**Import stats example (real data):**
```
Source entries:  93,300 (LaunchBox games with recognized platforms)
Matched:         8,501 (found in ROM index)
Inserted:       17,043 (some matches have multiple ROM files)
Skipped:         4,686 (entries with no description or rating)
```

---

## Display Name Resolution

When listing ROMs for the UI, `GameRef::new()` resolves the display name:

**Arcade systems:**
1. `arcade_db::arcade_display_name(filename)` — looks up by zip name, falls back to filename

**Non-arcade systems:**
1. `game_db::game_display_name(system, filename)` — tries exact stem match, then normalized title, then tilde-split for multi-disc
2. If no match: strip filename tags (remove `(...)` and `[...]`) and use the base name
3. Append useful tags via `rom_tags::display_name_with_tags()` — adds region, revision, translation info as a suffix

```
"Legend of Zelda, The - A Link to the Past (USA).sfc"
  → game_db match → "The Legend of Zelda: A Link to the Past"
  → with tags → "The Legend of Zelda: A Link to the Past (USA)"

"sf2.zip"
  → arcade_db → "Street Fighter II: The World Warrior"
```

---

## GameInfo: Unified API Response

Server functions return a single `GameInfo` struct regardless of data source. The `resolve_game_info()` function in `server_fns/mod.rs` is the only place that branches on arcade vs. non-arcade:

```rust
pub struct GameInfo {
    // Identity (always present)
    pub system: String,
    pub rom_filename: String,
    pub display_name: String,

    // Embedded metadata
    pub year: String,
    pub genre: String,
    pub developer: String,    // manufacturer for arcade
    pub players: u8,

    // Arcade-specific
    pub rotation: Option<String>,
    pub driver_status: Option<String>,
    pub is_clone: Option<bool>,
    pub parent_rom: Option<String>,

    // Console-specific
    pub region: Option<String>,

    // External metadata (from metadata_db, None if not downloaded)
    pub description: Option<String>,
    pub rating: Option<f32>,
    pub box_art_url: Option<String>,
    pub screenshot_url: Option<String>,
}
```

Resolution chain:
1. **Embedded DB** (always available) → display name, year, genre, developer, players
2. **External metadata cache** (if downloaded) → description, rating, publisher, images
3. **Fallback** → filename stem as display name, fields as None/empty

---

## Coverage Results

As of March 2026, with ~22K ROMs across 16 systems:

### Embedded metadata
| Field | Coverage |
|-------|----------|
| Display Name | 17,372 / 22,356 (77.7%) |
| Year | 15,452 / 22,356 (69.1%) |
| Genre / Category | 16,584 / 22,356 (74.2%) |
| Players | 16,664 / 22,356 (74.5%) |

### External metadata (after LaunchBox import)
| Field | Coverage |
|-------|----------|
| Description | 14,873 / 22,356 (66.5%) |
| Rating | 15,683 / 22,356 (70.2%) |
| Publisher | 14,605 / 22,356 (65.3%) |

### Arcade matching improvement

The arcade display-name bridge (using `arcade_db` to translate MAME codenames to human titles before matching against LaunchBox) dramatically improved arcade coverage:

| System | Before | After |
|--------|--------|-------|
| arcade_fbneo (4,082 ROMs) | 0% external match | **89.2%** |
| arcade_mame (4,605 ROMs) | 5.8% external match | **67.8%** |
| arcade_dc (202 ROMs) | Not mapped | **56.9%** |

---

## Key Source Files

| File | Role |
|------|------|
| `replay-control-core/src/arcade_db.rs` | Arcade PHF map + lookup functions |
| `replay-control-core/src/game_db.rs` | Console PHF maps + lookup chain |
| `replay-control-core/src/launchbox.rs` | LaunchBox XML import, ROM index building, title normalization |
| `replay-control-core/src/metadata_db.rs` | SQLite cache for external metadata |
| `replay-control-core/src/game_ref.rs` | Display name resolution (`GameRef::new`) |
| `replay-control-core/src/rom_tags.rs` | Tag extraction for display suffixes |
| `replay-control-core/src/roms.rs` | ROM listing and filesystem scanning |
| `replay-control-core/src/systems.rs` | System definitions and categories |
| `replay-control-core/build.rs` | Build-time generation of game_db and arcade_db |
| `replay-control-app/src/server_fns/mod.rs` | `resolve_game_info()` — unified API response |
| `replay-control-core/src/bin/metadata_report.rs` | Coverage analysis tool |
