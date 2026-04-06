"""
Shared fixtures for auto-update e2e tests.

Works against either a real Pi (via SSH) or a container (via docker/podman exec).

Usage:
    # Against Pi:
    PI_IP=192.168.10.30 python -m pytest tests/e2e/ -v

    # Against container:
    CONTAINER=replay-test python -m pytest tests/e2e/ -v

    # Against localhost (e.g., dev server):
    APP_URL=http://localhost:8091 python -m pytest tests/e2e/ -v
"""

import json
import os
import subprocess
import time

import pytest

# Container name (if set, use docker/podman exec instead of SSH)
CONTAINER = os.environ.get("CONTAINER", "")
CONTAINER_ENGINE = os.environ.get("CONTAINER_ENGINE", "")

# App URL
PI_IP = os.environ.get("PI_IP", "192.168.10.30")
PI_URL = os.environ.get("APP_URL", f"http://{PI_IP}:8080" if not CONTAINER else "http://127.0.0.1:8080")

# SSH settings (Pi mode only)
PI_HOST = os.environ.get("PI_HOST", "replay.local")
PI_USER = "root"
PI_PASS = "replayos"


def _detect_engine() -> str:
    """Detect container engine (podman or docker)."""
    if CONTAINER_ENGINE:
        return CONTAINER_ENGINE
    for engine in ["podman", "docker"]:
        try:
            subprocess.run([engine, "--version"], capture_output=True, timeout=5)
            return engine
        except (FileNotFoundError, subprocess.TimeoutExpired):
            continue
    return "docker"


def exec_cmd(cmd: str, timeout: int = 30) -> str:
    """Run a command on the target (Pi via SSH or container via exec)."""
    if CONTAINER:
        engine = _detect_engine()
        result = subprocess.run(
            [engine, "exec", CONTAINER, "bash", "-c", cmd],
            capture_output=True, text=True, timeout=timeout,
        )
    else:
        result = subprocess.run(
            [
                "sshpass", "-p", PI_PASS,
                "ssh", "-o", "StrictHostKeyChecking=no",
                f"{PI_USER}@{PI_HOST}", cmd,
            ],
            capture_output=True, text=True, timeout=timeout,
        )
    return result.stdout.strip()


# Keep ssh_cmd as alias for backward compatibility
ssh_cmd = exec_cmd


def get_pi_version() -> dict:
    """Get the current version from the target's API."""
    raw = exec_cmd("curl -s http://localhost:8080/api/version")
    return json.loads(raw)


def set_channel(channel: str):
    """Set the update channel."""
    exec_cmd(
        f'sed -i "/^update_channel/d" /media/usb/.replay-control/settings.cfg 2>/dev/null; '
        f'echo \'update_channel = "{channel}"\' >> /media/usb/.replay-control/settings.cfg'
    )


def clean_update_state():
    """Remove all update runtime state."""
    exec_cmd(
        "rm -rf /var/tmp/replay-control-update "
        "/var/tmp/replay-control-update.lock "
        "/var/tmp/replay-control-do-update.sh 2>/dev/null; "
        'sed -i "/^skipped_version/d" '
        "/media/usb/.replay-control/settings.cfg 2>/dev/null"
    )


@pytest.fixture(scope="session")
def pi_url():
    return PI_URL


@pytest.fixture()
def clean_pi():
    """Ensure target is in a clean state before each test."""
    clean_update_state()
    set_channel("beta")
    time.sleep(2)
    yield
    clean_update_state()
    set_channel("stable")
