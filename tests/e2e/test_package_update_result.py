"""Browser result display after a package update reconnects."""

import os

import pytest
from playwright.sync_api import expect

from conftest import goto_hydrated

pytestmark = pytest.mark.skipif(
    "REPLAY_PACKAGE_VERSION" not in os.environ,
    reason="requires the package integration runner",
)


def _open_pending_update(page):
    version = os.environ["REPLAY_PACKAGE_VERSION"]
    goto_hydrated(page, "/", timeout=60000)
    if page.locator("#login-admin-password").count():
        password = page.locator("#login-admin-password")
        password.fill(os.environ.get("PI_PASS", "replayos"))
        page.locator("form").filter(has=password).locator("button").click()
        expect(password).to_have_count(0, timeout=15000)
        goto_hydrated(page, "/", timeout=60000)
    page.evaluate(
        "version => sessionStorage.setItem('replay-control-pending-update-version', version)",
        version,
    )
    goto_hydrated(page, "/updating", timeout=60000)


def test_package_update_success_visible_after_reconnect(page):
    _open_pending_update(page)
    expect(page.locator(".updating-page")).to_contain_text(
        "Update complete", timeout=45000
    )


def test_package_update_result_uses_browser_language(page):
    page.context.set_extra_http_headers({"Accept-Language": "es"})
    _open_pending_update(page)
    expect(page.locator(".updating-page")).to_contain_text(
        "Actualización completada", timeout=45000
    )
    expect(page.locator(".updating-page a")).to_have_text("Continuar")


def test_package_update_failure_visible_after_reconnect(page):
    page.route(
        "**/api/update-result",
        lambda route: route.fulfill(
            status=200,
            content_type="text/plain",
            body="failed:APT is busy. Retry when the other package operation finishes.\n",
        ),
    )
    _open_pending_update(page)
    expect(page.locator(".updating-page")).to_contain_text(
        "APT is busy", timeout=45000
    )
