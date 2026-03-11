# M3U (Multi-Disc / Playlist) Support Analysis

## Overview

M3U files serve as playlist/entry-point files for multi-disc games and multi-file
ROM sets across several systems supported by RePlayOS. This document analyzes how
the companion app currently handles M3U files, identifies gaps, and proposes
concrete improvements.

---

## 1. Current M3U Handling in the Codebase

### 1.1 ROM Scanning (`roms.rs`)

M3U detection happens at two levels:

**`is_rom_file()`** (line 262): M3U files are unconditionally accepted as valid
ROM files for *every* system, regardless of whether `.m3u` is in the system's
`extensions` list:

```rust
if ext_lower == "m3u" {
    return true;
}
```

**`collect_roms_recursive()`** (line 244): Each collected ROM gets an `is_m3u`
boolean flag:

```rust
let is_m3u = path
    .extension()
    .is_some_and(|ext| ext.eq_ignore_ascii_case("m3u"));
```

**`count_roms_recursive()`**: Used by `scan_systems()` for the home page system
summaries. Counts every file that passes `is_rom_file()`, meaning both M3U files
and their referenced disc files are counted individually. A 3-disc game with an
M3U appears as **4 games** in the count.

### 1.2 The `is_m3u` Field

The field propagates through the full stack:

| Layer | Type | Field |
|-------|------|-------|
| Core | `RomEntry` | `is_m3u: bool` |
| Server fns | `RomDetail` | `is_m3u: bool` |
| Client mirror | `types::RomEntry` | `is_m3u: bool` |

**Critical finding: `is_m3u` is never read by any component or server function
logic.** It is serialized to the client and included in `RomDetail`, but:

- The ROM list component (`rom_list.rs`) does not check `is_m3u` for filtering
  or display.
- The game detail page (`game_detail.rs`) does not reference `is_m3u` at all.
- The search page (`search.rs`) does not filter on it.
- No filter chip or toggle exists for hiding disc files.

The field was added with the intent to support M3U-aware behavior, but no
consumer code was ever written.

### 1.3 Sharp X68000 Special Case

Sharp X68000 (`sharp_x68k`) is the only system that includes `"m3u"` in its
`extensions` list (alongside `"dim"` and `"hdf"`). This means X68000 M3U files
are matched twice -- once by the extension list and once by the universal M3U
check. The result is the same (file is accepted), but it shows that M3U is a
first-class format for X68000 specifically.

### 1.4 ScummVM System Definition

ScummVM's extension list contains only `"scummvm"`, not `"svm"`. The M3U
universal check means `.m3u` files inside `roms/scummvm/` are accepted, but
`.svm` files inside subfolders are **not** matched. ScummVM games on RePlayOS
use a specific convention where the M3U at the root references an `.svm` file
inside a subfolder.

### 1.5 Double-Counting Problem

For any system where M3U files exist alongside the disc files they reference,
the current scanning produces inflated game counts:

| Scenario | Files on disk | Game count reported |
|----------|--------------|-------------------|
| Single-disc with M3U | `Game.dim` + `Game.m3u` | **2** (should be 1) |
| 3-disc with M3U | `Game (Disk 1).dim` + `Game (Disk 2).dim` + `Game (Disk 3).dim` + `Game.m3u` | **4** (should be 1) |
| 5-disc with M3U | 5x `.dim` + `Game.m3u` | **6** (should be 1) |

This affects:
- Home page total game count
- System summary game count
- Storage bar (sizes are correct per-file, but game count is misleading)
- ROM list showing duplicate entries

---

## 2. Real-World Data from NFS Mount

### 2.1 Systems with M3U Files

Two systems have M3U files on the NFS-mounted storage
(`<NFS_MOUNT>/`):

| System | M3U files | Game data files | Total storage |
|--------|-----------|----------------|--------------|
| `sharp_x68k` | 1,005 | 1,863 .dim + 295 .hdf | -- |
| `scummvm` | 134 | 105 .svm + 16 .scummvm + ~40K data files across 119 subfolders | ~35 GB |

Other systems that commonly use M3U (PlayStation, Dreamcast, Sega CD, IBM PC)
have empty ROM folders on this mount.

### 2.2 M3U File Structure (X68000)

X68000 M3U files are plain text (ASCII with CRLF line terminators) listing the
disc image filenames. References use **relative paths** (bare filenames in the
same directory).

**Single-disc game** (`15 Puzzle (1991)(Sygnas).m3u`):
```
15 Puzzle (1991)(Sygnas).dim
```

**Multi-disc game** (`Alshark.m3u`):
```
Alshark (1991)(Right Stuff)(Disk 1 of 5)(System).dim
Alshark (1991)(Right Stuff)(Disk 2 of 5)(Data).dim
Alshark (1991)(Right Stuff)(Disk 3 of 5)(Opening).dim
Alshark (1991)(Right Stuff)(Disk 4 of 5)(Visual).dim
Alshark (1991)(Right Stuff)(Disk 5 of 5)(Ending).dim
```

**3-disc game** (`4th Unit Act 2, The.m3u`):
```
4th Unit Act 2, The (1988)(Data West)(Disk 1 of 3)(Disk A).dim
4th Unit Act 2, The (1988)(Data West)(Disk 2 of 3)(Disk B).dim
4th Unit Act 2, The (1988)(Data West)(Disk 3 of 3)(Disk C).dim
```

### 2.3 M3U File Structure (ScummVM)

ScummVM M3U files use **absolute paths** starting with `/media/nfs/roms/scummvm/`
(the Pi-side mount point). Each M3U references exactly **one** `.svm` or
`.scummvm` file inside a game subfolder. The M3U is a pure entry point, not a
multi-disc playlist.

**Typical structure:**
```
roms/scummvm/
  Grim Fandango (CD Spanish)/
    Grim Fandango (CD Spanish).svm     # Contains ScummVM engine ID: "grim"
    DATA000.LAB                         # ~29 MB
    DATA001.LAB                         # ~116 MB
    DATA002.LAB                         # ~115 MB
    [more game data files]
  Grim Fandango (CD Spanish).m3u        # Entry point (single line)
```

**M3U content** (single line, absolute path):
```
/media/nfs/roms/scummvm/Grim Fandango (CD Spanish)/Grim Fandango (CD Spanish).svm
```

**`.svm` / `.scummvm` file content** -- a single-line ScummVM game engine ID:
```
grim
```

Other examples of engine IDs: `toltecs`, `amazon`, `bladerunner`, `sword1`,
`sky-1`, `darkseed-cd-es`, `gob1-cd-es`.

### 2.4 ScummVM M3U Breakdown

| Metric | Count |
|--------|-------|
| M3U files at root level | 134 |
| Game subfolders | 119 |
| .svm files inside subfolders | 105 |
| .scummvm files inside subfolders | 16 |
| M3U referencing .svm | 117 |
| M3U referencing .scummvm | 17 |
| M3U with missing game data folder | 17 |
| M3U with present game data folder | 117 |
| Orphan folders (encoding mismatch) | 2 |
| Non-ROM files at root (clues/PDFs/TXTs) | 12 |

**All 134 M3U files contain exactly one line** (the referenced .svm/.scummvm
path). No ScummVM M3U is multi-line or contains comments.

**File sizes** are uniformly small: average 112 bytes, total 15 KB for all 134
M3U files.

**17 M3U files reference game data that is not yet present on the NFS share**
(the subfolder does not exist). These are placeholder entries for games still to
be added. They will show in the ROM list as broken entries since the referenced
.svm target does not exist.

**2 folders appear orphaned** due to UTF-8 encoding mismatches between the M3U
filename and the filesystem directory name (e.g., `1 1/2 Ritter` with different
representations of the "1/2" character). The M3U file does exist, but the path
inside it uses a different encoding than the actual directory on disk.

### 2.5 ScummVM Naming Patterns

The M3U filenames follow a consistent convention:

```
Game Title (Media Platform Language).m3u
```

Tags observed in filenames:

| Tag | Occurrences | Example |
|-----|-------------|---------|
| Spanish | 104 | `Full Throttle (CD DOS Spanish).m3u` |
| DOS | 76 | `Codename Iceman (DOS Spanish).m3u` |
| CD | 64 | `Blade Runner (CD Spanish).m3u` |
| Windows | 8 | `Dirty Split (Windows, Spanish).m3u` |
| Floppy | 4 | `EcoQuest 2 (Floppy DOS Spanish).m3u` |
| SCI | 4+1 | `Police Quest I (SCI, DOS Spanish).m3u` |
| FM-Towns | 3 | `Zak McKracken (FM-Towns).m3u` |
| Multi-lingual | 1 | `Broken Sword 2,5 (Multi-lingual).m3u` |
| Portuguese | 1 | `Croustibat (DOS, Portuguese).m3u` |
| AGS | 1 | `Maniac Mansion Deluxe (AGS).m3u` |

The collection is predominantly Spanish-language translations (104/134).

**M3U name vs subfolder name mismatches** occur in 21 out of 134 entries. The
M3U filename and the subfolder it references often differ slightly:

| M3U stem | Subfolder name |
|----------|---------------|
| `Chewy - Esc From F5` | `Chewy - Esc From F5 (CD Spanish)` |
| `Cruise for a Corpse (DOS Spanish)` | `Cruise for a Corpse (256 DOS, Spanish)` |
| `Sfinx (CD)` | `Sfinx (Dos Spanish)` |
| `King's Quest II` | `King's Quest II (Spanish)` |

This means the M3U filename cannot be trivially derived from the subfolder name
or vice versa -- each is independently named.

### 2.6 ScummVM Non-ROM Files

12 non-ROM files exist at the ScummVM root level alongside the M3U files:

- Copy protection hints/clues: `.TXT`, `.txt`, `.jpg`, `.JPG`, `.pdf` files
  (e.g., `Bargon Attack - Claves.TXT`, `Laura Bow II - Claves.pdf`)

These are documentation files the user has placed alongside games. The app's
`is_rom_file()` check correctly ignores them since they have no recognized
extension and are not `.m3u` files.

### 2.7 X68000 M3U Breakdown

Of the first 100 X68000 M3U files sampled:
- **62** reference a single disc (single-disc entry point)
- **38** reference multiple discs (true multi-disc playlists)

### 2.8 File Sizes

**X68000 M3U files** vary dramatically in size:

| File | Size |
|------|------|
| `Alshark.m3u` (text playlist, 5 discs) | 269 bytes |
| `15 Puzzle (1991)(Sygnas).m3u` (single disc) | 1,261,568 bytes (~1.2 MB) |
| `15 Puzzle (1991)(Sygnas).dim` (the disc image) | 1,261,824 bytes (~1.2 MB) |

The large M3U files appear to be a special X68000 format where the M3U contains
the disc filename on line 1 followed by binary data (effectively embedding the
disk image). The `file` command identifies them as "ISO-8859 text, with very long
lines" because the binary content happens to be parseable as a very long text
line.

**ScummVM M3U files** are uniformly tiny: 60-200 bytes each (just an absolute
path string). Total for all 134 files: ~15 KB.

### 2.9 Systems That Commonly Use M3U (Not Present on This Mount)

For reference, these systems typically use M3U in the broader RePlayOS/RetroArch
ecosystem:

| System | M3U purpose | Disc format |
|--------|------------|------------|
| PlayStation (`sony_psx`) | Multi-disc games (Final Fantasy VII = 3 discs) | `.chd`, `.cue`/`.bin`, `.pbp` |
| Sega CD (`sega_cd`) | Multi-disc games | `.chd`, `.cue` |
| Sega Dreamcast (`sega_dc`) | Some multi-disc games | `.gdi`, `.chd`, `.cdi` |
| Sega Saturn (`sega_st`) | Multi-disc games | `.chd`, `.cue` |
| PC Engine CD (`nec_pcecd`) | Multi-disc games | `.cue`, `.chd` |
| 3DO (`panasonic_3do`) | Multi-disc games | `.chd`, `.cue` |
| Neo Geo CD (`snk_ngcd`) | Multi-disc games | `.chd`, `.cue` |
| IBM PC / DOSBox (`ibm_pc`) | Multi-disc DOS games | Various |

---

## 3. Game Visualization

### 3.1 Current Behavior

All ROM files -- M3U and individual disc files -- appear as separate entries in
the ROM list. A 3-disc PlayStation game would show:

```
Final Fantasy VII (Disc 1) (USA).chd    600 MB    .chd
Final Fantasy VII (Disc 2) (USA).chd    600 MB    .chd
Final Fantasy VII (Disc 3) (USA).chd    600 MB    .chd
Final Fantasy VII (USA).m3u             120 B     .m3u
```

All four entries are independently searchable, favoritable, and displayed with
their own box art lookup (which will fail for the M3U since libretro-thumbnails
won't have an entry named `Final Fantasy VII (USA).m3u`).

### 3.2 Desired Behavior

When an M3U file exists:
- The M3U should be the **only** entry shown in the ROM list
- Individual disc files referenced by the M3U should be **hidden**
- The display name should derive from the M3U filename
- The displayed file size should be the **aggregate** of all referenced disc files
  (not the tiny M3U text file itself)

### 3.3 Name Derivation

The M3U filename typically provides a cleaner game name than individual disc
files:

| M3U filename | Disc filenames |
|-------------|---------------|
| `Final Fantasy VII (USA).m3u` | `Final Fantasy VII (Disc 1) (USA).chd` |
| `Alshark.m3u` | `Alshark (1991)(Right Stuff)(Disk 1 of 5)(System).dim` |
| `4th Unit Act 2, The.m3u` | `4th Unit Act 2, The (1988)(Data West)(Disk 1 of 3)(Disk A).dim` |

The M3U name is almost always the base game name, making it a better source for
display name and thumbnail matching.

---

## 4. Search and Filtering

### 4.1 Current Behavior

Both M3U files and disc files appear in search results independently. Searching
for "Final Fantasy VII" would return 4 hits (3 discs + 1 M3U).

### 4.2 Proposed Behavior

- When M3U exists, only the M3U entry should appear in search results
- The search should still match against the base game name
- Disc numbers ("Disc 1", "Disc 2") should not pollute search results

### 4.3 Filter Toggle

A "Hide disc files" filter toggle is not recommended. Instead, disc files
referenced by an M3U should be **automatically** hidden. The user should not need
to manage this -- it should be the default behavior since RePlayOS itself treats
the M3U as the canonical entry point.

---

## 5. Storage Impact

### 5.1 Size Calculation Problem

The M3U file itself is tiny (typically < 1 KB for a text playlist). The actual
storage is in the referenced disc files. Currently:

- The M3U entry shows its own file size (e.g., 269 bytes for `Alshark.m3u`)
- Each disc file shows its own size independently
- The system total size is correct (sums all files) but the game count is wrong

### 5.2 Home Page Storage Bar

The storage bar uses `disk_usage()` from the OS, not ROM scanning, so it reports
correct total disk usage regardless of M3U handling. However, the "total games"
count on the home page comes from `scan_systems()` which double-counts.

### 5.3 Per-System Size

`SystemSummary.total_size_bytes` sums all files found by `count_roms_recursive()`.
This is correct for total size but the `game_count` is inflated.

### 5.4 X68000-Specific Size Anomaly

Some X68000 M3U files are nearly as large as the disc images they reference
(~1.2 MB). This means for single-disc X68000 games with embedded M3U files, the
storage is effectively doubled: the M3U file IS a copy of the disc data. Hiding
the disc file from the UI does not change the actual storage consumed.

---

## 6. Box Art and Image Matching

### 6.1 Current Thumbnail Matching

`thumbnail_filename()` normalizes a ROM stem for matching against
libretro-thumbnails. The matching pipeline:

1. Strip file extension to get the stem
2. Replace special characters (`&*/:\`<>?\\|"`) with `_`
3. Try exact match against repo files
4. Fuzzy: strip parenthesized tags, match
5. Fuzzy: strip version strings, match

### 6.2 M3U Matching Problems

**M3U filenames may differ from libretro-thumbnails naming:**

- libretro-thumbnails typically uses No-Intro or Redump naming conventions
- M3U filenames may use simplified names (e.g., `Alshark.m3u` vs the full
  Redump name `Alshark (1991)(Right Stuff)`)
- The fuzzy matching (strip-tags tier) should handle this, as it strips
  parenthesized content

**Individual disc files with "Disc N" tags:**

- `Final Fantasy VII (Disc 1) (USA)` -- the tag stripping would reduce this to
  `Final Fantasy VII`, which should match `Final Fantasy VII (USA)` in the
  thumbnails repo
- However, currently both the M3U and disc files attempt independent thumbnail
  lookups

### 6.3 ScummVM Matching

With 134 real ScummVM M3U files now on the NFS share, thumbnail matching
challenges are concrete:

**Games with English base titles** (e.g., `Full Throttle (CD DOS Spanish).m3u`):
stripping tags yields `Full Throttle`, which should match. Approximately 50-60
games fall in this category.

**Games with Spanish titles** (e.g., `La Pantera Rosa - Mision Peligrosa.m3u`,
`Los Archivos Secretos de Sherlock Holmes`): these have no English equivalent
in the filename and will not match English libretro-thumbnails entries.
Approximately 20-30 games are affected.

**Games with near-English titles plus minor differences** (e.g.,
`Indiana Jones y la ultima cruzada` vs `Indiana Jones and the Last Crusade`):
fuzzy matching cannot bridge language translations.

libretro-thumbnails has a `ScummVM` system folder that uses display names (not
engine IDs), so English-titled games should match after tag stripping. The
`.svm` engine ID (e.g., `grim`, `toltecs`) is not useful for libretro-thumbnails
matching but could be used for an alternative ScummVM-specific database.

---

## 7. ScummVM Specifics

### 7.1 Observed Structure (Real Data)

134 ScummVM games are present on the NFS share, organized as:

```
roms/scummvm/
  Grim Fandango (CD Spanish)/              # Game subfolder
    Grim Fandango (CD Spanish).svm         # Engine ID file: "grim"
    DATA000.LAB                            # Game data (~29 MB)
    DATA001.LAB                            # Game data (~116 MB)
    [more data files]
  Grim Fandango (CD Spanish).m3u           # Entry point (134 bytes)
  Bargon Attack - Claves.TXT               # User documentation (not a ROM)
```

Key structural observations from the 134-game collection:

- **119 subfolders** contain game data files (.svm/.scummvm + game assets)
- **134 M3U files** at the root level serve as entry points
- **105 .svm files** + **16 .scummvm files** = 121 engine config files in subfolders
- Each game folder ranges from a few MB to over 1.5 GB (Blade Runner)
- Total storage: ~35 GB across ~40,000 game data files

### 7.2 M3U as Entry Point (Not Multi-Disc)

For ScummVM, the M3U file serves a fundamentally different purpose than on
disc-based systems:

- It is the **game entry point** that makes the game appear in the RePlayOS menu
- It always references exactly **one** file (never multi-line)
- It uses **absolute paths** from the Pi's perspective (`/media/nfs/roms/scummvm/...`)
- The referenced file is a `.svm` (117 cases) or `.scummvm` (17 cases) file
  inside the game's subfolder

The `.svm` and `.scummvm` files are functionally identical -- both contain a
single-line ScummVM engine ID (e.g., `toltecs`, `sky-1`, `gob1-cd-es`). The
two extensions appear to come from different RePlayOS tooling versions or
manual creation.

### 7.3 Absolute vs Relative Path Difference

**X68000 M3U files** use relative filenames (just the .dim filename, files are
in the same directory):
```
Alshark (1991)(Right Stuff)(Disk 1 of 5)(System).dim
```

**ScummVM M3U files** use absolute paths rooted at the Pi mount point:
```
/media/nfs/roms/scummvm/Grim Fandango (CD Spanish)/Grim Fandango (CD Spanish).svm
```

This absolute path convention means:
- The M3U only works when the storage is mounted at `/media/nfs/` on the Pi
- If the mount point changes, all M3U files break
- The companion app (which accesses via NFS at a different mount point) cannot
  resolve these paths directly -- but since it only displays the M3U entry in
  the ROM list (not resolving the referenced file), this is not currently a
  problem for display purposes

**Implication for the `parse_m3u_references()` implementation:** when parsing
ScummVM M3U files to build the exclusion set, the parser will see an absolute
path rather than a bare filename. The referenced file is inside a subfolder
(not alongside the M3U), so it would never appear as a separate entry in the
ROM list anyway (the scanner processes `roms/scummvm/` at depth 1, not
recursing into subfolders for top-level M3U-referenced files). No exclusion
filtering is needed for ScummVM -- the structure is naturally correct.

### 7.4 Incomplete Games (Missing Data)

17 of the 134 M3U files reference subfolders that do not exist yet on the NFS
share. These are placeholder entries for games that are still being prepared.
They will appear in the ROM list but will fail to launch since the game data is
absent.

Notable missing games include:
- Dirty Split (Windows, Spanish)
- Dreamweb (CD DOS Spanish)
- Freddy Pharkas (CD DOS Spanish)
- Phantasmagoria 1 & 2 (CD Spanish)
- Zork: Grand Inquisitor (CD Spanish)

### 7.5 Current App Behavior

The ScummVM system definition only accepts `.scummvm` extension files:
```rust
extensions: &["scummvm"],
```

This means:
- `.svm` files are **not** matched by the extension check
- `.m3u` files **are** matched by the universal M3U check in `is_rom_file()`
- The M3U entry will appear in the ROM list
- The `.svm`/`.scummvm` files inside subfolders will **not** appear (they are
  not at the root scan level, and even if they were, `.svm` is not in the
  extensions list)

This is actually the correct behavior for ScummVM: the M3U is the only entry
that should be visible, and the `.svm`/`.scummvm` file should remain hidden
inside its subfolder. The system is accidentally well-behaved because:
1. The universal M3U check picks up the entry points
2. The game data lives in subfolders that the flat scanner does not recurse into
3. Neither `.svm` nor `.scummvm` needs to be in the extensions list

### 7.6 Double-Counting: Not a Problem for ScummVM

Unlike X68000, ScummVM does **not** suffer from double-counting because:
- The M3U files are at the root of `roms/scummvm/`
- The `.svm`/`.scummvm` files are inside subfolders
- The ROM scanner only finds the M3U files (the subfolder contents are not
  scanned as ROM files since the system does not recurse)
- Result: each game appears exactly once in the ROM list

The 12 non-ROM files (clues, PDFs) at the root are also correctly ignored.

### 7.7 ScummVM Thumbnail Matching Challenges

ScummVM M3U filenames (e.g., `Full Throttle (CD DOS Spanish).m3u`) present
specific challenges for thumbnail matching:

1. **Language-specific names**: many games use Spanish titles
   (`Los Archivos Secretos de Sherlock Holmes`, `La Pantera Rosa`) which
   will not match English-named thumbnails in libretro-thumbnails
2. **Platform tags**: `(CD DOS Spanish)`, `(FM-Towns)`, `(SE-Talkie Spanish)`
   are not standard No-Intro/Redump tags. The fuzzy matching strip-tags tier
   should remove these, leaving the base title
3. **English titles with Spanish tags**: `Full Throttle (CD DOS Spanish)` ->
   stripping tags yields `Full Throttle`, which should match
4. **Pure Spanish titles**: `La Pantera Rosa - Mision Peligrosa` has no
   English equivalent in the filename, so thumbnail matching will likely fail

The `.svm` engine ID (e.g., `grim`, `toltecs`) could theoretically be used as
a fallback lookup key against a ScummVM-specific thumbnail mapping, but this
would require reading the `.svm` file content during thumbnail resolution.

---

## 8. Proposed Improvements

### 8.1 Hide Disc Files When M3U Exists (Priority: High)

**Where:** `collect_roms_recursive()` in `roms.rs`

After collecting all files, do a second pass:

1. Identify all M3U files
2. Parse each M3U to extract referenced filenames (read lines, trim whitespace,
   ignore blank lines and lines starting with `#`)
3. Build a `HashSet<String>` of all referenced filenames
4. Filter out any `RomEntry` whose `rom_filename` is in the referenced set

**Also update** `count_roms_recursive()` with the same logic so that
`SystemSummary.game_count` is accurate.

**Caveats:**

- X68000 M3U files that embed binary data after the first line need careful
  parsing -- only extract filenames from text lines that look like valid
  filenames (contain a `.` and end with a known extension).
- ScummVM M3U files use absolute paths (`/media/nfs/roms/scummvm/Game/Game.svm`)
  rather than bare filenames. The parser should extract just the filename from
  the path. However, since ScummVM referenced files are in subfolders (not
  alongside other ROMs at the root level), they would never appear in the ROM
  list anyway -- so ScummVM M3U parsing for exclusion is unnecessary.

**Implementation sketch:**

```rust
fn parse_m3u_references(m3u_path: &Path) -> Vec<String> {
    let Ok(content) = std::fs::read_to_string(m3u_path) else {
        // Binary or unreadable -- try reading just the first line
        let Ok(bytes) = std::fs::read(m3u_path) else { return vec![] };
        let first_line = bytes.split(|&b| b == b'\n' || b == b'\r')
            .next()
            .and_then(|l| std::str::from_utf8(l).ok())
            .unwrap_or("");
        return if looks_like_filename(first_line) {
            vec![first_line.to_string()]
        } else {
            vec![]
        };
    };
    content.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter(|l| looks_like_filename(l))
        .map(|l| l.to_string())
        .collect()
}

fn looks_like_filename(s: &str) -> bool {
    s.contains('.') && s.len() < 300 && s.chars().all(|c| !c.is_control() || c == '\t')
}
```

### 8.2 Aggregate Storage Display for M3U Games (Priority: High)

**Where:** `collect_roms_recursive()` in `roms.rs`

When an M3U entry is kept and its referenced files are hidden, sum the sizes of
all referenced files and assign that total to the M3U's `RomEntry.size_bytes`.
This way the ROM list shows the true storage cost of the multi-disc game.

The M3U file's own size can be included in the sum (it may be negligible or, in
the X68000 case, significant).

### 8.3 Box Art Matching for M3U Games (Priority: Medium)

**Where:** `find_image_on_disk()` and `import_system_thumbnails()` in
`thumbnails.rs`

When resolving box art for an M3U file, strip the `.m3u` extension and use the
resulting stem for thumbnail lookup. This already happens naturally since
`find_image_on_disk()` strips the extension:

```rust
let stem = rom_filename.rfind('.').map(|i| &rom_filename[..i]).unwrap_or(rom_filename);
```

So `Alshark.m3u` becomes `Alshark`, which will fuzzy-match against
`Alshark (1991)(Right Stuff)` in the thumbnails repo. No code change needed for
the basic case.

For disc files that would otherwise be searched independently (before 8.1 hides
them), the disc numbers in parentheses (`(Disc 1)`) would be stripped by the
fuzzy matching tier, also producing correct matches. After implementing 8.1,
disc files won't be looked up at all.

### 8.4 ScummVM Extension Update (Priority: Low)

Add `"svm"` to the ScummVM system's `extensions` list so that `.svm` files are
recognized as valid ROM files. However, this should only be done **after**
implementing 8.1, otherwise `.svm` files inside subfolders would appear as
separate entries in the ROM list.

With the real data now available, this is confirmed as **not needed**: the 134
M3U entry points work correctly, and the 105 `.svm` + 16 `.scummvm` files
remain properly hidden inside subfolders. Adding `.svm` to the extensions list
would only cause problems.

### 8.4b ScummVM Missing Data Validation (Priority: Medium -- New)

**Where:** `collect_roms_recursive()` or a new validation pass

17 ScummVM M3U files reference game data subfolders that do not exist. These
entries appear in the ROM list but cannot be launched. Options:

1. **Hide M3U entries whose referenced .svm/.scummvm target does not exist.**
   This requires parsing the M3U content and checking file existence -- more
   I/O but gives a clean ROM list.
2. **Show but mark as "incomplete"** with a visual indicator. Less disruptive
   but adds UI complexity.
3. **Do nothing** -- the user sees the entry, attempts to launch, and gets an
   error from RePlayOS. This is the current behavior.

Option 1 is recommended since it is consistent with the "M3U as authoritative
entry point" model -- if the M3U points to nothing, the game is not available.

### 8.5 M3U Content Parsing for Game Detail (Priority: Low)

On the game detail page, when viewing an M3U game, show the list of referenced
disc files as a "Disc Files" section. This gives the user visibility into what
the M3U contains without cluttering the main ROM list.

### 8.6 Favorites and Recents (Priority: Medium)

Ensure that when an M3U game is favorited or appears in recents, the reference
uses the M3U filename (not individual disc filenames). RePlayOS itself uses the
M3U as the launch target, so favorites/recents from the OS should already
reference the M3U.

---

## 9. Impact Assessment

### 9.1 X68000 (Immediate Impact)

With 1,005 M3U files and 1,863 .dim files, implementing disc-file hiding would
significantly reduce the visible game count for X68000. The exact reduction
depends on how many .dim files are referenced by M3U files, but roughly:

- Current game count: ~3,163 (1,005 M3U + 1,863 .dim + 295 .hdf)
- After M3U dedup: ~1,300 (1,005 M3U entries + ~295 standalone .hdf files)

This is a ~60% reduction in game count, reflecting the actual number of distinct
games rather than counting every floppy disk image separately.

### 9.2 ScummVM (Immediate Impact -- New)

With 134 M3U files and the subfolder-based structure, ScummVM is **not affected
by double-counting** -- each game already appears exactly once. However:

- **17 M3U files reference missing game data** -- these appear as entries that
  cannot be launched. A validation pass could flag or hide these.
- **2 entries have encoding mismatches** between the M3U filename (UTF-8) and
  the filesystem directory name. These may cause issues when the app attempts
  to resolve the game data path.
- **Thumbnail matching will be poor** for the ~80% of games with Spanish titles
  or Spanish-tagged filenames that differ from English libretro-thumbnails names.
- **Total visible ScummVM game count**: 134 games (correct, no inflation).

### 9.3 PlayStation / Dreamcast / Sega CD (Future Impact)

These systems have empty ROM folders on the current NFS mount but are the most
common M3U users in the broader retro gaming community. When users add PSX games
with M3U files, the improvement will prevent the confusing display of 3-4
entries per game.

### 9.4 Performance Considerations

Parsing M3U files adds I/O during ROM scanning. For 1,005 + 134 = 1,139 M3U
files on NFS:
- X68000: each parse reads a small text file (< 1 KB for most)
- ScummVM: each parse reads a tiny file (60-200 bytes)
- The HashSet lookup for filtering is O(1) per ROM
- Total overhead: negligible compared to the existing `read_dir` traversal

For X68000 M3U files that contain binary data, reading the full file to parse
could be expensive (1.2 MB per file). The implementation should read only the
first few KB and parse lines, or use `BufReader` to read line-by-line.

ScummVM M3U files are always small and contain a single line, so parsing is
trivial. However, the parser needs to handle absolute paths (extracting just
the filename from the full path) rather than expecting bare filenames.

---

## 10. Summary of Key Findings

1. **`is_m3u` is a dead field** -- it propagates through the entire stack but is
   never consumed by any component or logic.

2. **Double-counting is active for X68000** -- 1,005 M3U files alongside 1,863
   .dim files produce ~60% game count inflation.

3. **ScummVM has no double-counting problem** -- 134 M3U files serve as entry
   points, with game data in subfolders that the scanner does not recurse into.
   Each game appears exactly once.

4. **Two systems now have M3U files**: Sharp X68000 (1,005) and ScummVM (134),
   totaling 1,139 M3U files on the NFS share.

5. **M3U path conventions differ by system**: X68000 uses relative filenames,
   ScummVM uses absolute paths (`/media/nfs/...`). The M3U parser must handle
   both formats.

6. **No disc-file hiding logic exists** -- M3U files and their referenced disc
   files appear as independent entries everywhere (ROM list, search, game count).
   However, this only causes visible problems for X68000, not ScummVM.

7. **Box art matching already works** for M3U files via the existing fuzzy
   matching pipeline, since the M3U stem matches the thumbnail naming after tag
   stripping. ScummVM thumbnails will be harder to match due to Spanish titles.

8. **ScummVM handling is accidentally correct** -- the `.svm` extension is not in
   the system's extension list and data lives in subfolders, so only the M3U
   entry appears. Both `.svm` (105 files) and `.scummvm` (16 files) coexist as
   engine ID formats in the collection.

9. **17 incomplete ScummVM games** have M3U entry points but missing game data
   subfolders. These will appear as broken entries in the ROM list.

10. **The fix for X68000 is localized** -- the primary change is in `roms.rs`
    (parsing M3U content and filtering referenced files). ScummVM needs no
    disc-hiding fix but would benefit from validation of missing game data.
