# Image Matching Analysis

Investigation of image (boxart/snap) matching across all systems on the Pi.

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

### 2. Runtime path (`server_fns.rs` -- `find_image_on_disk`)

Runs on every page load to find the boxart file for a ROM. Uses a 2-tier match:

1. **Exact**: `thumbnail_filename(rom_stem) + ".png"` exists and is >= 200 bytes
2. **Fuzzy**: Strip tags from both ROM stem and all boxart filenames (via `base_title()`), case-insensitive compare

**Notable gaps vs the import path**: The runtime path does NOT use `strip_version()` and does NOT handle colon variants.

## Full Coverage Data (Pi, 2026-03-11)

Data source: Pi at <PI_IP>, USB storage at /media/usb/. All 21 thumbnail repos cloned. Disk is 100% full (233G used).

### Coverage Table

"FS ROMs" = filesystem count (from `/api/systems`). "DB Rows" = entries in metadata.db. "Boxart" and "Snap" = DB entries with a non-null image path. "Box%" and "Snap%" are relative to FS ROM count.

| System | Display Name | FS ROMs | DB Rows | DB/FS | Boxart | Box% | Snaps | Snap% |
|--------|-------------|---------|---------|-------|--------|------|-------|-------|
| nintendo_snes | Super Nintendo | 7,275 | 5,785 | 80% | 5,545 | 76% | 5,580 | 77% |
| arcade_mame | Arcade (MAME) | 4,605 | 3,908 | 85% | 1,660 | 36% | 0 | 0% |
| amstrad_cpc | Amstrad CPC | 4,168 | 3,636 | 87% | 2,910 | 70% | 3,491 | 84% |
| arcade_fbneo | Arcade (FBNeo) | 4,082 | 4,061 | 99% | 3,241 | 79% | 4,050 | 99% |
| sharp_x68k | Sharp X68000 | 3,163 | 3,010 | 95% | 1,544 | 49% | 2,884 | 91% |
| sega_smd | Mega Drive / Genesis | 3,100 | 2,765 | 89% | 2,286 | 74% | 2,289 | 74% |
| sega_sms | Sega Master System | 1,035 | 904 | 87% | 811 | 78% | 877 | 85% |
| sega_gg | Sega Game Gear | 729 | 607 | 83% | 595 | 82% | 593 | 81% |
| nintendo_n64 | Nintendo 64 | 639 | 624 | 98% | 623 | 97% | 615 | 96% |
| arcade_dc | Arcade (Atomiswave/Naomi) | 211 | 150 | 71% | 124 | 59% | 148 | 70% |
| sega_sg | Sega SG-1000 | 196 | 180 | 92% | 151 | 77% | 180 | 92% |
| sega_32x | Sega 32X | 60 | 50 | 83% | 50 | 83% | 50 | 83% |
| sega_st | Sega Saturn | 55 | 51 | 93% | 48 | 87% | 48 | 87% |
| sega_dc | Sega Dreamcast | 33 | 27 | 82% | 24 | 73% | 26 | 79% |
| sega_cd | Sega CD / Mega-CD | 32 | 30 | 94% | 30 | 94% | 30 | 94% |
| sony_psx | PlayStation | 24 | 22 | 92% | 22 | 92% | 22 | 92% |
| nintendo_nes | NES | 15 | 15 | 100% | 15 | 100% | 15 | 100% |
| ibm_pc | IBM PC (DOS) | 8 | 8 | 100% | 8 | 100% | 7 | 88% |
| commodore_ami | Commodore Amiga | 1 | 0 | 0% | 0 | 0% | 0 | 0% |
| **TOTAL** | | **29,431** | **25,833** | **88%** | **19,687** | **67%** | **20,905** | **71%** |

### Physical Media Files on Disk

Distinct boxart/snap PNG files in `.replay-control/media/`. Multiple ROMs can share the same image file (e.g., translations, revisions all map to one region variant).

| System | Unique Boxart Files | Unique Snap Files | DB Boxart Refs | DB Snap Refs |
|--------|-------------------|-------------------|---------------|-------------|
| nintendo_snes | 3,376 | 3,368 | 5,545 | 5,580 |
| arcade_fbneo | 2,947 | 3,966 | 3,241 | 4,050 |
| amstrad_cpc | 1,870 | 2,432 | 2,910 | 3,491 |
| sega_smd | 1,621 | 1,570 | 2,286 | 2,289 |
| arcade_mame | 682 | 0 | 1,660 | 0 |
| sharp_x68k | 507 | 1,201 | 1,544 | 2,884 |
| nintendo_n64 | 621 | 610 | 623 | 615 |
| sega_gg | 487 | 460 | 595 | 593 |
| sega_sms | 447 | 531 | 811 | 877 |
| arcade_dc | 122 | 144 | 124 | 148 |
| sega_sg | 74 | 168 | 151 | 180 |
| sega_32x | 50 | 50 | 50 | 50 |
| sega_st | 45 | 45 | 48 | 48 |
| sega_cd | 30 | 30 | 30 | 30 |
| sega_dc | 24 | 26 | 24 | 26 |
| sony_psx | 22 | 22 | 22 | 22 |
| nintendo_nes | 15 | 15 | 15 | 15 |
| ibm_pc | 8 | 7 | 8 | 7 |

### Metadata Source Breakdown

DB entries come from two sources: LaunchBox XML import ("launchbox") and thumbnail import scan ("thumbnails" -- created for ROMs that matched a thumbnail but had no LaunchBox entry).

| System | Total DB | LaunchBox | Thumbnails |
|--------|----------|-----------|------------|
| nintendo_snes | 5,785 | 4,689 | 1,096 |
| arcade_fbneo | 4,061 | 3,639 | 422 |
| arcade_mame | 3,908 | 3,123 | 785 |
| amstrad_cpc | 3,636 | 2,491 | 1,145 |
| sharp_x68k | 3,010 | 1,632 | 1,378 |
| sega_smd | 2,765 | 2,341 | 424 |
| sega_sms | 904 | 767 | 137 |
| nintendo_n64 | 624 | 504 | 120 |
| sega_gg | 607 | 514 | 93 |
| sega_sg | 180 | 169 | 11 |
| arcade_dc | 150 | 122 | 28 |
| sega_st | 51 | 48 | 3 |
| sega_32x | 50 | 45 | 5 |
| sega_cd | 30 | 26 | 4 |
| sega_dc | 27 | 1 | 26 |
| sony_psx | 22 | 18 | 4 |
| nintendo_nes | 15 | 15 | 0 |
| ibm_pc | 8 | 8 | 0 |
| **TOTAL** | **25,833** | **20,152** | **5,681** |

3,598 ROMs exist on the filesystem but have no DB entry at all (neither LaunchBox nor thumbnails matched them).

### Cloned Repos on Pi

21 repos cloned at `/media/usb/.replay-control/tmp/libretro-thumbnails/`:

Amstrad - CPC, Atomiswave, Commodore - Amiga, DOS, FBNeo - Arcade Games, MAME, Nintendo - Nintendo 64, Nintendo - Nintendo Entertainment System, Nintendo - Super Nintendo Entertainment System, Sega - 32X, Sega - Dreamcast, Sega - Game Gear, Sega - Master System - Mark III, Sega - Mega-CD - Sega CD, Sega - Mega Drive - Genesis, Sega - Naomi, Sega - Naomi 2, Sega - Saturn, Sega - SG-1000, Sharp - X68000, Sony - PlayStation

## Systems with Poor Coverage and Root Causes

### arcade_mame: 36% boxart, 0% snaps -- WORST PERFORMER

**Snap coverage**: The Pi's MAME clone only has `Named_Boxarts` -- it is missing `Named_Snaps`, `Named_Logos`, and `Named_Titles`. The NFS clone (which has more disk space) has all four directories and achieves 86% snap coverage. **Root cause: Pi disk was full during clone** (233G/233G, 0.15% free). The MAME repo is 6GB total; the Pi only got 5.1GB.

**Boxart coverage (36%)**: Two factors:

1. **No arcade_db entry (1,111 of 2,248 unmatched)**: These ROMs have MAME codenames (e.g., `280zzzap.zip`) but no entry in the compiled `arcade_db`. Without a display name translation, the codename is used as-is, which never matches the repo's display-name filenames (e.g., `280 Zzzap (280 Zzzap).png`). Many are obscure/niche titles.

2. **Gambling/slot machines (419 of 2,248)**: Video poker, slot machines, pachinko, etc. Even when they have an arcade_db entry, many don't have thumbnails in the libretro repo. These are not "real" arcade games and often lack any promotional art.

3. **Other games (657 of 2,248)**: Games that have arcade_db entries and display names, but no matching thumbnail in the repo. Some are Japan-only titles, quiz games, or uncommon hardware.

4. **Clones/bootlegs (61 of 2,248)**: Parent game may have art but the clone-specific variant doesn't.

### sharp_x68k: 49% boxart, 91% snaps

**Snap coverage is excellent** (91%) but boxart is poor (49%). The repo has 507 boxart PNGs but 1,201 snap PNGs -- the X68000 libretro-thumbnails community contributed far more screenshots than box art.

ROM naming uses TOSEC format: `"Game Name (Year)(Publisher).dim"`. The strip_tags tier handles this well (strips from first ` (`). The version stripping tier also helps with names like `"Adventure Land 2 v0.50 (1994)(Keima)"`.

Many X68000 titles are Japanese doujin/indie software that simply have no boxart in the libretro-thumbnails database.

Additionally, many ROMs have both `.dim` and `.m3u` files (946 .m3u entries in DB). These are duplicate entries for the same game -- the .m3u is a playlist referencing the .dim files.

### arcade_fbneo: 79% boxart, 99% snaps

Snap coverage is near-perfect (99%). Boxart gap (79%) is due to:
- ROMs not in arcade_db (no display name translation)
- Titles that exist in arcade_db but have no boxart in the FBNeo repo (6,465 boxart PNGs in repo)
- The FBNeo repo has excellent snap coverage but weaker boxart coverage

### nintendo_snes: 76% boxart, 77% snaps

The main gap is **1,490 ROMs not in the DB** (filesystem has 7,275 but DB only has 5,785). These are ROMs that matched neither LaunchBox metadata nor any thumbnail.

Of the 240 DB entries without boxart:
- Fan translations (61): e.g., `"(Traducido Es)"`, `"(Traduzido Por)"` -- strip_tags matches these to the base game's region variant
- Prototypes (37): Many have no thumbnails
- Homebrew/aftermarket (33): No thumbnails expected
- Pirates (12): No thumbnails expected
- FastRom variants (7): Usually match, but some have no equivalent in the repo
- Other (81): Mix of naming mismatches and titles not in the repo

Specific naming issues found:
- `"Alien vs. Predator (USA)"` vs repo `"Alien vs Predator (USA)"` (period after "vs" in ROM name)
- `"Battletoads & Double Dragon"` vs repo `"Battletoads-Double Dragon"` (& gets converted to `_`, but repo uses `-`)

### sega_smd: 74% boxart, 74% snaps

335 ROMs not in DB. Of 479 unmatched DB entries:
- Aftermarket/homebrew (290): Largest category -- modern homebrew releases with no thumbnails
- Prototypes (54): No thumbnails expected
- Unlicensed (44): Many without thumbnails
- Other (62): Naming mismatches (e.g., `"El. Viento (USA)"` vs repo `"El.Viento (USA)"` -- space difference)

### amstrad_cpc: 70% boxart, 84% snaps

532 ROMs not in DB. The repo has 3,409 boxart PNGs but many CPC titles are obscure European software without box art in the libretro-thumbnails database. TOSEC naming is used and strip_tags handles it well. Snaps have better coverage (84%) than boxart (70%) because the community contributed more screenshots.

### arcade_dc: 59% boxart, 70% snaps

61 ROMs not in DB (29% gap). Multi-repo system (Atomiswave + Naomi + Naomi 2). Repos have 33 + 116 + 10 = 159 boxart PNGs total. 26 unmatched DB entries are games where:
- The ROM codename has no arcade_db entry
- The game's display name doesn't match any of the three repos
- Some Naomi/Naomi 2 titles simply don't have thumbnails

### sega_dc: 73% boxart, 79% snaps

Only 3 unmatched DB entries:
- `"Metropolis Street Racer v1.009"` -> strip_version -> `"Metropolis Street Racer"`, but repo has `"MSR - Metropolis Street Racer"` (abbreviated prefix)
- `"Super Street Fighter IIX for Matching Service"` vs repo `"Super Street Fighter II X"` (missing space between II and X)
- `"Virtua Tennis 2 v1.009"` -> strip_version -> `"Virtua Tennis 2"`, but repo has `"Virtua Tennis 2 - Sega Professional Tennis"` (subtitle in repo)

### commodore_ami: 0% (1 ROM, 0 matches)

Only 1 ROM exists. The Commodore - Amiga repo was cloned but it has 0 boxart files on the Pi (likely another disk space issue during clone -- the media/commodore_ami directory exists but is empty).

## Common Mismatch Patterns

### 1. No arcade_db entry for MAME codenames

The most impactful issue. 1,111 arcade_mame ROMs and an unknown number of arcade_fbneo ROMs lack arcade_db translations. Without a display name, the MAME codename (e.g., `39in1`) is used directly, which never matches the repo's display-name filenames.

### 2. Aftermarket/homebrew ROMs with no thumbnails

Across all systems, modern homebrew and aftermarket releases account for a large fraction of unmatched ROMs. These titles simply don't exist in the libretro-thumbnails database. Especially prevalent in sega_smd (290 titles) and nintendo_snes (33 titles).

### 3. ROMs not in DB at all

3,598 ROMs (12% of total) exist on the filesystem but have no DB entry. These are ROMs that matched neither LaunchBox metadata (title normalization) nor any thumbnail filename. They are invisible to the image coverage system.

### 4. Abbreviated or alternative title prefixes

Games where the repo uses an abbreviated form: `"MSR - Metropolis Street Racer"` vs `"Metropolis Street Racer"`. The strip_tags tier cannot handle cases where the base title itself differs.

### 5. Special character inconsistencies

- `&` in ROM names gets converted to `_` by `thumbnail_filename()`, but repos sometimes use `-` instead: `"Battletoads & Double Dragon"` -> `"Battletoads _ Double Dragon"` vs repo `"Battletoads-Double Dragon"`
- Period/space differences: `"El. Viento"` vs `"El.Viento"`, `"Alien vs. Predator"` vs `"Alien vs Predator"`
- `IIX` vs `II X` (missing space in Roman numerals)

### 6. Incomplete repo clones due to disk space

The Pi's disk is 100% full. The MAME repo clone is incomplete (5.1GB vs 6.0GB full), missing Named_Snaps entirely. This directly causes the 0% snap rate for arcade_mame.

### 7. Dual-file systems (dim + m3u)

Sharp X68000 has many games with both `.dim` disk images and `.m3u` playlist files. Both get DB entries, inflating the ROM count. The match rate for .m3u files (38% boxart) is worse than .dim files (58% boxart).

## Is the 3-Tier Matching Effective?

**Yes, the 3-tier system is working well for its intended purpose.** Evidence:

### Tier 1 (Exact match) handles the majority

Most console ROMs use No-Intro naming that matches the repo exactly. Systems like NES (100%), PlayStation (92%), Sega CD (94%), and Sega 32X (83%) achieve high rates from exact matching alone.

### Tier 2 (Strip-tags) is essential for fan translations and regional variants

Fan translations add tags like `(Traducido Es)` or `(Translated Fre)`. The strip_tags tier matches these to the original game. This is clearly working: many SNES translations (61+ ROMs) and other systems get matched through this tier. Example: `"3 Ninjas Kick Back (USA) (Traducido Es)"` -> matches `"3 Ninjas Kick Back (USA).png"`.

### Tier 3 (Version-stripped) is critical for TOSEC-named systems

Dreamcast, Sharp X68000, and Amstrad CPC use TOSEC naming with version strings. The version stripping tier successfully handles these. Example: `"Confidential Mission v1.002 (2001)(Sega)(PAL)(M5)[!]"` -> strip tags -> `"Confidential Mission v1.002"` -> strip version -> `"Confidential Mission"` -> matches.

### Colon variant handling helps with arcade display names

Games with colons (e.g., `"Capcom Vs. SNK: Millennium Fight 2000"`) get `_` substitution, but the repo may use ` -` or drop the colon. The colon variant fallback addresses this.

### Where the tiers fall short

The matching fails when the **base title itself differs** between ROM and repo (after all stripping). This includes:
- Abbreviated prefixes: `"MSR - Metropolis Street Racer"` vs `"Metropolis Street Racer"`
- Missing words: `"Virtua Tennis 2"` vs `"Virtua Tennis 2 - Sega Professional Tennis"`
- Completely different names: MAME codenames without arcade_db translation
- Subtle character differences: `"vs."` vs `"vs"`, `"El."` vs `"El."`

## Recommendations

### High Priority

1. **Free disk space on Pi or use larger storage** -- The 100% full disk caused incomplete MAME repo clone (0% snaps for 4,605 games) and likely the empty Commodore Amiga clone. Clearing the cloned repos after import would reclaim ~40GB+.

2. **Expand arcade_db coverage** -- 1,111 MAME ROMs and an unknown number of FBNeo ROMs lack display name translations. Expanding the database (from a more complete MAME XML) would significantly improve arcade boxart matching. Current coverage: MAME 36% boxart -> could reach ~60-70% with better DB coverage.

3. **Re-run image import after freeing disk** -- Many systems would benefit from a fresh import with complete repo clones.

### Medium Priority

4. **Add `strip_version()` to runtime `find_image_on_disk`** -- Currently only the import path uses version stripping. Adding it to the runtime path would fix Dreamcast TOSEC ROMs that fail to find their already-imported boxart at runtime.

5. **Substring/prefix matching tier** -- A new tier that checks if the ROM's stripped title is a prefix of (or is contained in) a repo filename would catch:
   - `"Virtua Tennis 2"` matching `"Virtua Tennis 2 - Sega Professional Tennis"`
   - `"Metropolis Street Racer"` partially matching `"MSR - Metropolis Street Racer"`

### Low Priority

6. **N64DD prefix stripping + fallback repo** -- Strip `N64DD - ` prefix and add `"Nintendo - Nintendo 64DD"` as fallback. Impact: 8 games.

7. **Normalize periods and `&` symbols** -- `"vs."` -> `"vs"`, `"&"` -> `"and"` or try both `_` and `-`. Would fix a handful of games across multiple systems.

8. **Deduplicate .m3u entries** -- Consider not creating DB entries for .m3u playlist files (or linking them to the same image as their .dim counterpart) to avoid inflating unmatched counts.

## NFS vs Pi Comparison

The NFS mount at `<NFS_MOUNT>/` has fewer cloned repos (7 vs 21 on Pi) but its MAME clone is complete (has Named_Snaps). NFS arcade_mame shows 1,875 boxart (vs Pi's 1,660) and 3,363 snaps (vs Pi's 0) thanks to the complete clone. NFS has more disk space available.

Some systems on the NFS have different ROM counts (e.g., SNES 7,275 on Pi vs 4,457 on NFS-local, sega_cd 32 on Pi vs 27 on NFS-local) because the NFS mount may be a subset or different snapshot of the collection.

## Previously Identified Issues -- Status

| Issue | Status | Notes |
|-------|--------|-------|
| Fix 1: Broken fake symlinks + fuzzy fallback | DONE | `resolve_fake_symlink()` + fuzzy fallback in `copy_png()` |
| Fix 1b: Post-clone fake symlink resolution | DONE | `resolve_fake_symlinks_in_dir()` runs after `git clone` |
| Fix 2: Runtime fake symlink resolution | DONE | `try_resolve_fake_symlink()` in media directory |
| Re-import N64 thumbnails | PENDING | Would recover 23 games blocked by fake symlinks |
| Fix 3: `strip_version` in runtime path | TODO | Needed for Dreamcast TOSEC ROMs |
| Fix 5: N64DD prefix + fallback repo | TODO | 8 games |
| Fix 7: Underscore normalization | TODO | 1 Sega CD game |
