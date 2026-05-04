# Changelog

Chronological timeline of changes to the Replay Control companion app for RePlayOS.

---

## [0.4.0-beta.7](https://github.com/lapastillaroja/replay-control/releases/tag/v0.4.0-beta.7) - 2026-05-04

### Highlights

- **External-metadata pipeline redesign.** LaunchBox text and libretro thumbnail manifests move out of per-storage `library.db` into a host-global `external_metadata.db` at `/var/lib/replay-control/external_metadata.db`. Newly added ROMs (after a LaunchBox refresh) automatically pick up metadata on the next enrichment pass — fixes the long-standing one-shot-import bug. Storage swaps no longer re-parse the 460 MB XML; only the binary keeps a stamp of the last-parsed file's CRC32. Image files stay per-storage at `<storage>/.replay-control/media/`, only the manifest of available filenames moves host-global.
- **`library.db` schema bumped 1 → 3** in two migration steps. v2 drops `game_metadata`, `thumbnail_index`, `data_sources` (relocated to `external_metadata.db`). v3 adds `game_description` (description + publisher denormalized so the game-detail page stays on a single pool). A downgrade guard refuses to open a DB stamped with a newer version than the binary.
- **One-button metadata refresh.** The legacy "Import LaunchBox metadata" UI is replaced by a single host-global "Refresh metadata" button that downloads, parses, and re-enriches every system in one flow with live SSE progress (Downloading → Parsing → Enriching → Complete). New `Activity::RefreshExternalMetadata` SSE variant — clients should add a render branch.
- **Game-detail page request path is now strictly single-pool.** Description, publisher, ratings, genre, and image stats all read from `library.db`; the host-global `external_metadata.db` is touched only at enrichment time.
- **Per-system enrichment writes collapse to one transaction.** Six separate `db.write` calls per system (developer / cooperative / year / release-date resolver / game_description / box-art-genre-rating) are now bundled into a single library-pool write — saves ~1.5 s of per-commit fsync across a 30-system re-enrichment on Pi.

### Added

- New host-global `external_metadata.db` (LaunchBox `launchbox_game` / `launchbox_alternate`, libretro `thumbnail_manifest` / `data_source`, `external_meta` key-value).
- New per-storage `game_description (system, rom_filename, description, publisher)` table in `library.db`. Truncate-and-repopulate per system on every enrichment pass.
- `Activity::RefreshExternalMetadata { progress }` SSE variant + `RefreshMetadataPhase` (Checking → Downloading → Parsing → Enriching → Complete/Failed/**UpToDate**) + `RefreshMetadataProgress` (source_entries, downloaded_bytes, elapsed_secs, error). Mirrored on both SSR-side `api::activity` and WASM-side `types`.
- `BackgroundManager::spawn_external_metadata_refresh` and `spawn_external_metadata_download_and_refresh` for UI-triggered refreshes (regenerate, download).
- `library_db::resolve_launchbox_xml(cache_dir, storage_rc_dir)` — single helper that picks the LaunchBox XML across the host-global cache and per-storage legacy locations (boot-time hash check + UI download both use it).
- `replay_control_core::title_utils::normalize_title_for_metadata` — single canonical normalizer used by both the import-time index and the per-row read-time lookup, so the two sides can never drift.
- Setup checklist's "metadata imported?" now reads `external_metadata.launchbox_game`'s row count.
- First-boot data seeding (Phase 0.5): on a fresh install, the startup pipeline silently downloads the LaunchBox XML and libretro thumbnail manifest before the first ROM scan so initial enrichment has full data. Network failures are warn-logged and the pipeline continues — offline-ready behaviour is preserved. Subsequent boots skip the phase entirely.
- `launchbox::fetch_upstream_head()` — single HEAD request to the LaunchBox ZIP URL that returns both ETag and Content-Length, eliminating the two-subprocess pattern used by the former download flow.

### Changed

- Boot pipeline Phase 1 (`phase_auto_import`) is now a content-derived hash check + refresh against `external_metadata.db` — replaces the legacy "DB-empty" gate that broke when ROMs were added after a one-shot import. Hash + stamp-read run in parallel via `tokio::join!`. After refresh, every active system is re-enriched so launchbox data flows through `game_library` + `game_description`.
- Enrichment now reads launchbox via a single batched per-system query (`external_metadata::system_launchbox_rows`) instead of seven separate one-field queries. Per-ROM lookup uses normalized-title candidates (handles arcade clones via the parent's display name).
- Image index construction (`build_image_index`) drops its DB-connection arg — it's now a pure filesystem walk plus the pre-loaded libretro repo data, called from `tokio::task::spawn_blocking`.
- Game-detail page lookup of description + publisher reads from per-storage `game_description` (single library-pool acquire) instead of cross-pool acquiring `external_metadata.db`.
- `LibraryDb::all_ratings`, `image_stats`, `rom_genre` are now sourced from `game_library` (already populated by enrichment) instead of the dead per-storage `game_metadata` table.
- Download-progress callback throttled from per-64-KB-chunk to every-1-MB so the activity SSE channel doesn't churn 3 200 lock+broadcast cycles per 200 MB download.
- Parse-progress now updates the activity stream every 5 000 entries so the UI banner shows a live counter during the 30–90 s LaunchBox parse.
- "Refresh metadata" now performs an HTTP ETag check before downloading — the stored `launchbox_upstream_etag` key in `external_meta` is compared against the server's current ETag via a single HEAD request. On match, the flow short-circuits to `RefreshMetadataPhase::UpToDate` and shows an "Already up to date" result for 5 seconds, skipping the 100+ MB download entirely. Clearing metadata also clears the stored ETag so a post-clear refresh always re-fetches.

### Removed

- Per-storage `game_metadata`, `thumbnail_index`, `data_sources` tables (relocated to `external_metadata.db`).
- Legacy `LibraryDb::bulk_upsert` / `lookup` / `system_metadata_*` / `clear` / `is_empty` / `delete_orphaned_metadata` / `bulk_update_image_paths` / `system_box_art_paths` / `entries_per_system` / `stats` and the `GameMetadata` / `MetadataStats` / `DataSourceInfo` / `DataSourceStats` / `ThumbnailIndexEntry` / `ImagePathUpdate` types.
- Legacy `library/imports/launchbox.rs` import functions (`import_launchbox`, `run_bulk_import`, `build_rom_index`, `build_index_entries`, `import_launchbox_aliases`) and the `library/matching/metadata.rs` auto-match module — replaced by `library/external_metadata_refresh.rs::refresh_launchbox`.
- Legacy `api::ImportPipeline` and the `import_launchbox_metadata` server fn — replaced by `BackgroundManager::spawn_external_metadata_refresh` + `spawn_external_metadata_download_and_refresh`.
- Legacy `cleanup_legacy_metadata_db` (pre-0.5 `metadata.db` cleanup); the upgrade path is now far past it.
- `update_image_paths_from_disk` and the `ImagePathUpdate` flow (legacy thumbnail-download path that wrote `box_art_path` to `game_metadata`); the new flow writes `box_art_url` directly to `game_library`.

### Fixed

- Recents list arcade-display-name resolution is now one catalog round-trip per system instead of one per ROM (N→1), matching the batch approach already used by favorites.
- "Update Thumbnails" no longer re-fetches the libretro manifest from GitHub (~70 API calls) when clicked a second time within 5 minutes — the last-fetched timestamp is stored in `external_meta` and used as a 5-minute TTL gate.
- "Refresh metadata" clicked when no upstream change occurred now shows "Already up to date" in the result strip for 5 seconds instead of producing no visible feedback. The ETag check runs in the `Checking` phase so the banner is visible before the result is known.
- ROMs added to a system after a one-shot LaunchBox import are now enriched on the next pass (was: silently skipped forever).
- Two concurrent boots (boot pipeline + storage-watcher restart) no longer race the LaunchBox refresh — the activity slot is claimed before the hash check, so the second caller cleanly bails.
- Activity SSE no longer flickers `Idle` between the download and parse phases of a one-button refresh — the guard is threaded from the download path into `phase_auto_import_inner` via an explicit parameter.
- Neo Geo (`snk_ng`) re-categorized from `Console` → `Arcade` so MAME-shortname ROMs (`mslug.zip`, `kof98.zip`) route through `arcade_db` instead of failing to match LaunchBox.

### Migration / Upgrade Notes

- **Downgrade is not supported.** Once `library.db` is stamped with v3, an older binary refuses to open it (the downgrade-guard check in `LibraryDb::run_migrations` raises an error). Roll forward only.
- First boot after upgrade re-parses the LaunchBox XML (~5–8 minutes on Pi at the typical ~150 K-game XML size) because the new `external_metadata.db` starts empty. Subsequent boots are no-op when the XML hash matches the stamp.
- Existing per-storage `<storage>/.replay-control/launchbox-metadata.xml` continues to work — the refresh path checks the host-global cache first, then falls back to the per-storage location.

---

## [0.4.0-beta.6](https://github.com/lapastillaroja/replay-control/releases/tag/v0.4.0-beta.6) - 2026-05-03

### Highlights

- The "library shows no metadata after upgrade" silent-failure mode is now caught and surfaced. Beta.4-to-beta.5 upgrades that replaced the binary without refreshing `catalog.sqlite` (auto-update from a release whose updater predated catalog-swap) are detected at startup; arcade lookups short-circuit cleanly instead of spamming per-row SQL errors, and the new `<AssetHealthBanner>` tells the user to reinstall. Generic enough that future shipped-asset incompatibilities (themes, fonts, …) plug into the same surface.
- Production fd exhaustion under heavy thumbnail fan-out is fixed structurally. A new `ThumbnailDownloadOrchestrator` replaces the previous "every cache-miss spawns a `tokio::spawn`" pattern with a bounded concurrency cap, shared dedup, and visible-vs-bulk priority. Beta.5 telemetry showed 1 012 / 1 024 fds open mid-rescan with 993 sockets — that class of failure can no longer occur.
- NFS slow-mount on cold boot no longer kills startup. The 15 s `STORAGE_READY_TIMEOUT` that put beta.5 NFS users in a `Restart=on-failure` loop is gone; the not-ready case routes through the existing `/waiting` page until the mount surfaces, and the background re-detection loop activates storage as soon as it does.
- Cold-cache rebuilds across the home and metadata pages no longer trip the 15 s `INTERACT_TIMEOUT` tripwire on 100 k+ ROM libraries. `metadata_page_snapshot` split its 8-query closure (then parallelized via `tokio::join!`); `bulk_insert_aliases` chunks into 5 k-row transactions; `get_recommendations` moved to the same `SsrSnapshot<T>` pattern as the metadata page (event-driven invalidation, single-flight rebuild, stale-on-`None`). Per-ROM warmup rate improved ~7×.
- Game-detail page now has a unified lightbox carousel covering box art, title screen, in-game screenshot, and user captures — tap any image and swipe through them all.
- `/media/*` and `/rom-docs/*` get HTTP `ETag` + 304 revalidation on top of the existing 1-day `Cache-Control`. Box art / thumbnails / marquees that dominate game-grid traffic now revalidate body-less when the browser's max-age expires.

### Added

- HTTP `ETag` + 304 revalidation on `/media/*` and `/rom-docs/*`. Strong tag derived from `mtime + size`; once a browser's 1-day `max-age` expires, the next reload sends `If-None-Match` and the server replies with a body-less 304 if nothing changed instead of re-shipping the bytes. Box art / thumbnails / marquees see the biggest win — they dominate game-grid traffic. Hot path adds one `tokio::fs::metadata` call on cache-miss; on warm page-cache it's noise. Cache-Control max-age stays at 1 day.
- Game-detail lightbox now covers box art, title screen, in-game screenshot, and user captures as a single carousel — tap any image and swipe through them all. Per-image rendering hint (`LightboxImage { url, pixelated }`) keeps nearest-neighbour upscaling on pixel-art screenshots while letting box-art covers scale smoothly. Combined image list is reactive, so picking a new cover via the picker updates the lightbox in place.
- `ThumbnailDownloadOrchestrator` (`replay-control-app/src/api/thumbnail_orchestrator.rs`) — single coordinator for all thumbnail-download work with bounded concurrency (`Semaphore::new(10)`), shared dedup across pipelines, priority via two channels + `select! biased` (visible preempts bulk), per-job completion delivery, and `AtomicUsize` in-flight + `AtomicU64` lifetime counters. Wired through the on-demand box-art enrichment path in `library/enrichment.rs::queue_on_demand_download` to fix the production fd-leak: previously every cache-miss did an unbounded `tokio::spawn`, so a fresh-system rescan with thousands of missing thumbnails opened thousands of HTTP sockets simultaneously and burned through the 1024 fd soft limit (993/1012 fds were sockets in beta.5 telemetry). Bulk pre-fetch path keeps its existing local Semaphore for now — wiring it through the orchestrator is a follow-up that requires migrating its `Activity`-state progress callback to the completion-channel model.
- "Rescan Library" button on the metadata page — additive rescan of all systems without touching previously-imported metadata. Surfaces under the same activity-gating as the existing import; the button disables itself when another metadata operation is running so two concurrent rescans can't race the L2 write path.

### Changed

- Tapping the box art on the game-detail page now opens the lightbox instead of the variant picker. The "Change cover ›" link below the cover is now the only entry point to the picker. Cleaner separation of concerns: tap = view, link = swap.
- `metadata_page_snapshot::compute` no longer bundles all 8 stats queries into a single `pool.read` closure. The closure was the right shape on small libraries but became a problem at scale — on the 141k-ROM beta.5 reporter it ran 80–170 s, well past the 15 s `INTERACT_TIMEOUT` tripwire, and held a read-pool slot the whole time so concurrent SSR requests starved behind it. Now 8 small `pool.read` calls fanned out via `tokio::join!`; the pool's 3 read slots overlap them instead of running back-to-back, no individual closure exceeds the cap, and SSR readers can slot in between.
- `bulk_insert_aliases` chunks into 5 000-row transactions instead of one monolithic transaction. The user-supplied beta.5 log showed `library_db: write exceeded 15s` mid-LaunchBox-import on a ~30 k-alias batch; chunking keeps each transaction well under the cap. `INSERT OR REPLACE` is row-idempotent so cross-batch atomicity is not required (a power loss mid-import re-inserts cleanly on the next run).
- `get_recommendations` migrated from a 5-minute `TtlSlot<RecommendationData>` to the existing `SsrSnapshot<T>` pattern (already used by `metadata_page_snapshot`). Strictly better caching: event-driven invalidation via the same write-completion sites that already invalidate the metadata snapshot, single-flight rebuild on miss, stale-on-`None` so the home page keeps rendering during long writes. Cold-case behaviour: see follow-up item under Internal — the planned cold-instant-return is captured as a pending task in the beta.5 NFS investigation doc. New `AppState::invalidate_user_caches()` helper consolidates the parallel `response_cache.invalidate_all()` + `cache.invalidate_recommendations()` calls so they stay in lockstep across ~22 write sites.
- Enrichment setup (`enrich_system_cache`) hoists `visible_filenames` once instead of querying it twice (`auto_match_metadata` was independently re-fetching the same rows — a per-system N+1). `build_image_index` + `auto_match_metadata` + `ArcadeInfoLookup::build` then run in `tokio::join!` so they overlap on the pool's read slots; bails early when the system has no visible filenames.
- LaunchBox import end-of-run summary now logs the *real* metadata-row count (`COUNT(*) FROM game_metadata`-equivalent) — typically 2–3× the matched-ROM count due to regional variants. Previously the line read "0 inserted" because the parser-side counter is always 0 in the bulk-import path (the writer task publishes the real count via an atomic, patched into stats after both tasks join). New format: `LaunchBox import: N source entries, M matched ROMs, K metadata rows inserted, S skipped`. The misleading parser-local log demoted to `debug!` and re-tagged "LaunchBox parse:".

### Fixed

- Catalog schema mismatch when a beta.4 → beta.5 upgrade replaced the binary without refreshing the bundled catalog. `init_catalog` now compares the `arcade_games` column set against `ARCADE_COL_NAMES` at startup (reuses the library's `table_columns_diverge` primitive). On divergence: log one loud journal `ERROR` directing the user to reinstall, set the `CATALOG_SCHEMA_OUTDATED` flag, and short-circuit `with_catalog` so subsequent arcade lookups return `None` instead of spamming `no such column: source` per row. Surfaced in the SPA via the new `<AssetHealthBanner>` (`api::AssetHealthIssue` + `ConfigEvent::AssetHealthChanged` + `replay-control-core::asset_health`) so the user sees the banner immediately on page load.
- Drop the 15 s `STORAGE_READY_TIMEOUT` from `wait_until_mount_point` (renamed to `is_ready` — now a one-shot bool check). `prepare_storage_dbs` no longer fails startup on slow NFS first-mount; instead, the detect site routes the not-ready case into the existing no-storage path (which already redirects every request to `/waiting`). The background re-detection loop picks up the mount when it appears. Beta.5 NFS users were hitting "Storage not ready: did not become a mount point within 15s" → service exit → `Restart=on-failure` cycle; the new model keeps the service up indefinitely with a clear UI signal until the mount surfaces. `refresh_storage` gets the same gate so a transient mount-not-ready blip doesn't tear down a working storage state.
- Two stale slugs in the libretro-thumbnails repo mapping: `Atari - 7800 ProSystem` was renamed upstream to `Atari - 7800` (the old slug 404s); `Philips - CDi` was renamed to `Philips - CD-i` (added a hyphen between CD and i). Both 404s appeared in beta.5 telemetry. Stopgap fixes in the hardcoded `mod.rs` table; the real fix is catalog-build-time slug resolution from the live GitHub org listing — separate design pass.
- `build.rs` now emits `cargo:rerun-if-changed=../.git/HEAD` and `../.git/index` so the embedded `GIT_HASH` doesn't go stale on incremental builds. `/api/version` was reporting the *previous* commit's hash after a deploy of new code (the binary itself was always correct; only the displayed string lied).
- NeoGeo AES and MVS systems now route through the arcade metadata path. They were previously treated as console systems, so MAME / FBNeo curated names didn't apply and game listings showed raw ROM filenames.
- `ThumbnailDownloadOrchestrator::submit_visible` / `submit_bulk` no longer leak a dedup-set entry when the calling future is cancelled mid-await. New RAII `ClaimGuard` rolls back the claim on drop unless `disarm()` is called after a successful send. Without the guard, a cancelled submit between `try_claim` and `send().await` would leave the key in the pending set forever, silently dedup-skipping every subsequent submit for that thumbnail.

### Internal

- Per-connection WAL-fallback log line in `sqlite.rs::open_connection` demoted from `info!` to `debug!`. The "filesystem does not support WAL, using DELETE journal" message was firing on every connection open against an exFAT / NFS DB — 4× per startup with no actionable content. The two real fallback paths (`open_wal` failed, `open_nolock` failed after FS reportedly unsupported WAL) keep `info!` since those indicate something unusual.
- `db_pool::dispatch` now logs every error path explicitly (`Corrupt`, `Busy`, `Closed`, `RwLock-poisoned`, deadpool acquire failure). Five of seven `DbError` variants were silent — investigations of "`pool.read` returned `None`" had to guess at the cause. Now: `debug!` for transient/expected states (closed during shutdown, gate during DELETE-mode writes, corrupt while recovery runs), `warn!` for connection-acquire failures, `error!` for poisoned RwLock and the existing 15 s timeout.
- `dev.sh` seeds `RUST_LOG=info,replay_control_app=debug,replay_control_core=debug,replay_control_core_server=debug` for dev bootstraps so the new diagnostic logs surface in dev-Pi logs immediately. `install.sh` keeps the `info`-only default for shipped installs.
- Code-review pass on the beta.6 cycle: extracted `LibraryDb::update_box_art_url` helper (deduped 3 raw SQL sites in `boxart.rs` and `enrichment.rs`); collapsed `submit_visible` / `submit_bulk` enqueue logic into a shared helper; collapsed `Outcome::DownloadFailed | SaveFailed` arms in the on-demand on-complete hook; dropped a dead `_count` parameter from `get_recommendations`; trimmed change-history narration from several files. No behaviour change.
- E2E suite fixes for CI failures introduced by the beta.5 path move + an `ls` / `test -f` bug in the Pi storage fixture. `tests/integration/run.sh` and the affected Playwright cases now exercise the post-storage-id paths correctly.
- `cargo clippy` cleanup pass on `asset_health_banner` and `recommendations` server-fn signatures.

---

## [0.4.0-beta.5](https://github.com/lapastillaroja/replay-control/releases/tag/v0.4.0-beta.5) - 2026-04-30

### Highlights

- The `/settings/metadata` page no longer hangs on rapid force-refresh and stays interactive throughout long-running imports and thumbnail updates. The fix is structural — a single in-memory page snapshot replaces six fan-out server fns, with single-flight rebuild and stale-on-`None` fallback.
- Page transitions feel snappier across the board on Pi 4 / USB+exFAT. The response-level cache TTL is 5 minutes (was 10 s), so the recommendations / favorites carousels stay warm across navigation pauses instead of paying a 100–300 ms recompute on the next click.
- A stale-NFS race that occasionally wiped the cached system metadata (and made the library look empty until a manual recovery) is fixed at four layers — readiness check, scan-error signalling, SQL-level zero-overwrite guard, application-level warning.
- Thumbnail update finally emits structured logs (`Manifest import: starting / complete`, `Thumbnail update done: …`) instead of being silent. GitHub API rate-limit responses are detected and surfaced once with a "configure GitHub API key" hint.
- Auto-update now downloads and swaps the bundled `catalog.sqlite` atomically alongside the binary and site assets, with a clean rollback path if the swap fails.
- Arcade ROM names and metadata now respect each system's upstream curation. `arcade_fbneo` shows FBNeo's `"Galaga '88"`; `arcade_mame` shows MAME's name as-is for the same ROM. Cross-source field merge fills gaps too — e.g. on `arcade_fbneo` an FBNeo row with no rotation tag falls back to MAME's `vertical`. Resolution order per system: see `arcade_source_priority` in `replay-control-core/src/platform/systems.rs`.
- The library database now lives centrally on the host SD card at `/var/lib/replay-control/storages/<storage-id>/library.db`. Each ROM storage gets a stable id derived from its filesystem UUID, so re-plugging a USB after a reboot keeps every cached row — no rescan, no rematch, no enrichment delay. User overrides and saved videos still travel with the storage on `<storage>/.replay-control/user_data.db`. One-shot migration on first attach for users coming from beta.4.
- A "library shows 0 games" regression caused by per-connection WAL recovery unlinking sidecars under live connections is fixed at four layers — recovery is now scoped to pool open, lifecycle ops drain before unlinking, the write gate is mode-aware so WAL pools never block readers, and `try_read`/`try_write` return typed errors so cascade gates can no longer mistake "pool busy" for "library is empty".

### Added

- `MetadataPageSnapshot` — in-memory single-flight cache of the `/settings/metadata` payload. Six per-stat server fns (`get_metadata_stats`, `get_system_coverage`, `get_thumbnail_data_source`, `get_image_stats`, `get_builtin_db_stats`, `get_library_summary`) collapse to one `get_metadata_page_snapshot`. The compute path runs all DB queries in one `pool.read` closure (single pool acquisition, single cancellation-orphan slot if the SSR future is dropped); off-pool work (cached_systems, builtin_stats, media_dir_size) follows after the connection releases. Pre-warmed at boot in `run_pipeline`; invalidated at every existing write-completion site.
- Generic `SsrSnapshot<T>` helper in `replay-control-app/src/api/library/ssr_snapshot.rs`. Future SSR pages that want "compute once per write cycle, share across concurrent requests, fall back to stale on transient unavailability" can opt in with one field declaration and one accessor. Backed by `RwLock<Option<T>>` + double-check inside the write lock; stale-on-`None` rule preserves the previous value when the builder returns `None` (DB transiently unavailable). Drives `metadata_page_snapshot` directly.
- 15-second `tokio::time::timeout` cap on every `conn.interact()` closure in `db_pool.rs`. The closure can't be cancelled (it's a `spawn_blocking` task and Tokio's blocking-pool work isn't cancellable on `JoinHandle` drop), but the awaiting caller bails with `Err(DbError::Timeout)` instead of hanging, and the offending site is surfaced via a loud `tracing::error`. Defense-in-depth against any future code path that re-introduces a slow closure.
- `PoolMetrics` atomic counters on `DbPool`: `reads_started/completed/returned_none/timed_out`, `writes_started/completed/timed_out`, `gate_blocked_reads`. Snapshot is `Serialize`/`Deserialize`. Cheap (single atomic load to read), wires straight into a future `/debug/pool` HTTP endpoint when needed.
- `DbError` typed errors on `DbPool::try_read` / `try_write`: `Closed`, `Corrupt`, `Busy`, `Timeout`, `Sql`, `Acquire`, `Interact`, `Other`. Replaces the `Option<R>` "anything went wrong = None" idiom that caused the visible "library shows 0 games" regression — cascade gates that read a row to decide "is the library empty?" can now distinguish *pool unavailable* (skip, retry later) from *query ran and returned no rows* (genuine empty state). Legacy `read()`/`write()` adapters remain as `try_*().ok()` for sites where best-effort is genuinely correct.
- `DbPool::reset_to_empty()` and `replace_with_file(src)` — the supported "clear and rebuild" / "restore from backup" entry points. Drain in-flight `Object`s before unlinking; abort the operation (returning `false`) if drain times out, so a stuck closure can't hold an fd into a deleted inode while a new pool opens at the same path. Both are atomic in the order: drain → unlink sidecars → mutate → reopen.
- Storage id (`<kind>-<8 hex>`, e.g. `usb-9a3a700d`) — derived deterministically from the filesystem identifier (volume UUID for block devices, `server:/share` for NFS) via CRC32. Self-healing if the marker file is lost (regenerates the same id). Random fallback only when no FS identifier is obtainable (tmpfs, exotic mounts). Kind tag (`usb` / `sd` / `nvme` / `nfs`) lets a glance at `/var/lib/replay-control/storages/` tell what each entry corresponds to. New `replay-control-core-server/src/storage_id.rs` and `data_dir.rs` modules.
- `--data-dir` CLI flag on `replay-control-app` for parking library DBs somewhere other than `/var/lib/replay-control` (NVMe, alternate mount). Default unchanged on Pi.
- `LibraryDb::SCHEMA_VERSION` + `run_migrations` framework: numbered, additive migrations (`ADD COLUMN`, `CREATE INDEX`, `UPDATE … WHERE …`) that preserve user-populated tables across schema bumps. Sits alongside the existing column-set-diff drop path that's still used for the four rebuildable derived tables (`game_library`, `game_library_meta`, `game_metadata`, `game_release_date`); migrations are the future-facing path for any table whose content shouldn't be flushed.
- Property tests on `DbPool`: `concurrent_writes_visible_to_all_readers` (forces lazy connection creation, asserts every reader observes every commit — the test that would have caught the WAL-unlink regression), `reset_to_empty_blocks_until_drain`, `crash_recovery_simulation`, `gate_blocked_read_returns_typed_error`, `closed_pool_try_read_returns_typed_error`, `corrupt_pool_try_read_returns_typed_error`, `wal_writes_do_not_block_concurrent_reads`. Plus `rebuild_corrupt_library_wipes_table_content` integration test that asserts a sentinel row inserted before `mark_corrupt` is gone after the rebuild — proves the lifecycle actually drains and unlinks rather than just flipping the flag.
- `ScanError` enum on `replay_control_core_server::roms::scan_systems`: `RomsDirUnreadable` and `AllSystemsMissing` distinguish "filesystem not yet ready" from "user genuinely has no ROMs". New `wait_for_storage_ready(roms_dir, timeout)` polls `read_dir` with backoff; called from `run_pipeline` before any scan. Defends against the NFS / autofs / USB-hot-plug race where the storage root resolves before subdirectories surface.
- `LibraryDb::save_system_meta` now refuses at SQL level to lower a non-zero `rom_count` to zero on UPDATE. Returns the post-write count so callers can detect and log when the guard fired. INSERTs into a fresh row are unaffected.
- Auto-update downloads, extracts, swaps, and rolls back `catalog.sqlite` alongside the binary and site assets. New `backup` / `swap` / `unbak` / `restore` shell helpers in `generate_update_script` keep the three swaps atomic with a single rollback path. Releases without a catalog asset (< v0.4.0-beta.3) skip the catalog step cleanly via an empty `CATALOG_SRC`.
- `install.sh --purge` wipes all on-Pi data (catalog, settings, env file, cached LaunchBox XML) for clean reinstalls.
- `install.sh --pi-pass` flag handles non-default RePlayOS SSH passwords from the curl-piped one-liner.
- `dev.sh` bootstraps the systemd unit + `/etc/default/replay-control` env file on the Pi when missing — mirrors what `install.sh` emits, runs as a no-op when the unit already exists.
- ReplayOS custom user skins (slots 11+) appear in the selector as a disabled `CUSTOM #N` entry instead of being invisible. Active-skin badge subscribes to the live `current_skin` signal so changes from the Pi reflect immediately.
- 22-case e2e test suite (`tests/e2e/test_page_health.py`) covering route-content health, navigation budgets, force-refresh resilience, server-fn registration. Plus a 3-case `tests/e2e/test_response_cache.py` that pins `RESPONSE_TTL` ≥ ~30 s. Integration suite (`tests/integration/run.sh`) fixed: the `/system/<x>` route assertions were checking the leptos 404 fallback (status-only check missed the regression) — now anchored on `/games/<x>` by content.

### Changed

- `arcade_games` catalog table restructured to row-per-source (PK `(rom_name, source)`). Replaces the previous one-row-per-rom schema where MAME current's last-write overrode every field, losing FBNeo's curated names like `"Galaga '88"`. The runtime `lookup_arcade_game(system, rom)` merges fields by per-system priority (`replay_control_core::systems::arcade_source_priority`); MAME's name wins on `arcade_mame`, FBNeo's on `arcade_fbneo`, with field-level fallback (e.g. FBNeo lacks rotation → falls through to MAME). `arcade_release_dates` gets per-source attribution as a side benefit. Catalog file 12.5 MB → ~14.8 MB; 27,272 rows for 15,439 distinct ROMs. PK index covers `WHERE rom_name = ?` via leading-prefix scan — no extra index needed.
- `WriteGate` is now pool-private (`pub(crate)`) and only auto-activates on DELETE-mode pools (exFAT/NFS user_data). On WAL pools (the library on the host SD) it is *never* set — SQLite's MVCC means writers don't conflict with readers, so gating is pure overhead and was actively harmful: the previous always-gate behavior caused the destructive `is_empty` cascades that wiped `box_art_url` after a thumbnail update. The gate is held only across a single `try_write` call; long write sequences (LaunchBox import, thumbnail manifest sync, populate_all_systems) drop the gate between batches, so SSR readers stay responsive throughout. `pool.read_through_gate` API and the public `WriteGate::activate(pool.write_gate_flag())` pattern are gone.
- `cache.invalidate(&db)` and `invalidate_system(system, &db)` return `Result<(), DbError>` instead of swallowing the L2 clear's failure. Destructive callers (`rebuild_game_library`) propagate so a no-op clear-then-rebuild can't silently write new rows over old ones (the same hazard pattern as the WAL-unlink regression). Cache-clearing afterthoughts on already-successful writes log at `debug` and continue.
- `import.rs::regenerate_metadata` and the three corruption-recovery server fns (`rebuild_corrupt_library`, `repair_corrupt_user_data`, `restore_user_data_backup`) migrated to the new lifecycle primitives. The previous `pool.close(); delete_db_files(); pool.reopen()` choreography is replaced by `pool.reset_to_empty()` / `pool.replace_with_file(backup)` — single atomic transitions, drain-aware, with `delete_db_files` now `pub(crate)` so future callers can't reintroduce the unlink-while-open hazard.
- `refresh_storage` and `AppState::new` now share `prepare_storage_dbs` (storage readiness, id assignment, library migration, path resolution) and `reopen_user_data_or_mark_corrupt` (header pre-flight). Adding a new pre-attach step now happens in one place — the previous parallel inline blocks drifted, which is how the storage-swap path missed the bad-header pre-flight that `AppState::new` had at startup.
- Library DB read-pool size: 3 connections (was effectively 1 across the pre-redesign + brief `read_bg` slot). WAL on ext4 SD lets concurrent reads actually parallelise; 3 covers SSR fan-out (recommendations + recents + favorites + system info) overlapping with one long enrichment / thumbnail-planning pass without queueing. User_data pool stays at 1 reader (DELETE-mode pool, the gate serialises against writers anyway).
- `cached_systems` now distinguishes three outcomes from `load_systems_from_db`: `Some(non-empty)` (cache hit), `Some(empty)` (DB reachable, no systems cached → fall through to filesystem scan), and `None` (DB transiently unavailable → return empty without caching, retry on next call). Avoids triggering an expensive multi-thousand-ROM scan on every transient pool unavailability.
- `cached_systems` no longer caches a poisoned result from a racy `scan_systems`. When the new `ScanError` fires, the L1 cache is left empty so the next caller retries once storage settles.
- Read-connection page cache bumped from 500 pages (~2 MB) to 1 000 pages (~4 MB). Recommendations / system-coverage / metadata-snapshot rebuild queries scan tens of thousands of rows; the bigger cache keeps hot indexes resident across calls. Write connection unchanged at 500 pages.
- `RESPONSE_TTL` in `api/response_cache.rs` raised from 10 s to 5 min. Every navigation pause longer than the old TTL paid a 100–300 ms recompute on Pi 4 / USB+exFAT, which surfaced as "stale browser load" on the next click. All write paths that *could* invalidate (favorites toggle, library invalidate, image clear, post-import cache invalidate) already call `response_cache.invalidate_all()`; the TTL is an upper bound when no write happens.
- `fetch_repo_tree` and `check_repo_freshness` route through a new `gh_api_get` helper that inspects status code + `X-RateLimit-*` headers. A 403 with `X-RateLimit-Remaining: 0` returns a structured `GhResponse::RateLimited { reset_unix, message }` instead of being mashed into an opaque error. `import_all_manifests` bails on the first rate-limit response (every subsequent request would hit the same wall) and emits a single user-actionable warning.
- Thumbnail update progress label format: `System · Boxarts 42% · 12 new, 87 cached` instead of `System: 7/15`. Banner uses `: ` separator instead of parentheses.
- Setup checklist surfaces an immediate "pending" state on click; the flag clears once SSE confirms the matching activity has started, avoiding the "did my click register?" flash.
- LaunchBox import is now pipelined: the sync XML parser sends parsed records over a channel to an async writer that drains them onto `pool.write` in batches. `replay-control-core-server` no longer needs `Handle::block_on` to bridge the two halves. ~40% faster on WAL-mode storage.
- `install.sh` no longer downloads the 489 MB LaunchBox metadata XML at install time. Fetch on demand from Settings → Download metadata when you need it; the catalog ships embedded with the binary.

### Fixed

- The user-reported `/settings/metadata` "second force-refresh hangs" pattern. Root cause was `deadpool-sync::SyncWrapper::interact()` running closures on a `spawn_blocking` task that doesn't cancel when the awaiting future drops — a force-refresh left an orphan closure holding the `SyncWrapper`'s inner mutex, blocking every subsequent `interact()` until it finished. Six per-page server fns multiplied the orphan count. Fixed structurally by collapsing to one acquisition per page and adding the 15 s wall-clock cap on `interact()`.
- The user-reported NFS startup race that wiped `game_library_meta` (every system reset to `rom_count = 0` after a reboot when NFS subdirectories hadn't materialised yet). Repro: `/media/nfs/roms` resolves but per-system folders are not yet listable; every `system_dir.exists()` returns false; `scan_systems` returned 41 zero-count summaries; `save_systems_to_db` UPSERTed all zeros over the previous boot's correct counts. Defends at four layers: `wait_for_storage_ready` at startup, `ScanError` signalling from `scan_systems`, SQL-level zero-overwrite guard in `save_system_meta`, application-level warning when the SQL guard fires.
- Thumbnail update used to be silent in `journalctl` — no INFO logs at all, only per-system warnings. A failed update (commonly: GitHub API rate limit on the unauthenticated 60 req/h cap, with the pipeline making ~70 calls per full run) left no diagnostic trace. Now logs entry / per-phase / completion lines and surfaces rate-limit failures with a "configure GitHub API key" hint.
- `manifest_import` no longer holds the `WriteGate` across the multi-minute GitHub HTTP loop. The gate is acquired per-batch inside `pool.write`, so SSR `pool.read()` calls succeed in the gaps between batches.
- `/system/<x>` style routes used by an earlier integration test never actually existed — they fell through to the leptos `Page not found` fallback while still returning HTTP 200. The integration suite has been corrected to assert `/games/<x>` and to anchor real-route detection by content (`Hide Hacks`, `All Genres`) instead of status alone.
- Settings page surfaces update-channel save errors instead of swallowing them.
- "Library shows 0 games" regression seen on a long-running Pi after rebuild or thumbnail-update operations. Root cause was `sqlite::recover_stale_wal` running unconditionally inside per-connection `open_connection`: a second concurrent connection in the same process triggered recovery, which checkpointed + switched journal mode to DELETE + unlinked `-wal`/`-shm`. The first connection kept its file descriptors but those inodes were now orphaned, so its reads saw only the pre-WAL state — i.e. an empty `game_library` if recent writes hadn't been checkpointed yet. Matching `/proc/<pid>/fd/` showed three live readers against the same main inode but only one with an intact WAL fd. Fixed by scoping recovery to a single one-shot call inside `DbPool::new` / `DbPool::reopen` (renamed `recover_after_unclean_shutdown`), and by making `delete_db_files` `pub(crate)` so the only public path to the WAL files is the drain-first lifecycle.
- Destructive `is_empty` cascade in `spawn_cache_enrichment` / `spawn_rebuild_enrichment` / `phase_cache_verification`. The previous `library_pool.read(...).await.unwrap_or(true)` pattern conflated "pool busy" with "library is empty", silently triggering full populate-from-filesystem (which DELETE+INSERTs `game_library` with no `box_art_url`) every time a cache-clear write happened to be in flight. Migrated to `try_read` + match: pool unavailability is "skip, retry later", never "library is empty".
- Schema rebuild on column-set diff for the four rebuildable derived tables (`game_library`, `game_library_meta`, `game_metadata`, `game_release_date`) was momentarily replaced with a `WARN` log during refactor, which would have left users with a broken DB on the next schema bump until a numbered migration shipped. Restored to drop-and-recreate; the new `run_migrations` framework is the additive path for tables that should *not* be dropped.
- `refresh_storage` (storage swap at runtime) now runs the same `has_invalid_sqlite_header` pre-flight that `AppState::new` runs at startup. A re-attached USB whose `user_data.db` got clobbered while the Pi was off no longer leaves the pool silently closed — the corruption banner fires and Recovery / Reset is one click away.

### Other

- New tests across the cycle (1 100+ pass total, 18 in `db_pool` alone, 9 in `corruption_tests`):
  - 5 `core-server` unit tests for `scan_systems` paths (`RomsDirUnreadable`, `AllSystemsMissing`, populated, empty-but-readable, missing) and `wait_for_storage_ready`.
  - 5 SQL-level zero-overwrite-guard tests on `save_system_meta`.
  - 3 `ManifestImportStats` serde / back-compat / rate-limit-flag tests.
  - 4 `SsrSnapshot<T>` unit tests including the 10-racer single-flight coalescing test.
  - 7 new `DbPool` property tests covering the WAL-unlink regression (`concurrent_writes_visible_to_all_readers`, `reset_to_empty_blocks_until_drain`, `crash_recovery_simulation`, `gate_blocked_read_returns_typed_error`, `closed_pool_try_read_returns_typed_error`, `corrupt_pool_try_read_returns_typed_error`, `wal_writes_do_not_block_concurrent_reads`).
  - `rebuild_corrupt_library_wipes_table_content` integration test asserting a sentinel row inserted before `mark_corrupt` is gone after the rebuild — content-survival check that catches refactors which would no-op the file wipe and just flip the flag.
  - 11 storage-id unit tests (deterministic derivation from FS UUID, NFS shape, kind-hex format validation, parse round-trip, generate-collision sanity).
  - 22 e2e cases in `test_page_health.py` (route content health, navigation budgets, force-refresh resilience, server-fn registration).
  - 3 e2e cases in `test_response_cache.py` pinning the new `RESPONSE_TTL`.
- Live validated against a Pi 4 + USB+exFAT (DELETE journal, no WAL): `/favorites` after a 12 s pause went from 112 ms (curl) / 173 ms (Playwright SPA navigation) to 28 ms / 77 ms after the response-cache TTL change. WAL-unlink fix verified on a Pi 5 + ext4 SD by repeatedly triggering Rebuild + Update Thumbnails — `get_roms_page nintendo_snes` returns 7 231 / `get_library_summary` returns 23 666 / 21 systems with zero `(deleted)` `library.db-wal` fds throughout.
- Architecture docs (`docs/architecture/connection-pooling.md`, `design-decisions.md`, `database-schema.md`, `technical-foundation.md`) updated to current state.
- WAL-unlink regression analysis in `replay-control-private/investigations/2026-05-01-library-wal-unlink-under-live-connections.md` (the seven independent data-loss vectors and the safety-by-design redesign that closes them). Pool design / cancellation-orphan analysis in `2026-04-29-pool-design-findings.md`. NFS race investigation in `2026-04-29-nfs-startup-race-and-thumbnail-silent-failure.md`. SSR-cache-snapshot proposal in `2026-04-29-ssr-cache-snapshot-vs-pool-starvation.md`.

---

## [0.4.0-beta.4](https://github.com/lapastillaroja/replay-control/releases/tag/v0.4.0-beta.4) - 2026-04-25

### Highlights

- A torn-write or clobbered library database no longer crash-loops the service. Rebuildable caches recover silently on the next start; if your saved overrides and videos are affected, a banner appears with a one-click **Reset** (renamed from Repair).
- Auto-update no longer leaves browsers stuck in a reload loop. After the service restarts, open tabs cleanly pick up the new version on their own.
- Corruption banners now appear instantly instead of after a few seconds of polling, and stale browser tabs reconnect on their own after a server restart.
- Smaller fixes: the captures lightbox no longer crashes when navigating away mid-keypress.

### Added

- Corruption status now pushes over `/sse/config` instead of being polled. Pool-flag transitions broadcast on the existing config stream (init payload + push events); banners read from context `RwSignal`s fed by `SseConfigListener` and a new `SseActivityListener`. The `get_corruption_status` server fn is removed. A new `sqlite::has_invalid_sqlite_header` pre-flight survives torn-write magic-header damage so a clobbered DB no longer crash-loops the service via systemd: `LibraryDb::open` silently delete-recreates (rebuildable cache, no banner), and `user_data` wires through new `DbPool::new_corrupt` so the recovery banner appears via the SSE init payload. `check_for_corruption` now also flags `SQLITE_NOTADB (26)` alongside `SQLITE_CORRUPT (11)`. The user-data "Repair" button is renamed to "Reset".
- Content-hashed WASM and JS asset filenames break the browser cache cleanly across server restarts. `LeptosOptions` sets `hash_files` and reads `hash.txt` from the resolved site root; `build.sh` and `dev.sh` hash the bundle, write `hash.txt`, and rewrite the wasm import inside the JS so wasm-bindgen still resolves. `/static/pkg` now sends `Cache-Control: immutable` since URLs are versioned. Fixes an update-reload loop where the cached pre-restart WASM hydrated with the old `VERSION`, the SSE init reported a mismatch, `location.reload()` re-fetched the same cache, and the loop repeated.
- `build.sh` gains `SKIP_DATA=1` to skip catalog rebuilds for fast iterative WASM-only test builds.

### Changed

- Renamed the on-storage `metadata.db` to `library.db` and folded the grab-bag `metadata::` module into the existing `library::` module across both `replay-control-core` and `replay-control-core-server`. The old module name was a holdover from before the catalog migration — with `catalog.sqlite` now owning embedded reference data, `library.db` clearly names the user's on-storage rebuildable DB.
- Reorganized the former `metadata::` grab-bag into purpose-scoped submodules: `library/db/` (SQLite), `library/imports/` (LaunchBox XML), `library/matching/` (pure alias + metadata matching), `library/thumbnails/` (manifest, fuzzy match, resolution), `library/manuals/` (game docs + retrokit). Hoisted `user_data_db` to its own top-level `user_data/` module (persistent user data is semantically distinct from rebuildable library data). Moved shared SQLite helpers from `metadata/db_common.rs` to top-level `src/sqlite.rs`.
- Renamed the `metadata` cargo feature on `replay-control-core-server` to `library`. Renamed the `metadata_report` bin to `library_report` (`cargo run --bin library_report --features library`). Server fns `rebuild_corrupt_metadata` / `metadata_corrupt` become `rebuild_corrupt_library` / `library_corrupt`; the recovery banner copy reads "Library database is corrupt".
- User-facing "metadata" vocabulary is preserved where it describes external-enrichment sources (the `/settings/metadata` page, the `Game Metadata` i18n label, the `game_metadata` SQL table, the `download_metadata` / `clear_metadata` / `get_metadata_stats` server functions, and `launchbox-metadata.xml`). Only the container DB file and module changed names.
- `DbPool`, `SqliteManager`, and `WriteGate` move from `replay-control-app/src/api/mod.rs` to `replay-control-core-server/src/db_pool.rs`. The types had no app-specific coupling — they just wrap `deadpool-sqlite` around `core-server::sqlite::open_connection` — so SSR consumers now see a single crate for pool + open helpers. App's `api/mod.rs` re-exports `DbPool` / `WriteGate` / `rusqlite` so existing imports keep resolving; `deadpool-*` deps drop out of `replay-control-app`.
- Native I/O for the update system (GitHub release polling, asset download, `available.json` handling) moves from `BackgroundManager` in the app crate into `replay-control-core-server::update` (gated behind the `http` feature). `BackgroundManager` keeps the `AppState` / `Activity` / `systemctl`-coupled orchestration (`update_check_loop`, `start_update*`, `generate_update_script`, etc.). `check_github_update` and `resolve_asset_urls` now take `repo: &str` instead of reading a const.
- Native I/O for the LaunchBox import and thumbnail pipelines extracted into three pure core-server fns: `launchbox::run_bulk_import` (sync XML importer wrapped in `spawn_blocking` with the `Handle::block_on` → `pool.write` bridge for batched flushes), `launchbox::import_launchbox_aliases`, and `thumbnails::update_image_paths_from_disk`. App-side `ImportPipeline::run_import` loses ~50 lines of boilerplate and just wires per-batch ticks into Activity; pipeline ownership stays on the AppState side of the boundary.
- The Axum upload handler delegates filesystem writes to a new `replay_control_core_server::roms::write_rom`, which also creates the system directory if missing. No behavior change.
- `dev.sh` drops the unused `--watch` flag (the Pi auto-redeploy-on-save mode was unused — removed CLI arg, `cargo-watch` loop, and the inline build recipe inside it).

### Fixed

- `CapturesLightbox` keydown listener no longer panics when the page unmounts before the listener detaches. The handler now uses `try_get` on the parent's `current_index` signal so it bails silently on a disposed `RwSignal` instead of unwrapping.
- `setup_checklist` no longer logs a reactive-graph warning on every hydrate. The `query.read().get_str("setup")` call ran in the component body, eagerly reading an `ArcMemo<ParamsMap>` with no tracking context established yet; moved into the `Resource::new` source closure so the read happens inside a tracked context (and the resource now also re-runs if `?setup` is added or removed).
- `SseConfigListener` reconnects after a server restart. The `onerror` handler called `es.close()`, canceling `EventSource`'s built-in retry — stale tabs open during an auto-update therefore never received the fresh init payload that triggers the version-mismatch reload, and silently kept running the previous WASM. Dropped the `onerror` handler so the browser's default ~3s retry kicks in.

### Migration

- Legacy `metadata.db`, `metadata.db-wal`, `metadata.db-shm`, and `metadata.db-journal` files are removed on first boot via an idempotent `cleanup_legacy_metadata_db` step inside `LibraryDb::open`. No data migration is needed: the startup pipeline re-scans ROMs into the new `library.db`, re-imports LaunchBox data from `launchbox-metadata.xml` (Phase 1), and rebuilds the thumbnail index from disk (Phase 3). User overrides and saved videos (`user_data.db`) are untouched.
- `sqlite::delete_db_files` extended to cover `.db-journal`, closing a stale-sidecar gap in the four existing corruption-recovery callers.

### Other

- 7-test lifecycle suite for `DbPool` (read/write roundtrip, closed-pool returns `None`, close-then-read, reopen after close, `mark_corrupt` closing the pool, `WriteGate` RAII guard blocking reads — the last is the exFAT data-corruption guard that justifies the type's existence). Adds tests for `launchbox::run_bulk_import` (covers the `spawn_blocking` + `Handle::block_on` async bridge), `launchbox::import_launchbox_aliases`, `thumbnails::update_image_paths_from_disk`, and `roms::write_rom`. Update tests relocate from app to core-server and switch from an in-process axum listener to mockito; 16/16 green via `cargo test --features http -p replay-control-core-server`.
- Shared `test_utils` pub module in `core-server` (`build_library_pool`, `insert_game_library_row`) avoids fixture duplication across launchbox and thumbnails test modules. No feature flag — workspace-internal helpers compile in unconditionally and are LTO-dropped from release binaries. `tempfile` moves from dev-dep to regular dep.
- 8 Rust integration tests, 6 Rust unit tests, and 4 Playwright e2e tests covering the SSE corruption broadcast, recovery server fns, idempotent `mark_corrupt`, clobbered-header startup, the live browser SSE wire, and the library no-crash-loop path. `conftest` switches from `sshpass` to `SSH_ASKPASS` to drop the system dep.
- `bench.sh` discovers the hashed wasm URL from the served HTML so it tracks whatever the deploy is actually serving; pipefail-safe when the server is on a pre-hash build (falls back with a warning). `mock_github.py`'s fake site tarball ships hashed filenames + `hash.txt` so the post-update server can serve the hydration scripts in container/e2e auto-update tests.
- Architecture docs (`connection-pooling.md`, `technical-foundation.md`, `design-decisions.md`, `enrichment.md`, `server-functions.md`) updated for the core-server extraction: `DbPool` / `SqliteManager` / `WriteGate`, update I/O, `run_bulk_import`, `write_rom`, and `update_image_paths_from_disk` now point at `replay-control-core-server`. Stale `api/cache/*` paths swept after the metadata→library rename.
- `DbPool::new` no longer warms deadpool connections via `block_in_place` + `Handle::block_on`. The sync `sqlite::open_connection` warmup already validates the file (used to detect journal mode); the deadpool warmup it then ran caught no error the sync warmup didn't, since `Manager::create` only adds trivial role pragmas (`cache_size`, `query_only`, `wal_autocheckpoint`). Connections now create lazily on first `pool.get()`. The `block_in_place` pattern requires multi-thread runtime and interacts pathologically with thread oversubscription on small CI runners — corruption_tests had to be marked `#[serial]` to dodge a CI hang triggered by it. With the pattern gone, `#[serial]` and the `serial_test` dev-dep are removed; the 8 corruption tests run in parallel again.
- `thumbnails::manifest::import_all_manifests` takes `&DbPool` instead of `&mut Connection`. The thumbnail-pipeline caller drops the `pool.write(|db| Handle::current().block_on(...))` bridge, and the write connection is now only checked out for each repo's SQL trio (source upsert + entries insert + count patch, still atomic in one tx) rather than held across the per-repo GitHub HTTP fetches. Same on-disk behaviour, lower deadpool occupancy.
- `release-plz` config fix.

---

## [0.4.0-beta.3](https://github.com/lapastillaroja/replay-control/releases/tag/v0.4.0-beta.3) - 2026-04-23

### Highlights

- Snappier homepage, log viewer, and game launches under load. Subprocess and database calls that used to block for 1–2 seconds at a time now run asynchronously, and the new connection pool more than doubles homepage throughput.
- Arcade box art now matches games with apostrophes in their names (e.g. "Galaga '88") instead of falling back to a placeholder.
- A failed game launch no longer leaves behind a stale autostart trigger that could fire on the next boot.

### Added

- Async catalog connection pool via `deadpool-sqlite` replaces the single `OnceLock<Mutex<Connection>>` that serialized lookups under load. Adds `prepare_cached` on every hot path, tuned pragmas (`mmap_size=64MiB`, `cache_size=8MiB`, `temp_store=MEMORY`), and batch APIs for the N+1 sites in `favorites`, `related`, `scan_pipeline`, and `search`. Homepage c=10 throughput: **113 → 265 req/s** vs v0.3.0. See `docs/features/benchmarks.md`.
- New workspace crate `replay-control-core-server` holds all native (linux) server-side code — SQLite, filesystem, HTTP, process spawning, XML parsing. `replay-control-core` is now pure and compiles for both native and `wasm32-unknown-unknown`, eliminating all 89 `#[cfg(target_arch = "wasm32")]` attributes that previously stubbed DB/fs/HTTP on WASM.
- `tools/pi-memory.sh` reads `VmRSS` / `VmHWM` / `RssAnon` / `free -m` from the Pi over SSH. `--restart` for a clean idle baseline, `--wait N` for settle time, `--json` for machine-readable output.

### Changed

- 17 wire types (`Favorite`, `RomEntry`, `SystemSummary`, `GameRef`, `ImportProgress`, `VideoEntry`, `GameDocument`, …) promoted to `replay-control-core`. The `app/src/types.rs` mirror layer is gone; adding a field now means editing one definition, not two, and the `#[cfg(feature = "ssr")] pub use` / `#[cfg(not(ssr))]` switches in `server_fns/*.rs` collapse to unconditional imports.
- Subprocess and filesystem calls on the async request path (`df`, `ip`, `journalctl`, `tail`, `systemctl restart`, `launch_game`'s autostart writes) migrated to `tokio::process::Command` / `tokio::fs::*`. Previously each blocked the reactor for 1–2s on every homepage, log-viewer, and game-launch request.
- `install.sh` now respects `CARGO_TARGET_DIR` (same behaviour as `build.sh`); `--local` deploys no longer need a `target/ → $CARGO_TARGET_DIR` symlink.

### Fixed

- `build-catalog` fails loudly when input data files are missing or unreadable, instead of producing a degraded catalog that passes tests but loses rows at runtime.
- Arcade box-art matcher handles apostrophes in display names (e.g., "Galaga '88"); regression test locks this in.
- `launch_game` cleans up its autostart marker via `tokio::fs::remove_file` when `systemctl restart` fails, preventing a stale trigger on the next boot.

### Other

- Pool warmup validates the catalog schema at `init_catalog` time and surfaces a clear error if the file is missing or schemaless. Previously a 0-byte `/catalog.sqlite` left at the systemd CWD would silently break every query and show bare filenames in the UI.
- Local `DpSql(DatePrecision)` newtype in `library_db` carries the `rusqlite::ToSql` / `FromSql` impls without violating Rust's orphan rule (`DatePrecision` stays pure in core).
- `docs/architecture/` updated for the 3-crate layout with a new "Crate split" design-decisions entry. `CLAUDE.md` gains a "Crate boundary" rule listing the deps forbidden in core.
- `docs/features/benchmarks.md` refreshed for v0.4.0 (Pi 5, 2GB, ~23K ROMs). Memory section now shows idle / right-after-load / +60s-settled: peak 189 MB, settled 62 MB within a minute (jemalloc returns cleanly).
- CI self-heal: `build-release.yml` now creates a missing release instead of failing when fired from a pushed tag without one.

---

## [0.4.0-beta.2](https://github.com/lapastillaroja/replay-control/releases/tag/v0.4.0-beta.2) - 2026-04-21

### Highlights

- Game detail pages now show the full release date (e.g. "Aug 31, 2000") whenever the data is precise enough, instead of always showing only the year.
- Changing your region preference re-resolves release dates instantly — no library re-import needed.

### Added

- Per-region release dates with precision: new `game_release_date` side table stores ISO 8601 partial dates (`YYYY` / `YYYY-MM` / `YYYY-MM-DD`) per (system, base_title, region). `game_library` gets `release_date`, `release_precision`, and `release_region_used` mirror columns resolved against the user's region preference, with `idx_release_date_chrono` for indexed range scans.
- TGDB emit in `build.rs` folds region_ids into four buckets (Canada → USA, Korea → Japan) and records per-region precision heuristics. Arcade pipeline (MAME/FBNeo/Naomi) extracts year-only rows from driver metadata. LaunchBox enrichment upgrades to day-precision USA dates via `ON CONFLICT … DO UPDATE WHERE precision_rank` improves.
- `DatePrecision` enum (Year/Month/Day) with `serde` + rusqlite `ToSql`/`FromSql`, usable from both SSR and WASM. `format_release_date(&str, Option<DatePrecision>, Locale)` renders the game detail page through i18n month-short keys instead of hardcoded strings.

### Changed

- Region preference and secondary region preference saves now re-resolve the `game_library` release-date mirror columns in-place — no re-import required.
- `SearchFilter` year range migrated from `substr(release_date, 1, 4)` to lexicographic compare (`release_date >= 'YYYY' AND < '(Y+1)'`) with `saturating_add` for `u16` overflow. Hits the chrono index directly.
- Decade list query reads from `release_date` instead of `release_year`, using `substr(release_date, 1, 3) || '0'` to form decade buckets.

### Fixed

- Game detail page now shows a formatted release date (e.g. "Aug 31, 2000") when day or month precision is available, labeled "Released" instead of "Release Year". Previously always rendered as year-only regardless of the available data.

### Other

- Resolver SQL refactored from 9 correlated subqueries to a single `ROW_NUMBER() OVER PARTITION BY` CTE with row-value `UPDATE`.
- `build.rs` shares `title_utils::base_title` via `#[path]` module include instead of a duplicate `compute_base_title_build()`.
- SG-1000 lookup tests now run against the canonical DAT after adding `Sega - SG-1000.dat` to `scripts/download-metadata.sh` outputs.
- Metadata analysis rule added to `AI_CONTEXT.md`: exclude ROM hacks, translations, homebrew, and aftermarket when measuring source coverage.

---

## [0.4.0-beta.1](https://github.com/lapastillaroja/replay-control/releases/tag/v0.4.0-beta.1) - 2026-04-19

### Highlights

- Metadata page redesigned: six summary cards (Total Games, Enrichment, Systems, Co-op, Year Span, Library Size) and a mobile-friendly per-system accordion replace the cramped 7-column table.
- Summary cards now refresh automatically after import, rebuild, thumbnail update, or clear — no full page reload required.

### Added

- Redesigned metadata page: six library summary cards (Total Games, Enrichment, Systems, Co-op, Year Span, Library Size) plus a per-system accordion list replacing the cramped 7-column table. Mobile-friendly with no horizontal scroll; expanded rows show coverage bars, composition ratios, arcade driver status, and a footer with year range / verified / co-op counts.
- Fast-CI path via `REPLAY_BUILD_STUB=1`: `build.rs` reads small committed fixtures under `replay-control-core/fixtures/` instead of ~180MB of upstream arcade/No-Intro/TGDB/Wikidata downloads. Lint and Test CI jobs use stub mode; a new nightly `test-full` workflow runs the same tests against real data.

### Changed

- `game_series` lookups resolve aliases at read time via a `candidates` CTE (self + canonical + aliases + sibling aliases), replacing the `propagate_series_to_aliases` denormalization helper that was dropped entirely. Single source of truth, no duplicate rows, no O(N²) per-system propagation pass.
- Wikidata series extraction uses `en,mul` label fallback instead of `en,ja`: prevents Japanese labels from leaking into series names while still resolving series whose only English-equivalent representation is Wikidata's curated multilingual default (e.g. Kirby → Q2569953).

### Fixed

- Metadata page summary cards refetch after import / rebuild / thumbnail update / clear-metadata; previously stayed stale until a full page reload.
- `Cache::invalidate` now clears `game_series` and `game_alias` in addition to `game_library`, so rows populated by a previous binary's embedded data don't survive a Rebuild Game Library.
- `scripts/wikidata-series-extract.py` reads the Flycast CSV from its new `data/arcade/` location (was pointing at the pre-move path, silently dropping Flycast/Naomi names from `series.json`).

### Other

- Integration tests no longer hang under parallel execution — `TestEnv` RAII helper replaces the manual `close_state` pattern that was causing `DbPool::close` to contend with in-flight test traffic.
- Rust 1.95 clippy fixes (`sort_by_key` + `Reverse`, `checked_div`, feature-gated atomic imports, collapsible match arm).

---

## [0.3.1-beta.3](https://github.com/lapastillaroja/replay-control/releases/tag/v0.3.1-beta.3) - 2026-04-14

### Other

- fix CHANGELOG.md to include proper per-release sections (was causing release-plz to dump full history into release notes)

---

## [0.3.1-beta.2](https://github.com/lapastillaroja/replay-control/releases/tag/v0.3.1-beta.2) - 2026-04-14

### Other

- replace softprops/action-gh-release with `gh release upload` CLI

---

## [0.3.1-beta.1](https://github.com/lapastillaroja/replay-control/releases/tag/v0.3.1-beta.1) - 2026-04-14

### Fixed

- restore settings sidebar highlight on back-navigation — IntersectionObserver deferred via requestAnimationFrame
- use sshpass for automatic SSH authentication in installer, with SSH_ASKPASS fallback
- send required `force` body param in GetSetupStatus integration test
- update E2E and SSR tests for `/more` → `/settings` page rename

### Other

- chain build-release from release-plz via workflow_call to fix missing binary assets
- add workflow_dispatch trigger to build-release for manual builds

---

## [0.3.0](https://github.com/lapastillaroja/replay-control/releases/tag/v0.3.0) - 2026-04-13

### Highlights

- ROMs with non-standard filenames now display the correct canonical name and box art (~1,105 name fixes, ~1,682 thumbnail fixes), thanks to CRC hash matching.
- Redesigned Settings page with a two-pane layout, scroll-spy sidebar, and five sections.
- A first-run setup checklist on the home page now guides you through LaunchBox metadata import and thumbnail indexing.
- LaunchBox metadata downloads automatically at install time (skip with `--no-metadata`).
- Anonymous usage analytics are enabled by default; opt out from Settings > Privacy.

### Added

- CRC hash-matched display names and thumbnails — ROMs with non-standard filenames now show correct canonical name and box art (~1,105 name fixes, ~1,682 thumbnail fixes)
- redesigned settings page with two-pane layout, scroll-spy sidebar, and five sections
- anonymous usage analytics with opt-out from Settings > Privacy
- first-run setup checklist on home page for LaunchBox metadata and thumbnail index
- LaunchBox metadata download at install time (skip with `--no-metadata`)
- JPG image support and improved box art variant picker with filesystem scan
- local Pi install auto-detection and `--version` flag in installer

### Fixed

- Clear Images now correctly removes `box_art_url` references from the database
- settings sidebar highlight on back-navigation
- reactivity warning in play order navigation (sequel/prequel links)
- silent DB errors in library and enrichment operations now logged
- install script env var positioning for piped commands
- desktop settings layout max-width on inline items

### Performance

- in-memory user preferences cache (skin, locale, region, font size loaded once at startup)

### Other

- HTTP client migration from curl subprocesses to reqwest with shared async client
- settings architecture moved to system-level with SettingsStore abstraction
- game_metadata table schema validation with column count checks and COALESCE upserts
- image matching fixes for JPG symlinks, filesystem media scan, and exFAT stat ordering

---

## [0.2.0](https://github.com/lapastillaroja/replay-control/releases/tag/v0.2.0) - 2026-04-10

### Added

- add "Same as browser" locale option and bilingual locale names
- add cooperative (co-op) play as search filter and game detail field
- add auto-update system (check, download, install, rollback)
- add i18n support with Spanish and Japanese translations
- box art placeholders for games without cover art
- arcade clone fallback + aggressive normalization for box art
- fuzzy manifest matching + fix on-demand download panic
- metadata page streaming SSR with skeleton loaders
- add tracing instrumentation to server functions
- improve organize favorites UX
- skeleton loaders for streaming SSR
- response cache (10s TTL) + query cache for recommendations
- Phase 3 recommendations — Hidden Gems, Similar, Series Spotlight
- Phase 2 recommendations — rotating curated spotlights
- Phase 1 recommendation improvements — smart rotating pills
- change root password from web UI
- update PWA icons with arcade logo
- add rotating gaming icon to top bar
- add system controller icons to game lists and system cards
- improve change cover variant labels and layout
- PWA app shell caching and offline fallback
- add Search tab to bottom nav, system category icons, unfixed header
- graceful startup when storage unavailable + move assets to /static/
- show app version in More page footer, /api/version endpoint, and HTML meta tag
- alternate versions and cross-system sections on game detail page
- resolve 95% of TOSEC CPC duplicate display names
- show TOSEC bracket flag labels in display names
- TOSEC bracket flag classification and duplicate disambiguation
- TOSEC structured tag parsing (year, publisher, side/disk)
- broadcast SSE for skin and storage change notifications
- organize favorites by developer
- runtime SQLite corruption detection with recovery UI
- auto-generate M3U playlists for multi-part TOSEC games
- streaming download progress for LaunchBox metadata import
- developer name normalization for search and grouping
- unified metadata page — SSE rebuild, any_busy signal, on-load resume
- improve driver_status UX + gitignore load test raw files
- genre badges in favorites, CSS cleanup, fix favorites hydration bug
- multi-file ROM management — safe delete, rename restrictions, orphan cascade
- deadpool-sqlite connection pool for concurrent DB reads
- inline delete confirmation for downloaded manuals
- language preferences + manual fixes
- game manuals — in-folder detection + archive.org on-demand download
- share videos across regional variants via base_title
- add GameListItem shared component
- add REST API endpoints for libretro core
- responsive tablet/desktop CSS breakpoints
- parse CommunityRatingCount + weighted top-rated scoring
- developer search UI and game list page
- developer column, search, and game list page backend
- add Named_Titles support and screenshot gallery
- sequel/prequel play order navigation
- restructure More page + declutter game detail
- unify region preferences into single settings section
- show arcade clone siblings as "Arcade Versions" on game detail
- add pull-to-refresh for iOS PWA standalone mode
- concise labels for Other Versions and clippy cleanup
- add Wikidata series data with arcade support
- add game series and cross-name variant relationship system
- add CRC32 hash-based ROM identification for cartridge systems
- add secondary region preference with Strategy C sort order
- add text size toggle (normal/large) to settings page
- add pull-to-refresh for PWA standalone mode
- redesign metadata page layout with embedded DB stats
- add unified GameInfo API with lightweight RomListEntry
- parse developer, release year, and cooperative from LaunchBox XML
- filter non-playable MAME entries, preserve BIOS with flag
- parse MaxPlayers from LaunchBox XML for player count enrichment
- add orphaned image cleanup with manual UI button
- two-tier genre system with genre_group for unified filtering
- block DB operations during game library rebuild, add completion feedback
- auto-detect new/changed ROMs via filesystem watcher
- add is_special flag and genre fallback from LaunchBox
- add is_hack support — filter hacks from variants/dedup, show Hacks section
- parse genre from LaunchBox XML as fallback for baked-in game_db
- add translations section and filter translations from variants/dedup
- add related games section and improve recommendation diversity
- deduplicate recommendations by filtering clones and regional variants
- randomize top rated and "because you love" recommendations
- switch thumbnail indexing from git clone to GitHub REST API
- metadata busy banner and graceful DB unavailability handling
- auto-match metadata for externally added ROMs
- box art swap — pick alternate cover art per ROM
- prevent parallel metadata operations + SSE fixes + git-based thumbnail indexing
- libretro-thumbnails manifest-based pipeline + metadata page redesign
- integrate launch recents tracking into game launch flow
- SSR recommendations with L2 warmup, enrichment, and race condition fixes
- enable recommendations on home page with client-side loading
- persistent SQLite ROM cache (L2) with nolock-first DB open
- favorites/rating recommendations, fix ScummVM dedup
- game recommendations on home page (Phase 1)
- metadata-enriched search (genre, year) and min-rating filter
- word-level fuzzy search, word-boundary scoring, CPU mitigations
- region preference setting on /more page
- megabit size display for cartridge systems, split CSS into modules
- rating display, multiplayer filter, re-match images, git freshness check
- arcade driver badges, favorites filter, image matching improvements
- unified game list patterns, search navigation fixes, hide Alpha Player
- box art on home/favorites, ROM list filters, storage bar, and search fixes
- extended search filters and ROM list filter persistence
- merge Games tab into Home, rename to Games
- user screenshots with lightbox viewer
- game launch with health check recovery
- search icon in top bar, recent searches, random game, and / shortcut
- global search with filters and home page search bar
- game videos with search, inline preview, and multi-API fallback
- responsive image import UX with SSE and cancellable clone
- search, thumbnails, logs page, image import cancel, and UX fixes
- game images, metadata download, and metadata page redesign
- background metadata import with progress, auto-import, per-system coverage
- add game metadata system with LaunchBox import
- unified GameInfo type, skin sync toggle, theme->skin rename
- interactive skin selection and CSS theming fixes

### Fixed

- use i18n key for series position indicator instead of hardcoded format
- clippy warnings (hydrate target) and Docker e2e networking
- resolve clippy warnings and CI artifact path
- remove unused UpdatingPhase variants (clippy dead_code warning)
- correct style.css test URI to match /static/style.css route
- add /style.css endpoint for integration tests
- move ErrorBoundary to route level, fix metadata result messages
- Suspense must wrap ErrorBoundary for non-blocking resources
- organize favorites preview — all combinations, correct labels
- organize preview shows nested folders and uses genre_group
- organize preview uses genre_group instead of raw genre
- developer page reactive signals and URL filter persistence
- URL-encode # in box art paths, fix reactive signal warning
- search filter persistence, highlights, and back button
- search page back button navigation
- pull-to-refresh visible below Dynamic Island on iPhone
- search page width and input height consistency
- prevent DB corruption on exFAT with write gate
- enrichment reads filenames from L2 instead of L1 cache
- startup pipeline detects incomplete scans, improved cache clarity
- detect external skin changes from replay.cfg and broadcast SkinChanged
- include clone entries in display name disambiguation
- use 1h cache for pkg assets (no content hash in filenames)
- show system display name in startup scanning banner
- show phase, system, and progress count in rebuild banner
- use read_untracked for system display name in favorites page
- unfavorite from any page, recursive search, mtime sort
- resolve all clippy warnings
- add pool timeout and increase DELETE mode readers to 3
- use deadpool async API to prevent tokio worker starvation
- add CSS for rebuild progress text inside action card
- tablet text overflow — hero titles wrap, scroll cards 2-line clamp
- simplify skin change to page reload, fix disabled cursor
- move Clear Downloaded Images to Advanced section
- move 9 write operations from read pool to write pool
- explicit WAL checkpoints after bulk writes, scanning flag for ROM lookups
- filesystem-aware SQLite journal mode — WAL only on POSIX filesystems
- check server busy state before starting metadata/thumbnail operations
- remove hydration mismatch in GameListItem + improve curl_get_json
- batch player lookups to eliminate N+1 in multiplayer filter
- resolve clippy warnings, add path traversal protection, and reduce Closure leaks
- remove param_key Memo causing WASM panic on game navigation
- wrap manual server functions in spawn_blocking + register DeleteManual
- persist skin preference in settings.cfg, not replay.cfg
- resolve code review items — dead code, system display, WhereBuilder
- arcade snap/title resolution via unified resolve_image_on_disk
- prevent tokio worker starvation during image index build
- compact developer search block, arcade box art, query text
- merge developer from LaunchBox metadata into game detail
- use Suspense for game detail to fix sequel link navigation
- clear thumbnail progress after completion
- filesystem-aware SQLite locking + thumbnail auto-rebuild
- review fixes for startup refactoring
- eliminate rogue DB connections causing corruption
- non-blocking startup when game library is empty
- unify box art resolution between cards and detail page
- unify alias resolution with fuzzy matching for colon/dash variants
- metadata page horizontal overflow on mobile
- on-demand thumbnail download panics outside Tokio runtime
- thumbnail download counter starts at 1 instead of 0
- version-stripped box art matching checks fuzzy index too
- prevent orphan cleanup from deleting all images
- path traversal check blocks filenames containing ".."
- resolve Leptos hydration warnings on games page
- guarantee metadata_operation_in_progress is cleared after rebuild
- improve variant labels, filter arcade clones, skip broken symlink previews
- populate rom_cache after import when cache is empty
- stop event propagation on boxart picker close button
- re-enrich rom_cache after metadata/thumbnail imports
- case-ininternal exact matching for thumbnail resolution
- add arcade_db translation for thumbnail matching
- resolve recommendation box art from filesystem
- use fuzzy matching in update_image_paths_from_disk
- invalidate image cache after metadata import
- fall back to log files when journald is disabled
- auto-reopen DB connections when file is deleted externally
- resolve all clippy warnings across codebase
- region preference styling, SSR genres, and box art swap design
- auto-delete image repos after match, add cache management
- keep cloned image repos on disk, add staleness check to Download All
- validate library DB image paths against disk to catch fake-symlink artifacts
- search input focus on client-side navigation, inline genre loading
- revert dropdown arrow to SVG data URI for reliable positioning

### Other

- *(deps)* bump the production group across 1 directory with 3 updates ([#15](https://github.com/lapastillaroja/replay-control/pull/15))
- cache user preferences in memory to avoid per-request file I/O
- move Locale enum to core crate, eliminate hardcoded strings
- apply cargo fmt
- add integration tests for enrichment, schema rebuild, and co-op filter
- extract image resolution, thumbnail pipeline, and search scoring
- apply cargo fmt
- bump app and core version to 0.2.0
- fix clippy warnings across workspace
- split ReplayConfig into SystemConfig and AppSettings
- apply cargo fmt
- update attribution for TGDB developer/publisher/coop/rating data
- reorganize More page into five distinct sections
- *(deps)* bump the production group with 1 update ([#13](https://github.com/lapastillaroja/replay-control/pull/13))
- remove service worker offline support, update dependabot grouping
- Revert "fix: add /style.css endpoint for integration tests"
- update benchmarks to beta.4, rename Pi Configuration
- cargo fmt
- fix clippy warnings — collapsible ifs, dead code, too-many-args
- fix formatting in generate-test-fixtures
- simplify post-refactoring — type alias, Default impl, comments
- restructure cache module — rename, split, simplify
- remove remaining unnecessary #[allow] attributes
- fix clippy warnings and add #[allow] comments
- add skeleton loader CSS for favorites page
- move enrichment pipeline to core crate
- reduce SQLite page cache and read pool to 1 connection
- add jemalloc allocator for better memory management
- remove ImageIndex from request path, use DB box_art_url only
- Revert "fix: organize preview uses genre_group instead of raw genre"
- remove L1 ROM cache — unused after search unification
- review round 2 — GameSection for random picks, fix region format
- simplify review — shared component, remove duplication
- extract shared enrichment, unify GlobalSearchResult into RomListEntry
- use DB box_art_url, skip ImageIndex when possible
- optimize recommendations — eliminate DB round-trip, fix i64 overflow
- unify search backend — single query, shared enrichment
- update dependencies to latest compatible versions
- add license and repository metadata to Cargo.toml files
- unify home search bar with search page input
- add accent-colored logo to top bar, remove system card icons
- use replay.local instead of hardcoded IP, remove stale M3U comment
- fix clippy warnings and remove allow annotations
- extract cache-control header values to constants
- LaunchBoxMetadata tuple to named struct fields
- remove auto M3U generation (should not modify user romset)
- restore blocking SSR for homepage (streaming broke hydration)
- convert activity SSE from polling to broadcast
- SQL pre-filter with search_text column (search 220ms → 14ms)
- parallelize global search across systems via tokio::spawn
- add Cache-Control headers for static assets
- convert homepage to streaming SSR (TTFB 169ms → 7ms)
- limit get_recents to 15 entries (homepage only shows 11)
- apply cargo fmt
- SQL-level pagination for system ROM list
- apply cargo fmt
- single-row DB lookup for game detail pages
- unified Activity enum replacing busy/scanning/rebuild_progress
- unified any_busy signal for metadata page, fix SSE cleanup
- remove is_local from DB layer, use JournalMode enum
- upgrade rusqlite 0.32→0.38, SQLite 3.46.0→3.51.1
- full SSR for all pages — eliminate loading spinner flash
- remove clippy suppressions, extract param structs, consolidate helpers
- remove cache TTL for local storage, extract shared Freshness struct
- split global_search into focused helper functions
- deduplicate SSE handlers with generic sse_progress_stream builder
- extract rom_docs_handler into serve_rom_doc function
- standardize lock expect() messages in import.rs
- deduplicate MEGABIT_SYSTEMS — SSR delegates to core crate
- add integration tests for search helpers, ROM path parsing, and batch player lookup
- add Copy derive to qualifying types
- increase default text size to 110%, large to 140%
- remove all legacy DB Mutex shims, use pool exclusively
- simplify developer query + add 12 tests
- extract reusable hooks and reduce duplication
- remove redundant developer matching from global_search
- limit cargo parallelism to 8 jobs to prevent OOM during builds
- replace RomItem with unified GameListItem across all game lists
- replace remaining tuples with named structs + fix clippy
- cleanup dead code and minor fixes
- extract matching logic to core crate (#2-4)
- unify image matching into single core path
- eliminate hardcoded thumbnail strings across codebase
- consolidate thumbnail logic into core crate
- add Wikidata attribution to metadata page
- unify busy flags, fix startup bugs, per-batch DB locking
- split cache.rs, extract image matching, Arc-wrap ROM cache
- address code review findings — perf, safety, dedup
- sequenced startup pipeline, extract AppState, single DB connection
- split library_db.rs into sub-modules and consolidate utils
- Revert "feat: add pull-to-refresh for PWA standalone mode"
- derive thumbnail counts from game_library.box_art_url
- migrate video storage from videos.json to SQLite user_data.db
- rename rom_cache → game_library across codebase
- move find_image_on_disk and helpers to core crate
- shared DB initialization with eager open and corruption recovery
- replace reqwest with curl for video search API calls
- tier 1+2 optimizations — 98% faster page loads
- remove genre/year from search scoring, add min-rating UI filter
- add integration tests, extract router builder
- SSE metadata progress, .replay-control renames, box art dedup, tests
- extract game_detail sub-components, typed filter state, update docs
- split server_fns.rs and api/mod.rs into domain modules
- extract RebootButton, unify Transition, auto-close SSE stream
- rename log prefix from replay-companion to replay-control
- rename crates to replay-control-app/core, add hostname page, NFS reboot

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
- feat: runtime SQLite corruption detection with recovery UI — error-triggered `SQLITE_CORRUPT` detection, per-DB corrupt flag, full-page banner with Rebuild (library.db) or Restore/Repair (user_data.db) options (`1f6aa8c`)
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
- feat: orphan cascade on delete/rename — favorites, screenshots, user_data.db (videos, box art), library.db all cleaned up via new `delete_for_rom`/`rename_for_rom` methods (`445abc9`)
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
- refactor: make LibraryDb + UserDataDb stateless query namespaces — methods take `conn: &Connection` (`40072d9`)
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
- fix: single DB connection policy — import holds Mutex directly, eliminated 3 rogue LibraryDb::open() calls causing SQLite corruption (`f38f77a`)
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
- refactor: split library_db.rs (2,895 lines) into 7 focused sub-modules (`84cf3d5`)
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
- fix: validate library DB image paths against disk to catch fake-symlink artifacts (`49413d9`)
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
