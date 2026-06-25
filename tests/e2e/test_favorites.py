"""
End-to-end coverage for the favorites lifecycle.

Favorites are `.fav` marker files under `roms/_favorites/`. This drives the full
loop in one browser session to keep it fast: toggle a favorite on the game-detail
page (`add_favorite`), confirm it persists on disk and shows on `/favorites`,
then remove it from the favorites page (`remove_favorite`) and confirm it's gone.

Container only — mutates `/media/usb`. The `page` fixture also asserts no JS
console errors occurred.
"""

import pytest
from playwright.sync_api import expect

from conftest import (
    CONTAINER,
    FAVORITES_DIR,
    goto_hydrated,
    path_exists,
    wait_until,
)

pytestmark = pytest.mark.skipif(
    not CONTAINER,
    reason="favorites e2e mutates container storage and is unsafe for Pi targets",
)


def test_favorite_add_then_remove_lifecycle(page, seeded_game):
    system, rom = seeded_game["system"], seeded_game["rom_filename"]
    marker = f"{FAVORITES_DIR}/{system}@{rom}.fav"

    # Add via the game-detail toggle.
    goto_hydrated(page, seeded_game["detail_url"])
    fav_btn = page.locator("button.game-action-fav")
    expect(fav_btn).to_be_visible(timeout=15000)
    fav_btn.click()

    wait_until(lambda: path_exists(marker))
    assert path_exists(marker), "favorite marker should be written on disk"

    # Shows on the favorites page.
    goto_hydrated(page, "/favorites")
    expect(page.locator(".fav-item")).to_have_count(1, timeout=15000)

    # Remove via the favorites-page star + inline confirm.
    page.locator(".fav-star-btn").first.click()
    confirm = page.locator(".rom-action-confirm-delete")
    expect(confirm).to_be_visible(timeout=5000)
    confirm.click()

    expect(page.locator(".fav-item")).to_have_count(0, timeout=10000)
    wait_until(lambda: not path_exists(marker))
    assert not path_exists(marker), "favorite marker should be removed on disk"
