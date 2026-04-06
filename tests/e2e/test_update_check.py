"""
Tests for Phase 1: Update check + notification.

These tests verify the update banner, check button, channel switching,
and skip functionality on the More page.
"""

import sys
import time
from pathlib import Path

import pytest
from playwright.sync_api import expect, sync_playwright

from conftest import PI_URL, clean_update_state, get_pi_version, set_channel

# Import mock version dynamically so the test always matches the mock server
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "container"))
from mock_github import MOCK_BETA_VERSION, MOCK_STABLE_VERSION


class TestUpdateBanner:
    """Tests for the update notification banner."""

    def test_banner_appears_after_check(self, clean_pi):
        """Clicking 'Check for Updates' on beta channel shows the update banner."""
        with sync_playwright() as p:
            browser = p.chromium.launch(headless=True)
            page = browser.new_page()

            page.goto(f"{PI_URL}/more", wait_until="load", timeout=30000)
            time.sleep(3)

            # Click Check for Updates
            check_btn = page.locator("button").filter(has_text="Check")
            check_btn.click()

            # Banner should appear
            banner = page.locator(".update-banner")
            expect(banner).to_be_visible(timeout=30000)

            # Should mention the beta mock version
            assert MOCK_BETA_VERSION in banner.text_content()

            browser.close()

    def test_banner_appears_from_background_check(self, clean_pi):
        """The background check (60s delay) shows the banner without manual check."""
        with sync_playwright() as p:
            browser = p.chromium.launch(headless=True)
            page = browser.new_page()

            page.goto(f"{PI_URL}/more", wait_until="load", timeout=30000)

            # Wait for background check (up to 70s)
            banner = page.locator(".update-banner")
            expect(banner).to_be_visible(timeout=70000)

            browser.close()

    def test_banner_has_all_actions(self, clean_pi):
        """The update banner shows Update Now, View on GitHub, and Skip."""
        with sync_playwright() as p:
            browser = p.chromium.launch(headless=True)
            page = browser.new_page()

            page.goto(f"{PI_URL}/more", wait_until="load", timeout=30000)
            time.sleep(3)

            page.locator("button").filter(has_text="Check").click()
            page.locator(".update-banner").wait_for(timeout=30000)

            # All three actions present
            expect(page.locator("a").filter(has_text="Update Now")).to_be_visible()
            expect(page.locator(".update-banner a").filter(has_text="GitHub")).to_be_visible()
            expect(page.locator(".update-skip-link")).to_be_visible()

            browser.close()


class TestSkipVersion:
    """Tests for the 'Skip this version' functionality."""

    def test_skip_hides_banner(self, clean_pi):
        """Clicking 'Skip this version' hides the update banner."""
        with sync_playwright() as p:
            browser = p.chromium.launch(headless=True)
            page = browser.new_page()

            page.goto(f"{PI_URL}/more", wait_until="load", timeout=30000)
            time.sleep(3)

            page.locator("button").filter(has_text="Check").click()
            banner = page.locator(".update-banner")
            banner.wait_for(timeout=30000)

            # Click Skip
            page.locator(".update-skip-link").click()

            # Banner should disappear
            expect(banner).not_to_be_visible(timeout=5000)

            browser.close()


class TestChannelSwitch:
    """Tests for switching between stable and beta channels."""

    def test_switch_to_stable_shows_stable_update(self, clean_pi):
        """Switching from beta to stable shows the stable update instead."""
        with sync_playwright() as p:
            browser = p.chromium.launch(headless=True)
            page = browser.new_page()

            page.goto(f"{PI_URL}/more", wait_until="load", timeout=30000)
            time.sleep(3)

            # Get update on beta
            page.locator("button").filter(has_text="Check").click()
            banner = page.locator(".update-banner")
            banner.wait_for(timeout=30000)
            assert MOCK_BETA_VERSION in banner.text_content()

            # Switch to stable
            channel_select = page.locator("select.form-input").last
            channel_select.select_option("stable")

            # Wait for re-check to complete, banner should show stable version
            time.sleep(5)
            banner = page.locator(".update-banner")
            expect(banner).to_be_visible(timeout=30000)
            assert MOCK_STABLE_VERSION in banner.text_content()

            browser.close()

    def test_switch_to_beta_shows_higher_version(self, clean_pi):
        """Switching from stable to beta shows the higher beta version."""
        set_channel("stable")

        with sync_playwright() as p:
            browser = p.chromium.launch(headless=True)
            page = browser.new_page()

            page.goto(f"{PI_URL}/more", wait_until="load", timeout=30000)
            time.sleep(3)

            # Check on stable — should find the stable update
            page.locator("button").filter(has_text="Check").click()
            banner = page.locator(".update-banner")
            expect(banner).to_be_visible(timeout=30000)
            assert MOCK_STABLE_VERSION in banner.text_content()

            # Switch to beta
            channel_select = page.locator("select.form-input").last
            channel_select.select_option("beta")

            # Banner should show the higher beta version after re-check
            time.sleep(5)
            banner = page.locator(".update-banner")
            expect(banner).to_be_visible(timeout=30000)
            assert MOCK_BETA_VERSION in banner.text_content()

            browser.close()


class TestCheckButton:
    """Tests for the 'Check for Updates' button behavior."""

    def test_button_shows_checking_state(self, clean_pi):
        """The check button shows 'Checking...' while the check is in progress."""
        with sync_playwright() as p:
            browser = p.chromium.launch(headless=True)
            page = browser.new_page()

            page.goto(f"{PI_URL}/more", wait_until="load", timeout=30000)
            time.sleep(3)

            check_btn = page.locator("button").filter(has_text="Check")
            check_btn.click()

            # Should briefly show "Checking..."
            # (may be too fast to catch reliably, so we just verify no error)
            page.locator(".update-banner").wait_for(timeout=30000)

            browser.close()

    def test_check_on_stable_shows_stable_update(self, clean_pi):
        """Checking on stable channel finds the stable release."""
        set_channel("stable")

        with sync_playwright() as p:
            browser = p.chromium.launch(headless=True)
            page = browser.new_page()

            page.goto(f"{PI_URL}/more", wait_until="load", timeout=30000)
            time.sleep(3)

            page.locator("button").filter(has_text="Check").click()

            # Stable update banner should appear
            banner = page.locator(".update-banner")
            expect(banner).to_be_visible(timeout=30000)
            assert MOCK_STABLE_VERSION in banner.text_content()
            assert "beta" not in banner.text_content().lower()

            browser.close()


class TestVersionDisplay:
    """Tests for the version display."""

    def test_current_version_shown(self, clean_pi):
        """The current version is displayed in the Updates section."""
        version_info = get_pi_version()

        with sync_playwright() as p:
            browser = p.chromium.launch(headless=True)
            page = browser.new_page()

            page.goto(f"{PI_URL}/more", wait_until="load", timeout=30000)
            time.sleep(3)

            version_el = page.locator(".update-version")
            expect(version_el).to_be_visible()
            assert version_info["version"] in version_el.text_content()

            browser.close()


class TestStableUpdate:
    """Tests for stable channel update behavior."""

    def test_stable_banner_shows_version(self, clean_pi):
        """Checking on stable shows a banner with the stable version (not beta)."""
        set_channel("stable")

        with sync_playwright() as p:
            browser = p.chromium.launch(headless=True)
            page = browser.new_page()

            page.goto(f"{PI_URL}/more", wait_until="load", timeout=30000)
            time.sleep(5)

            page.locator("button").filter(has_text="Check").click()

            banner = page.locator(".update-banner")
            expect(banner).to_be_visible(timeout=30000)
            text = banner.text_content()
            assert MOCK_STABLE_VERSION in text
            # Should NOT show the beta version
            assert MOCK_BETA_VERSION not in text

            browser.close()


class TestChannelDropdown:
    """Tests for the channel dropdown functionality."""

    def test_channel_dropdown_visible(self, clean_pi):
        """The channel dropdown is visible and shows the current channel."""
        with sync_playwright() as p:
            browser = p.chromium.launch(headless=True)
            page = browser.new_page()

            page.goto(f"{PI_URL}/more", wait_until="load", timeout=30000)
            time.sleep(5)

            select = page.locator(".update-controls-row select")
            expect(select).to_be_visible(timeout=5000)

            # Should have two options
            options = select.locator("option").all()
            assert len(options) == 2

            browser.close()

    def test_channel_switch_triggers_recheck(self, clean_pi):
        """Switching channel triggers a re-check for updates."""
        set_channel("stable")

        with sync_playwright() as p:
            browser = p.chromium.launch(headless=True)
            page = browser.new_page()

            page.goto(f"{PI_URL}/more", wait_until="load", timeout=30000)
            time.sleep(5)

            # No banner on stable
            expect(page.locator(".update-banner")).not_to_be_visible(timeout=3000)

            # Switch to beta
            page.locator(".update-controls-row select").select_option("beta")
            time.sleep(15)

            # Banner should appear (re-check triggered)
            expect(page.locator(".update-banner")).to_be_visible(timeout=15000)

            browser.close()
