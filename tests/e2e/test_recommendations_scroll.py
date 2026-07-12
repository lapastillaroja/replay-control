"""
E2E: Last Played and recommendation rows resume their horizontal scroll on Back.

Two cooperating mechanisms keep these rows where the user left them when they
press browser Back:

1. Recommendation/favorites data is frozen client-side (``client_cache``), so
   Back re-renders the *same* cards instead of re-fetching a regenerated
   snapshot. The recommendation test asserts this directly (the card links are
   identical before and after Back).
2. The ``use_scroll_memory`` hook saves each row's ``scrollLeft`` and re-applies
   it when the row re-mounts — tagged with a signature of the row's cards, so a
   row whose content changed (e.g. Last Played after playing/removing a game)
   starts at 0 instead of restoring a stale offset. (That reset-on-change path
   only happens on a full re-mount and is awkward to force deterministically in
   this container, so it isn't asserted here; the restore path is.)

Chromium auto-restores inner scroll on history navigation (which masks the bug);
iOS Safari — where it was reported — does not, so these tests force manual scroll
restoration to measure *our* behaviour. Seeded games have no box art, so their
placeholder cards shrink to fit and the row never overflows; the tests inject
fixed-width cards to recreate the production (real box art) overflow.

Container only — seeds /media/usb and restarts the app to scan.
"""

import pytest
from playwright.sync_api import expect

from conftest import (
    CONTAINER,
    exec_cmd,
    goto_hydrated,
    reset_library,
    restart_app_and_scan,
    seed_recent,
    seed_rom,
    wait_for_app,
    wait_hydrated,
)

pytestmark = pytest.mark.skipif(
    not CONTAINER,
    reason="scroll-memory e2e seeds container storage and restarts the app",
)

# Distinctly-named ROMs across several systems. The random-picks row is built by
# `diversify_picks`, which spreads picks across systems (capping each), so a
# single-system library yields only ~2 cards. Extensions must match each system
# or the scan skips the file.
_SEEDS = [
    ("nintendo_nes", "Alpha Quest.nes"),
    ("nintendo_nes", "Bravo Strike.nes"),
    ("nintendo_snes", "Cosmic Run.sfc"),
    ("nintendo_snes", "Delta Force.sfc"),
    ("nintendo_gb", "Echo Valley.gb"),
    ("nintendo_gb", "Fox Hunt.gb"),
    ("nintendo_gba", "Galaxy Wars.gba"),
    ("nintendo_gba", "Hyper Drive.gba"),
    ("nintendo_n64", "Iron Fist.z64"),
    ("nintendo_n64", "Jungle Dash.z64"),
    ("sega_gg", "Knight Moves.gg"),
    ("sega_gg", "Laser Blast.gg"),
]

# Force fixed-width, non-shrinking cards so a row overflows (and is horizontally
# scrollable) exactly as it does in production with real box art. The recs rows'
# flex items are `.scroll-card-item`; the Last Played row wraps each card in a
# `.recent-card-wrapper`. The stylesheet lives in <head> and survives SPA Back.
_FORCE_SCROLLABLE = (
    ".scroll-card-row{display:flex !important;overflow-x:auto !important;}"
    ".scroll-card-item,.recent-card-wrapper{flex:0 0 150px !important;"
    "min-width:150px !important;max-width:150px !important;}"
)


def _teardown_library():
    reset_library()
    try:
        exec_cmd("systemctl restart replay-control")
        wait_for_app(60)
    except Exception:  # noqa: BLE001
        pass


@pytest.fixture()
def seeded_recs_library():
    """Seed games across several systems so the home page renders a
    recommendations row with enough cards to scroll, then scan them."""
    reset_library()
    for system, rom in _SEEDS:
        seed_rom(system, rom)
    restart_app_and_scan(sorted({system for system, _ in _SEEDS}))
    yield
    _teardown_library()


@pytest.fixture()
def seeded_recents_library():
    """Seed games plus a recents marker for each so the home "Recently Played"
    row renders (and the cards link to scannable detail pages)."""
    reset_library()
    for system, rom in _SEEDS:
        seed_rom(system, rom)
        seed_recent(system, rom)
    restart_app_and_scan(sorted({system for system, _ in _SEEDS}))
    yield
    _teardown_library()


def _hrefs(locator) -> list:
    return locator.evaluate(
        "el => Array.from(el.querySelectorAll('a')).map(a => a.getAttribute('href'))"
    )


def _prepare(page):
    """Phone viewport + manual scroll restoration (so we measure our code, not
    chromium's), then navigate to a hydrated home page."""
    page.set_viewport_size({"width": 390, "height": 844})
    page.add_init_script("window.history.scrollRestoration = 'manual';")
    goto_hydrated(page, "/")


def _scroll_and_save(page, row):
    """Force the row scrollable, scroll it right, and return the saved offset."""
    page.add_style_tag(content=_FORCE_SCROLLABLE)
    page.wait_for_timeout(50)
    cards = row.locator("a").count()
    max_scroll = row.evaluate("el => el.scrollWidth - el.clientWidth")
    assert max_scroll > 0, f"row should overflow (cards={cards}, max_scroll={max_scroll})"
    row.evaluate("(el, x) => { el.scrollLeft = x; }", min(200, max_scroll))
    page.wait_for_timeout(150)
    saved = row.evaluate("el => Math.round(el.scrollLeft)")
    assert saved > 0, f"row should be scrolled before navigating away (got {saved})"
    return saved


def _assert_restored(page, row_selector_js, saved, label):
    # Wait for the rAF restore to land (generous: dev-fast hydration is slow).
    page.wait_for_function(
        f"([want]) => {{ const el = {row_selector_js}; return el && Math.abs(el.scrollLeft - want) <= 3; }}",
        arg=[saved],
        timeout=10000,
    )


def test_recommendations_row_keeps_horizontal_scroll_on_back(page, seeded_recs_library):
    _prepare(page)

    first_card = page.locator(".scroll-card-row a").first
    expect(first_card).to_be_visible(timeout=20000)
    row = page.locator(".scroll-card-row", has=page.locator("a")).first

    hrefs_before = _hrefs(row)
    saved = _scroll_and_save(page, row)

    href = first_card.get_attribute("href")
    first_card.click()
    page.wait_for_url(f"**{href}", timeout=15000)
    wait_hydrated(page)
    page.go_back()
    wait_hydrated(page)

    # Wait for a real card to re-appear before measuring (robust to slow hydration).
    expect(page.locator(".scroll-card-row a").first).to_be_visible(timeout=25000)
    row_back = page.locator(".scroll-card-row", has=page.locator("a")).first

    # Freeze: Back resumes the exact same set of cards (not a regenerated one).
    assert _hrefs(row_back) == hrefs_before, "Back should resume the same recommendation set"

    # …and the row is scrolled back to where it was.
    _assert_restored(page, "document.querySelector('.scroll-card-row')", saved, "recommendations")
    restored = row_back.evaluate("el => Math.round(el.scrollLeft)")
    assert abs(restored - saved) <= 3, f"expected ~{saved}, got {restored}"


def _recents_row(page):
    return page.locator(".scroll-card-row", has=page.locator(".recent-delete-btn")).first


def test_last_played_row_keeps_horizontal_scroll_on_back(page, seeded_recents_library):
    _prepare(page)

    expect(page.locator(".recent-delete-btn").first).to_be_visible(timeout=20000)
    row = _recents_row(page)
    saved = _scroll_and_save(page, row)

    first_link = row.locator("a").first
    href = first_link.get_attribute("href")
    first_link.click()
    page.wait_for_url(f"**{href}", timeout=15000)
    wait_hydrated(page)
    page.go_back()
    wait_hydrated(page)

    expect(page.locator(".recent-delete-btn").first).to_be_visible(timeout=25000)
    row_back = _recents_row(page)
    _assert_restored(
        page,
        "Array.from(document.querySelectorAll('.scroll-card-row')).find(r => r.querySelector('.recent-delete-btn'))",
        saved,
        "last-played",
    )
    restored = row_back.evaluate("el => Math.round(el.scrollLeft)")
    assert abs(restored - saved) <= 3, f"Last Played should restore scroll on Back: expected ~{saved}, got {restored}"
