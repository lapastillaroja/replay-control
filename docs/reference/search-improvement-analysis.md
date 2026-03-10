# Search Improvement Analysis

> Status: Implemented (Phases 1-3)
> Date: 2026-03-10

## Problem Statement

When searching for a game like "Super Mario World", hack ROMs appear before the original game in the results. For example:

```
Super Mario World - Kaizo Mario (Hack)
Super Mario World - Learn 2 Kaizo (Hack)
Super Mario World - Return to Dinosaur Land (SMW Hack)
Super Mario World (Europe)
Super Mario World (Europe, 60Hz)
Super Mario World (Japan)
Super Mario World (USA)                   <-- the one the user wants
Super Mario World (USA, ES Translation)
```

This happens because search results are filtered from an alphabetically sorted list, and "Kaizo", "Learn", and "Return" sort before the bare title. The user almost always wants the original, clean ROM -- not a hack, translation, or beta.

---

## Current Implementation

### Data Flow

1. **ROM scanning** (`replay-control-core/src/roms.rs` -- `list_roms()`): Reads the filesystem, builds `RomEntry` structs. Each entry gets a `GameRef` with a resolved `display_name`.

2. **Alphabetical sort** (`roms.rs` line 82-86): ROMs are sorted by `display_name` (or `rom_filename` if no display name). This is a pure alphabetical sort with no awareness of ROM type.

3. **Caching** (`replay-control-app/src/api/mod.rs` -- `RomCache`): The sorted list is cached for 30 seconds to avoid repeated filesystem scans.

4. **Search filtering** (`replay-control-app/src/server_fns.rs` -- `get_roms_page()`): A simple `contains()` filter on the display name and filename:

```rust
let q = search.to_lowercase();
all_roms.into_iter().filter(|r| {
    let display = r.game.display_name.as_deref().unwrap_or(&r.game.rom_filename);
    display.to_lowercase().contains(&q)
        || r.game.rom_filename.to_lowercase().contains(&q)
}).collect()
```

5. **Pagination** (`get_roms_page()`): After filtering, the results are paginated with `skip(offset).take(limit)`.

6. **UI** (`replay-control-app/src/components/rom_list.rs`): The `RomList` component renders results with a debounced search bar (300ms delay), infinite scroll, and URL query param sync.

### Why Hacks Sort First

The sort is purely alphabetical on display names. A hack titled `"Super Mario World - Kaizo Mario (Hack)"` has the display name `"Super Mario World"` (from the game DB normalized title lookup) with tags appended as `"(Hack)"`. But many hacks have completely custom filenames like `"Super Mario World - Kaizo Mario World (SMW Hack).sfc"` where the base title matches the search query but the full display name (with the hyphenated subtitle) sorts before the plain `"Super Mario World (USA)"` because `-` (hyphen) comes before `(` (open paren) in ASCII.

Even when display names are identical, the alphabetical sort has no concept of "original vs. modified" -- hacks, translations, and betas are treated identically to clean ROMs.

### Tag Detection Already Exists

The `rom_tags` module (`replay-control-core/src/rom_tags.rs`) already parses all the relevant tags:
- `is_hack` -- `(Hack)`, `(SMW Hack)`, `(SA-1 SMW Hack)`, and patterns ending with ` hack`
- `is_beta` -- `(Beta)`, `(Beta X)`
- `is_proto` -- `(Proto)`, `(Prototype)`
- `is_demo` -- `(Demo)`
- `is_unlicensed` -- `(Unl)`, `(Unlicensed)`
- `is_aftermarket` -- `(Aftermarket)`, `(Homebrew)`
- `is_pirate` -- `(Pirate)`
- `translation` -- `(Traducido Es)`, `[T-Spa1.0v_Wave]`, etc.
- `region` -- `(USA)`, `(Europe)`, `(Japan)`, `(World)`, etc.
- `revision` -- `(Rev 1)`, `(Rev A)`, etc.

This tag detection is currently only used for display name formatting, not for sorting or ranking.

---

## Proposed Solution: Relevance-Ranked Search

### Phase 1: Sort-Time ROM Classification (Core Layer)

Add a `rom_tags::classify()` function that returns a lightweight `RomClassification` struct. This can be used at sort time to group ROMs by quality tier.

**File: `replay-control-core/src/rom_tags.rs`**

```rust
/// Classification of a ROM for sorting purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RomTier {
    /// Clean original ROM with standard region tag (USA, Europe, Japan, World).
    Original = 0,
    /// Revision of an original (Rev 1, Rev A) -- still a clean ROM.
    Revision = 1,
    /// Region variant for a non-primary region (Spain, France, Brazil, etc.).
    RegionVariant = 2,
    /// Translation patch applied to a clean ROM.
    Translation = 3,
    /// Unlicensed but commercially released game.
    Unlicensed = 4,
    /// ROM hack (modified game).
    Hack = 5,
    /// Beta, prototype, or demo.
    PreRelease = 6,
    /// Pirate / bootleg.
    Pirate = 7,
    /// Homebrew / aftermarket.
    Homebrew = 8,
}

/// Region priority for sorting within the same tier.
/// Lower = shown first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RegionPriority {
    World = 0,
    Usa = 1,
    Europe = 2,
    Japan = 3,
    Other = 4,
    Unknown = 5,
}

/// Classify a ROM filename into a tier and region priority.
pub fn classify(filename: &str) -> (RomTier, RegionPriority) {
    // Reuse existing tag extraction logic...
    // Parse tags, determine tier, determine region priority.
    // ...
}
```

The `classify` function would reuse the existing `extract_tags()` parsing logic (or better, share a common internal parser) to avoid duplicating the tag detection.

**Key design point**: The classification is derived purely from the filename, so it can be computed once during ROM scanning and cached as part of the `RomEntry` or `GameRef`. No additional I/O needed.

### Phase 2: Tiered Sort in ROM Listing

Modify `list_roms()` in `replay-control-core/src/roms.rs` to sort by tier first, then alphabetically within each tier:

```rust
roms.sort_by(|a, b| {
    let a_name = a.game.display_name.as_deref().unwrap_or(&a.game.rom_filename);
    let b_name = b.game.display_name.as_deref().unwrap_or(&b.game.rom_filename);

    let (a_tier, a_region) = rom_tags::classify(&a.game.rom_filename);
    let (b_tier, b_region) = rom_tags::classify(&b.game.rom_filename);

    // Primary: sort by base title (alphabetical)
    let a_base = rom_tags::base_title(a_name);
    let b_base = rom_tags::base_title(b_name);
    a_base.to_lowercase().cmp(&b_base.to_lowercase())
        // Secondary: within same title, originals before hacks
        .then(a_tier.cmp(&b_tier))
        // Tertiary: within same tier, preferred region first
        .then(a_region.cmp(&b_region))
        // Quaternary: alphabetical on full display name as tiebreaker
        .then(a_name.to_lowercase().cmp(&b_name.to_lowercase()))
});
```

This means when browsing the full ROM list (no search), the user sees:

```
Super Mario World (USA)
Super Mario World (Europe)
Super Mario World (Japan)
Super Mario World (USA, Rev 1)
Super Mario World (USA, ES Translation)
Super Mario World - Kaizo Mario (Hack)
Super Mario World - Learn 2 Kaizo (Hack)
Super Mario World - Return to Dinosaur Land (Hack)
```

A helper function `rom_tags::base_title()` would extract the base game title (stripping parenthesized tags AND hyphenated subtitles that come after the original title). For hacks, this is tricky because the hack name IS the subtitle, but the grouping would still work since the `" - "` separator sorts after the base title.

### Phase 3: Relevance-Scored Search

Replace the simple `contains()` filter in `get_roms_page()` with a scoring function:

```rust
/// Compute a relevance score for a ROM against a search query.
/// Higher = more relevant. Returns 0 for no match.
fn search_score(query: &str, display_name: &str, filename: &str) -> u32 {
    let query_lower = query.to_lowercase();
    let display_lower = display_name.to_lowercase();
    let filename_lower = filename.to_lowercase();

    let mut score: u32 = 0;

    // 1. Exact match on display name (highest priority)
    if display_lower == query_lower {
        score += 10000;
    }
    // 2. Display name starts with query
    else if display_lower.starts_with(&query_lower) {
        score += 5000;
    }
    // 3. A word in display name starts with query
    else if display_lower.split_whitespace()
        .any(|w| w.starts_with(&query_lower))
    {
        score += 2000;
    }
    // 4. Display name contains query
    else if display_lower.contains(&query_lower) {
        score += 1000;
    }
    // 5. Filename contains query (but display name doesn't)
    else if filename_lower.contains(&query_lower) {
        score += 500;
    }
    // No match at all
    else {
        return 0;
    }

    // Bonus: shorter display name = more likely to be what the user wants
    // (originals tend to have shorter names than hacks)
    if display_name.len() < 40 {
        score += 100;
    }

    // Penalty: deprioritize non-original ROMs
    let (tier, region) = rom_tags::classify(filename);
    score = score.saturating_sub(match tier {
        RomTier::Original => 0,
        RomTier::Revision => 5,
        RomTier::RegionVariant => 10,
        RomTier::Translation => 50,
        RomTier::Unlicensed => 60,
        RomTier::Hack => 200,
        RomTier::PreRelease => 250,
        RomTier::Pirate => 300,
        RomTier::Homebrew => 100, // homebrew is interesting, don't bury it
    });

    // Bonus: preferred region
    score += match region {
        RegionPriority::World => 20,
        RegionPriority::Usa => 15,
        RegionPriority::Europe => 10,
        RegionPriority::Japan => 5,
        RegionPriority::Other => 0,
        RegionPriority::Unknown => 0,
    };

    score
}
```

Then in `get_roms_page()`:

```rust
let filtered: Vec<RomEntry> = if search.is_empty() {
    all_roms // already sorted by tiered sort
} else {
    let q = search.to_lowercase();
    let mut scored: Vec<(u32, RomEntry)> = all_roms
        .into_iter()
        .filter_map(|r| {
            let display = r.game.display_name.as_deref()
                .unwrap_or(&r.game.rom_filename);
            let score = search_score(&q, display, &r.game.rom_filename);
            if score > 0 { Some((score, r)) } else { None }
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0)); // highest score first
    scored.into_iter().map(|(_, r)| r).collect()
};
```

### Phase 4: Metadata-Enriched Search (Future)

Once the metadata DB has good coverage, extend the search to also match against:

- **Genre**: searching "platform" shows all platformers
- **Year**: searching "1991" shows games from that year
- **Developer**: searching "Capcom" shows all Capcom games

This would require a `search_score_with_metadata()` variant that receives a `GameInfo` instead of just display name/filename.

```rust
// Additional scoring for metadata fields
if !info.genre.is_empty() && info.genre.to_lowercase().contains(&query_lower) {
    score += 300; // genre match, lower than title match
}
if !info.developer.is_empty() && info.developer.to_lowercase().contains(&query_lower) {
    score += 300;
}
if !info.year.is_empty() && info.year == query {
    score += 200;
}
```

### Phase 5: Search Filters (Future)

Add filter chips/toggles in the UI to exclude categories:

- [ ] Hide hacks
- [ ] Hide translations
- [ ] Hide betas/protos
- [ ] Region filter (USA only, Europe only, etc.)
- [ ] Genre filter

These filters would be sent as additional parameters to `get_roms_page()` and applied before scoring.

On the server side, filtering before scoring is cheaper:

```rust
#[server(prefix = "/sfn")]
pub async fn get_roms_page(
    system: String,
    offset: usize,
    limit: usize,
    search: String,
    hide_hacks: bool,
    hide_translations: bool,
    hide_prerelease: bool,
) -> Result<RomPage, ServerFnError> { ... }
```

---

## Implementation Plan

### Step 1: `rom_tags::classify()` Function ✓

**Where**: `replay-control-core/src/rom_tags.rs`

- Added `RomTier` enum (Original, Revision, RegionVariant, Translation, Unlicensed, Homebrew, Hack, PreRelease, Pirate)
- Added `RegionPriority` enum (World, Usa, Europe, Japan, Other, Unknown)
- Added `classify(filename: &str) -> (RomTier, RegionPriority)` reusing the existing `ParenTags`/`BracketTags` parsing

### Step 2: Tiered Sort ✓

**Where**: `replay-control-core/src/roms.rs` -- `list_roms()`

- Sort now uses: display name → tier (originals first) → region priority
- Browsing a system now shows original ROMs before hacks/translations even without searching

### Step 3: Scored Search ✓

**Where**: `replay-control-app/src/server_fns.rs` -- `search_score()` + `get_roms_page()`

- Added `search_score(query, display_name, filename) -> u32` with:
  - Base score: exact match (10K) > starts_with (5K) > word_starts_with (2K) > contains (1K) > filename_only (500)
  - Length bonus: +100 for short names (< 40 chars)
  - Tier penalty: -200 for hacks, -250 for pre-release, -300 for pirate
  - Region bonus: +20 World, +15 USA, +10 Europe, +5 Japan
- `get_roms_page()` now scores and sorts by relevance when a search query is active

### Step 4: UI Filters (Optional — Future)

**Where**: `replay-control-app/src/components/rom_list.rs` + `server_fns.rs`

- Add filter toggles to the search bar area
- Pass filter params to `get_roms_page()`
- Filter before scoring on the server

**Effort**: Medium. UI work + additional server fn params.

---

## How Other Frontends Handle This

### EmulationStation (ES-DE)

- Custom collections allow excluding hacks manually
- Metadata scraper assigns "hack" flag to games
- Default sort is alphabetical but scraped metadata allows filtering by genre, players, rating
- No automatic hack detection from filenames

### LaunchBox / Big Box

- Games have a `Status` field (Released, Unreleased, Hack)
- Filtering by status is a first-class feature
- Supports custom playlists/collections that exclude hacks
- Relevance search with fuzzy matching

### Retroarch Ozone/XMB

- Pure alphabetical sort with no search
- Playlists are separate from the filesystem
- No hack detection

### RetroArch Desktop (WIMP)

- Database-driven with separate "hack" category
- Not filename-based -- relies on metadata

### RePlayOS (the OS itself)

- Uses alphabetical sort in its own UI
- The companion app (our app) has the opportunity to provide a better experience since it has more compute resources and a metadata DB

---

## Performance Considerations

- `classify()` is O(n) in filename length (same as `extract_tags()`) -- negligible
- Scoring adds O(n * m) where n = ROMs in system and m = query length -- still fast for ~10K ROMs
- The tiered sort is O(n log n) same as current sort, just with a different comparator
- No additional I/O needed -- everything is derived from filenames already in memory
- The cache (`RomCache`) continues to work since the sorted order is stable

---

## Files to Modify

| File | Change |
|------|--------|
| `replay-control-core/src/rom_tags.rs` | Add `RomTier`, `RegionPriority`, `classify()`, `base_title()` |
| `replay-control-core/src/roms.rs` | Use tiered sort in `list_roms()` |
| `replay-control-app/src/server_fns.rs` | Replace `contains()` filter with scored search in `get_roms_page()` |
| `replay-control-app/src/components/rom_list.rs` | (Phase 4+) Add filter toggles |

---

## Open Questions

1. **Should region preference be configurable?** A European user might want `(Europe)` ROMs to sort first. This could be a setting in replay.cfg or auto-detected from the system locale.

2. **Should hacks be hidden by default?** Some users have ROM sets that are almost entirely hacks (e.g., Super Mario World hack collections). Hiding hacks by default would make their collection appear nearly empty. A toggle is safer than a default.

3. **Fuzzy matching**: Should we add Levenshtein distance or similar fuzzy matching? Probably not in Phase 1 -- the normalized title fallback in `game_db.rs` already handles most misspelling cases, and the `contains()` check catches partial matches well. Fuzzy matching adds complexity and can produce surprising results (matching unrelated games).

4. **Cross-system search**: Currently search is per-system (you browse to SNES, then search within SNES). A global search across all systems would be a bigger feature. The scoring approach would still work -- just add a system priority and run the search across all cached ROM lists.
