"""Debian and Fedora standalone package integration."""

import json

import pytest

from scenarios import (
    assert_desktop_launcher,
    assert_package_files,
    configure,
    install_package,
    run_update_now,
    select_storage,
    wait_storage,
)
from support import eventually, package_version


def test_standalone_install_update_and_recovery(package, container_factory, browser_enabled):
    if package.arch == "arm64":
        pytest.skip("arm64 standalone is covered by the RePlayOS image scenario")

    container = container_factory()
    root = "/srv" if package.kind == "deb" else "/media/sd"
    container.exec("mkdir", "-p", f"{root}/roms")
    configure(container, skip_seed=package.kind == "deb")
    if package.kind == "deb":
        container.exec("mkdir", "-p", "/srv/other/roms", "/srv/other/bios", "/srv/fail/roms", "/var/lib/replay-control/cache")
        container.write(
            "/var/lib/replay-control/cache/launchbox-metadata.xml",
            '<?xml version="1.0"?><LaunchBox><Game><Name>Package Test Game</Name>'
            '<DatabaseID>424242</DatabaseID><Platform>Nintendo Entertainment System</Platform>'
            '<Overview>Package integration fixture.</Overview></Game></LaunchBox>',
        )
    install_package(container, package)
    assert_package_files(container, package)
    assert_desktop_launcher(container)

    page = container.wait_http("/storage-setup")
    assert "Choose your storage location" in page and "STEP 1 OF 2" in page
    script = container.wait_http("/storage-setup/app.js")
    assert "/?setup" in script
    if package.kind == "deb":
        assert "/storage-setup/messages" in script
        status, messages = container.http("/storage-setup/messages", headers=("Accept-Language: es-ES,es;q=0.9",))
        assert status == 200 and json.loads(messages)["title"] == "Elige la ubicación de almacenamiento"
        settings = "/etc/replay-control/settings.cfg"
        prior = container.read(settings) if container.exists(settings) else None
        container.write(settings, 'locale = "ja"\n')
        status, messages = container.http("/storage-setup/messages", headers=("Accept-Language: es-ES",))
        assert status == 200 and json.loads(messages)["locale"] == "ja"
        if prior is None:
            container.exec("rm", settings)
        else:
            container.write(settings, prior + "\n")
        status, listing = container.http("/storage-setup/list?path=/srv")
        assert status == 200 and json.loads(listing)["valid"] is True

    select_storage(container, root)
    wait_storage(container, root)
    assert json.loads(container.wait_http())["version"] == package.app_version
    assert "Choose your game library folder" in container.wait_http("/?setup")
    assert container.exists("/etc/replay-control")
    assert container.exists("/var/lib/replay-control/storages")

    if package.kind == "deb":
        assert "Storage location" in container.wait_http("/settings")
        eventually(
            lambda: container.exec(
                "python3", "-c",
                'import sqlite3; db=sqlite3.connect("/var/lib/replay-control/external_metadata.db"); '
                'assert db.execute("SELECT COUNT(*) FROM provider_game WHERE provider = ?", '
                '("launchbox",)).fetchone()[0] == 1',
                check=False,
            ).returncode == 0,
            "LaunchBox metadata import",
        )
        assert not container.exists("/srv/.replay-control-data/external_metadata.db")
        _check_storage_switching(container)
    else:
        _check_fedora_metadata_and_route(container)

    configure(container, storage=root, mock_updates=True)
    container.exec("systemctl", "restart", "replay-control")
    container.wait_http()
    if browser_enabled:
        container.browser_test("test_package_setup.py", REPLAY_PACKAGE_BROWSER="1")
    assert json.loads(container.wait_http())["version"] == package.app_version

    if package.kind == "rpm":
        _downgrade_rpm(container, package)
    run_update_now(container, package)
    if browser_enabled:
        container.browser_test("test_package_update_result.py", REPLAY_PACKAGE_VERSION=package.app_version)
    if package.kind == "deb":
        _check_bootstrap_update_result(container, package)


def _check_storage_switching(container):
    select_storage(container, "/srv/missing", expected=400)
    assert container.read("/var/lib/replay-control/standalone-storage-path") == "/srv"
    select_storage(container, "/srv/other")
    wait_storage(container, "/srv/other")
    container.wait_http()

    container.exec("systemctl", "stop", "replay-control")
    container.exec("mv", "/srv/other/roms", "/srv/other/roms.offline")
    container.exec("systemctl", "start", "replay-control")
    eventually(
        lambda: "storage_error" in container.http("/storage-setup/status")[1],
        "unavailable storage recovery page",
    )
    assert "This folder must contain a readable roms/ directory" in container.http("/storage-setup/status")[1]
    assert container.read("/var/lib/replay-control/standalone-storage-path") == "/srv/other"
    container.exec("mv", "/srv/other/roms.offline", "/srv/other/roms")
    container.exec("systemctl", "restart", "replay-control")
    container.wait_http()

    container.exec("touch", "/run/replay-control-fail-start-once")
    select_storage(container, "/srv/fail")
    eventually(lambda: container.exists("/var/lib/replay-control/storage-switch-status")
               and container.read("/var/lib/replay-control/storage-switch-status").partition("\n")[0] == "error",
               "failed storage switch")
    assert container.read("/var/lib/replay-control/standalone-storage-path") == "/srv/other"
    container.wait_http()

    container.exec("cp", "/var/lib/replay-control/standalone-storage-path", "/var/lib/replay-control/storage-switch-previous")
    container.write("/var/lib/replay-control/standalone-storage-path", "/srv/fail\n")
    container.write("/var/lib/replay-control/storage-switch-pending", "/srv/fail\n")
    container.exec("systemctl", "restart", "replay-control")
    eventually(lambda: container.read("/var/lib/replay-control/standalone-storage-path") == "/srv/other"
               and not container.exists("/var/lib/replay-control/storage-switch-pending"),
               "interrupted storage switch rollback")

    configure(container, storage="/srv", mock_updates=True)
    container.exec("systemctl", "restart", "replay-control")
    _wait_active_root(container, "/srv/other", "saved path precedence")
    container.exec("rm", "/var/lib/replay-control/standalone-storage-path")
    container.exec("systemctl", "restart", "replay-control")
    _wait_active_root(container, "/srv", "environment path fallback")
    container.wait_http()


def _wait_active_root(container, expected, description):
    def matches():
        status, body = container.http("/storage-setup/active-root")
        assert status == 200 and body.strip() == expected, (status, body)
        return True

    eventually(matches, description)


def _check_fedora_metadata_and_route(container):
    eventually(
        lambda: "Downloading LaunchBox metadata from" in container.read("/var/log/replay-control-test.log"),
        "Fedora automatic metadata download",
    )
    container.copy("tests/package/extract-server-function-route.py", "/root/extract-server-function-route.py")
    wasm_hash = container.read("/usr/share/replay-control/site/hash.txt").split("wasm: ")[1].splitlines()[0]
    route = container.output(
        "python3", "/root/extract-server-function-route.py",
        f"/usr/share/replay-control/site/pkg/replay_control_app.{wasm_hash}.wasm",
        "/sfn/get_system_favorites",
    )
    status, body = container.http(route, method="POST", data="system=nintendo_nes", headers=("content-type: application/x-www-form-urlencoded",))
    assert status == 200, body
    configure(container, storage="/media/sd", https_port=9443, mock_updates=True)
    container.exec("systemctl", "restart", "replay-control")
    container.wait_http(port=9443, timeout=120)
    configure(container, storage="/media/sd", mock_updates=True)
    container.exec("systemctl", "restart", "replay-control")
    container.wait_http()


def _downgrade_rpm(container, package):
    container.exec("mkdir", "-p", "/root/old-payload", "/root/rpmbuild/RPMS/x86_64")
    container.copy("packaging/replay-control-rpm.spec", "/root/replay-control-rpm.spec")
    container.exec("bash", "-lc", f"cd /root/old-payload && rpm2cpio /root/{package.path.name} | cpio -idm --quiet")
    container.exec(
        "rpmbuild", "-bb", "/root/replay-control-rpm.spec",
        "--define", "_topdir /root/rpmbuild",
        "--define", "payload /root/old-payload",
        "--define", "version 0.0.1",
        "--define", "_build_id_links none",
    )
    rpm = container.output("find", "/root/rpmbuild/RPMS/x86_64", "-name", "replay-control-0.0.1-1.*.x86_64.rpm").splitlines()[0]
    container.exec("dnf", "downgrade", "-y", rpm)
    assert package_version(container, "rpm") == "0.0.1"


def _check_bootstrap_update_result(container, package):
    container.write("/var/lib/replay-control/standalone-storage-path", "/srv/other\n")
    container.exec("mv", "/srv/other/roms", "/srv/other/roms-offline")
    message = "failed:Package installed but storage is unavailable."
    container.write("/var/lib/replay-control/update-result", message + "\n")
    container.exec("systemctl", "restart", "replay-control")
    container.wait_http("/storage-setup")
    assert container.wait_http("/api/update-result").strip() == message
    assert json.loads(container.wait_http())["version"] == package.app_version
    status, _ = container.http("/settings")
    assert status == 307
    container.exec("mv", "/srv/other/roms-offline", "/srv/other/roms")
    container.exec("systemctl", "restart", "replay-control")
