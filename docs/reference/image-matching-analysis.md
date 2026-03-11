# Image Matching Analysis

Investigation of missing box art in the RePlayOS companion app.

Last updated: 2026-03-11

## How Image Matching Works

There are two paths where ROM filenames are matched to image files:

### 1. Import path (`thumbnails.rs` -- `import_system_thumbnails`)

Runs when the user imports thumbnails from a cloned libretro-thumbnails repo. Uses a 3-tier match:

1. **Exact**: `thumbnail_filename(rom_stem)` matches a `.png` file in the repo
2. **Strip-tags fuzzy**: Strip parenthesized tags `(...)` and bracketed tags `[...]` from both sides, case-insensitive compare
3. **Version-stripped**: Additionally strip TOSEC version strings like ` v1.008`

Also handles **colon variants** (`:` in display names mapped to `_`, ` -`, or dropped).

For arcade systems, ROM codenames (e.g., `sf2`) are translated to display names via `arcade_db` before matching.

Matched images are copied via `copy_png()`, which calls `resolve_fake_symlink()` to handle git fake-symlink artifacts (small text files containing a relative path to the real PNG).

Post-clone, `resolve_fake_symlinks_in_dir()` walks the cloned repo and replaces fake symlinks with copies of their targets (where the target exists). This ensures that subsequent `copy_png()` calls get real PNGs.

### 2. Runtime path (`server_fns.rs` -- `find_image_on_disk`)

Runs on every page load to find the boxart file for a ROM. Uses a 2-tier match:

1. **Exact**: `thumbnail_filename(rom_stem) + ".png"` exists and is >= 200 bytes
2. **Fuzzy**: Strip tags from both ROM stem and all boxart filenames (via `base_title()`), case-insensitive compare. Also handles tilde dual-names (`Name1 ~ Name2` uses Name2).

Files < 200 bytes are rejected by `is_valid_image()` to filter out git fake-symlink artifacts. When `is_valid_image()` fails, `try_resolve_fake_symlink()` reads the file content and checks if the referenced target exists and is valid.

**Notable gaps vs the import path**: The runtime path does NOT use `strip_version()` and does NOT handle colon variants.

## Current Coverage (2026-03-11)

### NFS mount (development)

The NFS mount at `<NFS_MOUNT>/` has media directories for 4 systems. The Pi (production) has thumbnails for 19 systems but was unreachable during this analysis.

| System | ROMs | Boxart Files | Valid | Fake Symlinks | Exact | Exact+Symlink | Fuzzy | Matched | Missing | Coverage |
|--------|------|-------------|-------|---------------|-------|---------------|-------|---------|---------|----------|
| ibm_pc | 8 | 8 | 8 | 0 | 6 | 0 | 2 | 8 | 0 | 100% |
| nintendo_n64 | 639 | 621 | 569 | 52 | 568 | 3 | 31 | 602 | 37 | 94.2% |
| sega_32x | 60 | 50 | 50 | 0 | 50 | 0 | 0 | 50 | 10 | 83.3% |
| sega_cd | 27 | 25 | 25 | 0 | 24 | 0 | 1 | 25 | 2 | 92.6% |

**Note**: ROM count for N64 increased from 637 to 639 since the initial analysis (2 ROMs added). The 3 "Exact+Symlink" matches are ECW Hardcore Revolution (Germany) and two F-Zero X iQue variants -- these are fake symlinks whose targets exist in the media directory, so `try_resolve_fake_symlink()` resolves them at runtime.

### Systems without thumbnails on NFS

These 12 systems have ROMs but no imported thumbnails on the NFS mount. All have valid `thumbnail_repo_names` mappings and could have thumbnails imported. Some may already have thumbnails on the Pi (which was unreachable).

| System | ROMs |
|--------|------|
| arcade_dc | 204 |
| arcade_fbneo | 4,082 |
| arcade_mame | 4,605 |
| commodore_ami | 1 |
| nintendo_snes | 4,464 |
| sega_dc | 119 |
| sega_gg | 729 |
| sega_sg | 196 |
| sega_smd | 3,096 |
| sega_sms | 1,025 |
| sega_st | 58 |
| sharp_x68k | 3,163 |
| **Total** | **21,742** |

## Root Cause Breakdown

### N64: 37 missing games

#### Category 1: Fake symlink artifacts (23 games, 62%) -- RECOVERABLE via re-import

These games have boxart files in the media directory that are git fake-symlink text files (< 200 bytes), and ALL files with the same base_title are also fake symlinks. The targets they point to don't exist in the media directory.

**Verified against the libretro-thumbnails repo**: ALL 23 games have valid base images (the non-Rev version) in the repo, ranging from 128KB to 893KB. The `resolve_fake_symlinks_in_dir()` post-clone fix would resolve the fake symlinks in the cloned repo, and the import would successfully copy them. Alternatively, the fuzzy fallback in `copy_png()` (Fix 1) would find the non-Rev version.

Notably, 29 additional boxart files are ALSO fake symlinks, but happen to have a valid alternative with a different region tag in the media directory that the fuzzy match catches at runtime (e.g., Banjo-Kazooie (USA) (Rev 1) -> Banjo-Kazooie (Europe)).

**Summary of the 52 fake symlinks**:
- 29 have valid alternatives in media dir (runtime fuzzy match works)
- 23 have NO valid alternatives (all files with same base_title are fake)
- 3 of the 52 have resolvable targets (ECW, F-Zero X iQue) -- handled by `try_resolve_fake_symlink()`

#### Category 2: Homebrew / Translation hacks (5 games, 14%)

ROMs containing `(PD)` or `T+Eng`/`T-Spa` tags where no matching boxart exists. Note that some translation ROMs (e.g., Custom Robo (J) T+Eng) DO match via fuzzy because the `(J)` tag is stripped and the base title matches the Japanese original's art. The 5 below have no such match.

- 77a Special Edition (PD)
- Asteroid Shooter (PD)
- Yeti3D Pro (PD)
- Bomberman 64 - Arcade Edition (J) T+Eng -- base title "bomberman 64 - arcade edition" has no match (the official game is just "Bomberman 64")
- Sin and Punishment Spanish Dubbed (J) T-Spa -- base title doesn't match "Sin and Punishment" (different formatting)

#### Category 3: N64DD games (8 games, 22%) -- PARTIALLY RECOVERABLE

ROMs prefixed with `N64DD - `. The prefix prevents matching. Verified against repos:

| ROM | In N64 repo? | In N64DD repo? |
|-----|-------------|----------------|
| N64DD - Dezaemon 3D | Yes (898KB) | No (has "Dezaemon DD" instead) |
| N64DD - Doshin The Giant | No | Yes (120KB, via "Doshin the Giant (Japan)") |
| N64DD - Doshin the Giant - Tinkling Toddler... | No | Yes (partial, matches "Doshin the Giant") |
| N64DD - F-Zero X - Expansion Kit (J) | No | Yes (486KB) |
| N64DD - Mario Artist Communication Kit | No | Yes (337KB) |
| N64DD - Mario Artist Polygon Studio | No | Yes (381KB) |
| N64DD - Mario Artist Talent Studio | No | Yes (337KB) |
| N64DD - Sim City 64 | No | Yes (565KB) |

To recover these: (a) strip the `N64DD - ` prefix in the matching logic, and (b) add `"Nintendo - Nintendo 64DD"` as a fallback repo for `nintendo_n64` in `thumbnail_repo_names`. With both, all 8 would get art (1 from N64 repo, 7 from N64DD repo).

#### Category 4: Minor naming differences (1 game, 3%)

`GT64 - Championship Edition (Europe) (En,Fr,De).z64` -- the repo has `GT 64 - Championship Edition` (with a space between GT and 64). Base titles `"gt64 - championship edition"` vs `"gt 64 - championship edition"` don't match.

### Sega 32X: 10 missing games

All homebrew (PD). No art expected. No fix needed.

### Sega CD: 2 missing games

| ROM | Reason | In repo? |
|-----|--------|----------|
| Sonic MegaMix (USA) (Unl).chd | Unlicensed homebrew | No |
| Sing!!\_Sega\_Game\_Music\_...\_Japan.chd | Naming convention mismatch | Yes (511KB) |

The Sing!! ROM uses underscore-separated CHD naming (`Sing!!_Sega_Game_Music_Presented_by_B_B_Queens_Japan`) while the repo uses No-Intro naming with spaces and dots (`Sing!! Sega Game Music Presented by B. B. Queens (Japan)`). The base_title extraction produces completely different strings. This is the only ROM on the NFS mount using this naming convention.

## All Games Without Box Art (N64)

### Blocked by fake symlinks -- RECOVERABLE via re-import

All 23 games have valid base images in the libretro-thumbnails repo. A clean re-import (delete existing N64 media + re-download from metadata page) would recover all of them.

| ROM | Fake Symlink Target | Repo Source (fuzzy fallback) |
|-----|--------------------|-----------------------------|
| Hoshi no Kirby 64 (Japan) (Rev 3) | ...64 (Japan).png | Hoshi no Kirby 64 (Japan).png (893KB) |
| Jikkyou J.League 1999 - Perfect Striker 2 (Japan) (Rev 1) | ...(Japan).png | same title (Japan).png (744KB) |
| Jikkyou Powerful Pro Yakyuu 2000 (Japan) (Rev 1) | ...(Japan).png | same title (Japan).png (518KB) |
| Jikkyou Powerful Pro Yakyuu 4 (Japan) (Rev 1) | ...(Japan).png | same title (Japan).png (536KB) |
| Jikkyou Powerful Pro Yakyuu 5 (Japan) (Rev 2) | ...(Japan).png | same title (Japan).png (534KB) |
| Jikkyou Powerful Pro Yakyuu 6 (Japan) (Rev 2) | ...(Japan).png | same title (Japan).png (545KB) |
| Jikkyou Powerful Pro Yakyuu - Basic Ban 2001 (Japan) (Rev 1) | ...(Japan).png | same title (Japan).png (370KB) |
| Jikkyou World Soccer - World Cup France '98 (Japan) (Rev 1) | ...(Japan).png | same title (Japan).png (488KB) |
| Legend of Zelda, The - Ocarina of Time (Europe) (En,Fr,De) (Rev 1) | ...(En,Fr,De).png | same title (En,Fr,De).png (128KB) |
| Legend of Zelda, The - Ocarina of Time (USA) (Rev 2) | ...(USA).png | same title (USA).png (167KB) |
| NFL Blitz 2000 (USA) (Rev 1) | ...(USA).png | same title (USA).png (480KB) |
| Nushi Zuri 64 (Japan) (Rev 1) | ...(Japan).png | same title (Japan).png (799KB) |
| Ogre Battle 64 - Person of Lordly Caliber (USA) (Rev 1) | ...(USA).png | same title (Japan)(Rev 1)(Wii VC).png (597KB) |
| Star Wars - Rogue Squadron (Europe) (En,Fr,De) (Rev 1) | ...(En,Fr,De).png | same title (En,Fr,De).png (344KB) |
| Star Wars - Rogue Squadron (USA) (Rev 1) | ...(USA).png | same title (USA).png (414KB) |
| Tony Hawk's Pro Skater (USA) (Rev 1) | ...(USA).png | same title (USA).png (337KB) |
| Toy Story 2 - Captain Buzz Lightyear auf Rettungsmission! (Germany) (Rev 1) | ...(Germany).png | same title (Germany).png (337KB) |
| Turok - Dinosaur Hunter (Europe) (Rev 2) | ...(Europe).png | same title (Europe).png (333KB) |
| Turok - Dinosaur Hunter (Germany) (Rev 2) | ...(Germany).png | same title (Germany).png (333KB) |
| Turok - Dinosaur Hunter (USA) (Rev 2) | ...(USA).png | same title (USA).png (333KB) |
| WinBack (Japan) (Rev 1) | ...(Japan).png | same title (Japan).png (391KB) |
| WWF No Mercy (Europe) (Rev 1) | ...(Europe).png | same title (Europe).png (424KB) |
| WWF No Mercy (USA) (Rev 1) | ...(USA).png | same title (USA).png (340KB) |

### Homebrew / Translation hacks (no art expected)

- 77a Special Edition (PD)
- Asteroid Shooter (PD)
- Bomberman 64 - Arcade Edition (J) T+Eng
- Sin and Punishment Spanish Dubbed (J) T-Spa
- Yeti3D Pro (PD)

### N64DD games (no matching due to filename prefix)

All have boxart available in either the N64 or N64DD libretro-thumbnails repos.

- N64DD - Dezaemon 3D -- in N64 repo as "Dezaemon 3D (Japan)" (898KB)
- N64DD - Doshin The Giant -- in N64DD repo as "Doshin the Giant (Japan)" (120KB)
- N64DD - Doshin the Giant - Tinkling Toddler Liberation Front! Assemble! -- in N64DD repo (partial match)
- N64DD - F-Zero X - Expansion Kit -- in N64DD repo (486KB)
- N64DD - Mario Artist Communication Kit -- in N64DD repo (337KB)
- N64DD - Mario Artist Polygon Studio -- in N64DD repo (381KB)
- N64DD - Mario Artist Talent Studio -- in N64DD repo (337KB)
- N64DD - Sim City 64 -- in N64DD repo (565KB)

### Minor naming differences

- GT64 - Championship Edition (Europe) (En,Fr,De) -- repo has "GT 64" (with space, 370KB)

### Other systems (32X, Sega CD)

- **32X**: All 10 missing games are homebrew (PD) -- no art expected.
- **Sega CD**: 1 unlicensed (Sonic MegaMix), 1 naming convention mismatch (Sing!!).

## Proposed Improvements

### Fix 1: Resolve fake symlinks in `copy_png` (import path) -- DONE

**Status**: DONE. `resolve_fake_symlink()` returns an error when the target doesn't exist. `copy_png()` failure triggers a fuzzy-only fallback via `find_thumbnail_fuzzy()`.

### Fix 1b: Resolve fake symlinks post-clone -- DONE

**Status**: DONE. `resolve_fake_symlinks_in_dir()` runs after `git clone`, replacing fake symlinks in the cloned repo with copies of their targets. This means `copy_png()` gets real PNGs for exact matches.

### Fix 2: Resolve fake symlinks at runtime -- DONE

**Status**: DONE. `try_resolve_fake_symlink()` resolves fake symlinks in the media directory when `is_valid_image()` fails. Currently resolves 3 games at runtime (ECW Hardcore Revolution Germany, 2 F-Zero X iQue variants).

### Re-import N64 thumbnails -- ACTION NEEDED

**Status**: Pending user action. Delete existing N64 media + re-download from the metadata page. This will:
- Resolve all 52 fake symlinks (post-clone fix replaces them in the repo)
- Recover all 23 currently-blocked games
- Expected coverage: 625/639 (97.8%), up from 602/639 (94.2%)

### Fix 3: Add `strip_version()` to runtime `find_image_on_disk` -- TODO

**Problem**: The runtime fuzzy matching does not strip TOSEC version strings (e.g., `v1.009`). This is done during import but not at runtime.

**Solution**: Add a third matching tier in `find_image_on_disk` that applies `strip_version()` to the base_title. This would fix Dreamcast TOSEC-named ROMs like "Virtua Tennis 2 v1.009" that fail to match at runtime.

**Impact**: High for Sega DC (all 119 ROMs use TOSEC naming). Also affects any future system with TOSEC ROMs.

### Fix 5: N64DD prefix stripping + fallback repo -- LOW IMPACT, TODO

**Problem**: 8 N64DD games have a `N64DD - ` prefix that prevents matching. Only 1 of 8 exists in the N64 repo; the other 7 are in the separate `Nintendo - Nintendo 64DD` repo.

**Solution** (two parts):
1. Strip `N64DD - ` prefix before matching in `import_system_thumbnails`
2. Add `"Nintendo - Nintendo 64DD"` as a fallback repo in `thumbnail_repo_names` for `nintendo_n64`

**Impact**: 8 games. Low priority.

### Fix 7: Underscore-to-space normalization for CHD naming -- VERY LOW IMPACT

**Problem**: 1 Sega CD ROM (`Sing!!_Sega_Game_Music_...`) uses underscore-separated naming instead of spaces. The base_title contains underscores while the repo uses spaces.

**Solution**: Normalize underscores to spaces in `base_title()` for the fuzzy match. Very low ROI for 1 game.

## Impact Summary

| Fix | Games Recovered | Effort | Priority | Status |
|-----|----------------|--------|----------|--------|
| Fix 1: Error on broken fake symlinks + fuzzy fallback | 23 N64 (on re-import) | Low | High | DONE |
| Fix 1b: Post-clone fake symlink resolution | Enables Fix 1 | Low | High | DONE |
| Fix 2: Resolve fake symlinks at runtime | 3 N64 (immediate) | Low | High | DONE |
| **Re-import N64** | **23 N64** | **User action** | **High** | **PENDING** |
| Fix 3: strip_version in runtime path | DC (119 ROMs), others | Low | Medium | TODO |
| Fix 5: N64DD prefix + fallback repo | 8 N64DD | Low | Low | TODO |
| Fix 7: Underscore normalization | 1 Sega CD | Trivial | Very Low | TODO |

### Expected coverage after re-import (N64 only)

| Category | Count | Coverage |
|----------|-------|----------|
| Matched (current) | 602 | 94.2% |
| + Recovered via re-import (fake symlinks) | +23 | |
| **Matched after re-import** | **625** | **97.8%** |
| Remaining: Homebrew/translations | 5 | |
| Remaining: N64DD prefix | 8 | |
| Remaining: Naming mismatch (GT64) | 1 | |
| **Total ROMs** | **639** | |

With Fix 5 (N64DD), coverage would reach 633/639 (99.1%). The remaining 6 would be 5 homebrew/translations (no art exists) + 1 naming mismatch (GT64).

## Previously Reported Issues -- Status

### Zelda: Ocarina of Time -- WILL BE RESOLVED by re-import

The 48-byte fake symlink in media dir has no valid target. But the repo has the real `Legend of Zelda, The - Ocarina of Time (USA).png` (167KB). Post-clone resolution would replace the fake symlink in the cloned repo, allowing `copy_png()` to copy the real image. If the exact match still fails, the fuzzy fallback would find the `(Europe) (En,Fr,De).png` variant (128KB).

### Chelnov (Arcade) -- NEEDS INVESTIGATION (Pi unreachable)

The Pi has 2947 boxart files for arcade_fbneo. Whether Chelnov specifically has art requires checking the Pi, which was unreachable during this analysis.

### Virtua Tennis 2 (Dreamcast) -- NEEDS Fix 3

The Pi has 24 boxart files for sega_dc. Even if the import matched successfully (using `strip_version`), the runtime `find_image_on_disk` does NOT use `strip_version()`, so the TOSEC version string `v1.009` in the ROM name would prevent runtime matching. Fix 3 is needed.
