"""
Shared fixtures for auto-update e2e tests.

Prerequisites:
- Pi running at PI_IP:8080
- sshpass installed on the test runner
- Playwright installed: pip install playwright && playwright install chromium

Usage:
    PI_IP=192.168.10.30 python -m pytest tests/e2e/ -v --timeout=180
"""

import json
import os
import subprocess
import time

import pytest

PI_IP = os.environ.get("PI_IP", "192.168.10.30")
PI_HOST = os.environ.get("PI_HOST", "replay.local")
PI_USER = "root"
PI_PASS = "replayos"
PI_URL = f"http://{PI_IP}:8080"


@pytest.fixture(scope="session")
def pi_url():
    return PI_URL


def ssh_cmd(cmd: str, timeout: int = 30) -> str:
    """Run a command on the Pi via SSH."""
    result = subprocess.run(
        [
            "sshpass", "-p", PI_PASS,
            "ssh", "-o", "StrictHostKeyChecking=no",
            f"{PI_USER}@{PI_HOST}", cmd,
        ],
        capture_output=True, text=True, timeout=timeout,
    )
    return result.stdout.strip()


def get_pi_version() -> dict:
    """Get the current version from the Pi's API."""
    raw = ssh_cmd("curl -s http://localhost:8080/api/version")
    return json.loads(raw)


def set_channel(channel: str):
    """Set the update channel on the Pi."""
    ssh_cmd(
        f'sed -i "/^update_channel/d" /media/usb/.replay-control/settings.cfg 2>/dev/null; '
        f'echo \'update_channel = "{channel}"\' >> /media/usb/.replay-control/settings.cfg'
    )


def clean_update_state():
    """Remove all update runtime state."""
    ssh_cmd(
        "rm -rf /var/tmp/replay-control-update "
        "/var/tmp/replay-control-update.lock "
        "/var/tmp/replay-control-do-update.sh 2>/dev/null; "
        'sed -i "/^skipped_version/d" '
        "/media/usb/.replay-control/settings.cfg 2>/dev/null"
    )


@pytest.fixture()
def clean_pi():
    """Ensure Pi is in a clean state before each test."""
    clean_update_state()
    set_channel("beta")
    # Brief pause to let the service pick up changes
    time.sleep(2)
    yield
    # Cleanup after test
    clean_update_state()
    set_channel("stable")
