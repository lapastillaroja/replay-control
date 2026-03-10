# Region Preference Analysis

> Status: Proposal
> Date: 2026-03-10

## Problem Statement

The Replay Control app currently hardcodes a USA-centric region priority order: World > USA > Europe > Japan > Other > Unknown. This ordering is baked into two critical code paths:

1. **ROM list sorting** (`roms.rs:list_roms()`) -- when browsing a system, ROMs with the same display name are sorted by `RegionPriority`, which places USA before Europe and Japan.
2. **Search scoring** (`server_fns.rs:search_score()`) -- search results receive region bonuses: World +20, USA +15, Europe +10, Japan +5. An American user searching "Super Mario World" sees `(USA)` first, but a Japanese user also sees `(USA)` first.

For a user in Japan, the ideal experience would be to see `(Japan)` ROMs at the top of both the browse list and search results. For a European user, `(Europe)` ROMs should surface first. The current fixed priority forces all users into a USA-first experience.

This is especially important for two reasons:

- **Language**: Japan-region ROMs are in Japanese. A Japanese user browsing their collection wants to see the Japanese version first, not the English (USA) version. The same applies to European users who want the multi-language European release.
- **Compatibility**: Some ROMs behave differently by region (PAL vs NTSC timing, 50Hz vs 60Hz). Users generally want the ROM that matches their display setup.

---

## Current Behavior

### RegionPriority Enum

**File**: `replay-control-core/src/rom_tags.rs`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RegionPriority {
    World = 0,
    Usa = 1,
    Europe = 2,
    Japan = 3,
    Other = 4,
    Unknown = 5,
}
```

The `Ord` derivation uses discriminant values, so sorting by `RegionPriority` always produces `World < Usa < Europe < Japan < Other < Unknown`. This ordering is used directly in two places.

### ROM List Sort

**File**: `replay-control-core/src/roms.rs` (line 86-94)

```rust
roms.sort_by(|a, b| {
    // ...
    a_name.to_lowercase().cmp(&b_name.to_lowercase())
        .then(a_tier.cmp(&b_tier))
        .then(a_region.cmp(&b_region))  // <-- hardcoded USA-first
});
```

When multiple ROMs share the same display name and tier (e.g., "Super Mario World" with tier `Original` in USA, Europe, and Japan), the region sort determines the final order. Today this is always World > USA > Europe > Japan.

### Search Score

**File**: `replay-control-app/src/server_fns.rs` (line 458-466)

```rust
let region_bonus = match region {
    RegionPriority::World => 20,
    RegionPriority::Usa => 15,
    RegionPriority::Europe => 10,
    RegionPriority::Japan => 5,
    RegionPriority::Other => 0,
    RegionPriority::Unknown => 0,
};
```

The region bonus can change the relative order of search results. When two ROMs have the same base match score and tier, the 10-point gap between USA (+15) and Japan (+5) is enough to push USA above Japan.

### classify() and region_to_priority()

**File**: `replay-control-core/src/rom_tags.rs` (line 164-178)

The `region_to_priority()` function maps filename tags like `(USA)`, `(Europe)`, `(Japan)` to `RegionPriority` variants. Multi-region tags like `(USA, Europe)` are classified by the first region listed. This function itself is not region-biased -- it is a neutral classifier. The bias comes from the `Ord` implementation on `RegionPriority`.

---

## Proposed Solution

### Core Concept: User-Configurable Region Preference

Add a "preferred region" setting that controls the sort order and search scoring for region variants. The setting maps to an enum with these values:

```rust
/// User's preferred region for sorting and search prioritization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RegionPreference {
    #[default]
    Usa,
    Europe,
    Japan,
    World,
}
```

Note: this is a **preference**, not a filter. All ROMs remain visible regardless of the setting. The preference only affects sort order and search ranking.

The `World` option is included because some users genuinely prefer World releases (which often contain all languages and have the fewest regional quirks).

### Region Priority Function

Replace the hardcoded `Ord` derivation with a function that returns a priority score based on the user's preference:

**File**: `replay-control-core/src/rom_tags.rs`

```rust
impl RegionPriority {
    /// Return a sort key for this region given the user's preference.
    /// Lower value = shown first.
    pub fn sort_key(self, pref: RegionPreference) -> u8 {
        match pref {
            RegionPreference::Usa => match self {
                RegionPriority::World => 0,
                RegionPriority::Usa => 1,
                RegionPriority::Europe => 2,
                RegionPriority::Japan => 3,
                RegionPriority::Other => 4,
                RegionPriority::Unknown => 5,
            },
            RegionPreference::Europe => match self {
                RegionPriority::World => 0,
                RegionPriority::Europe => 1,
                RegionPriority::Usa => 2,
                RegionPriority::Japan => 3,
                RegionPriority::Other => 4,
                RegionPriority::Unknown => 5,
            },
            RegionPreference::Japan => match self {
                RegionPriority::World => 0,
                RegionPriority::Japan => 1,
                RegionPriority::Usa => 2,
                RegionPriority::Europe => 3,
                RegionPriority::Other => 4,
                RegionPriority::Unknown => 5,
            },
            RegionPreference::World => match self {
                RegionPriority::World => 0,
                RegionPriority::Usa => 1,
                RegionPriority::Europe => 2,
                RegionPriority::Japan => 3,
                RegionPriority::Other => 4,
                RegionPriority::Unknown => 5,
            },
        }
    }
}
```

Design rationale:
- **World always sorts first** (or ties for first when preferred). World releases are region-neutral and typically contain all languages. A user who prefers Japan still benefits from seeing `(World)` at the top since it includes Japanese.
- **The preferred region sorts second**, immediately after World.
- **The remaining major regions follow** in a stable secondary order (USA > Europe > Japan for non-preferred regions). This keeps the sort deterministic.
- **Other and Unknown always sort last** -- these are niche regional variants (Spain, Brazil, Korea) or ROMs without region tags.

### Search Score Adjustment

**File**: `replay-control-app/src/server_fns.rs`

```rust
fn region_bonus(region: RegionPriority, pref: RegionPreference) -> u32 {
    match region.sort_key(pref) {
        0 => 20, // World (or preferred when World is the preference)
        1 => 15, // User's preferred region
        2 => 10, // Second-best major region
        3 => 5,  // Third major region
        _ => 0,  // Other / Unknown
    }
}
```

This preserves the existing bonus spread (20/15/10/5/0) while remapping which region gets which bonus. The relative scoring dynamics stay the same -- only the assignment changes.

---

## Default Behavior Analysis

### Option A: Default to USA (Recommended)

Set `RegionPreference::Usa` as the default. This preserves backward compatibility: existing users see no change until they explicitly choose a different preference.

Pros:
- No surprise changes for the existing user base (which is predominantly English-speaking, based on the app having English-only i18n).
- Simple and predictable.
- The app currently only supports English, so a USA default aligns with the only available language.

Cons:
- New users in other regions must discover and change the setting manually.

### Option B: Auto-detect from browser locale

Read `Accept-Language` on the server side (or `navigator.language` on the client) and map `ja` to Japan, `de`/`fr`/`it`/`es` to Europe, etc.

Pros:
- Better out-of-box experience for non-US users.

Cons:
- Browser locale does not reliably indicate ROM region preference. A bilingual user with `en-US` locale might still prefer Japan-region ROMs.
- On a RePlayOS system connected to a TV, the browser is often a phone on the same network. The phone's locale may differ from the user's ROM preference.
- Adds complexity for marginal benefit.
- The mapping from language to ROM region is lossy (what does `pt-BR` map to? There is no "Brazil" preference; it would need to fall back to USA or Other).

### Option C: Auto-detect from RePlayOS system locale

Read a `system_language` or `system_region` key from `replay.cfg`.

Pros:
- Aligns with the device's own locale setting.

Cons:
- RePlayOS currently has no language/region setting in `replay.cfg`. This would require upstream RePlayOS support.
- The app runs in a browser, possibly remotely -- it may not have access to the same config the OS uses.

### Recommendation

**Default to USA** (Option A). When i18n support expands beyond English, revisit auto-detection. The setting is easy enough to change manually, and a wrong auto-detected default is worse than a predictable default that requires one-time configuration.

---

## Settings Storage Design

### Where to Store the Setting

**Important constraint**: `replay.cfg` belongs to RePlayOS and must NOT be modified by the companion app for app-specific settings. Only RePlayOS-native settings (skin, wifi, video mode) should live there.

| Location | Pros | Cons |
|----------|------|------|
| `replay.cfg` | Shared with RePlayOS | **Not allowed** for app-specific settings |
| `.replay-control/config.cfg` | App-specific, same format as replay.cfg, lives alongside metadata.db | Not shared with RePlayOS (which is correct) |
| In-memory (AppState) | Simplest implementation | Lost on server restart |
| Cookie / localStorage | Client-side, no server changes | Different per browser; lost when clearing cookies |

**Recommendation**: Store in `.replay-control/config.cfg` using the same `key = "value"` format as `replay.cfg`. This keeps app-specific settings separate from the OS config, avoids any risk of breaking RePlayOS, and reuses the existing `ReplayConfig` parser. The `.replay-control/` directory already exists on ROM storage for metadata.db and media files.

### Config File

**Path**: `<storage>/.replay-control/config.cfg`

```
# Replay Control app settings
region_preference = "usa"
```

Valid values: `"usa"`, `"europe"`, `"japan"`, `"world"`. Default when missing: `"usa"`.

### AppConfig Abstraction

**File**: `replay-control-core/src/config.rs`

Add a new `AppConfig` struct (or reuse the `ReplayConfig` parser) that reads from `.replay-control/config.cfg`:

```rust
/// App-specific settings stored in `.replay-control/config.cfg`.
/// Uses the same key = "value" format as replay.cfg but is separate
/// to avoid modifying the RePlayOS system configuration.
pub struct AppConfig {
    inner: ReplayConfig,
    path: PathBuf,
}

impl AppConfig {
    pub fn open(storage_root: &Path) -> Result<Self> {
        let path = storage_root.join(RC_DIR).join("config.cfg");
        let inner = if path.exists() {
            ReplayConfig::from_file(&path)?
        } else {
            ReplayConfig::parse("")?
        };
        Ok(Self { inner, path })
    }

    pub fn region_preference(&self) -> RegionPreference {
        match self.inner.get("region_preference").unwrap_or("usa") {
            "europe" => RegionPreference::Europe,
            "japan" => RegionPreference::Japan,
            "world" => RegionPreference::World,
            _ => RegionPreference::Usa,
        }
    }

    pub fn set_region_preference(&mut self, pref: &str) -> Result<()> {
        self.inner.set("region_preference", pref);
        self.save()
    }

    fn save(&self) -> Result<()> {
        // Write using the existing write_to_file method or direct write
    }
}
```

### AppState Integration

**File**: `replay-control-app/src/api/mod.rs`

```rust
impl AppState {
    pub fn region_preference(&self) -> RegionPreference {
        self.app_config.read().expect("lock").region_preference()
    }
}
```

### Cache Invalidation

The `RomCache` caches sorted ROM lists. When the region preference changes, the cached sort order becomes stale. Two options:

1. **Invalidate on change**: When `set_region_preference` is called, also call `self.cache.invalidate()`. Simple and correct. The next request triggers a re-scan and re-sort.

2. **Sort at query time**: Move the sort from `list_roms()` (core layer) to `get_roms_page()` (app layer). This way the cache stores an unsorted list and sorting happens per-request with the current preference. This is more efficient if the preference changes often but avoids unnecessary re-scans.

**Recommendation**: Option 1 (invalidate on change). The preference changes rarely (once during initial setup, then never), so the one-time cache miss cost is negligible. This keeps the architecture simple and avoids changing the data flow.

---

## Impact on Sort and Search

### list_roms() Sort Change

**File**: `replay-control-core/src/roms.rs`

The `list_roms()` function currently does not accept a region preference. It needs to, since it is called from the cache layer.

Option A: Add a `RegionPreference` parameter to `list_roms()`.

```rust
pub fn list_roms(
    storage: &StorageLocation,
    system_folder: &str,
    region_pref: RegionPreference,
) -> Result<Vec<RomEntry>> {
    // ...
    roms.sort_by(|a, b| {
        let (a_tier, a_region) = rom_tags::classify(&a.game.rom_filename);
        let (b_tier, b_region) = rom_tags::classify(&b.game.rom_filename);
        a_name.to_lowercase().cmp(&b_name.to_lowercase())
            .then(a_tier.cmp(&b_tier))
            .then(a_region.sort_key(region_pref).cmp(&b_region.sort_key(region_pref)))
    });
    // ...
}
```

Option B: Keep `list_roms()` unchanged and sort in the app layer.

**Recommendation**: Option A. The core layer already sorts, so it should sort correctly. The app layer passes the preference down. The `RomCache::get_roms()` method would also need the parameter, which naturally invalidates the cache when the preference changes (since the cache key could include the preference, or the cache is simply cleared on preference change).

However, adding a parameter to `list_roms()` means the core crate needs to know about `RegionPreference`. Since `RegionPreference` is closely related to `RegionPriority` (both live in the same domain), defining it in the core crate alongside `RegionPriority` in `rom_tags.rs` is natural.

### search_score() Change

**File**: `replay-control-app/src/server_fns.rs`

Add a `RegionPreference` parameter:

```rust
fn search_score(
    query: &str,
    display_name: &str,
    filename: &str,
    region_pref: RegionPreference,
) -> u32 {
    // ... (existing logic) ...

    let region_bonus = match region.sort_key(region_pref) {
        0 => 20,
        1 => 15,
        2 => 10,
        3 => 5,
        _ => 0,
    };

    (base + length_bonus + region_bonus).saturating_sub(tier_penalty)
}
```

The caller (`get_roms_page`) reads the preference from AppState:

```rust
let region_pref = state.region_preference();
let score = search_score(&q, display, &r.game.rom_filename, region_pref);
```

### Global Search (Future)

If/when a global search function is added, it would also accept the region preference. The pattern is the same -- read from AppState, pass to the scoring function.

---

## UI Design

### Settings Page Entry

**File**: `replay-control-app/src/pages/more.rs`

Add a menu item in the More page:

```rust
<MenuItem icon="\u{1F30D}" label_key="more.region" href=Some("/more/region") />
```

Position it near the top of the menu list, after "Skin" and before "Wi-Fi Configuration", since it affects the core browsing experience.

### Region Preference Page

**File**: `replay-control-app/src/pages/region.rs` (new file)

The page follows the same pattern as the Skin page: load current value, display options, save on selection.

```
+-------------------------------------+
|  <- Back          Region            |
+-------------------------------------+
|                                      |
|  Select your preferred region.       |
|  ROMs from this region appear first  |
|  in game lists and search results.   |
|                                      |
|  +-------------------------------+   |
|  |  [*] USA                      |   |
|  |  [ ] Europe                   |   |
|  |  [ ] Japan                    |   |
|  |  [ ] World                    |   |
|  +-------------------------------+   |
|                                      |
|  [Region saved]                      |
|                                      |
+-------------------------------------+
```

The UI is a simple radio button group. No complex grid like the Skin page -- just four options with labels.

### Component Structure

```rust
#[component]
pub fn RegionPage() -> impl IntoView {
    let i18n = use_i18n();
    let current = Resource::new(|| (), |_| server_fns::get_region_preference());

    view! {
        <div class="page settings-page">
            <div class="rom-header">
                <A href="/more" attr:class="back-btn">
                    {move || t(i18n.locale.get(), "games.back")}
                </A>
                <h2 class="page-title">{move || t(i18n.locale.get(), "region.title")}</h2>
            </div>
            <p class="form-hint">{move || t(i18n.locale.get(), "region.hint")}</p>
            <ErrorBoundary fallback=|errors| view! { <ErrorDisplay errors /> }>
                <Suspense fallback=move || view! { <div class="loading">"..."</div> }>
                    {move || Suspend::new(async move {
                        let pref = current.await?;
                        Ok::<_, ServerFnError>(view! { <RegionSelector current=pref /> })
                    })}
                </Suspense>
            </ErrorBoundary>
        </div>
    }
}

#[component]
fn RegionSelector(current: String) -> impl IntoView {
    let i18n = use_i18n();
    let active = RwSignal::new(current);
    let saving = RwSignal::new(false);
    let status = RwSignal::new(Option::<(bool, String)>::None);

    let options = [
        ("usa", "region.usa"),
        ("europe", "region.europe"),
        ("japan", "region.japan"),
        ("world", "region.world"),
    ];

    let on_select = move |value: String| {
        if saving.get_untracked() || active.get_untracked() == value {
            return;
        }
        saving.set(true);
        status.set(None);
        let v = value.clone();
        leptos::task::spawn_local(async move {
            match server_fns::set_region_preference(v.clone()).await {
                Ok(()) => {
                    active.set(v);
                    let locale = use_i18n().locale.get_untracked();
                    status.set(Some((true, t(locale, "region.saved").to_string())));
                }
                Err(e) => {
                    status.set(Some((false, e.to_string())));
                }
            }
            saving.set(false);
        });
    };

    // Render radio options...
}
```

### Server Functions

```rust
#[server(prefix = "/sfn")]
pub async fn get_region_preference() -> Result<String, ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    let pref = state.region_preference();
    Ok(match pref {
        RegionPreference::Usa => "usa",
        RegionPreference::Europe => "europe",
        RegionPreference::Japan => "japan",
        RegionPreference::World => "world",
    }.to_string())
}

#[server(prefix = "/sfn")]
pub async fn set_region_preference(value: String) -> Result<(), ServerFnError> {
    let state = expect_context::<crate::api::AppState>();
    state.update_config(|cfg| {
        cfg.set("region_preference", &value);
    }).map_err(|e| ServerFnError::new(e.to_string()))?;
    state.cache.invalidate(); // re-sort with new preference
    Ok(())
}
```

Note: Both server functions need `register_explicit` in `main.rs` since they are defined in the app crate's library (same pattern as the metadata server functions).

### i18n Keys

```rust
// Region preference
"more.region" => "Region",
"region.title" => "Region Preference",
"region.hint" => "ROMs from your preferred region appear first in game lists and search results.",
"region.usa" => "USA",
"region.europe" => "Europe",
"region.japan" => "Japan",
"region.world" => "World",
"region.saved" => "Region preference saved",
```

---

## Edge Cases

### Multi-Region ROMs: `(USA, Europe)`

The `region_to_priority()` function classifies multi-region tags by their first listed region. `(USA, Europe)` maps to `RegionPriority::Usa`. This is already the correct behavior: a multi-region ROM that includes the user's preferred region should sort favorably.

The first-region heuristic works well because ROM naming conventions list the "primary" region first. `(USA, Europe)` is primarily a USA release that also works in Europe -- not the other way around.

No change needed for multi-region handling.

### `(World)` ROMs

World releases always sort first regardless of the user's preference (sort key 0 in all preference modes). This is intentional: `(World)` means "all regions" and is the most universally compatible. A user who prefers Japan still benefits from `(World)` appearing at the top since it includes Japanese content.

The only exception is when the user explicitly sets `RegionPreference::World` -- in that case, World is both the preference and the top sort position, which is a no-op (World was already first).

### No Region Tag

ROMs without any region tag get `RegionPriority::Unknown`, which always sorts last (sort key 5). This is correct: if we cannot determine the region, it should not be prioritized over ROMs with known regions.

### Translation ROMs

A ROM like `(Japan) (Translated En)` has `RegionPriority::Japan` and `RomTier::Translation`. Even with a Japan preference, the tier sort places it below `Original` tier ROMs. The region preference only affects ordering within the same tier. This is correct: an English translation of a Japanese ROM is not a clean Japan-region release -- it is a modified ROM that happens to be region-tagged as Japan.

### GoodTools Compact Codes

Tags like `(U)`, `(E)`, `(J)`, `(UE)`, `(JU)` are already expanded by `region_to_priority()` to their full `RegionPriority` variants. No additional handling needed.

### Preference Value Validation

If `replay.cfg` contains an invalid value (e.g., `region_preference = "brazil"`), the config accessor falls through to `RegionPreference::Usa` via the default match arm. This is safe and predictable.

### Cache Coherence

When the preference changes, `cache.invalidate()` clears all cached ROM lists. The next `get_roms_page()` call triggers a fresh `list_roms()` with the new preference. This is a simple approach that works because:
- The preference changes very rarely (once during setup).
- The cache TTL is only 30 seconds anyway.
- A full re-scan of a system with ~10K ROMs takes <100ms on local storage.

For NFS storage, the re-scan might take a few seconds. This is acceptable for a one-time settings change.

### Multiple Concurrent Users

If two users access the app from different browsers with different region preferences, they will see the same ordering because the preference is stored server-side in `replay.cfg`. This is a known limitation: RePlayOS is a single-user system (one person's retro gaming console), so a shared server-side setting is appropriate. Per-user preferences via cookies would add complexity without benefiting the target use case.

---

## Implementation Plan

### Phase 1: Core Region Preference Support

| Task | File | Effort |
|------|------|--------|
| Define `RegionPreference` enum | `replay-control-core/src/rom_tags.rs` | 10 min |
| Add `RegionPriority::sort_key()` method | `replay-control-core/src/rom_tags.rs` | 15 min |
| Add `ReplayConfig::region_preference()` accessor | `replay-control-core/src/config.rs` | 5 min |
| Add `AppState::region_preference()` method | `replay-control-app/src/api/mod.rs` | 5 min |
| Update `list_roms()` to accept `RegionPreference` | `replay-control-core/src/roms.rs` | 10 min |
| Update `RomCache::get_roms()` to pass preference | `replay-control-app/src/api/mod.rs` | 10 min |
| Update `search_score()` to accept `RegionPreference` | `replay-control-app/src/server_fns.rs` | 10 min |
| Update `get_roms_page()` to read preference | `replay-control-app/src/server_fns.rs` | 5 min |
| Unit tests for `sort_key()` with all preferences | `replay-control-core/src/rom_tags.rs` | 15 min |

**Subtotal**: ~1.5 hours

### Phase 2: Settings UI

| Task | File | Effort |
|------|------|--------|
| Add `get_region_preference` server function | `replay-control-app/src/server_fns.rs` | 10 min |
| Add `set_region_preference` server function | `replay-control-app/src/server_fns.rs` | 15 min |
| Register server functions in `main.rs` | `replay-control-app/src/main.rs` | 5 min |
| Add i18n keys | `replay-control-app/src/i18n.rs` | 5 min |
| Create `RegionPage` component | `replay-control-app/src/pages/region.rs` | 30 min |
| Add route to app router | `replay-control-app/src/app.rs` | 5 min |
| Add menu item to More page | `replay-control-app/src/pages/more.rs` | 5 min |
| CSS for radio button group | `replay-control-app/style/` | 15 min |

**Subtotal**: ~1.5 hours

### Phase 3: Testing and Polish

| Task | Effort |
|------|--------|
| Manual testing: browse SNES with each preference, verify sort order | 20 min |
| Manual testing: search with each preference, verify ranking | 20 min |
| Manual testing: change preference, verify cache invalidation | 10 min |
| Manual testing: NFS storage with preference change | 10 min |
| Edge case testing: multi-region ROMs, World ROMs, no-region ROMs | 15 min |

**Subtotal**: ~1.25 hours

### Total Estimated Effort: ~4.25 hours

---

## Files to Modify

| File | Change |
|------|--------|
| `replay-control-core/src/rom_tags.rs` | Add `RegionPreference` enum, `RegionPriority::sort_key()` method |
| `replay-control-core/src/config.rs` | Add `AppConfig` for `.replay-control/config.cfg`, `region_preference()` accessor |
| `replay-control-core/src/roms.rs` | Add `RegionPreference` parameter to `list_roms()` |
| `replay-control-app/src/api/mod.rs` | Add `region_preference()` to `AppState`, update `RomCache::get_roms()` |
| `replay-control-app/src/server_fns.rs` | Update `search_score()`, add `get_region_preference`/`set_region_preference` |
| `replay-control-app/src/main.rs` | Register new server functions |
| `replay-control-app/src/i18n.rs` | Add region-related i18n keys |
| `replay-control-app/src/pages/region.rs` | New file: `RegionPage` component |
| `replay-control-app/src/pages/mod.rs` | Export `region` module |
| `replay-control-app/src/pages/more.rs` | Add Region menu item |
| `replay-control-app/src/app.rs` | Add `/more/region` route |

---

## Future Considerations

1. **i18n interaction**: When the app adds Japanese or Spanish locale support, the region preference could be linked to the locale selection. Changing locale to Japanese could prompt the user to also set their region preference to Japan. This is a UI nicety, not a technical requirement -- the two settings should remain independent.

2. **Per-system overrides**: Some users might want USA for SNES but Japan for PC Engine. Per-system region overrides would be a power-user feature that adds significant complexity (storage, UI, cache keying). Not recommended for the initial implementation.

3. **Region filtering**: The search improvement analysis (Phase 5) proposes filter toggles to hide specific regions. Region preference and region filtering are complementary: the preference controls sort order while filters control visibility. They should remain separate features.

4. **RePlayOS locale upstream**: If RePlayOS adds a `system_language` or `system_region` key in `replay.cfg` in the future, the app could read it as a fallback default when `region_preference` is not explicitly set in `.replay-control/config.cfg`. The app reads `replay.cfg` for OS-level settings but never writes to it — only `.replay-control/config.cfg` is used for app-specific settings.
