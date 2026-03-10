# Alpha Player Analysis

## What is Alpha Player?

Alpha Player is a libretro core (`alpha_player_libretro.so`) bundled with
RePlayOS that functions as a media player. It is registered as a "system" with
folder name `alpha_player`, manufacturer "RePlayOS", and category `Utility`.

Its supported file extensions are: `mkv`, `avi`, `mp4`, `mp3`, `flac`, `ogg`.

Users place video or audio files in `roms/alpha_player/` on their storage
device, and RePlayOS can launch them through the libretro frontend just like any
other core.

## Why it doesn't fit the current UI

The companion app's interface is designed around games:

| Aspect | Games | Alpha Player media |
|--------|-------|--------------------|
| Terminology | "Games", "ROMs" | Videos, music files |
| Metadata | Year, genre, developer, players, region | Duration, resolution, codec, format |
| Box art | Game cover art from thumbnail repos | Movie poster / video thumbnail |
| Game DB / Arcade DB | Lookup by ROM filename | No equivalent database |
| LaunchBox metadata | Matches by title | No video entries in LaunchBox |
| Favorites / recents | Makes sense | Makes sense (same mechanism) |
| Duplicate detection | By filename + size | Same logic works, but "duplicate ROM" label is wrong |
| Clone filtering | Arcade parent/clone | Not applicable |
| ROM tags (region, hack, etc.) | Parsed from filename tags | Not applicable |

Displaying Alpha Player alongside game systems leads to:
- Confusing "0 games" count on the home/more page when no videos are present
- Wrong labels ("games", "ROMs") applied to video files
- Metadata fields (year, developer, players, genre) that are meaningless for videos
- Box art lookup that will never find matches

## How it could be integrated nicely

### 1. Dedicated "Media" or "Videos" section

Add a separate tab or section in the app (alongside Games, Favorites, etc.)
that presents Alpha Player content with video-appropriate UI.

**Design considerations:**
- List or grid view with video-oriented cards (filename, file size, format icon)
- No metadata fields like developer, players, genre
- Simple alphabetical sorting (no region/clone/hack filtering)
- "Videos" / "Media" label instead of "Games" / "ROMs"

### 2. Video thumbnails

**Does RePlayOS generate thumbnails for videos?** Likely not automatically.
Libretro cores can produce screenshots during playback, which would end up in
`captures/alpha_player/`, but there is no pre-launch thumbnail generation.

Options:
- Show a generic video icon or format-based icon (film reel for video, music
  note for audio)
- If captures exist from a previous playback, use those as thumbnails
- Future: server-side thumbnail generation using ffmpeg (if available on the Pi)

### 3. Browsable and/or launchable?

**Browsable:** Yes. Users should be able to see what media files are on their
device, rename them, and delete them — the same file management features
available for ROMs.

**Launchable:** Launching is handled entirely by RePlayOS (the companion app
does not launch anything directly). The app can show the file list, but
launch is done on the device itself. This is the same as games — the app is for
browsing and managing, not launching.

### 4. Search integration

Two reasonable approaches:

- **Include in global search** with a "Media" category badge so results are
  clearly distinguished from games. This is simpler and lets users find
  everything from one place.
- **Separate search** within the Media section only. Cleaner separation but adds
  UI complexity.

Recommended: include in global search with a visual indicator, since the search
infrastructure already iterates over systems and adding a category badge is
trivial.

### 5. Audio files

Alpha Player also supports audio formats (mp3, flac, ogg). The UI should
account for both video and audio, possibly with different icons. A "Media"
label is more appropriate than "Videos" for this reason.

## Recommended approach

### Phase 1 (current): Hide it
Alpha Player is filtered out via `HIDDEN_SYSTEMS` in `systems.rs`. The system
definition remains intact for compatibility (RePlayOS may reference it in
favorites/recents files). This is the current state.

### Phase 2 (future): Minimal media section
- Add a "Media" tab/page that shows Alpha Player content
- Simple file list with format icons, file size, and rename/delete actions
- No metadata fields, no box art lookup
- Include media files in global search with a "Media" badge
- **Priority: Low** — Alpha Player is a niche feature

### Phase 3 (if demand exists): Rich media experience
- Thumbnail generation (ffmpeg on Pi, or capture-based)
- Duration/resolution display (requires parsing media files server-side)
- Separate audio vs. video views
- **Priority: Very low** — only if users request it

## Current status

Hidden from the UI. The `HIDDEN_SYSTEMS` list in
`replay-control-core/src/systems.rs` excludes `alpha_player` from
`visible_systems()`, which is used by `scan_systems()` (system listing) and
`find_duplicates()` (duplicate detection). The `find_system()` function still
resolves it for backward compatibility with favorites/recents that may reference
Alpha Player entries.
