# End-to-End Tests

Browser-based tests using Playwright. Two flavours of test live here:

1. **Auto-update UI tests** (`test_update_check.py`, `test_update_install.py`).
2. **Page health + responsiveness tests** (`test_page_health.py`,
   `test_response_cache.py`, `test_corruption_banner.py`,
   `test_library_build_pipeline.py`). These guard
   the user-facing behaviours the pool-design / cancellation-orphan work
   was supposed to fix; they catch regressions in route definitions,
   navigation latency, force-refresh resilience, and response-cache
   warmth.
3. **Feature behaviour tests** (`test_manual_upload.py`, `test_captures.py`,
   `test_recents.py`, `test_favorites.py`, `test_search.py`,
   `test_resource_management.py`, `test_admin_gating.py`). These seed a tiny
   library on disk and drive concrete features end to end: PDF/TXT manual
   upload, screenshot (capture) deletion, recents deletion, the favorites
   lifecycle, global search, the data-management/cleanup actions, and the auth
   guard (gating + login). They run **container only** (they mutate
   `/media/usb`); see "Container e2e (default)" below.

**No-JS-errors guard:** the shared `page` fixture fails any browser test that
logs a JS console error/warning or throws an uncaught exception. Network
resource 404s (e.g. an unmatched game's missing box art) are filtered via
`CONSOLE_IGNORE_SUBSTRINGS`; real script faults are not.

## Prerequisites

- Raspberry Pi running Replay Control at `https://replay.local:8443`
- Python 3.10+ with Playwright + pytest-timeout in a venv (`pip install`
  outside a venv is blocked by PEP 668 on modern Debian/Ubuntu/Fedora;
  the `--timeout` flag in the examples below is a `pytest-timeout`
  plugin flag, without it pytest exits with `error: unrecognized
  arguments: --timeout`):
  ```bash
  python3 -m venv tests/e2e/.venv
  tests/e2e/.venv/bin/pip install playwright pytest pytest-timeout
  tests/e2e/.venv/bin/playwright install chromium
  ```
  Then either prefix every command with `tests/e2e/.venv/bin/python`
  (used in the examples below) or `source tests/e2e/.venv/bin/activate`
  for the session.

## Running

```bash
# All tests
PI_IP=192.168.10.30 python -m pytest tests/e2e/ -v --timeout=180

# Just the check/notification tests (no service restart)
PI_IP=192.168.10.30 python -m pytest tests/e2e/test_update_check.py -v

# Just the install tests (triggers real service restart!)
PI_IP=192.168.10.30 python -m pytest tests/e2e/test_update_install.py -v
```

## Container e2e (default)

The feature behaviour tests run inside the RePlayOS test container, which boots
the app in standalone mode (`--storage-path /media/usb`, auth bypassed) so the
seeding helpers can mutate `/media/usb` freely. The container runner builds the
image, starts it, and runs Playwright against it:

```bash
# All feature tests (release build — slow, do this before final validation)
./tests/container/run.sh

# Fast local dev loop (debug WASM, reuse a warm build):
SKIP_BUILD=1 BUILD_PROFILE=debug PODMAN_DIRECT_BRIDGE=1 \
  PYTEST_ARGS='tests/e2e/test_captures.py tests/e2e/test_recents.py \
    tests/e2e/test_manual_upload.py tests/e2e/test_resource_management.py -v' \
  ./tests/container/run.sh
```

The seeding helpers live in `conftest.py`:

- `seeded_game` fixture — resets the library, seeds one NES ROM, restarts the
  service, and waits for the scan + activity to go idle. Yields the system /
  rom_filename / detail URL the tests build selectors from.
- `seed_capture`, `seed_recent`, `seed_rom` — drop on-disk artifacts that the
  live readers (game-detail captures, home recents) pick up without a rescan.
- `wait_hydrated(page)` — blocks on the global `.initial-loading-shell` overlay
  hiding (see CLAUDE.md) so click handlers are attached before interaction.
- `post_sfn` / `http_status` — raw server-fn / page probes used by the auth
  gating tests.

`test_admin_gating.py` is the exception: its module fixture relaunches the app
in **device mode** (no `--storage-path`, with first-setup marked done) so the
auth guard is active, then restores standalone on teardown. Run it on its own to
avoid interleaving the mode flip with the standalone feature tests:

```bash
SKIP_BUILD=1 BUILD_PROFILE=debug PODMAN_DIRECT_BRIDGE=1 \
  PYTEST_ARGS='tests/e2e/test_admin_gating.py -v' ./tests/container/run.sh
```

## Test Requirements

For update tests to work, the Pi must be running a version lower than the
available release. Set `version = "0.0.1"` in `replay-control-app/Cargo.toml`
and deploy before running install tests.

The update channel is set to `beta` by the test fixtures (the only release
is `v0.1.0-beta.4`, a prerelease).

## Test Categories

### `test_update_check.py` — Safe, no side effects
- Update banner appears after manual check
- Update banner appears from background check (60s)
- Banner has all action buttons
- Skip hides the banner
- Channel switch hides/shows prereleases
- Check button states
- Version display

### `test_update_install.py` — Triggers real service restart
- "Update Now" navigates to /updating
- /updating shows downloading progress
- /updating shows "do not navigate away"
- /updating shows restarting + auto-reloads
- Direct navigation to /updating without update redirects
- Temp files cleaned after update

### `test_page_health.py` — Safe, read-only

Catches regressions surfaced during the 2026-04-29 pool-design work:

- `TestRoutesRenderRealContent` — every main route renders real content,
  not the Leptos router fallback ("Page not found"). Anchors the lesson
  that `/system/<x>` is **not** a real route (it's `/games/<x>`); the
  earlier integration suite's status-only check missed this for a year.
- `TestSpaNavigationIsResponsive` — clicks through the bottom-nav and
  asserts each transition makes new content visible within a budget
  (1.5 s cold, 800 ms warm). The first user-facing complaint that
  drove the pool-design work lives here.
- `TestForceRefreshDoesNotHang` — rapid double force-reload on each
  main page, asserting the second reload completes within 3 s. The
  original cancellation-orphan bug surfaced as a multi-second hang
  exactly here.
- `TestServerFnsRegistered` — POST-smoke each server fn we added
  (notably `GetMetadataPageSnapshot`) so a missing `register_explicit`
  in `main.rs` flips a 200/400/405 to 404 and trips the test.
- `TestBrowserRouteSweep` — visits every route (browsing, discovery, all
  settings sub-pages) in one browser session, asserting each renders real
  content; the `page` fixture's no-JS-errors guard makes this a single cheap
  check of rendering + hydration + script health across the whole app.

### `test_response_cache.py` — Safe, read-only

Anchors `RESPONSE_TTL` >= ~30 s. Loads a page, waits 12 s (slightly
longer than the *old* 10 s TTL), reloads, and asserts the post-pause
hit is in the same ballpark as the warm hit. If the TTL is reverted
to 10 s this test fails immediately. Includes a baseline
absolute-warm-time check (`/favorites` warm < 200 ms on Pi 4).

### `test_library_build_pipeline.py` — Container only, mutates storage

Clicks the metadata page's `Rescan Library` action, listens to
`/sse/activity`, verifies the rescan transitions into background ROM matching,
and asserts a second rescan is blocked while identity owns the activity slot.
This test wipes and recreates `/media/usb`, so it is skipped outside the
container runner.

### `test_corruption_banner.py` — Triggers service restart + DB corruption
Covers the live client wire that the Rust integration suite can't reach:
`/sse/events` event → `SseEventsListener` → context signal → banner toggle.

`user_data.db`:
- Banner appears via the `init` payload after corrupt + service restart
- Restore from backup clears the banner via SSE push (no page reload)
- Reset clears the banner via SSE push (no page reload)

`library.db`:
- Service does not crash-loop on a clobbered header (silent recreate path)

The `preserve_*` fixtures snapshot the target DB before each test and
restore it after — even on failure — so the target ends in its pre-test
state.

**Not covered here (intentional):**
- Library *banner* in a real browser. Library startup corruption is silent
  (rebuildable cache → no banner) and runtime library corruption can't be
  triggered deterministically from outside the process (SQLite page cache
  hides the on-disk damage until the page is evicted). The Rebuild flow is
  validated at the Rust integration layer
  (`rebuild_corrupt_library_clears_flag_and_broadcasts_inverse`).
- Runtime page corruption on an open DB (post-open queries hitting bad
  pages). Same reason — no deterministic external trigger. The
  `check_for_corruption` path in `DbPool::read`/`write` is exercised via
  manual `mark_corrupt` calls in the Rust integration suite.
- Partial-header (1–15 byte) files surfaced through `LibraryDb::open` /
  `UserDataDb::open`. The detection helper `has_invalid_sqlite_header`
  is unit-tested for this case in `sqlite.rs`; the consumer code calls
  it the same way for both DBs.

### `test_manual_upload.py` — Container only, mutates storage

POSTs the multipart endpoint `POST /manuals/upload/<system>` directly and asserts
both the HTTP contract and the on-disk result under `.replay-control/manuals/`:

- Valid PDF (`%PDF-`) and valid UTF-8 TXT are accepted (200) and persisted
- Binary bytes named `.pdf` are rejected (400) and nothing is written
- A disallowed extension (`.exe`) is rejected (400)
- A path-traversal `rom_filename` is rejected (400)
- An empty `base_title` is rejected (400)

### `test_captures.py` — Container only, mutates storage

Seeds 1×1 PNG captures under `captures/<system>/` (read live by game-detail) and
drives both delete affordances (per-thumbnail `×` and the lightbox delete), each
behind a JS `confirm()` → `delete_user_capture`:

- A seeded capture renders on the game-detail page
- Deleting via the thumbnail button removes the card and the file on disk
- Deleting via the lightbox removes the file on disk
- Deleting one of two captures leaves the other intact

### `test_recents.py` — Container only, mutates storage

Seeds `.rec` markers under `roms/_recent/` (read live by the home page) and drives
the per-card delete button (`.recent-delete-btn`) → `delete_recent`:

- A seeded recent renders on the home page
- Deleting removes the marker file on disk
- Deleting one of two recents leaves the other intact

### `test_favorites.py` — Container only, mutates storage

Drives the full favorites loop in one browser session (fast): toggle a favorite
on game-detail (`button.game-action-fav` → `add_favorite`), confirm the
`roms/_favorites/<system>@<rom>.fav` marker appears and the game shows on
`/favorites`, then remove it via the favorites-page star + inline confirm
(`remove_favorite`) and confirm the marker is gone.

### `test_search.py` — Container only, mutates storage

Searches the scanned library: a seeded ROM is findable by a filename fragment
(`/search?q=Seed` → result group), and a non-matching query renders the
`p.empty-state` no-results state.

### `test_rom_management.py` — Container only, mutates storage

Drives the destructive ROM actions on game-detail (admin-gated on device, open in
the standalone container):

- Rename via the inline `.game-rename-inline` editor renames the file under
  `roms/<system>/` and navigates to the new game URL
- Delete via the two-step `.game-action-delete` → `.game-action-delete-confirm`
  removes the file and returns to the system list

### `test_media_serving.py` — Container only, mutates storage

Seeds files and asserts the root serving routes stream them with the right
Content-Type (extension-based): `/captures/*` (image/png), `/owned-manuals/*`
(application/pdf), `/media/*` (image/png), and a 404 for a missing file. This
complements the capture/manual *deletion* tests by covering the *serving* side.

### `test_resource_management.py` — Container only, mutates storage

Drives the data-management actions on `/settings/game-library` (behind the
"Advanced" disclosure), each a reveal-then-confirm `ClearActionCard`:

- Clear Downloaded Images removes the media dir on disk
- Cleanup Orphaned Images / Clear Metadata / Clear Thumbnail Index report a result
- Cancelling the confirm leaves files untouched

These wait for the activity slot to be Idle first, since the maintenance server
fns refuse to start while a scan/identity owns the slot.

### `test_admin_gating.py` — Container only, relaunches in device mode

Relaunches the app in device mode (auth enforced) with first-setup done, then
asserts the guard's fail-closed behaviour for an unauthenticated caller:

- Non-public / unknown server fns reject anonymous with 401 (a 401 also
  confirms device mode — standalone would bypass the guard)
- The guard lets public server fns through (not 401)
- Admin and browsing pages redirect anonymous callers to `/login`
- The health endpoint stays open

It also covers the **positive** login path: the fixture sets a known root
password with `chpasswd`, then a browser signs in with the device password
(`#login-admin-password` → "Sign in as admin") and reaches an otherwise-gated
page, while a wrong password surfaces `.login-field-error` and stays on `/login`.
This requires `python3` + `libcrypt1` + `passwd` in `Containerfile.replayos`
(the app verifies the OS password by crypting against `/etc/shadow`).

The classification completeness of every server fn is also guarded by the Rust
meta-test `auth_guard_classifies_every_server_function_intentionally`.
