# Search Improvement Analysis

> Status: Implemented (Phases 1-3, 4 partial [genre+year], 5 partial, 6)
> Date: 2026-03-11

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

4. **Search scoring** (`replay-control-app/src/server_fns/search.rs` -- `search_score()`): A multi-tier scoring function that assigns a relevance score based on match quality (exact > prefix > word-boundary > substring > filename-only > word-level), with bonuses for short names and preferred region, and penalties for hacks/translations/pre-release ROMs.

5. **Pagination** (`server_fns/roms.rs` -- `get_roms_page()`): After scoring and sorting, results are paginated with `skip(offset).take(limit)`.

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

### Phase 5: Search Filters (Partially Implemented)

Filter toggles to exclude categories. The following are implemented in both `get_roms_page()` and `global_search()`:

- [x] Hide hacks (`hide_hacks: bool`)
- [x] Hide translations (`hide_translations: bool`)
- [x] Hide betas/protos (`hide_betas: bool`)
- [x] Hide clones (`hide_clones: bool` -- arcade only, checks `arcade_db`)
- [x] Genre filter (`genre: String` -- exact match on normalized genre)
- [x] Multiplayer-only filter (`multiplayer_only: bool` -- players >= 2)
- [ ] Region filter (USA only, Europe only, etc.)

Filters are applied before search scoring on the server side for efficiency.

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

**Where**: `replay-control-app/src/server_fns/search.rs` -- `search_score()` + `get_roms_page()` in `server_fns/roms.rs`

- Added `search_score(query, display_name, filename, region_pref) -> u32` with:
  - Base score: exact match (10K) > starts_with (5K) > word_starts_with (2K) > contains (1K) > filename_only (500) > word-level (400/300)
  - Length bonus: +100 for short names (< 40 chars)
  - Tier penalty: -200 for hacks, -250 for pre-release, -300 for pirate
  - Region bonus: based on `RegionPreference` sort_key (+20/+15/+10/+5/+0)
- `get_roms_page()` now scores and sorts by relevance when a search query is active
- `global_search()` uses the same scoring across all systems

### Step 4: UI Filters ✓ (Partial)

**Where**: `replay-control-app/src/components/rom_list.rs` + `server_fns/roms.rs` + `server_fns/search.rs`

- Filter toggles added to the ROM list and global search pages
- `get_roms_page()` and `global_search()` accept `hide_hacks`, `hide_translations`, `hide_betas`, `hide_clones`, `genre`, and `multiplayer_only` parameters
- Filters are applied before scoring on the server

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

| File | Change | Status |
|------|--------|--------|
| `replay-control-core/src/rom_tags.rs` | Add `RomTier`, `RegionPriority`, `classify()`, `base_title()` | Done |
| `replay-control-core/src/roms.rs` | Use tiered sort in `list_roms()` | Done |
| `replay-control-app/src/server_fns/search.rs` | `search_score()` with word-level matching, `global_search()` | Done |
| `replay-control-app/src/server_fns/roms.rs` | Scored search in `get_roms_page()` with filter params | Done |
| `replay-control-app/src/components/rom_list.rs` | Filter toggles in UI | Done |

---

## Open Questions

1. **Should region preference be configurable?** A European user might want `(Europe)` ROMs to sort first. This could be a setting in `.replay-control/settings.cfg` (the app's own config file on ROM storage, not `replay.cfg` which belongs to RePlayOS on the SD card) or auto-detected from the system locale.

2. **Should hacks be hidden by default?** Some users have ROM sets that are almost entirely hacks (e.g., Super Mario World hack collections). Hiding hacks by default would make their collection appear nearly empty. A toggle is safer than a default.

3. ~~**Fuzzy matching**: Should we add Levenshtein distance or similar fuzzy matching? Probably not in Phase 1 -- the normalized title fallback in `game_db.rs` already handles most misspelling cases, and the `contains()` check catches partial matches well. Fuzzy matching adds complexity and can produce surprising results (matching unrelated games).~~ **Addressed in Phase 6 below** -- word-level matching (not Levenshtein) solves the real problem.

4. **Cross-system search**: Currently search is per-system (you browse to SNES, then search within SNES). A global search across all systems would be a bigger feature. The scoring approach would still work -- just add a system priority and run the search across all cached ROM lists.

---

## Phase 6: Word-Level Fuzzy Matching

> Status: Implemented
> Date: 2026-03-11

### Problem

Searching "sonic 3" returns zero results for "Sonic The Hedgehog 3" because the current `search_score()` treats the entire query as a single substring. The string `"sonic 3"` does not appear as a contiguous substring in `"sonic the hedgehog 3"`, so the match fails at every tier and returns 0.

This is the most common complaint pattern for multi-word searches: users type the game name with filler words omitted, expecting the search to find games where all typed words appear somewhere in the title.

**More examples of queries that currently fail:**

| Query | Expected match | Why it fails |
|-------|---------------|--------------|
| `sonic 3` | Sonic The Hedgehog 3 | "sonic 3" not a contiguous substring |
| `mario kart 64` | Super Mario Kart (SNES) / Mario Kart 64 (N64) | "mario kart 64" is not in the SNES title |
| `street fighter 2` | Street Fighter II - The World Warrior | "2" vs "II" -- different issue (numeral normalization, out of scope) |
| `zelda link` | The Legend of Zelda - A Link to the Past | "zelda link" not contiguous |
| `mega man x` | Mega Man X | This one actually works (contiguous substring), included for contrast |
| `castlevania 4` | Super Castlevania IV | "4" vs "IV" -- numeral normalization, out of scope |

Note: numeral normalization ("2" matching "II", "4" matching "IV") is a separate problem and is explicitly out of scope for this phase. The focus here is on non-contiguous word matching.

### Current Scoring Tiers (for reference)

```
Tier                    Base Score   Condition
────────────────────    ──────────   ─────────
Exact match             10,000       display_lower == query
Prefix match             5,000       display_lower.starts_with(query)
Word-boundary match      2,000       any word in display starts with query
Substring match          1,000       display_lower.contains(query)
Filename-only match        500       filename_lower.contains(query)
No match                     0       (filtered out)
```

All tiers treat the query as a single contiguous string. The proposal adds a new tier between "filename-only" and "no match" for word-level matching.

### Proposed Algorithm

#### When to activate

Word matching only activates when:
1. The query contains whitespace (i.e., multiple words) -- single-word queries keep current behavior unchanged
2. The full query did NOT already match via any existing tier (substring or above)

This means word matching is a **fallback**, not a replacement. If "sonic 3" happened to appear as a substring (e.g., in a title like "Sonic 3 & Knuckles"), that would still match at the higher substring tier.

#### Pseudocode

```
fn search_score(query, display_name, filename, region_pref) -> u32:
    display_lower = display_name.to_lowercase()
    filename_lower = filename.to_lowercase()

    // --- Existing tiers (unchanged) ---
    base = if display_lower == query:          10,000
           elif display_lower.starts_with(query):  5,000
           elif word_starts_with(display_lower, query): 2,000
           elif display_lower.contains(query):  1,000
           elif filename_lower.contains(query):   500
           else: 0

    if base > 0:
        return base + bonuses - penalties   // existing logic, unchanged

    // --- NEW: Word-level matching (fallback) ---
    if !query.contains(' '):
        return 0   // single-word query already failed all tiers

    query_words = query.split_whitespace()   // e.g., ["sonic", "3"]
    if query_words.len() < 2:
        return 0   // safety check

    // Check display name first, then filename
    target = display_lower
    target_words = display_lower.split_whitespace()
    is_filename_only = false

    matched_count = count of query_words where target.split_whitespace()
                    .any(|tw| tw.starts_with(qw))

    if matched_count < query_words.len():
        // Try filename
        target_words = filename_lower.split_whitespace()
        matched_count_fn = count of query_words where target_words
                           .any(|tw| tw.starts_with(qw))
        if matched_count_fn > matched_count:
            matched_count = matched_count_fn
            target_words = filename_lower.split_whitespace()
            is_filename_only = true

    if matched_count < query_words.len():
        return 0   // not all query words found

    // --- Scoring ---
    // All query words matched. Base score = 400 (below filename-only=500,
    // because word matching is less precise than substring containment).
    base = if is_filename_only: 300 else: 400

    // Bonus: word order preservation
    // If query words appear in the same relative order in the title,
    // it's a stronger signal. E.g., "sonic 3" in "Sonic The Hedgehog 3"
    // has sonic before 3 -- order preserved.
    if words_in_order(query_words, target_words):
        base += 50

    // Bonus: word coverage (what fraction of title words did the query cover?)
    // "sonic 3" matches 2 of 4 words in "Sonic The Hedgehog 3" = 50%
    // "sonic hedgehog 3" matches 3 of 4 = 75%
    // Higher coverage = more specific match = higher score
    coverage = matched_count as f32 / target_words.len() as f32
    base += (coverage * 50.0) as u32   // 0-50 bonus

    // Bonus: adjacency -- consecutive query words that are also adjacent
    // in the title get a bonus. "mario kart" in "Super Mario Kart" has
    // both words adjacent in the title.
    // (Implementation detail: iterate query word pairs, check if their
    // positions in the title are consecutive)
    adjacent_pairs = count_adjacent_pairs(query_words, target_words)
    base += adjacent_pairs * 20

    return base + bonuses - penalties   // same tier/region/length bonuses
```

#### Helper: `words_in_order`

```
fn words_in_order(query_words, target_words) -> bool:
    last_pos = -1
    for qw in query_words:
        found_pos = target_words.iter().position(|tw| tw.starts_with(qw))
        if found_pos is None or found_pos <= last_pos:
            return false
        last_pos = found_pos
    return true
```

Note: uses `starts_with` rather than exact equality for word matching. This means "son" in query would match "sonic" in the title. This is consistent with the existing word-boundary tier which also uses `starts_with`.

### Updated Tier Table

```
Tier                        Base Score   Condition
────────────────────────    ──────────   ─────────
Exact match                 10,000       display_lower == query
Prefix match                 5,000       display_lower.starts_with(query)
Word-boundary match          2,000       any word starts with query (single string)
Substring match              1,000       display_lower.contains(query)
Filename-only match            500       filename_lower.contains(query)
All-words match (display)      400       all query words found in display (+ bonuses up to ~520)
All-words match (filename)     300       all query words found in filename (+ bonuses up to ~420)
No match                         0       (filtered out)
```

The gap between tiers is intentional:
- Substring match (1,000) is significantly above all-words match (400) because a contiguous substring is a stronger signal
- All-words in display (400) is above all-words in filename (300) for the same reason as the existing display > filename preference
- The bonuses (order + coverage + adjacency) can add up to ~120 points, which keeps word matches well below substring matches but allows differentiation within the word-match tier

### Ranking Within Word Matches

When multiple games match via word-level matching, the scoring differentiates them:

| Query | Title | Words | Order? | Coverage | Adj. | Total |
|-------|-------|-------|--------|----------|------|-------|
| `sonic 3` | Sonic The Hedgehog 3 | 2/2 | yes (+50) | 2/4=50% (+25) | 0 | 475 |
| `sonic 3` | Sonic 3D Blast | 2/2 | yes (+50) | 2/3=67% (+33) | 1 (+20) | 503 |
| `sonic 3` | Sonic The Hedgehog 3 (shorter name, +100 length bonus) | 2/2 | yes (+50) | 2/4=50% (+25) | 0 | 575 |
| `sonic 3` | Sonic 3D Blast (shorter, +100) | 2/2 | yes (+50) | 2/3=67% (+33) | 1 (+20) | 603 |

Hmm -- "Sonic 3D Blast" scores higher than "Sonic The Hedgehog 3" because it has higher word coverage and an adjacency bonus ("sonic 3" -- "3" starts the word "3D" which is adjacent to "sonic"). This is actually a problem: the user searching "sonic 3" almost certainly wants "Sonic The Hedgehog 3", not "Sonic 3D Blast".

**Mitigation**: Add an **exact word match bonus**. When a query word matches a title word exactly (not just starts_with), it gets extra points. In "Sonic The Hedgehog 3", the word "3" matches exactly. In "Sonic 3D Blast", "3" only matches via starts_with ("3D" starts with "3"). This distinction can add +30 per exact word match:

| Query | Title | Exact words | Adjusted total |
|-------|-------|-------------|----------------|
| `sonic 3` | Sonic The Hedgehog 3 | 2 (sonic, 3) = +60 | 635 |
| `sonic 3` | Sonic 3D Blast | 1 (sonic) = +30 | 633 |

Close, but "Sonic The Hedgehog 3" now edges ahead. The length bonus (+100 for short names) also helps since both titles are under 40 characters, so it cancels out here. In practice, the tier/region penalties will further separate results.

**Revised bonus values**: Exact word match bonus should be +30 per word. This makes exact word matches consistently outrank prefix-only matches.

### Edge Cases

#### 1. Short query words matching too broadly

Query `"a 3"` -- the word "a" appears in almost every title. Should this match?

**Decision**: Yes, but it will rank low because the word "a" contributes very little coverage and the overall match quality is poor. The 2-word minimum for activating word matching prevents truly degenerate single-character queries. The existing debounce (300ms) and the fact that users rarely search for "a 3" make this acceptable.

No minimum word length filter is needed. The coverage scoring naturally penalizes short, common words.

#### 2. Duplicate words in query

Query `"sonic sonic"` -- should it match "Sonic The Hedgehog"?

**Decision**: Yes. Each query word is checked independently. Both instances of "sonic" will find a match in the title. This is harmless and not worth special-casing.

#### 3. Query words that are substrings of title words

Query `"son 3"` -- "son" starts the word "Sonic" in "Sonic The Hedgehog 3".

**Decision**: Match. The `starts_with` check is consistent with the existing word-boundary tier. The exact-word-match bonus (+30) won't apply to "son" but will apply to "3", naturally ranking this below "sonic 3".

#### 4. Punctuation and special characters

Query `"mario bros."` -- the period in "bros." doesn't appear in some display names.

**Decision**: The query is already lowercased. Word splitting via `split_whitespace` handles this. The period stays attached to "bros." so `"bros."` would need to start a title word. This could be improved by stripping punctuation from both query and title words before comparison, similar to how `normalize_filename` strips non-alphanumeric characters. **Recommendation**: normalize both query words and title words by stripping trailing punctuation (periods, commas, colons, exclamation marks) before comparison.

#### 5. Numbers as words

Query `"3"` alone -- this is a single-word query, so word matching does NOT activate. It goes through existing tiers. `"3"` as a substring will match many titles (scoring 1,000 for substring match). This is fine -- the user gets many results but they're all relevant.

#### 6. Hyphenated words

Title: `"X-Men - Mutant Apocalypse"` with query `"x men"`.

`split_whitespace` would produce `["X-Men", "-", "Mutant", "Apocalypse"]` for the title. The query word "x" would match "X-Men" (starts_with), and "men" would match... nothing, because no word starts with "men" ("X-Men" starts with "X", not "men").

**Recommendation**: When splitting title words, also split on hyphens. `"X-Men"` becomes `["X", "Men"]`. This allows `"x men"` to match. Apply the same to `" - "` separators (common in subtitles like `"Zelda - A Link to the Past"`).

#### 7. Performance

Word splitting adds minimal overhead. For each ROM that fails all existing tiers (returns 0), we additionally:
- Split the query into words: O(q) where q = query length (tiny, typically < 30 chars)
- Split the display name into words: O(n) where n = display name length (typically < 60 chars)
- For each query word, scan title words: O(qw * tw) where qw = query word count (2-4) and tw = title word count (3-8)

Total additional work per ROM: ~O(30) character comparisons. For a system with 10,000 ROMs where none match via substring (worst case), this adds ~300K character comparisons -- well under 1ms on any hardware, including the Pi.

The word splitting of the query string should be done **once** before the loop, not inside `search_score`. This means either:
- (a) Pre-split the query and pass `&[&str]` to `search_score`, or
- (b) Add a wrapper function that pre-splits and calls `search_score` per ROM, or
- (c) Accept the minor redundancy of re-splitting a 2-4 word query string inside `search_score` (the allocator cost is negligible for such short strings)

**Recommendation**: Option (c) for simplicity. The query is tiny and the allocation is insignificant compared to the I/O and serialization costs of the server function call.

### Implementation Plan

#### Files modified

| File | Change |
|------|--------|
| `replay-control-app/src/server_fns/search.rs` | Extended `search_score()` with word-matching fallback, added helper functions |

No changes were needed outside `search_score()`. The function's signature includes `region_pref: RegionPreference` (added by the region preference feature). The `global_search` server function and `get_roms_page` already use `search_score` correctly -- they just get more non-zero results.

#### Implementation steps

1. **Add word-matching logic to `search_score()`** after the existing `return 0` branch:
   - Check if query contains whitespace (multi-word)
   - Split query into words
   - Split display name into words (also splitting on hyphens)
   - Check if all query words match (via `starts_with`) any title word
   - If yes, compute score with order/coverage/adjacency/exact-word bonuses
   - If display name fails, try filename words (lower base score)

2. **Add helper functions** (private, in the same file):
   - `split_into_words(s: &str) -> Vec<&str>` -- split on whitespace and hyphens, strip trailing punctuation
   - `words_in_order(query_words: &[&str], title_words: &[&str]) -> bool`
   - `count_adjacent_pairs(query_words: &[&str], title_words: &[&str]) -> u32`
   - `count_exact_matches(query_words: &[&str], title_words: &[&str]) -> u32`

3. **Add tests** covering:
   - `"sonic 3"` matches `"Sonic The Hedgehog 3"` (the motivating case)
   - `"sonic 3"` ranks `"Sonic The Hedgehog 3"` above `"Sonic 3D Blast"`
   - `"zelda link"` matches `"The Legend of Zelda - A Link to the Past"`
   - `"mario kart"` matches `"Super Mario Kart"`
   - `"x men"` matches `"X-Men - Mutant Apocalypse"` (hyphen splitting)
   - Single-word queries do NOT activate word matching (unchanged behavior)
   - Queries that match via substring do NOT fall through to word matching
   - `"sonic mario"` does NOT match `"Sonic The Hedgehog 3"` (not all words present)
   - Word-match scores are always below substring-match scores (tier ordering preserved)

### Test Cases (Detailed)

```rust
// --- Word-level matching (new tier) ---

#[test]
fn word_match_sonic_3() {
    let score = search_score("sonic 3", "Sonic The Hedgehog 3", "Sonic The Hedgehog 3 (USA).md", PREF);
    assert!(score > 0, "\"sonic 3\" should match \"Sonic The Hedgehog 3\", got {score}");
    assert!(score < 1000, "Word match ({score}) should be below substring tier (1000)");
}

#[test]
fn word_match_zelda_link() {
    let score = search_score(
        "zelda link",
        "The Legend of Zelda - A Link to the Past",
        "Legend of Zelda, The - A Link to the Past (USA).sfc",
        PREF,
    );
    assert!(score > 0, "\"zelda link\" should match Zelda ALTTP");
}

#[test]
fn word_match_ranks_exact_word_above_prefix() {
    // "sonic 3" should rank "Sonic The Hedgehog 3" (exact "3") above
    // "Sonic 3D Blast" (prefix "3" matches "3D")
    let hedgehog = search_score("sonic 3", "Sonic The Hedgehog 3", "Sonic The Hedgehog 3 (USA).md", PREF);
    let blast = search_score("sonic 3", "Sonic 3D Blast", "Sonic 3D Blast (USA).md", PREF);
    assert!(hedgehog > blast, "Hedgehog 3 ({hedgehog}) should beat 3D Blast ({blast})");
}

#[test]
fn word_match_does_not_activate_for_single_word() {
    // Single-word query that fails existing tiers should still return 0
    let score = search_score("zzzznotfound", "Sonic The Hedgehog 3", "Sonic The Hedgehog 3 (USA).md", PREF);
    assert_eq!(score, 0);
}

#[test]
fn word_match_requires_all_words() {
    // "sonic mario" -- "mario" is not in "Sonic The Hedgehog 3"
    let score = search_score("sonic mario", "Sonic The Hedgehog 3", "Sonic The Hedgehog 3 (USA).md", PREF);
    assert_eq!(score, 0, "Not all query words present, should return 0");
}

#[test]
fn word_match_below_substring() {
    // A substring match should always score higher than a word match
    let substring = search_score("sonic", "Sonic The Hedgehog 3", "Sonic The Hedgehog 3 (USA).md", PREF);
    let word = search_score("sonic 3", "Sonic The Hedgehog 3", "Sonic The Hedgehog 3 (USA).md", PREF);
    assert!(substring > word, "Substring ({substring}) should beat word match ({word})");
}

#[test]
fn word_match_x_men_hyphen() {
    let score = search_score(
        "x men",
        "X-Men - Mutant Apocalypse",
        "X-Men - Mutant Apocalypse (USA).sfc",
        PREF,
    );
    assert!(score > 0, "\"x men\" should match \"X-Men\" via hyphen splitting");
}

#[test]
fn word_match_preserves_existing_substring_match() {
    // "mega man x" is a contiguous substring -- should match at substring tier, not word tier
    let score = search_score("mega man x", "Mega Man X", "Mega Man X (USA).sfc", PREF);
    assert!(score >= 5000, "Contiguous match should hit prefix tier, got {score}");
}

#[test]
fn word_match_mario_kart() {
    let score = search_score("mario kart", "Super Mario Kart", "Super Mario Kart (USA).sfc", PREF);
    // "mario kart" IS a contiguous substring of "Super Mario Kart", so this
    // should match at substring tier (1000), not word tier.
    assert!(score >= 1000, "\"mario kart\" is a substring, should score >= 1000, got {score}");
}
```

### How Other Retro Gaming UIs Handle This

**LaunchBox** uses a word-tokenized search similar to this proposal. Typing "sonic 3" finds all games where both "sonic" and "3" appear in the title. It also supports field-specific search (e.g., `developer:sega`), but that's beyond what we need.

**EmulationStation (ES-DE v3.x)** uses a simple substring search similar to our current implementation. It does NOT support word-level matching -- typing "sonic 3" will not find "Sonic The Hedgehog 3". This is a known limitation users complain about.

**RetroArch** does not have a text search feature in its standard interfaces (Ozone, XMB). The desktop WIMP interface has database-driven search that operates on metadata fields, not filenames.

**Pegasus Frontend** supports regex-based search, which power users can leverage for word matching but is not user-friendly for casual use.

Our proposed approach matches LaunchBox's behavior, which is widely considered the best search experience among retro gaming frontends.

---

## Phase 7: Rating-Based Search and Filtering

> Status: Proposed
> Date: 2026-03-11

### Problem

The app already displays ratings from LaunchBox metadata (0.0-5.0 community rating scale) on game detail pages and carries them through the `RomEntry.rating` field and `GlobalSearchResult.rating` field. However, ratings are purely informational -- users cannot filter or sort by rating, and ratings do not influence search result ranking. A user who wants to find "the best SNES platformers" has no way to express that query.

### Current Rating Data Flow

Ratings originate from the LaunchBox `CommunityRating` XML field (parsed in `launchbox.rs`) and are stored as `REAL` in the `game_metadata` SQLite table (`metadata_db.rs`). The scale is 0.0-5.0. The data flow:

1. **Storage**: `game_metadata.rating REAL` in `metadata_db.rs`, keyed by `(system, rom_filename)`
2. **Single lookup**: `MetadataDb::lookup()` returns `GameMetadata { rating: Option<f64>, ... }`
3. **Batch lookup**: `MetadataDb::lookup_ratings()` takes a system + list of filenames, returns `HashMap<String, f64>` for those with non-null ratings. Used by both `get_roms_page()` and `global_search()` to populate results after scoring/filtering
4. **Full dump**: `MetadataDb::all_ratings()` returns all `(system, rom_filename) -> f64` pairs. Currently used only by `organize_favorites()` for rating-based folder organization
5. **Display**: `RomEntry.rating: Option<f32>` (core crate) and `GlobalSearchResult.rating: Option<f32>` (search results) carry the rating to the UI, where it is shown as "X.X / 5.0"

**Important architectural detail**: Ratings are currently looked up **after** search scoring and pagination in both `get_roms_page()` (line 176-189) and `global_search()` (line 440-450). The `search_score()` function never sees ratings -- it works purely on display name, filename, and region preference. Ratings are fetched only for the final result set (the page of results being returned to the client).

### Coverage Reality

From the LaunchBox import stats: approximately **70.2%** of matched games have ratings (15,683 out of 22,356 matched entries). This means ~30% of games in a typical collection will have no rating data. Coverage varies significantly by system -- popular consoles (SNES, Genesis, NES) have high coverage while obscure systems may have very low coverage. Arcade games matched via the `arcade_db` display-name bridge also have good coverage since those tend to be well-known titles.

User ratings (from a future `user_metadata` table, see `user-editable-metadata.md`) would use the same 0.0-5.0 scale but are not yet implemented.

### Use Cases

#### 1. Filter by minimum rating

Allow users to set a minimum rating threshold, e.g., "show only games rated 3.5 or higher."

**Parameters**: `min_rating: Option<f32>` added to `get_roms_page()` and `global_search()`.

**Behavior**:
- When set, exclude all ROMs whose rating is below the threshold
- Unrated games: **excluded by default** when a minimum rating filter is active, since including them would defeat the purpose of the filter. An optional `include_unrated: bool` toggle could allow users to also see unrated games alongside the filtered results
- Applied as a pre-filter (before search scoring), same as genre and multiplayer filters

**UI**: A dropdown or slider in the filter panel: "Minimum rating: Any / 3.0+ / 3.5+ / 4.0+ / 4.5+"

#### 2. Sort by rating

Allow users to sort the game list by rating (highest first) instead of the default alphabetical sort.

**Parameters**: `sort_by: SortCriteria` enum (Alphabetical, Rating, ...) added to `get_roms_page()`.

**Behavior**:
- When active and no search query is present, sort all games by rating descending
- Unrated games are placed at the end (treated as rating 0.0 for sorting purposes)
- When a search query IS present, this conflicts with relevance sorting -- the two options should be mutually exclusive (search always sorts by relevance)

**UI**: A sort selector beside the search bar: "Sort by: Name / Rating"

#### 3. Search modifiers (future, speculative)

Natural-language-style query modifiers like "top rated" or "best" that implicitly activate rating filtering or boosting.

**Examples**:
- `"top rated"` with no other query words -> show all games sorted by rating descending
- `"best platformers"` -> genre=Platformer + sort by rating
- `"top snes"` -> within the SNES system, sort by rating

**Feasibility**: Low priority. This requires query parsing to detect modifier keywords and strip them before passing the remaining query to `search_score()`. The modifier vocabulary is hard to define exhaustively and risks false positives (e.g., a game literally called "Top Gear" should not trigger rating sorting). Explicit UI controls (filter + sort dropdowns) are more reliable and discoverable.

**Recommendation**: Do not implement search modifiers initially. Focus on explicit filter/sort controls.

#### 4. Combined with other filters

Rating filters compose naturally with existing filters:

- "4+ star platformers" = `min_rating=4.0` + `genre=Platformer`
- "Top rated multiplayer games" = `sort_by=Rating` + `multiplayer_only=true`
- "Best SNES games without hacks" = `min_rating=4.0` + `hide_hacks=true` (within SNES system page)

No special handling needed -- filters are applied sequentially in the existing pipeline.

### Rating Boost for Search Relevance

**Question**: Should highly-rated games rank higher in search results when match quality is equal?

**Analysis**: When a user searches "mario" and gets 50 results across SNES, all matching at the same tier (e.g., word-boundary match at 2,000 base score), should "Super Mario World" (rated 4.5) rank above "Mario Paint" (rated 3.2)?

**Arguments for**:
- Matches user intent: users searching by name generally want the "best" version of a game
- LaunchBox and Steam both boost popular/highly-rated results

**Arguments against**:
- Ratings are subjective and source-dependent (LaunchBox community ratings may not match user preference)
- Creates a "rich get richer" effect where highly-rated games dominate search results
- ~30% of games have no rating -- these would be systematically deprioritized
- The existing scoring already handles the most common case (original clean ROMs rank above hacks) through tier penalties and region bonuses
- Adds complexity to `search_score()` which currently works without any DB access

**Recommendation**: Do not add a rating boost to `search_score()`. The function should remain a pure function of (query, display_name, filename, region_pref) with no database dependency. Rating-based ranking belongs in the sort layer, not the relevance layer. If a user wants to find highly-rated games, they should use the explicit rating filter/sort.

If a rating boost is ever added, it should be small (e.g., +10 per full star, so a 5.0-rated game gets +50 -- less than the difference between scoring tiers) and should not penalize unrated games (treat them as neutral, not as 0-rated). The boost should only apply as a tiebreaker between results at the same scoring tier.

### Integration with the Search Scoring System

The key design decision is **where** in the pipeline ratings enter:

```
ROM list
  -> pre-filter (tier-based, genre, multiplayer, clones)      [existing]
  -> NEW: pre-filter (min_rating)                              [proposed]
  -> search scoring (search_score() -- pure, no DB)            [existing, unchanged]
  -> sort by score (or by rating if sort_by=Rating)            [modified]
  -> pagination (skip/take)                                    [existing]
  -> post-enrich (favorites, box art, ratings, players, etc.)  [existing]
```

Rating filtering happens early (pre-filter) because it eliminates rows before the more expensive search scoring. Rating sorting replaces relevance sorting only when explicitly requested and no search query is active.

**`search_score()` remains unchanged** -- it is a pure function with no side effects and no DB access. This is important for testability and performance (no DB lookup per ROM during scoring).

### Performance Considerations

#### Current lookup pattern

Both `get_roms_page()` and `global_search()` use `lookup_ratings()` as a **post-enrichment** step: they score and paginate first, then fetch ratings only for the final page of results (typically 20-50 ROMs). This is efficient because it minimizes DB queries.

#### Impact of rating as a filter

If `min_rating` is added as a pre-filter, ratings must be available **before** scoring -- for every ROM in the system, not just the final page. Two approaches:

**Option A: Batch pre-load all ratings for the system**

Use `lookup_ratings()` with all ROM filenames in the system, or add a new `MetadataDb::system_ratings(system: &str) -> HashMap<String, f64>` method that fetches all ratings for a system in one query. For a system with 5,000 ROMs, this is a single SQLite query returning ~3,500 rows (70% coverage) -- fast even on the Pi.

Cost: one extra DB query per `get_roms_page()` call when the rating filter is active. The HashMap stays in memory for the duration of the request (not cached across requests).

**Option B: Cache ratings in `RomCache`**

Extend the existing `RomCache` (which caches ROM lists for 30 seconds) to also cache ratings per system. This amortizes the DB cost across multiple requests but adds memory usage and cache invalidation complexity.

**Recommendation**: Option A for simplicity. A single `SELECT system, rom_filename, rating FROM game_metadata WHERE system = ?1 AND rating IS NOT NULL` query is well within acceptable latency for a request-scoped lookup. Only execute this query when a rating filter or rating sort is actually requested.

#### Impact on `global_search()`

Global search iterates over all systems. If a rating filter is active, it would need to fetch ratings for every system -- potentially 20+ DB queries. This is acceptable because:
- Each query is fast (single indexed lookup on system)
- Global search already iterates all systems and all ROMs (it's inherently expensive)
- The rating filter would reduce the number of scored ROMs, offsetting the DB cost

For the sort-by-rating case in global search, the system-level grouping makes it less useful: rating sorting within each system group is meaningful, but cross-system rating comparison is less so (a 4.0 on SNES may not be comparable to a 4.0 on NES). The global search could sort systems by their top-rated match.

### UX Considerations

#### Unrated games

The biggest UX challenge. With ~30% of games unrated, any rating-based feature must handle the "no data" case gracefully:

1. **Filter with min_rating**: Unrated games are excluded. The UI should clearly indicate this: "Showing 847 games rated 4.0+ (2,341 unrated games hidden)". An "Include unrated" toggle provides an escape hatch.

2. **Sort by rating**: Unrated games sort to the bottom. The list effectively becomes "rated games by quality, then unrated games alphabetically." The UI could show a visual separator or label: "--- Unrated ---"

3. **Rating display in results**: Already handled -- `rating: Option<f32>` renders as stars when present, nothing when absent. No change needed.

#### Rating distribution awareness

LaunchBox community ratings tend to cluster in the 2.5-4.5 range. Very few games are rated below 2.0 or at exactly 5.0. The filter thresholds should reflect this:
- "3.0+" captures most games with ratings (too broad to be useful alone, but good combined with genre)
- "4.0+" is a meaningful "good games" threshold
- "4.5+" is "excellent" and will produce a short list

The UI should NOT offer "1.0+" or "2.0+" thresholds -- these would be virtually identical to "show all rated games" and waste dropdown space.

#### System-level variation

Some systems have excellent rating coverage (popular consoles), while others may have near-zero coverage (obscure systems, certain arcade boards). When the user activates a rating filter on a system with poor coverage, they might see an unexpectedly empty list.

**Mitigation**: When the rating filter produces zero results but unfiltered results exist, show a message: "No rated games found for this system. Try removing the rating filter or importing metadata."

#### Discoverability

Rating filter/sort should be in the same filter panel as existing filters (hide hacks, genre, multiplayer). It should not require a separate UI affordance. The filter panel already exists in `rom_list.rs` -- the rating controls would be additional items.

### Files to Modify (When Implemented)

| File | Change |
|------|--------|
| `replay-control-app/src/server_fns/roms.rs` | Add `min_rating` and `sort_by` parameters to `get_roms_page()` |
| `replay-control-app/src/server_fns/search.rs` | Add `min_rating` parameter to `global_search()`; no changes to `search_score()` |
| `replay-control-core/src/metadata_db.rs` | Optionally add `system_ratings()` method for efficient per-system batch lookup |
| `replay-control-app/src/components/rom_list.rs` | Add rating filter dropdown and sort selector to the filter panel |
| `replay-control-app/src/pages/search.rs` | Add rating filter to global search page |

### Dependencies

- **Metadata import**: Rating data requires LaunchBox metadata to be imported. Without import, all games are unrated and rating features are inert. The UI should indicate when metadata is not available.
- **User ratings (future)**: The `user-editable-metadata.md` proposal describes a `user_metadata` table where users can set their own ratings. When implemented, the rating lookup should merge user ratings (preferred) with LaunchBox ratings (fallback), using the same 0.0-5.0 scale. The filter/sort infrastructure built here would work unchanged with user ratings.

### Summary of Recommendations

1. **Add `min_rating` filter** to `get_roms_page()` and `global_search()` as a pre-filter. Exclude unrated games when the filter is active (with optional include toggle).
2. **Add `sort_by: Rating`** option to `get_roms_page()` for non-search browsing. Unrated games sort last.
3. **Do not modify `search_score()`**. Ratings should not influence search relevance scoring. Keep the function pure and DB-free.
4. **Do not implement search modifiers** ("top rated", "best") -- use explicit UI controls instead.
5. **Use request-scoped batch lookup** (Option A) rather than caching ratings in `RomCache`.
6. **Handle unrated games explicitly** in the UI with counts, messages, and an "include unrated" toggle.
