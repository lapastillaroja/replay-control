# Performance Improvements Analysis

> **Status:** Key items implemented. WASM bundle optimized (wasm-opt + pre-compressed gzip), box art resolution cached via image index, SQLite ROM cache eliminates repeated filesystem scans. See `docs/reference/performance-benchmarks.md` for before/after measurements.

Analysis of the Replay Control app performance, with focus on Games page response time. Findings ranked by estimated impact and implementation feasibility.

---

## Executive Summary

The app has several low-hanging fruit optimizations that would dramatically improve perceived and actual performance, particularly on the Pi's constrained hardware. The three highest-impact items were:

1. **WASM bundle is 33 MB unoptimized, served uncompressed** (loading time) -- **FIXED**: `wasm-opt -Oz`, `[profile.wasm-release]`, pre-compressed `.wasm.gz`, `CompressionLayer`
2. **`resolve_box_art_url()` does per-ROM filesystem I/O in a loop** (server response time) -- **FIXED**: per-system image index cache, box art URLs stored in SQLite `rom_cache`
3. **Full ROM list is cloned from cache on every request, then filtered/paginated** (memory + CPU) -- **MITIGATED**: SQLite ROM cache with indexed queries

---

## 1. WASM Bundle Size and Delivery

**Impact: CRITICAL | Feasibility: EASY**

### Current state
- `target/site/pkg/replay_control_app_bg.wasm` is **33 MB**
- No `wasm-opt` pass in `build.sh`
- No `lto` or `opt-level = "z"` in the release profile for WASM
- No HTTP compression (no `tower_http::CompressionLayer`, no pre-compressed `.wasm.gz`/`.wasm.br`)
- The Pi's upload bandwidth is limited; 33 MB over WiFi at typical Pi speeds = multi-second download

### Recommendations

**A) Add wasm-opt to build.sh** (est. 60-80% size reduction)
```sh
wasm-opt -Oz --enable-bulk-memory -o "$PKG_DIR/${CRATE//-/_}_bg.wasm" \
  "$PKG_DIR/${CRATE//-/_}_bg.wasm"
```
This alone typically takes a 33 MB WASM file down to ~8-12 MB.

**B) Add WASM-specific release profile** in workspace `Cargo.toml`:
```toml
[profile.release.package.replay-control-app]
# Only applied when building the WASM target
opt-level = "z"   # Optimize for size
lto = true
codegen-units = 1
```
Or use a dedicated `[profile.wasm-release]` and pass `--profile wasm-release` to the hydrate build.

**C) Enable HTTP compression** via `tower_http::CompressionLayer`:
```rust
use tower_http::compression::CompressionLayer;
let app = app.layer(CompressionLayer::new());
```
This gives ~70% compression on WASM (33 MB -> ~8 MB gzip, ~6 MB brotli). Combined with wasm-opt, the final transfer size could be ~2-3 MB.

**D) Pre-compress static assets** at build time and serve them with `ServeDir::precompressed_gzip()`:
```rust
use tower_http::services::ServeDir;
ServeDir::new("target/site/pkg").precompressed_gzip()
```
This avoids compressing on every request (important on Pi's weak CPU).

**E) Consider code splitting** if Leptos supports it in the future. Currently all pages are in one WASM bundle.

---

## 2. `resolve_box_art_url()` per-ROM Filesystem I/O

**Impact: HIGH | Feasibility: MEDIUM**

### Current state
In `get_roms_page()`, after pagination, each ROM in the page slice gets:
```rust
for rom in &mut roms {
    rom.box_art_url = resolve_box_art_url(&state, &system, &rom.game.rom_filename);
}
```

`resolve_box_art_url()` does:
1. Lock metadata_db mutex
2. SQLite query (`SELECT ... WHERE system = ? AND rom_filename = ?`) -- one per ROM
3. `is_valid_image()` -- calls `std::fs::metadata()` to check file size >= 200 bytes
4. If DB miss: `find_image_on_disk()` -- calls `std::fs::read_dir()` and iterates ALL files in the boxart directory, doing `base_title()` string comparisons for fuzzy matching

For a page of 100 ROMs, this means:
- 100 metadata_db mutex lock/unlock cycles
- 100 individual SQLite queries
- 100+ `stat()` calls (one per valid DB hit, plus directory scans for misses)
- Potential `read_dir()` of the entire boxart directory (could have thousands of files) for each ROM that lacks a DB entry

On NFS, each `stat()` call can take 1-10ms. On SD card, latency is lower but still adds up.

### Recommendations

**A) Batch SQLite lookups** -- query all 100 box_art_paths in one query:
```sql
SELECT rom_filename, box_art_path FROM game_metadata
WHERE system = ?1 AND rom_filename IN (?2, ?3, ..., ?101)
```
Or use a single `SELECT ... WHERE system = ?` to get all image paths for the system and cache them in a HashMap. The metadata_db already has `system_ratings()` which does this pattern.

**B) Cache the boxart directory listing** -- read the boxart directory once per system and build an in-memory index. Currently `find_image_on_disk()` calls `read_dir()` for every ROM that doesn't have a DB path. Build a `HashMap<String, String>` (lowercase base title -> filename) and reuse it.

**C) Pre-populate box_art_url in the RomCache** -- when the cache entry is built (on `list_roms()`), also resolve box art URLs. This way the per-page cost is zero and the per-cache-miss cost is amortized.

**D) Skip is_valid_image() for most files** -- the fake-symlink check (< 200 bytes) is a workaround for exFAT clones. On the Pi, the media files were copied from the repo, not cloned directly. Consider making the check conditional on storage type, or resolving fake symlinks once at import time (which `resolve_fake_symlinks_in_dir()` already does).

---

## 3. Full Vec Clone from Cache on Every Request

**Impact: HIGH | Feasibility: MEDIUM**

### Current state
```rust
// cache.rs
fn get(&self) -> Option<&T> {
    if Instant::now() < self.expires { Some(&self.data) } else { None }
}

// But the caller does:
pub fn get_roms(...) -> Result<Vec<RomEntry>, ...> {
    if let Some(data) = entry.get() {
        return Ok(data.clone());  // <-- clones the entire Vec<RomEntry>
    }
}
```

For a system with 5,000 ROMs, each `RomEntry` contains multiple `String` fields (`rom_filename`, `rom_path`, `display_name`, `system`, `system_display`). That's ~5,000 heap allocations per request. Then `get_roms_page()` applies filters and takes a 100-item slice, discarding the other 4,900 cloned entries.

### Recommendations

**A) Use `Arc<Vec<RomEntry>>`** for the cache value. Return `Arc::clone()` instead of deep-cloning the Vec. This is nearly free (just an atomic increment). The caller already treats the data as read-only for filtering.

**B) Move filtering into the cache layer** -- provide a method like `get_roms_page(system, offset, limit, filters)` that holds the read lock, applies filters, and returns only the paginated slice. No full-collection clone needed.

**C) For search, consider a pre-built search index** instead of linear scan + scoring over all ROMs. Even a simple `Vec<(lowercase_display, lowercase_filename, index)>` that avoids re-lowercasing on every request would help.

---

## 4. Redundant `lookup_genre()` and `lookup_players()` Calls

**Impact: MEDIUM | Feasibility: EASY**

### Current state
In `get_roms_page()`, genre and player lookups happen in the filter phase (for all ROMs) and then again in the enrichment phase (for the paginated slice):

```rust
// Filter phase: for ALL pre_filtered ROMs
.filter(|r| {
    let rom_genre = lookup_genre(&system, &r.game.rom_filename);
    rom_genre.eq_ignore_ascii_case(&genre)
})
.filter(|r| {
    lookup_players(&system, &r.game.rom_filename) >= 2
})

// Enrichment phase: for just the 100 paginated ROMs
for rom in &mut roms {
    let p = lookup_players(&system, &rom.game.rom_filename);
    ...
}
```

`lookup_genre()` and `lookup_players()` both call `systems::find_system()` (PHF lookup), then either `arcade_db::lookup_arcade_game()` or `game_db::lookup_game()` + fallback to `lookup_by_normalized_title()`. For a system with 5,000 ROMs with genre filter active, that's 5,000 calls to `find_system()` + 5,000 game_db lookups, then another 100 for enrichment.

### Recommendations

**A) Pre-compute genre and players during ROM list construction** -- store them in `RomEntry` fields (they already exist: `rating`, `players`). Do this once when building the cache, not on every request.

**B) At minimum, cache the `is_arcade` flag and system lookup** -- call `find_system()` once per request, not once per ROM.

**C) For `global_search()`, the problem is worse** -- it iterates ALL systems x ALL ROMs, calling `lookup_genre()` and `lookup_players()` for every ROM across every system.

---

## 5. `mark_favorites()` Reads Filesystem on Every Page Request

**Impact: MEDIUM | Feasibility: EASY**

### Current state
```rust
pub fn mark_favorites(storage, system, roms) {
    let fav_set = list_favorites_for_system(storage, system)  // reads ALL .fav files from disk
        .into_iter().map(|f| f.game.rom_filename).collect::<HashSet>();
    for rom in roms { rom.is_favorite = fav_set.contains(&rom.game.rom_filename); }
}
```

`list_favorites_for_system()` calls `list_favorites()`, which:
1. Reads the `_favorites` directory recursively
2. For each `.fav` file: reads its content (`std::fs::read_to_string`)
3. Constructs a `GameRef` for each (which calls `game_db::game_display_name()`)
4. Then filters to the requested system

This happens on **every** `get_roms_page()` call (every page load, every search keystroke after debounce, every "load more").

### Recommendations

**A) Cache favorites in AppState** with a short TTL (5-10 seconds), similar to `RomCache`. Favorites change rarely (only when user clicks the star button, and we know exactly when that happens).

**B) Use a simpler function** that just checks `.fav` file existence (glob `{system}@*.fav`) without parsing the file contents or constructing `GameRef` objects. All we need for `mark_favorites()` is the set of filenames, not full `Favorite` structs.

**C) Invalidate the favorites cache explicitly** when `add_favorite()` or `remove_favorite()` is called, rather than relying on TTL.

---

## 6. `GameRef::new()` Does DB Lookups During ROM Scanning

**Impact: MEDIUM | Feasibility: MEDIUM**

### Current state
`collect_roms_recursive()` calls `GameRef::new()` for every ROM file, which:
1. `systems::find_system()` -- PHF lookup (fast)
2. `arcade_db::arcade_display_name()` or `game_db::game_display_name()` -- PHF lookup + potential normalized fallback
3. `rom_tags::display_name_with_tags()` -- string processing

For a system with 5,000 ROMs, this is 5,000 game_db lookups during the initial scan. Since the cache TTL is 30 seconds and scans can be triggered by any request, this CPU cost recurs frequently.

### Recommendations

**A) Increase cache TTL** from 30 seconds to something longer (e.g., 5 minutes or even until invalidated). The filesystem scan detects changes; the app knows when mutations happen (delete, rename, upload). A TTL-based approach is overly conservative.

**B) Use filesystem modification time** (`mtime` of the system directory) to invalidate the cache entry instead of a fixed TTL. Only re-scan when the directory has actually changed. This is nearly free (one `stat()` call) compared to a full directory traversal.

---

## 7. SSR Serialization Overhead

**Impact: MEDIUM | Feasibility: MEDIUM**

### Current state
`get_roms_page()` returns a `RomPage` struct that gets serialized to JSON by server_fn. Each `RomEntry` contains:
- `game: GameRef` (5 String fields: system, system_display, rom_filename, display_name, rom_path)
- `size_bytes: u64`
- `is_m3u: bool`
- `is_favorite: bool`
- `box_art_url: Option<String>`
- `driver_status: Option<String>`
- `rating: Option<f32>`
- `players: Option<u8>`

For 100 ROMs, the serialized JSON is likely 50-100 KB. On the initial SSR page load, this data is embedded in the HTML as Leptos hydration data.

### Recommendations

**A) Reduce `RomEntry` payload for list views** -- the list only needs: `rom_filename`, `display_name`, `rom_path`, `system`, `size_bytes`, `box_art_url`, `is_favorite`, `driver_status`, `rating`, `players`. The `system_display` is the same for all ROMs in a system page and is already returned in `RomPage.system_display`. Consider removing per-ROM `system` and `system_display` for the page view and adding it to the page-level metadata.

**B) Use `#[serde(skip_serializing_if = "Option::is_none")]`** on optional fields (already partially done) and `#[serde(skip_serializing_if = "is_default")]` on boolean/numeric fields that are usually their default value.

**C) Consider a more compact page size** for the first SSR render (e.g., 30-50 instead of 100). The first page determines initial load time; subsequent pages load asynchronously via infinite scroll.

---

## 8. Home Page Waterfall: Three Independent Server Functions

**Impact: MEDIUM | Feasibility: EASY**

### Current state
```rust
let info = Resource::new(|| (), |_| server_fns::get_info());
let recents = Resource::new(|| (), |_| server_fns::get_recents());
let systems = Resource::new(|| (), |_| server_fns::get_systems());
```

During SSR, these three resources resolve sequentially (Leptos SSR processes them one at a time in the reactive graph). Each involves:
- `get_info()`: cache.get_systems() + list_favorites() + disk_usage()
- `get_recents()`: reads recents file + resolve_box_art_url per recent entry
- `get_systems()`: cache.get_systems() (duplicate of get_info)

`get_systems()` and `get_info()` both call `cache.get_systems()`, so the second call hits the cache. But `list_favorites()` (called by `get_info()`) does a full filesystem scan. And `get_recents()` calls `resolve_box_art_url()` for each recent entry (same per-ROM I/O issue as #2).

### Recommendations

**A) Combine into a single `get_home_data()` server function** that returns all three in one round-trip. This eliminates duplicate cache lookups and reduces serialization overhead.

**B) During SSR, Leptos resolves resources before streaming HTML. Combining them ensures a single round of work** instead of three sequential server function calls.

---

## 9. Image Loading on ROM List

**Impact: MEDIUM | Feasibility: EASY**

### Current state
- ROM thumbnails use `loading="lazy"` -- good
- Thumbnail images are PNG files served from disk with `cache-control: public, max-age=86400` (24h)
- No `width`/`height` attributes on `<img>` -- browser can't reserve layout space before image loads, causing layout shifts
- No responsive image sizes -- full-resolution PNGs (libretro-thumbnails are typically 256x256 or larger) served for 40x40px display

### Recommendations

**A) Add explicit `width` and `height` attributes** to thumbnail `<img>` elements:
```rust
<img class="rom-thumb" src=... loading="lazy" width="56" height="40" />
```
This eliminates Cumulative Layout Shift (CLS) and lets the browser allocate space immediately.

**B) Generate smaller thumbnail versions** during the image import process. Resize to 120x120 or 80x80 for list views. A 40px display needs at most an 80px image (for 2x displays). This would reduce image file sizes from ~30-50 KB each to ~3-5 KB, dramatically reducing bandwidth for a page of 100 thumbnails.

**C) Consider WebP conversion** during import. WebP is typically 30-50% smaller than PNG for thumbnails.

**D) Use `srcset` for responsive images** if you generate multiple sizes:
```html
<img srcset="thumb-80.webp 80w, thumb-160.webp 160w" sizes="56px" ... />
```

---

## 10. CSS Delivery

**Impact: LOW-MEDIUM | Feasibility: EASY**

### Current state
- CSS is concatenated from 17 files at build time and served as `include_str!` (embedded in binary)
- No minification
- All CSS loads on every page (no code splitting)
- The CSS is relatively small (likely < 20 KB), so this is lower priority

### Recommendations

**A) Minify CSS at build time** (e.g., `lightningcss` or a simple whitespace removal in the build script).

**B) Add `Cache-Control` header to `/style.css` response** -- currently it's served without caching headers. Add `Cache-Control: public, max-age=604800` (1 week) with a content-hash in the URL for cache busting.

---

## 11. Pi-Specific Considerations

**Impact: VARIES | Feasibility: VARIES**

### A) NFS Latency Amplification
Every `stat()`, `read_dir()`, and `read()` call over NFS incurs network latency (typically 1-10ms per call). The current code has many per-ROM filesystem calls that are fine for local SD but brutal over NFS. The recommendations in #2, #5, and #6 above address this by reducing filesystem calls.

### B) SD Card I/O
SD card random read performance is poor (1-5 MB/s for random 4K reads). Sequential reads are much faster. The `read_dir()` pattern (which issues many small reads for directory entries) is particularly slow. Caching directory listings and reducing filesystem traversals helps here too.

### C) Memory Pressure
The Pi 4 has 1-4 GB RAM. Cloning large Vec<RomEntry> (issue #3) wastes memory. Using `Arc` for shared data and reducing per-request allocations helps stay within memory limits.

### D) CPU: Single-Threaded Performance
The Pi 4's Cortex-A72 cores are relatively slow for single-threaded work. The search scoring in `search_score()` does string operations (lowercase, substring search) on every ROM for every search query. Pre-computing lowercase versions in the cache would help.

### E) Consider `tokio::task::spawn_blocking`
The ROM scanning and filesystem operations in server functions currently run on the async tokio runtime. Heavy filesystem I/O (especially over NFS) blocks the async executor. Wrapping blocking filesystem work in `spawn_blocking` would prevent it from starving other requests.

---

## 12. Pre-Computed Search Index

**Impact: MEDIUM | Feasibility: MEDIUM**

### Current state
Every search query triggers:
1. Full clone of `Vec<RomEntry>` from cache
2. `to_lowercase()` on every ROM's display name and filename
3. Multiple `contains()` / `starts_with()` string operations per ROM
4. Sort by score

For 5,000 ROMs, this is 10,000 string allocations (lowercase) + 10,000+ string comparisons per keystroke (after debounce).

### Recommendations

**A) Store pre-lowercased names in the cache** alongside the original data:
```rust
struct CachedRom {
    entry: RomEntry,
    display_lower: String,
    filename_lower: String,
}
```

**B) Build a trigram or prefix index** at cache population time. For prefix/substring search, an Aho-Corasick automaton or even a simple sorted prefix array would be far faster than linear scan.

**C) For word-level matching, pre-split display names into word arrays** at cache time instead of splitting on every search.

---

## 13. `global_search()` Scans ALL Systems

**Impact: MEDIUM | Feasibility: MEDIUM**

### Current state
`global_search()` iterates every system with games, calls `cache.get_roms()` (which clones the full Vec for each system), applies filters, and scores every ROM. For a collection with 20 systems and 50,000 total ROMs, this is 50,000 ROM clones + 50,000 score computations per search.

### Recommendations

**A) Implement early termination** -- once enough results are found (e.g., 50 across all systems), stop searching remaining systems.

**B) Search systems in parallel** using `tokio::task::spawn_blocking` for each system.

**C) Build a unified cross-system search index** at startup that can handle global queries without per-system iteration.

---

## 14. Server Function Serialization Format

**Impact: LOW-MEDIUM | Feasibility: EASY**

### Current state
Leptos server functions use `PostUrl` encoding by default, which serializes arguments as URL-encoded form data and responses as JSON. For large responses (100 ROM entries), JSON adds verbosity.

### Recommendations

**A) Consider using CBOR or rkyv** for server function encoding:
```rust
#[server(encoding = "Cbor", prefix = "/sfn")]
```
CBOR is typically 30-50% smaller than JSON for structured data, reducing transfer size and parse time.

**B) This is particularly impactful for the first SSR page**, where the full RomPage data is embedded in the HTML stream.

---

## 15. Infinite Scroll vs. Virtual Scrolling

**Impact: LOW-MEDIUM | Feasibility: HARD**

### Current state
The ROM list uses infinite scroll: every loaded ROM remains in the DOM. After scrolling through 1,000 ROMs, there are 1,000 `.rom-item` elements in the DOM, each with multiple child elements (favorite button, thumbnail, info, actions).

### Recommendations

**A) Virtual scrolling** would only render the ~20-30 visible items, dramatically reducing DOM size and memory usage. However, this is complex to implement in Leptos and may not be worth the effort unless users regularly scroll through thousands of items.

**B) A simpler middle ground**: increase the page size but only render items that are near the viewport. Use CSS `content-visibility: auto` on `.rom-item`:
```css
.rom-item {
    content-visibility: auto;
    contain-intrinsic-size: 0 60px; /* approximate height */
}
```
This tells the browser to skip rendering off-screen items, which significantly reduces paint and layout costs. Browser support is good (Chrome, Edge, Firefox 124+).

---

## Summary: Priority Ranking

| # | Improvement | Impact | Effort | ROI |
|---|-----------|--------|--------|-----|
| 1 | WASM bundle: wasm-opt + compression | Critical | Easy | Highest |
| 2 | Batch box art URL resolution | High | Medium | Very High |
| 3 | Arc<Vec> cache to avoid full clones | High | Medium | Very High |
| 5 | Cache favorites in memory | Medium | Easy | High |
| 4 | Pre-compute genre/players in cache | Medium | Easy | High |
| 6 | Longer/smarter cache TTL (mtime-based) | Medium | Easy | High |
| 8 | Combine home page server functions | Medium | Easy | High |
| 9a | Add img width/height | Medium | Easy | High |
| 15b | CSS content-visibility: auto | Medium | Easy | High |
| 11e | spawn_blocking for filesystem I/O | Medium | Medium | Medium |
| 12 | Pre-computed search data | Medium | Medium | Medium |
| 7c | Smaller first-page size | Medium | Easy | Medium |
| 9b | Resize thumbnails during import | Medium | Medium | Medium |
| 10 | CSS minification + caching | Low | Easy | Medium |
| 14 | CBOR encoding for server fns | Low | Easy | Medium |
| 13 | Global search optimization | Medium | Medium | Medium |
| 7a | Slim down RomEntry serialization | Low | Easy | Low |
| 15a | Virtual scrolling | Low | Hard | Low |

The top 6 items (WASM optimization, batch box art, Arc cache, favorites cache, pre-computed metadata, smarter cache TTL) would likely cut the Games page load time by 50-70% with moderate implementation effort.
