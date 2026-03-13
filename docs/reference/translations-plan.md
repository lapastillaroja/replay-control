# Translations Handling in Regional Variants

## Problem

The "Regional Variants" section on the game detail page groups ROMs by `base_title` in the `game_library` table. Since `base_title` is computed by stripping parenthesized/bracketed tags and lowercasing (via `thumbnails::base_title()`), **translations end up with the same `base_title` as the original**:

```
Super Mario World (USA).smc                    -> base_title: "super mario world"
Super Mario World (USA) (Traducido Es).smc     -> base_title: "super mario world"
Super Mario World (USA) (Translated Ger).sfc   -> base_title: "super mario world"
Super Mario World (Europe) (Rev 1).sfc         -> base_title: "super mario world"
```

This means the regional variants chips for Super Mario World currently show all translations alongside the genuine regional variants (USA, Europe, Japan), which is misleading. A Spanish fan translation of the USA ROM is not a "regional variant" -- it's a modified copy of a specific region's ROM.

## Current State

### Translation tags recognized by `rom_tags.rs`

**Parenthesized (No-Intro / custom sets):**
- `(Traducido Es)`, `(Traduccion ...)` -- Spanish
- `(Traduzido Por)`, `(Traduzido)` -- Brazilian Portuguese
- `(Translated En)`, `(Translated Fre)`, `(Translated Ger)`, etc.
- `(PT-BR)` -- standalone

**Bracketed (GoodTools / older sets):**
- `[T-Spa1.0v_Wave]`, `[T+Fre]`, `[T+Bra_TMT]`, `[T+Rus Pirate]`, `[T-Eng v1.2 Zoinkity]`, etc.
- Pattern: `[T+lang...]` or `[T-lang...]`

### Translation ROMs on disk (NFS mount)

~2,740 translation ROMs across 4 systems:
- `nintendo_snes`: 1,305 (in `02 Translations/`, `02 Project FastROM/`, `02 BS-X Patched/`)
- `sega_smd`: 884 (in `02 Translations/`)
- `sega_sms`: 402 (in `02 Translations/`)
- `sega_gg`: 149 (in `02 Translations/`)

These are in numbered subfolders (not `_`-prefixed), so they ARE scanned into `game_library`.

### No-Intro DATs do NOT contain translations

The No-Intro DAT files in `data/no-intro/` contain only official releases. All translation ROMs come from separate ROM sets (GoodTools, fan translation compilations, etc.).

### How `classify()` handles translations

`rom_tags::classify()` already assigns `RomTier::Translation` (value 3) to any ROM with a translation tag. This tier is higher than `Original` (0), `Revision` (1), and `RegionVariant` (2), meaning translations sort AFTER originals in game lists. The `region` field for a translation reflects the base ROM's region (e.g., a `(USA) (Traducido Es)` ROM gets region "usa").

### The dedup problem in home page queries

The `PARTITION BY system, base_title` dedup used in `random_cached_roms_diverse()`, `top_rated_cached_roms()`, etc. picks one ROM per base_title using region preference ordering. Translations have the same `base_title` as originals, so they compete in the dedup window. Since translations have a real region (from the base ROM), a translation could theoretically win over an original if the region matches the user preference. In practice this is unlikely since `ORDER BY CASE WHEN region = ?2 THEN 0 ...` would rank both equally, and SQLite's ROW_NUMBER is non-deterministic for ties. But it's still conceptually wrong.

## Proposal

### 1. Exclude translations from Regional Variants

Translations should NOT appear in the regional variants chips. They are fan-modified copies of a specific region's ROM, not official regional releases.

**How:** Add an `is_translation INTEGER NOT NULL DEFAULT 0` column to `game_library`. Filter it out in the `regional_variants()` query.

### 2. Add a "Translations" section on the game detail page

Show a separate section (similar to Regional Variants) listing available translations of the same game. This gives translations proper visibility without polluting the regional variants.

### 3. Exclude translations from "Change Cover" picker

The boxart picker uses `find_boxart_variants()` in `thumbnail_manifest.rs`, which matches thumbnail index entries by `strip_tags(filename)` base title. Since libretro thumbnail repos only contain official game names, translation ROMs don't have their own thumbnail entries. However, when viewing a translation ROM's detail page, the picker still matches the original game's thumbnails (same base title after stripping tags).

**Approach:** Hide the "Change Cover" link entirely on translation ROM detail pages. A translation is a patched copy of a specific region's ROM — it should inherit the base ROM's cover art, not offer its own picker. In `GameDetailContent`, check if the current ROM is a translation (via `classify()` tier or a new `is_translation` field on `RomDetail`) and suppress `has_variants` when true.

### 4. Exclude translations from home page dedup

The `PARTITION BY system, base_title` dedup windows should also exclude translations (via `WHERE is_translation = 0`) so translations never compete with originals for the "representative ROM" slot.

## Data Model Changes

### New column on `game_library`

```sql
ALTER TABLE game_library ADD COLUMN is_translation INTEGER NOT NULL DEFAULT 0;
```

Alternative considered: a `translation_lang TEXT` column storing the normalized language code ("ES", "PT-BR", "EN", etc.) -- this would enable richer display ("ES Translation", "EN Translation") in the chips. However, the display name already contains this info via `extract_tags()` (e.g., `"Super Mario World (USA, ES Translation)"`), so a boolean is sufficient for filtering. The translation language can be extracted at display time from the existing `display_name` or `rom_filename`.

**Decision: use `is_translation INTEGER NOT NULL DEFAULT 0`.** Simpler, and the language info is already in the display name.

### Population

In `cache.rs` where `GameEntry` is built, the `classify()` function already returns `RomTier::Translation` for translations. Use this:

```rust
let (tier, region_priority) = replay_control_core::rom_tags::classify(rom_filename);
let is_translation = tier == RomTier::Translation;
```

### GameEntry struct

Add `is_translation: bool` to `GameEntry` in `metadata_db.rs`. Add it to the INSERT/SELECT column lists (there are ~10 queries to update).

## SQL Query Changes

### Regional Variants (filter out translations)

```sql
SELECT rom_filename, region FROM game_library
WHERE system = ?1
  AND base_title != ''
  AND is_translation = 0
  AND base_title = (
      SELECT base_title FROM game_library
      WHERE system = ?1 AND rom_filename = ?2
  )
ORDER BY
  CASE region
      WHEN 'USA' THEN 1
      WHEN 'Europe' THEN 2
      WHEN 'Japan' THEN 3
      ELSE 4
  END,
  region;
```

### New: Translations of the same game

```sql
SELECT rom_filename, display_name FROM game_library
WHERE system = ?1
  AND base_title != ''
  AND is_translation = 1
  AND base_title = (
      SELECT base_title FROM game_library
      WHERE system = ?1 AND rom_filename = ?2
  )
ORDER BY display_name;
```

### Home page dedup queries (all `PARTITION BY` queries)

Add `AND is_translation = 0` to the inner `FROM game_library WHERE ...` clause in each dedup CTE. This affects:
- `random_cached_roms_diverse()`
- `top_rated_cached_roms()`
- `system_roms_excluding()` (both branches)

## Server Function Changes

### Extend `RelatedGamesData`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedGamesData {
    pub regional_variants: Vec<RegionalVariant>,
    pub translations: Vec<TranslationVariant>,  // NEW
    pub similar_games: Vec<RecommendedGame>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslationVariant {
    pub rom_filename: String,
    /// Short label extracted from display_name, e.g., "ES Translation"
    pub label: String,
    pub href: String,
    pub is_current: bool,
}
```

### New DB method: `translations()`

```rust
pub fn translations(
    &self,
    system: &str,
    rom_filename: &str,
) -> Result<Vec<(String, Option<String>)>>
```

Returns `(rom_filename, display_name)` pairs for translations sharing the same `base_title`.

### Extend `get_related_games` server function

Add a `translations` query alongside the existing `regional_variants` and `similar_by_genre` queries inside the single `with_db_read()` call. Build `TranslationVariant` from the results, extracting the translation language label from the display name.

## Component Design

### `TranslationChips` component

Reuse the same pattern as `RegionalVariantsChips`:

```rust
#[component]
fn TranslationChips(translations: Vec<TranslationVariant>) -> impl IntoView {
    let i18n = use_i18n();
    view! {
        <section class="section game-section">
            <h2 class="game-section-title">{move || t(i18n.locale.get(), "game_detail.translations")}</h2>
            <div class="regional-variants">
                {translations.into_iter().map(|v| {
                    let class = if v.is_current { "region-chip active" } else { "region-chip" };
                    view! {
                        <A href=v.href attr:class=class>{v.label}</A>
                    }
                }).collect::<Vec<_>>()}
            </div>
        </section>
    }
}
```

Reuses the `.regional-variants` and `.region-chip` CSS classes -- no new styles needed.

### Placement

In `RelatedGamesSection`, render between `RegionalVariantsChips` and `SimilarGamesRow`:

```
RegionalVariantsChips  (USA, Europe, Japan -- official releases only)
TranslationChips       (ES Translation, PT-BR Translation, EN Translation, ...)
SimilarGamesRow        (More Like This)
```

### Label extraction

For the chip label, extract the translation language from the display name. The display name already contains it, e.g., `"Super Mario World (USA, ES Translation)"`. Parse the `"XX Translation"` suffix, or fall back to the full display name tags.

Alternatively, use `rom_tags::extract_tags()` on the `rom_filename` and find the `"XX Translation"` part from the tag string. This is more reliable since `extract_tags` is the canonical source.

## i18n Keys

Add to all `.ftl` files:
- `game_detail-translations = Translations`

## Edge Cases

| Case | Handling |
|------|----------|
| **Game has no translations** | `translations` vec is empty; section hidden |
| **Current game IS a translation** | It appears in the translations list with `is_current = true` (active chip). Regional variants show the originals only. |
| **Translation of a Japan-only game** (e.g., `Bahamut Lagoon (Japan) (Translated En).sfc`) | Regional variants may show only "Japan" (the original). Translations show "EN Translation". The translation's base ROM region is Japan. |
| **Multiple translations to the same language** (e.g., two Spanish translations by different hackers) | Both appear as chips. The label may be the same ("ES Translation"); the filenames differ. Could add the hacker credit to distinguish, but the filename tooltip on hover is probably sufficient for now. |
| **Translation that is also a hack** (e.g., `(Hack) (Translated En)`) | `classify()` returns `RomTier::Hack`, not `Translation`, because hack check comes first. This ROM would NOT get `is_translation = true`. This is correct -- it's a hack first. We could refine this later if needed, but hacks are already excluded from dedup. |
| **FastROM + translation** (e.g., `Actraiser (Japan) (FastRom) (Translated En).sfc`) | `classify()` returns `Translation` because FastROM is not a tier modifier (it's a patch tag). `is_translation` = true. The chip label would be "EN Translation, FastROM" from `extract_tags`. |
| **game_library migration** | Use `ALTER TABLE ADD COLUMN ... DEFAULT 0` so existing rows get `is_translation = 0`. On next cache refresh, the column is populated correctly. No forced cache rebuild needed. |

## Files to Modify

| File | Change |
|------|--------|
| `replay-control-core/src/metadata/metadata_db.rs` | Add `is_translation` to `GameEntry`, schema, INSERT/SELECT, all dedup CTEs. Add `translations()` method. Modify `regional_variants()` to filter `is_translation = 0`. |
| `replay-control-app/src/api/cache.rs` | Populate `is_translation` from `classify()` tier during cache build. |
| `replay-control-app/src/server_fns/related.rs` | Add `TranslationVariant` struct, extend `RelatedGamesData`, call `translations()` in `get_related_games`. |
| `replay-control-app/src/server_fns/mod.rs` | Re-export `TranslationVariant`. |
| `replay-control-app/src/pages/game_detail.rs` | Add `TranslationChips` component, render it in `RelatedGamesSection`. |
| `replay-control-app/src/server_fns/roms.rs` (or wherever `RomDetail` is built) | Add `is_translation: bool` to `RomDetail` struct, populate from `classify()`. |
| `replay-control-app/src/pages/game_detail.rs` | Suppress "Change Cover" link when `is_translation` is true. |
| `replay-control-app/src/i18n.rs` | Add `game_detail.translations` key. |

## Implementation Order

### Phase 1: Translations (current)

1. Add `is_translation` column to schema + `GameEntry` struct
2. Populate `is_translation` in cache build
3. Filter translations from `regional_variants()` query
4. Filter translations from dedup CTEs (home page queries)
5. Add `translations()` DB method
6. Extend server function + response struct
7. Add `TranslationChips` component + i18n key
8. Hide "Change Cover" on translation ROM detail pages
9. Test with NFS mount (where translation ROMs exist)

### Phase 2: Hacks (future improvement)

Same pattern as translations, but for ROM hacks (e.g., `Super Mario World (USA) (Hack).smc`, `Sonic the Hedgehog (USA) [h1C]`).

**Why:** Hacks share the same `base_title` as originals and currently pollute regional variants, home page dedup, and the cover picker — the same problems translations have. Hacks are even more numerous than translations on typical romsets.

**What to add:**

1. **`is_hack INTEGER NOT NULL DEFAULT 0`** column on `game_library`. Populated from `rom_tags::classify()` returning `RomTier::Hack`.
2. **Filter hacks from regional variants**: `AND is_hack = 0` in the `regional_variants()` query.
3. **Filter hacks from home page dedup**: `AND is_hack = 0` in all `PARTITION BY` CTEs, alongside `is_translation = 0`.
4. **Hide "Change Cover" on hack ROM detail pages**: same approach as translations — suppress `has_variants` when `is_hack` is true.
5. **"Hacks" section on game detail page**: a new chip row (reusing the generic chip component) showing available hacks of the same game. Placed after Translations, before "More Like This":
   ```
   Regional Variants  (USA, Europe, Japan)
   Translations       (ES Translation, EN Translation, ...)
   Hacks              (Hack, Hack v1.2 by Author, ...)
   More Like This     (genre-based scroll cards)
   ```
6. **`hacks()` DB method**: query `game_library WHERE is_hack = 1 AND base_title = (subquery)`, similar to `translations()`.
7. **i18n key**: `game_detail.hacks` => "Hacks"

**Edge cases:**
- **Translation + hack overlap**: `classify()` checks hack first, so `(Hack) (Translated En)` gets `RomTier::Hack`, not `Translation`. These appear in the Hacks section only, not Translations. This is correct — the ROM is primarily a hack.
- **Hack labels**: Use `rom_tags::extract_tags()` on the filename to generate chip labels (e.g., "Hack v1.2 by Author"). If no meaningful label beyond "Hack", show the filename.
- **Arcade clones vs hacks**: Arcade `is_clone` (from `arcade_db`) is a different concept from `is_hack` (from filename tags). An arcade clone is an official regional/revision variant in MAME's parent/clone tree. A hack is a fan-modified ROM. Both can coexist: `is_clone = 1, is_hack = 0` for official clones, `is_clone = 0, is_hack = 1` for fan hacks.
