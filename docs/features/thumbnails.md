# Thumbnails

How box art and screenshot images are matched, downloaded, and served.

## Image Sources

All images come from [libretro-thumbnails](https://github.com/libretro-thumbnails) GitHub repos. There are ~40 repos relevant to RePlayOS systems, each containing `Named_Boxarts/`, `Named_Snaps/`, and `Named_Titles/` directories.

Image filenames use the game's display name (not ROM hash), with special characters `&*/:\`<>?\\|"` replaced by `_`.

## Thumbnail Index (Manifest)

The `thumbnail_index` table in `metadata.db` stores a manifest of all available images across all repos. It is populated by querying the GitHub REST API (`git/trees` endpoint) for each repo's tree, which returns filenames, sizes, and symlink targets.

This index enables:
- On-demand single-image downloads (no need to clone entire repos)
- Fuzzy matching against the full catalog
- Variant discovery for the box art swap feature

The `data_sources` table tracks per-repo freshness (last indexed commit SHA, timestamp).

## Matching Pipeline

`resolve_box_art()` in `cache.rs` resolves a ROM filename to a box art URL using a 5-tier fallback:

### Tier 1: DB Path
Check `game_metadata.box_art_path` (set during image import). If present, the image was already matched and copied to `media/`.

### Tier 2: Exact Match
`thumbnail_filename(stem)` normalizes the ROM filename stem to match the libretro naming convention, then checks for an exact file on disk at `media/{system}/boxart/{name}.png`.

### Tier 3: Fuzzy Match (Strip Tags)
`base_title()` strips region/revision tags `(...)` and `[...]`, lowercases, and reorders trailing articles ("Title, The" becomes "The Title"). Matches against files on disk.

### Tier 4: Version-Stripped Match
`strip_version()` further removes version numbers and revision indicators from the tag-stripped name for even looser matching. This tier checks both exact files on disk and the fuzzy index, fixing cases like Dreamcast TOSEC-named ROMs (e.g., `v1.004`) matching No-Intro thumbnails.

### Tier 5: On-Demand Download
If no local match is found but the `thumbnail_index` has a manifest entry, `queue_on_demand_download()` fetches the single PNG from `raw.githubusercontent.com` in a background thread. The image appears on the next page load.

## Arcade Image Matching

Arcade ROMs use MAME codenames (`sf2.zip`), not human-readable names. The matching pipeline translates codenames to display names via `arcade_db` before matching against thumbnail filenames.

Multi-repo support: `arcade_dc` maps to both Sega Naomi and Sega Naomi 2 repos. The `thumbnail_repo_names()` function handles this mapping.

## Thumbnail Counts

The metadata page displays per-system thumbnail counts. These are derived from `game_library.box_art_url` (live enrichment data) rather than `game_metadata.box_art_path` (stale import-time data). This ensures counts reflect the current state of enriched games, not historical import records that may reference deleted or orphaned images.

## Image Import (Legacy Git Clone Path)

The metadata page offers per-system and "Download All" image import. This path:
1. Shallow-clones the libretro-thumbnails repo for the system
2. Walks `Named_Boxarts/` and `Named_Snaps/`, fuzzy-matching against ROM filenames
3. Copies matched images to `media/{system}/boxart/` and `media/{system}/snap/`
4. Auto-deletes the cloned repo after matching to save disk space
5. Reports progress via SSE (`/sse/image-progress`)

Supports cancellation (kills git clone subprocess, stops copy loop via `AtomicBool`).

## Box Art Swap

Users can pick alternate region-variant cover art on the game detail page. The feature queries `thumbnail_index` for all boxart entries sharing the same base title, de-duplicates by symlink target, and presents them in a bottom sheet picker.

Overrides are stored in `user_data.db` (`box_art_overrides` table), which survives metadata clears.

See `research/investigations/box-art-swap.md` for the full design.

## exFAT Symlink Resolution

libretro-thumbnails repos use symlinks for region variants pointing to the same image. On exFAT (common for USB drives), git writes symlink targets as small text files. The import code detects files under 200 bytes as potential fake symlinks and resolves them.

## Key Source Files

| File | Role |
|------|------|
| `replay-control-core/src/metadata/thumbnails.rs` | Fuzzy matching, base_title, strip_tags, image import |
| `replay-control-core/src/metadata/thumbnail_manifest.rs` | Manifest index, on-demand download, GitHub API |
| `replay-control-app/src/api/cache.rs` | `resolve_box_art()`, `queue_on_demand_download()`, `ImageIndex` |
| `replay-control-app/src/api/import.rs` | Image import pipeline, SSE progress, `update_image_paths_from_disk` |
| `replay-control-core/src/user_data_db.rs` | Box art overrides storage |
