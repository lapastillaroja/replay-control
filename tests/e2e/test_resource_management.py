"""
End-to-end coverage for the data/resource-management actions on the game-library
settings page (`/settings/game-library`).

The destructive actions live behind the "Advanced" disclosure as `ClearActionCard`
widgets: a first click reveals an inline confirm (`.game-delete-confirm`), the
danger button runs the server fn, and a result line (`.manage-action-result`)
reports the outcome. Covered actions: Clear Downloaded Images, Cleanup Orphaned
Images, Clear Thumbnail Index, Clear Metadata — plus the cancel path.

Container only — these mutate `/media/usb` and call maintenance server fns.
"""

import pytest
from playwright.sync_api import expect

from conftest import (
    CONTAINER,
    MEDIA_DIR,
    exec_cmd,
    goto_hydrated,
    path_exists,
    wait_for_activity_idle,
    wait_until,
)

pytestmark = pytest.mark.skipif(
    not CONTAINER,
    reason="resource-management e2e mutates container storage and is unsafe for Pi targets",
)


def _open_advanced(page):
    goto_hydrated(page, "/settings/game-library")
    page.locator(".manage-actions.is-hydrated").wait_for(timeout=20000)
    page.get_by_role("button", name="Advanced").click()
    page.locator(".manage-actions-grid").wait_for(timeout=10000)


def _run_action(page, label: str):
    """Click an action's label, confirm the danger button, return its result locator."""
    page.get_by_role("button", name=label).click()
    confirm = page.locator(".manage-actions-grid .game-delete-confirm .form-btn-danger")
    expect(confirm).to_be_visible(timeout=5000)
    confirm.click()


def _seed_media_file() -> str:
    """Drop a fake downloaded-image file under the media dir so Clear has work."""
    marker = f"{MEDIA_DIR}/nintendo_nes/boxart.png"
    exec_cmd(f'mkdir -p "{MEDIA_DIR}/nintendo_nes" && printf x > "{marker}"')
    return marker


def test_clear_downloaded_images_removes_media(page, seeded_game):
    wait_for_activity_idle(timeout=30)
    marker = _seed_media_file()
    assert path_exists(marker)

    _open_advanced(page)
    _run_action(page, "Clear Downloaded Images")

    expect(page.locator(".manage-action-result")).to_be_visible(timeout=20000)
    wait_until(lambda: not path_exists(marker), timeout=15)
    assert not path_exists(marker), "Clear Downloaded Images should remove the media dir"


def test_cleanup_orphaned_images_reports_result(page, seeded_game):
    wait_for_activity_idle(timeout=30)
    _open_advanced(page)
    _run_action(page, "Cleanup Orphaned Images")
    expect(page.locator(".manage-action-result")).to_be_visible(timeout=20000)


def test_clear_metadata_reports_result(page, seeded_game):
    wait_for_activity_idle(timeout=30)
    _open_advanced(page)
    _run_action(page, "Clear Metadata")
    expect(page.locator(".manage-action-result")).to_be_visible(timeout=20000)


def test_clear_thumbnail_index_reports_result(page, seeded_game):
    wait_for_activity_idle(timeout=30)
    _open_advanced(page)
    _run_action(page, "Clear Thumbnail Index")
    expect(page.locator(".manage-action-result")).to_be_visible(timeout=20000)


def test_cancel_keeps_media(page, seeded_game):
    wait_for_activity_idle(timeout=30)
    marker = _seed_media_file()
    assert path_exists(marker)

    _open_advanced(page)
    page.get_by_role("button", name="Clear Downloaded Images").click()
    cancel = page.locator(".manage-actions-grid .game-delete-confirm").get_by_role(
        "button", name="Cancel"
    )
    expect(cancel).to_be_visible(timeout=5000)
    cancel.click()

    # Nothing ran: the media file is still there and no result line is shown.
    assert path_exists(marker), "cancelling must not delete media"
