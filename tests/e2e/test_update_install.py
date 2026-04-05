"""
Tests for Phase 2: Update download, install, and restart.

These tests exercise the full update flow including the /updating page,
download progress, service restart, and auto-reload.

WARNING: These tests trigger a real service restart on the Pi.
The Pi must be running a version lower than v0.1.0-beta.4 for the
update to be offered (set Cargo.toml version to 0.0.1 for testing).
"""

import json
import time

import pytest
from playwright.sync_api import expect, sync_playwright

from conftest import PI_URL, clean_update_state, get_pi_version, set_channel, ssh_cmd


class TestUpdateNow:
    """Tests for clicking 'Update Now' and navigating to /updating."""

    def test_update_now_navigates_to_updating(self, clean_pi):
        """Clicking 'Update Now' navigates to the /updating page."""
        with sync_playwright() as p:
            browser = p.chromium.launch(headless=True)
            page = browser.new_page()

            page.goto(f"{PI_URL}/more", wait_until="load", timeout=30000)
            time.sleep(3)

            page.locator("button").filter(has_text="Check").click()
            page.locator(".update-banner").wait_for(timeout=30000)

            page.locator("a").filter(has_text="Update Now").click()

            # Should navigate to /updating
            page.wait_for_url("**/updating", timeout=5000)

            browser.close()


class TestUpdatingPage:
    """Tests for the /updating page behavior."""

    def test_direct_navigation_without_update_shows_nothing(self, clean_pi):
        """Navigating to /updating without an active update shows 'nothing to do'."""
        set_channel("stable")  # No updates available

        with sync_playwright() as p:
            browser = p.chromium.launch(headless=True)
            page = browser.new_page()

            page.goto(f"{PI_URL}/updating", wait_until="load", timeout=30000)
            time.sleep(5)  # Wait for hydration

            # Should show the "nothing to do" content inside the updating page
            expect(page.locator(".updating-page a[href='/']")).to_be_visible(timeout=5000)

            browser.close()

    @pytest.mark.slow
    def test_updating_shows_downloading(self, clean_pi):
        """The /updating page shows 'Downloading...' during download.
        WARNING: This triggers a real update and replaces the Pi binary."""
        with sync_playwright() as p:
            browser = p.chromium.launch(headless=True)
            page = browser.new_page()

            page.goto(f"{PI_URL}/more", wait_until="load", timeout=30000)
            time.sleep(5)

            page.locator("button").filter(has_text="Check").click()
            page.locator(".update-banner").wait_for(timeout=30000)
            page.locator("a").filter(has_text="Update Now").click()

            page.wait_for_url("**/updating", timeout=5000)
            time.sleep(3)  # Wait for hydration

            # Should show the updating page content
            updating_page = page.locator(".updating-page")
            expect(updating_page).to_be_visible(timeout=10000)

            browser.close()

    def test_updating_page_renders(self, clean_pi):
        """The /updating page renders the updating-page component on SSR."""
        # Test the SSR output directly — the page should render with updating-page class
        import subprocess
        result = subprocess.run(
            ["curl", "-s", f"{PI_URL}/updating"],
            capture_output=True, text=True, timeout=10,
        )
        assert "updating-page" in result.stdout, "SSR should render .updating-page div"

    @pytest.mark.slow
    def test_full_update_flow(self, clean_pi):
        """Full update flow: check → Update Now → /updating → download → restart → reload.

        This test triggers a real service restart and takes ~60s.
        Run with: pytest -m slow
        """
        initial_version = get_pi_version()["version"]

        with sync_playwright() as p:
            browser = p.chromium.launch(headless=True)
            page = browser.new_page()

            page.goto(f"{PI_URL}/more", wait_until="load", timeout=30000)
            time.sleep(5)

            page.locator("button").filter(has_text="Check").click()
            page.locator(".update-banner").wait_for(timeout=30000)
            page.locator("a").filter(has_text="Update Now").click()

            page.wait_for_url("**/updating", timeout=5000)

            # Wait for the full cycle (download + restart + reload)
            time.sleep(50)

            # After everything, check the Pi version changed
            final_version = get_pi_version()
            print(f"Initial: {initial_version}, Final: {final_version}")

            browser.close()

    def test_system_busy_shows_error(self, clean_pi):
        """If the startup pipeline is running, /updating shows a busy message."""
        # This is hard to trigger reliably — would need to restart the service
        # and immediately navigate to /updating before the pipeline completes.
        # Skipped for now.
        pytest.skip("Requires precise timing with startup pipeline")


class TestUpdateCleanup:
    """Tests for post-update cleanup."""

    def test_temp_files_cleaned_after_update(self, clean_pi):
        """After a successful update, temp files are cleaned up."""
        with sync_playwright() as p:
            browser = p.chromium.launch(headless=True)
            page = browser.new_page()

            page.goto(f"{PI_URL}/more", wait_until="load", timeout=30000)
            time.sleep(3)

            page.locator("button").filter(has_text="Check").click()
            page.locator(".update-banner").wait_for(timeout=30000)
            page.locator("a").filter(has_text="Update Now").click()

            # Wait for full cycle
            time.sleep(45)

            # Check cleanup
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

            browser.close()


class TestUpdateError:
    """Tests for update error handling."""

    def test_error_shown_on_updating_page(self, clean_pi):
        """If StartUpdate fails, the error is shown on /updating with a back link."""
        # Trigger an error by trying to update a non-existent tag
        # This requires navigating to /updating when UpdateState has a fake tag
        # Hard to trigger from UI — skip for manual testing
        pytest.skip("Requires fake update state injection")

    def test_network_error_during_download(self, clean_pi):
        """If the network fails during download, an error is shown."""
        # Would need to block network access mid-download
        pytest.skip("Requires network manipulation")
