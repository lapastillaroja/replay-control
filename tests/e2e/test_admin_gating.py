"""
End-to-end coverage for the auth guard's role gating.

The container normally boots in standalone mode (`--storage-path`), where the
guard is bypassed and everything is open. To exercise the real gating this
module relaunches the in-container app in *device* mode (no `--storage-path`,
which trips `is_device()` via the `/opt/replay` marker) with first-setup already
completed, then asserts the middleware's decisions for an unauthenticated
caller:

  - admin/default server fns reject anonymous with 401
  - the guard lets public server fns through (not 401)
  - admin/browsing pages redirect anonymous callers to /login
  - static/health endpoints stay open

It also covers the positive login path (correct password grants a session;
wrong password is rejected), enabled by python3 + a known root password in the
test image. The shared `device_mode_app` fixture (in conftest) relaunches the
app in device mode and restores standalone on teardown.
"""

import pytest
from playwright.sync_api import expect

from conftest import (
    ADMIN_PW,
    CONTAINER,
    goto_hydrated,
    http_status,
    post_sfn,
)

pytestmark = pytest.mark.skipif(
    not CONTAINER,
    reason="admin-gating e2e relaunches the in-container app in device mode",
)


def test_admin_server_fn_rejects_anonymous(device_mode_app):
    # Non-public server fn + anonymous caller -> guard returns 401 (fail-closed).
    # A 401 here also confirms the instance is in device mode (standalone would
    # bypass the guard and return 200).
    assert post_sfn("clear_images") == 401


def test_unknown_server_fn_is_fail_closed_for_anonymous(device_mode_app):
    # An unclassified /sfn path defaults to Admin, so anonymous gets 401, not 404.
    assert post_sfn("definitely_not_a_real_server_fn") == 401


def test_guard_lets_public_server_fn_through(device_mode_app):
    # The guard classifies by normalized name: a public fn (get_auth_status) is
    # NOT blocked with 401, unlike an admin fn. It passes through to Leptos,
    # which returns 400 here only because the real handler is registered at a
    # hashed route, not this normalized snake path. The point under test is the
    # guard's decision (not 401), not Leptos routing.
    status = post_sfn("get_auth_status")
    assert status != 401, f"public server fn must not be blocked by the guard, got {status}"
    assert status in (200, 400), f"unexpected status for public server fn: {status}"


def test_admin_page_redirects_anonymous_to_login(device_mode_app):
    status, location = http_status("/settings/access")
    assert status in (302, 303, 307), f"expected redirect, got {status}"
    assert "/login" in location, f"admin page should redirect to /login, got {location!r}"


def test_browsing_page_redirects_anonymous_to_login(device_mode_app):
    status, location = http_status("/")
    assert status in (302, 303, 307), f"expected redirect, got {status}"
    assert "/login" in location, f"browsing page should redirect anonymous to /login, got {location!r}"


def test_health_endpoint_stays_open(device_mode_app):
    status, _ = http_status("/api/version")
    assert status == 200


# ── Positive login path (needs python3 + a known root password) ───────────


def test_admin_login_with_correct_password_grants_access(device_mode_app, page):
    # Sign in with the device password; success navigates away from /login.
    goto_hydrated(page, "/login")
    page.fill("#login-admin-password", ADMIN_PW)
    page.get_by_role("button", name="Sign in as admin").click()
    page.wait_for_function("!location.pathname.startsWith('/login')", timeout=10000)

    # The session persists: a fresh gated request is NOT bounced back to /login.
    # (Anonymous would redirect to /login — see the gating tests. In this device
    # container it lands on /waiting, the storage-readiness gate, which is
    # orthogonal to auth; the point is it is not /login.)
    goto_hydrated(page, "/settings/access")
    assert "/login" not in page.url, (
        f"authenticated session should not be bounced to /login, got {page.url}"
    )


def test_admin_login_with_wrong_password_is_rejected(device_mode_app, page):
    goto_hydrated(page, "/login")
    page.fill("#login-admin-password", "definitely-the-wrong-password")
    page.get_by_role("button", name="Sign in as admin").click()
    # An inline error appears and we stay on /login (no session granted).
    expect(page.locator(".login-field-error")).to_be_visible(timeout=10000)
    assert "/login" in page.url
