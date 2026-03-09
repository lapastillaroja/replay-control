# Arcade DB / Game DB Unification Analysis

Can arcade_db and game_db be merged into a single system? The original analysis (sections 1-8) concluded "keep separate" based on data model differences. Section 9 identifies cross-cutting features that need to query both databases. **Final recommendation: keep the databases separate at the module/binary level, but unify game information at the backend API and SSR layer so that server functions and UI components see a single `GameInfo` type regardless of source.**

---

## 1. Data Model Differences

**arcade_db** (flat, one struct):
- `ArcadeGameInfo`: rom_name, display_name, year (`&str`), manufacturer, players, rotation, status, is_clone, parent, category
- Arcade-specific fields with no console equivalent: `rotation` (Horizontal/Vertical/Unknown), `status` (Working/Imperfect/Preliminary/Unknown), `is_clone`, `parent`
- Year stored as `&'static str` (some MAME years are "198?" or empty)

**game_db** (two-level):
- `CanonicalGame`: display_name, year (`u16`), genre, developer, players
- `GameEntry`: canonical_name, region, crc32, game (reference to CanonicalGame)
- Console-specific fields with no arcade equivalent: `region`, `crc32`, developer
- Two-level model allows multiple ROM variants (USA, Europe, Japan, Rev A) to share one CanonicalGame

**Incompatibilities:**
- Year type differs (`&str` vs `u16`) -- minor, reconcilable
- arcade_db has manufacturer; game_db has developer -- similar but sourced differently
- arcade_db has rotation/status/is_clone/parent; game_db has region/crc32 -- fundamentally different
- Lookup keys differ: arcade uses rom_name (zip stem), game_db uses filename stem (No-Intro name) or CRC32
- arcade_db is system-agnostic (single flat map for all arcade systems); game_db is partitioned per system

## 2. Lookup Pattern Differences

**arcade_db:**
- Single global PHF map keyed by ROM zip stem (e.g., "mslug6")
- One lookup function, no system parameter needed
- No CRC fallback -- arcade ROMs always use their canonical zip names

**game_db:**
- Per-system PHF maps, dispatched by system folder name (e.g., "nintendo_nes")
- Primary: filename stem lookup. Fallback: CRC32 index lookup, then normalized title fallback
- System parameter required for all queries

These are architecturally different. A unified API would need both patterns, meaning no simplification at the call site.

## 3. Build Pipeline Differences

**arcade_db sources:** MAME XML (current + 2003+), FBNeo DAT, Flycast CSV, catver.ini files
**game_db sources:** No-Intro DATs, TheGamesDB JSON, libretro metadata DATs

Zero overlap in source files, parsing code, or merge logic. The arcade pipeline has a priority-based override system (Flycast > MAME current > MAME 2003+ > FBNeo). The game_db pipeline groups ROM variants into canonical games and cross-references three metadata sources by CRC and normalized title.

Unifying the build pipeline would mean interleaving completely unrelated parsing logic, making the already-large build.rs harder to maintain for no benefit.

## 4. The Clone/Parent Problem

Arcade clones (e.g., `pacman` is a clone of `puckman`) could theoretically map to a game_db-style CanonicalGame. But this breaks the existing semantics: arcade_db callers use `is_clone` and `parent` to filter or navigate between variants. In game_db, the equivalent concept is implicit (multiple GameEntries referencing the same CanonicalGame), with no parent/child directionality.

Forcing arcade clones into the CanonicalGame model would lose the parent ROM name, which is needed for "hide clones" and "show parent" features.

## 5. What Breaks If We Merge the Databases

Direct API consumers:
- `game_ref.rs`: branches on `SystemCategory::Arcade` to choose arcade_db vs game_db -- a unified DB would simplify this one branch
- `server_fns.rs`: directly calls `lookup_arcade_game()` and reads `Rotation` variants for the game detail view
- All arcade_db tests (14 tests) reference `ArcadeGameInfo` fields, `Rotation`, `DriverStatus`

A unified struct would need `Option<Rotation>`, `Option<DriverStatus>`, `Option<bool>` for is_clone, `Option<&str>` for parent, `Option<&str>` for region, `Option<u32>` for crc32 -- a struct full of Options that is worse than two focused structs.

## 6. Size Impact

Currently: arcade_db PHF map (~2.2 MB) + game_db PHF maps (~1.5 MB) = ~3.7 MB total.

A unified struct would be **larger** because every arcade entry gains unused region/crc32 fields, and every console entry gains unused rotation/status/is_clone/parent fields. PHF map overhead is proportional to entry count, which stays the same. No savings from unification.

## 7. What Merging Would Actually Buy

The only concrete win: `game_ref.rs` could drop the `if arcade { ... } else { ... }` branch (6 lines). Everything else gets worse -- larger structs, more Options, interleaved build logic, loss of type safety (rotation on a console game makes no sense).

## 8. Database-Level Conclusion: Keep Separate

**Keep arcade_db and game_db as separate modules.** They solve different problems for different system types with different data sources and different access patterns. The small amount of conceptual duplication (both are "game metadata databases with PHF maps") is not worth the complexity of a forced unification.

The question is: where should the unification happen instead?

---

## 9. Cross-Cutting Features: Where Separation Hurts

The original analysis focused on structural differences and concluded there was no practical benefit to DB-level unification. But several existing and planned features expose the cost of having no shared abstraction:

### 9.1 Features That Need Cross-DB Queries

1. **Global search by name** -- search the entire library (arcade + console + handheld) by game title. Currently requires querying `arcade_db::ARCADE_DB` (single flat map, 25K+ entries) and then each per-system PHF map in `game_db` separately. No unified iterator or search function exists.

2. **Filter/browse by genre** -- show all "Platform" games, all "RPGs", etc. across every system. arcade_db stores `category` (e.g., "Platform / Run Jump", "Fighter / Versus"); game_db stores `genre` (e.g., "Platform", "Fighting"). Different field names, different taxonomies, different granularity.

3. **Filter by number of players** -- "show me all 4-player games." Both databases have a `players: u8` field but in different structs (`ArcadeGameInfo` vs `CanonicalGame`), so a cross-system query must branch on system type.

4. **Organize favorites by genre** -- group the user's favorites not just by system but by genre ("All my platformers", "All my RPGs"). The `Favorite` struct contains a `GameRef` which has no genre field. Resolving genre requires a DB lookup that differs by system type, and the genre values themselves are incompatible between arcade and non-arcade.

5. **Cross-system game discovery** -- "show me all RPGs" or "all 2-player co-op games" regardless of platform. This is the general case of (2) and (3) combined.

6. **Future metadata scraping** -- fetching box art, descriptions, or ratings from external APIs. The scraping logic would need to resolve game identity from both DBs, and store/cache results in a common format.

### 9.2 The Genre Taxonomy Problem

This is the hardest part. The two databases use fundamentally different classification systems:

**arcade_db categories** (from catver.ini): ~184 unique values using a "Primary / Secondary" format:
- "Fighter / Versus", "Fighter / 2D", "Fighter / 3D"
- "Shooter / Flying Vertical", "Shooter / Flying Horizontal", "Shooter / Field"
- "Platform / Run Jump", "Platform / Fighter Scrolling"
- "Driving / Race", "Driving / Motorbike"
- "Sports / Baseball", "Sports / Tennis"
- Also includes non-game categories: "System", "Utilities", "Electromechanical"

**game_db genres** (from libretro-database + TGDB): ~29 unique values, flat single-level:
- "Platform", "Action", "Fighting", "Role-playing (RPG)"
- "Shoot'em Up", "Lightgun Shooter", "Beat'em Up"
- "Racing", "Sports", "Puzzle", "Strategy"
- "Adventure", "Simulation", "Board", "Card"

The catver primary categories map roughly but not exactly to libretro genres:

| catver.ini primary | libretro genre | Notes |
|---|---|---|
| Platform | Platform | Direct match |
| Fighter | Fighting | Direct match |
| Shooter | Shoot'em Up / Shooter | libretro splits these |
| Driving | Racing | Direct match conceptually |
| Sports | Sports | Direct match |
| Puzzle | Puzzle | Direct match |
| Maze | -- | No libretro equivalent (Pac-Man is "Action" in libretro) |
| Climbing | Platform | Would need mapping |
| Ball & Paddle | -- | No direct equivalent |
| Tabletop | Board / Card | Rough match |
| Quiz | Quiz | Direct match |
| Casino | Gambling | Rough match |

A normalized genre taxonomy would need to:
- Map catver.ini's ~23 primary categories to a common set
- Map libretro's ~29 genres to the same common set
- Handle the "Maze" problem (catver has it, libretro puts maze games under "Action")
- Decide the granularity (catver's subcategories are useful but libretro doesn't have them)

### 9.3 Current Pain: Branching in Real Code

The branching problem is not hypothetical. It already exists in the codebase:

**`game_ref.rs`** -- display name resolution branches on system category:
```rust
if s.category == SystemCategory::Arcade {
    arcade_db::arcade_display_name(&rom_filename)
} else {
    game_db::game_display_name(system, &rom_filename)
}
```

**`server_fns.rs`** -- the game detail endpoint has an `arcade_info: Option<ArcadeMetadata>` that is entirely separate from the base `RomEntry` metadata. The UI component `ArcadeInfoSection` only renders for arcade games. Non-arcade games show no year, no developer, no genre, no player count in the detail view -- even though `game_db` has all of this data.

**`favorites.rs`** -- the `criteria_folder_raw` function for genre and players only queries `game_db`. It never consults `arcade_db`, meaning arcade favorites organized by genre or players always fall into "Other" / "Unknown":
```rust
OrganizeCriteria::Genre => {
    let genre = game_db::lookup_game(system, stem)
        .map(|e| e.game.genre)
        // ... never checks arcade_db
        .unwrap_or("");
    if genre.is_empty() { "Other".to_string() } else { genre.to_string() }
}
```

Every new feature that touches game metadata will face this same fork: "is it arcade? query arcade_db. otherwise? query game_db." As the feature set grows, this branching multiplies.

### 9.4 Unified Data Model at the DB Level: Still Not Worth It

Even with these features in mind, merging `ArcadeGameInfo` and `CanonicalGame` into one struct remains a bad idea. The reasons from sections 1-6 still hold:
- The structs have different shapes for good reasons (rotation, clone/parent, region, CRC32)
- The build pipelines have zero overlap
- A union struct full of Options is strictly worse for type safety and binary size

---

## 10. Recommended Approach: Unify at the Backend API / SSR Layer

The right place to unify is not at the embedded database level (too costly, wrong abstraction) and not with a trait-object query layer in replay-core (adds complexity to the library crate that only the app needs). Instead, unify where the data crosses the server-function boundary: the **backend API types and server functions** that the Leptos SSR app uses to serve pages and respond to client requests.

This approach:
- **Keeps both databases at optimal sizes** -- no struct bloat, no wasted fields, no interleaved build logic
- **Eliminates branching in features** -- server functions resolve metadata from the right DB internally, then return a unified type
- **Is the natural serialization boundary** -- server functions already convert `&'static str` references to owned `String` types for serde. The conversion to a unified struct costs nothing extra
- **Makes future features trivial** -- search, metadata scraping, genre filtering all operate on one type

### 10.1 The Unified `GameInfo` Type

Replace the current split between "base `RomEntry` + optional `ArcadeMetadata`" with a single `GameInfo` struct that every server function returns for game-related data:

```rust
/// Unified game metadata returned by server functions.
/// Populated from arcade_db or game_db depending on the system,
/// but consumers never need to know which source was used.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameInfo {
    // --- Identity (always present) ---
    pub system: String,
    pub system_display: String,
    pub rom_filename: String,
    pub rom_path: String,
    pub display_name: String,   // resolved, never None

    // --- Common metadata (from either DB) ---
    pub year: String,           // "1991", "198?", or "" if unknown
    pub genre: String,          // normalized genre, or raw if no mapping
    pub developer: String,      // manufacturer (arcade) or developer (console)
    pub players: u8,            // 0 = unknown

    // --- Arcade-specific (empty/default for non-arcade) ---
    pub rotation: Option<String>,       // "Horizontal", "Vertical"
    pub driver_status: Option<String>,  // "Working", "Imperfect", "Preliminary"
    pub is_clone: Option<bool>,
    pub parent_rom: Option<String>,
    pub arcade_category: Option<String>, // raw catver.ini category, for detail view

    // --- Console-specific (empty/default for arcade) ---
    pub region: Option<String>,
}
```

Key design decisions:
- **`display_name` is always a `String`, never `Option`** -- the server function is the right place to resolve it with fallback, so the UI never has to handle the "no display name" case.
- **Common fields are non-optional** -- `year`, `genre`, `developer`, `players` exist on both DB types. If unknown, they use empty string / 0. No `Option` wrapping for fields that have a natural "unknown" value.
- **System-specific fields use `Option`** -- `rotation` only makes sense for arcade, `region` only for console. These are `Option` because their absence is semantically meaningful ("this game has no rotation concept"), not just "data unavailable."
- **`arcade_category` preserves the raw catver.ini value** -- the detail view can show "Fighter / Versus" alongside the normalized "Fighting" genre. This avoids losing useful information.

### 10.2 Building `GameInfo` from Each DB

The conversion logic lives in one place -- the server function layer -- not scattered across features:

```rust
/// Resolve full game metadata for any system.
/// This is the single function that bridges arcade_db and game_db.
fn resolve_game_info(system: &str, rom_filename: &str, rom_path: &str) -> GameInfo {
    let sys_info = systems::find_system(system);
    let system_display = sys_info
        .map(|s| s.display_name.to_string())
        .unwrap_or_else(|| system.to_string());
    let is_arcade = sys_info
        .is_some_and(|s| s.category == SystemCategory::Arcade);

    if is_arcade {
        let stem = rom_filename.strip_suffix(".zip").unwrap_or(rom_filename);
        match arcade_db::lookup_arcade_game(stem) {
            Some(info) => GameInfo {
                system: system.to_string(),
                system_display,
                rom_filename: rom_filename.to_string(),
                rom_path: rom_path.to_string(),
                display_name: info.display_name.to_string(),
                year: info.year.to_string(),
                genre: normalize_arcade_genre(info.category),
                developer: info.manufacturer.to_string(),
                players: info.players,
                rotation: Some(format_rotation(info.rotation)),
                driver_status: Some(format_driver_status(info.status)),
                is_clone: Some(info.is_clone),
                parent_rom: if info.is_clone {
                    Some(info.parent.to_string())
                } else {
                    None
                },
                arcade_category: if info.category.is_empty() {
                    None
                } else {
                    Some(info.category.to_string())
                },
                region: None,
            },
            None => GameInfo::unknown(system, &system_display, rom_filename, rom_path),
        }
    } else {
        let stem = rom_filename.rfind('.').map(|i| &rom_filename[..i])
            .unwrap_or(rom_filename);
        let entry = game_db::lookup_game(system, stem);
        let game = entry.map(|e| e.game);
        let region = entry.map(|e| e.region).unwrap_or("");

        GameInfo {
            system: system.to_string(),
            system_display,
            rom_filename: rom_filename.to_string(),
            rom_path: rom_path.to_string(),
            display_name: game.map(|g| g.display_name.to_string())
                .unwrap_or_else(|| rom_filename.to_string()),
            year: game.map(|g| if g.year > 0 {
                g.year.to_string()
            } else {
                String::new()
            }).unwrap_or_default(),
            genre: game.map(|g| g.genre.to_string()).unwrap_or_default(),
            developer: game.map(|g| g.developer.to_string()).unwrap_or_default(),
            players: game.map(|g| g.players).unwrap_or(0),
            rotation: None,
            driver_status: None,
            is_clone: None,
            parent_rom: None,
            arcade_category: None,
            region: if region.is_empty() { None } else { Some(region.to_string()) },
        }
    }
}
```

This is the **only place** that branches on arcade vs. non-arcade. Every other piece of code -- server functions, UI components, search, favorites -- works with `GameInfo` and never needs to know where the data came from.

### 10.3 Impact on Server Functions

**`get_rom_detail`** -- currently returns `RomDetail { rom: RomEntry, is_favorite: bool, arcade_info: Option<ArcadeMetadata> }`. With the unified type, it returns `RomDetail { game: GameInfo, size_bytes: u64, is_m3u: bool, is_favorite: bool }`. The `ArcadeMetadata` type is eliminated. The detail page component receives everything it needs in one struct, regardless of system type.

**`get_roms_page`** -- currently returns `Vec<RomEntry>` where `RomEntry` contains a `GameRef` with `display_name: Option<String>`. With `GameInfo`, the display name is always resolved. Search filtering becomes simpler because `GameInfo.display_name` is always populated:

```rust
let filtered: Vec<GameInfo> = all_games.iter().filter(|g| {
    g.display_name.to_lowercase().contains(&query)
        || g.rom_filename.to_lowercase().contains(&query)
}).cloned().collect();
```

No need to handle `Option<String>` for display_name.

**`get_favorites` / `get_recents`** -- currently return types containing `GameRef`. They would return types containing `GameInfo` (or a slimmed-down subset). Crucially, genre and player count are already resolved, so organize-by-genre works correctly for arcade games without any special handling.

### 10.4 Impact on SSR Components

**Game detail page** -- currently has a separate `ArcadeInfoSection` component that only renders for arcade games. With `GameInfo`, a single component renders metadata for all games. Arcade-specific fields (`rotation`, `driver_status`) render conditionally based on `Option`, but the overall page structure is the same:

```rust
#[component]
fn GameMetaSection(info: GameInfo) -> impl IntoView {
    let has_year = !info.year.is_empty();
    let has_genre = !info.genre.is_empty();
    let has_developer = !info.developer.is_empty();
    let has_players = info.players > 0;

    view! {
        <div class="game-meta-grid">
            // Common fields -- rendered for ALL games
            <Show when=move || has_year>
                <MetaItem label="Year" value=info.year.clone() />
            </Show>
            <Show when=move || has_developer>
                <MetaItem label="Developer" value=info.developer.clone() />
            </Show>
            <Show when=move || has_genre>
                <MetaItem label="Genre" value=info.genre.clone() />
            </Show>
            <Show when=move || has_players>
                <MetaItem label="Players" value=info.players.to_string() />
            </Show>

            // Arcade-specific -- only rendered when present
            {info.rotation.clone().map(|r| view! {
                <MetaItem label="Rotation" value=r />
            })}
            {info.driver_status.clone().map(|s| view! {
                <MetaItem label="Status" value=s />
            })}
            {info.region.clone().map(|r| view! {
                <MetaItem label="Region" value=r />
            })}
        </div>
    }
}
```

The key improvement: non-arcade games now display year, developer, genre, and player count on their detail page. Currently they show none of this despite game_db having the data.

### 10.5 Impact on Search

Global search across all systems becomes straightforward. A single server function queries both databases and returns `Vec<GameInfo>`:

```rust
#[server(prefix = "/sfn")]
pub async fn search_games(query: String, limit: usize) -> Result<Vec<GameInfo>, ServerFnError> {
    let q = query.to_lowercase();
    let mut results = Vec::new();

    // Search arcade DB
    for (rom_name, info) in arcade_db::ARCADE_DB.entries() {
        if info.display_name.to_lowercase().contains(&q)
            || rom_name.to_lowercase().contains(&q) {
            results.push(resolve_game_info("arcade", rom_name, &format!("/{rom_name}.zip")));
        }
    }

    // Search all game_db systems
    for system in game_db::supported_systems() {
        if let Some(db) = game_db::get_system_db(system) {
            for (stem, entry) in db.entries() {
                if entry.game.display_name.to_lowercase().contains(&q)
                    || stem.to_lowercase().contains(&q) {
                    results.push(resolve_game_info(system, stem, ""));
                }
            }
        }
    }

    results.truncate(limit);
    Ok(results)
}
```

The caller (a Leptos component) receives `Vec<GameInfo>` and renders each result identically. No branching on system type in the UI.

### 10.6 Impact on Favorites Organization

The `criteria_folder_raw` function in `favorites.rs` currently only queries `game_db` for genre and players. With the unified approach, it would call `resolve_game_info` (or a lighter-weight variant that only resolves the needed field) and get consistent results for both arcade and non-arcade games:

```rust
OrganizeCriteria::Genre => {
    let info = resolve_game_info(system, rom_filename, "");
    if info.genre.is_empty() { "Other".to_string() } else { info.genre }
}
OrganizeCriteria::Players => {
    let info = resolve_game_info(system, rom_filename, "");
    match info.players {
        0 => "Unknown".to_string(),
        1 => "1 Player".to_string(),
        2 => "2 Players".to_string(),
        n => format!("{n} Players"),
    }
}
```

This fixes the existing bug where arcade favorites always get "Other" genre and "Unknown" players.

### 10.7 Impact on Future Metadata Scraping

When adding external metadata (box art URLs, descriptions, ratings) from APIs like TheGamesDB, ScreenScraper, or IGDB, the scraping system needs to:
1. Identify the game (by title, system, year)
2. Fetch metadata
3. Cache/store results
4. Serve to the UI

With `GameInfo` as the common identity type, step 1 works identically for arcade and non-arcade games. The scraper can match on `(display_name, year, system)` without caring whether the original source was arcade_db or game_db. Cached results can be keyed by `(system, rom_filename)` -- a key that `GameInfo` always provides.

Without unification, the scraper would need two code paths: one that takes `ArcadeGameInfo` fields and another that takes `CanonicalGame` fields, constructing search queries differently for each.

### 10.8 Build-Time Genre Normalization (Prerequisite)

For genre to be useful across both databases, the taxonomy must be consistent. This is best done at build time:

Add a `normalized_genre: &'static str` field to both `ArcadeGameInfo` and `CanonicalGame`. During the build, map catver.ini categories and libretro genres to a shared taxonomy. The raw `category`/`genre` fields stay for system-specific views.

A reasonable shared taxonomy (~18 genres):

```
Action, Adventure, Beat'em Up, Board & Card, Driving, Educational,
Fighting, Maze, Music, Pinball, Platform, Puzzle, Quiz,
Role-Playing, Shooter, Simulation, Sports, Strategy, Other
```

The `resolve_game_info` function then reads `normalized_genre` for the `genre` field, while the raw category is available via `arcade_category` for the arcade detail view.

Implementation cost: ~60 lines in build.rs (a mapping function + wiring into both output paths). One new field on each struct.

---

## 11. What This Approach Buys

| Concern | Current (two DBs, no abstraction) | Unified API layer |
|---|---|---|
| Display name resolution | Branch in `game_ref.rs` | `resolve_game_info()` -- one call |
| Game detail page | `RomEntry` + `Option<ArcadeMetadata>`, two UI paths | `GameInfo` -- one UI path, one component |
| Non-arcade metadata display | **Not shown** (year, developer, genre, players hidden despite being in game_db) | Always shown when available |
| Global search | Not implemented, would need dual iteration + merge | Single server function, uniform results |
| Genre filtering | Not implemented, incompatible taxonomies | Normalized genre field, one filter |
| Players filtering | `favorites.rs` only queries game_db, arcade always "Unknown" | Works for all systems |
| Favorites by genre | Arcade favorites always "Other" | Correct genre for all |
| Future metadata scraping | Two code paths for game identification | One path via `GameInfo` identity |
| New cross-cutting feature | Fork on system type at every call site | Operate on `GameInfo` uniformly |

## 12. What Stays Separate

This approach deliberately does **not** touch:

- **The embedded databases** -- `arcade_db.rs` and `game_db.rs` in replay-core remain separate modules with separate structs, separate PHF maps, separate build pipelines. Their binary footprint stays optimal.
- **The build.rs pipeline** -- no interleaving of arcade and console parsing logic. The only addition is a shared genre normalization function called from both output paths.
- **System-specific lookup APIs** -- `lookup_arcade_game()`, `game_db::lookup_game()`, and `game_db::lookup_by_crc()` stay as-is. They are still useful for targeted queries.
- **Type safety in replay-core** -- `ArcadeGameInfo` still has `rotation: Rotation` (an enum), not `Option<String>`. `CanonicalGame` still has `year: u16`, not a string. The type erasure only happens at the serialization boundary.

## 13. Implementation Plan

### Step 1: Build-time genre normalization (~60 lines)
- Add `normalized_genre: &'static str` to `ArcadeGameInfo` and `CanonicalGame`
- Add genre mapping function to build.rs
- Wire into both arcade and game_db code generation

### Step 2: Define `GameInfo` type (~30 lines)
- Add `GameInfo` struct in `server_fns.rs` (server side) and `types.rs` (client mirror)
- Define `resolve_game_info()` helper function (~80 lines)
- This is the only function that branches on arcade vs. non-arcade

### Step 3: Migrate `get_rom_detail` to return `GameInfo` (~50 lines changed)
- Replace `RomDetail { rom, arcade_info }` with `RomDetail { game: GameInfo, size_bytes, is_m3u, is_favorite }`
- Remove `ArcadeMetadata` type
- Update `GameDetailContent` component to use unified `GameInfo` fields
- Non-arcade games gain year/developer/genre/players display for free

### Step 4: Migrate `get_roms_page` (~30 lines changed)
- `RomPage` returns `Vec<GameInfo>` (or `Vec<RomListEntry>` with `GameInfo` subset + file metadata)
- Search filtering uses `GameInfo.display_name` directly (never None)

### Step 5: Fix favorites genre/players for arcade (~20 lines changed)
- Update `criteria_folder_raw` in `favorites.rs` to use `arcade_db` when the system is arcade
- Or, have it call a shared `resolve_metadata()` function that bridges both DBs

### Step 6: Add global search server function (~40 lines)
- New `search_games(query, limit)` server function
- Queries both DBs, returns `Vec<GameInfo>`
- UI component renders results uniformly

**Total estimated change: ~300 lines of new/modified code across 4-5 files.**

## 14. Final Recommendation

**Keep arcade_db and game_db as separate embedded databases** (the original recommendation from sections 1-8 stands). **Unify at the backend API and SSR layer** by introducing a `GameInfo` type that server functions return for all game-related queries.

The databases are separate for good structural reasons: different data models, different lookup patterns, different build pipelines, and different binary size constraints. The unification point is the server function boundary, where data is already being serialized to owned types. This is where the "arcade vs. non-arcade" branching should be absorbed -- once, in `resolve_game_info()` -- rather than leaked into every feature that touches game metadata.

The concrete benefits:
1. Non-arcade games gain metadata display in the detail view (year, developer, genre, players) -- data that already exists in game_db but is currently not surfaced
2. Arcade favorites correctly organize by genre and player count (currently broken)
3. Global search and genre filtering become straightforward to implement
4. Future features (metadata scraping, ratings, recommendations) operate on one type
5. UI components become simpler -- one game info section, not a conditional arcade section
