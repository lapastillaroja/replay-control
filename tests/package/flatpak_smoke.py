"""Smoke-test an installed Flatpak in a disposable desktop or CI account."""

import json
import os
import signal
import ssl
import subprocess
import tempfile
import time
from pathlib import Path
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen


APP_ID = "io.github.lapastillaroja.ReplayControl"
BASE_URL = "https://127.0.0.1:18443"
TLS = ssl._create_unverified_context()


def request(path: str, payload: dict | None = None) -> tuple[int, str]:
    data = json.dumps(payload).encode() if payload is not None else None
    headers = {"Content-Type": "application/json"} if data else {}
    req = Request(BASE_URL + path, data=data, headers=headers)
    try:
        with urlopen(req, timeout=3, context=TLS) as response:
            return response.status, response.read().decode()
    except HTTPError as error:
        return error.code, error.read().decode()


def wait_for_root(root: Path, process: subprocess.Popen, seconds: int = 60) -> None:
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise AssertionError(f"Flatpak exited with status {process.returncode}")
        try:
            status, current = request("/storage-setup/active-root")
            result_status, result = request("/storage-setup/status")
            if status == 200 and current.strip() == str(root) and result_status == 200:
                if json.loads(result)["status"] == "ok":
                    return
        except (OSError, ValueError, KeyError, URLError):
            pass
        time.sleep(0.5)
    raise AssertionError(f"Flatpak did not activate {root}")


def start_app(temporary: str, log) -> subprocess.Popen:
    return subprocess.Popen(
        [
            "flatpak",
            "run",
            "--user",
            "--env=REPLAY_CONTROL_SKIP_FIRST_RUN_SEED=1",
            f"--env=REPLAY_CONTROL_DATA_DIR={Path(temporary, 'data')}",
            f"--env=REPLAY_CONTROL_SETTINGS_DIR={Path(temporary, 'config')}",
            APP_ID,
            "--no-browser",
        ],
        stdout=log,
        stderr=subprocess.STDOUT,
        start_new_session=True,
    )


def stop_app(process: subprocess.Popen) -> None:
    if process.poll() is None:
        os.killpg(process.pid, signal.SIGTERM)
    process.wait(timeout=15)


def main() -> None:
    permissions = subprocess.check_output(
        ["flatpak", "info", "--user", "--show-permissions", APP_ID], text=True
    )
    filesystem_entries = [
        line.partition("=")[2].strip(" ;")
        for line in permissions.splitlines()
        if line.startswith("filesystems=")
    ]
    assert not any(filesystem_entries), permissions
    try:
        request("/api/version")
    except (OSError, URLError):
        pass
    else:
        raise AssertionError("Port 18443 is already in use")

    app_data = Path.home() / ".var" / "app" / APP_ID / "data"
    app_data.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="replay-flatpak-", dir=app_data) as temporary:
        roots = [Path(temporary, name) for name in ("first", "second")]
        for root in roots:
            (root / "roms").mkdir(parents=True)
        log_path = Path(temporary, "app.log")
        with log_path.open("w+") as log:
            process = start_app(temporary, log)
            try:
                for index, root in enumerate(roots):
                    deadline = time.monotonic() + 30
                    while True:
                        try:
                            status, _ = request("/storage-setup")
                            if status == 200:
                                break
                        except (OSError, URLError):
                            pass
                        if process.poll() is not None or time.monotonic() > deadline:
                            raise AssertionError("Flatpak storage selector did not start")
                        time.sleep(0.5)
                    if index == 0:
                        status, _ = request("/storage-setup/active-root")
                        assert status == 503, f"Expected clean first-run setup, got {status}"
                        status, _ = request("/storage-setup/pick", {})
                        assert status == 403, f"Folder chooser accepted a request without its action header: {status}"
                        with tempfile.TemporaryDirectory(prefix="replay-ungranted-", dir=Path.home()) as ungranted:
                            (Path(ungranted) / "roms").mkdir()
                            status, _ = request("/storage-setup/select", {"path": ungranted})
                            assert status == 400, f"Flatpak accessed a home folder without a portal grant: {status}"
                    status, body = request("/storage-setup/select", {"path": str(root)})
                    assert status == 202, (status, body)
                    wait_for_root(root, process)

                status, version = request("/api/version")
                assert status == 200 and json.loads(version)["version"]
                status, home = request("/")
                assert status == 200 and "/static/pkg/" in home

                stop_app(process)
                unavailable = roots[-1].with_name("second-unavailable")
                roots[-1].rename(unavailable)
                process = start_app(temporary, log)
                deadline = time.monotonic() + 30
                while True:
                    try:
                        status, _ = request("/storage-setup")
                        if status == 200:
                            break
                    except (OSError, URLError):
                        pass
                    if process.poll() is not None or time.monotonic() > deadline:
                        raise AssertionError("Flatpak recovery selector did not start")
                    time.sleep(0.5)
                unavailable.rename(roots[-1])
                wait_for_root(roots[-1], process)
                print("Flatpak HTTPS, storage setup, restart, recovery, and SSR: OK")
            except Exception:
                log.flush()
                print(log_path.read_text()[-4000:])
                raise
            finally:
                stop_app(process)


if __name__ == "__main__":
    main()
