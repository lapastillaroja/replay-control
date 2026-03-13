# Genre Fallback Analysis: LaunchBox Genre Not Reaching rom_cache

## Current State

### Genre Data Sources

There are three places genre data is sourced, each serving a different layer:

1. **Baked-in game_db / arcade_db** (compile-time, embedded in binary)
   - `game_db::lookup_game()` for consoles, `arcade_db::lookup_arcade_game()` for arcade
   - This is the **only** source used when building `rom_cache` entries

2. **LaunchBox fallback at display time** (`server_fns/mod.rs:278-310`, `enrich_from_metadata_cache()`)
   - Called by `resolve_game_info()` when building a `GameInfo` for the game detail page
   - Fills `info.genre` from `game_metadata.genre` when the baked-in genre is empty (line 308)
   - This data is **never persisted back** to `rom_cache`

3. **LaunchBox fallback in search** (`server_fns/search.rs:282-325`, `lookup_genre()`)
   - Called by `global_search()` for genre filtering and display, `get_related_games()` for "More Like This", and `get_all_genres()` / `get_system_genres()` for genre dropdown lists
   - Falls back to `game_metadata.genre` when the baked-in genre is empty (lines 313-322)
   - This data is **never persisted back** to `rom_cache`

### How Genre Flows into rom_cache

During cache build (`cache.rs:417-528`, `save_roms_to_db()`):

```
ROM scan -> for each ROM:
    if arcade:
        arcade_db::lookup_arcade_game(stem) -> info.normalized_genre
    else:
        game_db::lookup_game(system, stem) -> game.normalized_genre
    -> CachedRom { genre, ... }
-> save_system_roms() writes to rom_cache table
```

The genre value comes **exclusively** from the baked-in databases. There is no LaunchBox lookup during this step.

### How Genre is Used After Cache Build

During enrichment (`cache.rs:1026-1112`, `enrich_system_cache()`):

```
enrich_system_cache():
    1. Resolve box_art_url from thumbnails
    2. Load ratings from game_metadata (LaunchBox)
    3. Auto-match new ROMs to existing metadata by normalized title
    4. Build enrichment tuples: (filename, box_art_url, rating)  <-- NO genre
    5. Call update_box_art_and_rating() -> UPDATE rom_cache SET box_art_url, rating
```

The enrichment step updates **only** `box_art_url` and `rating`. Genre is not touched. The `update_box_art_and_rating()` SQL (metadata_db.rs:978-1022) confirms this -- it only has UPDATE statements for `box_art_url` and `rating`.

## The Gap

### rom_cache.genre stays empty for games with no baked-in genre

When a game has no entry in `game_db` or `arcade_db` (or the entry has an empty genre), the `rom_cache.genre` column remains `NULL` or empty string. Even if LaunchBox has genre data for that exact game, it is never written to `rom_cache`.

### Impact on features that query rom_cache

These SQL-based features read `rom_cache.genre` directly and **never** see LaunchBox genre data:

| Feature | Query | File:Line |
|---------|-------|-----------|
| "More Like This" (similar games) | `similar_by_genre()` -- `WHERE genre = ?2 AND genre != ''` | metadata_db.rs:1571 |
| Genre-filtered system browsing | `system_roms_excluding()` -- `WHERE genre = ?2` | metadata_db.rs:1374 |
| Genre distribution stats | `genre_counts()` -- `WHERE genre IS NOT NULL AND genre != ''` | metadata_db.rs:1344 |
| Top-rated games | `top_rated_cached_roms()` -- reads `genre` column | metadata_db.rs:1299 |
| Random diverse picks | `random_cached_roms_diverse()` -- reads `genre` column | metadata_db.rs:1237 |

### The asymmetry

The `get_related_games()` function in `related.rs` demonstrates the asymmetry clearly:

1. Line 61: It calls `lookup_genre()` which **does** fall back to LaunchBox -- so it finds genre "Platform" for a game like "Sonic & Knuckles"
2. Line 87: It passes that genre to `similar_by_genre()` which queries `rom_cache.genre` -- so it finds similar games whose `rom_cache.genre` is "Platform"
3. But **other** games that also have genre "Platform" only in LaunchBox (not in baked-in DB) will **not** appear in the results because their `rom_cache.genre` is empty

Similarly, `global_search()` uses `lookup_genre()` for the genre filter (with LaunchBox fallback), but the candidate ROMs whose genre is only in LaunchBox won't match the filter because `lookup_genre()` is called per-ROM (expensive but correct). However, the genre list dropdowns (`get_all_genres()`, `get_system_genres()`) iterate all ROMs calling `lookup_genre()` per-ROM so they **do** include LaunchBox genres. This means the dropdown shows genres that may return fewer results than expected.

## Proposed Fix: Genre Fallback During Cache Enrichment

### Approach: Fill empty rom_cache.genre from LaunchBox during enrichment

During `enrich_system_cache()`, after setting `box_art_url` and `rating`, also check if `rom_cache.genre` is empty and fill it from `game_metadata.genre` (LaunchBox) if available.

**Priority**: baked-in game_db/arcade_db genre (set during cache build) > LaunchBox genre (filled during enrichment if empty)

This is the safe approach because:
- Baked-in genre is already in rom_cache from cache build -- enrichment only fills gaps
- LaunchBox coverage may be lower than baked-in (~65% vs ~80%) but fills the remaining ~15-20% that baked-in misses entirely
- No risk of overwriting good data with worse data

### Why not prefer LaunchBox over baked-in?

LaunchBox genre data is human-curated and may be more accurate for edge cases (e.g., "Sonic & Knuckles" might have a better genre classification). However:
- Baked-in coverage is higher, so switching to "prefer LaunchBox" would lose genres for the 15-20% of games that baked-in has but LaunchBox doesn't
- Genre normalization may differ between sources (baked-in uses `normalized_genre`, LaunchBox uses its own genre strings)
- The safe approach (fill gaps only) gets 90%+ of the benefit with no risk

### How to trigger re-enrichment

After deploying the code change:
- **Automatic**: The next LaunchBox import (or re-import via Settings) calls `spawn_cache_enrichment()` which calls `enrich_system_cache()` for all systems
- **Manual**: Delete `metadata.db` to force a full re-populate on next startup (via `spawn_cache_verification()` which calls `populate_all_systems()` which calls `enrich_system_cache()`)
- **Existing flow**: `spawn_cache_enrichment()` at import.rs:314 already runs after import -- no new trigger needed

### Does spawn_cache_enrichment() already handle this?

**No.** Tracing the chain:

1. `run_import_blocking()` (import.rs:201) runs the LaunchBox import
2. On success, calls `self.spawn_cache_enrichment()` (import.rs:314)
3. `spawn_cache_enrichment()` (background.rs:150-181) checks if rom_cache is empty:
   - If empty: calls `populate_all_systems()` which scans ROMs (using baked-in genre) then calls `enrich_system_cache()` -- **genre not updated from LaunchBox**
   - If not empty: calls `enrich_system_cache()` for each system -- **genre not updated from LaunchBox**
4. `enrich_system_cache()` (cache.rs:1026-1112) only updates `box_art_url` and `rating` -- **genre not touched**

## Implementation Details

### File to modify

`<WORKSPACE>/replay-control-app/src/api/cache.rs`, function `enrich_system_cache()` (line 1026).

### Code change

The change adds genre loading from LaunchBox `game_metadata` and includes it in the enrichment tuples. Instead of only updating `box_art_url` and `rating`, we also update `genre` when the rom_cache entry has an empty genre and LaunchBox has one.

In `enrich_system_cache()`, starting at line 1030, add genre loading alongside ratings:

```rust
    pub fn enrich_system_cache(&self, state: &crate::api::AppState, system: &str) {
        let storage = state.storage();
        let index = self.get_image_index(state, system);

        // Load ratings from game_metadata table (from LaunchBox import).
        let ratings: HashMap<String, f64> = state
            .metadata_db()
            .and_then(|guard| guard.as_ref()?.system_ratings(system).ok())
            .unwrap_or_default();

        // Load genres from game_metadata table (from LaunchBox import).
        // Used to fill empty rom_cache.genre entries.
        let lb_genres: HashMap<String, String> = state
            .metadata_db()
            .and_then(|guard| {
                guard.as_ref()?.system_metadata_all(system).ok().map(|all| {
                    all.into_iter()
                        .filter_map(|(filename, meta)| {
                            meta.genre
                                .filter(|g| !g.is_empty())
                                .map(|g| (filename, g))
                        })
                        .collect()
                })
            })
            .unwrap_or_default();

        // Auto-match new ROMs: build a normalized-title index from existing
        // game_metadata entries so ROMs added after the last import can inherit
        // metadata from entries that share the same normalized title.
        let auto_matched_ratings = self.auto_match_metadata(state, system);

        // Merge auto-matched ratings into the main ratings map.
        let mut all_ratings = ratings;
        for (filename, rating) in &auto_matched_ratings {
            all_ratings.entry(filename.clone()).or_insert(*rating);
        }

        // Read current ROMs from L1 cache to get filenames and current genres.
        let rom_data: Vec<(String, Option<String>)> = if let Ok(guard) = self.roms.read() {
            guard
                .get(system)
                .map(|entry| {
                    entry
                        .data
                        .iter()
                        .map(|r| (r.game.rom_filename.clone(), r.genre.clone()))
                        .collect()
                })
                .unwrap_or_default()
        } else {
            return;
        };

        if rom_data.is_empty() {
            return;
        }

        // Build enrichment tuples: (filename, box_art_url, genre, rating)
        let enrichments: Vec<(String, Option<String>, Option<String>, Option<f32>)> = rom_data
            .iter()
            .filter_map(|(filename, current_genre)| {
                let art = self.resolve_box_art(state, &index, system, filename);
                let rating = all_ratings.get(filename).map(|&r| r as f32);
                // Fill genre from LaunchBox only when rom_cache has no genre.
                let genre = if current_genre.as_ref().is_none_or(|g| g.is_empty()) {
                    lb_genres.get(filename).cloned()
                } else {
                    None
                };
                if art.is_none() && rating.is_none() && genre.is_none() {
                    return None;
                }
                Some((filename.clone(), art, genre, rating))
            })
            .collect();

        if enrichments.is_empty() {
            return;
        }

        let count = enrichments.len();
        // Use targeted SQL update for box_art_url, genre, and rating.
        self.with_db_mut(&storage, |db| {
            if let Err(e) = db.update_enrichment(system, &enrichments) {
                tracing::warn!("Enrichment failed for {system}: {e}");
            }
        });

        // Also update L1 cache entries.
        if let Ok(mut guard) = self.roms.write()
            && let Some(entry) = guard.get_mut(system)
        {
            for rom in &mut entry.data {
                for (filename, art, genre, rating) in &enrichments {
                    if rom.game.rom_filename == *filename {
                        if art.is_some() {
                            rom.box_art_url = art.clone();
                        }
                        if let Some(g) = genre {
                            rom.genre = Some(g.clone());
                        }
                        if let Some(r) = rating {
                            rom.rating = Some(*r);
                        }
                        break;
                    }
                }
            }
        }

        tracing::debug!("L2 enrichment: {system} — {count} ROMs updated with box art/genre/ratings");
    }
```

### New DB method needed

In `replay-control-core/src/metadata/metadata_db.rs`, add a new method `update_enrichment()` that updates `box_art_url`, `genre`, and `rating` (or modify `update_box_art_and_rating()` to also handle genre). The simplest approach is to add a new method:

```rust
    /// Batch update box_art_url, genre, and rating for ROMs in rom_cache.
    /// Only updates non-None fields (preserves existing values).
    pub fn update_enrichment(
        &mut self,
        system: &str,
        enrichments: &[(String, Option<String>, Option<String>, Option<f32>)],
    ) -> Result<()> {
        let tx = self
            .conn
            .transaction()
            .map_err(|e| Error::Other(format!("Transaction start failed: {e}")))?;

        {
            let mut art_stmt = tx
                .prepare(
                    "UPDATE rom_cache SET box_art_url = ?2
                     WHERE system = ?3 AND rom_filename = ?1",
                )
                .map_err(|e| Error::Other(format!("Prepare box_art update: {e}")))?;

            let mut genre_stmt = tx
                .prepare(
                    "UPDATE rom_cache SET genre = ?2
                     WHERE system = ?3 AND rom_filename = ?1
                       AND (genre IS NULL OR genre = '')",
                )
                .map_err(|e| Error::Other(format!("Prepare genre update: {e}")))?;

            let mut rating_stmt = tx
                .prepare(
                    "UPDATE rom_cache SET rating = ?2
                     WHERE system = ?3 AND rom_filename = ?1",
                )
                .map_err(|e| Error::Other(format!("Prepare rating update: {e}")))?;

            for (filename, box_art_url, genre, rating) in enrichments {
                if let Some(url) = box_art_url {
                    art_stmt
                        .execute(params![filename, url, system])
                        .map_err(|e| Error::Other(format!("Update box_art_url: {e}")))?;
                }
                if let Some(g) = genre {
                    genre_stmt
                        .execute(params![filename, g, system])
                        .map_err(|e| Error::Other(format!("Update genre: {e}")))?;
                }
                if let Some(r) = rating {
                    rating_stmt
                        .execute(params![filename, r, system])
                        .map_err(|e| Error::Other(format!("Update rating: {e}")))?;
                }
            }
        }

        tx.commit()
            .map_err(|e| Error::Other(format!("Transaction commit failed: {e}")))?;
        Ok(())
    }
```

Note the `WHERE (genre IS NULL OR genre = '')` guard on the genre UPDATE -- this is a safety net ensuring LaunchBox genre never overwrites an existing baked-in genre, even if the Rust code already filters this. Belt-and-suspenders.

### Also needed: read current genre from L1 cache

The current `enrich_system_cache()` reads only `rom_filename` from L1 cache (lines 1048-1061). The modified version needs to also read the current `genre` to decide whether to fill from LaunchBox. The `RomEntry` struct does not carry genre directly, but the L1 cache stores `RomEntry` values, not `CachedRom`. However, `RomEntry` has a `genre` field since it's populated from the `CachedRom` during L2->L1 promotion.

Actually, looking more carefully at the L1 cache: `roms` is `HashMap<String, CacheEntry<Vec<RomEntry>>>` (cache.rs:86). `RomEntry` is defined in `replay-control-core/src/roms.rs` and has a `game: GameCore` with fields like `rom_filename`, `display_name`, etc. It does **not** have a `genre` field directly. Genre would need to be read from the L2 database or from the CachedRom before it's converted.

Alternative approach: instead of reading genre from L1, load the current rom_cache genres from L2 (SQLite) at the start of enrichment:

```rust
// Load current rom_cache genres to know which are empty.
let current_genres: HashMap<String, Option<String>> = self
    .with_db_read(&storage, |db| {
        db.load_system_roms(system)
            .map(|roms| {
                roms.into_iter()
                    .map(|r| (r.rom_filename, r.genre))
                    .collect()
            })
            .unwrap_or_default()
    })
    .unwrap_or_default();
```

This is cleaner and avoids any dependency on L1 cache structure. The full modified code should use this approach.

### Summary of files to change

| File | Change |
|------|--------|
| `replay-control-app/src/api/cache.rs` | Modify `enrich_system_cache()` to load LaunchBox genres and include them in enrichment |
| `replay-control-core/src/metadata/metadata_db.rs` | Add `update_enrichment()` method (or extend `update_box_art_and_rating()`) |

### No other files need changes

- `save_roms_to_db()` stays the same (still uses baked-in genre at cache build time)
- `enrich_from_metadata_cache()` stays the same (still fills genre at display time as a safety net)
- `lookup_genre()` stays the same (still provides LaunchBox fallback for search/related)
- `spawn_cache_enrichment()` stays the same (already calls `enrich_system_cache()`)
- The existing post-import enrichment flow already triggers `enrich_system_cache()`, so the genre fill will happen automatically after the next import
