"""Browser behavior of an installed standalone package."""

import os

import pytest
from playwright.sync_api import expect

from conftest import PI_URL, goto_hydrated

pytestmark = pytest.mark.skipif(
    "REPLAY_PACKAGE_BROWSER" not in os.environ,
    reason="requires a standalone package integration runner",
)


def test_storage_step_precedes_metadata(page):
    goto_hydrated(page, "/?setup", timeout=60000)
    expect(page.locator(".setup-checklist")).to_be_visible(timeout=30000)
    expect(page.get_by_text("Choose your game library folder")).to_be_visible()
    expect(page.get_by_text("Finish setting up your game library.")).to_be_visible()
    expect(page.locator(".setup-task-row")).to_have_count(2)
    expect(page.locator(".setup-task-row").first.locator(".setup-task.done")).to_be_visible()
    expect(page.locator(".setup-task-row").nth(1).get_by_text("Download metadata sources")).to_be_visible()
    expect(page).to_have_url(f"{PI_URL}/?setup")


def test_remote_settings_shows_storage_location_with_local_access_hint(page):
    goto_hydrated(page, "/settings", timeout=60000)

    storage = page.get_by_role("link", name="Storage location", include_hidden=True)
    expect(storage).to_be_visible(timeout=30000)
    expect(storage).to_have_attribute("aria-disabled", "true")
    expect(page.get_by_text("To change storage, open Replay Control on this computer using localhost.")).to_be_visible()


def test_packaged_site_hydrates_and_loads_settings(page):
    goto_hydrated(page, "/", timeout=60000)

    with page.expect_response(
        lambda response: "/sfn/get_font_size" in response.url,
        timeout=30000,
    ) as font_size:
        page.locator('a[href="/settings"]').first.click()

    assert font_size.value.status == 200
    expect(page.locator(".settings-page")).to_be_visible()
    expect(page.get_by_text("Could not find a server function")).to_have_count(0)
