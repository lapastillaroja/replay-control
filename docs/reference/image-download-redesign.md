# Image Download Redesign

Analysis and proposal for redesigning the image download section on the metadata page.

Last updated: 2026-03-11

## Current State

### Architecture

```
User clicks "Download" (per system) or "Download All"
    │
    ▼
clone_thumbnail_repo()
    │  git clone --depth 1 into .replay-control/tmp/libretro-thumbnails/<repo>/
    │  If repo already exists and is not stale → reuse (skip clone)
    │  If stale (local HEAD != remote HEAD) → delete and re-clone
    │
    ▼
resolve_fake_symlinks_in_dir()
    │  Replaces git fake symlinks (exFAT text files) with real copies
    │
    ▼
import_system_thumbnails()
    │  Walks all ROM filenames, matches against repo via 3-tier fuzzy match
    │  Copies matched PNGs to .replay-control/media/<system>/boxart/ and snap/
    │  Updates metadata.db with image paths
    │
    ▼
Repo stays in tmp/  ← THE PROBLEM
```

### Current UI Layout (Images Section)

```
┌──────────────────────────────────────┐
│ Images                               │
│ "Download box art and screenshots    │
│  from libretro-thumbnails..."        │
│                                      │
│ Box Art:      19,687                 │
│ Screenshots:  20,905                 │
│ Media Size:   7.6 GB                 │
│                                      │
│ [Download All]  [Re-match All]       │
│                                      │
│ ┌──────────────────────────────────┐ │
│ │ Amstrad CPC      2910/4168  [Up]│ │
│ │ Arcade (FBNeo)   3241/4082  [Up]│ │
│ │ Arcade (MAME)    1660/4605  [Up]│ │
│ │ ...per system with Download/    │ │
│ │    Update button...             │ │
│ └──────────────────────────────────┘ │
├──────────────────────────────────────┤
│ Data Management                      │
│ [Clear Images]                       │
└──────────────────────────────────────┘
```

### Current Server Functions

| Function | Action |
|---|---|
| `import_system_images(system)` | Clone repo + match + copy for one system |
| `import_all_images()` | Clone + match + copy for ALL systems with ROMs |
| `rematch_all_images()` | Re-match using existing clones (no download), re-clones if stale |
| `cancel_image_import()` | Sets cancel flag |
| `clear_images()` | Deletes `media/` only -- does NOT touch `tmp/` |

### Current Staleness Check

`is_repo_stale()` compares local `git rev-parse HEAD` against remote `git ls-remote --heads <url> master`. If they differ, the repo is considered stale and is deleted + re-cloned. This runs on both "Download" and "Re-match" paths.

Key observation: "Re-match All" is not purely offline. If a repo is stale, it re-clones it, which requires network access and disk space. The button label "Re-match All" with tooltip "no download needed" is misleading.

## Problems Identified (with Real Numbers)

### 1. Disk Space: 10:1 Overhead

On the Pi (233 GB USB drive):
- `tmp/` (21 cloned repos): **76 GB**
- `media/` (matched images): **7.6 GB**
- Ratio: **10:1** -- for every 1 GB of useful images, 10 GB of repo data is retained

The disk is 100% full (233G/233G). This directly caused:
- Incomplete MAME repo clone (5.1 GB vs 6.0 GB full) -- 0% snap coverage for 4,605 games
- Likely empty Commodore Amiga clone
- No room for new ROMs or other data

### 2. Download All is a Disk Bomb

"Download All" clones repos for every system with ROMs. On the Pi:
- 21 systems with repos = 76 GB of clones
- On a 233 GB disk already holding ROMs, this fills the disk completely
- No warning, no estimated size, no way to know this before clicking
- Takes a very long time (hours on slow connections)

### 3. No Way to Reclaim tmp/ Space

"Clear Images" only deletes `media/` (7.6 GB). The much larger `tmp/` (76 GB) has no UI to clear it. Users must SSH in and manually `rm -rf .replay-control/tmp/` to reclaim space.

### 4. Download and Match are Coupled

The per-system "Download" button does clone + match as one atomic operation. If a user adds new ROMs or renames files, they must re-download the entire repo to re-match, even though the repo data is already on disk.

The "Re-match All" button partially addresses this, but:
- It only works for systems that already have cloned repos
- It re-clones stale repos (so it is not truly offline)
- There is no per-system "Re-match" button

### 5. Individual Repo Sizes are Invisible

Users cannot see how large a repo is before downloading. Repo sizes range from 43 MB (Naomi 2) to 15 GB (NES, PlayStation, SNES). A user downloading "Nintendo - NES" for 15 ROMs gets a 15 GB repo for 15 matched images.

### 6. Repos are Kept Permanently by Default

The design doc (`game-metadata.md`) recommended transient processing (download, process, delete) as the default. The current implementation keeps repos permanently for staleness optimization, but this optimization costs 76 GB.

## Proposed Redesign

### A. Separate Download from Match (Already Partially Done)

The infrastructure for separated operations already exists:
- `clone_thumbnail_repo()` handles download
- `import_system_thumbnails()` handles matching
- `rematch_all_images()` exists as a server function

What is needed:
1. **Per-system "Re-match" button** -- currently only "Re-match All" exists at the top level. Add an individual button per system row.
2. **Make re-match truly offline** -- remove the staleness check from the re-match path. If the user wants fresh data, they click "Download". If they want to re-match against local data, it should work without network.
3. **Show per-system repo status** -- indicate whether a repo is cloned locally, so users know re-match is available.

### B. Storage Strategy: Delete After Match (Default) + Optional Retention

**Recommended approach**: Return to the original transient design. Delete repos after matching, with a clear option to keep them.

#### Phase 1: Auto-Cleanup After Match

After `import_system_thumbnails()` completes for a system, delete the cloned repo:

```
clone repo → match ROMs → copy images → DELETE REPO
```

This immediately reclaims the 10:1 overhead. On the Pi, this would free ~76 GB.

**Trade-off**: The next "Download" for that system must re-clone. But with `--depth 1`, clone times are reasonable (a few minutes per system on broadband), and the alternative (disk full, incomplete clones) is worse.

#### Why Not Other Options?

| Option | Verdict | Reason |
|---|---|---|
| **Keep repos, add "Clear cache"** | Rejected as default | Users forget to clear; disk fills up silently. OK as opt-in. |
| **Selective retention (keep small, delete large)** | Over-engineered | Adds complexity for marginal benefit. The threshold is arbitrary. |
| **Compressed archive after match** | Rejected | git objects are already compressed; the bulk is the working tree. Compressing would save ~30-50% but adds complexity. Still multi-GB. |
| **`--filter=blob:none` / sparse checkout** | Investigated | `--filter=blob:none` with sparse checkout of `Named_Boxarts` + `Named_Snaps` would skip other dirs (`.git` metadata, `Named_Logos`, `Named_Titles`). However, libretro-thumbnails repos store images as regular blobs (not LFS), so `blob:none` would download them on checkout anyway. Sparse checkout could help slightly (skip `Named_Logos`/`Named_Titles`), but the savings are small (~5-10%) and add git complexity. |
| **GitHub raw URLs per file** | Rejected | Would require knowing exact filenames upfront (chicken-and-egg with matching). Also, 5,000+ individual HTTP requests per system would be slower and rate-limited. |

#### Phase 2 (Optional): Keep-Repo Toggle

For power users who want staleness checks and fast re-imports, add a setting: "Keep downloaded repos for faster updates" (default: off). When on, repos are kept in `tmp/` as today. When off (default), repos are deleted after matching.

### C. Download All: Rethink, Don't Remove

"Download All" is useful but needs guardrails.

**Proposal**: Keep it, but add:

1. **Estimated size warning** -- Before starting, show: "This will download approximately X GB of data. Your disk has Y GB free. Continue?"
2. **Process-and-delete pipeline** -- With the Phase 1 cleanup, "Download All" becomes: clone system 1 → match → delete → clone system 2 → match → delete → ... Peak disk usage is one repo at a time (max ~15 GB for NES/PlayStation), not all 76 GB simultaneously.
3. **Skip systems with full coverage** -- Optionally skip systems where boxart coverage is already 100% (or above a threshold like 90%).

The process-and-delete pipeline is the key change. Instead of downloading all 21 repos simultaneously (76 GB), it downloads one at a time (max 15 GB peak), processes it, deletes it, then moves to the next. This makes "Download All" viable even on constrained disks.

### D. Add "Clear Image Cache" to UI

Add a visible "Clear cache" action in the Data Management section:

```
┌──────────────────────────────────────┐
│ Data Management                      │
│                                      │
│ [Clear Images]    [Clear Cache]      │
│  (7.6 GB)          (76 GB)           │
│                                      │
│ "Clear Images" removes all box art   │
│ and screenshots.                     │
│ "Clear Cache" removes downloaded     │
│ repo data used for image matching.   │
└──────────────────────────────────────┘
```

Implementation: Add a `clear_image_cache()` server function that deletes `.replay-control/tmp/libretro-thumbnails/`. Show the size of `tmp/` on the page so users understand the impact.

### E. Show Cache/Disk Info on Page

Add informational rows to the image stats section:

```
Box Art:        19,687
Screenshots:    20,905
Media Size:     7.6 GB
Cache Size:     76 GB        ← NEW
Disk Free:      0 bytes      ← NEW (or omit if not easily available)
```

This makes the cost of cached repos visible. Users can then decide to clear the cache or keep it.

### F. Per-System Button Improvements

Current per-system row:
```
Amstrad CPC       2910/4168   [Update]
```

Proposed:
```
Amstrad CPC       2910/4168   [Download]  [Re-match]
                               (1.2 GB)    (cached)
```

- Show repo size estimate (can be fetched from GitHub API or hardcoded from known sizes)
- Show cache status: "cached" if repo exists in `tmp/`, blank if not
- "Re-match" button only enabled when repo is cached (otherwise greyed out with tooltip "Download first")
- "Download" always available; does clone + match + (optionally) delete

**Simplification alternative**: If the auto-cleanup design (Phase 1) is adopted, repos are never cached, so "Re-match" per system becomes rare. In that case, keep only the "Download" button per system, and keep "Re-match All" as a top-level action for users who manually keep repos.

## Proposed UI Mockup

```
┌──────────────────────────────────────────┐
│ Images                                    │
│ "Download box art and screenshots         │
│  from libretro-thumbnails."               │
│                                           │
│ Box Art:        19,687                    │
│ Screenshots:    20,905                    │
│ Media Size:     7.6 GB                    │
│ Cache Size:     76 GB                     │
│                                           │
│ [Download All]  [Re-match All]  [Stop]    │
│                                           │
│  ┌─────────────────────────────────────┐  │
│  │ Amstrad CPC     2910/4168  [Update] │  │
│  │ Arcade (FBNeo)  3241/4082  [Update] │  │
│  │ Arcade (MAME)   1660/4605  [Update] │  │
│  │ ...                                 │  │
│  └─────────────────────────────────────┘  │
├───────────────────────────────────────────┤
│ Data Management                           │
│                                           │
│ [Clear Images] (7.6 GB)                   │
│  Removes all box art and screenshots      │
│                                           │
│ [Clear Cache] (76 GB)                     │
│  Removes downloaded thumbnail repos.      │
│  Re-match will not work until repos       │
│  are downloaded again.                    │
└───────────────────────────────────────────┘
```

## Implementation Plan

### Phase 1: Auto-Cleanup + Clear Cache Button (High Priority)

**Goal**: Prevent disk from filling up; give users a way to reclaim space.

1. **Auto-delete repos after successful match** -- In `import_system_images_blocking()`, after `import_system_thumbnails()` completes successfully for a system, delete the repo directory. This is a ~5-line change in `import.rs`.

2. **Add `clear_image_cache()` server function** -- Delete `.replay-control/tmp/libretro-thumbnails/`. Register in `main.rs`.

3. **Add `tmp_dir_size()` function** to `thumbnails.rs` -- Calculate size of `tmp/libretro-thumbnails/`.

4. **Show cache size** in image stats (add to `get_image_stats()` return value).

5. **Add "Clear Cache" button** to Data Management section in `metadata.rs`.

6. **Add i18n keys**: `metadata.clear_cache`, `metadata.clearing_cache`, `metadata.cleared_cache`, `metadata.confirm_clear_cache`, `metadata.cache_size`.

**Estimated effort**: Small -- mostly plumbing. The core change (auto-delete) is trivial.

### Phase 2: Download All Pipeline (Medium Priority)

**Goal**: Make "Download All" safe on constrained disks.

1. **Sequential process-and-delete** -- Modify `start_all_images_import()` to delete each repo after its system is processed, before moving to the next system. This ensures peak disk usage is one repo at a time.

2. **Pre-flight disk check** -- Before starting "Download All", estimate total download size and check available disk space. Show a warning if free space is less than the largest single repo (~15 GB).

3. **Skip fully-covered systems** -- Add an option to skip systems where coverage is above a threshold (e.g., 90% boxart).

**Estimated effort**: Medium -- the sequential delete is straightforward; the pre-flight check requires estimating repo sizes.

### Phase 3: Re-match Improvements (Low Priority)

**Goal**: Make re-match a first-class operation.

1. **Remove staleness check from re-match path** -- In `rematch_system_images_blocking()`, remove the `is_repo_stale()` check. Re-match should be purely local.

2. **Per-system re-match button** -- Add individual "Re-match" buttons to system rows (only enabled when repo is cached).

3. **Better re-match visibility** -- Move "Re-match All" to be more prominent, with a clearer label like "Re-scan ROM matches" and a description.

**Estimated effort**: Small for the staleness fix; medium for UI changes.

### Phase 4: Informational Improvements (Low Priority)

**Goal**: Better visibility into what is happening and what it costs.

1. **Show per-system repo size** -- Either from GitHub API or a hardcoded lookup table.

2. **Show download progress with size** -- During cloning, show bytes downloaded (requires parsing git clone stderr or using a progress callback).

3. **Show disk free space** -- Add disk stats to the page.

**Estimated effort**: Medium -- GitHub API integration or size table maintenance.

## Migration Considerations

### Existing Users with Cached Repos

Users who already have 76 GB of cached repos need a path to reclaim space.

1. **Phase 1 gives them the "Clear Cache" button** -- they can reclaim space immediately.
2. **Auto-cleanup only applies to new downloads** -- existing cached repos are untouched until the user clears them or downloads again.
3. **No data loss** -- clearing `tmp/` only loses cached repos, not matched images in `media/`. All previously matched images remain intact.
4. **Re-match still works** until cache is cleared -- users who want to re-match can do so before clearing.

### Behavioral Change

Users who relied on the staleness optimization (repos kept for quick updates) will now experience full re-downloads when updating. This is acceptable because:
- `--depth 1` clones are fast (minutes, not hours)
- The disk space savings (76 GB) far outweigh the time cost of re-cloning
- The staleness check itself requires network access (`git ls-remote`), so there is no fully-offline advantage to keeping repos

### Settings Migration

No settings file changes needed. The auto-cleanup is a code-level behavior change, not a user setting. If the optional "keep repos" toggle (Phase 2) is added, it would be a new key in `settings.cfg`.

## Summary

| Change | Impact | Effort | Priority |
|---|---|---|---|
| Auto-delete repos after match | Prevents disk fill (saves ~76 GB) | Small | P0 |
| Add "Clear Cache" button | Lets existing users reclaim space | Small | P0 |
| Show cache size on page | Makes cost visible | Small | P0 |
| Sequential process-and-delete for Download All | Peak usage = 1 repo instead of all | Medium | P1 |
| Remove staleness check from re-match | Makes re-match truly offline | Small | P2 |
| Per-system re-match button | Convenience | Medium | P3 |
| Pre-flight disk check for Download All | Safety warning | Medium | P3 |
| Show per-system repo size | Informed decisions | Medium | P3 |
