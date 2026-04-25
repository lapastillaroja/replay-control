"""
Browser e2e tests for the database-corruption banner over SSE.

Covers what the Rust integration suite can't: the live client-side wire.
Server-side flag transition → ConfigEvent::CorruptionChanged broadcast →
/sse/config delivery → SseConfigListener parses → context signal updates →
<Show> toggles the banner. The Rust tests stop at the broadcast send; these
take over at receive.

Each test corrupts a DB file on the target. The `preserve_*` fixtures
snapshot before and restore after, so the target ends each run in the same
state it started — even on test failure.
"""

import os
import time
from urllib.request import urlopen

import pytest

from conftest import PI_URL, exec_cmd

SEL_CORRUPTION_BANNER = ".corruption-banner"
USER_DATA_DB = "/media/usb/.replay-control/user_data.db"
LIBRARY_DB = "/media/usb/.replay-control/library.db"


def _wait_for_server(timeout_s: int = 30) -> None:
    """Block until the app responds to /api/version, or fail."""
    deadline = time.monotonic() + timeout_s
    while time.monotonic() < deadline:
        try:
            urlopen(f"{PI_URL}/api/version", timeout=2)
            return
        except Exception:
            time.sleep(0.5)
    pytest.fail(f"server did not respond at {PI_URL}/api/version within {timeout_s}s")


def _corrupt_db_and_restart(db_path: str) -> None:
    """Stop service → clobber the SQLite magic header on disk → drop WAL/SHM
    sidecars so SQLite can't replay them against the clobbered main file →
    restart → wait for /api/version. The 4 KiB random write is the same
    shape as a torn write on power loss. Sidecar removal is a no-op when
    journal mode is DELETE (e.g. exFAT)."""
    exec_cmd("systemctl stop replay-control")
    exec_cmd(
        f"dd if=/dev/urandom of={db_path} bs=4096 count=1 conv=notrunc status=none"
    )
    exec_cmd(f"rm -f {db_path}-wal {db_path}-shm {db_path}-journal")
    exec_cmd("systemctl start replay-control")
    _wait_for_server()


def _preserve_db(db_path: str):
    """Generator backing the `preserve_*` fixtures: snapshot db + sidecars,
    yield, restore, restart, wait. Used via `yield from` so each fixture
    function stays a one-liner."""
    db_name = os.path.basename(db_path)
    db_dir = os.path.dirname(db_path)
    snapshot_dir = f"/tmp/replay-control-e2e-snapshot-{db_name}"

    exec_cmd(f"rm -rf {snapshot_dir} && mkdir -p {snapshot_dir}")
    # Sidecars may not exist (DELETE journal mode); ignore those misses, but
    # the main DB file must be there or the test setup is broken.
    exec_cmd(
        f"cp -p {db_path}-wal {db_path}-shm {db_path}-journal "
        f"{snapshot_dir}/ 2>/dev/null || true"
    )
    exec_cmd(f"cp -p {db_path} {snapshot_dir}/")
    if exec_cmd(f"test -f {snapshot_dir}/{db_name} && echo ok") != "ok":
        pytest.fail(f"snapshot setup failed: {db_path} not present in {snapshot_dir}")

    yield

    exec_cmd("systemctl stop replay-control")
    exec_cmd(f"rm -f {db_path} {db_path}-wal {db_path}-shm {db_path}-journal")
    # Glob matches db_name + any -wal/-shm/-journal that came along.
    exec_cmd(f"cp -p {snapshot_dir}/{db_name}* {db_dir}/")
    if exec_cmd(f"test -f {db_path} && echo ok") != "ok":
        pytest.fail(f"snapshot restore failed: {db_path} missing after cp")
    exec_cmd(f"rm -rf {snapshot_dir}")
    exec_cmd("systemctl start replay-control")
    _wait_for_server()


@pytest.fixture()
def preserve_user_data():
    yield from _preserve_db(USER_DATA_DB)


@pytest.fixture()
def preserve_library_db():
    yield from _preserve_db(LIBRARY_DB)


# ── user_data.db ─────────────────────────────────────────────────────────────


def test_banner_appears_after_corrupt_and_restart(page, pi_url, preserve_user_data):
    """Init-payload path: corrupt + restart → banner shows on first load."""
    _corrupt_db_and_restart(USER_DATA_DB)

    page.goto(pi_url, wait_until="load", timeout=30000)

    banner = page.locator(SEL_CORRUPTION_BANNER)
    banner.wait_for(timeout=10000)

    text = banner.inner_text()
    assert "User data is corrupt" in text, f"unexpected banner text: {text!r}"

    # Both action buttons should be present — the auto-saved .bak exists, so
    # Restore is offered alongside Reset.
    assert (
        page.locator("button").filter(has_text="Restore from backup").count() == 1
    ), "Restore from backup button missing"
    assert (
        page.locator("button").filter(has_text="Reset").count() == 1
    ), "Reset button missing"


def test_restore_from_backup_clears_banner_via_sse_push(
    page, pi_url, preserve_user_data
):
    """Clear-direction push: clicking Restore removes the banner without a
    navigation — proves the inverse `CorruptionChanged` event is delivered
    and the SseConfigListener wires it back to the context signal."""
    _corrupt_db_and_restart(USER_DATA_DB)

    page.goto(pi_url, wait_until="load", timeout=30000)
    page.locator(SEL_CORRUPTION_BANNER).wait_for(timeout=10000)

    page.locator("button").filter(has_text="Restore from backup").click()

    # `<Show>` un-renders the banner when `any_corrupt()` flips false. Wait
    # for it to detach, not just hide.
    page.locator(SEL_CORRUPTION_BANNER).wait_for(state="detached", timeout=10000)


def test_reset_clears_banner_via_sse_push(page, pi_url, preserve_user_data):
    """Same path as Restore but exercises `repair_corrupt_user_data`'s
    delete-and-recreate flow on the pool, which also fires the callback via
    `reopen()`."""
    _corrupt_db_and_restart(USER_DATA_DB)

    page.goto(pi_url, wait_until="load", timeout=30000)
    page.locator(SEL_CORRUPTION_BANNER).wait_for(timeout=10000)

    page.locator("button.corruption-banner-btn-danger").click()

    page.locator(SEL_CORRUPTION_BANNER).wait_for(state="detached", timeout=10000)


# ── library.db ───────────────────────────────────────────────────────────────
#
# Library.db corruption is handled differently than user_data.db: it's a
# rebuildable cache, so `LibraryDb::open` silently delete-and-recreates on
# startup corruption (no banner, no user action). The runtime
# mark_corrupt → banner → Rebuild flow exists and is covered by the Rust
# integration suite (`library_mark_corrupt_broadcasts_event`,
# `rebuild_corrupt_library_clears_flag_and_broadcasts_inverse`).
#
# The remaining e2e-only concern is that a clobbered header on disk doesn't
# crash-loop the service at startup before the silent-recreate path runs.


def test_library_clobbered_header_does_not_crash_service(
    page, pi_url, preserve_library_db
):
    """Service must not crash-loop on a clobbered library.db header at
    startup. `LibraryDb::open`'s pre-flight should detect the bad header
    and delete the file before `Connection::open` errors."""
    _corrupt_db_and_restart(LIBRARY_DB)

    page.goto(pi_url, wait_until="load", timeout=30000)
    page.wait_for_selector("body", timeout=10000)

    # No banner: library startup corruption is silent (rebuildable cache).
    assert page.locator(SEL_CORRUPTION_BANNER).count() == 0, (
        "library startup corruption should not surface a banner — "
        "LibraryDb::open should silently delete + recreate"
    )
