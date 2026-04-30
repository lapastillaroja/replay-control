# End-to-End Tests

Browser-based tests using Playwright. Two flavours of test live here:

1. **Auto-update UI tests** (`test_update_check.py`, `test_update_install.py`).
2. **Page health + responsiveness tests** (`test_page_health.py`,
   `test_response_cache.py`, `test_corruption_banner.py`). These guard
   the user-facing behaviours the pool-design / cancellation-orphan work
   was supposed to fix; they catch regressions in route definitions,
   navigation latency, force-refresh resilience, and response-cache
   warmth.

## Prerequisites

- Raspberry Pi running Replay Control at `replay.local:8080`
- Python 3.10+ with Playwright:
  ```bash
  pip install playwright pytest
  playwright install chromium
  ```

## Running

```bash
# All tests
PI_IP=192.168.10.30 python -m pytest tests/e2e/ -v --timeout=180

# Just the check/notification tests (no service restart)
PI_IP=192.168.10.30 python -m pytest tests/e2e/test_update_check.py -v

# Just the install tests (triggers real service restart!)
PI_IP=192.168.10.30 python -m pytest tests/e2e/test_update_install.py -v
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

### `test_response_cache.py` — Safe, read-only

Anchors `RESPONSE_TTL` >= ~30 s. Loads a page, waits 12 s (slightly
longer than the *old* 10 s TTL), reloads, and asserts the post-pause
hit is in the same ballpark as the warm hit. If the TTL is reverted
to 10 s this test fails immediately. Includes a baseline
absolute-warm-time check (`/favorites` warm < 200 ms on Pi 4).

### `test_corruption_banner.py` — Triggers service restart + DB corruption
Covers the live client wire that the Rust integration suite can't reach:
`/sse/config` event → `SseConfigListener` → context signal → banner toggle.

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
