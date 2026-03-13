# ROM Auto Re-Match and Image Repo Auto-Cleanup

Two related features for keeping the ROM library, metadata, and thumbnail images
in sync as the collection changes over time. Both address the same user scenario:
ROMs are added or removed externally (scp, file manager, USB copy/delete), and
the app should react without manual intervention.

---

## Feature 1: New ROMs Auto Re-Match

### Goal

When ROMs are added externally, the app should automatically match them against
existing metadata (LaunchBox) and thumbnails on the next cache refresh, instead
of requiring a manual re-import.

### Current Behavior

Tracing the full lifecycle of a ROM added externally (e.g., scp a new `.sfc`
into `/roms/nintendo_snes/`):

#### Step 1: Detection (works today)

The ROM appears in the game list automatically. The cache invalidation chain is:

1. Adding a file to a directory updates the directory's mtime.
2. On the next request (or at startup via `spawn_cache_verification`), the cache
   checks `dir_mtime()` against the stored `game_library_meta.dir_mtime_secs`.
3. Mtime mismatch triggers an L3 filesystem scan (`list_roms()`), which discovers
   the new ROM and writes through to L1 + L2.
4. `spawn_cache_verification` (background.rs:17-100) calls `get_roms()` for stale
   systems, then calls `enrich_system_cache()`.

**Key code path**: background.rs:72-77:
```rust
let _ = state.cache.get_roms(&storage, &meta.system, region_pref);
state.cache.enrich_system_cache(&state, &meta.system);
```

**Limitation**: `spawn_cache_verification` runs only once at startup, not
periodically. While the server is running, cache invalidation only happens when a
user visits a page that triggers `get_roms()` or `get_systems()`. There is no
periodic background rescan.

#### Step 2: Metadata Auto-Match (partially works today)

`enrich_system_cache` (cache.rs:1026-1112) calls `auto_match_metadata`
(cache.rs:1122-1246), which:

1. Loads all existing `game_metadata` entries for the system.
2. Builds a normalized-title index (`normalize_title(stem) -> GameMetadata`).
3. Iterates all ROMs in L1 cache, finds those without a `game_metadata` entry.
4. For unmatched ROMs, normalizes the filename and looks for a match in the title
   index.
5. If found, creates a new `game_metadata` row (source: `"launchbox-auto"`) with
   the donor's description, rating, publisher, and genre.

**This already works.** When `enrich_system_cache` runs after detecting a stale
system, new ROMs are auto-matched to existing LaunchBox entries by normalized
title. The matched metadata is persisted to `game_metadata` so future lookups
hit directly.

However, the auto-match has a prerequisite: existing `game_metadata` entries for
the system must already exist (from a prior LaunchBox import). If no import has
ever been done, there is nothing to match against.

#### Step 3: Thumbnail Resolution (partially works today)

`enrich_system_cache` also resolves box art via `resolve_box_art`
(cache.rs:807-890), which uses a 5-tier resolution strategy:

1. **DB path** -- checks `game_metadata.box_art_path` via `ImageIndex.db_paths`
2. **Exact match** -- `thumbnail_filename(stem)` against files on disk
3. **Colon variants** -- for arcade games with `: ` in display names
4. **Fuzzy match** -- `base_title()` strips tags, lowercases, reorders articles
5. **On-demand download** -- if `thumbnail_index` has a manifest match, queues
   a background download via `queue_on_demand_download`

**Tier 5 (on-demand) works for new ROMs**, but only if the thumbnail index has
been populated (user ran "Update Images" at least once). The manifest is built
from GitHub repo listings, not from local ROMs, so it already contains entries
for games the user hasn't added yet.

**The on-demand download writes the image to disk and invalidates the system's
image cache**, but does NOT update `game_metadata.box_art_path`. This means the
DB path lookup (tier 1) won't find it, but the exact/fuzzy disk-based lookup
(tiers 2-4) will.

### Where the Gaps Are

#### Gap 1: No periodic background rescan

`spawn_cache_verification` runs once at startup. If the server is running when
ROMs are added, the new ROMs are only detected when:
- A user visits the system page (triggers `get_roms` -> L2 mtime check -> L3 scan)
- The hard TTL expires (300 seconds) and a request triggers a rescan

The home page game counts stay stale until the system is visited, because
`get_systems` reads from L2 `game_library_meta` without checking individual system
directory mtimes.

**File**: `replay-control-app/src/api/background.rs`
**Lines**: 17-100 (runs once at startup, no periodic rescheduling)

#### Gap 2: `game_metadata.box_art_path` not updated for on-demand downloads

When `queue_on_demand_download` saves a new thumbnail, it calls
`invalidate_system_images` so the `ImageIndex` is rebuilt on next request. But
it does NOT call `update_image_paths_from_disk` or update
`game_metadata.box_art_path`. This means:

- The image appears in the UI (via disk-based lookup in `resolve_box_art` tiers 2-4)
- But the `game_metadata` table doesn't know about it (tier 1 won't hit)
- The metadata coverage page (`get_system_coverage`) undercounts thumbnails

This is a minor cosmetic gap -- the image shows up correctly -- but makes the
coverage stats slightly inaccurate.

**File**: `replay-control-app/src/api/cache.rs`
**Lines**: 892-945 (`queue_on_demand_download`)

#### Gap 3: New system directories not detected

`spawn_cache_verification` only iterates systems already in `game_library_meta`.
If ROMs are added for a system that has never been scanned (e.g., user creates
a new `roms/atari_lynx/` directory), it won't be detected until:
- A user visits the home page and triggers `get_systems` (L3 scan)
- Or the hard TTL on the systems cache expires

**File**: `replay-control-app/src/api/background.rs`
**Lines**: 58-79 (only iterates `cached_meta`)

### Filesystem Watching vs Polling

Before choosing an implementation, it's worth evaluating whether the app should
use OS-level filesystem watching (instant notification) instead of or alongside
periodic polling (mtime checks every N seconds).

#### inotify on local filesystems (ext4 on USB/SD)

Linux's `inotify` subsystem works well on local filesystems like ext4, which is
the filesystem used on USB drives and SD cards in RePlayOS. It is fully
supported on ARM Linux (the Pi's kernel includes inotify), with negligible
overhead: the kernel delivers events via file descriptors, so there is no CPU
cost when nothing changes.

The project already uses `inotify` successfully via the `notify` crate (v8) for
config file watching in `spawn_storage_watcher` (background.rs:226-265). The
`recommended_watcher()` call selects `inotify` automatically on Linux. Watching
the `roms/` directory tree would use the same mechanism.

For ROM detection, watching with `RecursiveMode::Recursive` on the `roms/`
directory would deliver `Create`, `Modify`, and `Remove` events immediately when
files are added or deleted. This would eliminate the detection lag inherent in
polling.

#### inotify on NFS

inotify does **not** work for detecting changes made by other NFS clients. This
is a well-known, fundamental limitation: `inotify` is a kernel-level facility
that hooks into the VFS layer for **local** filesystem operations. When another
machine writes to the NFS share, the NFS server does not push notifications to
connected clients — the local kernel never sees the write, so no inotify event
fires.

Changes made by the Pi **itself** (e.g., saves written by a running game) would
still generate inotify events, because those go through the local VFS. But the
primary use case for NFS is a remote collection managed from a desktop, so
changes made externally are the norm.

The `notify` crate will attempt to set up an inotify watch on NFS-mounted paths.
It may succeed (the `inotify_add_watch` syscall does not reject NFS paths), but
the watch will silently miss remote changes. This is worse than an outright
failure because it gives a false sense of real-time detection.

However, automatic polling on a timer is not needed for NFS either. The existing
metadata page already has "Update" and "Clear" buttons that trigger
`enrich_system_cache()` for all systems, which runs `auto_match_metadata()` —
the exact code path that matches new ROMs. When a user adds ROMs via scp, their
natural next step is visiting the app's metadata page and triggering an update.
This manual flow is sufficient for the NFS use case.

#### fanotify

`fanotify` (the newer Linux filesystem notification API) has the same
limitation: it monitors the local kernel's VFS, not remote NFS server events.
It offers advantages over inotify for other use cases (system-wide monitoring,
permission decisions), but does not solve the NFS problem. Not worth pursuing
here.

#### Rust crate options

The `notify` crate (v8) is **already a dependency** of `replay-control-app`,
gated behind the `ssr` feature flag (`Cargo.toml:34,75`). It is already used
for config file watching (`background.rs:270`), so adding ROM directory watching
introduces no new dependencies or binary size increase.

`notify` uses `inotify` on Linux and provides a cross-platform
`recommended_watcher()` that selects the best backend for the current OS. It
also has a `PollWatcher` backend that can be used as an explicit fallback, but
the project's existing mtime-based polling is more efficient for this use case
because it only stats one directory per system rather than scanning the entire
tree.

#### Hybrid approach recommendation

Use a **hybrid strategy**: `notify` (inotify) for instant detection on local
mounts, and the existing manual "Update" flow on the metadata page for NFS.

The project already distinguishes `StorageKind::Nfs` from local storage types
(`Sd`, `Usb`, `Nvme`) via `StorageLocation::kind`. This is the same boundary
used for the `nolock` VFS fallback in SQLite access (`db_common.rs`). The
detection strategy can branch on this:

- **Local storage** (`Sd`, `Usb`, `Nvme`): Set up a `notify` watcher on
  `roms/` with `RecursiveMode::Recursive`. React to `Create` and `Remove`
  events by marking the affected system as stale and triggering
  `get_roms` + `enrich_system_cache`. Use a debounce window (2-3 seconds) to
  batch rapid changes (e.g., bulk copy operations). This gives sub-second
  detection latency for the common case.

- **NFS storage**: No watcher, no periodic polling timer. The existing "Update"
  and "Clear" buttons on the metadata page already trigger
  `enrich_system_cache()` for all systems, which calls `auto_match_metadata()`.
  This is the exact code path that matches new ROMs. When a user adds ROMs
  via scp, their natural workflow is to visit the metadata page and trigger an
  update. The 300-second hard TTL remains as an additional safety net for
  page-level cache freshness.

**Benefits of the hybrid approach:**

- Near-instant detection on local storage (the majority deployment on Pi)
- No watcher or timer complexity for NFS — the existing UI flow handles it
- Zero new dependencies (reuses existing `notify` crate)
- Graceful degradation: if `notify` fails to set up a watcher (permissions,
  kernel limits), the user can always trigger a manual update from the
  metadata page

### Proposed Implementation

#### Change 1: Filesystem watcher for local storage (background.rs)

For **local storage** (`Sd`, `Usb`, `Nvme`): set up a `notify` filesystem
watcher on the `roms/` directory using `RecursiveMode::Recursive`, following
the same pattern as the existing config file watcher
(`try_start_config_watcher`). React to `Create` and `Remove` events by
marking the affected system stale and scheduling `get_roms` +
`enrich_system_cache`. Use a 2-3 second debounce window to batch bulk
operations.

For **NFS storage**: no watcher or polling timer is needed. The existing
"Update" button on the metadata page already triggers `enrich_system_cache()`
for all systems, which runs `auto_match_metadata()`. When a user adds ROMs
via scp, they naturally visit the metadata page and trigger an update.

**File**: `replay-control-app/src/api/background.rs`

Add a new method `spawn_rom_watcher` (parallel to `spawn_storage_watcher`)
that:

1. Checks `storage.kind` to decide whether to set up a watcher.
2. For local storage, calls a new `try_start_rom_watcher` that sets up a
   `notify` watcher on `storage.roms_dir()` with `RecursiveMode::Recursive`.
   Events are sent through a `tokio::sync::mpsc` channel (same pattern as
   `try_start_config_watcher`). The event loop debounces and extracts the
   system folder name from the event path, then triggers a targeted rescan.
3. If the watcher setup fails, logs a warning — the user can still trigger
   rescans manually from the metadata page.
4. For NFS, skips watcher setup entirely (inotify is unreliable on NFS, and
   the manual "Update" flow covers this case).
5. Also checks if the `roms/` directory mtime has changed, and if so, triggers
   a `get_systems` refresh to detect new system directories.
6. Guards against concurrent execution with the `metadata_operation_in_progress`
   atomic.

**Estimated lines of code**: ~80 new lines for watcher setup + debounce +
event handling.

The new `check_stale_systems` method would mirror the logic from
`spawn_cache_verification` lines 56-92, but wrapped to be callable both from
the watcher event handler and from the metadata page "Update" flow. Protect
against overlap with the `metadata_operation_in_progress` atomic.

#### Change 2: Detect new system directories (background.rs)

In `check_stale_systems` (or as part of the periodic check), also store and
compare the `roms/` directory mtime. When it changes, call
`cache.get_systems(storage)` to do an L3 scan, which will discover new system
directories. Then iterate the new systems and trigger `get_roms` +
`enrich_system_cache` for each.

**File**: `replay-control-app/src/api/background.rs`
**Estimated lines**: ~15 additional lines within the periodic check method.

#### Change 3 (optional): Update game_metadata.box_art_path on download

In `queue_on_demand_download` (cache.rs:892-945), after successfully saving the
thumbnail, also update `game_metadata.box_art_path` for the matching ROM. This
would require knowing the `rom_filename` (currently not passed through) and
calling `db.bulk_update_image_paths`.

**File**: `replay-control-app/src/api/cache.rs`
**Lines**: 919-934 (the success branch of `download_thumbnail`)
**Estimated lines**: ~15 additional lines.

This is low priority since the image already appears correctly via disk lookup.

### Edge Cases

1. **Partially copied files**: A large ROM being copied via scp will show up in
   the directory listing while still incomplete. `list_roms` will include it.
   With filesystem watching, the debounce window (2-3 seconds) helps for small
   files but won't fully solve large transfers. The ROM won't be launchable by
   RePlayOS either, so this is a non-issue in practice -- the user would simply
   see it appear and then work once the copy finishes. On local storage, the
   watcher will fire again when the copy completes (`CLOSE_WRITE` triggers a
   `Modify` event via `notify`), causing a second rescan that picks up the
   final state. On NFS, the user triggers the update manually from the metadata
   page after the copy finishes.

2. **NFS mounts — no auto-detection, by design**: The hybrid approach does not
   attempt automatic detection on NFS (no watcher, no polling timer). This is
   acceptable because the user's workflow naturally involves visiting the
   metadata page after adding ROMs via scp. The "Update" button triggers
   `enrich_system_cache()` for all systems, which runs `auto_match_metadata()`
   — the exact code path that detects and matches new ROMs. The 300-second
   hard TTL remains as a safety net for page-level cache freshness (e.g., if
   the user navigates to a system page directly instead of using the metadata
   page).

3. **Concurrent access**: `metadata_operation_in_progress` atomic prevents the
   watcher-triggered rescan from running during a metadata import (which takes
   the DB exclusively). This is already the pattern used by
   `spawn_cache_verification`.

4. **Race with import**: If a metadata import is running when new ROMs are added,
   the watcher-triggered rescan is skipped. The `spawn_cache_enrichment` call
   at the end of import will catch up, since it runs `enrich_system_cache` for
   all systems, which includes `auto_match_metadata`.

5. **Large ROM collections**: Iterating all systems' mtimes is cheap (one `stat`
   per system). Only systems with changed mtimes trigger L3 scans. A typical
   setup has ~40 systems, so this is ~40 stat calls per watcher event or
   manual update trigger.

### Effort Estimate

| Change | Effort |
|--------|--------|
| Filesystem watcher for local storage | 2-3 hours |
| New system directory detection | 1 hour |
| Update box_art_path on download (optional) | 1 hour |
| Testing (local + Pi + NFS) | 2-3 hours |
| **Total** | **6-8 hours** |

---

## Feature 2: Image Repo Auto-Cleanup

### Goal

Delete orphaned thumbnail files on disk for ROMs that have been removed, to
reclaim disk space (especially important on Pi with limited storage).

### Current Behavior

#### Where thumbnails are stored

Thumbnails live at `<storage>/.replay-control/media/<system>/<kind>/<name>.png`:

```
.replay-control/media/
  nintendo_snes/
    boxart/
      Super Mario World (USA).png       # ~50-150 KB each
      Donkey Kong Country (USA).png
    snap/
      Super Mario World (USA).png
      Donkey Kong Country (USA).png
  sega_smd/
    boxart/
      Sonic The Hedgehog (USA, Europe).png
    snap/
      ...
```

**Size**: Typical boxart images are 50-150 KB (PNG). A full collection with ~40
systems and ~2000 matched ROMs uses 200 MB - 2 GB of thumbnail storage
(documented in `docs/reference/replay-control-folder.md`, lines 126-131).

#### How thumbnails are referenced

Thumbnails are referenced in two places:

1. **`game_metadata.box_art_path`** and **`game_metadata.screenshot_path`**:
   Relative paths like `"boxart/Super Mario World (USA).png"`. Set by
   `update_image_paths_from_disk` (import.rs:736-894) and `bulk_update_image_paths`
   (metadata_db.rs:643-691).

2. **`game_library.box_art_url`**: Full URL paths like
   `"/media/nintendo_snes/boxart/Super Mario World (USA).png"`. Set by
   `enrich_system_cache` -> `resolve_box_art`.

#### What happens when a ROM is removed

When a user deletes a ROM via the app's delete feature:

1. The ROM file is removed from disk.
2. `cache.invalidate_system(system)` clears L1 + L2 for that system
   (cache.rs:975-988), which deletes the `game_library` row.
3. The `game_metadata` row is NOT deleted (it stays orphaned in the DB).
4. The thumbnail files on disk are NOT deleted.
5. The `ImageIndex` is not invalidated (but will refresh on next access).

When a user deletes a ROM externally (scp, file manager):

1. The ROM file is removed.
2. On next cache invalidation (mtime change detected), the L3 scan produces a
   new ROM list that excludes the deleted ROM.
3. `save_system_entries` (metadata_db.rs:768-835) does `DELETE FROM game_library WHERE
   system = ?1` followed by re-insert of all current ROMs. This effectively
   removes the deleted ROM's `game_library` entry.
4. But again, `game_metadata` rows and thumbnail files remain orphaned.

#### How `update_image_paths_from_disk` works (import.rs:736-894)

This function scans the media directory and fuzzy-matches thumbnail filenames
against `visible_filenames` (ROMs in `game_library`). It updates
`game_metadata.box_art_path` and `game_metadata.screenshot_path`. It does NOT
delete orphaned files -- it only writes paths into the DB.

The function `visible_filenames` (metadata_db.rs:1039-1048) queries
`game_library` for filenames. If a ROM has been removed from `game_library`, its
thumbnail won't be matched by `update_image_paths_from_disk`, but the file
remains on disk.

### Where the Gaps Are

#### Gap 1: No cleanup of orphaned thumbnail files

When a ROM is removed (by any mechanism), the corresponding thumbnail files
(boxart + snap) remain on disk indefinitely. Over time, as the user adds and
removes ROMs, orphaned thumbnails accumulate.

The only way to clean them up today is:
- "Clear Images" on the metadata page (deletes ALL thumbnails, not just orphans)
- Manual `rm` of files

#### Gap 2: No cleanup of orphaned game_metadata rows

When a ROM is removed, its `game_metadata` row stays in the DB. This doesn't
waste much space (a few hundred bytes per entry), but it inflates metadata
coverage stats.

#### Gap 3: Thumbnails are named after the libretro display name, not the ROM filename

A key complexity: thumbnail filenames follow the libretro-thumbnails naming
convention, which often differs from the ROM filename:

- ROM: `sf2.zip` (arcade)
- Thumbnail: `Street Fighter II_ The World Warrior (World 920615).png`

The mapping between ROM filename and thumbnail filename goes through multiple
fuzzy matching tiers. To identify orphaned thumbnails, we need to:
1. List all thumbnail files on disk for a system.
2. For each file, check if any current ROM would match it via the fuzzy pipeline.
3. If no ROM matches, the file is orphaned.

This is the reverse of the normal lookup (ROM -> thumbnail). It's more expensive
because it requires iterating all files and running the fuzzy match pipeline in
reverse.

### Proposed Implementation

#### Approach: Post-rescan orphan sweep

After a cache rescan detects that ROMs have been removed from a system (the
new ROM count is less than the previous count), run an orphan sweep for that
system.

**File**: `replay-control-app/src/api/cache.rs` (new method on `GameLibrary`)
**File**: `replay-control-core/src/metadata/thumbnails.rs` (new function)

##### Step 1: Identify orphaned thumbnails (core crate)

New function in `thumbnails.rs`:

```rust
pub fn find_orphaned_thumbnails(
    storage_root: &Path,
    system: &str,
    active_rom_filenames: &[String],
) -> Vec<PathBuf>
```

This function:
1. Lists all `.png` files in `media/<system>/boxart/` and `media/<system>/snap/`.
2. Builds a set of "active" thumbnail stems from `active_rom_filenames` using the
   same fuzzy matching pipeline (`thumbnail_filename`, `base_title`,
   `strip_version`, arcade display name translation).
3. For each file on disk, checks if its stem (or base_title, or version-stripped
   form) appears in the active set.
4. Returns the paths of files that are NOT matched by any active ROM.

For arcade systems, we need to handle the reverse mapping: a thumbnail named
`"Street Fighter II_ The World Warrior (World 920615).png"` should be
considered active if `sf2.zip` is in the ROM list. This requires:
- Building a set of all possible thumbnail stems from each ROM filename
- Including the arcade display name translation for arcade systems
- Including colon variants

**Estimated lines**: ~80-100 lines in `thumbnails.rs`.

##### Step 2: Delete orphaned files

New function in `thumbnails.rs`:

```rust
pub fn delete_orphaned_thumbnails(
    storage_root: &Path,
    system: &str,
    active_rom_filenames: &[String],
) -> Result<usize>
```

Calls `find_orphaned_thumbnails`, then deletes each orphaned file. Returns the
count of deleted files.

**Estimated lines**: ~15 lines.

##### Step 3: Clean up orphaned game_metadata rows

New function in `metadata_db.rs`:

```rust
pub fn delete_orphaned_metadata(&mut self, system: &str) -> Result<usize>
```

```sql
DELETE FROM game_metadata
WHERE system = ?1
  AND rom_filename NOT IN (
    SELECT rom_filename FROM game_library WHERE system = ?1
  )
```

This deletes `game_metadata` rows for ROMs that no longer exist in `game_library`
for a given system. Should be called after `save_system_entries` completes for a
stale system.

**Estimated lines**: ~15 lines.

##### Step 4: Trigger cleanup on ROM removal detection

In the periodic background check (from Feature 1), or in the startup
verification, after rescanning a stale system:

**File**: `replay-control-app/src/api/background.rs`

```rust
// After get_roms + enrich_system_cache for a stale system:
let new_count = roms.len();
let old_count = meta.rom_count;
if new_count < old_count {
    // ROMs were removed -- clean up orphans.
    let filenames: Vec<String> = roms.iter()
        .map(|r| r.game.rom_filename.clone())
        .collect();
    let deleted = thumbnails::delete_orphaned_thumbnails(
        &storage.root, &meta.system, &filenames
    );
    if let Ok(n) = deleted && n > 0 {
        tracing::info!("Cleaned up {n} orphaned thumbnails for {}", meta.system);
    }
    // Also clean up game_metadata.
    state.cache.with_db_mut(&storage, |db| {
        let _ = db.delete_orphaned_metadata(&meta.system);
    });
}
```

**Estimated lines**: ~20 lines in the background check.

##### Step 5 (optional): Manual cleanup button in UI

Add a "Clean Up Orphans" button on the metadata management page. This would
iterate all systems and run the orphan sweep. Useful for one-time cleanup of
existing collections.

### Edge Cases

1. **Shared thumbnails**: Multiple ROMs can match the same thumbnail (e.g.,
   different region variants of the same game). The orphan detection must check
   ALL active ROMs before marking a thumbnail as orphaned. The proposed approach
   handles this correctly because it builds a set of all active thumbnail stems
   first, then checks each file against the entire set.

2. **Arcade multi-mapping**: Arcade systems have extra complexity because:
   - Multiple MAME codenames can map to the same display name (clones)
   - The thumbnail is named after the display name, not the codename
   - A single thumbnail file may serve multiple ROM files (parent + clones)

   The orphan detector must translate each active ROM's MAME codename to a
   display name via `arcade_db::lookup_arcade_game`, then add the normalized
   thumbnail filename to the active set.

3. **Fuzzy match edge cases**: A thumbnail matched via tier 3 (version-stripped)
   might be considered orphaned if the orphan detector only checks tiers 1-2. The
   detector must use the same full fuzzy pipeline as `resolve_box_art`.

4. **NFS latency**: Listing and deleting files on NFS can be slow. The cleanup
   should run in a background thread and not block request handling.

5. **Concurrent writes**: If a thumbnail download is in progress while cleanup
   runs, we could delete a file that was just downloaded. Mitigation: run cleanup
   only when `metadata_operation_in_progress` is false (same guard as the periodic
   check).

6. **User-overridden box art**: The `user_data.db` `box_art_overrides` table
   stores user-chosen region variants. The cleanup must check these overrides too:
   if a user has explicitly chosen a variant, that thumbnail file must not be
   deleted even if the ROM it was originally downloaded for has been removed.
   However, box art overrides reference files that are active variants of
   *existing* ROMs, so this should not be an issue unless the ROM itself was
   deleted.

7. **Disk space recovery on Pi**: The Pi uses an ext4 filesystem on SD/USB. File
   deletion takes effect immediately. No special handling needed for space
   reclamation.

### Space Savings Estimate

Typical thumbnail sizes:
- Boxart: 50-150 KB per image (average ~80 KB)
- Snap: 30-100 KB per image (average ~50 KB)

For a collection that has churned 500 ROMs (added then removed), orphaned
thumbnails would use approximately:
- 500 * 80 KB (boxart) + 500 * 50 KB (snap) = ~65 MB

On a Pi with a 16 GB SD card where storage is tight, this is meaningful. For
USB-mounted collections the savings are less critical but still worthwhile for
cleanliness.

### Effort Estimate

| Change | Effort |
|--------|--------|
| `find_orphaned_thumbnails` implementation | 2-3 hours |
| `delete_orphaned_metadata` DB function | 0.5 hours |
| Background trigger (in periodic check) | 1 hour |
| Testing (local + Pi, arcade + console) | 2 hours |
| UI cleanup button (optional) | 1-2 hours |
| **Total** | **6.5-8.5 hours** |

---

## Combined Implementation Plan

Both features are closely related and should be implemented together, since the
periodic background check (Feature 1) is the natural place to trigger the orphan
cleanup (Feature 2).

### Phase 1: ROM change detection (Feature 1, core) — **DONE** (2026-03-14)

Implemented in commit `5bec806`:

1. `StorageKind::is_local()` method added to `storage.rs` — returns true for
   `Sd`, `Usb`, `Nvme`, false for `Nfs`.
2. `spawn_rom_watcher()` + `try_start_rom_watcher()` in `background.rs` —
   recursive `notify` watcher on `roms/`, 3-second debounce, extracts
   affected system names from event paths, invalidates cache + re-enriches.
3. New system directory detection: when `roms/` top-level changes, refreshes
   systems list and enriches newly discovered systems.
4. NFS skipped entirely — manual "Update" from metadata page is sufficient.
5. Wired up in `main.rs` startup sequence.

### Phase 2: Orphan identification and cleanup (Feature 2, core)

5. Add `find_orphaned_thumbnails` and `delete_orphaned_thumbnails` to
   `thumbnails.rs`.
6. Add `delete_orphaned_metadata` to `metadata_db.rs`.

### Phase 3: Integration (both features)

7. In `check_stale_systems`, after rescanning a system with fewer ROMs than
   before, trigger orphan cleanup.
8. Update `spawn_cache_verification` to use the same `check_stale_systems` logic
   (dedup the startup path).

### Phase 4: Testing

9. Test on Pi with USB storage: add/remove ROMs, verify watcher fires and
   detection is near-instant.
10. Test with local NFS mount: add ROMs via scp, verify watcher is **not**
    started for NFS. Use the metadata page "Update" button, confirm new ROMs
    are detected and matched.
11. Test removal: delete ROMs via scp, verify orphaned thumbnails are cleaned up
    after watcher fires (local) or manual update (NFS).
12. Test watcher failure gracefully: simulate by making `roms/` unreadable
    briefly, confirm the app continues to work and manual updates still function.
13. Test edge cases: arcade systems, partial copies, new system directories.

### Total Effort: 12-16 hours

---

## Key Source Files

| File | Role |
|------|------|
| `replay-control-app/src/api/background.rs` | Startup verification, periodic watcher, auto-import |
| `replay-control-app/src/api/cache.rs` | GameLibrary, get_roms, enrich_system_cache, auto_match_metadata, resolve_box_art, queue_on_demand_download |
| `replay-control-app/src/api/import.rs` | Metadata import, thumbnail update pipeline, update_image_paths_from_disk |
| `replay-control-app/src/api/mod.rs` | AppState, refresh_storage |
| `replay-control-core/src/platform/storage.rs` | StorageLocation, StorageKind (Sd/Usb/Nvme/Nfs), used for watcher vs poll branching |
| `replay-control-core/src/metadata/metadata_db.rs` | game_metadata table, game_library table, save_system_entries, bulk_update_image_paths |
| `replay-control-core/src/metadata/thumbnails.rs` | Thumbnail naming, fuzzy matching (base_title, strip_version, strip_tags), media_dir_size, clear_media |
| `replay-control-core/src/metadata/thumbnail_manifest.rs` | Manifest index, on-demand download, save_thumbnail |
| `replay-control-core/src/metadata/launchbox.rs` | LaunchBox XML import, build_rom_index, normalize_title |
| `replay-control-app/src/main.rs` | Startup sequence (lines 64-71) |
| `docs/reference/replay-control-folder.md` | .replay-control folder structure |
| `docs/reference/thumbnail-new-roms-behavior.md` | Existing analysis of new-ROM detection behavior |
