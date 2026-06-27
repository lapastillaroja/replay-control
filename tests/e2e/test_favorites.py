"""
End-to-end coverage for the favorites lifecycle.

Favorites are `.fav` marker files under `roms/_favorites/`. This drives the full
loop in one browser session to keep it fast: toggle a favorite on the game-detail
page (`add_favorite`), confirm it persists on disk and shows on `/favorites`,
then remove it through the app confirmation dialog (`remove_favorite`) and
confirm it's gone.

Container only — mutates `/media/usb`. The `page` fixture also asserts no JS
console errors occurred.
"""

import pytest
from playwright.sync_api import expect

from conftest import (
    CONTAINER,
    FAVORITES_DIR,
    exec_cmd,
    goto_hydrated,
    path_exists,
    seed_favorite,
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

    # Remove via the favorites-page star + app confirm dialog.
    page.locator(".fav-star-btn").first.click()
    dialog = page.locator(".app-confirm-dialog")
    expect(dialog).to_be_visible(timeout=5000)
    expect(dialog).to_contain_text("E2E Seed Game")

    # Cancel first so the dialog test proves the action is actually gated.
    dialog.get_by_role("button", name="Cancel").click()
    expect(dialog).to_be_hidden(timeout=5000)
    assert path_exists(marker), "favorite marker should remain after cancelling removal"

    page.locator(".fav-star-btn").first.click()
    expect(dialog).to_be_visible(timeout=5000)
    dialog.get_by_role("button", name="Unfavorite").click()

    expect(page.locator(".fav-item")).to_have_count(0, timeout=10000)
    wait_until(lambda: not path_exists(marker))
    assert not path_exists(marker), "favorite marker should be removed on disk"


def _fav_marker_depths(marker: str) -> list[int]:
    """Subfolder depth of each .fav copy under _favorites: 0=root, 1=one
    subfolder, 2=double-nested. (Organize keeps a root copy by default, so there
    can be more than one.)"""
    out = exec_cmd(f'find "{FAVORITES_DIR}" -name "{marker}"')
    return [
        line[len(FAVORITES_DIR):].strip("/").count("/")
        for line in out.splitlines()
        if line.strip()
    ]


def test_organize_by_system_and_board_collapses_console(page, seeded_game):
    # Organizing a console favorite by System + Board must not double-nest: Board
    # falls back to the system name, so it collapses to a single <System>/ level
    # (depth 1), never <System>/<System>/ (depth 2).
    system, rom = seeded_game["system"], seeded_game["rom_filename"]
    marker = f"{system}@{rom}.fav"
    seed_favorite(system, rom)

    goto_hydrated(page, "/favorites")
    page.locator(".organize-toggle").click()
    selects = page.locator("select.form-input")
    expect(selects.first).to_be_visible(timeout=10000)
    selects.nth(0).select_option("system")
    selects.nth(1).select_option("board")
    page.get_by_role("button", name="Organize", exact=True).click()

    wait_until(lambda: 1 in _fav_marker_depths(marker))
    depths = _fav_marker_depths(marker)
    assert 1 in depths, f"favorite should be organized into one subfolder, got depths={depths}"
    assert 2 not in depths, f"favorite must not be double-nested, got depths={depths}"
