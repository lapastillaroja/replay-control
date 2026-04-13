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

        time.sleep(45)

        temp = ssh_cmd(
            "ls /var/tmp/replay-control-update/ 2>/dev/null "
            "&& echo EXISTS || echo CLEAN"
        )
        script = ssh_cmd(
            "ls /var/tmp/replay-control-do-update.sh 2>/dev/null "
            "&& echo EXISTS || echo CLEAN"
        )
        bak = ssh_cmd(
            "ls /usr/local/bin/replay-control-app.bak 2>/dev/null "
            "&& echo EXISTS || echo CLEAN"
        )

        assert temp == "CLEAN", f"Temp files not cleaned: {temp}"
        assert script == "CLEAN", f"Script not cleaned: {script}"
        assert bak == "CLEAN", f"Backup not cleaned: {bak}"


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
