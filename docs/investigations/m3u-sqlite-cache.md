# M3U Support with SQLite ROM Cache

> **Status**: Not yet implemented. M3U files are handled at the ROM scanning level (`roms.rs`) but not integrated with the SQLite ROM cache for disc-label display or parent game grouping.

Investigation date: 2026-03-12

## 1. Current M3U Handling in `list_roms()`

**File:** `replay-control-core/src/roms.rs`

### Flow

1. `collect_roms_recursive()` walks the system directory tree, collecting all ROM files.
   M3U files are always accepted by `is_rom_file()` regardless of the system's extensions list
   (line 514-516: hardcoded `"m3u"` check before the system extensions check).

2. Each collected ROM gets an `is_m3u: bool` flag based on its file extension.

3. After collection, `apply_m3u_dedup(&mut roms, &roms_root)` runs:
   - Iterates all M3U entries, calling `parse_m3u_references()` on each to get referenced filenames.
   - Builds a `HashMap<String, Vec<usize>>` mapping lowercased referenced filenames to M3U indices.
   - ScummVM special case: if a reference ends in `.svm`, also adds the `.scummvm` variant (and vice versa).
   - Walks all non-M3U ROM entries; if a ROM's lowercased filename matches a reference, it's marked for removal.
   - Removed disc files' sizes are aggregated into the corresponding M3U entry's `size_bytes`.
   - Finally, `roms.retain()` strips out all referenced disc files.

4. The same dedup logic exists in `count_roms_inner()` (used by `scan_systems()`) but implemented
   differently: it operates per-directory during the recursive walk, passing M3U references
   down to subdirectories via `parent_m3u_refs`.

### `parse_m3u_references()`

- Reads at most 8192 bytes via `BufReader` + `.take(MAX_M3U_BYTES)`.
- Iterates lines; skips comments (`#`) and blank lines.
- Stops parsing on non-UTF-8 data (binary content) or lines that fail `looks_like_filename()`.
- Extracts just the filename component from potentially absolute paths (handles ScummVM style).
- `looks_like_filename()`: requires a dot, length < 512, no control chars except tab.

## 2. M3U Files in `game_library`

**File:** `replay-control-core/src/metadata_db.rs`

### Schema

```sql
CREATE TABLE IF NOT EXISTS game_library (
    system TEXT NOT NULL,
    rom_filename TEXT NOT NULL,
    rom_path TEXT NOT NULL,
    display_name TEXT,
    size_bytes INTEGER NOT NULL DEFAULT 0,
    is_m3u INTEGER NOT NULL DEFAULT 0,  -- boolean flag
    box_art_url TEXT,
    driver_status TEXT,
    genre TEXT,
    players INTEGER,
    rating REAL,
    PRIMARY KEY (system, rom_filename)
);
```

The `is_m3u` column is stored as an integer (0/1). `GameEntry` has `is_m3u: bool`.

### Dedup Timing: Before Caching

The dedup logic is applied **before** data reaches the cache. The flow is:

1. **L3 (filesystem scan):** `list_roms()` calls `collect_roms_recursive()` then `apply_m3u_dedup()`.
   The resulting `Vec<RomEntry>` already has disc files removed and M3U sizes aggregated.

2. **L2 (SQLite write-through):** `save_roms_to_db()` in `cache.rs` converts each post-dedup
   `RomEntry` into a `GameEntry` and calls `db.save_system_entries()`. The SQLite table only stores
   the deduplicated list.

3. **L2 (SQLite read):** `load_roms_from_db()` reads `GameEntry` entries and converts back to
   `RomEntry`. Since only M3U entries (not their referenced discs) were stored, the loaded
   data is already deduplicated.

This means: **disc files referenced by M3U playlists never appear in `game_library`**. Only the
M3U entry itself is stored, with its `size_bytes` already reflecting the aggregate of itself
plus all referenced disc files.

## 3. Edge Cases Where M3U Dedup Could Fail

### 3a. M3U Pointing to Non-Existent Files

**Not a problem for dedup.** The dedup works by matching filenames in the M3U against filenames
found on disk. If an M3U references `Game (Disk 2).dim` but that file doesn't exist, the
reference simply won't match any collected ROM -- the M3U entry is still shown but no disc
file is incorrectly hidden.

However, the user sees a game they can't actually play (missing disc files). The app does not
currently validate M3U references or warn about broken playlists.

### 3b. M3U References in Different Directories

The dedup in `apply_m3u_dedup()` matches by **filename only** (not by path). This means:

- If `sharp_x68k/Game.m3u` references `Game.dim`, and `Game.dim` exists in a subdirectory
  `sharp_x68k/subfolder/Game.dim`, the dedup **will** hide it because the lowercased filename
  matches.
- Conversely, if two different subdirectories both have a `Disc1.chd` file and only one M3U
  references it, **both** copies would be hidden because the match is filename-only.

This is a potential issue for systems with deeply nested folder structures, but in practice
X68000 and PS1 collections are usually flat.

The `count_roms_inner()` variant (used by `scan_systems()`) is slightly different: it passes
references down to subdirectories, which is correct for ScummVM but means the dedup behavior
diverges between counting and listing. However, the actual effect is the same for typical
layouts.

### 3c. Binary M3U Files (X68000 Specific)

X68000 has a known pattern where `.m3u` files can be binary (disc image data appended after the
filename lines). The parser handles this via:

- `MAX_M3U_BYTES = 8192`: limits reading.
- `BufReader::lines()` stops on non-UTF-8 data.
- `looks_like_filename()` rejects garbage lines.

This is well-tested (see `parse_m3u_binary_stops_at_non_text` test).

### 3d. Case Sensitivity

All comparisons are lowercased (`to_lowercase()`), so `Game.DIM` in the M3U will match
`game.dim` on disk. This is correct and tested (`m3u_dedup_case_insensitive`).

### 3e. M3U Referencing Another M3U

If an M3U references another M3U file, the referenced M3U would be hidden. This is unlikely
in practice but could cause confusion.

## 4. Sharp X68000 Specifics

### System Config

```rust
System {
    folder_name: "sharp_x68k",
    display_name: "Sharp X68000",
    manufacturer: "Sharp",
    category: SystemCategory::Computer,
    extensions: &["dim", "hdf", "m3u"],
}
```

X68000 is the **only** system that explicitly lists `m3u` in its extensions. For all other
systems, M3U files are accepted by the hardcoded check in `is_rom_file()`. The explicit
listing is redundant but harmless.

### X68000 M3U Patterns

X68000 games heavily use M3U playlists because many games span multiple floppy disks (`.dim`
files). A typical layout:

```
sharp_x68k/
  Game.m3u              (text: "Game (Disk 1).dim\nGame (Disk 2).dim\n")
  Game (Disk 1).dim     (binary floppy image ~1.2MB)
  Game (Disk 2).dim     (binary floppy image ~1.2MB)
```

Some X68000 M3U files are also "binary M3U" files -- they contain a filename on the first line
followed by raw disc data. The parser handles this correctly.

### Known Issues for X68000

1. **Display name derivation**: `GameRef::new()` strips the `.m3u` extension and tags to produce
   a display name. For X68000 M3U files, the display name will be the M3U stem (e.g., "Game"),
   which is usually a reasonable title. No special handling needed.

2. **Thumbnail matching**: Thumbnails are matched by the ROM filename stem. Since the M3U file
   typically has the game's title as its filename, thumbnail matching should work. The `thumbnails.rs`
   module does not have M3U-specific logic, but `thumbnail_filename()` just strips the extension
   and normalizes -- this works for M3U filenames.

3. **No broken M3U detection**: If a user has M3U files without corresponding disc images (e.g.,
   after partial deletion), the M3U still appears as a game entry but would fail to launch.

## 5. Mtime Invalidation and M3U Files

### How It Works

The cache uses **directory mtime** for invalidation:

- **L1 (in-memory):** `CacheEntry.dir_mtime` stores the system directory's mtime at cache time.
  On access, it compares with the current mtime. Stale entries fall through.
  Hard TTL of 300 seconds as a fallback.

- **L2 (SQLite):** `game_library_meta.dir_mtime_secs` stores the directory mtime as Unix timestamp.
  On load, `load_roms_from_db()` compares stored mtime with current. Mismatch triggers L3 rescan.

### M3U-Specific Mtime Concerns

**Adding/removing M3U files:** This changes the directory's mtime, so the cache correctly
invalidates. Works correctly.

**Adding/removing disc files referenced by an M3U:** This also changes the directory mtime
(adding/removing a file in a directory updates its mtime). Works correctly.

**Editing an M3U file's contents** (e.g., adding/removing a disc reference): This does **not**
change the directory's mtime -- only the file's own mtime changes. The cache would serve stale
data until the hard TTL (300s) expires.

This is a real but minor edge case: users rarely edit M3U files directly. And when they do, the
5-minute hard TTL provides an eventual consistency guarantee.

**Subdirectory changes:** If disc files are in subdirectories, changes there don't update the
parent directory's mtime. However, the mtime check is on the system directory (e.g.,
`roms/sharp_x68k/`), and most M3U-using systems have flat layouts.

**The `invalidate()` and `invalidate_system()` methods** in `GameLibrary` clear both L1 and L2
caches entirely. These are called after explicit user actions (delete, rename, upload), so those
operations bypass the mtime issue entirely.

## 6. Potential Improvements

### 6a. Store Disc File References in SQLite

Currently, disc files referenced by M3U playlists are stripped before caching. If the M3U
references were stored in a separate table, the app could:

- Validate M3U integrity (detect broken references).
- Show disc count in the UI ("3 discs").
- Support operations on individual discs (e.g., replace a bad disc image).
- Avoid re-parsing M3U files when restoring from cache.

Possible schema:

```sql
CREATE TABLE IF NOT EXISTS m3u_references (
    system TEXT NOT NULL,
    m3u_filename TEXT NOT NULL,
    disc_filename TEXT NOT NULL,
    disc_index INTEGER NOT NULL,
    PRIMARY KEY (system, m3u_filename, disc_filename),
    FOREIGN KEY (system, m3u_filename) REFERENCES game_library(system, rom_filename)
);
```

### 6b. M3U Content Hash for Smarter Invalidation

Store a hash of the M3U file's text content alongside the cached entry. On cache load, re-hash
the M3U file and compare. This would catch edits to M3U files without waiting for the hard TTL.
Trade-off: requires reading M3U files on every cache validation, but they're tiny (< 1KB text).

### 6c. Broken M3U Detection and UI Warning

After loading from cache (or during L3 scan), validate that all M3U references resolve to
existing files. Surface broken M3Us in the UI (e.g., a warning icon, or a "health check" page).

### 6d. Consistent Dedup Between `count_roms_inner` and `apply_m3u_dedup`

The two dedup implementations diverge slightly:

- `count_roms_inner()` (for `scan_systems`) processes directories recursively, passing M3U
  references downward to subdirectories. It never actually parses M3U files outside the
  current directory scope.
- `apply_m3u_dedup()` (for `list_roms`) operates on the flat collected list globally,
  matching by filename across all directories.

In the common case (flat directory structure), these behave identically. But for nested layouts,
they could produce different counts vs. listing results. Unifying the logic would eliminate this
theoretical inconsistency.

### 6e. Aggregate Disc File Sizes at Cache Level

Currently, `size_bytes` in `game_library` stores the pre-aggregated M3U size (M3U file size + sum
of referenced disc sizes). This is correct but means the original disc sizes are lost. If disc
references were stored separately (6a), sizes could be reconstructed without re-scanning.

### 6f. M3U-Aware Metadata Matching

When matching game metadata (LaunchBox, etc.), M3U filenames are used as the lookup key.
If the M3U filename doesn't match the metadata database's naming convention, the match fails.
A fallback that also tries the first disc filename from the M3U could improve match rates,
especially for PS1 multi-disc games where metadata databases often use the disc 1 filename.

## Summary

The current M3U implementation is solid and well-tested for the common cases (X68000 binary
M3U, ScummVM wrapper M3U, multi-disc PS1/SegaCD). The SQLite cache stores post-dedup data,
so M3U handling is transparent to the cache layer. The main gaps are:

1. **No broken M3U detection** -- silently shows unplayable entries.
2. **Mtime blind spot** -- editing M3U contents without adding/removing files is not detected
   until the 300s hard TTL expires.
3. **Filename-only matching** -- could theoretically hide wrong files in deeply nested layouts.
4. **Lost disc metadata** -- referenced disc filenames/sizes are not preserved in the cache.

None of these are urgent; they're opportunities for incremental improvement if M3U-heavy
systems become a larger part of the user base.
