# Game Videos Feature — Implementation Plan

> **Status:** Implemented. All core features (paste URLs, search via Piped/Invidious, pin/remove, embedded playback, duplicate detection) are live. Video storage migrated from `videos.json` to `game_videos` table in `user_data.db` (2026-03-14). See `replay-control-core/src/capture/video_url.rs`, `replay-control-core/src/metadata/user_data_db.rs`, `replay-control-app/src/server_fns/videos.rs`, and the Videos section in `replay-control-app/src/pages/game_detail.rs`.

## Overview

Add video support to the game detail page. Users can paste video links, browse auto-recommended trailers and gameplay videos, and pin videos for later viewing. Video data is stored separately from auto-generated metadata to survive metadata clears.

---

## Storage

**File**: `<storage>/.replay-control/videos.json`

Separate from `metadata.db`. JSON format — data is small (hundreds of entries max at ~200 bytes each), simple access pattern, trivially portable and debuggable.

**Schema**:
```rust
struct VideoEntry {
    id: String,                    // hash of canonical URL
    url: String,                   // sanitized canonical URL
    platform: String,              // "youtube", "twitch", "vimeo", "dailymotion"
    video_id: String,              // platform-specific video ID
    title: Option<String>,         // from user or search results
    added_at: u64,                 // unix timestamp
    from_recommendation: bool,     // pinned from search vs manually pasted
    tag: Option<String>,           // "trailer", "gameplay", or None (manual)
}

struct GameVideos {
    games: HashMap<String, Vec<VideoEntry>>,  // key: "{system}/{rom_filename}"
}
```

**Write safety**: Atomic writes (write `.tmp` then `rename`). Mutex-guarded in AppState (same pattern as metadata_db).

---

## Supported Platforms

| Platform | URL patterns | Embed domain | Privacy |
|----------|-------------|--------------|---------|
| YouTube | `watch?v=`, `youtu.be/`, `shorts/`, `embed/`, `m.youtube.com` | `youtube-nocookie.com` | No tracking cookies |
| Twitch | `twitch.tv/videos/`, `clips.twitch.tv/` | `player.twitch.tv` | Needs `parent` param |
| Vimeo | `vimeo.com/{id}` | `player.vimeo.com` | Standard |
| Dailymotion | `dailymotion.com/video/{id}` | `dailymotion.com/embed` | Standard |

Unrecognized URLs are rejected with a clear error — no arbitrary iframe embedding.

---

## URL Parsing & Sanitization

New module: `replay-control-core/src/video_url.rs`

- Extract video ID only, strip ALL tracking params (`si=`, `list=`, `utm_*`, `fbclid=`, `gclid=`, `feature=`, `index=`, `t=`, etc.)
- Store canonical URL, compute embed URL from video ID
- Use `url` crate for proper URL parsing (new dependency on core)

**YouTube canonical**: `https://www.youtube.com/watch?v={VIDEO_ID}`
**YouTube embed**: `https://www.youtube-nocookie.com/embed/{VIDEO_ID}`

---

## UI Layout on Game Detail Page

The Videos section replaces the static "No videos available" placeholder with three subsections:

### 1. My Videos (always visible)
- List of saved videos (pasted + pinned) as embedded iframes
- Each video has an "x" remove button (top-right overlay)
- Responsive 16:9 iframes with `loading="lazy"`
- Only 2-3 shown initially, "Show all" button for more
- **Add input**: text field + "Add" button below the list
  - Placeholder: "Paste a YouTube or Twitch URL..."
  - Inline error for invalid URLs
  - Input clears on success, new video appears at top

### 2. Trailers (loaded on demand)
- **"Find Trailers" button** — user must click to search
- Shows promotional/official trailers for game discovery
- Search query: `"{normalized_title} {system} official trailer"`
- Results show: thumbnail + title + channel + duration + "Pin" button
- Pinned trailers go to "My Videos" with `tag: "trailer"`

### 3. Gameplay (loaded on demand)
- **"Find Gameplay" button** — user must click to search
- Shows gameplay footage for reference
- Search query: `"{normalized_title} {system} gameplay"`
- Same result format and pin behavior as trailers
- Pinned gameplay goes to "My Videos" with `tag: "gameplay"`

### Component hierarchy:
```
GameVideoSection
├── SavedVideoList
│   ├── VideoEmbed (iframe + remove button)  ×N
│   └── AddVideoInput (text field + Add button)
├── TrailerSearch (button → results panel)
│   └── RecommendationItem (thumbnail + title + Pin)  ×N
└── GameplaySearch (button → results panel)
    └── RecommendationItem (thumbnail + title + Pin)  ×N
```

---

## Video Recommendations

### Search API: Piped
- **Endpoint**: `GET https://pipedapi.kavin.rocks/search?q={QUERY}&filter=videos`
- Privacy-respecting YouTube frontend, no API key needed
- Returns: video titles, IDs, thumbnail URLs, durations, channel names

### Title Normalization
```
display_name → strip parenthesized tags "(USA)", "(World 910522)" → clean title
```

### System Label Mapping
- `arcade_fbneo`, `arcade_mame`, `arcade_mame_2k3p`, `arcade_dc` → **"arcade"**
- All other systems → use system display name (e.g., "Super Nintendo")

### Search Queries
- **Trailers**: `"{clean_title} {system_label} official trailer"`
- **Gameplay**: `"{clean_title} {system_label} gameplay"`

### Fallback
- If Piped is down → graceful error message, manual paste always works
- Optional: configurable Piped instance URL in `.replay-control/settings.cfg` (`piped_api_url`)
- Brief in-memory cache (5 min per query) to reduce API calls

---

## Server Functions

| Function | Purpose |
|----------|---------|
| `GetGameVideos(system, rom_filename)` | Load saved videos for a game |
| `AddGameVideo(system, rom_filename, url, title, from_recommendation, tag)` | Add a video (paste or pin) |
| `RemoveGameVideo(system, rom_filename, video_id)` | Remove a saved video |
| `SearchGameVideos(system, display_name, query_type)` | Search recommendations (`query_type`: "trailer" or "gameplay") |

All need `register_explicit` in `main.rs`.

---

## New Files

| File | Purpose |
|------|---------|
| `replay-control-core/src/videos.rs` | JSON storage CRUD for video entries |
| `replay-control-core/src/video_url.rs` | URL parsing, sanitization, embed URL generation |

## Modified Files

| File | Changes |
|------|---------|
| `replay-control-core/src/lib.rs` | Add `pub mod videos; pub mod video_url;` |
| `replay-control-core/Cargo.toml` | Add `url = "2"` |
| `replay-control-app/Cargo.toml` | ~~Add `reqwest` (SSR-only)~~ — uses `curl_get_json()` instead (reqwest was removed) |
| `replay-control-app/src/pages/game_detail.rs` | Replace Videos placeholder with full component |
| `replay-control-app/src/server_fns/videos.rs` | 4 server functions + `VideoRecommendation` type |
| `replay-control-app/src/main.rs` | 4 `register_explicit` calls |
| `replay-control-app/src/i18n.rs` | ~15 new keys |
| `replay-control-app/style/style.css` | Video embed, recommendation panel styles |
| `docs/reference/replay-control-folder.md` | Document `videos.json` |

---

## i18n Keys

| Key | English |
|-----|---------|
| `game_detail.my_videos` | `"My Videos"` |
| `game_detail.add_video` | `"Add"` |
| `game_detail.add_video_placeholder` | `"Paste a YouTube or Twitch URL..."` |
| `game_detail.add_video_error` | `"Invalid URL. Supported: YouTube, Twitch, Vimeo."` |
| `game_detail.add_video_duplicate` | `"This video is already saved."` |
| `game_detail.video_added` | `"Video added"` |
| `game_detail.remove_video` | `"Remove"` |
| `game_detail.find_trailers` | `"Find Trailers"` |
| `game_detail.find_gameplay` | `"Find Gameplay"` |
| `game_detail.searching` | `"Searching..."` |
| `game_detail.no_results` | `"No videos found"` |
| `game_detail.search_error` | `"Video search unavailable. Paste URLs directly."` |
| `game_detail.pin_video` | `"Pin"` |
| `game_detail.pinned` | `"Pinned"` |
| `game_detail.show_all_videos` | `"Show all"` |

---

## Dependencies

| Crate | Where | Purpose |
|-------|-------|---------|
| `url = "2"` | `replay-control-core` | URL parsing |
| `reqwest = { version = "0.12", features = ["json"], optional = true }` | `replay-control-app` (SSR) | Piped API HTTP calls |

---

## Edge Cases & Considerations

### Twitch `parent` parameter
Twitch embeds require a `parent` hostname matching the page's domain. Must be set dynamically from the request's `Host` header at embed render time, not stored.

### ROM renames
Renaming a ROM orphans the `videos.json` key. Videos persist but become unreachable. Acceptable for v1 — could add a migration helper later.

### ROM deletes
Does NOT clean up `videos.json` entries (harmless orphans).

### Offline Pi
Embeds show blank/error when no internet. Saved URLs persist for when internet returns. The local paste/remove functionality works offline.

### Piped API reliability
Community-maintained, could go down. Mitigations:
- Brief in-memory cache per query
- Configurable instance URL
- Manual paste always available as fallback

### Embed performance
Many iframes = slow page. Mitigations:
- `loading="lazy"` on all iframes
- Show max 2-3 initially, "Show all" button for the rest
- Consider thumbnail-only preview that opens iframe on click (future optimization)

### Security
- `sandbox="allow-scripts allow-same-origin allow-popups"` on all iframes
- Only allow known embed domains (no arbitrary URLs)
- Use `youtube-nocookie.com` for YouTube (no tracking cookies)

### NFS file locking
`videos.json` writes go through AppState Mutex (same pattern as metadata_db). Only one server process runs, so this is sufficient.

### Atomic writes
Write to `.tmp` file, then `std::fs::rename` (atomic on Linux) to prevent corruption on crash.

---

## Analysis: Should Video Storage Migrate to `user_data.db`?

> Added: 2026-03-12

With the introduction of `user_data.db` (a dedicated SQLite database for persistent user customizations — see `docs/investigations/box-art-swap.md`), the question arises: should video pinning data move from `videos.json` into `user_data.db`?

### Current `videos.json` Approach

**How it works:** A single JSON file at `<storage>/.replay-control/videos.json` stores a `HashMap<String, Vec<VideoEntry>>` keyed by `"{system}/{rom_filename}"`. Every read deserializes the entire file; every write serializes and atomically replaces the entire file. Concurrency is implicitly handled by AppState (single server process) and atomic rename.

**Pros:**
- Simple to implement — ~60 lines of Rust for the full CRUD layer (`videos.rs`)
- Human-readable and trivially debuggable: `cat videos.json | jq .`
- Zero infrastructure: no schema migrations, no `rusqlite` dependency (for this module alone)
- Portable: copy the file anywhere, open in any text editor
- Atomic writes via tmp+rename prevent corruption on crash

**Cons:**
- **Full-file rewrite on every mutation**: Adding or removing a single video serializes the entire map. With hundreds of games each having 2-3 videos, this is ~50-100 KB per write. Acceptable today, but scales linearly.
- **Full-file read on every query**: `get_game_videos()` for a single game deserializes all games' videos. The server function in `videos.rs:27` calls `get_videos()`, which calls `load_videos()`, which reads and parses the entire file. This is ~1-2 ms for typical sizes, but wasteful.
- **No indexing or querying**: Cannot answer "how many games have videos?" or "list all games with a trailer tag" without loading and scanning everything.
- **No per-field updates**: Changing a video's title requires rewriting the entire file.
- **No concurrent access safety beyond atomic rename**: If two requests mutate simultaneously (e.g., two browser tabs), one write can silently overwrite the other's changes. The current architecture has a single server process so this is not a problem today, but the pattern is inherently unsafe if the architecture ever changes.
- **Orphan cleanup is manual**: ROM renames/deletes leave orphaned entries. With JSON there is no easy way to query or clean these without loading the full file.

### SQLite in `user_data.db` Approach

**Proposed schema:**

```sql
CREATE TABLE IF NOT EXISTS game_videos (
    system TEXT NOT NULL,
    rom_filename TEXT NOT NULL,
    video_id TEXT NOT NULL,        -- "{platform}-{platform_video_id}"
    url TEXT NOT NULL,             -- sanitized canonical URL
    platform TEXT NOT NULL,        -- "youtube", "twitch", "vimeo", "dailymotion"
    platform_video_id TEXT NOT NULL, -- platform-specific ID
    title TEXT,                    -- from user or search results
    added_at INTEGER NOT NULL,     -- unix timestamp
    from_recommendation INTEGER NOT NULL DEFAULT 0,  -- boolean
    tag TEXT,                      -- "trailer", "gameplay", "1cc", or NULL (manual)
    sort_order INTEGER NOT NULL DEFAULT 0, -- for manual reordering (future)
    PRIMARY KEY (system, rom_filename, video_id)
);

-- Fast lookup: all videos for a single game (the hot path).
CREATE INDEX IF NOT EXISTS idx_game_videos_game
    ON game_videos (system, rom_filename);
```

**Pros:**
- **Point queries**: `get_game_videos()` becomes `SELECT ... WHERE system = ? AND rom_filename = ?` — reads only the relevant rows, no full-file scan.
- **Point mutations**: `add_video()` is a single `INSERT`, `remove_video()` is a single `DELETE` — no rewrite of unrelated data.
- **Duplicate detection at the DB level**: The `PRIMARY KEY (system, rom_filename, video_id)` constraint replaces the manual `list.iter().any()` duplicate check with `INSERT OR IGNORE` or a conflict clause.
- **Queryable**: Trivial to answer questions like "how many total pinned videos?" (`SELECT COUNT(*) FROM game_videos`), "which games have trailers?" (`SELECT DISTINCT system, rom_filename FROM game_videos WHERE tag = 'trailer'`), or "orphaned entries for ROMs that no longer exist" (join against ROM cache).
- **Consolidated user data**: One database (`user_data.db`) for all user customizations — box art overrides, video pins, future ratings/notes/tags. Single connection, single open/close lifecycle, single backup target.
- **Same infrastructure**: `UserDataDb` already exists with the `nolock` NFS fallback, lazy open in `AppState`, and `Mutex`-guarded access. Adding a table and a few methods is straightforward.
- **Transaction safety**: SQLite's ACID guarantees are stronger than tmp+rename for complex multi-step mutations (e.g., a future "replace all videos for a game" operation).
- **Future extensibility**: Adding columns (e.g., `sort_order` for manual reordering, `notes` for user annotations on a video) is a simple `ALTER TABLE` with a default value, no data migration needed.

**Cons:**
- **Not human-readable on disk**: Cannot `cat` and inspect. Mitigated by `sqlite3 user_data.db "SELECT * FROM game_videos"` — still easy to debug, just requires the sqlite3 CLI.
- **Schema coupling**: The `game_videos` table schema is now tied to the `UserDataDb` module. Any future field additions require either a migration or a permissive schema (both are standard SQLite patterns).
- **Marginal complexity increase**: `UserDataDb` gains ~40-50 lines for the video CRUD methods. However, this replaces the ~60-line `videos.rs` JSON module, so net code change is roughly neutral.
- **`rusqlite` is already a dependency**: No new crate needed.

### What the Migration Path Looks Like

1. **Add the `game_videos` table** to `UserDataDb::init()` (idempotent `CREATE TABLE IF NOT EXISTS`).
2. **Add CRUD methods** to `UserDataDb`: `add_video()`, `remove_video()`, `get_videos()`, `has_video()`.
3. **Update server functions** in `replay-control-app/src/server_fns/videos.rs` to use `state.user_data_db()` instead of `replay_control_core::videos::*`.
4. **Add a one-time migration**: On `UserDataDb::open()`, check if `videos.json` exists. If it does AND the `game_videos` table is empty, parse the JSON and `INSERT` all entries into SQLite. After successful migration, rename `videos.json` to `videos.json.migrated` (preserving it as a backup rather than deleting).
5. **Remove (or deprecate)** `replay-control-core/src/videos.rs` and the `VIDEOS_FILE` constant. The `video_url.rs` module (URL parsing) stays unchanged — it has no storage concerns.

The migration is safe because:
- It only runs once (when the table is empty and the JSON file exists)
- The original JSON file is preserved as a backup
- If the migration fails mid-way, the next open retries (table is still empty)
- The video data set is small (hundreds of entries at most), so the migration completes in milliseconds

### Should Videos Consolidate into `user_data.db`?

**Yes.** The case is clear:

1. **Videos are user data, not cache data.** They represent deliberate user choices (pasting URLs, pinning recommendations) and must survive metadata clears — exactly the same durability requirement as box art overrides. They belong in the same logical home.

2. **The JSON approach was the right call when `videos.json` was the only user data file.** The original plan document correctly noted: "data is small, simple access pattern, trivially portable." All true. But now that `user_data.db` exists for box art overrides (and will host future ratings, notes, and tags), maintaining a separate JSON file for one feature creates an inconsistency. Two different persistence mechanisms for the same category of data (user customizations) means two sets of concurrency patterns, two backup strategies, and two mental models.

3. **The performance difference is negligible at current scale but favors SQLite at any future scale.** A collection with 500 games averaging 2 videos each means 1,000 entries. The JSON file would be ~200 KB, fully parsed on every `get_game_videos()` call. SQLite returns the 2-3 relevant rows directly. Neither approach is slow, but SQLite does less pointless work.

4. **The migration is trivial and safe.** One-time JSON-to-SQLite import, original file preserved as backup, zero user-facing impact.

5. **Consolidation reduces the number of files the user needs to understand and back up.** "Back up `user_data.db` to preserve all your customizations" is simpler than "back up `user_data.db` AND `videos.json`."

### Recommendation

**Migrate video storage to `user_data.db`.** Do it alongside or shortly after the box art swap feature ships, not as a prerequisite. The JSON approach works today and is not blocking anything. But every new user-data feature that goes into `user_data.db` while videos remain in JSON increases the inconsistency cost.

**Implementation priority:** Low-medium. The box art swap feature is the forcing function that creates `user_data.db`. Once that lands and is stable, the video migration is a clean follow-up task (estimated: 1-2 hours of focused work).

### Updated `.replay-control/` Directory After Migration

```
.replay-control/
    settings.cfg               # App-specific settings (key=value)
    metadata.db                # Cache: game metadata, thumbnail index, rom cache
    user_data.db               # User customizations: box art overrides, video pins, future ratings/notes
    videos.json.migrated       # Backup of original JSON (safe to delete after confirming migration)
    launchbox-metadata.xml     # LaunchBox XML dump
    media/                     # Downloaded images
    tmp/                       # Cached git clones
```

The key invariant from the box art swap plan remains: **`metadata.db` is a cache that can be rebuilt. `user_data.db` contains user choices that cannot be reconstructed.**
