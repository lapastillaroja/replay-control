"""
End-to-end coverage for "recently played" deletion.

Recents are `.rec` marker files under `roms/_recent/`, read live off disk by the
home page. These tests seed markers, confirm they surface on the home page, and
drive the per-card delete button (`.recent-delete-btn`) which goes through a JS
`confirm()` and the `delete_recent` server fn (removes the marker file).

Container only — the fixtures mutate `/media/usb`.
"""

import pytest
from playwright.sync_api import expect

from conftest import (
    CONTAINER,
    RECENTS_DIR,
    accept_dialogs,
    goto_hydrated,
    list_files,
    path_exists,
    seed_recent,
    seed_rom,
    wait_until,
)

pytestmark = pytest.mark.skipif(
    not CONTAINER,
    reason="recents e2e mutates container storage and is unsafe for Pi targets",
)


def _rec_markers() -> list[str]:
    return [f for f in list_files(RECENTS_DIR) if f.endswith(".rec")]


def test_seeded_recent_renders_on_home(page, seeded_game):
    system, rom = seeded_game["system"], seeded_game["rom_filename"]
    seed_recent(system, rom)

    goto_hydrated(page, "/")
    expect(page.locator(".recent-delete-btn").first).to_be_visible(timeout=15000)


def test_delete_recent_removes_marker(page, seeded_game):
    system, rom = seeded_game["system"], seeded_game["rom_filename"]
    seed_recent(system, rom)
    marker_path = f"{RECENTS_DIR}/{system}@{rom}.rec"
    assert path_exists(marker_path)

    accept_dialogs(page)
    goto_hydrated(page, "/")

    delete_btn = page.locator(".recent-delete-btn").first
    expect(delete_btn).to_be_visible(timeout=15000)
    delete_btn.click()

    wait_until(lambda: not path_exists(marker_path))
    assert not path_exists(marker_path), "recent marker should be deleted on disk"


def test_delete_one_recent_keeps_others(page, seeded_game):
    system, rom = seeded_game["system"], seeded_game["rom_filename"]
    # A second seeded ROM + recent so the list has two entries.
    rom2 = "E2E Second Game.nes"
    seed_rom(system, rom2)
    seed_recent(system, rom)
    seed_recent(system, rom2)
    assert len(_rec_markers()) == 2

    accept_dialogs(page)
    goto_hydrated(page, "/")

    expect(page.locator(".recent-delete-btn")).to_have_count(2, timeout=15000)
    page.locator(".recent-delete-btn").first.click()
    expect(page.locator(".recent-delete-btn")).to_have_count(1, timeout=10000)

    wait_until(lambda: len(_rec_markers()) == 1)
    remaining = _rec_markers()
    assert len(remaining) == 1, f"exactly one recent marker should remain, found {remaining}"
