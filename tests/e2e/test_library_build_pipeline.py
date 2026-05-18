"""
End-to-end checks for the library build activity flow.

These tests intentionally run only in the container e2e environment. They
mutate `/media/usb`, restart the service, and drive the same server functions
the UI uses, so they should not run against a developer's real Pi library.
"""

import json
import subprocess
import time
from http.client import IncompleteRead
from urllib.request import urlopen

import pytest
from playwright.sync_api import expect

from conftest import CONTAINER, CONTAINER_ENGINE, PI_URL

pytestmark = pytest.mark.skipif(
    not CONTAINER,
    reason="library build e2e mutates container storage and is unsafe for Pi targets",
)


def _wait_for_app(timeout=60):
    deadline = time.monotonic() + timeout
    last_error = None
    while time.monotonic() < deadline:
        try:
            with urlopen(f"{PI_URL}/api/version", timeout=2) as resp:
                if resp.status == 200:
                    return
        except Exception as exc:
            last_error = exc
        time.sleep(0.5)
    raise AssertionError(f"app did not become ready: {last_error}")


@pytest.fixture()
def isolated_container_library():
    _reset_container_library(worker_count=1)
    yield
    _reset_container_library(worker_count=2)


def _exec_checked(cmd: str, timeout: int = 30) -> str:
    engine = CONTAINER_ENGINE or "podman"
    result = subprocess.run(
        [engine, "exec", CONTAINER, "bash", "-c", cmd],
        capture_output=True,
        text=True,
        timeout=timeout,
        check=False,
    )
    if result.returncode != 0:
        raise AssertionError(
            f"container command failed with {result.returncode}\n"
            f"stdout:\n{result.stdout}\n"
            f"stderr:\n{result.stderr}"
        )
    return result.stdout.strip()


def _reset_container_library(worker_count: int):
    logs_before = _container_logs()
    _exec_checked(
        f"""
        systemctl stop replay-control >/dev/null 2>&1 || true
        printf 'PORT=8080\\nREPLAY_CONTROL_IDENTITY_WORKERS={worker_count}\\n' \
            > /etc/default/replay-control
        rm -rf /media/usb/roms/* \
            /media/usb/.replay-control-data/storages \
            /media/usb/.replay-control/library.db \
            /media/usb/.replay-control/library.db-* \
            /media/usb/.replay-control/media
        mkdir -p /media/usb/roms /media/usb/.replay-control/media
        printf 'setup_dismissed = "true"\\n' > /media/usb/.replay-control/settings.cfg
        systemctl start replay-control >/dev/null
        """,
        timeout=30,
    )
    _wait_for_app(timeout=90)
    _wait_for_new_log(logs_before, "L2 populate: done", timeout=120)


def _create_hash_workload(count=80, size_mb=2):
    _exec_checked(
        f"""
        set -e
        roms=/media/usb/roms/nintendo_nes
        mkdir -p "$roms"
        for i in $(seq 0 {count - 1}); do
            name=$(printf 'Hash Work %03d.nes' "$i")
            truncate -s {size_mb}M "$roms/$name"
        done
        """,
        timeout=30,
    )


def _create_hash_workload_before_startup(count=80, size_mb=2):
    logs_before = _container_logs()
    _exec_checked("systemctl stop replay-control >/dev/null 2>&1 || true", timeout=30)
    _create_hash_workload(count=count, size_mb=size_mb)
    _exec_checked("systemctl start replay-control >/dev/null", timeout=30)
    _wait_for_app(timeout=90)
    _wait_for_new_log(logs_before, "L2 populate: done", timeout=120)
    _wait_for_new_log(logs_before, "Identity phase: queued work finished", timeout=120)


def _create_startup_skip_workload():
    _exec_checked(
        """
        set -e
        roms=/media/usb/roms/sony_psx
        mkdir -p "$roms"
        printf 'rom-data' > "$roms/Crash Bandicoot.chd"
        """,
        timeout=30,
    )


def _restart_service_and_wait_idle(timeout=60):
    logs_before = _container_logs()
    _exec_checked("systemctl restart replay-control >/dev/null", timeout=30)
    _wait_for_app(timeout=timeout)
    _wait_for_new_log(logs_before, "L2 populate: done", timeout=timeout)


def _container_logs() -> str:
    engine = CONTAINER_ENGINE or "podman"
    result = subprocess.run(
        [engine, "logs", CONTAINER],
        capture_output=True,
        text=True,
        timeout=30,
        check=False,
    )
    if result.returncode != 0:
        raise AssertionError(
            f"container logs failed with {result.returncode}\n"
            f"stdout:\n{result.stdout}\n"
            f"stderr:\n{result.stderr}"
        )
    return result.stdout + result.stderr


def _new_logs_since(previous_logs: str) -> str:
    current_logs = _container_logs()
    if current_logs.startswith(previous_logs):
        return current_logs[len(previous_logs) :]
    return current_logs


def _wait_for_new_log(previous_logs: str, pattern: str, timeout: int = 60) -> str:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        new_logs = _new_logs_since(previous_logs)
        if pattern in new_logs:
            return new_logs
        time.sleep(0.5)
    raise AssertionError(f"timed out waiting for log pattern: {pattern}")


def _read_one_activity(timeout=10):
    with ActivityStream(timeout=timeout) as stream:
        return stream.next_event()


def _wait_for_activity_idle(timeout=20):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        activity = _read_one_activity(timeout=5)
        if activity.get("type") == "Idle":
            return
        time.sleep(0.3)
    raise AssertionError("activity did not become idle")


class ActivityStream:
    def __init__(self, timeout=30):
        self.timeout = timeout
        self.response = None

    def __enter__(self):
        self.response = urlopen(f"{PI_URL}/sse/activity", timeout=self.timeout)
        return self

    def __exit__(self, *_):
        if self.response is not None:
            self.response.close()

    def next_event(self):
        data_lines = []
        while True:
            try:
                raw = self.response.readline()
            except (TimeoutError, IncompleteRead) as exc:
                raise AssertionError("timed out waiting for activity SSE event") from exc
            if not raw:
                raise AssertionError("activity SSE stream closed")
            line = raw.decode("utf-8", errors="replace").strip()
            if not line or line.startswith(":"):
                if data_lines:
                    return json.loads("\n".join(data_lines))
                continue
            if line.startswith("data:"):
                data_lines.append(line.removeprefix("data:").strip())

    def wait_for(self, predicate, timeout=30):
        deadline = time.monotonic() + timeout
        events = []
        while time.monotonic() < deadline:
            event = self.next_event()
            events.append(event)
            if predicate(event):
                return event
        raise AssertionError(f"timed out waiting for activity; events={events!r}")


def test_rebuild_streams_identity_progress_and_blocks_rescan(
    page,
    isolated_container_library,
):
    _create_hash_workload_before_startup()
    page.goto(f"{PI_URL}/settings/metadata", wait_until="load", timeout=30000)
    page.locator(".manage-actions.is-hydrated").wait_for(timeout=15000)
    rescan_button = page.get_by_role("button", name="Rescan Library")
    expect(rescan_button).to_be_enabled(timeout=10000)
    page.get_by_role("button", name="Advanced").click()
    advanced_actions = page.locator(".manage-actions-grid")
    rebuild_button = advanced_actions.get_by_role("button", name="Rebuild Game Library")
    expect(rebuild_button).to_be_enabled(timeout=10000)
    rebuild_button.click()
    expect(rebuild_button).to_be_enabled(timeout=10000)

    with ActivityStream(timeout=30) as stream:
        rebuild_button.click()

        rebuild = stream.wait_for(
            lambda event: event.get("type") == "Rebuild",
            timeout=10,
        )
        assert rebuild["progress"]["is_rescan"] is False

        identity = stream.wait_for(
            lambda event: event.get("type") == "Identity"
            and event["progress"]["phase"] == "Matching"
            and event["progress"]["rows_total"] > 0,
            timeout=30,
        )
        assert identity["progress"]["rows_total"] >= 1

        expect(page.locator(".metadata-busy-banner")).to_contain_text(
            "Matching ROMs",
            timeout=5000,
        )

        expect(rescan_button).to_be_disabled(timeout=5000)

        complete = stream.wait_for(
            lambda event: event.get("type") == "Identity"
            and event["progress"]["phase"] == "Complete",
            timeout=60,
        )
        assert complete["progress"]["rows_done"] == complete["progress"]["rows_total"]


def test_startup_verification_skips_unchanged_systems_but_reconciles_touched_rom(
    isolated_container_library,
):
    _create_startup_skip_workload()
    first_scan_before = _container_logs()
    _restart_service_and_wait_idle(timeout=90)
    first_scan_logs = _new_logs_since(first_scan_before)
    assert "L2 discovery save profile: sony_psx:" in first_scan_logs

    unchanged_before = _container_logs()
    _restart_service_and_wait_idle(timeout=90)
    unchanged_logs = _new_logs_since(unchanged_before)
    assert "L2 scan profile: sony_psx:" in unchanged_logs
    assert "unchanged; skipped discovery save and enrichment" in unchanged_logs

    time.sleep(1.1)
    _exec_checked("touch '/media/usb/roms/sony_psx/Crash Bandicoot.chd'", timeout=30)
    touched_before = _container_logs()
    _restart_service_and_wait_idle(timeout=90)
    touched_logs = _new_logs_since(touched_before)
    assert "L2 discovery save profile: sony_psx:" in touched_logs
    assert "L2 scan profile: sony_psx:" in touched_logs
    assert "sony_psx" in touched_logs
    assert "unchanged; skipped discovery save and enrichment" not in "\n".join(
        line for line in touched_logs.splitlines() if "sony_psx" in line
    )
