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
import sys
import time
from pathlib import Path

import pytest
from playwright.sync_api import sync_playwright

# Container name (if set, use docker/podman exec instead of SSH)
CONTAINER = os.environ.get("CONTAINER", "")
CONTAINER_ENGINE = os.environ.get("CONTAINER_ENGINE", "")

# App URL
PI_IP = os.environ.get("PI_IP", "192.168.10.30")
DEFAULT_PORT = "8080"
PI_URL = os.environ.get(
    "APP_URL",
    f"http://{PI_IP}:{DEFAULT_PORT}" if not CONTAINER else f"http://127.0.0.1:{DEFAULT_PORT}",
)

# SSH settings (Pi mode only)
PI_HOST = os.environ.get("PI_HOST", "replay.local")
PI_USER = "root"
PI_PASS = "replayos"

# Mock server versions (derived from Cargo.toml)
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "container"))
from mock_github import MOCK_BETA_VERSION, MOCK_STABLE_VERSION  # noqa: E402

# Mock server port
MOCK_PORT = os.environ.get("MOCK_PORT", "9999")

# CSS selectors
SEL_BANNER = ".update-banner"
SEL_UPDATING_PAGE = ".updating-page"
SEL_CHANNEL_SELECT = ".update-controls-row select"


def _detect_engine() -> str:
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


# Alias for backward compatibility (used in test_update_install.py)
ssh_cmd = exec_cmd


def get_pi_version() -> dict:
    raw = exec_cmd(f"curl -s http://localhost:{DEFAULT_PORT}/api/version")
    return json.loads(raw)


def set_channel(channel: str):
    exec_cmd(
        f'sed -i "/^update_channel/d" /media/usb/.replay-control/settings.cfg 2>/dev/null; '
        f'echo \'update_channel = "{channel}"\' >> /media/usb/.replay-control/settings.cfg'
    )


def clean_update_state():
    exec_cmd(
        "rm -rf /var/tmp/replay-control-update "
        "/var/tmp/replay-control-update.lock "
        "/var/tmp/replay-control-do-update.sh 2>/dev/null; "
        'sed -i "/^skipped_version/d" '
        "/media/usb/.replay-control/settings.cfg 2>/dev/null"
    )


def set_mock_downloads(fail: bool):
    """Toggle mock server download failures (for error testing)."""
    from urllib.request import urlopen
    mode = "fail" if fail else "ok"
    try:
        urlopen(f"http://127.0.0.1:{MOCK_PORT}/mock/downloads/{mode}", timeout=5)
    except Exception:
        pass


# ── Helpers for tests ────────────────────────────────────────────


def goto_settings(page):
    """Navigate to /settings and wait for the update controls to be ready."""
    page.goto(f"{PI_URL}/settings", wait_until="load", timeout=30000)
    page.locator(SEL_CHANNEL_SELECT).wait_for(timeout=10000)


# Backward-compatible alias
goto_more = goto_settings


def click_check(page):
    """Click the 'Check for Updates' button."""
    page.locator("button").filter(has_text="Check").click()


def wait_for_banner(page, timeout=30000):
    """Wait for the update banner to appear and return the locator."""
    banner = page.locator(SEL_BANNER)
    banner.wait_for(timeout=timeout)
    return banner


def click_update_now(page):
    """Click 'Update Now' and wait for navigation to /updating."""
    page.locator("a").filter(has_text="Update Now").click()
    page.wait_for_url("**/updating", timeout=5000)


# ── Fixtures ─────────────────────────────────────────────────────


@pytest.fixture(scope="session")
def browser():
    """Session-scoped browser — launched once, shared across all tests."""
    with sync_playwright() as p:
        b = p.chromium.launch(headless=True)
        yield b
        b.close()


@pytest.fixture()
def page(browser):
    """Per-test browser page with automatic cleanup."""
    p = browser.new_page()
    yield p
    p.close()


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
