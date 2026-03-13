# Genre Priority Fix: Prevent Beta ROMs from Overriding Primary ROM Genres

## Problem

In `replay-control-core/build.rs`, the genre assignment loop iterates through all
ROM variants in a canonical game group (including betas, prototypes, samples,
and demos) and takes the **first CRC match** against the libretro genre data.
Since indices follow No-Intro DAT file ordering (not sorted by priority), a beta
ROM's CRC can match before the primary ROM's CRC, causing the wrong genre to be
assigned.

More critically: in many cases the **primary ROM's CRC has no match at all** in
the libretro genre data (CRC mismatch between No-Intro and libretro datasets),
but beta ROMs' CRCs do match. This means genre data that should come from TGDB
fallback instead comes from a beta's libretro CRC match.

### Example: Sonic & Knuckles (Mega Drive)

The primary releases `Sonic & Knuckles (World)` (CRC `4DCFD55C`) and
`Sonic & Knuckles (USA) (Sega Channel)` (CRC `87F40B6A`) have **no CRC match**
in the libretro genre DAT. But all 7 beta ROMs do match, with genre `Shoot'em Up`.
The libretro genre DAT itself lists the primary game under CRC `0658F691` (a
different dump), which doesn't exist in the No-Intro DAT. So the beta's
`Shoot'em Up` genre gets used. Even the TGDB fallback wouldn't help here -- TGDB
classifies Sonic & Knuckles as "Puzzle", not "Platform". This particular example
is a data quality issue in both sources, but it illustrates how beta CRCs can
introduce unexpected genres.

## Current Behavior

**Code location:** `replay-control-core/build.rs`, lines 1533-1561.

```rust
// Lines 1538-1553: iterate ALL indices (no priority ordering)
for &idx in indices {
    let crc = nointro_entries[idx].crc32;
    if final_genre.is_empty()
        && let Some(genre_str) = genres.get(&crc)
    {
        final_genre = genre_str.clone();
    }
    if final_players > 0 && !final_genre.is_empty() {
        break;
    }
}

// Lines 1555-1561: TGDB is fallback only
if final_genre.is_empty() {
    final_genre = tgdb_genre;
}
```

**How groups form:** `normalize_title()` (line 1208) strips everything from the
first `(` onward and lowercases, so all region variants, revisions, betas, and
protos of the same game end up in one group.

**Index ordering:** indices are in DAT file insertion order -- no sorting within
a group. Betas may appear before or after primary ROMs depending on the DAT.

## Impact Analysis

Analysis script: `tools/analyze_genre_priority.py`
Full output: `tools/genre_priority_analysis_output.txt`

### Summary Across All 9 Systems

| Metric | Count |
|--------|-------|
| Total canonical game groups | 15,767 |
| Groups containing beta/proto/sample/demo ROMs | 2,630 (17%) |
| Groups where genre was set from a beta ROM's CRC | 408 (2.6%) |
| ...of which beta genre differs from primary ROM's genre (within libretro) | 1 |
| ...of which only betas have a CRC match (primary has none) | 299 |
| ...of which beta genre same as primary (both match, same result) | 108 |
| Groups where beta-sourced libretro genre != TGDB genre | 207 |
| User's games on disk affected (genre from beta CRC) | 64 |
| ...of which genre would actually change with fix | 0 |
| ...of which beta-only CRC match (no primary match exists) | 32 |
| ...of which beta and primary have identical genre | 32 |

### Breakdown by System

| System | Groups | Beta groups | Genre from beta | User affected |
|--------|--------|-------------|-----------------|---------------|
| NES | 4,040 | 716 | 73 | 0 |
| SNES | 2,407 | 507 | 83 | 14 |
| Game Boy | 1,640 | 191 | 38 | 0 |
| Game Boy Color | 1,785 | 212 | 38 | 0 |
| Game Boy Advance | 1,406 | 208 | 35 | 0 |
| N64 | 607 | 95 | 16 | 3 |
| Master System | 1,117 | 168 | 16 | 1 |
| Mega Drive | 2,003 | 441 | 92 | 45 |
| Game Gear | 762 | 92 | 17 | 1 |

### Key Finding

The overwhelming issue is **not** betas having a different genre than the
primary ROM in libretro data (only 1 case: Tintin in Tibet on Game Boy). The
real problem is that **299 games have genre data only from beta CRCs** because
the primary ROM's CRC doesn't exist in the libretro genre DAT at all. In these
cases, the libretro data from the beta overrides what would otherwise be a TGDB
fallback lookup.

Of the 207 games where the beta-sourced genre conflicts with TGDB, many are
TGDB data quality issues (e.g., TGDB classifying "Donkey Kong Land 2" as
"Puzzle" when it's clearly "Platform"). But some are genuine misclassifications
caused by the beta CRC providing wrong genre data.

## Proposed Fix

### Approach: Prioritize Non-Beta ROMs in CRC Lookup

Minimal change: split the genre/players CRC lookup loop into two passes.

```rust
// PROPOSED: Two-pass approach - primary ROMs first, then betas as fallback
let mut final_players: u8 = 0;
let mut final_genre = String::new();

// Pass 1: Try primary (non-beta) ROMs first
for &idx in indices {
    let name = &nointro_entries[idx].name;
    if is_beta_or_proto(name) {
        continue;
    }
    let crc = nointro_entries[idx].crc32;
    if final_players == 0 {
        if let Some(users_str) = maxusers.get(&crc) {
            final_players = users_str.parse().unwrap_or(0);
        }
    }
    if final_genre.is_empty() {
        if let Some(genre_str) = genres.get(&crc) {
            final_genre = genre_str.clone();
        }
    }
    if final_players > 0 && !final_genre.is_empty() {
        break;
    }
}

// Pass 2: Fall back to beta/proto ROMs if primary didn't match
if final_players == 0 || final_genre.is_empty() {
    for &idx in indices {
        let name = &nointro_entries[idx].name;
        if !is_beta_or_proto(name) {
            continue;
        }
        let crc = nointro_entries[idx].crc32;
        if final_players == 0 {
            if let Some(users_str) = maxusers.get(&crc) {
                final_players = users_str.parse().unwrap_or(0);
            }
        }
        if final_genre.is_empty() {
            if let Some(genre_str) = genres.get(&crc) {
                final_genre = genre_str.clone();
            }
        }
        if final_players > 0 && !final_genre.is_empty() {
            break;
        }
    }
}

// Pass 3: TGDB fallback (unchanged)
if final_players == 0 {
    final_players = tgdb_players;
}
if final_genre.is_empty() {
    final_genre = tgdb_genre;
}
```

### Helper Function

```rust
/// Check if a No-Intro ROM name indicates a beta, prototype, sample, or demo.
fn is_beta_or_proto(name: &str) -> bool {
    // Match tags like (Beta), (Beta 1), (Proto), (Sample), (Demo)
    name.contains("(Beta")
        || name.contains("(Proto")
        || name.contains("(Sample")
        || name.contains("(Demo")
}
```

### Priority Order (After Fix)

1. **Libretro genre from primary ROM CRC** -- most reliable
2. **Libretro genre from beta/proto ROM CRC** -- same data source, less reliable CRC
3. **TGDB genre** -- title-based matching, different taxonomy

## Edge Cases

### Only betas have genre data (299 games)

These games have no primary ROM CRC match in libretro. With the fix:
- Pass 1 finds nothing (no primary CRC match)
- Pass 2 finds the beta's genre (same as current behavior)
- Net effect: **no change** for these games

This is the correct behavior -- beta genre data from libretro is still better
than no data, and the TGDB fallback only kicks in if neither pass finds anything.

### Only 1 true mismatch (Tintin in Tibet)

The beta has genre "Platform" while the primary has "Action". After the fix, the
primary's "Action" will be used. Both are reasonable; the fix produces the more
correct result by trusting the released ROM's classification.

### Games where beta appears first in DAT but has same genre (108 games)

No behavioral change -- the result is the same regardless of which ROM's CRC
provides the genre.

### Practical Impact

For the current NFS romset, **0 games would have their genre actually change**
with this fix. Of the 64 user-affected games:
- 32 have no primary CRC match in libretro (beta-only), so the fix doesn't
  alter behavior -- beta genre is still used as pass-2 fallback
- 32 have both beta and primary CRC matches with identical genres

The fix is a **correctness improvement**, not an immediate behavior change. It
prevents latent bugs when DAT files are updated (new CRC entries could introduce
mismatches), and it makes the code's intent explicit: primary ROMs are the
authoritative source for metadata.

The Sonic & Knuckles example that motivated this investigation turns out to be a
data quality issue in both libretro (wrong CRC mapping) and TGDB (genre listed
as "Puzzle"), not solvable by reordering CRC lookups alone.

## Implementation Steps

1. Add `is_beta_or_proto()` helper function (near the other helper functions
   around line 780 in build.rs)
2. Replace the single-pass genre/players loop (lines 1538-1553) with the
   two-pass approach shown above
3. No changes needed to `normalize_title()`, `normalize_console_genre()`, TGDB
   lookup, or any other code
4. Re-run `tools/analyze_genre_priority.py` after the fix to verify:
   - `GENRE_FROM_BETA` should drop from 408 to 299 (only beta-only cases)
   - `GENRE_MISMATCH_BETA_VS_PRIMARY` should be 0
   - `USER_AFFECTED` should decrease

## Files to Modify

- `replay-control-core/build.rs` -- the only file that needs changes

## Testing

- Run `cargo build` in `replay-control-core` to verify the build script compiles
- Compare the generated game DB output before and after (the codegen writes to
  `$OUT_DIR/game_db_generated.rs`)
- Re-run the analysis script to verify the fix
