"""Reusable package operations, without shell-based assertions."""

import hashlib
import json
import re

from support import assert_owned, eventually, package_version


def configure(container, *, storage=None, https_port=8443, mock_updates=False, skip_seed=True):
    lines = ["REPLAY_PORT=8080", f"REPLAY_HTTPS_PORT={https_port}"]
    if skip_seed:
        lines.append("REPLAY_CONTROL_SKIP_FIRST_RUN_SEED=1")
    if storage:
        lines.append(f"REPLAY_STORAGE_PATH={storage}")
    if mock_updates:
        lines.append("REPLAY_GITHUB_API_URL=http://127.0.0.1:9000")
    container.write("/etc/default/replay-control", "\n".join(lines) + "\n")


def install_package(container, package, *, update_index=True):
    if package.kind == "rpm":
        container.exec("dnf", "install", "-y", f"/root/{package.path.name}")
    else:
        if update_index:
            container.exec("apt-get", "update", "-qq")
        container.exec(
            "apt-get", "install", "-y", f"/root/{package.path.name}",
            env={"DEBIAN_FRONTEND": "noninteractive"},
        )


def assert_package_files(container, package):
    assert container.exec("test", "-s", "/usr/share/replay-control/catalog.sqlite", check=False).returncode == 0
    assert container.exec("test", "-d", "/usr/share/replay-control/site/pkg", check=False).returncode == 0
    assert container.exec("test", "-s", "/usr/share/icons/hicolor/192x192/apps/replay-control.png", check=False).returncode == 0
    assert "Exec=/usr/lib/replay-control/open-browser" in container.read("/usr/share/applications/replay-control.desktop")
    assert_owned(
        container, package.kind,
        "/usr/bin/replay-control-app",
        "/usr/share/replay-control/catalog.sqlite",
        "/usr/share/applications/replay-control.desktop",
        "/usr/lib/replay-control/open-browser",
        f"/usr/lib/replay-control/package-update-{package.kind}",
    )
    if package.kind == "deb":
        container.exec("dpkg", "-V", "replay-control")
        assert "replay-" not in container.output("dpkg-query", "-W", "-f=${Depends}", "replay-control").lower()


def assert_desktop_launcher(container):
    launcher = "/usr/lib/replay-control/open-browser"
    config = container.read("/etc/default/replay-control")
    try:
        for directory, executable, contents in (
            ("/tmp/launcher-xdg", "xdg-open", '#!/bin/sh\nprintf "%s\\n" "$1" > "$REPLAY_LAUNCH_TEST_OUT"\n'),
            ("/tmp/launcher-gio", "gio", '#!/bin/sh\ntest "$1" = open || exit 1\nprintf "%s\\n" "$2" > "$REPLAY_LAUNCH_TEST_OUT"\n'),
        ):
            container.exec("mkdir", "-p", directory)
            container.write(f"{directory}/{executable}", contents)
            container.exec("chmod", "+x", f"{directory}/{executable}")
        cases = (
            ("REPLAY_PORT=8080\nREPLAY_HTTPS_PORT=8443\n", "/tmp/launcher-xdg", "8443"),
            ('REPLAY_PORT=8080\nREPLAY_HTTPS_PORT="9443"\nREPLAY_STORAGE_PATH=/srv/My Games\n', "/tmp/launcher-xdg", "9443"),
            ("REPLAY_PORT=9080\nREPLAY_HTTPS_PORT=9443\n", "/tmp/launcher-gio", "9443"),
            ("REPLAY_HTTPS_PORT='9553'\n", "/tmp/launcher-xdg", "9553"),
        )
        for settings, path, port in cases:
            container.write("/etc/default/replay-control", settings)
            container.exec(launcher, env={"PATH": path, "REPLAY_LAUNCH_TEST_OUT": "/tmp/launcher-url"})
            assert container.read("/tmp/launcher-url") == f"https://localhost:{port}/"
    finally:
        container.write("/etc/default/replay-control", config + "\n")


def select_storage(container, path, expected=202):
    status, body = container.http(
        "/storage-setup/select", method="POST", data=json.dumps({"path": path}),
        headers=("content-type: application/json",),
    )
    assert status == expected, body


def wait_storage(container, path, *, status="ok"):
    eventually(
        lambda: container.exists("/var/lib/replay-control/standalone-storage-path")
        and container.read("/var/lib/replay-control/standalone-storage-path") == path
        and container.exists("/var/lib/replay-control/storage-switch-status")
        and container.read("/var/lib/replay-control/storage-switch-status") == status,
        f"storage selection {path} ({status})",
    )


def make_deb_revision(container, source, version, destination):
    stage = "/root/package-revision-stage"
    container.exec("rm", "-rf", stage)
    container.exec("dpkg-deb", "--raw-extract", source, stage)
    control = container.read(f"{stage}/DEBIAN/control")
    assert re.search(r"^Version: .+$", control, re.MULTILINE)
    control = re.sub(r"^Version: .+$", f"Version: {version}", control, count=1, flags=re.MULTILINE)
    container.write(f"{stage}/DEBIAN/control", control + "\n")
    container.exec("dpkg-deb", "--build", "--root-owner-group", stage, destination)
    container.exec("rm", "-rf", stage)


def prepare_mock_release(container, package):
    name = package.path.name
    container.exec("mkdir", "-p", "/root/mock/repos/lapastillaroja/replay-control/releases/tags")
    container.exec("ln", "-f", f"/root/{name}", f"/root/mock/{name}")
    checksum = hashlib.sha256(package.path.read_bytes()).hexdigest()
    container.write("/root/mock/checksums.sha256", f"{checksum}  {name}\n")
    assets = [name, "checksums.sha256", "replay-control-app-aarch64-linux.tar.gz", "replay-site.tar.gz"]
    release = {"assets": [
        {"name": asset, "browser_download_url": f"http://127.0.0.1:9000/{asset}"}
        for asset in assets
    ]}
    container.write(
        f"/root/mock/repos/lapastillaroja/replay-control/releases/tags/v{package.app_version}",
        json.dumps(release),
    )
    container.exec("python3", "-m", "http.server", "9000", "--bind", "127.0.0.1", "-d", "/root/mock", detached=True)
    eventually(lambda: container.http("/checksums.sha256", port=9000, secure=False)[0] == 200, "mock release server", timeout=30)


def run_update_now(container, package):
    if package.kind == "deb":
        make_deb_revision(container, f"/root/{package.path.name}", "0.0.1", "/root/replay-control-old.deb")
        container.exec("apt-get", "install", "-y", "--allow-downgrades", "/root/replay-control-old.deb", env={"DEBIAN_FRONTEND": "noninteractive"})
        assert package_version(container, "deb") == "0.0.1"
    old_pid = container.read("/run/replay-control-test.pid")
    prepare_mock_release(container, package)
    routes = container.output("grep", "-aoE", "/sfn/start_update[0-9]+", "/usr/bin/replay-control-app").splitlines()
    assert routes, "start_update server function missing"

    def begin():
        status, body = container.http(
            routes[0], method="POST", data=f"tag=v{package.app_version}",
            headers=("content-type: application/x-www-form-urlencoded", "accept: application/x-www-form-urlencoded"),
            timeout=300,
        )
        if status == 200 and "ServerError" not in body:
            return True
        if not status or "Another operation is already running" in body:
            return False
        raise AssertionError(f"Update Now returned HTTP {status}: {body}")

    eventually(begin, "Update Now to start", timeout=180, interval=3)
    def finished_result():
        status, body = container.http("/api/update-result")
        return body.strip() if status == 200 and body.startswith(("ok:", "failed:")) else None

    result = eventually(finished_result, "package update result", timeout=120)
    assert result == f"ok:{package.app_version}", result
    assert package_version(container, package.kind) == package.version
    assert container.read("/run/replay-control-test.pid") != old_pid
