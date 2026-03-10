# Known Issues & TODOs

## ROM Rename Side Effects

When a ROM file is renamed via the companion app, several data sources that
reference the original filename become orphaned:

| Data | Location | Impact |
|------|----------|--------|
| User screenshots | `captures/{system}/{old_filename}_*.png` | Screenshots no longer match the renamed ROM |
| Pinned videos | `.replay-control/videos/{system}/{old_filename}.json` | Video links lost for the renamed ROM |
| Favorites | `_favorites/*/{system}@{old_filename}.fav` | RePlayOS manages these — .fav file content has the old path |
| Recent entries | `_recent/{system}@{old_filename}.rec` | Old path in .rec file |
| Metadata DB | `.replay-control/metadata.db` | Cached metadata keyed by old filename |

### Current behavior
- **Favorites**: The `rename_rom` function in `roms.rs` already updates `.fav`
  files when renaming (renames the .fav file and updates its content).
- **Recent entries**: Not updated — old entries become orphaned (acceptable,
  they expire naturally).
- **User screenshots**: Not updated — orphaned after rename.
- **Pinned videos**: Not updated — orphaned after rename.
- **Metadata DB**: Not updated — stale cache entry (re-import would fix).

### Proposed solution
When renaming a ROM, cascade the rename to related data:
1. Rename matching screenshot files in `captures/{system}/`
2. Rename video JSON file in `.replay-control/videos/{system}/`
3. Update metadata DB entry (or invalidate cache for that ROM)
4. Recent entries: skip (they expire naturally)

This should be a single `rename_rom_cascade()` function that wraps the existing
`rename_rom()` and handles all side effects. Failures in side effects should be
logged but not block the rename.

### Priority
Medium — affects users who rename ROMs and have screenshots/videos for them.
The workaround is to re-take screenshots or re-pin videos after renaming.

## Alpha Player Hidden from UI

The "Alpha Player" system (`alpha_player`) is a libretro video player core
whose "ROMs" are video files (mkv, avi, mp4, mp3, flac, ogg), not games. The
current game-centric UI — metadata fields, box art, "games"/"ROMs" labels —
does not make sense for video content.

### Current behavior
Alpha Player is listed in `HIDDEN_SYSTEMS` in `systems.rs` and filtered out by
`visible_systems()`, which is used by `scan_systems()` and `find_duplicates()`
in `roms.rs`. It will not appear in the systems list, global search results, or
duplicate detection. The system definition is still present in `SYSTEMS` and
`find_system()` still resolves it (needed if RePlayOS references it in
favorites/recents).

### Recommended future work
Build a dedicated "Media" section with video-appropriate UI. See
`docs/reference/alpha-player-analysis.md` for a full analysis.

### Priority
Low — Alpha Player is a niche feature and hiding it has no user-facing
downside. Revisit when media features are planned.
