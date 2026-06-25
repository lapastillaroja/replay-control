"""
End-to-end coverage for completing the first-setup flow (device mode).

`test_first_setup.py` covers the *gate* (unauthenticated requests redirect to
/first-setup). This covers actually *completing* it: entering the device password
on /first-setup runs `complete_first_setup`, which verifies the OS root password,
persists `first_setup_done`, and opens an admin session. Needs the device-mode
container (python3 + a known root password); reuses the `device_mode_first_setup`
fixture, which boots with first-setup pending.

The wrong-password test runs first so it leaves setup pending for the completion
test (the module fixture boots the device once).
"""

import pytest
from playwright.sync_api import expect

from conftest import (
    ADMIN_PW,
    CONTAINER,
    DEVICE_SETTINGS_DIR,
    exec_cmd,
    goto_hydrated,
)

pytestmark = pytest.mark.skipif(
    not CONTAINER,
    reason="first-setup completion e2e relaunches the app in device mode",
)


def test_first_setup_rejects_wrong_password(device_mode_first_setup, page):
    goto_hydrated(page, "/first-setup")
    pw = page.locator("#first-setup-password")
    expect(pw).to_be_visible(timeout=15000)
    pw.fill("definitely-the-wrong-password")
    page.get_by_role("button", name="Continue as admin").click()
    # Stays on /first-setup with an inline error; setup remains pending.
    expect(page.locator(".login-field-error")).to_be_visible(timeout=10000)
    assert "/first-setup" in page.url


def test_first_setup_completion_grants_admin(device_mode_first_setup, page):
    goto_hydrated(page, "/first-setup")
    pw = page.locator("#first-setup-password")
    expect(pw).to_be_visible(timeout=15000)
    pw.fill(ADMIN_PW)
    page.get_by_role("button", name="Continue as admin").click()

    # Success navigates away from /first-setup (an authenticated session; in the
    # device container it lands on "/" / the storage gate, never back to setup).
    page.wait_for_url(lambda url: "/first-setup" not in url, timeout=10000)

    setup_line = exec_cmd(f'grep first_setup_done "{DEVICE_SETTINGS_DIR}/settings.cfg"')
    assert "true" in setup_line, f"first_setup_done should be persisted true, got: {setup_line!r}"
