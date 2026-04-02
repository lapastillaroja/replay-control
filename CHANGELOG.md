# Changelog

Chronological timeline of changes to the Replay Control companion app for RePlayOS.

---

## 2026-03-30

### Features
- feat: PWA app shell caching and offline fallback — precache static assets (CSS, JS, WASM, icons), cache-first for `/static/`, network-first for navigation, offline error page (`0b34353`)
- feat: add Search tab to bottom nav, system category icons, unfixed header (`cd31b26`)
- feat: graceful startup when storage unavailable + move assets to `/static/` (`e365dbb`)
- feat: add SG-1000 and 32X to baked-in game_db (`2cad33e`)

### Bug Fixes
- fix: startup pipeline detects incomplete scans, improved cache clarity (`7fd46a4`)

### Style
- style: add accent-colored logo to top bar, remove system card icons (`e250af2`)

### Refactoring
- refactor: LaunchBoxMetadata tuple to named struct fields (`d65ba23`)
- refactor: extract cache-control header values to constants (`8e93551`)
- refactor: fix clippy warnings and remove allow annotations (`c338980`)
- revert: remove auto M3U generation — should not modify user romset (`980e2e2`)
- chore: use replay.local instead of hardcoded IP, remove stale M3U comment (`f184151`)

### Documentation
- docs: verify NFS startup v2 design works for all storage types (`b5882e5`)
- docs: mark game detail variant improvements as implemented (`194b3a3`)

---

## 2026-03-27

### Features
- feat: alternate versions section on game detail page — clones and regional variants shown as chip links (`c2f36b9`)
- feat: "Also Available On" cross-system section on game detail page — matches same `base_title` across other systems (`c2f36b9`)
- feat: show TOSEC bracket flag labels in display names — [a] Alternate, [h] Hack, [cr] Cracked, etc. with numbered variants ("Alternate 2", "Trained 3") (`9c9ab13`)
- feat: TOSEC bracket flag classification and duplicate disambiguation — square bracket flags parsed into structured types, used to distinguish otherwise identical display names (`5a34821`)
- feat: TOSEC structured tag parsing — year, publisher, side/disk extraction from TOSEC filenames (`0c4ade8`)
- feat: resolve 95% of TOSEC CPC duplicate display names — version stripping, country codes, bracket flags, format suffix disambiguation (`800515c`)

### Performance
- perf: SQL pre-filter with `search_text` column — search latency 220ms to 14ms (`f79d950`)
- perf: parallelize global search across systems via `tokio::spawn` (`c660635`)
- perf: add Cache-Control headers for static assets (`edaf1df`)
- perf: limit `get_recents` to 15 entries (homepage only shows 11) (`88756b1`)

### Bug Fixes
- fix: default region preference to World instead of USA (`659de9e`)
- fix: fill bidirectional sequel links at build time — reverse-link pass ensures both P155 and P156 are populated (`dbb0b9e`)
- fix: allow clone ROMs as sequel link targets, prefer non-clones (`7c60167`)
- fix: include clone entries in display name disambiguation (`4305d64`)
- fix: use 1h cache for pkg assets — no content hash in filenames, immutable was incorrect (`6c61ee8`)
- fix: show system display name in startup scanning banner (`b48d376`)
- fix: show phase, system, and progress count in rebuild banner (`4dd239c`)
- fix: EU region correctly maps to "Europe" (was "Europe, USA") (`18bfe9f`)

### Refactoring
- refactor: convert activity SSE from polling to broadcast (`598277d`)
- feat: broadcast SSE for skin and storage change notifications (`eb3912d`)

### Documentation
- docs: game detail variant improvements design (`cc5070f`)
- docs: CPC game detail variant coverage analysis (`2852873`)
- docs: TOSEC variant display analysis (`354de38`)
- docs: Discover section redesign with rotating spotlights (`481122d`)
- docs: brainstorm 15 recommendation ideas with priority assessment (`388bdd4`)
- docs: verify TOSEC changes don't break No-Intro parsing (`efce37d`)
- docs: TOSEC structured tag parsing design (`09b1d1a`)
- docs: NFS graceful startup v2 design (`3e08d32`)
- docs: mark sequel/prequel chains as implemented (`83aa121`)
- docs: update load test results, close http-client eval (`bed6c25`)
- docs: TOSEC duplicate analysis and NFS startup v1 (`9f95e48`)

---

## 2026-03-24

### Performance
- perf: async DB pool API (`pool.get().await` + `conn.interact()`) — fixes tokio worker starvation hang that deadlocked the app on game detail pages for large systems (`cf96bf5`)
- perf: pool timeout (10s) + 3 DELETE mode read connections — 3x throughput improvement for Homepage (6.5 → 20.6 req/s at c=5), light endpoints reach 1100+ req/s under mixed load (`6f9df97`)
- perf: single-row DB lookup for game detail pages — 15s → <1ms cold cache by fetching one GameEntry by PK instead of loading all ROMs for the system (`c5d6797`)
- perf: SQL-level pagination for system ROM list — `LIMIT`/`OFFSET` in SQLite instead of loading all rows into memory (`f4f778f`)

### Features
- feat: TOSEC version stripping + country code recognition — improves display names and thumbnail matching for TOSEC-named ROM sets (`18bfe9f`)
- feat: auto-generate M3U playlists for multi-part TOSEC games — detects `(Disc N of M)` / `(Disk N of M)` patterns, groups siblings, writes M3U files at scan time (`7895689`)
- feat: runtime SQLite corruption detection with recovery UI — error-triggered `SQLITE_CORRUPT` detection, per-DB corrupt flag, full-page banner with Rebuild (metadata.db) or Restore/Repair (user_data.db) options (`1f6aa8c`)
- feat: user_data.db backup at startup — copies healthy DB to `.bak` before background pipeline runs; corruption recovery offers restore from backup (`1f6aa8c`)
- feat: organize favorites by developer — new `Developer` criterion in favorites organize, with `normalize_developer()` handling MAME manufacturer variations (licensing, regional suffixes, joint ventures) (`643bf31`)

### Bug Fixes
- fix: unfavorite from any page when favorites are organized into subfolders — recursive search removes `.fav` from all locations (`5531966`)
- fix: favorites sorted by date added (newest first) instead of system+filename, consistent across subfolders (`5531966`)
- fix: preserve file mtime when copying favorites during reorganization — prevents "Latest Added" showing incorrect results (`bfde961`)
- fix: use `read_untracked()` for system display name in favorites page — fixes reactive tracking warning on WASM hydration (`f0ccd94`)
- fix: startup no longer silently deletes corrupt user_data.db — flags pool and shows recovery banner instead (`1f6aa8c`)

### Code Quality
- fix: resolve all clippy warnings across crates (`f002a9a`)

### Documentation
- docs: update changelog, feature docs, design docs, and known issues for 2026-03-24 changes (`5f48f0d`)

---

## 2026-03-23

### Performance
- perf: full SSR for all pages — `Resource::new_blocking()` + `Suspense` replaces `Transition` on 10 pages, eliminating loading spinner flash; Home 2KB->74KB first paint (`8bfccc6`)
- perf: remove cache TTL for local storage — inotify + mtime + explicit invalidation covers all change scenarios; NFS TTL increased from 5min to 30min (`c6c0aa2`)
- perf: add 4 SQLite indexes (base_title, data_sources_type, series_order, alias_system), optimize `is_empty()` with EXISTS and `delete_orphaned_metadata()` with NOT EXISTS (`1a1a858`)
- chore: upgrade rusqlite 0.32->0.38, SQLite 3.46.0->3.51.1 (`eb5958c`)

### Bug Fixes
- fix: filesystem-aware SQLite journal mode — WAL only on ext4/btrfs/xfs/f2fs; exFAT/FAT32 (USB) get DELETE mode, fixing SQLITE_IOERR_SHORT_READ (522) caused by WAL shared memory incompatibility (`11dc11c`)
- fix: move 9 write operations from read pool to write pool — caught by `query_only = ON` defense on read connections (`3921262`)
- fix: explicit WAL checkpoints after bulk writes, use `scanning` flag instead of `busy` so ROM lookups work during import/thumbnail update (`e1f0fcd`)
- fix: favorites showing empty on reload — replace Transition with Suspense for predictable SSR hydration (`e8e3a8b`)
- fix: check server busy state before starting metadata/thumbnail operations — prevents flash-then-error when another operation is running (`26b6db1`)
- fix: move Clear Downloaded Images to Advanced section — re-downloading all thumbnails is costly, now gated behind Advanced toggle (`1952844`)
- fix: iOS Safari box art rendering (`e8e3a8b`)

### Features
- feat: multi-file ROM delete — enumerates and deletes all associated files (M3U discs, CUE BINs, ScummVM data dirs, SBI companions, arcade CHDs) with file count + total size in confirmation dialog (`445abc9`)
- feat: ROM rename restrictions — block rename for CUE+BIN, ScummVM, binary M3U with reason displayed below actions (`445abc9`)
- feat: orphan cascade on delete/rename — favorites, screenshots, user_data.db (videos, box art), metadata.db all cleaned up via new `delete_for_rom`/`rename_for_rom` methods (`445abc9`)
- feat: multi-disc detection — `detect_disc_set` finds (Disc N) siblings for Saturn-style CHDs without M3U wrappers (`445abc9`)
- feat: genre badges in favorites cards (`e8e3a8b`)
- feat: improved driver_status UX — hide green "Working" dots (noise for 56% of games), user-friendly labels replacing MAME jargon, "Emulation" heading (`5273f51`)
- feat: production SQLite PRAGMAs — journal_size_limit, foreign_keys, busy_timeout, manual WAL checkpoints on write connections, `query_only` on read connections, hourly PRAGMA optimize, eager pool warmup (`11dc11c`)

### Code Quality
- refactor: remove `is_local` from DB layer, use `JournalMode` enum — DB auto-detects filesystem via `/proc/mounts`, pool sizing based on journal mode (WAL=3 readers, DELETE=1), clean separation from `StorageKind` (`c2abf22`)
- refactor: extract param structs (`FilterUrlParams`, `SystemLookups`, `PaginationParams`) replacing 3 `#[allow(clippy::too_many_arguments)]` (`6aa8661`)
- refactor: consolidate 3 duplicate `format_size` functions into `util::format_size` (`6aa8661`)
- refactor: extract shared `Freshness` struct for cache TTL logic, eliminating duplication across 3 files (`c6c0aa2`)
- style: remove ~50 lines dead CSS, rename `.recent-*` prefix to `.scroll-card-*` (`e8e3a8b`)
- chore: remove sysroot hack — use standard Fedora `dnf --installroot` cross-compile setup with clear setup instructions (`4f28ac2`)

### Documentation
- docs: update for filesystem-aware journal mode, SQLite upgrade, server lifecycle (`04db204`)
- docs: update all documentation for pool migration, ROM management, cache TTL (`9a278c6`)
- docs: add internal analysis documents (`8ccc06c`)
- docs: mark ROM rename cascade as resolved in known issues (`896c927`)
- docs: add cross-compilation reference guide for Fedora (`4f28ac2`)

## 2026-03-22

### Performance
- feat: deadpool-sqlite connection pool — 3 concurrent read connections + 1 write, replacing single Mutex (`2fc1016`, `618314a`)
- fix: batch player lookups to eliminate N+1 in multiplayer filter (`3447489`)
- docs: load test results — 2x throughput for DB-heavy endpoints, 89x for light endpoints under mixed load (`b9d60f9`)

### Bug Fixes
- fix: WASM panic on game detail navigation — ManualSection's param_key Memo triggered effects on disposed signals, freezing the page on "Loading..." (`a009d03`)
- fix: hydration mismatch in GameListItem — removed `#[cfg(ssr)]` system_label resolution that differed between server and client (`9694dd5`)
- fix: path traversal protection on delete_rom/rename_rom server functions (`478f6ec`)
- fix: Closure::forget memory leak in use_debounce — single closure instead of one per keystroke (`478f6ec`)
- fix: SystemTime unwrap → unwrap_or_default in videos.rs (`478f6ec`)

### Code Quality
- refactor: make MetadataDb + UserDataDb stateless query namespaces — methods take `conn: &Connection` (`40072d9`)
- refactor: add Copy derive to 9 qualifying types (`f21652a`)
- refactor: split global_search (295 lines) into focused helper functions (`dbbb2b0`)
- refactor: extract rom_docs_handler from 127-line inline closure in main.rs (`1952b30`)
- refactor: deduplicate SSE handlers with generic sse_progress_stream builder (`ad3968a`)
- refactor: deduplicate MEGABIT_SYSTEMS — SSR delegates to core crate (`dedbe97`)
- refactor: standardize 28 ad-hoc lock expect() messages in import.rs (`be09c8a`)
- fix: resolve 14 clippy warnings across crates (`478f6ec`)
- fix: improve curl_get_json with redirect following, connect timeout, Accept header (`9694dd5`)
- test: integration tests for search helpers, ROM path parsing, batch player lookup (117 tests) (`5678068`)
- style: increase default text size to 110%, large to 140% (`d272648`)

### Documentation
- docs: DB connection pool architecture — design + implementation status (`d01cd23`)
- docs: ROM management analysis — multi-file rename/delete patterns (`a74979a`)
- docs: add scroll restoration to known issues (`a641780`)

## 2026-03-21

- feat: game manuals — in-folder document detection + archive.org on-demand download via RetroKit TSV (`70f1c48`)
- feat: inline delete confirmation for downloaded manuals (`ae8ed86`)
- feat: language preferences for manual search (`e8ab675`)
- fix: wrap manual server functions in spawn_blocking + register DeleteManual (`fe8cdca`)
- fix: 7 correctness + performance fixes for libretro core (`9a9411f`)
- feat: home screen + screensaver design for libretro core (`713c0ff`)
- feat: skin/theme support for libretro core with 11 palette mappings (`203a85b`)
- fix: double-buffered video + UI polish for libretro core (`80c2f3c`)
- feat: multi-page UI, crash fixes, position memory for libretro core (`2cfd599`)
- docs: libretro core skin/theme design (`98c4bb5`)
- docs: RetroAchievements evaluation, core home screen + screensaver designs (`b28f753`)
- docs: update feature documentation through 2026-03-20 (`78e9731`)

## 2026-03-20

- feat: add Named_Titles support and screenshot gallery — title screen + in-game screenshot displayed as labeled gallery on game detail page (`4c10d4e`)
- feat: add developer column to game_library — populated from arcade_db manufacturer + LaunchBox enrichment (uncommitted)
- feat: developer search in global search — searching "Capcom" returns all Capcom games (score 250) (uncommitted)
- feat: "Games by Developer" search block — horizontal scroll of games by matched developer above regular results, with multi-match ranking (uncommitted)
- feat: "Other developers matching" list — up to 2 additional developer matches shown as tappable links with game counts (uncommitted)
- feat: developer game list page at `/developer/:name` — full game list with system filter chips, infinite scroll, empty state for non-existent developers (uncommitted)
- fix: merge developer from LaunchBox metadata into game detail — enrichment was skipping developer field (`a55119c`)
- fix: "Other developers matching" heading shows original query instead of matched developer name (uncommitted)
- refactor: replace remaining tuples with BoxArtGenreRating, ImagePathUpdate, RomEnrichment structs + fix clippy warnings (`0575040`)
- docs: add tablet landscape layouts to proposal C design (`c2855fb`)
- docs: add title screenshots analysis, developer coverage, expand libretro core feasibility with CRT/HDMI support (`46d0e89`)

## 2026-03-19

- feat: sequel/prequel play order navigation — breadcrumb `← Prev | N/M | Next →` using Wikidata P155/P156 chains with ordinal fallback (`8fbba16`)
- feat: cross-system Wikidata series matching — match library ROMs against all Wikidata entries regardless of platform, fixing games like Metal Slug X (Wikidata: sony_psx, ROM: arcade_fbneo) (`964c601`)
- feat: roman numeral normalization for Wikidata matching — "streets of rage ii" now matches "streets of rage 2" (`a04f9f3`)
- fix: correct 4 bogus Wikidata platform QIDs and add 17 missing platforms — DS, PCE, Sega CD, 32X, Atari, 3DO, CD-i, MSX, CPS-3, NAOMI 2, Model 3, ST-V, Neo Geo variants; series data 3,935 → 5,345 entries (+36%) (`e8767b3`)
- fix: exclude only current game from series siblings, not cross-system ports — same game on other systems shows in series, current ROM does not (`ae40730`)
- fix: use Suspense for game detail to fix sequel link navigation — Transition showed stale content making sequel links appear broken (`94e0188`)
- refactor: replace tuple types with AliasInsert/SeriesInsert structs, removing clippy type_complexity warnings (`964c601`)
- chore: cleanup dead code — gate test-only methods behind #[cfg(test)], remove debug eprintln (`a327837`)

## 2026-03-18

- refactor: extract matching logic to core crate — alias_matching, metadata_matching, image_matching modules (`2d9bb6d`)
- refactor: unify image matching into single core find_best_match path (`7f34fc4`)
- refactor: eliminate hardcoded thumbnail strings across codebase (`daedc01`)
- refactor: consolidate thumbnail logic into core crate (`968e051`)
- feat: restructure More page into Preferences / Game Data / System sections + declutter game detail (`e648264`)
- feat: unify region preferences into single settings section (`db0f673`)
- fix: subtitle-stripped fallback for Wikidata series matching — catches DonPachi II and 10+ additional series (`8de96fb`)
- fix: base_title tilde inside parens + enable arcade Wikidata series — 546 arcade entries now populate (`4866c18`)
- docs: add arcade thumbnail gaps + clone series analyses (`1d49dc4`)
- docs: update UI design proposals with new features (`35c99b4`)
- docs: add Wikidata attribution to metadata page (`670c886`)

## 2026-03-17

- refactor: sequenced startup pipeline replacing 4 independent racing tasks with ordered phases — auto-import → populate → enrich → watchers (`5a7abc8`)
- refactor: extract ImportPipeline + ThumbnailPipeline from AppState with shared busy flag for mutual exclusion (`5a7abc8`)
- feat: non-blocking startup — server responds immediately with empty data during warmup, "Scanning game library..." banner shown (`5a7abc8`)
- fix: single DB connection policy — import holds Mutex directly, eliminated 3 rogue MetadataDb::open() calls causing SQLite corruption (`f38f77a`)
- fix: filesystem-aware SQLite locking — WAL mode on local storage (USB/exFAT, SD/ext4), nolock+DELETE on NFS only (`257831f`)
- feat: auto-rebuild thumbnail index at startup when data_sources exists but index is empty (data loss recovery) (`257831f`)
- feat: single-pass LaunchBox XML parsing — was triple-parse taking 15min on Pi, now ~6s (`5a7abc8`)
- fix: remove 10-second cleanup thread delays — busy flag clears immediately after operations (`5a7abc8`)

## 2026-03-16

- feat: add game series and cross-name variant system — algorithmic series_key, TGDB alternates, LaunchBox alternate names (`0ff81d2`)
- feat: add Wikidata series data with arcade support — 3,935 entries across 194 series via SPARQL extraction (`63c07fa`)
- fix: unify alias resolution with fuzzy matching for colon/dash variants — bidirectional TGDB aliases (`a18d9a6`)
- feat: concise labels for "Other Versions" — region only for same-name, name+region for cross-name (`ed40b2c`)
- feat: add CRC32 hash-based ROM identification for cartridge systems — 9 systems with No-Intro DAT matching (`07e9815`)
- feat: add secondary region preference with Strategy C sort order — Primary > Secondary > World (`84879df`)
- feat: add text size toggle (normal/large) with rem-based image scaling (`8951b19`)
- feat: add pull-to-refresh for iOS PWA standalone mode — PullToRefresh.js lazy-loaded (`c53b6f9`)
- feat: show arcade clone siblings as "Arcade Versions" on game detail page (`8ca1cf2`)
- fix: unify box art resolution between cards and detail page — single resolve_box_art() path (`fa14928`)
- refactor: split metadata_db.rs (2,895 lines) into 7 focused sub-modules (`84cf3d5`)
- fix: tilde dual-title boxart matching — split on ~ and match either half (`84cf3d5`)
- fix: non-blocking startup when game library is empty (`f55ed74`)
- fix: eliminate rogue DB connections causing corruption (`f38f77a`)
- docs: add internal analysis and planning documents (`various`)

## 2026-03-14

- fix: metadata page horizontal overflow on mobile — system names wrap instead of truncating (`61226ab`)
- fix: on-demand thumbnail download panics outside Tokio runtime, breaking enrichment and thumbnail counts after rebuild (`ac36347`)
- fix: thumbnail download counter starts at 1 instead of 0 (`170f638`)
- feat: redesign metadata page layout with embedded DB stats — reorder sections, add built-in game data info card (`d0b2349`)
- feat: add unified GameInfo API with lightweight RomListEntry for ROM list views (`2adcf2b`)
- feat: parse `<Developer>`, `<ReleaseDate>`, `<Cooperative>` from LaunchBox XML (`68b267b`)
- feat: filter non-playable MAME entries at build time, preserve 26 BIOS with `is_bios` flag — arcade DB 28,593 → 15,440 entries (`adf12a2`)
- fix: version-stripped box art matching checks fuzzy index too — fixes Dreamcast TOSEC-named ROMs (`7af0a5f`)
- docs: add player count improvement analysis (`081ae64`)
- feat: parse `<MaxPlayers>` from LaunchBox XML for player count enrichment of 11 zero-coverage systems (`0e1bdd7`)
- refactor: derive thumbnail counts from `game_library.box_art_url` instead of stale `game_metadata.box_art_path` (`0529f8d`)
- fix: prevent orphan cleanup race condition with `metadata_operation_in_progress` guard, skip unenriched systems, 80% safety net (`3645623`)
- docs: add coverage snapshot and non-playable entry analysis (`76ed3f3`)
- feat: add orphaned image cleanup button on metadata page with `find_orphaned_thumbnails()` and `delete_orphaned_metadata()` (`6a522ce`)
- fix: path traversal check `path.contains("..")` → `path.split('/').any(|s| s == "..")` — restores 25 ROM images across 7 systems (`fe253cd`)
- feat: update catver.ini to v0.285 (merged with category.ini, 49,801 entries) and add nplayers.ini v0.278 as player count fallback (427 fills) (`4cddf36`)
- feat: improve image matching with slash dual-name, TOSEC version strip, and CHD filtering (`04ffb89`)
- refactor: consolidate LaunchBox platform mappings into System struct (`2eeea32`)
- feat: improve ScummVM detection and filter orphan M3U stubs (`8c89834`)
- docs: reorganize documentation structure (`9ad58c7`)
- feat: two-tier genre system with `genre_group` for unified filtering (`6afaafc`)
- refactor: migrate video storage from `videos.json` to SQLite `user_data.db` (`6927907`)
- docs: add conventional commits style guideline to CONTRIBUTING.md (`523ce2b`)
- docs: add chronological changelog with commit references (`bf3e91f`)
- fix: resolve Leptos hydration warnings on games page (`a2dfedc`)
- fix: guarantee `metadata_operation_in_progress` is cleared after rebuild, even on panic (`f5c16f8`)
- feat: block DB operations during game library rebuild with completion feedback (`ec47b6d`)
- refactor: rename `rom_cache` → `game_library` across codebase (`412793b`)
- test: fix broken tests and add coverage for is_special, variants, is_local (`cdd250e`)
- fix: improved variant labels, filtered arcade clones, skip broken symlink previews (`5be5e06`)
- feat: auto-detect new/changed ROMs via inotify filesystem watcher on local storage (`5bec806`)

## 2026-03-13

- feat: `is_special` flag to filter FastROM patches, unlicensed, homebrew, pre-release, and pirate ROMs (`9a29b96`)
- feat: `is_hack` support — filter hacks from variants/dedup, show in dedicated Hacks section (`fdbd788`)
- fix: metadata stats use LEFT JOIN with game library fallback for M3U dedup (`54ced4f`)
- feat: app-specific config file (`.replay-control/settings.cfg`) separate from `replay.cfg` (`9a29b96`)
- fix: populate game library after import when cache is empty — startup race condition (`309b8e4`)
- feat: genre fallback from LaunchBox when baked-in game_db has no genre (`f36b6b9`)
- fix: prioritize primary ROMs over betas for genre assignment in build (`89e4410`)
- feat: translation detection and filtering from variants/dedup with dedicated Translations section (`6a503d6`)
- fix: stop event propagation on boxart picker close button (`55a2cd6`)
- feat: related games section with genre-based similarity (`3ef8199`)
- fix: re-enrich game library after metadata/thumbnail imports (`fa76dcc`)
- fix: trailing article normalization in `base_title` for variant grouping (`5262c66`)
- feat: deduplicate recommendations by filtering clones and regional variants (`68f8938`)
- refactor: organize core crate into logical subdirectories (`4b14f20`)
- fix: case-insensitive exact matching for thumbnail resolution (`bb8391c`)
- fix: M3U dedup metadata stats, MAME/FBNeo fallback, PSX m3u extension (`e5e2426`)
- feat: randomize ordering for top-rated and favorites-based recommendation picks (`f46514f`)
- test: arcade image matching pipeline tests (`74e571e`)
- fix: arcade DB translation for thumbnail matching (`a36a6fe`)
- fix: resolve recommendation box art from filesystem (`acbf4d5`)
- fix: fuzzy matching in `update_image_paths_from_disk` (`48912cf`)
- fix: invalidate image cache after metadata import (`b1fd6e1`)
- feat: switch thumbnail indexing from git clone to GitHub REST API (`f7e2438`)
- fix: fall back to log files when journald is disabled (`a943c8c`)

## 2026-03-12

- feat: metadata busy banner and graceful DB unavailability handling (`a702a1d`)
- feat: NVMe storage support for Pi 5 PCIe (`1cee7eb`)
- refactor: shared DB initialization with eager open and corruption recovery (`83654d0`)
- fix: recommendations biased toward systems with downloaded thumbnails (`94675b0`)
- fix: eager DB open with auto-reopen on external file deletion (`b69ff78`)
- fix: filter out stub thumbnails (<200 bytes) during indexing (`6dac291`)
- fix: M3U Windows backslash paths and comma-inverted display names (`ef3258d`)
- feat: auto-match metadata for externally added ROMs using normalized title index (`bf66440`)
- feat: box art swap — pick alternate cover art per ROM from region variants (`abe23ac`)
- style: resolve all clippy warnings across codebase (`5c27f7f`)
- fix: region preference styling, SSR genres, and box art swap design (`cb85f8c`)
- feat: prevent parallel metadata operations with atomic guard (`701510e`)
- feat: manifest-based thumbnail index stored in SQLite for on-demand downloads from GitHub (`29f175d`)
- feat: enhance `dev.sh` with Pi deployment mode, add `strip=debuginfo` to dev profile (`82ef3ac`)
- feat: recents entry creation on successful launch for immediate home page reflection (`b09c8b6`)
- perf: build optimization with `dev.build-override` opt-level 2 (`acb6c94`)
- refactor: replace `reqwest` with `curl` subprocess for HTTP calls, eliminating 11 TLS crates (`9ffc41e`)
- fix: SSR recommendations with L2 warmup, enrichment, and race condition fixes (`36d4505`)
- feat: persistent SQLite game library (L2 cache) with write-through and `nolock` fallback for NFS (`cd47235`)
- perf: 98% faster page loads via tier 1+2 cache optimizations (`6a4e767`)

## 2026-03-11

- feat: favorites/rating-based recommendations and ScummVM dedup fix (`3385e18`)
- feat: home page recommendation blocks — random picks, top genres, multiplayer, favorites-based, top-rated (`e102987`)
- feat: M3U multi-disc support — hide individual disc files when playlist exists, aggregate sizes (`de13e74`)
- feat: metadata-enriched search using genre and year, min-rating filter (`c075242`)
- feat: word-level fuzzy search matching with word-boundary scoring (`6b76abc`)
- fix: auto-delete image repos after match, add cache management (`449e03c`)
- test: integration tests (50+ tests including 15 integration), extract router builder (`8a0bb34`)
- feat: region preference setting affecting sort order and search scoring (`faa135d`)
- feat: megabit size display for 24 cartridge-based systems, split CSS into 17 modules (`7c385b8`)
- refactor: extract game detail sub-components, typed filter state (`93dc64b`)
- refactor: split server functions and API into domain modules (`efc04b5`)
- refactor: extract reusable components — RebootButton, unified Transition, auto-close SSE stream (`e37ee72`)
- feat: arcade driver status badges, favorites filter, rating display, multiplayer filter (`7ef4564`, `54ceb93`)
- fix: validate metadata DB image paths against disk to catch fake-symlink artifacts (`49413d9`)
- feat: box art thumbnails on home page and favorites, storage disk usage bar (`1926e53`)
- feat: extended search filters and ROM list filter persistence (`5349b87`)
- refactor: merge Games tab into Home page, rename to Games (`ab1695b`)
- feat: user screenshots gallery with fullscreen lightbox viewer (`138cd3d`)
- feat: game launching on RePlayOS with health check and automatic recovery restart (`6f221e4`)
- fix: search input focus on client-side navigation (`2281faa`)
- feat: search icon in top bar, recent searches, random game button, "/" shortcut (`618cb9c`)
- fix: `.fav` suffix in recently played entries and deduplication (`08b28ad`)

## 2026-03-10

- feat: game videos — search via Piped/Invidious APIs, inline preview, pin/save (`b8145d8`)
- feat: dedicated `/search` page with URL-persisted query params (`b620800`)
- feat: image import with SSE progress streaming and cancel support (`638e026`)
- feat: global cross-system search with genre, driver status, and favorites-only filters (`b3bb571`)
- feat: arcade image support via multi-repo mapping (Atomiswave + Naomi + Naomi 2) (`d46a257`)
- fix: improved arcade LaunchBox matching (`b1d5aa1`)
- feat: game images — per-system image download from libretro-thumbnails (`7c53237`)
- feat: background metadata import with progress tracking, auto-import, per-system coverage (`f13a9f2`)
- feat: LaunchBox XML metadata import with streaming parser and normalized title matching (`1f9b515`)
- refactor: skin sync toggle and theme-to-skin rename (`f4e7cd0`)
- feat: interactive skin selection and CSS theming (`b82964a`)

## 2026-03-09

- feat: hostname configuration with mDNS address update (`a3c8386`)
- feat: skin theming — browse and apply RePlayOS skins, sync app colors to active skin (`f0cb7bf`)
- feat: Wi-Fi configuration page and NFS share settings page (`e3f27a3`)
- feat: favorites organization for grouping by system subfolder (`9311e90`)
- feat: internationalization (i18n) support (`9311e90`)
- feat: dynamic storage detection with config file watcher (SD, USB, NFS) (`f685eef`)
- feat: embedded non-arcade game database (~34K ROM entries across 20+ systems) (`693be18`)
- feat: ROM filename parsing for No-Intro and GoodTools naming conventions (`693be18`)
- feat: install script and aarch64 cross-compilation support (`ab0e032`)
- feat: storage type card and empty state on home page (`780dec8`)
- feat: system display name in ROM list header (`53a30c1`)
- fix: add timestamps to favorites for true "recently added" ordering (`2b7f172`)
- feat: game detail page with system, filename, size, format, and arcade metadata (`43a316a`)
- feat: expanded arcade DB with FBNeo, MAME 2003+, and MAME current — 28,593 entries (`5f78bf9`)
- feat: embedded arcade database (PHF map) with Flycast, Naomi, and Atomiswave data (`b54aab7`)
- feat: unfavorite action on favorites page with `ErrorBoundary` handling (`5f688c6`)
- feat: PWA support with manifest and service worker, in-memory cache layer (`c4f1556`)

## 2026-03-08

- feat: initial project setup — Leptos 0.7 SSR app with WASM hydration, Axum server, client-side routing (`af1d5e9`)
- feat: ROM browsing by system with infinite scroll and pagination (`af1d5e9`)
- feat: per-ROM favorite toggle, rename, and delete with confirmation (`af1d5e9`)
- feat: home page with last played hero card, recently played scroll, and library stats grid (`af1d5e9`)
- feat: favorites page with per-system cards (`af1d5e9`)
- chore: dev script (`dev.sh`) with auto-reload support (`a59c0a2`)
