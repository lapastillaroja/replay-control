"""
Regression: the game-list launch button must survive both SSR+hydration of a
result row and a browser Back. The bug was a hydration mismatch from an empty
text child (`{""}`) in the row's overlay <A>, which derailed hydration and left
the launch button (and the rest of the row) without handlers.
"""

import pytest
from playwright.sync_api import expect

from conftest import CONTAINER, goto_hydrated

pytestmark = pytest.mark.skipif(not CONTAINER, reason="container only")


def test_launch_icon_does_not_capture_taps(page, seeded_game):
    # The launch icon must be pointer-events:none so the tap target is the
    # <button>, not the decorative <span>. iOS Safari drops a *delegated* click
    # whose target is a non-interactive element, which killed the launch button
    # after a swipe-back re-render (fresh load used a direct handler and worked).
    goto_hydrated(page, "/search?q=Seed")
    expect(page.locator(".game-list-launch-btn").first).to_be_visible(timeout=15000)
    hit = page.evaluate(
        "() => {"
        " const icon = document.querySelector('.game-list-launch-btn .game-action-icon');"
        " const r = icon.getBoundingClientRect();"
        " const el = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);"
        " return el ? el.tagName : null;"
        "}"
    )
    assert hit == "BUTTON", f"tap on the launch icon must hit the <button>, got {hit}"


def test_launch_in_ssr_rendered_results(page, seeded_game):
    # SSR-load a results page with a real row, then hydrate. Before the fix this
    # threw an unrecoverable hydration error (caught by the page console guard)
    # and the launch button was dead/absent.
    goto_hydrated(page, "/search?q=Seed")
    launch = page.locator(".game-list-launch-btn").first
    expect(launch).to_be_visible(timeout=15000)
    # The launch handler fires the server call. (Assert the request, not the
    # transient pending class, which is too brief to catch reliably in
    # standalone mode where launch_game returns immediately.)
    with page.expect_request(lambda r: "/sfn/launch_game" in r.url, timeout=5000):
        launch.click()


def test_launch_after_browser_back(page, seeded_game):
    # Client-render the row (type a query), SPA-nav away, real browser Back.
    goto_hydrated(page, "/search")
    page.locator(".search-page-input").fill("Seed")
    launch = page.locator(".game-list-launch-btn").first
    expect(launch).to_be_visible(timeout=15000)
    expect(launch).to_be_enabled(timeout=5000)

    page.locator('.bottom-nav a[href="/favorites"]').click()
    page.wait_for_url("**/favorites", timeout=10000)
    page.go_back()
    page.wait_for_url("**/search**", timeout=10000)

    back_launch = page.locator(".game-list-launch-btn").first
    expect(back_launch).to_be_visible(timeout=15000)
    # Control: the inline favorite still toggles after Back.
    page.locator(".rom-fav-btn").first.click()
    expect(page.locator(".rom-fav-btn.rom-fav-active").first).to_be_visible(timeout=5000)
    # The launch handler fires the server call after Back.
    with page.expect_request(lambda r: "/sfn/launch_game" in r.url, timeout=5000):
        back_launch.click()
