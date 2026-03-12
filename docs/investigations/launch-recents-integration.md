# Launch on TV + Recents Integration

**Date:** 2026-03-12
**Status:** Implemented

## How "Launch on TV" Currently Works

The launch flow is implemented across two layers:

### Core layer: `replay-control-core/src/launch.rs`

`launch_game(storage, rom_path)` does the following:

1. **Validates** the ROM file exists on disk (resolves `rom_path` relative to storage root).
2. **Creates** `roms/_autostart/autostart.auto` containing the `rom_path` string.
3. **Restarts** `replay.service` via `systemctl restart replay.service`.
4. **Spawns a background thread** that:
   - Waits 5s, then deletes the `autostart.auto` file (cleanup).
   - Waits another 5s (10s total), then checks `/proc/PID/maps` for a loaded libretro game core.
   - If no game core is detected, restarts the service again (recovery to menu).

This is a reverse-engineered workaround -- RePlayOS has no official API for programmatic game launching. The autostart mechanism was designed for boot-time auto-launch, not companion app integration.

### App layer: `replay-control-app/src/server_fns/roms.rs`

The `launch_game` server function (line 288):

```rust
#[server(prefix = "/sfn")]
pub async fn launch_game(rom_path: String) -> Result<String, ServerFnError> {
    if !is_replayos() {
        return Ok("Launch simulated (not on RePlayOS)".into());
    }
    let state = expect_context::<crate::api::AppState>();
    replay_control_core::launch::launch_game(&state.storage(), &rom_path)
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    Ok("Game launching".into())
}
```

Key observations:
- Returns "Launch simulated" when not running on a real RePlayOS device (`/opt/replay` not present).
- The `rom_path` parameter is the path relative to the roms directory (e.g., `/roms/sega_smd/Sonic.md`).
- **No recents entry is created.** The function only triggers the launch mechanism.

### UI layer: `replay-control-app/src/pages/game_detail.rs`

The `GameLaunchAction` component (line 409) renders a "Launch on TV" button with states:
- Default -> Launching (spinner) -> Launched (success, 3s timeout) -> Back to default
- Also handles: simulated (not on RePlayOS), error states
- Calls `server_fns::launch_game(relative_path)` on click.

The component has access to `relative_path` (StoredValue<String>) which is `game.rom_path` -- the relative path from storage root.

## How RePlayOS Tracks Recents

### File format

Recents are stored in `roms/_recent/` as `.rec` marker files. The format is identical to favorites (`.fav` files):

- **Filename convention:** `<system_folder>@<rom_filename>.rec`
- **File content:** The relative path to the ROM from storage root (e.g., `/roms/sega_smd/Sonic.md`)
- **Timestamp:** The file's modification time (mtime) is used as the "last played" timestamp. No timestamp is stored inside the file content.

Examples from real devices:
```
arcade_dc@ggx15.zip.rec           -> /roms/arcade_dc/Atomiswave/Horizontal Games/00 Clean Romset/ggx15.zip
amstrad_cpc@R-Type (1988)(...).dsk.rec -> /roms/amstrad_cpc/00 Clean Romset/DSK/R-Type (1988)(...).dsk
```

Special case -- when a game is launched via a `.fav` symlink, RePlayOS creates a marker with `.fav.rec` extension:
```
arcade_fbneo@chelnov.zip.fav.rec  -> /roms/arcade_fbneo/chelnov.zip
```

### Core reading logic: `replay-control-core/src/recents.rs`

`list_recents(storage)`:

1. Scans `roms/_recent/` for `.rec` files.
2. For each file:
   - Reads content to get `rom_path`.
   - Extracts `system` from the filename (part before `@`) via `system_from_fav_filename()`.
   - Extracts `rom_filename` from the filename (part after `@`, stripping `.rec` and `.fav` suffixes).
   - Gets `last_played` from the file's mtime (as Unix epoch seconds).
3. Sorts by `last_played` descending (most recent first).
4. **Deduplicates** by `(system, rom_filename)` -- keeps only the most recent entry when both a `.rec` and a `.fav.rec` marker exist for the same game.

### Caching: `replay-control-app/src/api/cache.rs`

Recents use the same `CacheEntry<T>` pattern as other cached data:
- **L1 cache:** In-memory `RwLock<Option<CacheEntry<Vec<RecentEntry>>>>`.
- **Invalidation:** mtime-based (checks `_recent/` directory mtime) + hard TTL (300s).
- **No L2 (SQLite) cache** for recents -- they're read directly from the filesystem each time L1 misses.
- The full `invalidate()` method clears recents along with everything else.
- There is **no dedicated `invalidate_recents()` method** (unlike `invalidate_favorites()`).

### How RePlayOS creates recents

RePlayOS's C frontend creates `.rec` files when it launches a game. The file is created with the current timestamp as mtime. When launched via a favorite, the marker has a `.fav.rec` extension.

**Important:** RePlayOS creates the `.rec` marker when it *runs* the game, not when it reads the autostart file. This means:
- The autostart mechanism triggers a service restart.
- RePlayOS reads `_autostart/autostart.auto`, loads the ROM.
- At some point during/after loading, RePlayOS creates the `.rec` marker.

## How Favorites Work (for comparison)

The favorites system is relevant because its file format is nearly identical to recents:

### File format
- **Location:** `roms/_favorites/` (with optional subfolders for organization)
- **Filename:** `<system_folder>@<rom_filename>.fav`
- **Content:** Relative ROM path (e.g., `/roms/sega_smd/Sonic.md`)

### Write operations in core
`add_favorite(storage, system_folder, rom_relative_path, grouped_by_system)` in `favorites.rs`:
1. Constructs the `.fav` filename: `{system_folder}@{rom_filename}.fav`
2. Creates `_favorites/` directory (and system subfolder if grouped).
3. Writes the `rom_relative_path` as the file content.
4. Returns a `Favorite` struct.

### Cache invalidation pattern
In the server function `add_favorite` and `remove_favorite`:
```rust
state.cache.invalidate_favorites();
```
There is a dedicated `invalidate_favorites()` method on `RomCache`.

## Proposed Changes

### 1. Add `add_recent()` to core: `replay-control-core/src/recents.rs`

A new function to create or update a `.rec` marker file:

```rust
/// Create or update a recent entry for a game.
///
/// Creates `<system>@<rom_filename>.rec` in `_recent/` with the ROM path as content.
/// If the file already exists, its mtime is updated to the current time (touch).
pub fn add_recent(
    storage: &StorageLocation,
    system_folder: &str,
    rom_filename: &str,
    rom_path: &str,
) -> Result<()> {
    let recents_dir = storage.recents_dir();
    std::fs::create_dir_all(&recents_dir)
        .map_err(|e| Error::io(&recents_dir, e))?;

    let rec_filename = format!("{system_folder}@{rom_filename}.rec");
    let rec_path = recents_dir.join(&rec_filename);

    // Write (or overwrite) the marker file.
    // Overwriting an existing file also updates its mtime.
    std::fs::write(&rec_path, format!("{rom_path}\n"))
        .map_err(|e| Error::io(&rec_path, e))?;

    Ok(())
}
```

Key design decisions:
- **Always writes** (not just touches) -- this ensures the file content is correct even if the ROM was moved/renamed since the last play.
- **Adds a trailing newline** to match the format used in `launch_game()` for `autostart.auto` and to be consistent with how RePlayOS likely writes these files.
- **Does not return a `RecentEntry`** -- the caller doesn't need it, and computing mtime after write would be redundant.

### 2. Add `invalidate_recents()` to cache: `replay-control-app/src/api/cache.rs`

Following the `invalidate_favorites()` pattern:

```rust
/// Invalidate only the recents cache (after launch creates a new entry).
pub fn invalidate_recents(&self) {
    if let Ok(mut guard) = self.recents.write() {
        *guard = None;
    }
}
```

### 3. Update `launch_game` server function: `replay-control-app/src/server_fns/roms.rs`

Add recents entry creation after successful launch:

```rust
#[server(prefix = "/sfn")]
pub async fn launch_game(rom_path: String) -> Result<String, ServerFnError> {
    if !is_replayos() {
        return Ok("Launch simulated (not on RePlayOS)".into());
    }

    let state = expect_context::<crate::api::AppState>();
    let storage = state.storage();

    replay_control_core::launch::launch_game(&storage, &rom_path)
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    // Create a recents entry so the home page reflects the launch immediately.
    // We need to extract system and rom_filename from the rom_path.
    // rom_path format: "/roms/<system>/<optional_subdirs>/<rom_filename>"
    if let Some(parsed) = parse_rom_path(&rom_path) {
        if let Err(e) = replay_control_core::recents::add_recent(
            &storage,
            &parsed.system,
            &parsed.rom_filename,
            &rom_path,
        ) {
            // Log but don't fail the launch -- the game is already launching.
            tracing::warn!("Failed to create recents entry: {e}");
        }
        state.cache.invalidate_recents();
    }

    Ok("Game launching".into())
}
```

The `parse_rom_path` helper extracts system and rom_filename from a path like `/roms/sega_smd/Sonic.md` or `/roms/arcade_dc/Atomiswave/Horizontal Games/00 Clean Romset/ggx15.zip`:

```rust
struct ParsedRomPath {
    system: String,
    rom_filename: String,
}

fn parse_rom_path(rom_path: &str) -> Option<ParsedRomPath> {
    // Strip leading "/roms/" prefix
    let path = rom_path.strip_prefix("/roms/")?;
    // First component is the system folder
    let (system, rest) = path.split_once('/')?;
    // Last component is the ROM filename
    let rom_filename = rest.rsplit_once('/').map(|(_, f)| f).unwrap_or(rest);
    Some(ParsedRomPath {
        system: system.to_string(),
        rom_filename: rom_filename.to_string(),
    })
}
```

### 4. No changes needed to the UI

The `GameLaunchAction` component in `game_detail.rs` already calls `server_fns::launch_game(relative_path)`. The recents entry creation happens server-side, so no client code changes are needed.

The home page's recents section fetches data via `server_fns::get_recents()`, which reads from the cache. After `invalidate_recents()`, the next page load will re-scan the `_recent/` directory and pick up the new entry.

## Edge Cases and Considerations

### 1. Double entry: companion app + RePlayOS both create `.rec` files

**Scenario:** The companion app creates `sega_smd@Sonic.md.rec`, then RePlayOS also creates the same file when the game actually loads.

**Analysis:** This is harmless.
- Both write the same content (the ROM path) to the same file.
- RePlayOS's write will overwrite the companion app's file, updating the mtime to a slightly later time (when the game actually loaded vs. when the launch button was pressed). This is actually more accurate.
- The deduplication logic in `list_recents()` handles `(system, rom_filename)` uniqueness, so even if somehow two different markers exist, only one would be shown.

**However:** There is a nuance -- RePlayOS may create a `.fav.rec` file (with the `.fav` suffix) if the game was also a favorite, while the companion app creates a plain `.rec` file. The dedup logic in `list_recents()` already handles this case (keeps the most recent by mtime).

### 2. RePlayOS might NOT create a .rec if launch fails

**Scenario:** The companion app creates the `.rec` entry, but the game fails to load (bad ROM, missing BIOS, core crash).

**Analysis:** The companion app's `.rec` file will persist, showing the game as "recently played" even though it didn't actually play. This is acceptable because:
- The health check in `launch.rs` already handles the recovery (restarts to menu).
- The user tapped "Launch on TV" intentionally, so showing it as recent is reasonable (it was the last attempted game).
- This matches the behavior of "recently opened" in most file managers (attempted open counts).

### 3. Non-RePlayOS environment (development)

**Scenario:** When `is_replayos()` returns false, the function returns early with "Launch simulated".

**Analysis:** No recents entry is created in development mode. This is correct -- there's no actual game launch, and creating phantom recents entries during development would be confusing.

If we wanted recents entries in development (for testing the home page), we could add the recents logic *before* the `is_replayos()` check, but this is not recommended -- the simulated case should remain side-effect-free.

### 4. Cache invalidation timing

**Scenario:** User taps "Launch on TV", the server creates the `.rec` file and invalidates the cache. But the user is still on the game detail page. When they navigate back to the home page, will they see the updated recents?

**Analysis:** Yes. The `invalidate_recents()` call clears the L1 cache. The next call to `get_recents()` (when the home page loads) will re-scan the `_recent/` directory. Since we wrote the `.rec` file synchronously *before* returning the server function response, it will be on disk when the home page query runs.

### 5. Race condition with the background cleanup thread

**Scenario:** The background thread in `launch_game()` deletes `autostart.auto` after 5 seconds. Could this interfere with the `.rec` file?

**Analysis:** No. The autostart file is in `_autostart/` and the recents file is in `_recent/`. They are completely independent directories. The background thread only touches the autostart file.

### 6. NFS storage considerations

**Scenario:** When using NFS storage, file operations may be slower or fail.

**Analysis:** The `std::fs::write()` call for creating the `.rec` file uses the same filesystem path as all other storage operations. The error is caught and logged (`tracing::warn!`) but does not fail the launch. NFS mtime behavior should be consistent since we're writing (not just touching) the file.

### 7. rom_path format parsing

**Scenario:** The `rom_path` parameter varies in format:
- Simple: `/roms/sega_smd/Sonic.md`
- With subdirectories: `/roms/arcade_dc/Atomiswave/Horizontal Games/00 Clean Romset/ggx15.zip`
- Multi-disc (M3U): `/roms/sony_psx/Resident Evil.m3u`

**Analysis:** The `parse_rom_path()` helper handles all these cases:
- System is always the first path component after `/roms/`.
- ROM filename is always the last path component.
- The full `rom_path` is stored in the `.rec` file content (preserving subdirectory structure).

## Summary of Files to Modify

| File | Change |
|------|--------|
| `replay-control-core/src/recents.rs` | Add `add_recent()` function |
| `replay-control-app/src/api/cache.rs` | Add `invalidate_recents()` method |
| `replay-control-app/src/server_fns/roms.rs` | Update `launch_game` to create recents entry + add `parse_rom_path` helper |

No changes to: UI components, client-side code, lib.rs exports (recents module already public), main.rs registration (launch_game server fn already registered).

---

## Implementation Notes

All three proposed changes have been implemented as described:

1. **`add_recent()`** in `replay-control-core/src/recents.rs` -- implemented exactly as proposed, with tests (`add_recent_creates_marker`, `add_recent_overwrites_existing`, `add_recent_creates_directory`).
2. **`invalidate_recents()`** in `replay-control-app/src/api/cache.rs` -- implemented following the `invalidate_favorites()` pattern.
3. **`launch_game` server function** in `replay-control-app/src/server_fns/roms.rs` -- updated to call `add_recent()` after successful launch and `invalidate_recents()` after creating the entry. The `parse_rom_path()` helper is implemented as a standalone function.
