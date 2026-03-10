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
