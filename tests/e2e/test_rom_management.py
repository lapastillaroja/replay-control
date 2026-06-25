"""
End-to-end coverage for ROM rename and delete on the game-detail page.

Both are destructive library operations (admin-gated on device; the standalone
container bypasses auth) that mutate the filesystem under roms/<system>/ and the
library. These drive the inline rename UI (`.game-rename-inline`) and the
two-step delete confirm (`.game-action-delete` -> `.game-action-delete-confirm`),
then assert the on-disk effect. Container only — mutates `/media/usb`. The `page`
fixture also asserts no JS console errors.
"""

import pytest
from playwright.sync_api import expect

from conftest import (
    CONTAINER,
    ROMS_DIR,
    goto_hydrated,
    path_exists,
    wait_until,
)

pytestmark = pytest.mark.skipif(
    not CONTAINER,
    reason="ROM management e2e mutates container storage and is unsafe for Pi targets",
)


def test_rename_rom_renames_file_on_disk(page, seeded_game):
    system, rom = seeded_game["system"], seeded_game["rom_filename"]
    old_path = f"{ROMS_DIR}/{system}/{rom}"
    new_stem = "E2E Renamed Game"
    new_path = f"{ROMS_DIR}/{system}/{new_stem}.nes"
    assert path_exists(old_path)

    goto_hydrated(page, seeded_game["detail_url"])
    page.get_by_role("button", name="Rename").click()
    name_input = page.locator(".game-rename-inline .rename-input")
    expect(name_input).to_be_visible(timeout=10000)
    name_input.fill(new_stem)
    # The first inline button is the confirm (checkmark); on success the page
    # navigates to the renamed game's detail URL.
    page.locator(".game-rename-btns .rom-action-btn").first.click()

    wait_until(lambda: path_exists(new_path) and not path_exists(old_path))
    assert path_exists(new_path), "renamed ROM file should exist on disk"
    assert not path_exists(old_path), "old ROM file should be gone"


def test_delete_rom_removes_file_and_returns_to_system(page, seeded_game):
    system, rom = seeded_game["system"], seeded_game["rom_filename"]
    rom_path = f"{ROMS_DIR}/{system}/{rom}"
    assert path_exists(rom_path)

    goto_hydrated(page, seeded_game["detail_url"])
    page.locator(".game-action-delete").click()
    confirm = page.locator(".game-action-delete-confirm")
    expect(confirm).to_be_visible(timeout=5000)
    confirm.click()

    # Delete navigates back to the system ROM list.
    page.wait_for_url(lambda url: url.rstrip("/").endswith(f"/games/{system}"), timeout=10000)
    wait_until(lambda: not path_exists(rom_path))
    assert not path_exists(rom_path), "deleted ROM file should be gone on disk"
