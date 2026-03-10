# Game Videos Feature — Implementation Plan

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
- Optional: configurable Piped instance URL in `.replay-control/config.cfg` (`piped_api_url`)
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
| `replay-control-app/Cargo.toml` | Add `reqwest` (SSR-only) |
| `replay-control-app/src/pages/game_detail.rs` | Replace Videos placeholder with full component |
| `replay-control-app/src/server_fns.rs` | 4 new server functions + `VideoRecommendation` type |
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
