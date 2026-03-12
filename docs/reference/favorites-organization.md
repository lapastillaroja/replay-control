# Favorites Organization Design

> **Status**: Implemented. Core functions `organize_favorites()`, `OrganizeCriteria`, and `OrganizeResult` are in `replay-control-core/src/favorites.rs`. Server functions `organize_favorites`, `flatten_favorites` are in `replay-control-app/src/server_fns/favorites.rs`.

## Overview

Allow users to organize their favorites into subfolders using configurable criteria, with up to 2 levels of nesting.

## Current State

- Favorites stored as `.fav` marker files in `_favorites/` directory
- Filename format: `{system}@{rom_filename}.fav`
- File contents: relative ROM path (e.g., `/roms/sega_smd/Sonic.md`)
- Existing support: flat (root) or system-grouped (`group_by_system()`)
- `collect_favorites()` recursively scans all subfolders

## Organization Criteria

### Available

1. **System** — Group by system folder name (e.g., `nintendo_nes/`, `sega_smd/`)
2. **Genre** — Group by game genre from embedded game_db (e.g., `Platform/`, `Action/`, `RPG/`)
3. **Players** — Group by max player count: `1 Player`, `2 Players`, `Multiplayer`
4. **Alphabetical** — Group by first letter of display name: `A/`, `B/`, ..., `Z/`, `#/` (numbers/symbols)

### Nesting

Users can pick up to 2 criteria to create nested subfolders:
- **System → Genre**: `_favorites/nintendo_nes/Platform/game.fav`
- **Genre → System**: `_favorites/Platform/nintendo_nes/game.fav`
- **Players → Alphabetical**: `_favorites/2 Players/S/game.fav`
- Single criterion also supported: `_favorites/Platform/game.fav`

## Approaches Considered

### Approach A: Move files (chosen)

Organize moves `.fav` files from root into structured subfolders. A "keep originals" option copies instead of moves, leaving the root copies for ReplayOS compatibility.

**Pros**: Persistent on disk, ReplayOS could potentially read the structure, simple mental model.
**Cons**: Modifies disk layout, need to handle ReplayOS compatibility.

### Approach B: Virtual organization (display only)

Don't move files on disk. Replay Control organizes the display based on metadata at render time.

**Pros**: No disk changes, always compatible with ReplayOS, instant.
**Cons**: Not persistent, no benefit to ReplayOS UI, no folder browsing.

### Approach C: Metadata sidecar file

Store organization preferences in a JSON sidecar (e.g., `_favorites/.organize.json`) and apply at list time.

**Pros**: Reversible, separates data from layout.
**Cons**: Extra file format, overhead, ReplayOS won't read it.

### Decision

**Approach A** selected. File-based organization is consistent with how ReplayOS works (everything is file/folder based). The "keep originals" option (enabled by default) ensures backwards compatibility — ReplayOS UI reads root favorites while Replay Control shows the organized view.

## Keep Originals

When enabled (default), organizing **copies** `.fav` files to subfolders while keeping the originals at root. This ensures:
- ReplayOS native UI still sees all favorites at root
- Replay Control shows the organized view (deduplicates by preferring subfolder entries)
- Flattening just deletes the subfolders (originals at root are untouched)

When disabled, organizing **moves** files to subfolders. Flatten moves them back.

## Deduplication

When listing favorites with "keep originals" enabled, the same `.fav` file may exist at root AND in a subfolder. `list_favorites()` already collects all `.fav` files recursively. The caller deduplicates by `marker_filename`, preferring the subfolder version (more specific location).

When flattening, if a file already exists at root, the subfolder copy is simply deleted.

## Genre Fallback

Not all games have genre data in the game_db (arcade games use a separate DB, homebrew/translations may not match). When genre is unknown, files go into an "Other" folder.

## Flatten (Revert)

Enhanced `flatten_favorites()`:
- Recursively scans all nested subfolders (not just one level deep)
- Moves `.fav` files back to root
- Skips duplicates (if file already exists at root)
- Removes empty subfolders after moving

## API

### Core functions (`replay-control-core/src/favorites.rs`)

```rust
pub enum OrganizeCriteria {
    System,
    Genre,
    Players,
    Alphabetical,
}

pub struct OrganizeResult {
    pub organized: usize,
    pub skipped: usize,
}

pub fn organize_favorites(
    storage: &StorageLocation,
    primary: OrganizeCriteria,
    secondary: Option<OrganizeCriteria>,
    keep_originals: bool,
) -> Result<OrganizeResult>;
```

### Server functions

```
get_organize_options() -> OrganizeOptions  // returns available criteria
organize_favorites(primary, secondary, keep_originals) -> OrganizeResult
flatten_favorites() -> usize  // already exists, enhanced for deep nesting
```

## UX

The favorites page gets an "Organize" button in the header that opens a panel/modal:
- Primary criteria selector
- Optional secondary criteria selector
- "Keep originals" toggle (default on)
- "Organize" action button
- "Flatten All" button to revert

The organize state is reflected in the subfolder structure on disk. The UI shows the organized hierarchy when viewing "By System" or the organized grouping.
