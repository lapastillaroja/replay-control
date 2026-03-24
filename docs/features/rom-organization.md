# ROM Organization

How favorites, recents, and region preferences work.

## Favorites

### File Format
Favorites are `.fav` marker files in `roms/_favorites/`:
- Filename: `<system>@<rom_filename>.fav`
- Content: relative ROM path (e.g., `/roms/sega_smd/Sonic.md`)

### Operations
- `add_favorite()`: Creates the `.fav` file and `_favorites/` directory
- `remove_favorite()`: Deletes the `.fav` file
- Cache: In-memory L1 with mtime-based invalidation (no L2 SQLite for favorites)

### UI
- Favorite toggle on ROM list items and game detail page (optimistic UI)
- Favorites page with hero card, recently added, per-system cards, flat/grouped views
- Remove confirmation (star click shows "Remove?" before acting)
- `is_favorite` flag on `RomEntry` for SSR-ready display

### Organization
Favorites can be organized into subfolders using configurable criteria (up to 2 levels of nesting). Available criteria: System, Genre, Players, Alphabetical, Developer. The Developer criterion uses `normalize_developer()` to handle MAME manufacturer string variations (licensing info, regional suffixes, corporate names, joint ventures). Console games use the `game_library.developer` field from LaunchBox enrichment. Organize can copy (keeping originals at root for RePlayOS compatibility) or move files. `flatten_favorites()` reverses the organization.

## Recents

### File Format
Recents are `.rec` marker files in `roms/_recent/`:
- Filename: `<system>@<rom_filename>.rec`
- Content: relative ROM path
- Timestamp: file mtime = last played time

When launched via a favorite, RePlayOS creates `.fav.rec` extension files. The app deduplicates by `(system, rom_filename)`.

### Integration with Game Launch
`add_recent()` is called after a successful game launch, creating the `.rec` file before returning the server function response. `invalidate_recents()` clears the L1 cache so the home page reflects the launch immediately.

### Special Handling
- `.fav` suffix stripped from recently played entries
- Deduplication when both `.rec` and `.fav.rec` exist for the same game
- Sorted by mtime descending (most recent first)

## Region Preference

Stored in `.replay-control/settings.cfg` as `region_preference = "usa"` (default). A secondary region preference is also supported, providing a two-tier sort: Primary > Secondary > World > others.

Options: `usa`, `europe`, `japan`, `world`.

### Effects
- **ROM sort order**: Preferred region sorts first in game lists
- **Search scoring**: Region preference bonus in search results
- **Recommendation dedup**: Dedup CTE picks the preferred-region variant when multiple exist

### Config Boundary
Region preference lives in the app's own config file (`.replay-control/settings.cfg`), not in `replay.cfg`. This maintains the boundary: `replay.cfg` belongs to RePlayOS (on the SD card), app-specific settings go in `.replay-control/`.

## Key Source Files

| File | Role |
|------|------|
| `replay-control-core/src/library/favorites.rs` | Favorite add/remove/list |
| `replay-control-core/src/library/recents.rs` | Recent list/add |
| `replay-control-core/src/platform/storage.rs` | Settings file, RC_DIR, config boundary |
| `replay-control-app/src/api/cache/favorites.rs` | Favorites caching with mtime invalidation |
| `replay-control-app/src/api/cache/mod.rs` | Recents caching, CacheEntry |
| `replay-control-app/src/server_fns/roms.rs` | `launch_game` with recents integration |
