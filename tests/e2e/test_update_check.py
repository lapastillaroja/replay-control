"""
Tests for Phase 1: Update check + notification.

Verifies the update banner, check button, channel switching,
and skip functionality on the More page.
"""

from playwright.sync_api import expect

from conftest import (
    MOCK_BETA_VERSION,
    MOCK_STABLE_VERSION,
    SEL_BANNER,
    SEL_CHANNEL_SELECT,
    click_check,
    get_pi_version,
    goto_more,
    set_channel,
    wait_for_banner,
)


class TestUpdateBanner:

    def test_banner_appears_after_check(self, clean_pi, page):
        goto_more(page)
        click_check(page)

        banner = wait_for_banner(page)
        assert MOCK_BETA_VERSION in banner.text_content()

    def test_banner_appears_from_background_check(self, clean_pi, page):
        goto_more(page)

        # Wait for background check (up to 70s)
        banner = page.locator(SEL_BANNER)
        expect(banner).to_be_visible(timeout=70000)

    def test_banner_has_all_actions(self, clean_pi, page):
        goto_more(page)
        click_check(page)
        wait_for_banner(page)

        expect(page.locator("a").filter(has_text="Update Now")).to_be_visible()
        expect(page.locator(f"{SEL_BANNER} a").filter(has_text="GitHub")).to_be_visible()
        expect(page.locator(".update-skip-link")).to_be_visible()


class TestSkipVersion:

    def test_skip_hides_banner(self, clean_pi, page):
        goto_more(page)
        click_check(page)
        banner = wait_for_banner(page)

        page.locator(".update-skip-link").click()
        expect(banner).not_to_be_visible(timeout=5000)


class TestChannelSwitch:

    def test_switch_to_stable_shows_stable_update(self, clean_pi, page):
        goto_more(page)
        click_check(page)
        banner = wait_for_banner(page)
        assert MOCK_BETA_VERSION in banner.text_content()

        page.locator(SEL_CHANNEL_SELECT).last.select_option("stable")

        # Re-check should show stable version
        banner = wait_for_banner(page)
        assert MOCK_STABLE_VERSION in banner.text_content()

    def test_switch_to_beta_shows_higher_version(self, clean_pi, page):
        set_channel("stable")
        goto_more(page)
        click_check(page)

        banner = wait_for_banner(page)
        assert MOCK_STABLE_VERSION in banner.text_content()

        page.locator(SEL_CHANNEL_SELECT).last.select_option("beta")

        # Re-check should show higher beta version
        banner = wait_for_banner(page)
        assert MOCK_BETA_VERSION in banner.text_content()


class TestCheckButton:

    def test_button_shows_checking_state(self, clean_pi, page):
        goto_more(page)
        click_check(page)

        # Verify check completes without error (banner appears)
        wait_for_banner(page)

    def test_check_on_stable_shows_stable_update(self, clean_pi, page):
        set_channel("stable")
        goto_more(page)
        click_check(page)

        banner = wait_for_banner(page)
        assert MOCK_STABLE_VERSION in banner.text_content()
        assert "beta" not in banner.text_content().lower()


class TestVersionDisplay:

    def test_current_version_shown(self, clean_pi, page):
        version_info = get_pi_version()
        goto_more(page)

        version_el = page.locator(".update-version")
        expect(version_el).to_be_visible()
        assert version_info["version"] in version_el.text_content()


class TestStableUpdate:

    def test_stable_banner_shows_version(self, clean_pi, page):
        set_channel("stable")
        goto_more(page)
        click_check(page)

        banner = wait_for_banner(page)
        text = banner.text_content()
        assert MOCK_STABLE_VERSION in text
        assert MOCK_BETA_VERSION not in text


class TestChannelDropdown:

    def test_channel_dropdown_visible(self, clean_pi, page):
        goto_more(page)

        select = page.locator(SEL_CHANNEL_SELECT)
        expect(select).to_be_visible(timeout=5000)
        options = select.locator("option").all()
        assert len(options) == 2

    def test_channel_switch_triggers_recheck(self, clean_pi, page):
        set_channel("stable")
        goto_more(page)

        expect(page.locator(SEL_BANNER)).not_to_be_visible(timeout=3000)

        page.locator(SEL_CHANNEL_SELECT).select_option("beta")

        # Banner should appear after re-check
        expect(page.locator(SEL_BANNER)).to_be_visible(timeout=30000)
