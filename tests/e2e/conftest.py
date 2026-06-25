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

import atexit
import json
import os
import stat
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from urllib.error import HTTPError, URLError
from urllib.request import HTTPRedirectHandler, Request, build_opener, urlopen

import pytest
from playwright.sync_api import TimeoutError as PlaywrightTimeoutError
from playwright.sync_api import sync_playwright

# Container name (if set, use docker/podman exec instead of SSH)
CONTAINER = os.environ.get("CONTAINER", "")
CONTAINER_ENGINE = os.environ.get("CONTAINER_ENGINE", "")

# App URL
PI_IP = os.environ.get("PI_IP", "192.168.10.30")
DEFAULT_HTTP_PORT = "8080"
DEFAULT_HTTPS_PORT = "8443"
PI_URL = os.environ.get(
    "APP_URL",
    f"https://{PI_IP}:{DEFAULT_HTTPS_PORT}"
    if not CONTAINER
    else f"http://127.0.0.1:{DEFAULT_HTTP_PORT}",
)

# SSH settings (Pi mode only)
PI_HOST = os.environ.get("PI_HOST", "replay.local")
PI_USER = "root"
PI_PASS = "replayos"

# Mock server versions (derived from Cargo.toml)
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "container"))
from mock_github import MOCK_BETA_VERSION, MOCK_STABLE_VERSION  # noqa: E402

# Mock server port/control URL. Container runs can set MOCK_CONTROL_URL when
# the mock server is reachable somewhere other than localhost.
MOCK_PORT = os.environ.get("MOCK_PORT", "9999")
MOCK_CONTROL_URL = os.environ.get("MOCK_CONTROL_URL", f"http://127.0.0.1:{MOCK_PORT}")

# CSS selectors
SEL_BANNER = ".update-banner"
SEL_UPDATING_PAGE = ".updating-page"
SEL_HYDRATED_UPDATE_CONTROLS = ".update-controls-row.is-hydrated"
SEL_CHANNEL_SELECT = ".update-controls-row select"


class _NoRedirect(HTTPRedirectHandler):
    """Surface 3xx responses as HTTPError instead of following them, so tests
    can assert on the redirect status + Location."""

    def redirect_request(self, *_args, **_kwargs):
        return None


_NO_REDIRECT_OPENER = build_opener(_NoRedirect)


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


# SSH password is fed to ssh via SSH_ASKPASS instead of sshpass — same trick
# dev.sh uses, removes the system dep. The askpass file is created lazily on
# first use and removed at interpreter exit.
_ASKPASS_PATH: str | None = None


def _askpass_path() -> str:
    global _ASKPASS_PATH
    if _ASKPASS_PATH is None:
        fd, path = tempfile.mkstemp(prefix="e2e-askpass-", suffix=".sh")
        with os.fdopen(fd, "w") as f:
            f.write(f'#!/bin/sh\necho "{PI_PASS}"\n')
        os.chmod(path, stat.S_IRWXU)
        atexit.register(lambda p=path: os.path.exists(p) and os.unlink(p))
        _ASKPASS_PATH = path
    return _ASKPASS_PATH


def _ssh_env() -> dict:
    env = os.environ.copy()
    env["SSH_ASKPASS"] = _askpass_path()
    env["SSH_ASKPASS_REQUIRE"] = "force"
    env["DISPLAY"] = ""
    return env


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
                "ssh",
                "-o", "StrictHostKeyChecking=no",
                "-o", "UserKnownHostsFile=/dev/null",
                "-o", "LogLevel=ERROR",
                f"{PI_USER}@{PI_HOST}", cmd,
            ],
            capture_output=True, text=True, timeout=timeout, env=_ssh_env(),
        )
    return result.stdout.strip()


# Alias for backward compatibility (used in test_update_install.py)
ssh_cmd = exec_cmd


def get_pi_version() -> dict:
    raw = exec_cmd(f"curl -s http://localhost:{DEFAULT_HTTP_PORT}/api/version")
    return json.loads(raw)


def set_channel(channel: str):
    exec_cmd(
        f'sed -i "/^update_channel/d" /media/usb/.replay-control/settings.cfg 2>/dev/null; '
        f'echo \'update_channel = "{channel}"\' >> /media/usb/.replay-control/settings.cfg; '
        "systemctl restart replay-control"
    )
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        try:
            get_pi_version()
            return
        except Exception:
            time.sleep(0.5)
    raise RuntimeError("Replay Control did not restart after changing update channel")


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
        urlopen(f"{MOCK_CONTROL_URL}/mock/downloads/{mode}", timeout=5)
    except Exception:
        pass


# ── Helpers for tests ────────────────────────────────────────────


def goto_settings(page):
    """Navigate to /settings and wait for the update controls to be hydrated."""
    page.goto(f"{PI_URL}/settings", wait_until="load", timeout=30000)
    page.locator(SEL_HYDRATED_UPDATE_CONTROLS).wait_for(timeout=15000)
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


def wait_for_banner_text(page, text: str, timeout=30000):
    """Wait for the update banner to be visible and contain specific text."""
    from playwright.sync_api import expect

    banner = wait_for_banner(page, timeout=timeout)
    expect(banner).to_contain_text(text, timeout=timeout)
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


# Console messages that are network noise, not JS errors — a 404 for an
# unmatched game's box art surfaces as a console "error" but is not a script
# fault. Extend only with justification.
CONSOLE_IGNORE_SUBSTRINGS = (
    "Failed to load resource",  # network 404/500 for an <img>/asset, not JS
)


def _is_console_offender(msg) -> bool:
    if msg.type not in ("error", "warning"):
        return False
    text = msg.text or ""
    return not any(ignore in text for ignore in CONSOLE_IGNORE_SUBSTRINGS)


@pytest.fixture()
def page(browser):
    """Per-test browser page with automatic cleanup.

    Also fails the test if the page logs any JS console error/warning or throws
    an uncaught exception (pageerror). Network resource 404s are filtered (see
    CONSOLE_IGNORE_SUBSTRINGS); real script faults are not.
    """
    context = browser.new_context(ignore_https_errors=True)
    p = context.new_page()
    offenders: list[str] = []

    def record_console(msg):
        if _is_console_offender(msg):
            offenders.append(f"console.{msg.type}: {msg.text}")

    p.on("console", record_console)
    p.on("pageerror", lambda exc: offenders.append(f"pageerror: {exc}"))
    yield p
    context.close()
    assert not offenders, "JS console errors/warnings during test:\n  " + "\n  ".join(offenders)


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


# ── Library seeding helpers (container only) ─────────────────────────
#
# The container boots in standalone mode (`--storage-path /media/usb`), so auth
# is bypassed and every server fn / page is reachable without a session. These
# helpers seed a tiny on-disk library (ROMs + captures + recents + manuals)
# directly under /media/usb so feature tests have something to act on, then
# wait for the background scan to make the seeded ROM queryable.
#
# Paths mirror `StorageLocation` in
# replay-control-core-server/src/platform/storage.rs:
#   roms      -> /media/usb/roms/<system>/<file>
#   captures  -> /media/usb/captures/<system>/<rom>_<ts>.png
#   recents   -> /media/usb/roms/_recent/<system>@<rom>.rec   (content = rom path)
#   manuals   -> /media/usb/.replay-control/manuals/<system>/ (written by upload)

STORAGE_ROOT = "/media/usb"
ROMS_DIR = f"{STORAGE_ROOT}/roms"
CAPTURES_DIR = f"{STORAGE_ROOT}/captures"
RECENTS_DIR = f"{ROMS_DIR}/_recent"
FAVORITES_DIR = f"{ROMS_DIR}/_favorites"
RC_DIR = f"{STORAGE_ROOT}/.replay-control"
MANUALS_DIR = f"{RC_DIR}/manuals"
MEDIA_DIR = f"{RC_DIR}/media"

# A minimal valid 1x1 PNG so a seeded capture renders as a real image.
PNG_1X1_B64 = (
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg=="
)


def _require_container():
    if not CONTAINER:
        pytest.skip("library-seeding e2e mutates container storage; container only")


def wait_hydrated(page, timeout: int = 30000):
    """Wait for the global loading overlay to hide — the reliable 'app is
    interactive' signal. Until it hides the Leptos router hasn't attached its
    click interceptor (see CLAUDE.md). `wait_for(state="hidden")` also resolves
    immediately if the overlay is absent, so no presence check is needed."""
    page.locator(".initial-loading-shell").wait_for(state="hidden", timeout=timeout)


def wait_until(predicate, timeout: float = 10, interval: float = 0.3):
    """Poll `predicate` until it returns truthy or `timeout` seconds elapse.
    Returns the truthy value, or None on timeout (callers assert on the real
    state afterwards). Centralizes the disk/state polling the feature tests do
    after an async UI action lands."""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(interval)
    return None


def goto_hydrated(page, path: str, timeout: int = 30000):
    """Navigate to an app path and wait for hydration. `path` may be absolute or
    app-relative (joined onto PI_URL)."""
    url = path if path.startswith("http") else f"{PI_URL}{path}"
    page.goto(url, wait_until="load", timeout=timeout)
    wait_hydrated(page, timeout)


def accept_dialogs(page):
    """Auto-accept JS confirm() dialogs (delete confirmations)."""
    page.on("dialog", lambda dialog: dialog.accept())


def wait_for_app(timeout: int = 90):
    """Block until the app answers /api/version."""
    deadline = time.monotonic() + timeout
    last = None
    while time.monotonic() < deadline:
        try:
            with urlopen(f"{PI_URL}/api/version", timeout=2) as resp:
                if resp.status == 200:
                    return
        except Exception as exc:  # noqa: BLE001
            last = exc
        time.sleep(0.5)
    raise AssertionError(f"app did not become ready at {PI_URL}: {last}")


def exec_checked(cmd: str, timeout: int = 30) -> str:
    """Like exec_cmd, but fail loudly if the command exits non-zero."""
    engine = CONTAINER_ENGINE or _detect_engine()
    result = subprocess.run(
        [engine, "exec", CONTAINER, "bash", "-c", cmd],
        capture_output=True, text=True, timeout=timeout, check=False,
    )
    if result.returncode != 0:
        raise AssertionError(
            f"container command failed ({result.returncode})\n"
            f"cmd: {cmd}\nstdout:\n{result.stdout}\nstderr:\n{result.stderr}"
        )
    return result.stdout.strip()


def container_logs() -> str:
    engine = CONTAINER_ENGINE or _detect_engine()
    result = subprocess.run(
        [engine, "logs", CONTAINER],
        capture_output=True, text=True, timeout=30, check=False,
    )
    return result.stdout + result.stderr


def new_logs_since(previous_logs: str) -> str:
    """Return only the log text appended since the `previous_logs` snapshot."""
    current = container_logs()
    return current[len(previous_logs):] if current.startswith(previous_logs) else current


def wait_for_new_log(previous_logs: str, pattern: str, timeout: int = 120) -> str:
    """Wait for `pattern` to appear in logs emitted after `previous_logs`."""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        new = new_logs_since(previous_logs)
        if pattern in new:
            return new
        time.sleep(0.5)
    raise AssertionError(f"timed out waiting for log pattern: {pattern!r}")


class ActivityStream:
    """Reader for the `/sse/activity` server-sent-event stream.

    Each event is a JSON object with a `type` (Idle / Rebuild / Identity / ...).
    Used to assert background work transitions and to wait for the activity slot
    to free up before triggering maintenance actions.
    """

    def __init__(self, timeout: int = 30):
        self.timeout = timeout
        self.response = None

    def __enter__(self):
        self.response = urlopen(f"{PI_URL}/sse/activity", timeout=self.timeout)
        return self

    def __exit__(self, *_):
        if self.response is not None:
            self.response.close()

    def next_event(self) -> dict:
        data_lines: list[str] = []
        while True:
            try:
                raw = self.response.readline()
            except Exception as exc:  # noqa: BLE001
                raise AssertionError("timed out waiting for activity SSE event") from exc
            if not raw:
                raise AssertionError("activity SSE stream closed")
            line = raw.decode("utf-8", errors="replace").strip()
            if line.startswith("data:"):
                data_lines.append(line[len("data:"):].strip())
            elif not line and data_lines:
                return json.loads("\n".join(data_lines))

    def wait_for(self, predicate, timeout: int = 30) -> dict:
        deadline = time.monotonic() + timeout
        events = []
        while time.monotonic() < deadline:
            event = self.next_event()
            events.append(event)
            if predicate(event):
                return event
        raise AssertionError(f"timed out waiting for activity; events={events!r}")


def wait_for_activity_idle(timeout: int = 45):
    """Best-effort wait until /sse/activity reports Idle.

    A freshly restarted app runs scan + background identity hashing; the
    maintenance server fns (clear images, cleanup, etc.) refuse to start while
    any activity owns the slot. This returns quietly on timeout — callers use it
    only to let background work settle, so it is safe to call unwrapped.
    """
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            with ActivityStream(timeout=5) as stream:
                stream.wait_for(lambda e: e.get("type") == "Idle", timeout=6)
                return
        except Exception:  # noqa: BLE001
            time.sleep(0.3)


def restart_app_and_scan(timeout: int = 120):
    """Restart the service and wait for the per-system populate to finish.

    'L2 populate: done' is the signal that discovery has written the scanned
    ROM rows into library.db, so a seeded ROM is queryable afterwards. We then
    let the activity slot settle to Idle so background identity hashing doesn't
    collide with maintenance actions a test might trigger.
    """
    before = container_logs()
    exec_cmd("systemctl restart replay-control")
    wait_for_app(timeout)
    wait_for_new_log(before, "L2 populate: done", timeout)
    wait_for_activity_idle(timeout=45)


def reset_library():
    """Wipe the seeded library back to an empty, dismissed-setup state."""
    exec_cmd(
        "systemctl stop replay-control >/dev/null 2>&1 || true; "
        f"rm -rf {ROMS_DIR}/* {CAPTURES_DIR}/* {MANUALS_DIR} {MEDIA_DIR}/* "
        f"{RC_DIR}/library.db {RC_DIR}/library.db-* {RC_DIR}/user_data.db {RC_DIR}/user_data.db-* "
        "2>/dev/null || true; "
        f"mkdir -p {ROMS_DIR} {CAPTURES_DIR} {RECENTS_DIR} {MEDIA_DIR}; "
        f"printf 'setup_dismissed = \"true\"\\n' > {RC_DIR}/settings.cfg"
    )


def seed_rom(system: str, rom_filename: str, size_mb: int = 1):
    exec_cmd(
        f'mkdir -p "{ROMS_DIR}/{system}"; '
        f'truncate -s {size_mb}M "{ROMS_DIR}/{system}/{rom_filename}"'
    )


def seed_capture(system: str, rom_filename: str, suffix: str = "_20260101_120000") -> str:
    """Drop a 1x1 PNG capture for a ROM. Returns the capture filename."""
    capture_name = f"{rom_filename}{suffix}.png"
    exec_cmd(
        f'mkdir -p "{CAPTURES_DIR}/{system}"; '
        f"printf '%s' '{PNG_1X1_B64}' | base64 -d > \"{CAPTURES_DIR}/{system}/{capture_name}\""
    )
    return capture_name


def seed_recent(system: str, rom_filename: str):
    """Create a .rec recents marker pointing at the seeded ROM."""
    rom_path = f"/roms/{system}/{rom_filename}"
    exec_cmd(
        f'mkdir -p "{RECENTS_DIR}"; '
        f"printf '%s\\n' '{rom_path}' > \"{RECENTS_DIR}/{system}@{rom_filename}.rec\""
    )


def list_files(directory: str) -> list[str]:
    return exec_cmd(f'ls -1 "{directory}" 2>/dev/null || true').splitlines()


def path_exists(path: str) -> bool:
    # exec_cmd already strips its output.
    return exec_cmd(f'test -e "{path}" && echo yes || echo no') == "yes"


def post_sfn(name: str, body: bytes = b"{}", content_type: str = "application/json") -> int:
    """POST a server fn with no session and return the status code.

    Used by the auth-gating tests: an empty body is enough to observe the
    middleware's 200/401/403/404 decision before any arg decoding happens.
    """
    req = Request(
        f"{PI_URL}/sfn/{name}", data=body, method="POST",
        headers={"Content-Type": content_type},
    )
    try:
        return urlopen(req, timeout=10).status
    except HTTPError as exc:
        return exc.code
    except URLError:
        return 0


def http_status(path: str) -> tuple[int, str]:
    """GET a page as a browser would (Accept: text/html) WITHOUT following
    redirects. Returns (status, Location header) so callers can assert on a
    302->/login redirect."""
    req = Request(f"{PI_URL}{path}", headers={"Accept": "text/html"})
    try:
        resp = _NO_REDIRECT_OPENER.open(req, timeout=10)
        return resp.status, resp.headers.get("Location", "")
    except HTTPError as exc:
        # A redirect surfaces here as an HTTPError carrying the Location header.
        return exc.code, exc.headers.get("Location", "")
    except URLError:
        return 0, ""


@pytest.fixture()
def seeded_game():
    """Seed a single NES ROM and wait for it to be scanned into the library.

    Yields a dict describing the seeded game so tests can build URLs/selectors.
    Resets the library before and after so tests are independent.
    """
    _require_container()
    system = "nintendo_nes"
    rom_filename = "E2E Seed Game.nes"
    reset_library()
    seed_rom(system, rom_filename)
    restart_app_and_scan()
    yield {
        "system": system,
        "rom_filename": rom_filename,
        "detail_url": f"{PI_URL}/games/{system}/{rom_filename}",
        "rom_path": f"/roms/{system}/{rom_filename}",
    }
    reset_library()
    # Leave the service running with an empty library for the next test.
    try:
        exec_cmd("systemctl restart replay-control")
        wait_for_app(60)
    except Exception:  # noqa: BLE001
        pass


# ── Device-mode fixture (auth tests) ─────────────────────────────────
#
# The container normally boots standalone (`--storage-path`, guard bypassed).
# This fixture relaunches the in-container app in *device* mode (no
# --storage-path → is_device() via the /opt/replay marker) with first-setup done
# and a known root password, so the auth guard + login flow can be exercised.
# Shared here so any future device-mode test can reuse it.

# In device mode the settings store falls back to /etc/replay-control (see
# resolve_settings_store in api/mod.rs), NOT the storage's .replay-control dir.
DEVICE_SETTINGS_DIR = "/etc/replay-control"

# Known device (root) password so login_admin can verify it via /etc/shadow
# (needs python3 + libcrypt1, added to Containerfile.replayos).
ADMIN_PW = "replaytest123"

_DEVICE_PIDFILE = "/var/run/replay-control-device-e2e.pid"
# nohup + </dev/null + /proc/1/fd redirection keeps the detached instance alive
# after the podman-exec session closes (same trick as mock_systemctl.sh); the
# pidfile lets teardown stop exactly this instance.
_DEVICE_LAUNCH = (
    "nohup /usr/local/bin/replay-control-app --dangerous-disable-https "
    "--dangerous-allow-insecure-auth-over-http --port 8080 "
    "--site-root /usr/local/share/replay/site "
    f"</dev/null >/proc/1/fd/1 2>/proc/1/fd/2 & echo $! > {_DEVICE_PIDFILE}"
)


@pytest.fixture(scope="module")
def device_mode_app():
    """Relaunch the app in device mode with first-setup done + a known root
    password, for the duration of a test module. Restores standalone on teardown."""
    _require_container()
    # Stop the standalone service so port 8080 is free.
    exec_cmd("systemctl stop replay-control >/dev/null 2>&1 || true")
    time.sleep(1)
    # Point device-mode storage at /media/usb, mark first-setup complete (so the
    # guard enforces role gating instead of redirecting to /first-setup), and set
    # the device password.
    exec_cmd(
        'sed -i "/^system_storage/d" /media/sd/config/replay.cfg 2>/dev/null || true; '
        'printf "system_storage = \\"usb\\"\\n" >> /media/sd/config/replay.cfg; '
        f"mkdir -p {DEVICE_SETTINGS_DIR}; "
        f'sed -i "/^first_setup_done/d" {DEVICE_SETTINGS_DIR}/settings.cfg 2>/dev/null || true; '
        f'printf "first_setup_done = \\"true\\"\\n" >> {DEVICE_SETTINGS_DIR}/settings.cfg; '
        f"echo 'root:{ADMIN_PW}' | chpasswd"
    )
    exec_cmd(_DEVICE_LAUNCH)
    wait_for_app(timeout=60)
    yield
    # Always restore standalone, even if the device instance failed to boot.
    exec_cmd(
        f"[ -f {_DEVICE_PIDFILE} ] && kill $(cat {_DEVICE_PIDFILE}) 2>/dev/null || true; "
        f"rm -f {_DEVICE_PIDFILE}"
    )
    time.sleep(1)
    exec_cmd("systemctl start replay-control >/dev/null 2>&1 || true")
    wait_for_app(timeout=60)
