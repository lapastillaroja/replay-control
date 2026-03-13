# Rename `rom_cache` → `game_library`

## Scope

**~367 references across ~25 files.** Zero references to `rom_cache`, `CachedRom`, or `CachedSystemMeta` should remain in source code after this refactor.

| Category | Files | Occurrences |
|----------|-------|-------------|
| Core source (`replay-control-core/`) | 2 | 152 |
| App source (`replay-control-app/`) | 6 | 44 |
| `RomCache` struct references | 3 | 48 |
| Documentation (`docs/`) | 15+ | 124 |

## Naming Mapping

### Rust identifiers

| Old | New | Notes |
|-----|-----|-------|
| `struct RomCache` | `struct GameLibrary` | The library manager (L1 cache + L2 coordination) |
| `impl RomCache` | `impl GameLibrary` | |
| `Arc<RomCache>` | `Arc<GameLibrary>` | |
| `RomCache::new(...)` | `GameLibrary::new(...)` | |
| `struct CachedRom` | `struct GameEntry` | A single game row in the `game_library` table |
| `struct CachedSystemMeta` | `struct SystemMeta` | Per-system metadata from `game_library_meta` |
| `clear_system_rom_cache()` | `clear_system_game_library()` | |
| `clear_all_rom_cache()` | `clear_all_game_library()` | |
| `save_system_roms()` | `save_system_entries()` | |
| `load_system_roms()` | `load_system_entries()` | |
| `row_to_cached_rom()` | `row_to_game_entry()` | Internal helper |
| `make_cached_rom()` (test helper) | `make_game_entry()` | |

### SQL tables/indexes

| Old | New |
|-----|-----|
| `rom_cache` table | `game_library` |
| `rom_cache_meta` table | `game_library_meta` |
| `idx_rom_cache_genre` index | `idx_game_library_genre` |

### Database file

**No change needed.** Tables live inside `metadata.db`, not a separate file.

## Migration Strategy

`game_library` is a transient cache — it rebuilds automatically when empty. No `ALTER TABLE RENAME` needed:

1. `CREATE TABLE IF NOT EXISTS game_library (...)` creates the new table
2. `spawn_cache_verification()` detects empty L2, triggers `populate_all_systems()`
3. Old tables become orphaned — clean up with `DROP TABLE IF EXISTS rom_cache` in `init()`

## Ordered Steps

### Phase 1: Core crate (`metadata_db.rs`) — ~85 changes

1. Rename SQL table in `init()`: `rom_cache` → `game_library`, `rom_cache_meta` → `game_library_meta`
2. Update `TABLES` constant
3. Rename all SQL query strings (~50 statements)
4. Rename methods: `clear_system_rom_cache` → `clear_system_game_library`, etc.
5. Update error messages, doc comments, inline comments
6. Rename test functions containing `rom_cache`
7. Add `DROP TABLE IF EXISTS rom_cache; DROP TABLE IF EXISTS rom_cache_meta;` cleanup in `init()`

### Phase 2: Core crate (`game_ref.rs`) — 1 change

8. Update comment referencing `rom_cache`

### Phase 3: App crate (`cache.rs`) — ~22 changes

9. Rename `struct RomCache` → `struct GameLibrary`
10. Rename `impl RomCache` → `impl GameLibrary`
11. Update all comments
12. Update calls to renamed core methods

### Phase 4: App crate (`mod.rs`) — 3 changes

13. Update re-export, field type, constructor

### Phase 5: App crate (other files) — ~11 changes

14. `background.rs` — update comments (6)
15. `import.rs` — update comments (2)
16. `recommendations.rs` — update comments (3)

### Phase 6: "Rebuild Game Library" button

17. New server function `RebuildGameLibrary` in `metadata.rs`
18. Register in `main.rs`
19. Add button to metadata page (`pages/metadata.rs`)
20. Add i18n keys

### Phase 7: Documentation — ~124 changes across 15+ files

21. Bulk rename across all docs

### Phase 8: Verify

22. `cargo test -p replay-control-core --features metadata`
23. `cargo clippy`
24. Full build
25. `grep -ri rom_cache` to confirm zero remaining references

## "Rebuild Game Library" Button

### Server function

```rust
#[server(prefix = "/sfn")]
pub async fn rebuild_game_library() -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    state.cache.invalidate();
    state.spawn_cache_enrichment();
    Ok(())
}
```

### i18n keys

```
"metadata.rebuild_game_library" => "Rebuild Game Library"
"metadata.rebuilding_game_library" => "Rebuilding..."
"metadata.game_library_rebuilt" => "Game library rebuild started"
"metadata.confirm_rebuild_game_library" => "Rebuild the game library? This re-scans all games from disk."
```

### Metadata page reorganization

The current metadata page has all actions at the same level. Reorganize into two groups:

**Main actions** (visible by default):
- **Rebuild Game Library** — clears game_library tables, triggers full rescan + enrichment. This is the action users need when baked-in data changes (e.g., genre fix) or when they want to force a fresh scan.
- **Clear Downloaded Images** — deletes downloaded thumbnail files from disk to reclaim space. Straightforward, user-understandable.

**Advanced** (collapsed/hidden by default, toggle to reveal):
- **Clear Metadata** — deletes all `game_metadata` entries (LaunchBox imported data). Troubleshooting/developer tool — normal users should never need this.
- **Clear Thumbnail Index** — deletes the `thumbnail_index` table (manifest of available covers). Troubleshooting/developer tool — forces re-fetch of the manifest on next "Update Images."

Implementation:
- Wrap the advanced actions in a `<details>` / `<summary>` or a `<Show>` with a toggle signal
- Add i18n key: `"metadata.advanced_actions" => "Advanced"`
- Keep the same `ClearActionCard` pattern for all four actions, just grouped differently

This separation makes it clear which actions are safe for everyday use vs which are escape hatches.

## Risk Areas

| Risk | Mitigation |
|------|------------|
| Old `metadata.db` on deployed devices | `init()` creates new tables, `spawn_cache_verification` repopulates. `DROP TABLE IF EXISTS` cleans up old tables. |
| `probe_tables()` check | `init()` runs before probe — new tables exist before check. Safe. |
| Serialized data | `CachedRom` is internal, never serialized over the wire. Safe. |
| `AppState.cache` field name | Stays `cache` — generic enough, no rename needed. |
