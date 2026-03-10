# Game Launch — Implementation Plan

## Status: Phase 1 Implemented (2026-03-11)

> **NOTE**: This feature is a reverse-engineered workaround. RePlayOS has no
> official API for programmatic game launching. Check RePlayOS changelogs for
> official remote launch support in future releases.

## Overview

Add a "Launch on TV" button to the companion app that launches a game on the RePlayOS device via the autostart + `systemctl restart` mechanism.

## ROM Path Data Availability

`GameRef.rom_path` contains exactly the path the autostart mechanism needs (e.g., `/roms/sega_smd/00 Clean Romset/Sonic The Hedgehog (USA, Europe).md`).

| Context | Type | Has `rom_path`? | Notes |
|---------|------|-----------------|-------|
| Game detail page | `GameInfo` (via `RomDetail`) | Yes | Via `relative_path_sv` StoredValue |
| ROM list items | `RomEntry` (wraps `GameRef`) | Yes | Via `relative_path` StoredValue |
| Search results | `GlobalSearchResult` | **No** | Only `rom_filename` + `system` |
| Favorites | `Favorite` (wraps `GameRef`) | Yes | On each `Favorite` |
| Home (recents) | `RecentEntry` (wraps `GameRef`) | Yes | On each `RecentEntry` |

## Button Placement

### Phase 1: Game Detail Page (implemented)

Placed as a **prominent CTA below the cover art**, above the info section. This is the first interactive element after the game image — users see the game, then can immediately launch.

- Accent-colored background, full width on mobile, auto-width centered on desktop
- Icon: play triangle `▶` (`\u{25B6}`)
- Label: "Launch on TV"
- Section class: `.game-launch-cta`

The remaining action buttons (favorite, rename, delete) stay in the Actions section at the bottom.

### Phase 2: Home page + Favorites hero cards

Small launch button on the "Last Played" hero card (home) and "Latest Added" hero card (favorites). Re-launch shortcuts for the most common use case.

### Phase 3 (optional): ROM list inline

Small play icon on `RomItem` in the ROM list. More complex (row is already dense).

### Not recommended for search results

`GlobalSearchResult` lacks `rom_path`. Users can navigate to the detail page instead.

## Server Function

```rust
#[server(prefix = "/sfn")]
pub async fn launch_game(rom_path: String) -> Result<String, ServerFnError>
```

**Input**: `rom_path` as stored in `GameRef.rom_path`

**Steps**:
1. Validate ROM exists on disk (join `storage.root` + `rom_path`)
2. Guard: check `is_replayos()` — return simulation message in dev
3. Create `_autostart/` directory
4. Write `rom_path` to `_autostart/autostart.auto`
5. Run `systemctl restart replay.service`
6. Spawn background thread for cleanup + health check
7. Return success message immediately

**Core module**: `replay-control-core/src/launch.rs` (consistent with `delete_rom`, `rename_rom` pattern).

## Health Check / Recovery

After launching, a background thread runs a health check to recover from failed launches (e.g., Flycast/arcade_dc blank screen):

```
Timeline:
  0s  — systemctl restart replay.service
  5s  — delete autostart file (binary has had time to read it)
  10s — health check: is a libretro core loaded?
         ├── yes → success, done
         └── no  → restart service again (boots to menu, no autostart file)
```

**How it checks**: reads `/proc/PID/maps` for the replay process and looks for
`libretro.so` entries (excluding `replay_libretro` which is the menu frontend).

**Why**: some systems (notably `arcade_dc`/Flycast) fail silently on autostart,
leaving a blank screen. Without recovery, the user must manually reboot the Pi.

**Future-proof**: only uses standard Linux interfaces (`/proc`, `pgrep`,
`systemctl`) — no dependency on binary internals.

## UX Flow

### State machine

```
[Idle] --click--> [Launching...] --success--> [Launched!] --3s--> [Idle]
                                  --error----> [Error msg] --3s--> [Idle]
                                  --simulated-> [Not on RePlayOS] --3s--> [Idle]
```

### Button states

| State | Text | Style | Disabled? |
|-------|------|-------|-----------|
| Idle | "Launch on TV" | Accent bg, white text | No |
| Launching | "Launching..." | Accent bg, dimmed | Yes |
| Success | "Launched!" | Green bg | Yes (3s cooldown) |
| Simulated | "Not running on RePlayOS" | Gray bg | No (3s display) |
| Error | "Failed to launch" | Normal | No (error shown as text) |

### No confirmation dialog

Launching is non-destructive. Current game on TV will be interrupted, but that's expected behavior. A dialog adds friction.

### Cooldown

3-second state display prevents rapid re-launches.

## System Compatibility

Not all systems support autostart. Tested on Pi 5 (2026-03-11):

| System | Core | Autostart | Notes |
|--------|------|-----------|-------|
| `sega_smd` | Genesis Plus GX | Works | |
| `arcade_fbneo` | FBNeo | Works | |
| `sharp_x68000` | PX68K | Works | |
| `arcade_dc` | Flycast | **Fails** | Blank screen; health check recovers to menu |

More systems need testing. Flycast is known to be unstable (crashes during
gameplay reported by users on Telegram and GitHub issues).

## i18n Keys

```
"game_detail.launch" => "Launch on TV"
"game_detail.launching" => "Launching..."
"game_detail.launched" => "Launched!"
"game_detail.launch_error" => "Failed to launch"
"game_detail.launch_not_replayos" => "Not running on RePlayOS"
```

## Files Modified

| File | Change |
|------|--------|
| `replay-control-core/src/launch.rs` | Core launch logic + health check recovery |
| `replay-control-core/src/lib.rs` | `pub mod launch;` |
| `replay-control-app/src/server_fns.rs` | `launch_game` server function with `is_replayos()` guard |
| `replay-control-app/src/pages/game_detail.rs` | `GameLaunchAction` component, placed below cover art |
| `replay-control-app/src/main.rs` | `register_explicit::<LaunchGame>()` |
| `replay-control-app/src/i18n.rs` | Launch translation keys (5 keys) |
| `replay-control-app/style/style.css` | `.game-action-launch`, `.game-launch-cta` styles |

## Notes

- **Dev testing**: `is_replayos()` guard returns simulation message locally (gray button, "Not running on RePlayOS")
- **Storage path**: ROM path written to autostart file is relative (e.g., `/roms/sega_smd/Sonic.md`), not absolute
- **Concurrent launches**: Last write wins (acceptable — same as physical remote)
- **`systemctl restart`**: ~104ms command time, runs inline
- **Hack status**: This is a workaround using the autostart mechanism (designed for boot-time auto-launch). Monitor RePlayOS changelogs for official remote launch API support.
