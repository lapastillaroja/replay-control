"""
End-to-end page health + responsiveness tests.

Covers the regressions surfaced during the pool-design / cancellation-orphan
investigation. Each test class targets one class of bug:

  TestRoutesRenderRealContent
      The leptos router's fallback returns HTTP 200 with a "Page not found"
      body for unknown URLs, so a status-only check is not enough. These
      tests anchor each main route by *content*, not status. Catches the
      regression where the integration suite was hitting `/system/<x>`
      (a no-op fallback) for over a year while believing the route worked.

  TestSpaNavigationIsResponsive
      Clicks through the bottom-nav links and asserts each transition makes
      the new content visible within a budget. Catches regressions in the
      "click feels slow" axis — the original user complaint that drove the
      pool-design work.

  TestForceRefreshDoesNotHang
      Force-reloads each page twice in rapid succession and asserts the
      second reload finishes within a tight budget. The original
      cancellation-orphan bug manifested as the second refresh hanging
      indefinitely (or for the full 15 s interact timeout) until the
      orphan closure released the SyncWrapper's mutex.

  TestServerFnsRegistered
      Bare reachability check on the new server fns added by the pool
      work. If someone forgets to call `register_explicit::<…>` in main.rs
      these flip from 200 to 404 immediately.

The numeric budgets are deliberately generous (a small Pi 4 on USB+exFAT,
not Pi 5 / NVMe) — they exist to catch a regression of *order of magnitude*
("transition went from 250 ms to 5 s"), not to pin micro-perf.

Set `APP_URL` to point at any deployment (Pi or container or localhost).
"""

import time
from urllib.request import urlopen, Request

import pytest

from conftest import PI_URL


# Real routes from `replay-control-app/src/lib.rs`. /system/<x> is NOT a
# valid route — it falls through to the router fallback.
#
# Marker text is the literal in the SSR HTML. Two requirements:
#   1. Don't use ALL-CAPS strings from the rendered visual — those come
#      from CSS `text-transform: uppercase` and never appear in source.
#   2. Pick markers that render whether or not the test fixture has games
#      in the system. The CI e2e container ships an empty roms_dir, so
#      filter UI like "Hide Hacks" / "All Genres" is omitted (those only
#      render when the system has games). The system display name and the
#      `rom-count` element render regardless.
MAIN_ROUTES = [
    ("/", ["Replay Control", "Last Played"]),
    # rom-count is the literal CSS class on the count <p>; it's emitted
    # whether the system has games or not. Anchors that we're not on the
    # 404 fallback (which lacks any of this UI).
    ("/games/nintendo_snes", ["rom-count", "Super Nintendo"]),
    ("/games/sega_smd", ["rom-count", "Sega Mega Drive"]),
    ("/favorites", ["Replay Control"]),
    ("/search?q=mario", ["Replay Control"]),
    ("/settings", ["Replay Control"]),
    ("/settings/metadata", ["Replay Control"]),
]

# Generous transition budget (Pi 4 + USB+exFAT). The fast path is ~50–80 ms,
# cold path ~200 ms. We use 1.5 s as the regression threshold — anything
# slower than this is a real "the page hangs" regression worth catching.
TRANSITION_BUDGET_MS = 1500
# Force-reload should also finish within a budget. The cancellation-orphan
# bug was multi-second; 3 s is plenty of headroom for a real Pi.
FORCE_RELOAD_BUDGET_MS = 3000


def _http_get_text(url: str, timeout: int = 10) -> str:
    return urlopen(url, timeout=timeout).read().decode("utf-8", errors="replace")


def _http_post(url: str, timeout: int = 10) -> int:
    """POST {} and return status code. Used for server-fn reachability."""
    req = Request(url, data=b"{}", method="POST", headers={"Content-Type": "application/json"})
    try:
        return urlopen(req, timeout=timeout).status
    except Exception as e:
        # 4xx/5xx are urllib HTTPError instances with .code
        return getattr(e, "code", 0)


# ── Routes render real content (not the fallback) ────────────────────────


class TestRoutesRenderRealContent:

    @pytest.mark.parametrize("path,markers", MAIN_ROUTES)
    def test_route_renders_real_content(self, path, markers):
        body = _http_get_text(f"{PI_URL}{path}")
        assert "Page not found" not in body, (
            f"{path} fell through to the router fallback "
            f"— almost certainly a wrong URL in the test or a route rename. "
            f"Real routes are listed in `replay-control-app/src/lib.rs`."
        )
        for marker in markers:
            assert marker in body, f"{path} should contain {marker!r} but does not"

    def test_unknown_route_renders_fallback_with_200(self):
        # Leptos returns 200 for everything; the body is the discriminator.
        # Use a path that doesn't match ANY route pattern — `/games/:system`
        # would still match (just renders an empty system view).
        body = _http_get_text(f"{PI_URL}/this-route-definitely-does-not-exist")
        assert "Page not found" in body, (
            "Unknown route should render the router fallback content. "
            "If this fails the leptos route table changed."
        )

    def test_legacy_system_path_is_not_a_real_route(self):
        # Anchor the lesson learned: /system/<x> is *not* a valid route.
        # If somebody renames the route from /games to /system this test
        # flips, prompting them to also update the e2e suite.
        body = _http_get_text(f"{PI_URL}/system/nintendo_snes")
        assert "Page not found" in body


# ── SPA navigation is responsive ─────────────────────────────────────────


class TestPageNavigationIsResponsive:
    """
    Navigate through every main page and measure 'load complete' time.

    These tests are the productionised version of the playwright probes used
    during the pool-design investigation
    (`scratch-2026-04-29-pool-design/`). They catch the original user
    complaint ("page transition feels stale") at the navigation-cycle level.

    Uses `page.goto` rather than clicking a link so the test isn't coupled
    to specific UI selectors that may shift over time. The timing budget
    is what guards the user-perceived behaviour either way: an SPA-click
    or a `goto` both go through the same server-fn pipeline that the
    pool-design work optimised.
    """

    PAGES = [
        "/",
        "/favorites",
        "/search?q=mario",
        "/settings",
        "/settings/metadata",
        "/games/nintendo_snes",
    ]

    def _goto_and_measure(self, page, url):
        t0 = time.perf_counter()
        page.goto(url, wait_until="load", timeout=TRANSITION_BUDGET_MS * 4)
        return int((time.perf_counter() - t0) * 1000)

    def test_first_pass_within_budget(self, page):
        page.goto(f"{PI_URL}/", wait_until="load", timeout=15000)
        time.sleep(0.5)  # let initial hydration settle
        for path in self.PAGES:
            elapsed = self._goto_and_measure(page, f"{PI_URL}{path}")
            assert elapsed < TRANSITION_BUDGET_MS, (
                f"navigation to '{path}' took {elapsed} ms "
                f"(budget {TRANSITION_BUDGET_MS} ms). The original user-reported "
                f"hang manifested here."
            )

    def test_second_pass_is_fast_warm_cache(self, page):
        page.goto(f"{PI_URL}/", wait_until="load", timeout=15000)
        time.sleep(0.5)
        # First pass: warms the response_cache + L1 systems cache.
        for path in self.PAGES:
            self._goto_and_measure(page, f"{PI_URL}{path}")
        # Second pass: every nav should now be warm and tight.
        # Pi 4 / USB+exFAT baseline is <300 ms; budget 800 ms catches
        # order-of-magnitude regressions without false-flagging on slow days.
        warm_budget = 800
        for path in self.PAGES:
            elapsed = self._goto_and_measure(page, f"{PI_URL}{path}")
            assert elapsed < warm_budget, (
                f"warm-cache navigation to '{path}' took {elapsed} ms "
                f"(budget {warm_budget} ms). If this regresses, check "
                f"RESPONSE_TTL in api/response_cache.rs and the SsrSnapshot "
                f"invalidation hooks — both should keep these hot."
            )


# ── Force-refresh doesn't hang (the original reported bug) ────────────────


class TestForceRefreshDoesNotHang:
    """
    Reproduces the original user-reported scenario: rapid double force-reload.

    The cancellation-orphan bug had this signature: first refresh works,
    second refresh hangs because the first refresh's cancelled SSR future
    left a closure orphaned on the blocking pool, holding the SyncWrapper
    mutex, blocking every subsequent `interact()` until it finished. Tier 1
    (one closure per page render) + Tier 5 (15s interact timeout) defend
    against this.
    """

    PAGES = [
        "/",
        "/games/nintendo_snes",
        "/favorites",
        "/settings/metadata",
        "/search?q=mario",
    ]

    @pytest.mark.parametrize("path", PAGES)
    def test_double_force_reload(self, page, path):
        page.goto(f"{PI_URL}{path}", wait_until="load", timeout=10000)
        time.sleep(0.3)

        t0 = time.perf_counter()
        page.reload(wait_until="load", timeout=FORCE_RELOAD_BUDGET_MS)
        elapsed1 = int((time.perf_counter() - t0) * 1000)

        # Second reload immediately — this is the suspect operation.
        t0 = time.perf_counter()
        page.reload(wait_until="load", timeout=FORCE_RELOAD_BUDGET_MS)
        elapsed2 = int((time.perf_counter() - t0) * 1000)

        assert elapsed1 < FORCE_RELOAD_BUDGET_MS, (
            f"first reload of {path} took {elapsed1} ms "
            f"(budget {FORCE_RELOAD_BUDGET_MS} ms)"
        )
        assert elapsed2 < FORCE_RELOAD_BUDGET_MS, (
            f"second rapid reload of {path} took {elapsed2} ms "
            f"(budget {FORCE_RELOAD_BUDGET_MS} ms). The original "
            f"cancellation-orphan bug surfaced here as a multi-second hang."
        )


# ── Server fns wired ──────────────────────────────────────────────────────


class TestServerFnsRegistered:
    """
    Smoke-test that the new server fns from the pool-design work are
    registered. POST {} returns 200 / 400 / 405 if registered (depending
    on whether the empty body decodes), 404 if forgotten in main.rs.
    """

    def test_get_metadata_page_snapshot_registered(self):
        status = _http_post(f"{PI_URL}/sfn/GetMetadataPageSnapshot")
        assert status in (200, 400, 405), (
            f"GetMetadataPageSnapshot returned {status}. Likely missing a "
            f"register_explicit::<…>() in main.rs."
        )

    @pytest.mark.parametrize("fn_name", [
        "GetRecommendations",
        "GetFavoritesRecommendations",
        "GetSystems",
        "GetInfo",
        "GetMetadataPageSnapshot",
    ])
    def test_known_server_fns_registered(self, fn_name):
        status = _http_post(f"{PI_URL}/sfn/{fn_name}")
        assert status != 404, (
            f"{fn_name} returned 404 — server fn not registered."
        )
