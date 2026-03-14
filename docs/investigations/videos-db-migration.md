# Investigation: Migrate `videos.json` to `user_data.db`

> **Date:** 2026-03-14
> **Status:** Implemented (2026-03-14)
> **Prior art:** An analysis section already exists in `docs/reference/game-videos-plan.md` (added 2026-03-12, "Analysis: Should Video Storage Migrate to `user_data.db`?"). This document consolidates findings and adds implementation detail.

---

## Current Behavior

**Storage file:** `<storage>/.replay-control/videos.json`
**Source code:** `replay-control-core/src/capture/videos.rs` (~97 lines)
**URL parsing:** `replay-control-core/src/capture/video_url.rs` (unaffected by migration)

The `GameVideos` struct is a `HashMap<String, Vec<VideoEntry>>` keyed by `"{system}/{rom_filename}"`. Every operation (read a single game's videos, add one video, remove one video) deserializes the entire file and, for mutations, re-serializes and atomically replaces it.

### Data shape (`VideoEntry`)

| Field | Type | Description |
|---|---|---|
| `id` | `String` | `"{platform}-{video_id}"` |
| `url` | `String` | Sanitized canonical URL |
| `platform` | `String` | `"youtube"`, `"twitch"`, `"vimeo"`, `"dailymotion"` |
| `video_id` | `String` | Platform-specific ID |
| `title` | `Option<String>` | From user or search results |
| `added_at` | `u64` | Unix timestamp |
| `from_recommendation` | `bool` | Pinned from search vs manually pasted |
| `tag` | `Option<String>` | `"trailer"`, `"gameplay"`, `"1cc"`, or `None` (manual) |

### Access patterns

| Operation | Frequency | Current cost |
|---|---|---|
| `get_videos(game_key)` | Every game detail page load | Full file parse (~1-2 ms for typical sizes) |
| `add_video(game_key, entry)` | User pastes URL or pins recommendation | Full parse + full rewrite |
| `remove_video(game_key, video_id)` | User removes a saved video | Full parse + full rewrite |

### Concurrency

Single-process server with `Mutex`-guarded AppState. No concurrent write risk today, but the JSON pattern is inherently unsafe if architecture changes (e.g., multiple workers).

---

## Proposed Schema (in `user_data.db`)

```sql
CREATE TABLE IF NOT EXISTS game_videos (
    system TEXT NOT NULL,
    rom_filename TEXT NOT NULL,
    video_id TEXT NOT NULL,           -- "{platform}-{platform_video_id}"
    url TEXT NOT NULL,                -- sanitized canonical URL
    platform TEXT NOT NULL,           -- "youtube", "twitch", "vimeo", "dailymotion"
    platform_video_id TEXT NOT NULL,  -- platform-specific ID
    title TEXT,                       -- from user or search results
    added_at INTEGER NOT NULL,        -- unix timestamp
    from_recommendation INTEGER NOT NULL DEFAULT 0,  -- boolean
    tag TEXT,                         -- "trailer", "gameplay", "1cc", or NULL
    sort_order INTEGER NOT NULL DEFAULT 0,  -- for future manual reordering
    PRIMARY KEY (system, rom_filename, video_id)
);

CREATE INDEX IF NOT EXISTS idx_game_videos_game
    ON game_videos (system, rom_filename);
```

### Why `user_data.db` and not `metadata.db`?

Videos are user data (deliberate choices: pasting URLs, pinning recommendations), not cached data. They must survive metadata clears. This matches the same durability contract as `box_art_overrides`, which already lives in `user_data.db`.

A separate database was briefly considered but rejected: `user_data.db` already exists with the `nolock` NFS fallback, lazy open, and `Mutex` guard. Adding a table reuses all of that infrastructure. One backup target instead of two.

---

## Migration Strategy

1. **Add `game_videos` table** to `UserDataDb::init()` (idempotent `CREATE TABLE IF NOT EXISTS`).

2. **Update `UserDataDb::TABLES`** const to include `"game_videos"` for corruption probing.

3. **Add CRUD methods** to `UserDataDb`:
   - `get_game_videos(system, rom_filename) -> Vec<VideoEntry>`
   - `add_game_video(system, rom_filename, entry) -> Result<()>` (uses `INSERT OR IGNORE` for duplicate detection)
   - `remove_game_video(system, rom_filename, video_id) -> Result<()>`
   - `has_game_video(system, rom_filename, video_id) -> bool` (optional, for UI)

4. **One-time JSON import** in `UserDataDb::open()`:
   - If `videos.json` exists AND `game_videos` table is empty:
     - Parse JSON, `INSERT` all entries
     - Rename `videos.json` to `videos.json.migrated`
   - Safe: only runs once, original preserved as backup, retries if interrupted (table still empty)
   - Dataset is small (hundreds of entries max), completes in milliseconds

5. **Update server functions** in `replay-control-app/src/server_fns/videos.rs`:
   - Replace `replay_control_core::videos::get_videos()` with `state.user_data_db().get_game_videos()`
   - Replace `replay_control_core::videos::add_video()` with `state.user_data_db().add_game_video()`
   - Replace `replay_control_core::videos::remove_video()` with `state.user_data_db().remove_game_video()`
   - The `VideoEntry` struct moves from `capture/videos.rs` to `metadata/user_data_db.rs` (or a shared types module)
   - The `VideoRecommendation` type and search functions are unaffected

6. **Update client-side type import**: `replay-control-app/src/server_fns/videos.rs` re-exports `VideoEntry` for SSR vs client builds. The import path changes from `replay_control_core::videos::VideoEntry` to the new location.

7. **Remove or deprecate** `replay-control-core/src/capture/videos.rs`. The `video_url.rs` module stays (URL parsing has no storage concerns). Remove `VIDEOS_FILE` constant from `storage.rs`.

8. **Update docs**: `replay-control-folder.md` directory listing, `game-videos-plan.md` status.

---

## Files Touched

| File | Change |
|---|---|
| `replay-control-core/src/metadata/user_data_db.rs` | Add `game_videos` table, CRUD methods, JSON migration |
| `replay-control-app/src/server_fns/videos.rs` | Switch from `videos::*` to `user_data_db.*` calls |
| `replay-control-core/src/capture/videos.rs` | Delete (or keep as deprecated) |
| `replay-control-core/src/capture/mod.rs` | Remove `pub mod videos;` |
| `replay-control-core/src/platform/storage.rs` | Remove `VIDEOS_FILE` constant |
| `docs/reference/replay-control-folder.md` | Update directory listing |
| `docs/reference/game-videos-plan.md` | Update status |

---

## Benefits

1. **Point queries instead of full-file scans**: `SELECT ... WHERE system = ? AND rom_filename = ?` reads only relevant rows.
2. **Point mutations**: `INSERT` / `DELETE` one row instead of rewriting the entire file.
3. **Duplicate detection at the DB level**: `PRIMARY KEY` constraint replaces manual `list.iter().any()`.
4. **Queryable**: trivial to answer "how many games have videos?" or find orphaned entries.
5. **Consolidated user data**: single backup target (`user_data.db`) for all user customizations.
6. **Transaction safety**: SQLite ACID > tmp+rename for multi-step mutations.
7. **Extensible**: new columns via `ALTER TABLE ADD COLUMN ... DEFAULT ...` without data migration.

## Risks

1. **Not human-readable on disk**: mitigated by `sqlite3 user_data.db "SELECT * FROM game_videos"`.
2. **Migration failure on corrupt JSON**: if `videos.json` is malformed, `serde_json::from_str` returns `Err`. Handle by logging a warning and skipping migration (user loses no data since the original file is preserved).
3. **Schema coupling**: `UserDataDb` gains another table. This is acceptable -- it's purpose-built for this.

---

## Effort Estimate

**1-2 hours of focused work.** The implementation is straightforward:
- ~40-50 lines for new CRUD methods in `user_data_db.rs`
- ~15-20 lines for the migration function
- ~10 lines of changes in `server_fns/videos.rs` (swapping call sites)
- Net code change is roughly neutral (new SQLite methods replace the JSON module)

**Priority: low-medium.** The JSON approach works today. But every new user-data feature added to `user_data.db` while videos remain in JSON increases the inconsistency. Best done as a clean follow-up after box art swap is stable.
