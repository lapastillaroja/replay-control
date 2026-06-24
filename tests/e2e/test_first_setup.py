"""
Browser e2e coverage for the first setup gate.

The test snapshots app settings, forces `first_setup_done=false`, restarts the
service, verifies browser redirects/page content, then restores the original
settings file.
"""

import os
import time
from urllib.request import urlopen

import pytest
from playwright.sync_api import expect

from conftest import CONTAINER, PI_URL, exec_cmd

pytestmark = pytest.mark.skipif(
    os.environ.get("APP_URL") and not CONTAINER and "PI_HOST" not in os.environ and "PI_IP" not in os.environ,
    reason="first setup e2e needs command access to the target service",
)


def _wait_for_server(timeout_s: int = 30) -> None:
    deadline = time.monotonic() + timeout_s
    while time.monotonic() < deadline:
        try:
            urlopen(f"{PI_URL}/api/version", timeout=2)
            return
        except Exception:
            time.sleep(0.5)
    pytest.fail(f"server did not respond at {PI_URL}/api/version within {timeout_s}s")


def _settings_path() -> str:
    path = exec_cmd(
        "if [ -f /etc/replay-control/settings.cfg ] || [ -d /etc/replay-control ]; then "
        "echo /etc/replay-control/settings.cfg; "
        "else echo /media/usb/.replay-control/settings.cfg; fi"
    ).strip()
    if not path:
        pytest.fail("could not resolve Replay Control settings path")
    return path


def _is_standalone_mode() -> bool:
    cmdline = exec_cmd(
        "pid=$(cat /var/run/replay-control.pid 2>/dev/null || true); "
        "[ -n \"$pid\" ] && tr '\\0' ' ' < /proc/$pid/cmdline || true"
    )
    return "--storage-path" in cmdline


@pytest.fixture()
def first_setup_pending():
    if _is_standalone_mode():
        pytest.skip("first setup is only enforced in device mode")

    settings_path = _settings_path()
    snapshot = "/tmp/replay-control-e2e-settings.cfg"
    existed = exec_cmd(f"test -f {settings_path} && echo yes || echo no").strip() == "yes"

    if existed:
        exec_cmd(f"cp -p {settings_path} {snapshot}")
    else:
        exec_cmd(f"rm -f {snapshot}")

    exec_cmd(
        f"mkdir -p $(dirname {settings_path}); "
        f"touch {settings_path}; "
        f"sed -i '/^first_setup_done/d' {settings_path}; "
        f"printf 'first_setup_done = \"false\"\\n' >> {settings_path}; "
        "systemctl restart replay-control"
    )
    _wait_for_server()

    yield

    if existed:
        exec_cmd(f"cp -p {snapshot} {settings_path}; rm -f {snapshot}")
    else:
        exec_cmd(f"rm -f {settings_path} {snapshot}")
    exec_cmd("systemctl restart replay-control")
    _wait_for_server()


def test_first_setup_gate_redirects_login_and_renders_password_form(page, first_setup_pending):
    page.goto(f"{PI_URL}/login", wait_until="load", timeout=30000)
    page.wait_for_url("**/first-setup", timeout=10000)

    expect(page.locator("h1")).to_contain_text("First setup")
    expect(page.locator("#first-setup-password")).to_be_visible()
    expect(page.locator("body")).to_contain_text("default password is replayos")

    page.goto(f"{PI_URL}/", wait_until="load", timeout=30000)
    page.wait_for_url("**/first-setup", timeout=10000)
