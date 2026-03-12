# Box Art Swap: Region Variant Picker

> Investigation date: 2026-03-12

## Problem

A user plays the USA version of Sonic the Hedgehog on their Mega Drive (chosen for its 60Hz NTSC gameplay). But they grew up with the PAL version and prefer that box art. Currently, the companion app picks the "best" box art match automatically via fuzzy matching -- the user has no say in which region variant is displayed.

The libretro-thumbnails repos contain multiple region variants for most popular games:

```
Named_Boxarts/
    Sonic the Hedgehog (USA, Europe).png
    Sonic the Hedgehog (Europe).png
    Sonic the Hedgehog (Japan).png
    Sonic the Hedgehog 2 (World).png
    Sonic the Hedgehog 2 (Japan).png
```

The `thumbnail_index` table in `metadata.db` has ALL of these indexed. The problem is that `find_in_manifest()` returns the first match it finds (via exact -> strip-tags -> version-stripped tiers), so the user gets whichever variant the fuzzy matching happens to pick. They cannot browse alternatives or override the choice.

This is a quality-of-life feature for collectors who care about presentation. It should be effortless (1-2 taps) and always reversible.

---

## How Region Variants Exist in libretro-thumbnails

### Naming Convention

libretro-thumbnails filenames include region tags in parentheses:

| Filename | Region |
|---|---|
| `Sonic the Hedgehog (USA, Europe).png` | Multi-region (USA + Europe) |
| `Sonic the Hedgehog (Europe).png` | Europe-only |
| `Sonic the Hedgehog (Japan).png` | Japan-only |
| `Street Fighter II Turbo (USA).png` | USA |
| `Street Fighter II' - Special Champion Edition (Japan).png` | Japan |

Tags can include revisions, languages, and other metadata: `(USA) (Rev 1)`, `(Europe) (En,Fr,De)`, `(Japan) (Beta)`.

### Symlinks

Many region variants are symlinks to the same underlying image. The `thumbnail_index` table tracks this via the `symlink_target` column. When `symlink_target IS NOT NULL`, the entry points to another file. This is important for the variant picker: two entries with different names might resolve to the same image, so we should de-duplicate by resolved image when building the variant list.

### How Matching Currently Works

The `ManifestFuzzyIndex` (built by `build_manifest_fuzzy_index()`) stores entries in three tiers:

1. **Exact**: `thumbnail_filename(stem)` -- includes region tags
2. **By-tags**: `lowercase(strip_tags(stem))` -- region tags stripped
3. **By-version**: `lowercase(strip_version(strip_tags(stem)))` -- also version-stripped

Each tier is a `HashMap<String, ManifestMatch>` that stores only the **first** entry it encounters for a given key. This means for the by-tags tier, if the index processes `Sonic the Hedgehog (USA, Europe)` before `Sonic the Hedgehog (Europe)`, the USA/Europe variant wins -- and the user never sees the Europe-only variant.

The key insight: the `thumbnail_index` table in SQLite has **all** variants. The lossy collapse into a single match only happens in the in-memory `ManifestFuzzyIndex`. To find all variants, we query the DB directly using `strip_tags()` matching.

---

## Finding Variants for a ROM

Given a ROM like `Sonic the Hedgehog (USA).smd`, we need to find all box art entries in `thumbnail_index` that represent the same game but with different region tags.

### Algorithm

1. Compute the ROM's base title: `strip_tags(thumbnail_filename(stem))` = `"Sonic the Hedgehog"` (lowercased for matching)
2. Query `thumbnail_index` for the relevant repo(s), kind `Named_Boxarts`, where `strip_tags(filename)` matches the base title
3. Since SQLite has no built-in `strip_tags()`, we have two options:
   - **Option A**: Load all entries for the repo and filter in Rust (the same approach `build_manifest_fuzzy_index` uses -- already proven at scale)
   - **Option B**: Add a `base_title` column to `thumbnail_index` (pre-computed `strip_tags(filename).to_lowercase()`) and use a SQL `WHERE base_title = ?`

Option A is simpler and sufficient. A repo's boxart entries typically number 2,000-15,000. Scanning these in memory to collect variants takes microseconds. There is no need for a schema change.

### De-duplication

After collecting all entries with the same base title, de-duplicate by resolved image:
- If two entries have the same `symlink_target` (or one is the target of the other), they display the same image -- keep only one and note the aliased names.
- Present only entries that resolve to distinct images.

### Expected Result for "Sonic the Hedgehog"

```
Variants found:
  1. Sonic the Hedgehog (USA, Europe)  -- [current]
  2. Sonic the Hedgehog (Europe)       -- distinct image
  3. Sonic the Hedgehog (Japan)         -- distinct image
```

---

## User Customization Persistence Strategy

Before choosing a data model for overrides, we need to solve a broader problem: **where should user customizations live so they are never lost?**

### The Durability Problem

The `game_metadata` table in `metadata.db` is a **cache** of imported data (LaunchBox descriptions, ratings, image paths). The "Clear Metadata" operation (`MetadataDb::clear()`) runs `DELETE FROM game_metadata` to wipe it all and allow a fresh re-import. Any user customizations stored as columns on `game_metadata` -- such as a `box_art_override` column -- would be destroyed by this operation.

This is not a theoretical concern. Users clear metadata when:
- A new LaunchBox XML version is available and they want to re-import
- Data looks corrupt or incomplete
- They want a fresh start

User customizations (box art overrides, video associations, ratings, notes) are **not cache data**. They represent deliberate user choices and must survive all clear/re-import operations.

### Existing Precedent: `videos.json`

The game videos feature already solved this problem. Videos are stored in `.replay-control/videos.json`, a standalone JSON file completely separate from `metadata.db`. The rationale from the game-videos plan document:

> Separate from `metadata.db`. JSON format -- data is small (hundreds of entries max at ~200 bytes each), simple access pattern, trivially portable and debuggable.

This works well for videos, but the JSON-file-per-feature approach does not scale cleanly to multiple customization types (box art overrides, future ratings, notes, tags, etc.) -- each would need its own file, its own load/save code, its own mutex guard, and its own atomic-write boilerplate.

### Storage Options Evaluated

| Option | Pros | Cons |
|---|---|---|
| **A. Column on `game_metadata`** | Simple, no joins | Destroyed by "Clear Metadata"; mixes cache with user data |
| **B. Separate table in `metadata.db`, excluded from clear** | Single DB, easy joins | Requires careful discipline: every future clear-like operation must remember to skip this table; tight coupling between cache lifecycle and user data |
| **C. Separate `user_data.db` file** | Complete isolation from cache lifecycle; impossible to accidentally destroy via metadata operations; clean separation of concerns | Two SQLite connections to manage; cross-DB joins not possible (but not needed) |
| **D. JSON files per feature** (current videos approach) | Simple for a single feature; human-readable | Does not scale to multiple customization types; no query capability; full-file rewrite on every change |

### Recommendation: Separate `user_data.db` (Option C)

A dedicated `user_data.db` SQLite file in `.replay-control/` is the right approach:

1. **Complete isolation**: No metadata clear, re-import, or DB rebuild can touch it. There is zero risk of accidental data loss because the file is physically separate.

2. **Unified storage for all user customizations**: Box art overrides, video associations, future user ratings, notes, custom tags -- all in one place with proper schema, indexes, and query capability.

3. **Same infrastructure**: Reuses the same `rusqlite` dependency, same NFS `nolock` fallback pattern, same Mutex-guarded access via `AppState`. The `MetadataDb::open()` pattern can be cloned nearly verbatim for a `UserDataDb::open()`.

4. **No cross-DB join needed**: The box art resolution path already loads data into an in-memory `ImageIndex`. The override lookup is a simple `HashMap` check at index-build time, not a SQL join.

### `user_data.db` Schema

**File**: `<storage>/.replay-control/user_data.db`

```sql
-- Box art overrides: user-chosen region variant for a game's cover art.
-- NULL means "use the auto-matched default."
CREATE TABLE IF NOT EXISTS box_art_overrides (
    system TEXT NOT NULL,
    rom_filename TEXT NOT NULL,
    override_path TEXT NOT NULL,   -- relative media path, e.g. "boxart/Sonic the Hedgehog (Europe).png"
    set_at INTEGER NOT NULL,       -- unix timestamp
    PRIMARY KEY (system, rom_filename)
);

-- Future tables (not implemented yet, shown for schema direction):
--
-- CREATE TABLE IF NOT EXISTS user_ratings (
--     system TEXT NOT NULL,
--     rom_filename TEXT NOT NULL,
--     rating INTEGER NOT NULL,        -- 1-5 stars
--     set_at INTEGER NOT NULL,
--     PRIMARY KEY (system, rom_filename)
-- );
--
-- CREATE TABLE IF NOT EXISTS user_notes (
--     system TEXT NOT NULL,
--     rom_filename TEXT NOT NULL,
--     note TEXT NOT NULL,
--     updated_at INTEGER NOT NULL,
--     PRIMARY KEY (system, rom_filename)
-- );
```

### Implementation: `UserDataDb`

New file: `replay-control-core/src/user_data_db.rs`

```rust
pub const USER_DATA_DB_FILE: &str = "user_data.db";

pub struct UserDataDb {
    conn: Connection,
    db_path: PathBuf,
}
```

Follows the same pattern as `MetadataDb`:
- `open()` with nolock fallback for NFS
- `init()` creates tables idempotently
- Mutex-guarded via `AppState` (new field: `user_data_db: Mutex<Option<UserDataDb>>`)

### What About `videos.json`?

`videos.json` works well and stays as-is. There is no need to migrate it to `user_data.db`. New user customization features (box art overrides, future ratings, notes, tags) should use `user_data.db` from the start.

### Updated `.replay-control/` Directory Structure

```
.replay-control/
    settings.cfg           # App-specific settings (key=value)
    metadata.db            # Cache: game metadata, thumbnail index, rom cache
    user_data.db           # User customizations: box art overrides, future ratings/notes
    videos.json            # User-saved video links (existing, stays as-is)
    launchbox-metadata.xml # LaunchBox XML dump
    media/                 # Downloaded images
    tmp/                   # Cached git clones
```

The key invariant: **`metadata.db` is a cache that can be rebuilt from external sources. `user_data.db` and `videos.json` contain user choices that cannot be reconstructed.**

---

## Data Model for Box Art Overrides

With `user_data.db` as the storage layer, the override model is straightforward:

### The `box_art_overrides` Table

```sql
CREATE TABLE IF NOT EXISTS box_art_overrides (
    system TEXT NOT NULL,
    rom_filename TEXT NOT NULL,
    override_path TEXT NOT NULL,
    set_at INTEGER NOT NULL,
    PRIMARY KEY (system, rom_filename)
);
```

- **No row** (default): Use the auto-matched box art from `game_metadata.box_art_path`.
- **Row exists**: `override_path` contains the relative media path of the user-chosen variant, e.g., `"boxart/Sonic the Hedgehog (Europe).png"`.
- **Revert**: `DELETE FROM box_art_overrides WHERE system = ? AND rom_filename = ?`.

Resolution order in `resolve_box_art()`:
1. Check `box_art_overrides` in `user_data.db` -- if a row exists, use `override_path` (verify file exists on disk)
2. Fall back to `box_art_path` from `game_metadata` (existing behavior)
3. Fall back to fuzzy disk scan (existing behavior)

### Why Not a Column on `game_metadata`?

Adding a `box_art_override` column to `game_metadata` was the original proposal. It fails because `MetadataDb::clear()` runs `DELETE FROM game_metadata` -- the user's box art choices would be wiped every time they clear and re-import metadata. A separate table in `metadata.db` could work but requires careful discipline to exclude it from every clear-like operation, and conceptually mixes cache data with user data.

### Why Not a Symlink on Disk?

Replacing the boxart file with the chosen variant destroys the original. To support "revert," we would need to remember the original path -- which puts us back at needing a database entry anyway. Also fragile if the user clears and re-downloads images.

---

## UI/UX Options

### Context: Where Does This Live?

The game detail page (`/games/{system}/{filename}`) already shows the box art prominently as a hero image. This is the natural place for a variant picker -- the user is already looking at one specific game and its artwork.

The ROM list is too dense for this interaction. Each ROM row shows a 56x40px thumbnail; trying to swap art from there would be cramped and disruptive to the list flow.

### Option 1: Tap-to-Swap on Game Detail Hero Image (Recommended)

The box art image in the game detail hero section becomes tappable. Tapping it opens a bottom sheet (mobile-native pattern) showing all available region variants as a horizontal strip of thumbnails.

**Visual affordance (only when multiple variants exist):** A small "Change cover" text link appears directly below the hero image, styled as a subtle secondary-text link. This makes the feature discoverable without requiring users to guess that the image is tappable. When a game has only a single variant, the link is not rendered and the hero image looks exactly as it does today -- no visual changes for the common case.

The variant count check is lightweight: when building the game detail page data, we count matching `strip_tags()` entries in `thumbnail_index` for the ROM's base title and de-duplicate by symlink target. This piggybacks on data already loaded for the game detail page and adds negligible cost (<1ms).

```
 Game with multiple variants:

 +----------------------------------------------+
 |  < Back          Sonic the Hedgehog           |
 +----------------------------------------------+
 |                                               |
 |          +-----------------------+            |
 |          |                       |            |
 |          |    [  Box Art  ]      |  <-- tap   |
 |          |    (USA, Europe)      |            |
 |          |                       |            |
 |          +-----------------------+            |
 |              Change cover >                   |
 |                                               |
 |  System:    Sega - Mega Drive                 |
 |  Region:    USA                               |
 |  File:      Sonic the Hedgehog (USA).smd      |
 ...

 Game with single variant (no affordance):

 +----------------------------------------------+
 |  < Back          Columns                      |
 +----------------------------------------------+
 |                                               |
 |          +-----------------------+            |
 |          |                       |            |
 |          |    [  Box Art  ]      |            |
 |          |    (World)            |            |
 |          |                       |            |
 |          +-----------------------+            |
 |                                               |
 |  System:    Sega - Mega Drive                 |
 ...

 === Bottom sheet slides up after tap ===

 +----------------------------------------------+
 |  Choose Box Art                         [ X ] |
 +----------------------------------------------+
 |                                               |
 |  +-------+  +-------+  +-------+             |
 |  | .---. |  | .---. |  | .---. |             |
 |  | |USA| |  | |EUR| |  | |JPN| |             |
 |  | | + | |  | |   | |  | |   | |             |
 |  | |EUR| |  | |   | |  | |   | |             |
 |  | '---' |  | '---' |  | '---' |             |
 |  +-------+  +-------+  +-------+             |
 |   (USA,EU)   (Europe)   (Japan)               |
 |     [*]                                       |
 |                                               |
 |  [ Reset to Default ]                         |
 +----------------------------------------------+
```

**How it works:**
1. Game detail page loads and checks the variant count for this ROM
2. If multiple variants exist, a "Change cover >" link appears below the hero image; the image itself is also tappable
3. User taps the link or the image
4. A server function fetches all box art variants for this ROM from `thumbnail_index`
5. Bottom sheet opens showing variant thumbnails in a horizontal scrollable strip
6. Variants that are not yet downloaded show a placeholder with a download icon
7. Tapping a variant:
   - Downloads it if not already on disk (single HTTP fetch, ~0.5s)
   - Inserts/updates a row in `user_data.db` `box_art_overrides` with the chosen variant's path
   - Closes the sheet and updates the hero image immediately
8. The currently active variant has a check mark or highlight ring
9. A "Reset to Default" button deletes the override row from `box_art_overrides`

**Pros:**
- Discoverable: the "Change cover" link makes it obvious that alternatives exist
- Zero clutter for single-variant games: no link, no overlay, no change
- Bottom sheet is the standard mobile pattern for option pickers
- Variants are visual -- users see the actual box art, not just text labels
- On-demand download: only fetches images the user actually wants to see

**Cons:**
- First tap loads variants from the server (brief loading state)
- Variant count check adds a lightweight query to every game detail page load (but <1ms and already in-memory data)

### Option 2: Dedicated "Box Art" Section in Game Detail

Add a section below the hero image, always visible:

```
 +----------------------------------------------+
 |  < Back          Sonic the Hedgehog           |
 +----------------------------------------------+
 |          +-----------------------+            |
 |          |    [  Box Art  ]      |            |
 |          +-----------------------+            |
 |                                               |
 |  --- Box Art Variants ---                     |
 |  +-------+  +-------+  +-------+             |
 |  | USA+EU|  | EUR   |  | JPN   |             |
 |  |  [*]  |  |       |  |       |             |
 |  +-------+  +-------+  +-------+             |
 |                                               |
 |  --- Game Info ---                            |
 |  System: ...                                  |
 ...
```

**Pros:**
- Always visible -- no discovery problem
- Could show additional metadata per variant (image dimensions, etc.)

**Cons:**
- Clutters the page even when the user doesn't care about variants
- Many games have only one variant, making this section empty/useless most of the time
- Takes up vertical space on a mobile screen

### Option 3: Long-Press / Context Menu on Box Art in ROM List

Long-pressing a thumbnail in the ROM list opens a context menu with "Change box art."

**Pros:**
- Accessible directly from the browse view without navigating to the detail page

**Cons:**
- Long-press is not discoverable on touch (no hover state to hint at it)
- ROM list thumbnails are tiny (56x40px) -- hard to long-press accurately
- Context menus feel foreign in a mobile-first web app
- Adds complexity to the ROM list component, which is already performance-sensitive

### Option 4: Inline Carousel in ROM List

Tapping a thumbnail in the ROM list expands it into a mini-carousel.

**Cons:**
- Disrupts list layout (items shift, scroll position jumps)
- Performance concern: expanding an item in a virtualized/infinite-scroll list is tricky
- Too much interaction complexity for a secondary feature

### Recommendation: Option 1 (Tap-to-Swap Bottom Sheet)

Option 1 is the best fit for a mobile-first Pi companion app:
- Discoverable via "Change cover" link, but only when there are actual alternatives
- Zero visual overhead for single-variant games (the common case)
- Standard mobile UX pattern (bottom sheet)
- Visual comparison of variants side by side
- Clean integration with the existing game detail page
- On-demand download keeps storage minimal

---

## Implementation Notes

### New Module: `UserDataDb`

New file: `replay-control-core/src/user_data_db.rs`

Creates `user_data.db` in `.replay-control/` with the `box_art_overrides` table. Follows the same open/init pattern as `MetadataDb` (nolock fallback for NFS, `CREATE TABLE IF NOT EXISTS` for idempotent init).

`AppState` gets a new field: `user_data_db: Mutex<Option<UserDataDb>>`, initialized alongside `metadata_db` in the server startup.

### New Core Functions

**`find_boxart_variants()`** in `thumbnail_manifest.rs`:

```rust
pub fn find_boxart_variants(
    db: &MetadataDb,
    system: &str,
    rom_filename: &str,
) -> Vec<BoxArtVariant> { ... }
```

1. Get repo names via `thumbnail_repo_names(system)`
2. For each repo, call `db.query_thumbnail_index(source_name, "Named_Boxarts")`
3. Compute the ROM's base title via `strip_tags(thumbnail_filename(stem)).to_lowercase()`
4. Filter entries where `strip_tags(entry.filename).to_lowercase() == base_title`
5. De-duplicate by resolved image (follow symlink chains)
6. For each unique variant, check if the file exists on disk
7. Return a list of `BoxArtVariant` structs

```rust
pub struct BoxArtVariant {
    /// Filename stem in the thumbnail index (e.g., "Sonic the Hedgehog (Europe)")
    pub filename: String,
    /// Region tag extracted from the filename (e.g., "Europe")
    pub region_label: String,
    /// Whether the image is already downloaded to local media
    pub is_downloaded: bool,
    /// URL to serve the image (if downloaded), or None
    pub image_url: Option<String>,
    /// Whether this is the currently active variant (matches box_art_path or box_art_override)
    pub is_active: bool,
    /// ManifestMatch info needed for downloading
    pub repo_url_name: String,
    pub branch: String,
}
```

**`extract_region_label()`** helper:

```rust
/// Extract the region tag from a thumbnail filename.
/// "Sonic the Hedgehog (USA, Europe)" -> "USA, Europe"
/// "Sonic the Hedgehog (Japan) (Rev 1)" -> "Japan"
fn extract_region_label(filename: &str) -> String { ... }
```

Parse the first parenthesized group after the base title. Known region tokens: USA, Europe, Japan, World, Korea, Brazil, Asia, Australia, etc. If the first group doesn't contain a known region token, fall back to showing the full tag content.

### New Server Functions

**`GetBoxArtVariants`**: Returns the list of variants for a ROM.

```rust
#[server(GetBoxArtVariants)]
pub async fn get_boxart_variants(
    system: String,
    rom_filename: String,
) -> Result<Vec<BoxArtVariant>, ServerFnError> { ... }
```

**`SetBoxArtOverride`**: Downloads (if needed) and sets the override.

```rust
#[server(SetBoxArtOverride)]
pub async fn set_boxart_override(
    system: String,
    rom_filename: String,
    variant_filename: String,  // the thumbnail_index filename stem
) -> Result<String, ServerFnError> { ... }
```

1. Look up the variant in `thumbnail_index` (via `metadata_db`) to get repo/branch info
2. If not on disk, download from `raw.githubusercontent.com` to `.replay-control/media/{system}/boxart/`
3. Insert/replace row in `user_data_db.box_art_overrides` with the relative path
4. Invalidate the image index cache for this system
5. Return the new image URL

**`ResetBoxArtOverride`**: Clears the override.

```rust
#[server(ResetBoxArtOverride)]
pub async fn reset_boxart_override(
    system: String,
    rom_filename: String,
) -> Result<(), ServerFnError> { ... }
```

Deletes the row from `box_art_overrides` and invalidates the image index cache.

All three server functions need `register_explicit` in `main.rs`.

### Changes to Box Art Resolution

In `cache.rs`, modify `resolve_box_art()` to check overrides:

```
Current: db_paths -> exact -> fuzzy -> version
New:     override -> db_paths -> exact -> fuzzy -> version
```

The `ImageIndex` gets a new field: `overrides: HashMap<String, String>` mapping `rom_filename` to `override_path`. Built at index-build time by querying `user_data_db.box_art_overrides` for the system.

Alternatively (simpler): just merge override paths into `db_paths` when building the index -- if a row exists in `box_art_overrides` for this ROM, use its `override_path` instead of `game_metadata.box_art_path`. This way `resolve_box_art()` needs zero changes; the priority is handled at index-build time.

### UI Component: `BoxArtPicker`

A new Leptos component for the bottom sheet:

```rust
#[component]
fn BoxArtPicker(
    system: String,
    rom_filename: String,
    #[prop(into)] on_close: Callback<()>,
    #[prop(into)] on_change: Callback<String>,  // new image URL
) -> impl IntoView { ... }
```

- Fetches variants via `get_boxart_variants()` on mount
- Shows a loading spinner while fetching
- Renders variant thumbnails in a horizontal scrollable strip
- Tapping a variant calls `set_boxart_override()` (writes to `user_data.db`) and emits `on_change`
- "Reset to Default" button calls `reset_boxart_override()` (deletes from `user_data.db`)
- Close button or swipe-down dismisses the sheet

The bottom sheet pattern already exists in the app for other purposes and can be reused or adapted.

### Interaction with Existing Thumbnail Cache Layers

| Layer | Impact |
|---|---|
| `thumbnail_index` in `metadata.db` | Read-only for this feature -- we query it to find variants but never modify it |
| `game_metadata.box_art_path` in `metadata.db` | Untouched -- continues to store the auto-matched path |
| `box_art_overrides` in `user_data.db` | New table -- stores user choice; no row means "use default" |
| `ImageIndex` (in-memory cache) | Override paths take priority when building; cache invalidated after swap |
| `.replay-control/media/{system}/boxart/` | New variant images downloaded here on demand; same directory as auto-matched images |
| `ManifestFuzzyIndex` | Not modified -- the variant picker queries the DB directly rather than going through the single-match fuzzy index |

### Storage Considerations

- Variant images are downloaded on demand (only when the user explicitly picks one)
- A typical game has 2-4 region variants; each boxart PNG is 10-50 KB
- Even swapping 100 games adds only ~2-5 MB to disk
- Downloaded variants persist in the media directory; clearing images removes them too
- Override rows in `user_data.db` survive both metadata clears and image clears: next time thumbnails are re-downloaded, the override path guides which variant to re-fetch
- `user_data.db` itself is tiny (a few KB even with hundreds of overrides) and is never touched by any clear operation

### Edge Cases

1. **Game with only one variant**: The "Change cover" link is not rendered and the hero image is not styled as tappable. The page looks exactly as it does today. The variant count is checked at page load via an in-memory scan of the thumbnail index entries (<1ms).

2. **Override image deleted** (e.g., user cleared images): `resolve_box_art()` verifies the file exists on disk. If the override path points to a missing file, fall back to `box_art_path`. The override row in `user_data.db` is preserved so next time images are downloaded, it can be honored.

3. **ROM with no thumbnail index**: If the thumbnail index hasn't been built yet, the variant picker has no data to show. Display "Download thumbnail index first" with a link to the metadata page.

4. **Arcade games**: MAME codenames go through `arcade_db` translation before matching. The variant finder must apply the same translation. Most arcade games have a single region variant (since MAME names are already canonical), so this feature is less relevant for arcade systems.

5. **Same base title, different games**: Very rare in practice. Entries like `Frogger (USA)` and `Frogger (Japan)` for the same repo will always be the same game. Cross-repo collisions (different systems) cannot happen because the query is scoped to the system's repos.

### Performance Budget

- **Variant lookup**: Load repo entries (~5K-15K per repo), filter in memory -> <1ms
- **Download a single variant**: One HTTP fetch -> ~0.5-1s on Pi 4 with decent connection
- **DB update**: Single row UPDATE -> <1ms
- **Cache invalidation**: Discards the in-memory `ImageIndex` for one system -> next request rebuilds in ~10ms

The entire swap flow (tap -> load variants -> pick one -> download + save) should complete in under 2 seconds.

---

## Implementation Plan: File-by-File Changelist

This section maps the design to specific files in the codebase, in implementation order. Each step is self-contained and testable.

### Phase 1: `UserDataDb` Foundation

**Step 1.1: Create `UserDataDb` module**

New file: `replay-control-core/src/user_data_db.rs`

- `UserDataDb` struct with `conn: Connection` and `db_path: PathBuf`
- `open(storage_root: &Path)` -- same NFS `nolock` fallback as `MetadataDb::open()` in `replay-control-core/src/metadata_db.rs:83`
- `init()` -- `CREATE TABLE IF NOT EXISTS box_art_overrides (...)`
- `set_override(system, rom_filename, override_path)` -- `INSERT OR REPLACE`
- `remove_override(system, rom_filename)` -- `DELETE`
- `get_override(system, rom_filename) -> Option<String>` -- single row lookup
- `get_system_overrides(system) -> HashMap<String, String>` -- batch lookup for `ImageIndex` build

Modify: `replay-control-core/src/lib.rs` -- add `pub mod user_data_db;`

**Step 1.2: Wire `UserDataDb` into `AppState`**

Modify: `replay-control-app/src/api/mod.rs`

- Add field: `pub(crate) user_data_db: Arc<Mutex<Option<UserDataDb>>>`
- Add `user_data_db()` accessor method (same lazy-open pattern as `metadata_db()`)
- Initialize in `AppState::new()` as `Arc::new(Mutex::new(None))`

**Step 1.3: Update `.replay-control/` reference docs**

Modify: `docs/reference/replay-control-folder.md` -- add `user_data.db` entry

### Phase 2: Variant Discovery

**Step 2.1: Add `find_boxart_variants()` to thumbnail manifest**

Modify: `replay-control-core/src/thumbnail_manifest.rs`

- Add `BoxArtVariant` struct (see design above)
- Add `find_boxart_variants(db, system, rom_filename, storage_root) -> Vec<BoxArtVariant>`
- Add `extract_region_label(filename) -> String` helper
- Uses `db.query_thumbnail_index()` (already exists at `metadata_db.rs:1052`) to load all entries, filters by `strip_tags()` match, de-dups by `symlink_target`

### Phase 3: Box Art Override Resolution

**Prerequisite knowledge: Two distinct resolution paths exist.**

The ROM list page and the game detail page resolve box art through *different* code paths. Both need override support.

**Path A -- ROM list (batch resolution via `ImageIndex`):**
- `get_roms_page()` in `replay-control-app/src/server_fns/roms.rs:165` calls `cache.resolve_box_art()`
- `RomCache::resolve_box_art()` in `replay-control-app/src/api/cache.rs:747` checks: db_paths -> exact -> fuzzy -> version -> on-demand manifest
- `RomCache::get_image_index()` in `cache.rs:608` builds the `ImageIndex` with `db_paths` from `game_metadata.box_art_path`

**Path B -- Game detail page (single-ROM resolution):**
- `get_rom_detail()` in `roms.rs:228` calls `resolve_game_info()`
- `resolve_game_info()` in `replay-control-app/src/server_fns/mod.rs:106` calls `enrich_from_metadata_cache()`
- `enrich_from_metadata_cache()` in `mod.rs:274` checks `db.lookup()` for `box_art_path`, then `find_image_on_disk()` as fallback
- `find_image_on_disk()` in `mod.rs:381` does an exact -> fuzzy disk scan

**Step 3.1: Add overrides to `ImageIndex` build (Path A)**

Modify: `replay-control-app/src/api/cache.rs`

In `get_image_index()` (~line 689), after loading `db_paths` from `game_metadata`, also query `user_data_db.get_system_overrides(system)`. For each override, insert into `db_paths` (overwriting the auto-matched path). This way `resolve_box_art()` needs zero changes -- overrides take priority at index-build time.

Add `user_data_db` to `ImageIndex` build inputs (pass `AppState` reference, which already exists as a parameter).

**Step 3.2: Add overrides to `enrich_from_metadata_cache()` (Path B)**

Modify: `replay-control-app/src/server_fns/mod.rs`

In `enrich_from_metadata_cache()` (~line 274), before checking `game_metadata.box_art_path`, check `user_data_db.get_override(system, rom_filename)`. If an override exists and the file is on disk, use it and skip the metadata/filesystem fallback.

### Phase 4: Server Functions

**Step 4.1: Create box art server functions**

New file: `replay-control-app/src/server_fns/boxart.rs`

Three server functions:
- `get_boxart_variants(system, rom_filename)` -- calls `find_boxart_variants()` from Phase 2
- `set_boxart_override(system, rom_filename, variant_filename)` -- downloads if needed (reusing `thumbnail_manifest::download_thumbnail()` and `save_thumbnail()`), inserts override into `user_data_db`, invalidates image cache via `cache.invalidate_system_images()`
- `reset_boxart_override(system, rom_filename)` -- deletes override from `user_data_db`, invalidates image cache

Modify: `replay-control-app/src/server_fns/mod.rs` -- add `mod boxart;` and re-export types

**Step 4.2: Register server functions**

Modify: `replay-control-app/src/main.rs` -- add `register_explicit` calls for all three server functions (same pattern as metadata server functions -- required because library crate server functions get stripped by the linker)

### Phase 5: UI Components

**Step 5.1: Add variant count to `RomDetail`**

Modify: `replay-control-app/src/server_fns/roms.rs`

- Add `variant_count: usize` field to `RomDetail` struct (line 26)
- In `get_rom_detail()`, query variant count via `find_boxart_variants()` (or a lighter `count_boxart_variants()` that just returns the count without building full structs)

**Step 5.2: Create `BoxArtPicker` component**

New file: `replay-control-app/src/components/boxart_picker.rs`

- Bottom sheet overlay component
- Fetches variants via `get_boxart_variants()` on open
- Renders variant thumbnails in a horizontal scrollable strip
- Tap to select calls `set_boxart_override()`
- "Reset to Default" calls `reset_boxart_override()`
- Emits `on_change` callback with the new image URL

Pattern precedent: `GameVideoSection` in `replay-control-app/src/components/video_section.rs` shows how to build an interactive section on the game detail page that fetches data on mount and handles async mutations.

Modify: `replay-control-app/src/components/mod.rs` -- add `pub mod boxart_picker;`

**Step 5.3: Integrate into game detail page**

Modify: `replay-control-app/src/pages/game_detail.rs`

In `GameDetailContent` (~line 47):
- Add `box_art_url` as an `RwSignal` instead of `StoredValue` (so it can be reactively updated when the user swaps)
- Add `variant_count` from the `RomDetail`
- Below the hero `<img>`, conditionally render a "Change cover" link when `variant_count > 1`
- Add a `show_picker: RwSignal<bool>` signal
- Tapping the link (or the hero image when variants exist) sets `show_picker` to `true`
- Render `<BoxArtPicker>` conditionally with `<Show when=move || show_picker.get()>`
- `on_change` callback updates `box_art_url` signal and sets `show_picker` to `false`

**Step 5.4: Add i18n keys**

Modify: `replay-control-app/src/i18n.rs` (or the translation JSON files)

Keys needed:
- `game_detail.change_cover` -- "Change cover"
- `game_detail.choose_boxart` -- "Choose Box Art"
- `game_detail.reset_default` -- "Reset to Default"
- `game_detail.downloading` -- "Downloading..."
- `game_detail.no_variants` -- "No alternative covers found"
- `game_detail.build_index_first` -- "Download thumbnail index first"

### Phase 6: CSS

**Step 5.5: Add styles for the picker**

Modify: `replay-control-app/style/` (the CSS source files)

- `.boxart-picker-overlay` -- full-screen semi-transparent backdrop
- `.boxart-picker-sheet` -- bottom sheet container with slide-up animation
- `.boxart-picker-strip` -- horizontal scrollable flex container
- `.boxart-variant` -- individual variant thumbnail with label
- `.boxart-variant.active` -- highlight ring for the current selection
- `.change-cover-link` -- subtle secondary-text link below hero image

### File Summary

| Action | File |
|---|---|
| **Create** | `replay-control-core/src/user_data_db.rs` |
| **Create** | `replay-control-app/src/server_fns/boxart.rs` |
| **Create** | `replay-control-app/src/components/boxart_picker.rs` |
| Modify | `replay-control-core/src/lib.rs` -- add `pub mod user_data_db` |
| Modify | `replay-control-core/src/thumbnail_manifest.rs` -- add `find_boxart_variants()`, `BoxArtVariant`, `extract_region_label()` |
| Modify | `replay-control-app/src/api/mod.rs` -- add `user_data_db` field to `AppState` |
| Modify | `replay-control-app/src/api/cache.rs` -- query overrides in `get_image_index()` |
| Modify | `replay-control-app/src/server_fns/mod.rs` -- add `mod boxart`, check overrides in `enrich_from_metadata_cache()` |
| Modify | `replay-control-app/src/server_fns/roms.rs` -- add `variant_count` to `RomDetail` |
| Modify | `replay-control-app/src/pages/game_detail.rs` -- add picker integration to hero section |
| Modify | `replay-control-app/src/components/mod.rs` -- add `pub mod boxart_picker` |
| Modify | `replay-control-app/src/main.rs` -- add `register_explicit` for 3 server functions |
| Modify | `replay-control-app/src/i18n.rs` -- add translation keys |
| Modify | `replay-control-app/style/` -- add picker CSS |
| Modify | `docs/reference/replay-control-folder.md` -- add `user_data.db` entry |

---

## Summary

| Aspect | Decision |
|---|---|
| User data storage | New `user_data.db` SQLite file in `.replay-control/` -- separate from `metadata.db` cache |
| Data model | `box_art_overrides` table in `user_data.db` keyed by `(system, rom_filename)` |
| UI surface | "Change cover" link below hero image (only when multiple variants exist) -> bottom sheet picker |
| Variant discovery | Query `thumbnail_index` with `strip_tags()` matching, de-dup by symlink target |
| Image storage | On-demand download of chosen variant only |
| Revert | "Reset to Default" button deletes the override row |
| New server functions | `GetBoxArtVariants`, `SetBoxArtOverride`, `ResetBoxArtOverride` |
| New module | `replay-control-core/src/user_data_db.rs` (`UserDataDb` struct) |
| Impact on existing code | Minimal -- override check in `ImageIndex` build, no changes to fuzzy matching |
| Survives metadata clear | Yes -- `user_data.db` is never touched by any clear/re-import operation |
| Future extensibility | `user_data.db` is the home for ratings, notes, and tags; `videos.json` stays as-is |
