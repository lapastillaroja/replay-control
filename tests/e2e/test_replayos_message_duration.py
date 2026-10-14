"""Device-only regression for saving the on-screen message duration on change."""

import os

import pytest
from playwright.sync_api import expect

from conftest import PI_URL

pytestmark = pytest.mark.skipif(
    not os.environ.get("REPLAYOS_MESSAGE_DURATION_TEST"),
    reason="requires an authenticated RePlayOS device",
)


def open_replayos_settings(page):
    page.goto(PI_URL + "/settings/replayos", wait_until="domcontentloaded", timeout=60000)
    page.locator(".initial-loading-shell").wait_for(state="hidden", timeout=120000)
    if "/login" in page.url:
        password = page.locator("#login-admin-password")
        password.fill(os.environ.get("PI_PASS", "replayos"))
        page.locator("form").filter(has=password).locator("button").click()
        page.wait_for_url(lambda url: "/login" not in str(url), timeout=60000)
        page.goto(PI_URL + "/settings/replayos", wait_until="domcontentloaded")
        page.locator(".initial-loading-shell").wait_for(state="hidden", timeout=120000)


def test_duration_change_persists_without_sending(page):
    open_replayos_settings(page)
    duration = page.locator(".replayos-duration-select")
    expect(duration).to_be_visible(timeout=30000)
    previous = duration.input_value()
    chosen = "10" if previous != "10" else "5"
    changed = False

    try:
        with page.expect_response(
            lambda response: "/sfn/save_replayos_message_duration" in response.url
        ) as save:
            duration.select_option(chosen)
        assert save.value.ok
        changed = True
        expect(duration).to_be_enabled()
        open_replayos_settings(page)
        expect(page.locator(".replayos-duration-select")).to_have_value(chosen)
    finally:
        if changed:
            open_replayos_settings(page)
            duration = page.locator(".replayos-duration-select")
            expect(duration).to_be_visible(timeout=30000)
            with page.expect_response(
                lambda response: "/sfn/save_replayos_message_duration" in response.url
            ) as restore:
                duration.select_option(previous)
            assert restore.value.ok


def test_duration_save_failure_restores_previous_choice(page):
    save_requests = []

    def intercept(route):
        if "/sfn/save_replayos_message_duration" in route.request.url:
            save_requests.append(route.request.post_data)
            route.fulfill(status=500, content_type="text/plain", body="Test save failure")
        else:
            route.continue_()

    page.route("**/sfn/**", intercept)
    open_replayos_settings(page)

    duration = page.locator(".replayos-duration-select")
    expect(duration).to_be_visible(timeout=30000)
    previous = duration.input_value()
    duration.select_option("10" if previous != "10" else "5")

    expect(page.get_by_text("Could not save the message duration.", exact=False)).to_be_visible()
    expect(duration).to_have_value(previous)
    assert len(save_requests) == 1
    assert "duration_secs" in save_requests[0]
