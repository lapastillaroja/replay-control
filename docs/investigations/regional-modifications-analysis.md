# Regional Modifications Analysis

## What are regional modifications?

Regional modifications are ROMs that have been patched to add features while retaining the same base game and region classification. Unlike regional *variants* (USA vs Europe vs Japan releases of the same game), regional modifications produce multiple ROMs within the **same region** for the same title.

Common types:

- **Translation patches** -- A Spanish translation applied to a USA ROM: both the original and the translation are region "usa"
- **FastROM patches** -- Performance patches that reduce slowdown on SNES games: the original and the patched version share the same region
- **60Hz patches** -- PAL/Europe ROMs patched to run at 60Hz instead of 50Hz
- **Revisions** -- Publisher bug fixes or minor updates (Rev 1, Rev A)
- **Undub patches** -- Japanese audio restored to a USA/Europe version

## Current behavior

### Regional variants chip row

The game detail page shows a "Regional Variants" section as a row of clickable chips. The `regional_variants()` query returns ROMs sharing the same `base_title` and `system`, filtering out translations (`is_translation = 0`) and hacks (`is_hack = 0`).

**Problem:** FastROM patches and revisions are NOT flagged as translations or hacks. They pass through the filter and appear as variant chips. When a game has both `ActRaiser (USA).sfc` and `Actraiser (USA) (FastRom).sfc`, both appear as chips labeled "usa" -- duplicated labels with no way for the user to tell them apart.

### Translations and Hacks sections

These are shown in separate chip rows, filtered by `is_translation = 1` and `is_hack = 1` respectively. This works correctly for ROMs that rom_tags.rs classifies into those tiers.

### Recommendations dedup

The dedup CTE in recommendation queries uses:

```sql
WITH deduped AS (
    SELECT *, ROW_NUMBER() OVER (
        PARTITION BY system, base_title
        ORDER BY CASE WHEN region = ?pref THEN 0 WHEN region = 'world' THEN 1 ELSE 2 END
    ) AS rn
    FROM rom_cache
    WHERE is_clone = 0 AND is_translation = 0 AND is_hack = 0
)
SELECT ... FROM deduped WHERE rn = 1
```

This filters out translations and hacks, but **FastROM patches and revisions compete with originals in the same partition**. The winner is determined solely by region priority -- if both a clean original and a FastROM patch have the same region, it is non-deterministic which one wins (both get the same ORDER BY score, so SQLite picks arbitrarily).

## Data from the romset

Analysis script: `tools/analyze_regional_mods.py`
Romset: `<NFS_MOUNT>/roms/`

### Per-system statistics

| System | ROMs | Titles | Mod Groups | Mod ROMs |
|--------|------|--------|------------|----------|
| nintendo_snes | 4,464 | 2,498 | 582 | 975 |
| sega_smd | 3,096 | 1,744 | 384 | 535 |
| sega_sms | 1,025 | 578 | 84 | 114 |
| sega_gg | 729 | 450 | 75 | 86 |
| sharp_x68k | 3,163 | 1,275 | 6 | 12 |
| **TOTAL** | **62,987** | **36,267** | **1,131** | **1,722** |

"Mod Groups" = number of (base_title, region) pairs with more than one ROM.
"Mod ROMs" = total non-original ROMs that share a group with an original.

### ROM tier breakdown

| Tier | Count | % |
|------|-------|---|
| Original | 58,652 | 93.1% |
| Translation | 2,696 | 4.3% |
| Region Variant | 335 | 0.5% |
| Homebrew | 345 | 0.5% |
| Revision | 315 | 0.5% |
| Unlicensed | 309 | 0.5% |
| Pre-release | 258 | 0.4% |
| Hack | 44 | 0.1% |
| Pirate | 33 | 0.1% |

### Modification types within same-region groups

When a game has multiple ROMs sharing the same base_title AND region, the modifications break down as:

| Type | Count | % |
|------|-------|---|
| Translation | 1,638 | 95.1% |
| Homebrew | 33 | 1.9% |
| Revision | 20 | 1.2% |
| Unlicensed | 17 | 1.0% |
| Pre-release | 13 | 0.8% |
| FastROM | 1 | 0.1% |

### Duplicate region labels in the variants chip row

The `regional_variants()` query (which excludes translations and hacks) produces **450 games** with duplicate region labels. These are cases where two or more chips in the "Regional Variants" row would show the same region text.

Causes:

| Cause | Count |
|-------|-------|
| "Other" region bucket collisions | 184 |
| FastROM patches | 145 |
| Aftermarket/Homebrew | 51 |
| Unlicensed | 46 |
| Revision | 32 |
| Pre-release | 5 |

The "Other" category (184) is mostly legitimate -- multiple country-specific releases (France, Germany, Italy, Spain) all map to `region_priority = "other"`. These are distinct releases that share the catch-all bucket.

The FastROM category (145) represents the main problem: 145 SNES games have both a clean original and a FastROM-patched version with identical region classification, causing duplicate "usa" or "japan" chips.

### FastROM patch details

- 213 total FastROM ROMs on SNES
- 184 pure FastROM patches (no other modification)
- 29 FastROM + translation combos
- 143 overlap groups where a FastROM patch and clean original share the same (title, region)

### N64 bare translations (known limitation)

5 N64 ROMs use bare `T+Eng` in the filename without brackets (e.g., `Chameleon Twist (J) T+Eng v1 Zoinkity.z64`). The bracket parser does not detect these, so they are classified as originals. This is a known limitation documented in the rom_tags.rs tests.

## Classification of modification types

### 1. Translation patches (95.1% of same-region modifications)

The dominant category. These are correctly handled -- `is_translation = 1` filters them out of the variants row and into the dedicated "Translations" section.

Patterns:
- `(Traducido Es)`, `(Traduzido Por)`, `(Translated En/Fre/Ger/Ita/Swe/Pol/...)`
- `[T-Spa1.0v_Wave]`, `[T+Fre]`, `[T+Rus Pirate]`, `[T+Bra_TMT]`
- `(PT-BR)` standalone

### 2. FastROM patches (145 duplicate-label cases)

SNES-specific performance patches that reduce slowdown. These are NOT classified as a special tier -- they appear as `tier = original` because `classify()` does not check for FastROM. They are only detected by `extract_tags()` for display purposes.

Impact: In the variants chip row, a FastROM patch shows as a duplicate region chip indistinguishable from the clean original. In dedup, it competes with the clean original non-deterministically.

### 3. Revisions (32 duplicate-label cases)

Publisher updates like `(Rev 1)`, `(Rev A)`. These are classified as `tier = revision` but NOT filtered by `is_translation` or `is_hack`, so they appear in the variants row. The label is just the region -- no indication it is a revision.

### 4. Country-specific "Other" releases (184 cases)

Multiple releases for specific countries (France, Germany, Italy, Spain, Brazil) all map to `region_priority = "other"`. In the variants chip row they all show as "other" -- indistinguishable. These are legitimate separate releases, not patches.

### 5. Homebrew/Unlicensed/Pre-release (102 cases combined)

Various special ROMs that share a base_title with an original. Not currently filtered from the variants query.

## UX implications

### Current problems

1. **Duplicate region chips**: 450 games show two or more chips with the same label in the "Regional Variants" row. The user cannot tell which is the original and which is the modified version.

2. **FastROM patches invisible in recommendations**: Since FastROM patches compete in the dedup partition with the same region score as the original, the recommendation engine might show a FastROM patch instead of the clean original (or vice versa) non-deterministically.

3. **Country-specific "other" labels**: Games like Pokemon Snap with France, Germany, Italy, Spain releases all show as "other" -- the user sees five identical "other" chips with no way to distinguish them.

4. **Revisions mixed with originals**: A Rev 1 and the original both appear as regional variant chips with the same region label.

### What users would expect

- The "Regional Variants" row should show one chip per distinct release, with enough information to tell them apart
- FastROM patches are a niche feature most users do not care about; they should not pollute the main variant row
- Revisions could be shown as variants but need a label that indicates the revision
- Country-specific releases should show the country name, not "other"

## Impact on recommendations

### Queries affected

Five queries in `metadata_db.rs` use filtering or dedup logic that is affected by unclassified special ROMs:

| Query | Dedup CTE | Filters `is_translation` | Filters `is_hack` | Filters `is_clone` |
|-------|-----------|--------------------------|--------------------|--------------------|
| `random_cached_roms_diverse` | Yes (PARTITION BY system, base_title) | Yes | Yes | Yes |
| `top_rated_cached_roms` | Yes (PARTITION BY system, base_title) | Yes | Yes | Yes |
| `system_roms_excluding` | Yes (PARTITION BY system, base_title) | Yes | Yes | Yes |
| `similar_by_genre` | No (flat query) | Yes | Yes | Yes |
| `random_cached_roms` | No (flat query) | No | No | No |
| `regional_variants` | No (flat query) | Yes | Yes | No |

The first three use the same dedup CTE pattern. `similar_by_genre` filters the same flags but without dedup. `random_cached_roms` (per-system, non-diverse) has **no filtering at all** -- it can return any ROM including translations, hacks, and special ROMs.

### What can leak through today

The current filters (`is_clone = 0 AND is_translation = 0 AND is_hack = 0`) catch three categories. Everything else passes through unfiltered:

| ROM category | `classify()` tier | Caught by current filters? | Can appear in recommendations? |
|---|---|---|---|
| Clean original | `Original` | No (correctly included) | Yes -- intended |
| Translation | `Translation` | Yes (`is_translation = 1`) | No -- correct |
| Hack | `Hack` | Yes (`is_hack = 1`) | No -- correct |
| Arcade clone | (from arcade_db) | Yes (`is_clone = 1`) | No -- correct |
| **FastROM patch** | `Original` (misclassified) | **No** | **Yes -- unintended** |
| **60Hz patch** | `Original` (misclassified) | **No** | **Yes -- unintended** |
| **Revision** | `Revision` | **No** | **Yes -- generally fine, but competes with original in dedup** |
| **Unlicensed** | `Unlicensed` | **No** | **Yes -- unintended** |
| **Homebrew / Aftermarket** | `Homebrew` | **No** | **Yes -- unintended** |
| **Pre-release (Beta/Proto/Demo)** | `PreRelease` | **No** | **Yes -- unintended** |
| **Pirate** | `Pirate` | **No** | **Yes -- unintended** |

### Specific scenarios

**Can a FastROM patch appear in "Top Rated"?**
Yes. `classify()` does not check for `(FastRom)` or `(60hz)` tags -- these ROMs get `tier = Original`. They enter the dedup CTE with `is_translation = 0 AND is_hack = 0`, then compete in the `PARTITION BY system, base_title` window. If `Actraiser (USA).sfc` and `Actraiser (USA) (FastRom).sfc` both exist, both have `region = "usa"` and get the same ORDER BY score of `0` (assuming `region_pref = "usa"`). SQLite's `ROW_NUMBER()` assigns `rn = 1` to one of them **non-deterministically** -- the FastROM patch has a 50% chance of winning. If it wins and has a high `rating`, it appears in "Top Rated".

**Can an unlicensed ROM appear in "Random"?**
Yes. `classify()` returns `tier = Unlicensed` for `(Unl)` / `(Unlicensed)` ROMs, but the cache only stores `is_translation` and `is_hack` -- there is no `is_unlicensed` column. These ROMs have `is_translation = 0 AND is_hack = 0`, so they pass the dedup CTE filter. An unlicensed ROM with a unique `base_title` (no clean original to compete with) will always get `rn = 1` and appear in recommendations. Example: `Wisdom Tree` games on NES.

**Can homebrew appear in "Because You Love" (system_roms_excluding)?**
Yes, same mechanism. A `(Homebrew)` ROM with `is_hack = 0 AND is_translation = 0` passes the filter. If it has a genre and rating, it enters the pool.

**Can a beta/prototype appear in recommendations?**
Yes. `(Beta)`, `(Proto)`, `(Demo)` ROMs get `tier = PreRelease` but no corresponding cache column. They pass all current filters.

### The PARTITION BY dedup race condition

The dedup CTE picks one ROM per `(system, base_title)` group using:

```sql
ROW_NUMBER() OVER (
    PARTITION BY system, base_title
    ORDER BY CASE WHEN region = ?pref THEN 0 WHEN region = 'world' THEN 1 ELSE 2 END
) AS rn
```

When two ROMs share the same `base_title`, `system`, AND `region`, they get identical ORDER BY scores. SQLite does not guarantee a stable sort for ties -- `ROW_NUMBER()` assigns 1 to whichever row the query planner happens to encounter first. This means:

- `Sonic (Europe).sfc` vs `Sonic (Europe) (FastROM Patch).sfc` -- **non-deterministic winner**
- `Game (USA).sfc` vs `Game (USA) (Unlicensed).sfc` -- **non-deterministic winner** (if they somehow share a base_title)
- `Game (USA).sfc` vs `Game (USA) (Beta).sfc` -- **non-deterministic winner**

The user could see a different version of the same game on each page load. Worse, if the FastROM/beta/unlicensed version lacks box art or metadata, the recommendation card may look broken.

### What changes are needed

Once the new `is_special` flag is added (see Step 2 below), all recommendation queries need one additional filter:

```sql
AND is_special = 0
```

This applies to:

1. **`random_cached_roms_diverse`** -- add `AND is_special = 0` to the WHERE clause of the dedup CTE
2. **`top_rated_cached_roms`** -- same
3. **`system_roms_excluding`** -- same (both the genre-filtered and unfiltered branches)
4. **`similar_by_genre`** -- add `AND is_special = 0` to the flat WHERE clause
5. **`random_cached_roms`** -- this query currently has NO dedup and NO special filtering; add at minimum `AND is_special = 0`
6. **`regional_variants`** -- add `AND is_special = 0` to exclude special ROMs from the variant chip row

The dedup CTE ORDER BY does not need to change -- once special ROMs are excluded from the input set, the partition will only contain clean originals and revisions. Revisions competing with originals is acceptable (revisions are minor updates from the publisher and are fine to recommend).

## Proposed approach

**Recommendation: Option C with elements of Option A**

### Core idea

Improve the variant display to handle same-region duplicates gracefully, and improve dedup to consistently prefer unmodified originals.

### Changes needed

#### 1. Fix the `region` column for country-specific releases

Currently, `region_to_priority()` maps France, Germany, Italy, Spain, etc. all to `RegionPriority::Other`, and the cache stores the string "other". The `regional_variants()` query returns this string as the chip label.

**Fix:** Store the *display* region (the full text like "France", "Germany") in the `region` column instead of the priority bucket name. Use a separate `region_priority` column (or compute it at query time) for sorting. This way chips show "France", "Germany" etc. instead of all showing "other".

This is the biggest bang-for-buck change -- it fixes 184 of the 450 duplicate-label cases and makes country-specific variants actually useful.

#### 2. Add `is_special` flag for non-standard ROMs

Add a single boolean column `is_special INTEGER NOT NULL DEFAULT 0` to the `rom_cache` table. This catches **all ROM categories that should not appear in normal recommendations or the regional variants chip row**:

| Category | Tag patterns detected | `classify()` tier today | Count in romset |
|---|---|---|---|
| FastROM patches | `(FastRom)`, `(FastROM)` | `Original` (misclassified) | 213 |
| 60Hz patches | `(60hz)`, `(60Hz)` | `Original` (misclassified) | ~few |
| Unlicensed | `(Unl)`, `(Unlicensed)` | `Unlicensed` | 309 |
| Homebrew / Aftermarket | `(Homebrew)`, `(Aftermarket)` | `Homebrew` | 345 |
| Pre-release | `(Beta)`, `(Beta N)`, `(Proto)`, `(Prototype)`, `(Proto N)`, `(Demo)`, `(Demo N)` | `PreRelease` | 258 |
| Pirate | `(Pirate)` | `Pirate` | 33 |

Total: ~1,158 ROMs (1.8% of the romset) would be flagged as `is_special = 1`.

**Why a single flag?** These categories share the same behavior: they should be excluded from recommendations, excluded from the regional variants chip row, and optionally shown in a dedicated "Special Versions" section on the game detail page. Individual per-category columns (`is_patch`, `is_aftermarket`, `is_unlicensed`, `is_prerelease`) would add schema complexity for no practical UX benefit -- the user does not need to filter "show me only unlicensed ROMs in recommendations but not homebrew". A single flag keeps the queries simple.

**Why not `is_patch`?** The original proposal of `is_patch` only covers FastROM/60Hz (213 ROMs). That leaves 945 other non-standard ROMs leaking into recommendations. The `is_special` approach catches them all with one column and one filter condition.

**What about revisions?** Revisions (`(Rev 1)`, `(Rev A)`) are intentionally NOT flagged as special. They are publisher-issued updates to the original game and are legitimate recommendations. They do cause duplicate labels in the variant chip row (32 cases), but that is handled separately by Step 3 (using `extract_tags()` for richer chip labels).

Then:

- Exclude `is_special = 1` from `regional_variants()` (like translations and hacks are excluded)
- Exclude `is_special = 1` from all dedup CTEs (so clean originals always win)
- Exclude `is_special = 1` from `similar_by_genre()` and `random_cached_roms()`
- Optionally show special ROMs in a dedicated section on the game detail page

This fixes 248 of the 450 duplicate-label cases (145 FastROM + 51 homebrew + 46 unlicensed + 5 pre-release + 1 pirate overlap) and ensures recommendations only show clean originals and revisions.

#### 3. Improve variant chip labels for revisions

When `regional_variants()` returns results, the chip label is currently just the region. For revisions, append the revision info to make it e.g., "USA, Rev 1" instead of just "USA".

**Implementation:** The chip label could use `extract_tags()` output instead of the raw region. This already produces strings like "USA, Rev 1" and "Europe, 60Hz". The `RegionalVariant` struct already has `rom_filename` -- the server function can call `extract_tags()` on it to get a richer label.

#### 4. (Superseded by Step 2)

The original Step 4 proposed individual columns for homebrew/unlicensed/prerelease. This is now handled by the single `is_special` flag in Step 2, which catches all these categories with one column and one filter condition.

### Priority order

1. **Fix region display labels** (biggest impact, 184 cases, improves UX for legitimate variants)
2. **Add `is_special` and exclude from variants/dedup/recommendations** (248 duplicate-label cases fixed, all non-standard ROMs filtered from recommendations)
3. **Use `extract_tags()` for chip labels** (32 revision cases, makes revisions distinguishable)

## Implementation sketch

### Step 1: Store display region in rom_cache

In `cache.rs` where `CachedRom` is built, change the region mapping:

```
// Instead of mapping to priority bucket names:
//   RegionPriority::Other => "other"
// Map to the actual display region from extract_tags():
//   "France", "Germany", "Spain", etc.
```

The `extract_tags()` function already normalizes regions to display strings. Extract just the region part (first component before any comma in the tags output, or the region from the paren tags).

Add a `region_priority` field to `CachedRom` (u8 or string) for use in dedup ORDER BY, keeping `region` as the display string.

Update dedup CTEs to use `region_priority` instead of `region` for the CASE expression.

### Step 2: Add `is_special` classification

#### 2a. Extend `classify()` in `rom_tags.rs`

`classify()` already detects all the relevant tiers (`Unlicensed`, `Homebrew`, `PreRelease`, `Hack`, `Pirate`) but does NOT detect FastROM/60Hz patches -- those fall through to `Original`. Two changes needed:

1. Add patch detection to `classify()`: check for `(FastRom)` and `(60hz)` tags, similar to how `extract_tags()` already detects them. These could either get a new `RomTier::Patch` variant or set a separate boolean flag that `classify()` returns alongside the existing `(RomTier, RegionPriority)` tuple.

2. Handle `(Sample)` as pre-release: currently `(Sample)` is in `is_noise_tag()` and gets silently ignored by `classify()`, resulting in `tier = Original`. It should be treated like `(Demo)` and classified as `PreRelease`.

Detection patterns for each category:

```
FastROM/60Hz patches (new detection):
  Parenthesized: (FastRom), (FastROM), (60hz), (60Hz)
  Match: lower == "fastrom" || lower == "60hz"

Unlicensed (already detected by classify):
  Parenthesized: (Unl), (Unlicensed)
  Match: lower == "unl" || lower == "unlicensed"

Homebrew / Aftermarket (already detected by classify):
  Parenthesized: (Homebrew), (Aftermarket)
  Match: lower == "aftermarket" || lower == "homebrew"

Pre-release (already detected by classify, except Sample):
  Parenthesized: (Beta), (Beta 1), (Proto), (Prototype), (Proto 2),
                 (Demo), (Demo 1), (Sample)
  Match: lower == "beta" || lower.starts_with("beta ")
         || lower == "proto" || lower == "prototype" || lower.starts_with("proto ")
         || lower == "demo" || lower.starts_with("demo ")
         || lower == "sample"

Pirate (already detected by classify):
  Parenthesized: (Pirate)
  Match: lower == "pirate"
```

#### 2b. Approach options

**Option A: Extend the return type of `classify()`**

Change `classify()` to return a triple `(RomTier, RegionPriority, bool)` where the third element is `is_special`. This is the simplest approach since it avoids adding a new tier variant for patches.

```
let is_special = matches!(tier,
    RomTier::Unlicensed | RomTier::Homebrew | RomTier::PreRelease | RomTier::Pirate
) || is_fastrom || is_60hz;
```

**Option B: Add `RomTier::Patch` and derive `is_special` from tier**

Add a `Patch` variant to `RomTier`. Then in `cache.rs`, derive `is_special` from the tier:

```
let is_special = !matches!(tier,
    RomTier::Original | RomTier::Revision | RomTier::RegionVariant | RomTier::Translation | RomTier::Hack
);
```

This excludes `Translation` and `Hack` from `is_special` because they already have their own dedicated columns (`is_translation`, `is_hack`). The `is_special` flag covers everything else that should be hidden from recommendations.

Option A is recommended -- it is less invasive and does not change the existing tier ordering.

#### 2c. Set `is_special` in `cache.rs`

Follow the same pattern as `is_translation` and `is_hack`:

```rust
let (tier, region_priority) = replay_control_core::rom_tags::classify(rom_filename);
let is_translation = tier == RomTier::Translation;
let is_hack = tier == RomTier::Hack;

// New: detect all non-standard ROMs
let is_special = matches!(tier,
    RomTier::Unlicensed | RomTier::Homebrew | RomTier::PreRelease | RomTier::Pirate
) || has_patch_tag(rom_filename);  // FastROM/60Hz check
```

Where `has_patch_tag()` is a small helper (or inline check) that scans for `(FastRom)` / `(60hz)` in the filename. Alternatively, if `classify()` is extended to return the `is_special` flag directly (Option A above), just use that.

#### 2d. Add column and update queries in `metadata_db.rs`

Schema change:

```sql
CREATE TABLE IF NOT EXISTS rom_cache (
    ...
    is_translation INTEGER NOT NULL DEFAULT 0,
    is_hack INTEGER NOT NULL DEFAULT 0,
    is_special INTEGER NOT NULL DEFAULT 0,   -- NEW
    PRIMARY KEY (system, rom_filename)
);
```

Query changes -- add `AND is_special = 0` to:

- `random_cached_roms_diverse` dedup CTE WHERE clause
- `top_rated_cached_roms` dedup CTE WHERE clause
- `system_roms_excluding` dedup CTE WHERE clause (both genre-filtered and unfiltered branches)
- `similar_by_genre` WHERE clause
- `random_cached_roms` WHERE clause
- `regional_variants` WHERE clause

Example for the dedup CTE (same change in all three queries):

```sql
-- Before:
WHERE is_clone = 0 AND is_translation = 0 AND is_hack = 0

-- After:
WHERE is_clone = 0 AND is_translation = 0 AND is_hack = 0 AND is_special = 0
```

Example for `regional_variants`:

```sql
-- Before:
WHERE system = ?1 AND base_title != '' AND is_translation = 0 AND is_hack = 0 AND ...

-- After:
WHERE system = ?1 AND base_title != '' AND is_translation = 0 AND is_hack = 0 AND is_special = 0 AND ...
```

Add `is_special` to the `CachedRom` struct and `row_to_cached_rom` mapper. Update SELECT column lists in all queries that read from `rom_cache`.

#### 2e. Migration

The `rom_cache` table is rebuilt from scratch on each cache scan (not a persistent user database), so there is no migration concern. Adding the column to the CREATE TABLE statement and the INSERT logic is sufficient. Existing caches will be rebuilt on next scan.

#### 2f. Effort estimate

- `rom_tags.rs`: ~15 lines (add FastROM/60Hz detection to `classify()`, move `sample` from noise to pre-release)
- `cache.rs`: ~5 lines (compute `is_special`, add to `CachedRom` construction)
- `metadata_db.rs`: ~20 lines (add column to schema, add field to struct/mapper, add filter to 6 queries)
- Tests: ~20 lines (verify classify returns correct tiers for FastROM/60Hz/Sample, verify is_special derivation)
- Total: ~60 lines, ~1-2 hours

## Impact on "Change Cover" feature

### How the cover picker works

The "Change Cover" feature on the game detail page uses `thumbnail_index` (the libretro-thumbnails manifest stored in SQLite) as its data source — NOT `rom_cache`. The function `find_boxart_variants()` in `thumbnail_manifest.rs` works as follows:

1. Strips the ROM filename to its base title using `strip_tags()` + `thumbnail_filename()` (lowercased, no region/tag info)
2. Queries the `thumbnail_index` table for all `Named_Boxarts` entries whose stripped filename matches
3. Returns each match as a `BoxArtVariant` with region label, download status, and image URL

Because the data comes from official libretro-thumbnails repos (which contain only publisher-released cover art), the picker **already shows clean, curated content**. There are no "FastROM" or "Homebrew" cover variants in the thumbnail repos — those are community ROM patches, not separate commercial releases. Regional variants in the thumbnail repos represent actual different box art prints (USA vs Japan cover art), which is exactly what the user wants to choose from.

### Current visibility logic

The "Change Cover" link and tappable cover are controlled by:

```rust
let has_variants = variant_count > 1 && !detail.is_hack;
```

This already suppresses the affordance for hack ROMs (added in the hacks feature implementation). Translations are NOT suppressed — the link is still visible for translation ROMs.

### Should `is_special` or `is_translation` suppress the link?

**No — suppression is not needed.** The picker works correctly for all ROM types because it matches by base title against the thumbnail index. A translation ROM like `Sonic 2 (USA) (Traducido Es).sfc` strips to the same base title as `Sonic 2 (USA).sfc` and shows the same cover art options. The user can still legitimately want to change the cover for a translation they play regularly.

The hack suppression (`!detail.is_hack`) is a special case — hacks often have completely different game content (e.g., "Kaizo Mario"), so showing cover art from the original game would be misleading. This does not apply to translations, FastROM patches, unlicensed games, or homebrew, where the cover art is either the same game or the ROM is unique enough to have its own entry in the thumbnail repos.

**Summary:** No changes needed for the cover picker. The `is_hack` suppression already in place is sufficient.

### Step 3: Use extract_tags() for variant chip labels

In `related.rs`, when building `RegionalVariant` chips, use `extract_tags(&rom_fn)` as the label instead of the raw region string. This already produces rich labels like "USA, Rev 1", "Europe, 60Hz", "France".
