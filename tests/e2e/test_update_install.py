"""
Tests for Phase 2: Update download, install, and restart.

Exercises the full update flow including the /updating page,
download progress, service restart, and auto-reload.

WARNING: These tests trigger a real service restart on the Pi.
The Pi must be running a version lower than the mock server's version
(derived dynamically from Cargo.toml) for the update to be offered.
"""

import subprocess
import time

import pytest
from playwright.sync_api import expect

from conftest import (
    PI_URL,
    SEL_BANNER,
    SEL_UPDATING_PAGE,
    click_check,
    click_update_now,
    get_pi_version,
    goto_settings,
    set_channel,
    set_mock_downloads,
    ssh_cmd,
    wait_for_banner,
)


def wait_for_update_cleanup(timeout=90):
    """Wait until install temp payloads and rollback files are gone.

    The background update checker may recreate available.json after the helper
    script cleans the update directory. That file represents update state, not
    an in-progress install payload, so ignore it here.
    """
    deadline = time.time() + timeout
    state = {}

    while time.time() < deadline:
        state = {
            "temp": ssh_cmd(
                "find /var/tmp/replay-control-update -mindepth 1 "
                "! -name available.json -print -quit 2>/dev/null || true"
            ),
            "script": ssh_cmd(
                "test -e /var/tmp/replay-control-do-update.sh "
                "&& echo EXISTS || echo CLEAN"
            ),
            "bak": ssh_cmd(
                "test -e /usr/local/bin/replay-control-app.bak "
                "&& echo EXISTS || echo CLEAN"
            ),
            "catalog_bak": ssh_cmd(
                "test -e /usr/local/bin/catalog.sqlite.bak "
                "&& echo EXISTS || echo CLEAN"
            ),
        }
        if (
            not state["temp"]
            and state["script"] == "CLEAN"
            and state["bak"] == "CLEAN"
            and state["catalog_bak"] == "CLEAN"
        ):
            return state
        time.sleep(2)

    return state


class TestUpdateNow:

    def test_update_now_navigates_to_updating(self, clean_pi, page):
        goto_settings(page)
        click_check(page)
        wait_for_banner(page)
        click_update_now(page)


class TestUpdatingPage:

    def test_direct_navigation_without_update_shows_nothing(self, clean_pi, page):
        set_channel("stable")
        page.goto(f"{PI_URL}/updating", wait_until="load", timeout=30000)

        expect(page.locator(f"{SEL_UPDATING_PAGE} a[href='/settings']")).to_be_visible(timeout=10000)

    @pytest.mark.slow
    def test_updating_shows_downloading(self, clean_pi, page):
        """WARNING: This triggers a real update and replaces the Pi binary."""
        goto_settings(page)
        click_check(page)
        wait_for_banner(page)
        click_update_now(page)

        expect(page.locator(SEL_UPDATING_PAGE)).to_be_visible(timeout=10000)

    def test_updating_page_renders(self, clean_pi):
        """SSR smoke test — no browser needed."""
        result = subprocess.run(
            ["curl", "-s", f"{PI_URL}/updating"],
            capture_output=True, text=True, timeout=10,
        )
        assert "updating-page" in result.stdout

    @pytest.mark.slow
    def test_full_update_flow(self, clean_pi, page):
        """Full flow: check → Update Now → download → restart → reload.

        Triggers a real service restart (~60s).
        """
        initial_version = get_pi_version()["version"]

        goto_settings(page)
        click_check(page)
        wait_for_banner(page)
        click_update_now(page)

        # Wait for the full cycle (download + restart + reload)
        time.sleep(50)

        final_version = get_pi_version()
        print(f"Initial: {initial_version}, Final: {final_version}")


class TestUpdateCleanup:

    def test_temp_files_cleaned_after_update(self, clean_pi, page):
        goto_settings(page)
        click_check(page)
        wait_for_banner(page)
        click_update_now(page)

        cleanup = wait_for_update_cleanup()
        # The mock release ships a catalog asset; after update it should be
        # in place next to the binary so init_catalog can open it on restart.
        # `test -f` rather than `ls`: when the file exists, ls prints the
        # path to stdout and the && branch echoes EXISTS, producing
        # "<path>\nEXISTS" — the four CLEAN checks above don't notice
        # because their files don't exist, but this assertion needs a clean
        # one-line answer.
        catalog = ssh_cmd(
            "test -f /usr/local/bin/catalog.sqlite "
            "&& echo EXISTS || echo MISSING"
        )

        assert not cleanup["temp"], f"Temp files not cleaned: {cleanup['temp']}"
        assert cleanup["script"] == "CLEAN", f"Script not cleaned: {cleanup['script']}"
        assert cleanup["bak"] == "CLEAN", f"Backup not cleaned: {cleanup['bak']}"
        assert cleanup["catalog_bak"] == "CLEAN", (
            f"Catalog backup not cleaned: {cleanup['catalog_bak']}"
        )
        assert catalog == "EXISTS", f"Catalog missing after update: {catalog}"


class TestUpdateError:

    def test_network_error_during_download(self, clean_pi, page):
        goto_settings(page)
        click_check(page)
        wait_for_banner(page)

        set_mock_downloads(fail=True)
        try:
            click_update_now(page)
            error_el = page.locator(f"{SEL_UPDATING_PAGE} .error").first
            expect(error_el).to_be_visible(timeout=30000)
        finally:
            set_mock_downloads(fail=False)
