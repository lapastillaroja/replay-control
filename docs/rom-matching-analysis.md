# ROM Matching Analysis

Analysis of how well the game metadata DB matches actual ROM files on disk.

**Date**: 2026-03-09
**ROM storage**: `<USB_MOUNT>/roms/`

## Executive Summary

The current matching logic requires an **exact filename stem match** against No-Intro
canonical names in a PHF map. Across 12,801 ROM files in 6 systems with game_db coverage,
only **6,080 (47.5%)** match. A single improvement -- normalized title fallback matching --
would rescue 4,466 additional files, bringing the match rate to **82.4%**. Combined with
tilde-separated title handling, the rate reaches **82.7%**.

## Current Matching Logic

The lookup path (`game_db::game_display_name`) is:

1. Strip file extension from ROM filename: `"Super Mario World (USA).sfc"` -> `"Super Mario World (USA)"`
2. Exact lookup in system's PHF map keyed by No-Intro filename stems
3. If no match, `GameRef::new()` returns `display_name: None`

There is an existing CRC32 fallback (`lookup_by_crc`), but it is **never called** from
`GameRef::new()` or `game_display_name()`. It exists only as an unused API.

## Per-System Match Statistics

| System | Total ROMs | Exact Match | Match Rate | With Title Fallback | Combined Rate |
|--------|-----------|-------------|------------|---------------------|---------------|
| nintendo_snes | 7,282 | 3,436 | 47.2% | +2,268 | 78.3% |
| sega_smd | 3,101 | 1,234 | 39.8% | +1,413 | 85.4% |
| sega_sms | 1,035 | 416 | 40.2% | +496 | 88.1% |
| sega_gg | 729 | 370 | 50.8% | +272 | 88.1% |
| nintendo_n64 | 639 | 614 | 96.1% | +13 | 98.1% |
| nintendo_nes | 15 | 10 | 66.7% | +4 | 93.3% |
| **TOTAL** | **12,801** | **6,080** | **47.5%** | **+4,466** | **82.4%** |

N64 has excellent match rates because the user's collection is mostly a clean No-Intro set.
SNES, Mega Drive, and SMS have poor rates because the collections include many translations,
regional variants, patches, and homebrew alongside the clean set.

## Failure Mode Breakdown

Categorized across all systems (6,721 unmatched files):

### 1. Translation/Language Hacks (2,364 files, 18.5% of all ROMs)

ROM files that are fan translations or language patches. They share a base title with a
DB entry but have extra tags the DB doesn't know about.

**Patterns**:
- `Game Name (USA) (Traducido Es).smc` -- Spanish translation
- `Game Name (USA) (Traduzido Por).smc` -- Portuguese translation
- `Game Name (Japan) (Translated En).sfc` -- English translation
- `Game Name (E) [T-Spa1.0v_Wave].sms` -- GoodTools-style translation tag
- `Game Name (J) T+Eng v1 Author.z64` -- Inline translation credit

**Rescued by title fallback**: Yes. The base title before parentheses matches.

### 2. Region/Tag Variants (706 files, 5.5%)

Files with the same game title that exist in the DB but under a different region or with
different secondary tags (language codes, revision numbers, etc.).

**Examples**:
- `2020 Super Baseball (USA).smc` -- DB only has `(Japan)` variant
- `Altered Beast (USA, Europe).md` -- DB has `(USA, Europe)` but with different suffix
- `Columns (USA, Europe, Brazil) (Rev 2).gg` -- DB has different revision

**Rescued by title fallback**: Yes.

### 3. PAL-to-NTSC Patches / 60Hz Variants (630+ files, 4.9%)

Primarily SNES. European ROMs patched for 60Hz output, marked with `(60hz)` tag.

**Examples**:
- `ActRaiser (Europe) (60hz).sfc`
- `Asterix & Obelix (Europe) (En,Fr,De,Es) (60hz).sfc`

**Rescued by title fallback**: Yes.

### 4. FastROM Patches (182 files, 1.4%)

SNES ROMs patched for faster ROM access timing, marked with `(FastRom)`.

**Examples**:
- `Acrobat Mission (Japan) (FastRom).sfc`
- `Adventures of Batman and Robin, The (USA) (FastRom).sfc`

**Rescued by title fallback**: Yes.

### 5. Hacks (303 files, 2.4%)

Modified versions of existing games.

**Examples**:
- `BS Zelda Adventuras de Pikachu - Map One (Hack).sfc`
- `Mega Man X Alpha V.1.0.smc`

**Rescued by title fallback**: Sometimes (if the hack preserves the original title prefix).

### 6. Homebrew / Public Domain (305 files, 2.4%)

Games not in the No-Intro database because they are community-created.

**Examples**:
- `Bio Worm (Homebrew).smc`
- `2048 on Sega Mega Drive (Homebrew).bin`
- `77a Special Edition (PD) POM '98 v1 Count0.z64`

**Rescued by title fallback**: No. These titles don't exist in the DB at all.

### 7. Aftermarket / Unlicensed (430 files, 3.4%)

Commercially released games that aren't in the standard No-Intro DAT, or unlicensed
titles not covered by the DAT version in use.

**Examples**:
- `16Bit Rhythm Land (World) (Aftermarket) (Unl).md`
- `Action 52 (USA) (Unl).md`

**Rescued by title fallback**: Sometimes (if a different variant exists in the DB).

### 8. Non-Standard Region Codes (320 files, 2.5%)

Files using `(PT-BR)` instead of `(Brazil)`, or other non-No-Intro region codes.

**Examples**:
- `10 Super Jogos (PT-BR).md`
- `Action Fighter (PT-BR).sms`

**Rescued by title fallback**: Yes, if the title exists under a standard region.

### 9. Tilde-Separated Multi-Title Names (41 files, 0.3%)

No-Intro uses `~` to join regional title variants. The user's files use this format but
the DB entries don't always include the full multi-title form.

**Examples**:
- `Bare Knuckle II ~ Streets of Rage 2 ~ Streets of Rage II (World).gg`
  DB has: `Streets of Rage 2 (World) (Beta)`
- `GG Shinobi II, The ~ Shinobi II - The Silent Fury (World).gg`
  DB has: `Shinobi II - The Silent Fury (World)`

**Rescued by tilde splitting**: Yes (41 additional matches).

### 10. No-Intro Style but Missing from DAT (387 files, 3.0%)

Files that follow No-Intro naming conventions with standard region tags but simply aren't
in the DAT file used to build the DB.

**Examples**:
- `Alien vs. Predator (USA).smc` (SNES)
- `Advanced Busterhawk Gleylancer (Japan).md` (Mega Drive)
- `GT64 - Championship Edition (Europe) (En,Fr,De).z64` (N64)

These likely come from a different DAT version or were added to newer DAT releases.

**Rescued by title fallback**: No (the title itself doesn't exist in the DB).

### 11. GoodTools-Style Region Codes (33 files, 0.3%)

Files using abbreviated region codes from the GoodTools naming convention: `(U)`, `(E)`,
`(J)`, `(W)`, `(B)`, `(UE)`, `(JK)`, etc.

**Examples**:
- `Mickey's Playtown Adventure - A Day of Discovery! (U).smc`
- `Baby Boomer (Unl) (U).nes`

**Rescued by title fallback**: Sometimes (depends on whether the base title matches).

### 12. Bare Filenames / No Tags (36 files, 0.3%)

Files with no parenthesized tags at all, just a bare game title.

**Examples**:
- `Battletoads & Double Dragon.smc`
- `Choplifter III - Rescue & Survive.smc`
- `Doom Troopers.sfc`

**Rescued by title fallback**: Yes (stripping from `(` leaves the full filename, which
normalizes to match DB titles).

### 13. Non-Standard Extensions (18 files, 0.1%)

Files using `.bin` or `.gen` instead of the system-specific extension. The extension
stripping works correctly (it strips any extension), but the stem doesn't match.

**Examples**:
- `cadilac_dinosauro_v2.bin`
- `Cursed Knight, The (World).bin`

**Rescued by title fallback**: Sometimes.

## Proposed Improvements (Ranked by Impact)

### 1. Normalized Title Fallback (HIGH IMPACT: +4,466 matches, 47.5% -> 82.4%)

**What**: When exact stem lookup fails, normalize the filename to a title key (strip
everything from first `(`, lowercase, remove punctuation, collapse whitespace) and look
up in a secondary PHF map that maps normalized titles to canonical game entries.

**Why it works**: The vast majority of unmatched files share a base title with a DB entry
but differ in region tags, translation markers, revision numbers, or patch indicators.

**Implementation**:
- In `build.rs`, generate a second PHF map per system: `{prefix}_TITLE_DB` keyed by
  normalized title (same normalization as `normalize_title()` already used for grouping)
- Values are `&CanonicalGame` references (one per unique title)
- In `game_db.rs`, add `lookup_by_title(system, normalized_title) -> Option<&CanonicalGame>`
- In `game_display_name()`, try exact stem first, then normalized title fallback

**Estimated effort**: Small. The normalization logic and grouping already exist in
`build.rs`. The secondary map is a subset of existing data.

### 2. Tilde-Separated Title Handling (LOW IMPACT: +41 matches, but important for correctness)

**What**: When the normalized title doesn't match, split on `~` and try each part.

**Why**: No-Intro multi-title entries like `Bare Knuckle II ~ Streets of Rage 2` have
the alternative titles separated by tildes. The user's ROM files use this format, but
the DB may only index one variant.

**Implementation**: In the title fallback path, if the title contains `~`, try each
tilde-delimited segment as a separate normalized title lookup.

**Estimated effort**: Trivial addition to the fallback logic.

### 3. Activate CRC32 Fallback in GameRef::new (MEDIUM IMPACT for remaining 2,255 unmatched)

**What**: The `lookup_by_crc()` function already exists but is never called from the
display name resolution path. Computing CRC32 at ROM scan time and using it as a final
fallback would match any file whose binary content is in the No-Intro DAT, regardless
of filename.

**Why**: This would catch renamed files, files with completely non-standard names, and
files that happen to be exact binary matches of known dumps.

**Caveats**:
- Requires reading the entire file to compute CRC32, which is expensive for large ROMs
  (N64 ROMs are 8-64 MB each)
- Should only be used as a last resort after filename-based matching fails
- Could be done lazily (only when a ROM detail page is opened) or as a background task

**Implementation**:
- Add `crc32` field to `RomEntry`
- In `collect_roms_recursive()`, compute CRC32 for files that fail filename matching
- Call `lookup_by_crc()` as final fallback in `GameRef::new()`
- Consider async/background computation with caching to avoid repeated I/O

**Estimated effort**: Medium. The CRC index already exists; the work is in the I/O
and caching strategy.

### 4. GoodTools Region Code Normalization (LOW IMPACT: ~33 files)

**What**: Map GoodTools abbreviated region codes to No-Intro full names:
- `(U)` -> `(USA)`
- `(E)` -> `(Europe)`
- `(J)` -> `(Japan)`
- `(W)` -> `(World)`

Then retry the exact stem lookup with the expanded name.

**Why**: A small number of files use GoodTools naming. Most of these would already be
caught by the title fallback (improvement #1), so this is only needed if the title
itself differs.

**Implementation**: Simple string replacement before the title normalization step.

**Estimated effort**: Trivial.

### 5. Updated No-Intro DATs (MEDIUM IMPACT for the 387 "style-matches-but-missing" files)

**What**: Update the No-Intro DAT files to a newer version. Some files in the user's
collection appear to follow No-Intro conventions but aren't in the current DAT.

**Why**: No-Intro regularly adds new entries. The user's ROM collection may include files
from a newer DAT release than what's bundled with the app.

**Implementation**: Download and bundle newer DAT files, or provide a mechanism to
update them.

**Estimated effort**: Low (just replacing data files), but requires periodic maintenance.

## Implementation Recommendation

The single highest-impact change is **Improvement #1** (normalized title fallback).
It would increase the overall match rate from 47.5% to 82.4% with minimal code changes,
since the normalization logic already exists in `build.rs`.

The recommended implementation order:

1. **Normalized title fallback** -- immediate, covers ~4,500 files
2. **Tilde splitting** -- trivial add-on to #1, covers ~40 files
3. **CRC32 fallback** -- medium effort, covers unknown number of remaining files
4. **GoodTools region expansion** -- trivial, minor coverage improvement
5. **DAT file updates** -- ongoing maintenance

### Concrete Changes for #1 and #2

In `build.rs`:
- After building `{prefix}_ROM_DB`, build `{prefix}_TITLE_DB: phf::Map<&str, usize>`
  mapping normalized titles to canonical game indices
- Add dispatch function `get_system_title_db()`

In `game_db.rs`:
```rust
pub fn lookup_by_title(system: &str, title: &str) -> Option<&'static CanonicalGame> {
    let title_db = get_system_title_db(system)?;
    let game_idx = title_db.get(title)?;
    get_system_games(system).get(*game_idx)
}

pub fn game_display_name(system: &str, filename: &str) -> Option<&'static str> {
    let stem = filename.rfind('.').map(|i| &filename[..i]).unwrap_or(filename);

    // 1. Exact stem match
    if let Some(entry) = lookup_game(system, stem) {
        return Some(entry.game.display_name);
    }

    // 2. Normalized title fallback
    let normalized = normalize_stem(stem);
    if let Some(game) = lookup_by_title(system, &normalized) {
        return Some(game.display_name);
    }

    // 3. Tilde-split fallback
    if normalized.contains('~') {
        for part in normalized.split('~').map(str::trim) {
            let part_norm = normalize_str(part);
            if let Some(game) = lookup_by_title(system, &part_norm) {
                return Some(game.display_name);
            }
        }
    }

    None
}
```

The `normalize_stem()` function should mirror the existing `normalize_title()` from
`build.rs`: strip everything from the first `(`, lowercase, remove non-alphanumeric
characters (except spaces), collapse whitespace.
