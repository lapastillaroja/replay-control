"""
End-to-end coverage for user screenshot (capture) deletion.

Captures are read live off disk (`captures/<system>/<rom>_<ts>.png`) by the
game-detail page, so these tests seed PNG files directly and then drive the two
delete affordances the UI offers:

  - the per-thumbnail "x" button (`.capture-delete-btn`)
  - the lightbox delete button (`.lightbox-delete`)

Both go through the app confirmation dialog and the `delete_user_capture` server
fn, which removes the file from disk. Container only — the fixtures mutate
`/media/usb`.
"""

import pytest
from playwright.sync_api import expect

from conftest import (
    CAPTURES_DIR,
    CONTAINER,
    confirm_in_app_dialog,
    goto_hydrated,
    list_files,
    path_exists,
    seed_capture,
    wait_until,
)

pytestmark = pytest.mark.skipif(
    not CONTAINER,
    reason="capture e2e mutates container storage and is unsafe for Pi targets",
)


def test_seeded_capture_renders_on_game_detail(page, seeded_game):
    seed_capture(seeded_game["system"], seeded_game["rom_filename"])
    goto_hydrated(page, seeded_game["detail_url"])

    expect(page.locator(".screenshot-card-capture")).to_have_count(1, timeout=15000)
    expect(page.locator(".capture-delete-btn")).to_have_count(1)


def test_delete_capture_via_thumbnail_removes_file(page, seeded_game):
    system, rom = seeded_game["system"], seeded_game["rom_filename"]
    name = seed_capture(system, rom)
    capture_path = f"{CAPTURES_DIR}/{system}/{name}"
    assert path_exists(capture_path)

    goto_hydrated(page, seeded_game["detail_url"])

    expect(page.locator(".screenshot-card-capture")).to_have_count(1, timeout=15000)
    page.locator(".capture-delete-btn").first.click()
    confirm_in_app_dialog(page, "Delete")

    # Card disappears optimistically; file is removed by the server fn.
    expect(page.locator(".screenshot-card-capture")).to_have_count(0, timeout=10000)
    wait_until(lambda: not path_exists(capture_path))
    assert not path_exists(capture_path), "capture file should be deleted on disk"


def test_delete_capture_via_lightbox_removes_file(page, seeded_game):
    system, rom = seeded_game["system"], seeded_game["rom_filename"]
    name = seed_capture(system, rom)
    capture_path = f"{CAPTURES_DIR}/{system}/{name}"

    goto_hydrated(page, seeded_game["detail_url"])

    # Open the lightbox by tapping the capture thumbnail.
    thumb = page.locator(".screenshot-card-capture .screenshot-thumb-tappable")
    expect(thumb).to_have_count(1, timeout=15000)
    thumb.first.click()

    lightbox_delete = page.locator(".lightbox-overlay .lightbox-delete")
    expect(lightbox_delete).to_be_visible(timeout=5000)
    lightbox_delete.click()
    confirm_in_app_dialog(page, "Delete")

    wait_until(lambda: not path_exists(capture_path))
    assert not path_exists(capture_path), "capture file should be deleted on disk"


def test_delete_one_capture_keeps_the_others(page, seeded_game):
    system, rom = seeded_game["system"], seeded_game["rom_filename"]
    keep = seed_capture(system, rom, suffix="_20260101_120000")
    drop = seed_capture(system, rom, suffix="_20260102_130000")
    captures_dir = f"{CAPTURES_DIR}/{system}"
    assert len(list_files(captures_dir)) == 2

    goto_hydrated(page, seeded_game["detail_url"])

    expect(page.locator(".screenshot-card-capture")).to_have_count(2, timeout=15000)
    # Newest capture sorts first; deleting index 0 removes one, leaving one.
    page.locator(".capture-delete-btn").first.click()
    confirm_in_app_dialog(page, "Delete")
    expect(page.locator(".screenshot-card-capture")).to_have_count(1, timeout=10000)

    wait_until(lambda: len(list_files(captures_dir)) == 1)
    remaining = list_files(captures_dir)
    assert len(remaining) == 1, f"exactly one capture should remain, found {remaining}"
    assert remaining[0] in (keep, drop)
