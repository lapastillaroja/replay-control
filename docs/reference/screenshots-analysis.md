# Screenshots Feature — Analysis

> **Status**: Implemented. See `replay-control-core/src/screenshots.rs` and the "Your Captures" section in `replay-control-app/src/pages/game_detail.rs`.

How to surface RePlayOS screenshots in the Replay Control app.

## Background

RePlayOS stores screenshots as PNG files in `{storage_root}/captures/`. The directory mirrors the ROM system folder structure, with per-system subdirectories (e.g., `captures/sega_smd/`, `captures/arcade_fbneo/`).

**Naming convention:** `{rom_filename}_{YYYYMMDD}_{HHMMSS}.png`

Examples:
- `captures/sega_smd/Sonic The Hedgehog 2 (World) (Rev A).md_20260310_015805.png`
- `captures/arcade_fbneo/chelnov.zip_20260310_015833.png`
- `captures/mslug6.zip.png` (older format, no timestamp, in root captures dir)

Screenshot files are small (7-19 KB) — retro resolution PNGs from emulated systems.

The `captures/` directory also contains `_favorites`, `_recent`, `_extra` subdirectories used internally by RePlayOS (should be skipped when scanning).

---

## 1. Screenshot Discovery

### Matching screenshots to a ROM

Given a ROM with `system = "sega_smd"` and `rom_filename = "Sonic The Hedgehog 2 (World) (Rev A).md"`, find screenshots by scanning `captures/sega_smd/` for files whose name starts with `Sonic The Hedgehog 2 (World) (Rev A).md`.

Algorithm:

```
fn find_screenshots(storage, system, rom_filename) -> Vec<Screenshot>:
    let dir = storage.captures_dir().join(system)
    if !dir.exists(): also check storage.captures_dir() (root fallback)
    for each .png file in dir:
        if file.stem starts with rom_filename:
            parse timestamp from suffix
            add to results
    sort by timestamp descending (newest first)
```

The match is a **prefix match**: the screenshot filename starts with the exact ROM filename, followed by either `_YYYYMMDD_HHMMSS.png` or just `.png`.

### Parsing the timestamp

Strip the ROM filename prefix from the screenshot filename, then parse the remainder:

```
screenshot: "Sonic The Hedgehog 2 (World) (Rev A).md_20260310_015805.png"
rom_filename: "Sonic The Hedgehog 2 (World) (Rev A).md"
suffix: "_20260310_015805.png"
```

Regex for the timestamp suffix: `_(\d{4})(\d{2})(\d{2})_(\d{2})(\d{2})(\d{2})\.png$`

If no timestamp is present (suffix is just `.png`), use the file's filesystem modification time as fallback.

### Edge cases

1. **ROM filenames with special characters** — parentheses, brackets, spaces, ampersands are all common in No-Intro names (e.g., `Super Mario Bros. 3 (USA) (Rev A).nes`). The prefix match handles this naturally since we compare literal strings, not patterns.

2. **Screenshots in root `captures/` directory** — some screenshots (especially older ones) land directly in `captures/` without a system subdirectory. When looking up screenshots for a ROM, check both `captures/{system}/` and `captures/` (root). The root screenshots lack system context, so match by filename prefix only.

3. **Multiple screenshots per game** — a game may have many screenshots. Return all of them, sorted by timestamp.

4. **ROM renamed after screenshot taken** — if the user renames a ROM, the old screenshot filenames still reference the original name. These screenshots become orphaned (no match). Acceptable for now; a future "screenshot cleanup" feature could address this.

5. **Overlapping filename prefixes** — unlikely but possible: `game.zip` and `game.zip2` would both match screenshots starting with `game.zip`. Mitigated by requiring the character after the ROM filename to be either `_` (timestamp separator) or `.` (direct `.png`).

### Screenshot type

```rust
pub struct Screenshot {
    /// System folder (e.g., "sega_smd"), or empty if in root captures dir
    pub system: String,
    /// Screenshot filename (e.g., "Sonic...md_20260310_015805.png")
    pub filename: String,
    /// ROM filename this screenshot belongs to
    pub rom_filename: String,
    /// Timestamp extracted from filename, or file mtime. Seconds since epoch.
    pub timestamp: u64,
}
```

---

## 2. Serving Screenshots

Screenshots live on the Pi's filesystem and need to be served to the browser over HTTP.

### Option A: Static file serving with `ServeDir`

Mount the entire `captures/` directory under a URL prefix:

```rust
.nest_service("/captures", ServeDir::new(storage.captures_dir()))
```

URL: `/captures/sega_smd/Sonic...md_20260310_015805.png`

**Pros:** Zero code, leverages tower-http, automatic Content-Type, supports Range requests.
**Cons:** Exposes the full directory structure. URLs contain raw filenames with spaces and special characters (needs URL encoding). No access control. The storage root can change at runtime (USB hot-swap), so the `ServeDir` path would need to be re-mounted or use a dynamic handler.

### Option B: Server function returning screenshot bytes

```rust
#[server]
pub async fn get_screenshot(system: String, filename: String) -> Result<Vec<u8>, ServerFnError>
```

**Pros:** Full control over access and validation.
**Cons:** Server functions are designed for serialized data, not binary blobs. Would need base64 encoding or custom response handling. Inefficient for images. No caching headers.

### Option C: Dedicated API handler (recommended)

Add an axum handler that reads and serves the file with proper HTTP headers:

```rust
// Route: /api/screenshots/:system/:filename
async fn serve_screenshot(
    State(state): State<AppState>,
    Path((system, filename)): Path<(String, String)>,
) -> impl IntoResponse {
    let path = state.storage().captures_dir().join(&system).join(&filename);
    // Fallback: check root captures dir if not found in system subdir
    // Return the file with Content-Type: image/png and Cache-Control headers
}
```

**Pros:**
- Proper HTTP semantics (Content-Type, Cache-Control, ETag).
- Dynamic storage root (reads from `AppState` on each request).
- Can add validation (filename sanitization, path traversal prevention).
- Can set aggressive caching headers — screenshots are immutable once created.
- Clean URL structure: `/api/screenshots/sega_smd/filename.png`.

**Cons:** Small amount of handler code to write.

**Recommendation:** Option C. A dedicated `/api/screenshots/:system/:filename` route gives us proper caching, security, and dynamic storage support. The handler is straightforward (read file, return with headers) and follows the existing pattern of REST API routes under `/api/`.

For screenshots in the root `captures/` directory (no system subdirectory), use a special system value like `_root` in the URL: `/api/screenshots/_root/mslug6.zip.png`.

### Caching headers

Screenshots are immutable — once created, they never change. Set aggressive cache headers:

```
Cache-Control: public, max-age=31536000, immutable
```

This means the browser caches each screenshot for up to a year and never revalidates. Since each screenshot has a unique filename (with timestamp), cache busting is not needed.

### Image optimization

Given the small file sizes (7-19 KB, retro resolution), serving PNGs as-is is the right approach. Transcoding or resizing would add complexity for negligible benefit. PNG is already lossless and these are small pixel-art-style images where JPEG would actually increase size or introduce artifacts.

---

## 3. Game Detail Integration

### Data flow

The `get_rom_detail` server function currently returns `RomDetail` with `GameInfo`, `size_bytes`, `is_m3u`, and `is_favorite`. Add screenshot information:

```rust
pub struct RomDetail {
    pub game: GameInfo,
    pub size_bytes: u64,
    pub is_m3u: bool,
    pub is_favorite: bool,
    pub screenshots: Vec<ScreenshotInfo>,  // NEW
}

/// Lightweight screenshot reference for the client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenshotInfo {
    /// URL to fetch the screenshot image
    pub url: String,
    /// Timestamp (seconds since epoch), for display and sorting
    pub timestamp: u64,
}
```

Inside `get_rom_detail`, after resolving game info, scan for screenshots:

```rust
let screenshots = find_screenshots_for_rom(&storage, &system, &filename)
    .into_iter()
    .map(|s| ScreenshotInfo {
        url: format!("/api/screenshots/{}/{}", s.system_or_root(), urlencoding::encode(&s.filename)),
        timestamp: s.timestamp,
    })
    .collect();
```

The `ScreenshotInfo` contains pre-built URLs so the client never needs to construct them.

A mirror type for `ScreenshotInfo` goes in `types.rs` as usual.

### UI: Screenshot gallery on game detail page

Replace the current placeholder section in `game_detail.rs`:

```rust
// Current:
<section class="section game-section">
    <h2 class="game-section-title">"Screenshots"</h2>
    <p class="game-section-empty">"No screenshots available"</p>
</section>

// Proposed:
<section class="section game-section">
    <h2 class="game-section-title">"Screenshots"</h2>
    <Show when=move || !screenshots.is_empty()
          fallback=|| view! { <p class="game-section-empty">"No screenshots available"</p> }>
        <div class="screenshot-gallery">
            <For each=move || screenshots.clone()
                 key=|s| s.url.clone()
                 let:screenshot>
                <img class="screenshot-thumb"
                     src=screenshot.url
                     loading="lazy"
                     alt="Screenshot" />
            </For>
        </div>
    </Show>
</section>
```

Gallery layout: horizontal scrollable row (overflow-x: auto) at the same width as the other sections. Each thumbnail displays at native resolution (retro screenshots are small, so they look crisp at 1x or 2x). Tapping a thumbnail could open it in a simple fullscreen overlay (phase 2).

### Lazy loading

Use `loading="lazy"` on `<img>` elements. Since screenshots are below the fold (after metadata), they will only load when the user scrolls to them. Given the small file sizes, this is a minor optimization but still good practice.

---

## 4. Screenshots Gallery Page

### Route and navigation

- **Route:** `/screenshots` (or `/more/screenshots` if preferred, but a top-level route is cleaner since it's a browsing feature)
- **Navigation:** Add a menu item in the More page, similar to existing items (Wi-Fi, NFS, Hostname, Skin)
- **Page component:** `ScreenshotsPage` in `pages/screenshots.rs`

### Data loading

Server function:

```rust
#[server]
pub async fn get_all_screenshots(
    offset: usize,
    limit: usize,
) -> Result<ScreenshotsPage, ServerFnError> {
    // Scan captures/ and all system subdirectories
    // Return paginated list with total count
}

pub struct ScreenshotsPage {
    pub screenshots: Vec<ScreenshotEntry>,
    pub total: usize,
    pub has_more: bool,
}

pub struct ScreenshotEntry {
    pub url: String,
    pub system: String,
    pub system_display: String,
    pub rom_filename: String,
    pub display_name: Option<String>,
    pub timestamp: u64,
}
```

Each `ScreenshotEntry` carries enough info to display the screenshot with context (game name, system) and link to the game detail page.

### Layout

Group screenshots by system, with each system section showing:
- System display name as header
- Grid of screenshot thumbnails
- Each thumbnail shows the game name and timestamp below it
- Clicking the thumbnail opens it; clicking the game name navigates to game detail

Pagination: use the same infinite-scroll pattern as the ROM list (IntersectionObserver sentinel + "Load more" fallback).

### Delete functionality

Each screenshot gets a delete button (small trash icon). On click, show a confirmation step (same pattern as ROM delete). Server function:

```rust
#[server]
pub async fn delete_screenshot(system: String, filename: String) -> Result<(), ServerFnError> {
    let path = state.storage().captures_dir().join(&system).join(&filename);
    // Validate path, delete file
}
```

Use optimistic UI: remove the screenshot from the list immediately, call the server function in the background.

---

## 5. Performance Considerations

### Directory scanning

The `captures/` directory could grow large over time (hundreds or thousands of screenshots). Each scan calls `read_dir` on the captures root and each system subdirectory.

**Current request path for game detail:** `get_rom_detail` would scan one system subdirectory (or two, including root fallback) to find screenshots for a single game. A directory with, say, 200 screenshots for one system scans quickly — this is comparable to ROM listing which already handles thousands of files.

**Screenshots gallery page:** Scans ALL screenshots across all systems. This is the more expensive path, but `read_dir` is fast for thousands of entries on local storage. On NFS, latency could be higher.

### Caching

Follow the existing `GameLibrary` pattern. Add a `ScreenshotCache` (or extend `GameLibrary`) that caches per-system screenshot lists with the same TTL-based expiration:

```rust
pub struct ScreenshotCache {
    /// system -> list of screenshots
    by_system: RwLock<HashMap<String, CacheEntry<Vec<Screenshot>>>>,
    /// All screenshots (for gallery page)
    all: RwLock<Option<CacheEntry<Vec<Screenshot>>>>,
}
```

The cache is invalidated when a screenshot is deleted. No need to watch for new screenshots in real time — the 30-second TTL handles it naturally (screenshots are taken on the Pi while playing, not from the web UI).

### Thumbnail generation

Not needed for the initial implementation. Screenshots are 7-19 KB PNGs at retro resolution (240p-480p). They are already small enough to serve as-is, even in a gallery grid. A 100-screenshot gallery page would transfer ~1-2 MB total, which is fine on a local network.

If needed later, thumbnails could be generated on first access and cached to disk (e.g., `captures/.thumbs/`). But this adds significant complexity (image processing dependency, disk I/O, cache management) for marginal benefit.

---

## 6. Implementation Plan

### Phase 1: Screenshots on game detail page

- Add `find_screenshots_for_rom()` function in `replay-control-core` (in a new `screenshots.rs` module)
- Add `ScreenshotInfo` to `RomDetail` and populate it in `get_rom_detail`
- Add `/api/screenshots/:system/:filename` route handler in `replay-control-app/src/api/`
- Replace the placeholder screenshots section in `game_detail.rs` with the gallery component
- Add CSS for `.screenshot-gallery` (horizontal scroll, thumbnail sizing)

### Phase 2: Screenshots gallery page

- Add `get_all_screenshots` server function with pagination
- Create `ScreenshotsPage` component at `/screenshots` route
- Add menu item in the More page
- Add navigation link in `lib.rs` routes

### Phase 3: Delete and management

- Add `delete_screenshot` server function
- Add delete UI on the gallery page (confirmation step, optimistic removal)
- Add delete option on individual screenshots in game detail view
- Invalidate screenshot cache on delete

---

## 7. Estimated Complexity

| Component | Location | Effort |
|---|---|---|
| `screenshots.rs` module (discovery, parsing) | `replay-control-core/src/` | Small — ~100 lines. `read_dir` + prefix match + regex timestamp parse. |
| Screenshot serving handler | `replay-control-app/src/api/screenshots.rs` | Small — ~50 lines. Read file, return with headers. Similar to existing API handlers. |
| `ScreenshotInfo` on `RomDetail` | `server_fns.rs`, `types.rs` | Trivial — add field + mirror type. |
| Screenshot gallery in game detail | `pages/game_detail.rs`, `style.css` | Small — replace placeholder with `<For>` loop + `<img>` elements. ~20 lines Rust, ~30 lines CSS. |
| Screenshots gallery page | `pages/screenshots.rs` | Medium — new page with server function, pagination, grouped layout. ~200 lines Rust, ~50 lines CSS. Similar complexity to favorites page. |
| Delete screenshot | `server_fns.rs`, `screenshots.rs`, gallery UI | Small — server function + confirmation UI pattern (reuse existing pattern). |
| Screenshot cache | `api/mod.rs` | Small — extend existing cache pattern. |
| Route + nav + registration | `lib.rs`, `main.rs`, `more.rs` | Trivial — add route, menu item, register server function. |

**Total estimate:** ~500 lines of new Rust code, ~80 lines of CSS. No new dependencies needed. The `regex` crate is already in use for other features; `tokio::fs` is already available for async file reads.

The heaviest piece is the gallery page (Phase 2), which follows established patterns (paginated list, server function, infinite scroll). Phase 1 (screenshots on game detail) is straightforward and self-contained.
