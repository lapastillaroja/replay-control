# User Screenshots — Implementation Plan

## Overview

Display screenshots taken by the user on RePlayOS alongside metadata screenshots
on the game detail page. RePlayOS saves screenshots during gameplay to
`<storage>/captures/{system}/`.

## Screenshot Storage

**Location**: `<storage_root>/captures/{system}/{rom_filename}_{YYYYMMDD}_{HHMMSS}.png`

- Files are small (7-19KB, retro resolution 256x224 to 640x480)
- System subdirectories mirror ROM systems (`sega_smd`, `arcade_fbneo`, etc.)
- Legacy format (no timestamp): `{rom_filename}.png`
- `StorageLocation.captures_dir()` already exists in the codebase

## Display Approach: Separate Sections

Two distinct sections on the game detail page — no visual conflict:

```
Screenshots              <- metadata (from libretro-thumbnails)
  [official snap]
  "No official screenshots" fallback

Your Captures            <- user-taken on RePlayOS
  [wrapping gallery, newest first, capped at 12 with "View all" toggle]
  Click thumbnail to open fullscreen lightbox with prev/next navigation
  "Take screenshots during gameplay on your RePlayOS — they'll appear here!"
```

**Why separate over tabs/mixed gallery:**
- Metadata: 0-1 images. User captures: grows over time. Different scales.
- Different purposes: reference vs personal memories.
- No tab state management needed.
- Each section can evolve independently.

## File Matching

Match screenshots to ROMs by filename prefix:

```
ROM: Sonic The Hedgehog 2 (World) (Rev A).md
  -> Sonic The Hedgehog 2 (World) (Rev A).md_20260310_015805.png  ✓
  -> Sonic The Hedgehog 2 (World) (Rev A).md.png                  ✓ (legacy)
  -> Sonic The Hedgehog 2 (World).md_20260310_015805.png           ✗ (different ROM)
```

Require `_` or `.` immediately after the ROM filename to prevent false matches
on overlapping prefixes.

## Implementation

### Phase 1: Game detail integration — DONE

**Core** (`replay-control-core/src/screenshots.rs`):
- `UserScreenshot` struct: `{ filename, timestamp: Option<NaiveDateTime> }`
- `find_screenshots_for_rom(storage, system, rom_filename) -> Vec<UserScreenshot>`
- Parse timestamp from filename via regex: `_(\d{8})_(\d{6})\.png$`
- Sort by timestamp descending (newest first)

**Server function**:
- `user_screenshots: Vec<ScreenshotUrl>` in `RomDetail`
- `ScreenshotUrl`: `{ url: String, timestamp: Option<i64> }`
- Populated in `get_rom_detail()` by calling core function

**Serving**:
- `/captures/:system/:filename` static file handler
- Cache headers: `Cache-Control: public, max-age=31536000, immutable`

**UI** (`game_detail.rs`):
- "Your Captures" section below metadata screenshots
- Wrapping gallery (`.user-captures-gallery`), capped at 12 thumbnails by default
- "View all (N)" button expands to show all captures
- Click any thumbnail to open fullscreen lightbox (`CapturesLightbox` component)
- Lightbox: dark overlay, full-size image with `image-rendering: pixelated`, prev/next arrows, close button, keyboard navigation (Escape, Left/Right arrows)
- Empty state: encouraging message to take screenshots

**No thumbnail generation needed** — retro PNGs are already tiny.

### ~~Phase 2: Standalone gallery page~~ — DISCARDED

Not needed. The fullscreen lightbox on the game detail page provides sufficient
browsing capability for captures. A standalone `/captures` route would add
navigation complexity without meaningful benefit.

### Phase 3: Delete/management
- Delete button with confirmation per screenshot
- Optimistic removal from gallery

## Files Modified

| File | Change |
|------|--------|
| `replay-control-core/src/screenshots.rs` | Screenshot discovery + matching |
| `replay-control-core/src/lib.rs` | `pub mod screenshots;` |
| `replay-control-app/src/server_fns.rs` | `ScreenshotUrl` struct, extend `RomDetail` |
| `replay-control-app/src/pages/game_detail.rs` | "Your Captures" gallery + lightbox + cap at 12 |
| `replay-control-app/src/main.rs` | Captures file serving route |
| `replay-control-app/src/i18n.rs` | Screenshot i18n keys |
| `replay-control-app/style/style.css` | Gallery + lightbox styles |
