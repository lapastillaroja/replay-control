# Arcade Boards

How arcade games are attributed to a hardware board, stored, and surfaced.

## The `ArcadeBoard` type

`ArcadeBoard` (in `replay-control-core::arcade_board`, wasm-safe core) is a `Copy` enum with one variant per tracked board (CPS-1/2/3, Neo Geo MVS, the Sega System / Model families, Taito F2/F3/Z, IGS PGM, Cave, Midway, Namco System, Konami, Data East, Irem, Jaleco). Selection is **curated**: a board earns a variant only when it groups *several* games — single-game drivers, and per-maker gambling / mahjong driver families, are deliberately left out so a board page surfaces a family of titles rather than a lone game. Each variant exposes:

- `as_tag()` — stable ASCII slug stored in the databases and used in `/board/:tag` URLs (e.g. `cps2`, `neogeo_mvs`).
- `from_tag()` — inverse, for deserializing the stored slug.
- `display_name()` — bare label (`CPS-2`), used as a recognizer search token.
- `manufacturer()` — board maker (`Capcom`).
- `display_label()` — `"{display_name} ({manufacturer})"` (`CPS-2 (Capcom)`), used at every UI surface. The bare `display_name()` stays manufacturer-free so it doesn't pollute search tokens.

### `sourcefiles()` — the single source of truth

`ArcadeBoard::sourcefiles()` lists **every** emulator-driver sourcefile spelling that identifies a board, across all upstreams. `from_sourcefile()` is a scan over it. Adding a new upstream spelling is a one-line edit here. Each entry is one of:

- MAME-current canonical `manufacturer/board.cpp` (`igs/pgm.cpp`)
- FBNeo's `d_`-stripped form, which often uses a different directory or basename (`pgm/pgm.cpp`, `sega/sys16a.cpp`, `taito/taitoz.cpp`, `pre90s/namcos1.cpp`)
- MAME 2003+ legacy bare `board.c` (`pgm.c`, `system16.c`)

Invariants are test-pinned: every spelling round-trips to its board, and no spelling is shared across two boards (which would make the unordered scan order-dependent).

**Maintaining mappings on upstream updates.** Driver sourcefile names change between MAME/FBNeo releases — MAME renamed `taito/taitof3.cpp` → `taito/taito_f3.cpp`, moved `cave/cave.cpp` → `atlus/cave.cpp`, and `sega/atomiswave.cpp` → `sega/dc_atomiswave.cpp`. When a spelling vanishes upstream, `from_sourcefile()` silently returns `None` and that board quietly empties — a board can drop to zero with no error. The round-trip tests only prove the list is self-consistent, **not** that it still matches the bundled data. So after refreshing `data/upstream/` (`scripts/download-arcade-data.sh`), compare per-board game counts (Metadata → coverage, or `library_report`) before/after the bump and add any renamed spelling to `sourcefiles()`. There is no automated guard.

## Catalog-build attribution

`tools/build-catalog` resolves a board per arcade ROM **per source** and stores it in `arcade_game.board`. The four arcade sources differ in how they carry the driver:

| Source | Carries sourcefile? | Shape |
|--------|--------------------|-------|
| MAME 0.285 (`mame0285-arcade.xml`) | **Yes** — the extract keeps the `sourcefile` attribute | MAME-current `manufacturer/board.cpp` |
| MAME 2003+ (`mame2003plus.xml`) | Yes | legacy `board.c` |
| FBNeo (`fbneo-arcade.dat`) | Yes | `dir/d_board.cpp` |
| Flycast (Naomi CSV) | n/a | board derived from the `GDS-` / `GDL-` display-name prefix via `flycast_board()` |

`board_tag_from_sourcefile()` normalizes the one parser-shape quirk `from_sourcefile()` doesn't model — `normalize_sourcefile()` strips the FBNeo `d_` basename prefix — then defers to `ArcadeBoard::from_sourcefile()`. The legacy `.c` and canonical `.cpp` forms match verbatim against the `sourcefiles()` table. There is no separate legacy-to-canonical map; all spellings live on the enum.

## Runtime merge — board has its own priority

`merge_for_system()` (in `replay-control-core-server::game::arcade_db`) folds the per-source `arcade_game` rows for a ROM into one `ArcadeGameInfo`. Most fields follow the per-system metadata priority (`arcade_source_priority`), but **board is resolved on its own fixed order**, independent of it:

```
BOARD_PRIORITY = [Naomi, FBNeo, MAME 2003+, MAME]
```

A board is a physical property of the PCB — the same no matter which emulator's metadata names it — so it should not follow, say, `arcade_mame`'s "MAME first" preference. **FBNeo is preferred over MAME 2003+**: FBNeo is Replay's primary arcade core and carries the richest board coverage, while MAME 2003+ is legacy (enabled only on older Pis). MAME 0.285 sits last — its `sourcefile` (the extract keeps it) is used only when no higher-priority source carries the board, e.g. CoJag, which only MAME has. Naomi leads because its GD-ROM board hints live only in that source. An explicit sweep of any source not named in `BOARD_PRIORITY` follows, mirroring the metadata loop, so a newly added source still contributes a board rather than silently dropping.

## Storage

`library.db`'s `game_library.board` holds the resolved board slug (or empty), written at scan time from `ArcadeGameInfo::board`. A partial index `idx_gl_board(system, board) WHERE board != ''` backs the board queries. At read time the slug is deserialized back to `ArcadeBoard` via `from_tag()`.

Because the board column is written during scan, **changing the attribution or the merge order requires both a catalog rebuild and a `library.db` wipe/rescan** for existing installs to pick it up.

## Surfaces

All board queries live on `LibraryDb` (no SQL in the app crate) and exclude clones / translations / hacks / specials so counts match the original release set.

- **Board page** (`/board/:tag`, `pages/board.rs`) mirrors the developer page. Server fns `get_board_games` / `get_board_genres` drive it via `LibraryDb::board_systems`, `board_genre_groups`, and `search_game_library` with `SearchFilter::board` set.
- **Search** uses `replay-control-core-server::library::search_recognizer` two ways: `recognize()` strips a leading/trailing board phrase into `SearchFilter::board` (the per-system ROM-list pill), and `find_board_matches()` token-scores every board against the query for the `/search` discovery block (`search_by_board` → top match + others, reusing `games_by_board` and `board_game_counts`).
- **Recommendations** (`server_fns/recommendations.rs`) add `LibraryDb::top_boards` as a Discover-pill candidate and as spotlight rotation type 6, both linking to `/board/:tag` and reusing the shared pill / `GameSection` rendering.

## Notes

- **M84:** Irem's M84 hardware has no dedicated driver in any source — MAME and FBNeo both fold M84 games into the `m72` driver — so it can't be split from M72 without a per-ROM allowlist.
- The recognizer's per-variant synonyms (`board_tokens`) and the `sourcefiles()` spellings are deliberately separate lists: one is human-typed search terms matched fuzzily, the other is machine driver filenames matched exactly. Both anchor on `display_name()` / `as_tag()` from the enum.
