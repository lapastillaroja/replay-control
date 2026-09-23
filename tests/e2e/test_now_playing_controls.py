"""Read-only, opt-in check against an already loaded multidisc game."""

import os

import pytest
from playwright.sync_api import expect

from conftest import PI_URL


@pytest.mark.skipif(
    not os.environ.get("ACTIVE_MULTIDISC_GAME"),
    reason="requires an already loaded multidisc game",
)
def test_more_panel_has_save_states_without_disc_management(page):
    requests = []
    page.on("request", lambda request: requests.append(request.url))
    page.goto(PI_URL, wait_until="domcontentloaded", timeout=60000)
    page.locator(".initial-loading-shell").wait_for(state="hidden", timeout=120000)
    if "/login" in page.url:
        password = page.locator("#login-admin-password")
        password.fill(os.environ.get("PI_PASS", "replayos"))
        page.locator("form").filter(has=password).locator("button").click()
        page.wait_for_url(lambda url: "/login" not in str(url), timeout=60000)
        page.locator(".initial-loading-shell").wait_for(state="hidden", timeout=120000)

    # Keep the read-only disc indicator, but no disc-changing controls.
    expect(page.locator(".now-playing-bar-disc")).to_be_visible(timeout=30000)
    page.locator(".now-playing-control-more").click()
    panel = page.locator(".now-playing-more-panel")
    expect(panel.locator(".save-states-step-group")).to_be_visible()
    expect(panel.locator(".compact-controls-panel")).to_have_count(1)
    expect(panel.locator(".media-actions")).to_have_count(0)
    page.locator(".now-playing-control-more").click()
    expect(panel).to_have_count(0)
    assert not any("replay_media" in url for url in requests)
