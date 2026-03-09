# Arcade DB / Game DB Unification Analysis

Can arcade_db and game_db be merged into a single system? The original analysis (sections 1-8) concluded "keep separate" based on data model differences alone. Section 9 re-evaluates in light of cross-cutting features that need to query both databases uniformly. **Revised recommendation: keep the databases separate, but build a unified query layer and normalize genre taxonomy at build time.**

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
- Primary: filename stem lookup. Fallback: CRC32 index lookup
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

## 5. What Breaks If We Merge

Direct API consumers:
- `game_ref.rs`: branches on `SystemCategory::Arcade` to choose arcade_db vs game_db -- a unified DB would simplify this one branch
- `server_fns.rs`: directly calls `lookup_arcade_game()` and reads `Rotation` variants for the game detail view
- All arcade_db tests (14 tests) reference `ArcadeGameInfo` fields, `Rotation`, `DriverStatus`

A unified struct would need `Option<Rotation>`, `Option<DriverStatus>`, `Option<bool>` for is_clone, `Option<&str>` for parent, `Option<&str>` for region, `Option<u32>` for crc32 -- a struct full of Options that is worse than two focused structs.

## 6. Size Impact

Currently: arcade_db PHF map (~2.2 MB) + game_db PHF maps (~1.5 MB) = ~3.7 MB total.

A unified struct would be **larger** because every arcade entry gains unused region/crc32 fields, and every console entry gains unused rotation/status/is_clone/parent fields. PHF map overhead is proportional to entry count, which stays the same. No savings from unification.

## 7. What Unification Would Actually Buy

The only concrete win: `game_ref.rs` could drop the `if arcade { ... } else { ... }` branch (6 lines). Everything else gets worse -- larger structs, more Options, interleaved build logic, loss of type safety (rotation on a console game makes no sense).

## 8. Recommendation: Keep Separate

**Keep arcade_db and game_db as separate modules.** They solve different problems for different system types with different data sources and different access patterns. The small amount of conceptual duplication (both are "game metadata databases with PHF maps") is not worth the complexity of a forced unification.

If the two-branch pattern in `game_ref.rs` bothers us, the right fix is a thin trait or wrapper that abstracts "resolve display name for a ROM" -- not merging the underlying databases.

## 9. Cross-Cutting Features: The Case for a Unified Query Layer

The original analysis focused on structural differences and concluded there was no practical benefit to unification. But several planned features expose the cost of having two separate, incompatible databases with no shared abstraction:

### 9.1 Features That Need Cross-DB Queries

1. **Global search by name** -- search the entire library (arcade + console + handheld) by game title. Currently requires querying `arcade_db::ARCADE_DB` (single flat map, 25K+ entries) and then each per-system PHF map in `game_db` separately. No unified iterator or search function exists.

2. **Filter/browse by genre** -- show all "Platform" games, all "RPGs", etc. across every system. arcade_db stores `category` (e.g., "Platform / Run Jump", "Fighter / Versus"); game_db stores `genre` (e.g., "Platform", "Fighting"). Different field names, different taxonomies, different granularity.

3. **Filter by number of players** -- "show me all 4-player games." Both databases have a `players: u8` field but in different structs (`ArcadeGameInfo` vs `CanonicalGame`), so a cross-system query must branch on system type.

4. **Organize favorites by genre** -- group the user's favorites not just by system but by genre ("All my platformers", "All my RPGs"). The `Favorite` struct contains a `GameRef` which has no genre field. Resolving genre requires a DB lookup that differs by system type, and the genre values themselves are incompatible between arcade and non-arcade.

5. **Cross-system game discovery** -- "show me all RPGs" or "all 2-player co-op games" regardless of platform. This is the general case of (2) and (3) combined.

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

### 9.3 Two Separate DBs: What These Features Cost

Without any unification, implementing the five features above requires:

**Global search:**
```rust
fn search_all(query: &str) -> Vec<SearchResult> {
    let mut results = Vec::new();
    // Search arcade DB
    for entry in arcade_db::ARCADE_DB.entries() {
        if entry.1.display_name.contains(query) {
            results.push(SearchResult::from_arcade(entry.1));
        }
    }
    // Search every game_db system
    for system in game_db::supported_systems() {
        // Need access to per-system PHF maps -- not currently exposed
        // Would need new game_db::iter_system(system) API
    }
    results
}
```

Every cross-cutting query follows this same pattern: iterate arcade_db, then iterate each game_db system, converting results to some common type. The conversion logic (different field names, different genre taxonomy) is duplicated in every feature.

**Genre filtering** is worse because you can't just compare strings. You'd need a mapping function at every call site:
```rust
fn normalize_genre(system: &str, raw: &str) -> &str {
    if is_arcade(system) {
        map_catver_to_common(raw) // "Platform / Run Jump" -> "Platform"
    } else {
        map_libretro_to_common(raw) // "Role-playing (RPG)" -> "RPG"
    }
}
```

**Favorites by genre** requires resolving metadata for each favorite (currently just a `GameRef` with system + filename), then normalizing the genre. Every new grouping/filtering dimension adds another branch.

**Estimated cost with two DBs:** Each of the five features requires (a) dual iteration/lookup, (b) result type conversion, (c) genre normalization at runtime. This means ~5 instances of the "query both DBs and merge" pattern, each with its own ad-hoc conversion. Probably 200-300 lines of boilerplate total, plus the runtime genre mapping.

### 9.4 Unified Data Model: Still Not Worth It

Even with these features in mind, merging `ArcadeGameInfo` and `CanonicalGame` into one struct remains a bad idea. The reasons from sections 1-6 still hold:
- The structs have different shapes for good reasons (rotation, clone/parent, region, CRC32)
- The build pipelines have zero overlap
- A union struct full of Options is strictly worse for type safety and binary size

### 9.5 Recommended Approach: Thin Unified Query Layer + Build-Time Genre Normalization

The right solution has two parts:

**Part A: Normalize genre at build time.** Add a `normalized_genre: &'static str` field to both `ArcadeGameInfo` and `CanonicalGame`. During the build, map catver.ini categories and libretro genres to a shared taxonomy. This is a one-time cost in `build.rs` with zero runtime overhead. The raw `category`/`genre` fields stay for system-specific views. A reasonable shared taxonomy (~15 genres):

```
Action, Adventure, Beat'em Up, Board & Card, Driving, Educational,
Fighting, Maze, Music, Pinball, Platform, Puzzle, Quiz,
Role-Playing, Shooter, Simulation, Sports, Strategy, Other
```

This can be generated by a `fn normalize_genre(raw: &str, source: GenreSource) -> &str` function in build.rs that pattern-matches catver primary categories and libretro genre strings.

**Part B: A `GameMetadata` trait for cross-DB queries.** Define a trait that both database types implement:

```rust
/// Common metadata available for any game, regardless of system type.
pub trait GameMetadata {
    fn display_name(&self) -> &str;
    fn year_str(&self) -> &str; // arcade returns &str directly, console formats u16
    fn normalized_genre(&self) -> &str;
    fn players(&self) -> u8;
    fn system(&self) -> &str; // which system this game belongs to
}
```

Then provide unified query functions in a new `game_query` module:

```rust
/// Search all games (arcade + console) by display name substring.
pub fn search_all(query: &str) -> Vec<&'static dyn GameMetadata> { ... }

/// Iterate all games matching a normalized genre.
pub fn games_by_genre(genre: &str) -> Vec<&'static dyn GameMetadata> { ... }

/// Iterate all games with at least N players.
pub fn games_by_min_players(n: u8) -> Vec<&'static dyn GameMetadata> { ... }
```

The underlying databases stay separate. The trait is the unification point. Each query function iterates both databases internally, but callers see a single stream of results.

**Alternative to trait objects:** If `dyn GameMetadata` feels too heavy, a `GameSummary` struct could work instead -- a lightweight copy-out type populated from either DB:

```rust
pub struct GameSummary {
    pub display_name: &'static str,
    pub system: &'static str,
    pub normalized_genre: &'static str,
    pub players: u8,
    pub year: &'static str,
}
```

This avoids vtable overhead and is simpler to serialize for server functions. The conversion from `ArcadeGameInfo`/`CanonicalGame` to `GameSummary` happens once per query, not once per access.

### 9.6 What This Buys

| Feature | Two DBs, no abstraction | Two DBs + query layer |
|---|---|---|
| Global search | Dual iteration + ad-hoc merge | `game_query::search_all(q)` |
| Genre filter | Dual iteration + runtime genre mapping | `game_query::games_by_genre(g)` |
| Players filter | Dual iteration + struct branching | `game_query::games_by_min_players(n)` |
| Favorites by genre | Resolve metadata per-fav + normalize | Resolve metadata per-fav (same), but `normalized_genre` is a direct field read |
| Cross-system discovery | Combination of above, all ad-hoc | Compose query functions |

The query layer eliminates the "branch on system type, query different DB, convert result" pattern at every call site. Build-time genre normalization eliminates runtime mapping entirely.

### 9.7 Implementation Cost

- **Build-time genre normalization:** ~60 lines in build.rs (a mapping function + wiring into both output paths). Add `normalized_genre: &'static str` to both structs. Low risk, no architectural change.
- **GameSummary struct + query module:** ~150 lines for the struct, iterator adapters over both DBs, and the search/filter functions. Requires exposing iteration over arcade_db entries and per-system game_db entries (currently only key-based lookup is public).
- **Exposing iteration:** arcade_db needs `pub fn iter() -> impl Iterator<Item = &'static ArcadeGameInfo>`. game_db needs `pub fn iter_system(system: &str) -> impl Iterator<Item = &'static GameEntry>` or similar. Both are trivial wrappers around PHF map iteration.

Total: ~200-250 lines of new code, no changes to existing data structures beyond one new field each.

## 10. Revised Recommendation

**Keep arcade_db and game_db as separate modules** (original recommendation stands). **Add a unified query layer on top** (new recommendation).

Specifically:
1. Add `normalized_genre` field to both `ArcadeGameInfo` and `CanonicalGame`, populated at build time from a shared mapping function.
2. Expose iteration APIs on both databases.
3. Add a `game_query` module with `GameSummary` and cross-DB search/filter functions.

This gets the benefits of unification (single API for cross-cutting features, consistent genre taxonomy) without the costs (merged structs, interleaved build logic, loss of type safety). The databases stay separate where they should be separate; the query layer unifies where the application needs it unified.
