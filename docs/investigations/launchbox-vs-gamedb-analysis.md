# LaunchBox vs Baked-in game_db: Metadata Comparison Analysis

**Date:** 2026-03-13
**Status:** Investigation complete

## 1. Data Source Overview

### Baked-in game_db (current approach)

The game_db is generated at compile time by `replay-control-core/build.rs` from three data sources:

| Source | License | What it provides | How it matches |
|--------|---------|------------------|----------------|
| **No-Intro DATs** | Community, freely distributed | ROM filenames, CRC32 hashes, regions | Filename stem -> game identity |
| **libretro-meta** | MIT | Genre and max-players per ROM (by CRC32) | CRC32 hash lookup |
| **TheGamesDB JSON** | CC BY-NC-SA | Genre, players, year, developer (by title+platform) | Normalized title matching (fallback) |

Build pipeline:
1. Parse No-Intro DATs to get all known ROMs per system
2. Group ROMs into canonical games by normalized title
3. Look up genre/players from libretro-meta DATs (by CRC32) -- **primary**
4. Fall back to TheGamesDB JSON (by title matching) -- **secondary**
5. Emit PHF maps keyed by filename stem

The game_db covers 9 console systems: NES, SNES, GB, GBC, GBA, N64, SMS, SMD (Genesis), GG.

### LaunchBox metadata (current approach)

LaunchBox metadata is imported at runtime from a ~460 MB XML file that the user downloads on-demand. The app parses it with a streaming XML reader and stores matched entries in a SQLite database (`metadata.db`).

| Field | Coverage (across our 9 systems) |
|-------|------|
| Genre | 94-99% of LaunchBox entries |
| MaxPlayers | 66-99% of entries |
| Overview (description) | 91-98% of entries |
| Publisher | Most entries |
| CommunityRating | Many entries |

LaunchBox covers 189 platforms with ~178K total game entries. For our 9 console systems it has ~15K entries.

## 2. Coverage Comparison

### Raw entry counts (database size, not matched to ROMs)

| System | game_db entries | game_db with genre | LaunchBox entries | LB with genre |
|--------|---:|---:|---:|---:|
| nintendo_nes | 4,002 | 2,254 (56%) | 3,503 | 3,333 (95%) |
| nintendo_snes | 2,373 | 2,247 (94%) | 2,667 | 2,625 (98%) |
| nintendo_gb | 1,619 | 1,358 (83%) | 1,543 | 1,455 (94%) |
| nintendo_gbc | 1,774 | 1,281 (72%) | 1,441 | 1,369 (95%) |
| nintendo_gba | 2,270 | 2,007 (88%) | 2,281 | 2,162 (94%) |
| nintendo_n64 | 585 | 493 (84%) | 715 | 703 (98%) |
| sega_sms | 752 | 461 (61%) | 512 | 508 (99%) |
| sega_smd | 1,749 | 1,370 (78%) | 1,936 | 1,916 (98%) |
| sega_gg | 495 | 420 (84%) | 401 | 400 (99%) |

**Key finding:** LaunchBox has dramatically higher genre coverage percentages (94-99%) compared to game_db (56-94%). This is because game_db includes many obscure/unlicensed/homebrew ROMs from No-Intro DATs that have no genre data in libretro-meta. LaunchBox's database is more curated and focused on released commercial games, so a higher percentage of its entries have metadata.

### Coverage against actual ROMs on disk (NFS mount)

Only 5 systems had ROMs on the NFS mount at the time of testing:

| System | ROMs on disk | game_db genre coverage | LaunchBox genre coverage |
|--------|---:|---:|---:|
| nintendo_n64 | 380 | 354 (93%) | 273 (71%) |
| nintendo_snes | 2,442 | 2,077 (85%) | 1,614 (66%) |
| sega_gg | 463 | 370 (79%) | 312 (67%) |
| sega_smd | 1,723 | 1,264 (73%) | 1,113 (64%) |
| sega_sms | 567 | 410 (72%) | 340 (59%) |
| **Total** | **5,575** | **4,475 (80%)** | **3,652 (65%)** |

**Key finding:** game_db has substantially better ROM matching coverage (80% vs 65%). This is because:
1. game_db uses No-Intro filename stems for exact matching -- these directly correspond to ROM filenames on disk
2. LaunchBox uses human-readable game titles that must be fuzzy-matched via title normalization
3. Many ROMs on disk are regional variants, translations, or aftermarket releases that LaunchBox doesn't catalog

### Player count coverage against ROMs on disk

| System | ROMs on disk | game_db players coverage | LaunchBox players coverage |
|--------|---:|---:|---:|
| nintendo_n64 | 380 | 354 (93%) | 273 (71%) |
| nintendo_snes | 2,442 | 2,018 (82%) | 1,584 (64%) |
| sega_gg | 463 | 370 (79%) | 312 (67%) |
| sega_smd | 1,723 | 1,258 (73%) | 1,087 (63%) |
| sega_sms | 567 | 390 (68%) | 338 (59%) |
| **Total** | **5,575** | **4,390 (78%)** | **3,594 (64%)** |

## 3. Genre Quality Comparison

### Agreement rate

For ROMs where both sources provide a genre:

| System | Both have genre | Agree | Disagree | Agreement % |
|--------|---:|---:|---:|---:|
| nintendo_n64 | 273 | 189 | 84 | 69% |
| nintendo_snes | 1,571 | 1,039 | 532 | 66% |
| sega_gg | 310 | 189 | 121 | 60% |
| sega_smd | 971 | 551 | 420 | 56% |
| sega_sms | 331 | 190 | 141 | 57% |
| **Total** | **3,456** | **2,158** | **1,298** | **62%** |

A 62% agreement rate seems low, but much of the disagreement is structural, not quality-based.

### Disagreement analysis

Of 1,298 genre disagreements:

- **44% involve LaunchBox multi-genre entries.** LaunchBox assigns multiple genres separated by semicolons (e.g., "Action; Platform; Puzzle"). Our comparison takes only the first LaunchBox genre. If we check whether game_db's genre appears *anywhere* in LaunchBox's genre list, 30% of disagreements (392) are resolved, bringing effective agreement to ~74%.

- **The largest disagreement category is Action vs Platform.** This accounts for hundreds of disagreements. Both sources routinely disagree on whether a game with platforming elements is "Action" or "Platform" -- this is a genuine genre classification ambiguity, not a data quality issue.

Common disagreement patterns:
| game_db says | LaunchBox says | Count | Assessment |
|---|---|---|---|
| Action | Platform | ~150 | Ambiguous -- both valid |
| Platform | Action | ~90 | Ambiguous -- both valid |
| Action | Shooter | ~80 | LB is often more specific |
| Action | Compilation | ~30 | LB has "Compilation" genre; game_db doesn't |
| Sports | Fighting (wrestling) | ~15 | Debatable for wrestling/boxing games |
| Other | specific genre | ~98 | LB is better -- game_db's "Other" is a catch-all |

### Notable quality differences

**Where LaunchBox is better:**
- LaunchBox has specific genres where game_db uses vague catch-alls (e.g., "Other" -> "Sports" for fishing games)
- LaunchBox correctly identifies compilations as "Compilation" (game_db says "Action")
- LaunchBox has a "Life Simulation" genre (e.g., Animal Crossing) where game_db says "Role-Playing"
- LaunchBox's multi-genre approach is more nuanced (e.g., "Action; Platform; Puzzle" vs just "Platform")

**Where game_db is better:**
- Higher ROM matching rate (80% vs 65% of ROMs on disk)
- CRC-based matching is more reliable than title normalization for regional variants
- Covers more obscure/homebrew titles that LaunchBox doesn't catalog
- Does not suffer from multi-genre ambiguity -- always assigns exactly one genre

**Where both are questionable:**
- Both sometimes misclassify wrestling/boxing as "Sports" vs "Fighting"
- Both struggle with hybrid genres (action-RPGs, puzzle-platformers)
- Player count data sometimes reflects different things (simultaneous vs alternating, local vs link cable)

## 4. Players Data Comparison

### Agreement rate

| System | Both have players | Exact match | Off by 1 | Off by 2+ |
|--------|---:|---:|---:|---:|
| nintendo_n64 | 273 | 247 (90%) | 9 (3%) | 17 (6%) |
| nintendo_snes | 1,504 | 1,248 (82%) | 163 (10%) | 93 (6%) |
| sega_gg | 310 | 266 (85%) | 31 (10%) | 13 (4%) |
| sega_smd | 959 | 895 (93%) | 42 (4%) | 22 (2%) |
| sega_sms | 314 | 298 (94%) | 13 (4%) | 3 (0%) |
| **Total** | **3,360** | **2,954 (87%)** | **258 (7%)** | **148 (4%)** |

**Key finding:** Player count data agrees 87% exactly, with only 4% significantly different. This is much better agreement than genre data.

### Notable player count disagreements

Most "off by 1" cases reflect ambiguity in counting (e.g., 1-player game with 2-player alternating mode: is it 1 or 2?).

Larger disagreements (diff >= 7) fall into patterns:
- **Strategy games (Nobunaga's Ambition, Romance of the Three Kingdoms):** game_db says 1, LaunchBox says 8. LaunchBox likely counts hotseat multiplayer modes.
- **Game_db reporting 8 for single-player games (Claymates, Monstania):** This appears to be bad data in libretro-meta (erroneous CRC mapping).
- **Sports games:** Different interpretations of "max players" (simultaneous vs with multitap vs total supported).

## 5. Licensing Analysis

| Source | License | Can embed in binary? | Can redistribute? |
|--------|---------|---------------------|-------------------|
| **libretro-meta** | MIT | Yes, freely | Yes, freely |
| **No-Intro DATs** | Community, no formal license | Yes (widely done) | Yes (widely done) |
| **TheGamesDB** | CC BY-NC-SA | Yes, with attribution + non-commercial | Yes, with same terms |
| **LaunchBox** | **No explicit license** | **Legally ambiguous** | **Not without permission** |

LaunchBox's metadata database has no formal open-source or Creative Commons license. The bulk XML dump is publicly accessible, but the Terms of Use are oriented toward use with LaunchBox software. Embedding LaunchBox data in a compiled binary would be legally risky without explicit permission from the LaunchBox team.

The current approach (runtime import from user-downloaded XML into local SQLite) is the safest legal position and should be maintained.

See `tools/reports/licensing_analysis.txt` for the full licensing analysis.

## 6. Gap-Filling Analysis

How well does LaunchBox fill gaps where game_db has no data?

| System | game_db gaps | Filled by LB (genre) | Filled by LB (players) |
|--------|---:|---:|---:|
| nintendo_n64 | 26 | 0 (0%) | 0 (0%) |
| nintendo_snes | 365 | 43 (11%) | 80 (18%) |
| sega_gg | 93 | 2 (2%) | 2 (2%) |
| sega_smd | 459 | 142 (30%) | 128 (27%) |
| sega_sms | 157 | 9 (5%) | 24 (13%) |

LaunchBox's gap-filling value is moderate. It fills 30% of game_db's genre gaps on Mega Drive but only 2-5% on other systems. This makes sense: game_db's gaps are mostly obscure/regional titles that LaunchBox also lacks.

## 7. Recommendation

**Do NOT replace libretro-meta with LaunchBox as the embedded genre/players database.**

Reasons:
1. **Licensing blocks embedding.** LaunchBox has no open-source license; libretro-meta is MIT.
2. **game_db has better ROM matching.** 80% vs 65% coverage against actual ROMs on disk, thanks to No-Intro filename stem matching.
3. **Genre agreement is only 62%.** The sources often disagree, and neither is definitively "right" -- many disagreements are legitimate classification ambiguities.
4. **Player data already agrees 87%.** Not enough improvement to justify the migration effort and legal risk.

**Recommended approach: Keep the current hybrid architecture.**

| Layer | Source | Purpose |
|-------|--------|---------|
| Embedded (compile-time) | libretro-meta (MIT) + TheGamesDB (CC BY-NC-SA) | Baseline genre/players, available offline, no setup |
| Runtime enrichment | LaunchBox (user-downloaded) | Descriptions, ratings, publisher, fills genre gaps |

### Specific improvements to consider

1. **Improve LaunchBox matching for runtime enrichment.** The 65% ROM match rate could be improved by implementing the same CRC-based lookup that game_db uses, rather than relying solely on title normalization.

2. **Use LaunchBox genres as a secondary signal.** When LaunchBox metadata exists at runtime, prefer its genre for games where game_db reports "Other" (98 cases where LaunchBox has a more specific genre).

3. **Fix known bad data in game_db.** A handful of games have clearly wrong player counts (e.g., Claymates = 8 players). These are likely CRC mapping errors in libretro-meta.

4. **Add "Compilation" to the genre taxonomy.** LaunchBox identifies compilations that game_db labels as "Action." This is a useful distinction for users.

5. **Consider multi-genre support.** LaunchBox's semicolon-separated genres are more informative than single-genre. The UI could show primary + secondary genres.

## 8. Migration Effort (if pursued despite recommendation)

If the decision were made to switch to LaunchBox as the embedded source, the work would involve:

1. **Legal clearance:** Contact LaunchBox team for explicit redistribution permission for factual data (genre, players).
2. **Build system changes:** Replace libretro-meta DAT parsing with LaunchBox XML parsing in `build.rs`. The XML is ~460 MB, so build times would increase significantly.
3. **Matching strategy overhaul:** Switch from CRC-based to title-based matching at compile time, with a fallback to CRC (would need to generate a CRC->title mapping from No-Intro DATs).
4. **Genre taxonomy mapping:** Map LaunchBox's 27 genre strings to the current shared taxonomy (largely already done in `normalize_console_genre()`).
5. **Testing:** Validate that coverage doesn't regress, especially for obscure/regional ROMs.
6. **Data update process:** Replace the libretro-meta download script with a LaunchBox XML download.

Estimated effort: 2-3 days of development + legal clearance (unknown timeline).

## Appendix: Script Outputs

Full comparison data is available in:
- `tools/reports/genre_comparison.txt` -- Per-system genre comparison with all disagreements
- `tools/reports/players_comparison.txt` -- Per-system player count comparison with all disagreements
- `tools/reports/licensing_analysis.txt` -- Detailed licensing analysis

Scripts used:
- `tools/compare_genre_sources.py`
- `tools/compare_players_sources.py`
- `tools/launchbox_license_check.py`
