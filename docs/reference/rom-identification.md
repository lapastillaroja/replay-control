# ROM Identification, Grouping & Search

> **Status**: Partially implemented. See `docs/features/metadata.md` (ROM tag parsing) and `docs/features/search.md` (search scoring). ROM tag parsing is in `rom_tags.rs` (tier classification, region priority, tag extraction for display names) but uses a simpler `classify()`/`extract_tags()` API rather than the full `RomTags` struct proposed here. The `game_db` module handles the embedded metadata database (section 7). Search scoring with region preference is in `server_fns/search.rs`.

Design document for parsing non-arcade ROM filenames, grouping variants of the same game, improving search, and bridging to external metadata sources.

**Scope:** Non-arcade systems only. Arcade ROMs use zip-name-based lookup via the existing `arcade_db` module.

---

## 1. Naming Conventions

Non-arcade ROMs follow two dominant naming conventions: **No-Intro** (modern standard) and **GoodTools** (legacy). Both encode metadata in the filename using parentheses `()` and brackets `[]`.

### No-Intro Format

The current standard, maintained by the No-Intro group. Used by ROM preservation projects, Redump, and most modern ROM sets.

```
Title (Region) [(Region2)] [(Languages)] [(Revision)] [(Flags...)].ext
```

**Rules:**
- **Title** comes first, as-is. Articles are NOT moved: `Legend of Zelda, The - A Link to the Past` (note: comma + article, then ` - ` separator for subtitle).
- **Parenthesized groups** `(...)` contain official metadata: region, revision, languages, status flags.
- **Order matters:** Region always comes first after the title, then languages (if different from region default), then revision, then other flags.
- Multiple values within one group are comma-separated: `(USA, Europe)`.

**Region codes:** `USA`, `Europe`, `Japan`, `World`, `Australia`, `Brazil`, `Canada`, `China`, `France`, `Germany`, `Hong Kong`, `Italy`, `Korea`, `Netherlands`, `Spain`, `Sweden`, plus multi-region combos like `(USA, Europe)`.

**Revision:** `(Rev 1)`, `(Rev 2)`, `(Rev A)`, `(Rev B)`.

**Version:** `(v1.0)`, `(v1.1)`, `(v2.0)`.

**Status flags (parenthesized):**
- `(Beta)`, `(Proto)`, `(Sample)`, `(Demo)` -- pre-release status
- `(Unl)` -- unlicensed
- `(Virtual Console)` -- digital re-release
- `(Pirate)` -- bootleg

**Examples:**
```
Super Mario World (USA).sfc
Super Mario World (Europe) (Rev 1).sfc
Sonic the Hedgehog (USA, Europe).zip
Legend of Zelda, The - A Link to the Past (USA).sfc
Street Fighter II Turbo - Hyper Fighting (USA) (Rev 1).sfc
Mega Man X (USA) (Beta).sfc
Tetris (Japan) (En) (Virtual Console).gb
Pokemon - Red Version (USA, Europe) (SGB Enhanced).gb
```

### GoodTools Format

Legacy convention from the GoodTools ROM manager (GoodNES, GoodSNES, etc.). Still found in older ROM collections. Uses brackets `[]` for dump status and hack flags.

**Bracket codes:**
- `[!]` -- verified good dump
- `[b]` or `[b1]`, `[b2]` -- bad dump (with variant number)
- `[a1]`, `[a2]` -- alternate dump
- `[o1]`, `[o2]` -- overdump
- `[f1]`, `[f2]` -- fixed (header or otherwise)
- `[h1]`, `[h2]` -- hack (generic)
- `[p1]` -- pirate
- `[t1]`, `[t2]` -- trainer
- `[T+Spa]`, `[T-Spa]` -- translation (+ = newer/complete, - = older/partial), followed by language code
- `[T+Eng1.0_AuthorName]` -- translation with version and author

**Parenthesized info** in GoodTools follows similar patterns to No-Intro: `(U)` for USA, `(E)` for Europe, `(J)` for Japan, `(W)` for World, `(UE)` for USA+Europe.

**GoodTools region shortcodes:** `(U)` USA, `(E)` Europe, `(J)` Japan, `(F)` France, `(G)` Germany, `(S)` Spain, `(I)` Italy, `(W)` World, `(Unl)` unlicensed, `(PD)` public domain.

**Examples:**
```
Super Mario World (U) [!].sfc
Super Mario World (U) [T+Spa1.0_GroupName].sfc
Super Mario World (U) [h1].sfc
Sonic the Hedgehog (UE) [!].zip
Legend of Zelda, The (U) [b1].nes
```

### Hack and Homebrew Filenames

Hacks and translations don't follow a single standard. Common patterns:

```
Super Mario World (USA) [Hack] (Kaizo Mario World v3.1).sfc
Super Mario World Kaizo (Hack).sfc
Super Mario World (U) [T+Spa1.0_GroupName].sfc
Super Mario World - Return to Dinosaur Land (Hack).sfc
Metroid - Redesign (Hack).sfc
```

The parenthesized `(Hack)` or bracketed `[Hack]` marker is the most reliable indicator.

---

## 2. Parsing Strategy

Use a **regex-based parser** that handles both No-Intro and GoodTools conventions. The filename structure is regular enough that a single pass with well-ordered regex captures works reliably.

### Parsing Algorithm

```
Input: "Legend of Zelda, The - A Link to the Past (USA) (Rev 1).sfc"

1. Split off file extension → ext = "sfc", stem = "Legend of Zelda, The - A Link to the Past (USA) (Rev 1)"
2. Extract all parenthesized groups → ["USA", "Rev 1"]
3. Extract all bracketed groups → []
4. Title = everything before the first '(' or '[' → "Legend of Zelda, The - A Link to the Past"
5. Classify each group by content pattern matching
6. Normalize the title (handle "Name, The" → "The Name" for sorting)
```

### Group Classification

Each parenthesized/bracketed group is classified by pattern:

| Pattern | Classification | Examples |
|---------|---------------|----------|
| Known region name or code | Region | `USA`, `Europe`, `Japan`, `U`, `E`, `J`, `USA, Europe` |
| `Rev \w+` | Revision | `Rev 1`, `Rev A` |
| `v\d+\.\d+` | Version | `v1.0`, `v1.1` |
| `Beta\|Proto\|Sample\|Demo` | Status | `Beta`, `Proto` |
| `Unl` | Unlicensed | `Unl` |
| `Virtual Console` | Flag | `Virtual Console` |
| `Hack` | Hack marker | `Hack` |
| `[!]` | Verified | good dump |
| `[b\d*]` | Bad dump | `[b]`, `[b1]` |
| `[a\d*]` | Alternate | `[a1]` |
| `[o\d*]` | Overdump | `[o1]` |
| `[f\d*]` | Fixed | `[f1]` |
| `[h\d*]` | Hacked | `[h1]` |
| `[t\d*]` | Trainer | `[t1]` |
| `[T[+-]\w+]` | Translation | `[T+Spa]`, `[T-Eng1.0_Author]` |
| Two-letter ISO code | Language | `En`, `Fr`, `Es`, `De` |
| `SGB Enhanced\|GB Compatible` | Enhancement flag | `SGB Enhanced` |
| Anything after `[Hack]` in parens | Hack name | `(Kaizo Mario World v3.1)` |

### Regex Sketch

```rust
use regex::Regex;
use std::sync::LazyLock;

/// Matches parenthesized groups: (content)
static PAREN_GROUP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\(([^)]+)\)").unwrap()
});

/// Matches bracketed groups: [content]
static BRACKET_GROUP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[([^\]]+)\]").unwrap()
});

/// Matches revision patterns
static REVISION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^Rev ([A-Za-z0-9]+)$").unwrap()
});

/// Matches version patterns
static VERSION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^v(\d+\.\d+.*)$").unwrap()
});

/// Matches GoodTools translation codes
static TRANSLATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^T([+-])(\w+?)(\d+\.\d+)?(?:_(.+))?$").unwrap()
});
```

### Title Normalization

Handle the "Name, The" convention used by No-Intro:

```rust
/// Normalize "Legend of Zelda, The" → "The Legend of Zelda"
/// Normalize "Addams Family, The - Pugsley's Scavenger Hunt" → "The Addams Family - Pugsley's Scavenger Hunt"
fn normalize_title(raw: &str) -> String {
    // Check for ", The - " (article before subtitle separator)
    if let Some(idx) = raw.find(", The - ") {
        return format!("The {} - {}", &raw[..idx], &raw[idx + 8..]);
    }
    // Check for ", The" at end
    if let Some(stripped) = raw.strip_suffix(", The") {
        return format!("The {stripped}");
    }
    // Same for "A" and "An"
    if let Some(stripped) = raw.strip_suffix(", A") {
        return format!("A {stripped}");
    }
    raw.to_string()
}
```

---

## 3. Data Model

### Parsed ROM Info

```rust
use serde::{Deserialize, Serialize};

/// Parsed metadata extracted from a ROM filename.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RomInfo {
    /// Raw filename as found on disk (e.g., "Super Mario World (USA).sfc")
    pub filename: String,

    /// Clean game title, articles normalized
    /// e.g., "The Legend of Zelda - A Link to the Past"
    pub title: String,

    /// Title used for grouping: lowercased, articles stripped, punctuation removed
    /// e.g., "legend of zelda a link to the past"
    pub group_key: String,

    /// Region(s) detected
    pub regions: Vec<Region>,

    /// Revision, if any (e.g., "1", "A")
    pub revision: Option<String>,

    /// Version, if any (e.g., "1.0", "1.1")
    pub version: Option<String>,

    /// Status flags
    pub flags: Vec<RomFlag>,

    /// Translation info, if this is a translated ROM
    pub translation: Option<Translation>,

    /// Hack info, if this is a ROM hack
    pub hack: Option<HackInfo>,

    /// Dump status from GoodTools brackets
    pub dump_status: DumpStatus,

    /// File extension (lowercase, without dot)
    pub extension: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Region {
    USA,
    Europe,
    Japan,
    World,
    Australia,
    Brazil,
    Canada,
    China,
    France,
    Germany,
    HongKong,
    Italy,
    Korea,
    Netherlands,
    Spain,
    Sweden,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RomFlag {
    Beta,
    Proto,
    Sample,
    Demo,
    Unlicensed,
    VirtualConsole,
    Pirate,
    SgbEnhanced,
    GbCompatible,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Translation {
    /// Target language code (e.g., "Spa", "Eng", "Fre")
    pub language: String,
    /// Version of the translation, if present
    pub version: Option<String>,
    /// Author or group name, if present
    pub author: Option<String>,
    /// Whether this is a complete (+) or partial (-) translation
    pub complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HackInfo {
    /// Name of the hack, if identifiable
    pub name: Option<String>,
    /// Version of the hack, if present
    pub version: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DumpStatus {
    Verified,       // [!]
    BadDump,        // [b]
    Alternate,      // [a]
    Overdump,       // [o]
    Fixed,          // [f]
    Trainer,        // [t]
    #[default]
    Unknown,        // no indicator
}
```

### Game Group

```rust
/// A logical game: groups all regional variants, revisions, hacks, and translations
/// of the same base game.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameGroup {
    /// Canonical display title (normalized, from the "best" variant)
    pub title: String,

    /// Group key used for matching (lowercase, stripped)
    pub group_key: String,

    /// The system this group belongs to
    pub system: String,

    /// "Official" variants: different regions/revisions of the original game
    pub variants: Vec<GameVariant>,

    /// ROM hacks derived from this game
    pub hacks: Vec<GameVariant>,

    /// Translations derived from this game
    pub translations: Vec<GameVariant>,
}

/// A single ROM file within a game group.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameVariant {
    /// Parsed ROM info
    pub rom: RomInfo,

    /// Full path relative to storage root
    pub relative_path: String,

    /// File size in bytes
    pub size_bytes: u64,
}

impl GameGroup {
    /// Total number of files in this group (variants + hacks + translations).
    pub fn total_files(&self) -> usize {
        self.variants.len() + self.hacks.len() + self.translations.len()
    }

    /// The "primary" variant: prefer USA, then World, then Europe, then first available.
    /// Among same-region variants, prefer highest revision and verified dumps.
    pub fn primary_variant(&self) -> &GameVariant {
        self.variants
            .iter()
            .max_by_key(|v| variant_priority(v))
            .unwrap_or(&self.variants[0])
    }
}

/// Score a variant for "primary" selection. Higher = more preferred.
fn variant_priority(v: &GameVariant) -> (u8, u8, u8) {
    let region_score = v.rom.regions.iter().map(|r| match r {
        Region::USA => 10,
        Region::World => 9,
        Region::Europe => 8,
        Region::Japan => 7,
        _ => 5,
    }).max().unwrap_or(0);

    let dump_score = match v.rom.dump_status {
        DumpStatus::Verified => 2,
        DumpStatus::Unknown => 1,  // No-Intro sets don't use [!], treat as OK
        _ => 0,
    };

    let flag_penalty: u8 = if v.rom.flags.iter().any(|f| matches!(f,
        RomFlag::Beta | RomFlag::Proto | RomFlag::Sample | RomFlag::Demo
    )) { 0 } else { 1 };

    (flag_penalty, region_score, dump_score)
}
```

---

## 4. Grouping Algorithm

### Overview

Group ROMs into `GameGroup` clusters based on their parsed `group_key`. The key is derived from the title by:

1. Normalizing articles ("Legend of Zelda, The" -> "The Legend of Zelda")
2. Converting to lowercase
3. Stripping leading articles for grouping ("the ", "a ", "an ")
4. Removing punctuation and extra whitespace
5. Collapsing multiple spaces

This produces a stable key that matches across naming variations.

### group_key Generation

```rust
fn make_group_key(title: &str) -> String {
    let normalized = normalize_title(title);
    let lower = normalized.to_lowercase();

    // Strip leading articles for grouping purposes
    let stripped = lower
        .strip_prefix("the ")
        .or_else(|| lower.strip_prefix("a "))
        .or_else(|| lower.strip_prefix("an "))
        .unwrap_or(&lower);

    // Remove punctuation, collapse whitespace
    stripped
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
```

**Examples:**

| Raw Title | group_key |
|-----------|-----------|
| `Super Mario World` | `super mario world` |
| `Legend of Zelda, The - A Link to the Past` | `legend of zelda a link to the past` |
| `Sonic the Hedgehog` | `sonic hedgehog` |
| `Street Fighter II Turbo - Hyper Fighting` | `street fighter ii turbo hyper fighting` |

### Grouping Process

```rust
use std::collections::HashMap;

pub fn group_roms(roms: Vec<(RomInfo, String, u64)>) -> Vec<GameGroup> {
    let mut groups: HashMap<String, GameGroup> = HashMap::new();

    for (info, relative_path, size_bytes) in roms {
        let key = info.group_key.clone();
        let variant = GameVariant { rom: info, relative_path, size_bytes };

        let group = groups.entry(key.clone()).or_insert_with(|| GameGroup {
            title: variant.rom.title.clone(),
            group_key: key,
            system: String::new(), // set by caller
            variants: Vec::new(),
            hacks: Vec::new(),
            translations: Vec::new(),
        });

        if variant.rom.hack.is_some() {
            group.hacks.push(variant);
        } else if variant.rom.translation.is_some() {
            group.translations.push(variant);
        } else {
            group.variants.push(variant);
        }
    }

    groups.into_values().collect()
}
```

### Edge Cases

**Subtitled games that share a base name:**
- "Super Mario World" and "Super Mario World 2 - Yoshi's Island" must NOT group together.
- The group key includes the full title, so `super mario world` != `super mario world 2 yoshis island`. This works naturally.

**Hacks with heavily modified names:**
- "Kaizo Mario World (Hack).sfc" -- the `(Hack)` marker is detected, but the title "Kaizo Mario World" won't match "Super Mario World" automatically.
- For these, we rely on the user's folder organization or future metadata matching. The parser marks them as hacks but cannot always determine the parent game from the filename alone.

**GoodTools vs No-Intro region codes:**
- `(U)` and `(USA)` both map to `Region::USA`. The parser handles both formats.

---

## 5. Search Integration

### Search Index

When listing ROMs, build a lightweight search index from `GameGroup` data:

```rust
/// Terms that a game group is searchable by.
pub fn search_terms(group: &GameGroup) -> Vec<String> {
    let mut terms = Vec::new();

    // The canonical title
    terms.push(group.title.to_lowercase());

    // The group key (already normalized)
    terms.push(group.group_key.clone());

    // Hack names
    for hack in &group.hacks {
        if let Some(ref info) = hack.rom.hack {
            if let Some(ref name) = info.name {
                terms.push(name.to_lowercase());
            }
        }
        // Also index the hack's raw title
        terms.push(hack.rom.title.to_lowercase());
    }

    // Translation target languages
    for translation in &group.translations {
        if let Some(ref info) = translation.rom.translation {
            terms.push(info.language.to_lowercase());
            if let Some(ref author) = info.author {
                terms.push(author.to_lowercase());
            }
        }
    }

    terms
}
```

### Search Behavior

The current search in `get_roms_page` does a simple `filename.contains(&q)`. With parsed names, search improves to:

1. **Exact substring match** on the clean title (not the filename with region codes).
2. **Match across hacks/translations** -- searching "kaizo" finds the hack grouped under Super Mario World.
3. **Match on region** -- searching "japan" shows all Japanese variants.
4. **Ignore noise** -- searching "zelda" matches "The Legend of Zelda - A Link to the Past" without requiring the user to type the article or full title.

```rust
pub fn matches_search(group: &GameGroup, query: &str) -> bool {
    let q = query.to_lowercase();
    let terms = search_terms(group);
    terms.iter().any(|t| t.contains(&q))
}
```

### Future: Fuzzy Matching

For Phase 2, add fuzzy matching (e.g., Levenshtein distance or trigram similarity) to handle typos. The `strsim` crate provides this. Keep it optional -- exact substring matching covers 90% of use cases.

---

## 6. Display Model

### System ROM List (Grouped View)

The games page currently shows a flat list of ROM files. With grouping, it shows one row per `GameGroup`:

```
┌─────────────────────────────────────────────────────────┐
│ Super Mario World                          USA  ★  🗑  │
│ 3 versions · 1 hack                                    │
├─────────────────────────────────────────────────────────┤
│ The Legend of Zelda - A Link to the Past   USA  ★  🗑  │
│                                                         │
├─────────────────────────────────────────────────────────┤
│ Sonic the Hedgehog                         USA  ★  🗑  │
│ 2 versions                                              │
└─────────────────────────────────────────────────────────┘
```

**Primary row shows:**
- Clean game title (from `primary_variant()`)
- Region badge(s) of the primary variant
- Favorite toggle and actions

**Secondary line shows (only when > 1 file):**
- "N versions" count (variants with different regions/revisions)
- "N hacks" count
- "N translations" count

### Expanded Group Detail

Tapping a game with multiple variants expands or navigates to a detail showing all files:

```
Super Mario World
━━━━━━━━━━━━━━━━

Versions:
  Super Mario World (USA).sfc                    131 KB   ★
  Super Mario World (Europe) (Rev 1).sfc          131 KB
  Super Mario World (Japan).sfc                   128 KB

Hacks:
  Kaizo Mario World v3.1 (Hack).sfc              156 KB
```

### Data Flow: Server to Client

The server function returns `GameGroup` data. Since `RomInfo` contains only parsed filename data (no filesystem access needed on the client), it serializes cleanly.

```rust
/// Returned by the server function for grouped ROM listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameGroupSummary {
    /// Canonical display title
    pub title: String,
    /// Primary variant's region(s), for display
    pub primary_regions: Vec<Region>,
    /// Number of official variants
    pub variant_count: usize,
    /// Number of hacks
    pub hack_count: usize,
    /// Number of translations
    pub translation_count: usize,
    /// Whether any variant is favorited
    pub is_favorited: bool,
    /// Relative path of the primary variant (for favorite toggle, launch, etc.)
    pub primary_path: String,
    /// Total size of all files in this group
    pub total_size_bytes: u64,
}
```

For the initial implementation, the grouped view is opt-in -- the flat file list remains the default. A toggle (or automatic activation when a system has many variants) switches to grouped view.

---

## 7. Metadata Bridge

### From Parsed Name to External ID

The parsed `RomInfo` provides two paths to external metadata:

**Path 1: Hash-based matching (accurate, requires file I/O)**

```rust
use std::io::Read;

pub struct RomHashes {
    pub crc32: String,   // "A31BEAD4"
    pub md5: String,     // "abc123..."
    pub sha1: String,    // "def456..."
}

/// Compute hashes for a ROM file. For zipped ROMs, hash the inner file.
pub fn compute_rom_hashes(path: &std::path::Path) -> std::io::Result<RomHashes> {
    // Read file, compute CRC32 + MD5 + SHA1 in a single pass
    // For .zip files: extract the largest file inside and hash that
    todo!()
}
```

Hash-based matching is used with ScreenScraper API (primary metadata source) and No-Intro DATs for identification.

**Path 2: Name-based search (fast, no file I/O)**

The parsed `title` + system provides a search key for APIs that don't support hash lookup (IGDB, TheGamesDB):

```rust
pub struct MetadataSearchKey {
    /// Clean game title
    pub title: String,
    /// System identifier (mapped to the external API's platform ID)
    pub system: String,
}
```

### Metadata ID Storage

Once a ROM is matched to an external source, store the mapping so we don't re-query:

```rust
/// Stored mapping from a ROM file to its external metadata IDs.
/// Persisted in the future SQLite cache.
pub struct MetadataMapping {
    /// Hash of the ROM file (primary key for lookup)
    pub rom_sha1: String,
    /// ROM filename (fallback identifier)
    pub filename: String,
    /// System folder name
    pub system: String,

    /// External IDs (None = not yet queried or no match)
    pub screenscraper_id: Option<u64>,
    pub igdb_id: Option<u64>,
    pub retroachievements_id: Option<u64>,
}
```

### Matching Flow

```
ROM file on disk
       │
       ├─ parse_filename() ──→ RomInfo (instant, no I/O)
       │                           │
       │                           ├─ group_key ──→ GameGroup (local grouping)
       │                           └─ title + system ──→ name-based API search (fallback)
       │
       └─ compute_rom_hashes() ──→ RomHashes (requires reading file)
                                       │
                                       └─ ScreenScraper API query ──→ screenscraper_id
                                       └─ No-Intro DAT lookup ──→ verified name
                                       └─ RetroAchievements API ──→ ra_id
```

Filename parsing happens at scan time (fast, always available). Hash computation happens lazily when the user views a game's detail page or when a background indexer runs.

---

## 8. Implementation Plan

### Phase 1: Filename Parser (in `replay-control-core`)

Add a new module `replay-control-core/src/rom_info.rs`:

- `parse_filename(filename: &str) -> RomInfo` -- the core parser
- `normalize_title(raw: &str) -> String` -- article handling
- `make_group_key(title: &str) -> String` -- grouping key derivation
- Region enum, flag enum, translation/hack structs
- Comprehensive unit tests with real-world filenames

**Dependencies:** `regex` (already transitively available, but add explicitly).

**Estimated effort:** This is the foundation. Every subsequent phase depends on correct parsing.

### Phase 2: Game Grouping

Add grouping logic to `replay-control-core/src/rom_info.rs` or a new `game_groups.rs`:

- `group_roms(roms: Vec<(RomInfo, String, u64)>) -> Vec<GameGroup>`
- `GameGroup`, `GameVariant` structs
- `primary_variant()` selection logic
- Integration with `roms::list_roms()` -- add an option to return grouped results

Update `RomEntry` to include optional `RomInfo`:

```rust
pub struct RomEntry {
    // ... existing fields ...
    /// Parsed filename metadata (None for arcade systems, which use arcade_db)
    pub parsed: Option<RomInfo>,
}
```

### Phase 3: Search Improvement

Update `get_roms_page` server function to search on parsed titles instead of raw filenames:

- Build search terms from `RomInfo` fields
- Match on clean title, hack names, translation languages
- Keep backward compatibility: if parsing returns no useful data, fall back to filename search

### Phase 4: Grouped UI

Add a grouped view to the system ROM list page:

- New server function `get_game_groups(system, offset, limit, search)` returning `Vec<GameGroupSummary>`
- New component for rendering a game group row with variant/hack/translation counts
- Expandable detail showing all variants
- Toggle between flat file list and grouped view

### Phase 5: Metadata Bridge

When the metadata system (Phase 2 from the project roadmap) is implemented:

- Use `RomInfo.title` + system as the search key for external APIs
- Add `compute_rom_hashes()` for hash-based matching with ScreenScraper
- Store `MetadataMapping` in the SQLite cache
- The parsed title dramatically improves name-based search accuracy vs. raw filenames

### Phase 6: Hash-Based Identification

Background task that hashes ROM files and matches against No-Intro DATs:

- Confirms correct game identification
- Detects bad dumps, hacks, and unofficial releases
- Provides a verified game name that may differ from the filename
- Feeds into the metadata bridge for more accurate external lookups

---

## Appendix: Region Code Reference

### No-Intro Full Region Names

| Region | Code | Language Default |
|--------|------|-----------------|
| USA | USA | English |
| Europe | Europe | Multi |
| Japan | Japan | Japanese |
| World | World | English |
| Australia | Australia | English |
| Brazil | Brazil | Portuguese |
| Canada | Canada | English/French |
| China | China | Chinese |
| France | France | French |
| Germany | Germany | German |
| Hong Kong | Hong Kong | Chinese/English |
| Italy | Italy | Italian |
| Korea | Korea | Korean |
| Netherlands | Netherlands | Dutch |
| Spain | Spain | Spanish |
| Sweden | Sweden | Swedish |

### GoodTools Short Codes

| Code | Region |
|------|--------|
| (U) | USA |
| (E) | Europe |
| (J) | Japan |
| (W) | World |
| (F) | France |
| (G) | Germany |
| (S) | Spain |
| (I) | Italy |
| (A) | Australia |
| (B) | Brazil (non-standard) |
| (K) | Korea |
| (C) | China |
| (UE) | USA + Europe |
| (JU) | Japan + USA |
| (JUE) | Japan + USA + Europe |

### GoodTools Translation Language Codes

| Code | Language |
|------|----------|
| Eng | English |
| Spa | Spanish |
| Fre | French |
| Ger | German |
| Ita | Italian |
| Por | Portuguese |
| Dut | Dutch |
| Swe | Swedish |
| Nor | Norwegian |
| Dan | Danish |
| Fin | Finnish |
| Rus | Russian |
| Chi | Chinese |
| Kor | Korean |
| Jpn | Japanese |
| Ara | Arabic |
| Tur | Turkish |
| Pol | Polish |
| Gre | Greek |
| Cat | Catalan |
