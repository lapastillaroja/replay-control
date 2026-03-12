# On-Demand libretro-thumbnails: Virtual Clone Strategy

> Investigation date: 2026-03-12

## Problem

The current thumbnail import workflow requires a full `git clone --depth 1` of each system's libretro-thumbnails repo, then copies matching images to `.replay-control/media/`. This has significant costs on a Pi 4:

- **Download**: each repo is hundreds of MB to several GB (SNES is 3.2 GB, MAME is 12 GB)
- **Disk churn**: clones go to `tmp/`, images get copied to `media/`, then clones get deleted
- **Time**: cloning + matching a large system takes minutes
- **Wasted bandwidth**: we download every image in a repo but typically only need a fraction (the ones matching the user's ROMs)

Goal: fetch only the images the user actually needs, when they need them, while still supporting fuzzy matching.

---

## 1. libretro-thumbnails Repository Structure

### Organization

- **131 total repos** in the `libretro-thumbnails` GitHub organization
- One repo per system/console, named `Manufacturer_-_Console` (GitHub) / `Manufacturer - Console` (display)
- A meta-repo `libretro-thumbnails/libretro-thumbnails` contains all systems as git submodules
- **40 repos** are relevant to systems currently supported by RePlayOS

### Internal structure per repo

```
Named_Boxarts/
    Game Name (Region).png
    Game Name (Region) (Rev 1).png
    ...
Named_Snaps/
    Game Name (Region).png
    ...
Named_Titles/
    Game Name (Region).png
    ...
Named_Logos/
    Game Name (Region).png
    ...
.gitignore
```

RePlayOS only uses `Named_Boxarts` and `Named_Snaps`. Roughly 50% of files per repo are in the two categories we use (example: SNES has 3,768 boxarts + 3,742 snaps = 7,510 out of 14,857 total).

### File naming conventions

- Filenames use the game's **display name** (not ROM hash or codename)
- Special characters `&*/:\`<>?\\|"` are replaced with `_`
- Arcade games use the **display name** (not MAME codename): e.g., `Street Fighter II_ The World Warrior.png`
- Images are PNG, max 512px wide (per contributor guidelines)
- **Symlinks** are common: many region/revision variants point to the same image. These appear as small (< 100 byte) text files containing the relative path to the real image. On GitHub's tree API, they have `mode: "100644"` (not `120000` as might be expected) with a very small `size`.

### Branch naming

Most repos use `master` as default branch. Newer repos use `main`:
- `main`: Commodore - CD32, Commodore - CDTV, Sega - Naomi, Sega - Naomi 2, Philips - CD-i
- `master`: everything else

Any implementation must query the default branch via the API or try both.

### Scale (RePlayOS-relevant systems only)

| System | Files | Full Size |
|--------|------:|----------:|
| MAME | 35,075 | 11.7 GB |
| FBNeo - Arcade Games | 28,857 | 7.1 GB |
| Sony - PlayStation | 35,350 | 6.7 GB |
| Nintendo - NES | 48,167 | 3.7 GB |
| Nintendo - DS | 19,256 | 4.2 GB |
| Nintendo - SNES | 14,857 | 3.2 GB |
| Commodore - 64 | 58,758 | 1.6 GB |
| Sinclair - ZX Spectrum | 28,569 | 730 MB |
| DOS | 23,200 | 1.7 GB |
| Nintendo - GBA | 19,895 | 1.8 GB |
| Sega - Dreamcast | 3,984 | 1.7 GB |
| Sega - Saturn | 8,802 | 2.2 GB |
| *(27 more systems)* | ... | ... |
| **Total (40 repos)** | **416,243** | **57.2 GB** |

A typical RePlayOS user might have 50-500 ROMs per system, needing maybe 5-15% of available thumbnails.

---

## 2. Approaches Evaluated

### A. Blobless Partial Clone (`git clone --filter=blob:none`)

**How it works**: Clone only the git tree objects (directory listings) without any file content. Blobs (actual images) are fetched on demand when `git checkout` is used.

**Tested results**:

| Repo | Full Size | Blobless .git Size | File Count | Clone Time |
|------|----------:|-------------------:|-----------:|-----------:|
| Sega - Game Gear | 477 MB | 204 KB | 3,276 | 0.7s |
| Nintendo - SNES | 3.2 GB | 496 KB | 14,857 | 0.7s |
| Nintendo - NES | 3.7 GB | 976 KB | 48,167 | 0.9s |
| Sony - PlayStation | 6.7 GB | 904 KB | 35,350 | 0.8s |
| Commodore - 64 | 1.6 GB | 1.5 MB | 58,758 | 0.9s |
| MAME | 11.7 GB | 1.1 MB | 35,075 | 0.7s |

Key findings:
- **Blobless clone is 3,000x-16,000x smaller** than a full clone
- **Under 1 second** to clone, even for the largest repos
- `git ls-tree -r --name-only HEAD` gives the full file listing without fetching any blobs
- `git checkout HEAD -- "path/to/file.png"` fetches a single blob on demand (~0.5-1s per file)
- The .git directory grows as blobs are fetched (each blob is cached locally)
- Blobless clone with `--depth=1` combines both optimizations

**Pros**: Built-in git feature, works with any git hosting, gives you a real working tree for checked-out files.

**Cons**: Requires `git` on the device. Each on-demand fetch is a separate network round-trip (~0.5s each). The `.git/` directory grows unboundedly as blobs accumulate. Git's partial clone protocol may batch-fetch more than requested.

### B. GitHub Tree API + Raw URL Download

**How it works**: Use the GitHub REST API to get the file listing, then download images directly from `raw.githubusercontent.com`.

**File listing**: `GET /repos/libretro-thumbnails/{repo}/git/trees/{branch}?recursive=1`
- Returns all files with `path`, `sha`, `size`, `mode`, and `type`
- Even the largest repo (MAME, 35K files) returns without truncation
- Response is ~2-5 MB of JSON for large repos
- Single HTTP request, fast (~1-2s)

**Image download**: `https://raw.githubusercontent.com/libretro-thumbnails/{repo_url_name}/{branch}/Named_Boxarts/{url_encoded_filename}.png`
- Works for real files. Returns HTTP 200 with `image/png` content type.
- **Symlinks return the symlink text content** (the relative path), NOT the target image. This means we need to resolve symlinks ourselves by reading the text, finding the target filename, and fetching that instead.

**Identifying symlinks from the tree API**: Files with `size < 100` bytes that have a `.png` extension are symlinks. The tree API provides the `size` field, so we can flag them during manifest generation without downloading anything.

**Pros**: No git dependency. Precise control over what gets downloaded. Can parallelize downloads. Raw URLs are CDN-backed (fast globally).

**Cons**: Requires GitHub API access (rate limits: 5,000/hour authenticated, 60/hour unauthenticated). Symlink resolution adds complexity. URL encoding of filenames with special characters needs care.

### C. SQLite Manifest + On-Demand Fetch (Recommended Hybrid)

**How it works**: Combine approaches A/B into a two-tier system:

1. **Manifest generation** (one-time or periodic): Use the GitHub Tree API to build a SQLite database mapping `(system, kind, filename_stem)` to metadata (sha, size, is_symlink, symlink_target).
2. **On-demand fetch**: When box art is needed for a ROM, look it up in the manifest (using the existing fuzzy matching logic), then download from `raw.githubusercontent.com`.
3. **Local cache**: Downloaded images go to `.replay-control/media/` as they do today. Once downloaded, they're served from disk forever.

**Manifest schema**:
```sql
CREATE TABLE thumbnails (
    system TEXT NOT NULL,        -- e.g., "Nintendo - Super Nintendo Entertainment System"
    kind TEXT NOT NULL,          -- "Named_Boxarts" or "Named_Snaps"
    stem TEXT NOT NULL,          -- filename without .png extension
    sha TEXT,                    -- git blob SHA (useful for cache validation)
    size INTEGER,               -- file size in bytes
    symlink_target TEXT,         -- NULL if real file, target stem if symlink
    PRIMARY KEY (system, kind, stem)
);
CREATE INDEX idx_thumb_system_kind ON thumbnails(system, kind);
```

**Manifest size estimate** (for 40 RePlayOS systems, ~416K entries):
- SQLite with indexes: ~15-20 MB
- Compressed (gzip): ~3-5 MB

This is far smaller than even a single system's full clone.

---

## 3. Recommended Architecture

### Phase 1: Manifest Generation

A one-time script (or server function) that:
1. Iterates over all systems in `thumbnail_repo_names()`
2. For each system, calls the GitHub Tree API (1 request per repo)
3. Extracts `Named_Boxarts` and `Named_Snaps` entries
4. Identifies symlinks (size < 100 bytes) and resolves their targets from the tree listing
5. Inserts into a SQLite manifest (could be a new table in `metadata.db` or a separate file)

**API budget**: ~40 requests for all systems (well within the 5,000/hour limit).

**Update strategy**: The manifest changes slowly (community contributions). Monthly or on-demand refresh is sufficient. Store the commit SHA per repo to detect staleness with a single `git ls-remote` or `GET /repos/.../commits?per_page=1` call.

### Phase 2: On-Demand Image Resolution

Modify the existing `resolve_box_art()` code path:

```
resolve_box_art(system, rom_filename)
  1. Check local disk cache (.replay-control/media/{system}/boxart/)  -- existing logic
  2. If miss: look up in manifest DB using fuzzy matching
  3. If found in manifest:
     a. Construct raw.githubusercontent.com URL
     b. If entry is a symlink, use the resolved target filename instead
     c. Download PNG to .replay-control/media/{system}/boxart/
     d. Return the local URL
  4. If not in manifest: return None (no thumbnail available)
```

The download can happen:
- **Synchronously** (blocks the page render ~0.5-1s per image, but only on first view)
- **Asynchronously** (return a placeholder, trigger background download, refresh on completion) -- better UX

### Phase 3: Background Pre-fetch (Optional)

When one image from a system is requested, pre-fetch siblings in the background:
- All ROMs in the same system that have a manifest match but no local file
- Throttled to avoid saturating the Pi's network (e.g., 2-4 concurrent downloads)
- Lower priority than the directly-requested image

### Integration Points

The manifest-based approach integrates cleanly with the existing code:

| Existing Code | Change Needed |
|---|---|
| `thumbnails::thumbnail_repo_names()` | No change -- still maps systems to repo names |
| `thumbnails::thumbnail_filename()` | No change -- still normalizes filenames |
| `thumbnails::find_thumbnail()` | New variant: look up in manifest DB instead of filesystem |
| `cache::ImageIndex` | Add manifest-backed fallback when disk entry is missing |
| `cache::resolve_box_art()` | Trigger on-demand download when manifest has a match but disk doesn't |
| `import.rs` (ImportSystemImages, ImportAllImages) | Could become "pre-fetch all" using manifest instead of git clone |
| `metadata_db.rs` | Add `thumbnails` table for manifest data |

### Raw URL Construction

```
https://raw.githubusercontent.com/libretro-thumbnails/{repo_name}/{branch}/{kind}/{filename}.png
```

Where:
- `repo_name`: spaces replaced with `_` (e.g., `Nintendo_-_Super_Nintendo_Entertainment_System`)
- `branch`: `master` or `main` (must be determined per repo)
- `kind`: `Named_Boxarts` or `Named_Snaps`
- `filename`: URL-encoded PNG filename (spaces become `%20`, parentheses `%28`/`%29`, etc.)

For symlinks, the raw URL returns the text content (not the target image), so the code must:
1. Detect the symlink from the manifest
2. Use the target filename instead
3. Construct a new URL for the real file

---

## 4. Storage and Network Budget

### Manifest-only (Phase 1)
- **Disk**: ~15-20 MB for the manifest (all 40 systems)
- **Network**: ~40 API calls to generate (~5 MB total response data)
- **Time**: ~30-60 seconds to generate (API latency)

### On-demand images (Phase 2)
- **Disk**: only images actually needed. Typical user with 200 ROMs across 10 systems: ~50-200 MB
- **Network**: ~50-100 KB per image. 200 images = ~10-20 MB total
- **Latency**: ~0.3-1s per image on first access (CDN-backed raw.githubusercontent.com)

### Compared to current approach
- Current full clone for SNES alone: 3.2 GB download, ~3.2 GB temp disk, ~200 MB final media
- Manifest approach for SNES: 0.5 MB manifest data, ~200 MB final media, zero temp disk overhead

**Savings for a 10-system import**: ~20-30 GB download avoided, ~20-30 GB temp disk avoided.

---

## 5. RePlayOS-Specific Considerations

### Pi 4 Constraints
- **RAM**: 1-4 GB. The manifest fits comfortably in memory if needed, but SQLite avoids that requirement.
- **SD card wear**: Eliminates the massive temp clone write/delete cycle, significantly reducing wear.
- **CPU**: No heavy `git` operations (packfile decompression was the main CPU cost of cloning).
- **Network**: Pi 4 has gigabit ethernet and WiFi. Individual image fetches are small; latency is the bottleneck, not bandwidth.

### USB / SD / NFS Storage
- Manifest can live in `metadata.db` (same NFS `nolock` considerations apply).
- Downloaded images go to `.replay-control/media/` as today -- no change.
- On NFS, concurrent downloads from multiple Pis sharing the same storage would be fine (each Pi fetches its own needed images; duplicates are harmless).

### Offline Operation
- If no network: resolve_box_art falls through to "no thumbnail" for uncached images. Already-downloaded images continue to work.
- The manifest itself is offline-usable once generated. Only the image download step requires network.
- The existing "Re-match All" feature (offline rematch from cloned repos) remains available for users who manually place repos.

### Migration Path
- The new system is purely additive. Existing imported images remain in place.
- The "Download Images" UI could gain a new mode: "Download (fast)" using on-demand fetch vs. "Download (full repo)" for the legacy git-clone approach.
- Eventually the git-clone path could be deprecated.

---

## 6. Alternative: Pre-built Manifest Distribution

Instead of each Pi generating its own manifest via the GitHub API:

- Host a pre-built manifest (SQLite or compressed JSON) at a known URL
- Update it weekly/monthly via a GitHub Action in the replay repo
- Pi downloads ~3-5 MB compressed manifest file on first use or update

This avoids GitHub API rate limits entirely and makes initial setup instant. The GitHub Action would:
1. Use the Tree API to enumerate all files across all 40 repos
2. Build the SQLite manifest
3. Compress and upload to GitHub Releases or a static host

---

## 7. Summary of Recommendations

1. **Short term**: Implement the SQLite manifest + on-demand fetch approach (Phase 1 + 2). This eliminates the biggest pain point (multi-GB clones) with minimal code changes.

2. **Medium term**: Add background pre-fetch (Phase 3) so that browsing a system gradually fills in all thumbnails without explicit user action.

3. **Long term**: Consider the pre-built manifest distribution to eliminate even the API calls from the Pi.

The blobless partial clone approach (option A) is technically elegant but adds complexity (git dependency, growing .git directory, batch fetch unpredictability). The manifest + raw URL approach is simpler, more predictable, and better suited to the Pi's constraints.
