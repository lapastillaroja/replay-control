"""
End-to-end coverage for settings preference persistence (skin + locale).

These exercise the full set/persist/reload wiring for two user preferences. The
`clean_settings` fixture snapshots and restores settings.cfg so the change does
not leak into other tests. Container only (standalone mode — auth bypassed).
"""

import pytest
from playwright.sync_api import expect

from conftest import (
    CONTAINER,
    RC_DIR,
    exec_cmd,
    goto_hydrated,
    wait_hydrated,
)

pytestmark = pytest.mark.skipif(
    not CONTAINER,
    reason="settings-prefs e2e mutates container settings; container only",
)


def test_skin_selection_persists(page, clean_settings):
    goto_hydrated(page, "/settings/skin")
    expect(page.locator(".skin-card").first).to_be_visible(timeout=15000)

    # Skin cards are disabled while "Sync with ReplayOS" is on; turn it off first
    # (set_skin_sync reloads the page).
    sync_toggle = page.locator(".form-checkbox")
    if sync_toggle.is_checked():
        sync_toggle.click()

    # After sync is off (and the reload settles) pick an enabled, non-active skin.
    inactive = page.locator(".skin-card:not(.skin-card-active):not([disabled])").first
    expect(inactive).to_be_visible(timeout=20000)
    chosen = inactive.locator(".skin-name").inner_text()
    inactive.click()

    # set_skin reloads the page to apply the theme; the chosen skin is now active.
    expect(page.locator(".skin-card.skin-card-active .skin-name")).to_have_text(
        chosen, timeout=20000
    )

    # Persists across a fresh load.
    goto_hydrated(page, "/settings/skin")
    expect(page.locator(".skin-card.skin-card-active .skin-name")).to_have_text(
        chosen, timeout=15000
    )


def test_locale_selection_persists(page, clean_settings):
    goto_hydrated(page, "/settings")
    # The locale <select> is the one offering the "Spanish - Español" option.
    locale_select = page.locator("#settings-appearance select.form-input").filter(
        has=page.locator("option", has_text="Español")
    )
    expect(locale_select).to_be_visible(timeout=15000)
    locale_select.select_option("es")

    # save_locale persists the choice; a fresh load reflects it.
    goto_hydrated(page, "/settings")
    reloaded = page.locator("#settings-appearance select.form-input").filter(
        has=page.locator("option", has_text="Español")
    )
    expect(reloaded).to_have_value("es", timeout=15000)

    locale_line = exec_cmd(f'grep locale "{RC_DIR}/settings.cfg"')
    assert "es" in locale_line, f"locale should be persisted as es, got: {locale_line!r}"
