"""Debian arm64 installation and migration with the official RePlayOS frontend."""

import hashlib
import json
import os
import re
import pytest

from scenarios import (
    assert_desktop_launcher,
    assert_package_files,
    configure,
    install_package,
    make_deb_revision,
    run_update_now,
)
from support import Container, command, eventually, package_version


REPLAYOS_RELEASE = "https://github.com/rtomasa/replayos-ota-upd/releases/download/ota"
V1_RELEASE = "https://github.com/lapastillaroja/replay-control/releases/download/v1.0.0"
V1_ASSETS = (
    "install.sh",
    "replay-control-app-aarch64-linux.tar.gz",
    "replay-catalog.tar.gz",
    "replay-site.tar.gz",
)


def test_debian_beta_version_order(package):
    if package.arch != "arm64":
        pytest.skip("Debian arm64 release check")
    for older, newer in (
        ("1.2.9", "1.3.0~beta"),
        ("1.3.0~beta", "1.3.0~beta-1"),
        ("1.3.0~beta", "1.3.0~beta.1"),
    ):
        command("dpkg", "--compare-versions", older, "lt", newer)


def _download(url, destination):
    command("curl", "-fsSL", "--retry", "3", "-o", destination, url)


def _check_sha256(path, expected):
    assert re.fullmatch(r"[0-9a-fA-F]{64}", expected)
    assert hashlib.sha256(path.read_bytes()).hexdigest().lower() == expected.lower(), path


@pytest.fixture(scope="session")
def v1_assets(package, tmp_path_factory):
    if package.arch != "arm64":
        pytest.skip("v1.0 release fixture is arm64 only")
    folder = tmp_path_factory.mktemp("replay-control-v1")
    manifest = folder / "checksums.sha256"
    _download(f"{V1_RELEASE}/checksums.sha256", manifest)
    checksums = {
        fields[1].lstrip("*"): fields[0]
        for line in manifest.read_text().splitlines()
        if len(fields := line.split()) == 2
    }
    for name in V1_ASSETS:
        path = folder / name
        _download(f"{V1_RELEASE}/{name}", path)
        _check_sha256(path, checksums[name])
    return folder


@pytest.fixture(scope="session")
def replayos_image(package, engine, base_image, tmp_path_factory):
    if package.arch != "arm64":
        pytest.skip("official RePlayOS frontend fixture is arm64 only")
    image = os.environ.get("PACKAGE_TEST_REPLAYOS_IMAGE")
    if image:
        assert command(engine, "image", "exists", image, check=False).returncode == 0, image
        return image
    image = "replay-control-package-test:replayos-arm64"
    folder = tmp_path_factory.mktemp("replayos-debs")
    index = folder / "Packages"
    _download(f"{REPLAYOS_RELEASE}/Packages", index)
    packages = {}
    for paragraph in index.read_text().split("\n\n"):
        fields = dict(line.split(": ", 1) for line in paragraph.splitlines() if ": " in line)
        if fields.get("Package") in ("replay-libswitchres", "replay-frontend"):
            name = fields["Package"]
            filename = fields["Filename"].removeprefix("./")
            assert re.fullmatch(rf"{name}_[0-9A-Za-z.+~-]+_arm64\.deb", filename)
            destination = folder / filename
            _download(f"{REPLAYOS_RELEASE}/{filename}", destination)
            _check_sha256(destination, fields["SHA256"])
            packages[name] = destination
    assert set(packages) == {"replay-libswitchres", "replay-frontend"}

    staging = Container(engine, base_image, "arm64")
    try:
        for path in packages.values():
            staging.copy(path, f"/root/{path.name}")
        staging.exec("apt-get", "update", "-qq")
        staging.exec(
            "apt-get", "install", "-y", "-qq",
            *(f"/root/{path.name}" for path in packages.values()),
            env={"DEBIAN_FRONTEND": "noninteractive"},
        )
        staging.exec("dpkg-query", "-W", "replay-frontend", "replay-libswitchres")
        command(engine, "commit", staging.name, image)
    finally:
        staging.close()
    return image


def _device_storage(container, *, mock_updates=False):
    container.exec("mkdir", "-p", "/media/sd/config", "/media/sd/roms")
    container.write("/media/sd/config/replay.cfg", 'system_storage = "sd"\n')
    configure(container, storage="/media/sd", mock_updates=mock_updates)


def test_arm64_standalone_install(package, container_factory):
    if package.arch != "arm64":
        pytest.skip("Debian arm64 only")
    container = container_factory()
    container.copy("install.sh", "/root/install-new.sh")
    container.exec("mkdir", "-p", "/media/sd/roms")
    container.exec("apt-get", "update", "-qq")
    container.exec("bash", "/root/install-new.sh", "--package", f"/root/{package.path.name}")
    page = container.wait_http("/storage-setup")
    assert "Choose your storage location" in page
    for language, translation in (("es", "Elige la ubicación de almacenamiento"), ("ja", "ストレージの場所を選択")):
        status, body = container.http("/storage-setup/messages", headers=(f"Accept-Language: {language}",))
        assert status == 200 and translation in body
    configure(container, storage="/media/sd", mock_updates=True)
    container.exec("systemctl", "start", "replay-control")
    assert json.loads(container.wait_http())["version"] == package.app_version
    configure(container, storage="/media/sd", https_port=9443, mock_updates=True)
    container.exec("systemctl", "restart", "replay-control")
    container.wait_http(port=9443)
    assert_package_files(container, package)
    configure(container, storage="/media/sd", mock_updates=True)
    container.exec("systemctl", "restart", "replay-control")
    container.wait_http()
    run_update_now(container, package)


def test_replayos_clean_upgrade_and_remove(package, container_factory, replayos_image):
    container = container_factory(image=replayos_image, replayos=True)
    container.copy("install.sh", "/root/install-new.sh")
    _device_storage(container)
    install_package(container, package)
    assert_package_files(container, package)
    assert_desktop_launcher(container)
    container.exec("dpkg-query", "-W", "replay-frontend", "replay-control")
    assert json.loads(container.wait_http())["version"] == package.app_version
    container.write("/etc/replay-control/package-test-marker", "user data survives upgrades\n")
    replay_version = container.output("dpkg-query", "-W", "-f=${Version}", "replay-frontend")
    for revision in (1, 2):
        version = f"{package.version}+test{revision}"
        path = f"/root/replay-control-upgrade{revision}.deb"
        make_deb_revision(container, f"/root/{package.path.name}", version, path)
        container.exec("apt-get", "install", "-y", path, env={"DEBIAN_FRONTEND": "noninteractive"})
        assert package_version(container, "deb") == version
        assert container.output("dpkg-query", "-W", "-f=${Version}", "replay-frontend") == replay_version
        assert container.read("/etc/replay-control/package-test-marker") == "user data survives upgrades"
        container.wait_http()

    container.exec("mkdir", "-p", "/etc/systemd/system/replay-control.service.d")
    container.exec("touch", "/etc/systemd/system/replay-control.service.d/90-replay-control-dev.conf")
    container.exec("bash", "/root/install-new.sh", "--uninstall")
    removed = container.exec("dpkg-query", "-W", "-f=${Status}", "replay-control", check=False)
    assert "install ok installed" not in removed.stdout
    assert container.exists("/etc/default/replay-control")
    assert container.read("/etc/replay-control/package-test-marker") == "user data survives upgrades"
    assert not container.exists("/etc/systemd/system/replay-control.service.d/90-replay-control-dev.conf")
    container.exec("bash", "/root/install-new.sh", "--purge", "--yes")
    for path in ("/etc/default/replay-control", "/etc/replay-control", "/var/lib/replay-control"):
        assert not container.exists(path), path


def test_real_v1_binary_upgrades_through_helper(package, container_factory, replayos_image, v1_assets):
    container = container_factory(image=replayos_image, replayos=True)
    _device_storage(container)
    for asset in V1_ASSETS[1:]:
        container.copy(v1_assets / asset, f"/root/{asset}")
    stage = "/root/package-v1-stage"
    container.exec("dpkg-deb", "--raw-extract", f"/root/{package.path.name}", stage)
    container.exec("tar", "-xzf", "/root/replay-control-app-aarch64-linux.tar.gz", "-C", f"{stage}/usr/bin")
    container.exec("rm", "-rf", f"{stage}/usr/share/replay-control/site")
    container.exec("tar", "-xzf", "/root/replay-site.tar.gz", "-C", f"{stage}/usr/share/replay-control")
    container.exec("tar", "-xzf", "/root/replay-catalog.tar.gz", "-C", f"{stage}/usr/share/replay-control")
    control = container.read(f"{stage}/DEBIAN/control")
    control = re.sub(r"^Version: .+$", "Version: 1.0.0", control, count=1, flags=re.MULTILINE)
    container.write(f"{stage}/DEBIAN/control", control + "\n")
    container.exec("dpkg-deb", "--build", "--root-owner-group", stage, "/root/replay-control-v1.deb")
    container.exec("apt-get", "install", "-y", "/root/replay-control-v1.deb", env={"DEBIAN_FRONTEND": "noninteractive"})
    eventually(lambda: '"version":"1.0.0"' in container.http("/api/version")[1], "v1.0 application startup")
    container.write("/etc/replay-control/package-test-marker", "settings survive real upgrade\n")
    container.exec("cp", f"/root/{package.path.name}", "/tmp/current.deb")
    container.exec("/usr/lib/replay-control/package-update-deb", "/tmp/current.deb", package.app_version)
    assert container.read("/var/lib/replay-control/update-result") == f"ok:{package.app_version}"
    assert container.read("/etc/replay-control/package-test-marker") == "settings survive real upgrade"
    assert json.loads(container.wait_http())["version"] == package.app_version


def test_archive_migration_and_failed_updates(package, container_factory, replayos_image, v1_assets):
    container = container_factory(image=replayos_image, replayos=True)
    container.copy(v1_assets, "/root/v1.0.0")
    container.copy("tests/package/prepare-v1.sh", "/root/prepare-v1.sh")
    container.copy("install.sh", "/root/install-new.sh")
    container.exec("bash", "/root/prepare-v1.sh")
    assert "Would extract binary" in container.output("bash", "/root/install-new.sh", "--local", "/root/v1.0.0", "--dry-run")
    assert "Would install Debian package" in container.output("bash", "/root/install-new.sh", "--package", f"/root/{package.path.name}", "--dry-run")
    direct = container.exec("apt-get", "install", "-y", f"/root/{package.path.name}", check=False, env={"DEBIAN_FRONTEND": "noninteractive"})
    assert direct.returncode != 0, "direct APT install bypassed archive migration"
    assert container.exists("/etc/systemd/system/replay-control.service")

    container.exec("touch", "/run/replay-control-fail-enable-once")
    failed = container.exec("bash", "/root/install-new.sh", "--package", f"/root/{package.path.name}", check=False)
    assert failed.returncode != 0, "migration unexpectedly succeeded after activation failure"
    assert container.exists("/etc/systemd/system/replay-control.service")
    assert container.exists("/usr/local/bin/replay-control-app")
    container.exec("systemctl", "is-enabled", "replay-control.service")
    removed = container.exec("dpkg-query", "-W", "-f=${Status}", "replay-control", check=False)
    assert "install ok installed" not in removed.stdout
    eventually(lambda: '"version":"1.0.0"' in container.http("/api/version", port=8080, secure=False)[1], "restored archive service")

    container.write("/media/sd/.replay-control-data/migration-retry-marker", "copied on retry\n")
    settings = container.read("/media/sd/.replay-control/settings.cfg")
    assert 'font_size = "large"' in settings
    container.write("/media/sd/.replay-control/settings.cfg", settings.replace('font_size = "large"', 'font_size = "huge"') + "\n")
    # Keep the marker process alive for the package preinst check.
    container.exec("bash", "-lc", f'echo $$ > /run/replay-control-package-migration; '
                   f'DEBIAN_FRONTEND=noninteractive apt-get install -y /root/{package.path.name}; '
                   'result=$?; rm -f /run/replay-control-package-migration; exit "$result"')
    assert container.exists("/etc/systemd/system/replay-control.service")
    assert package_version(container, "deb") == package.version
    container.exec("bash", "/root/install-new.sh", "--package", f"/root/{package.path.name}")

    assert package_version(container, "deb") == package.version
    assert container.read("/etc/replay-control/package-test-marker") == "settings preserved"
    assert container.read("/var/lib/replay-control/package-test-marker") == "library preserved"
    assert 'font_size = "huge"' in container.read("/media/sd/.replay-control/settings.cfg")
    assert 'font_size = "huge"' in container.read("/etc/replay-control/settings.cfg")
    assert container.read("/var/lib/replay-control/migration-retry-marker") == "copied on retry"
    assert container.exec("test", "-s", "/media/sd/.replay-control/user_data.db", check=False).returncode == 0
    libraries = container.output("find", "/var/lib/replay-control/storages", "-name", "library.db", "-size", "+0c").splitlines()
    assert libraries
    eventually(lambda: any("/var/lib/replay-control/storages/" in line and "/library.db" in line
                           for line in container.output("ls", "-l", f'/proc/{container.read("/run/replay-control-test.pid")}/fd').splitlines()),
               "service opens migrated library")
    assert not container.exists("/usr/local/bin/replay-control-app")
    assert not container.exists("/etc/systemd/system/replay-control.service")
    assert json.loads(container.wait_http())["version"] == package.app_version
    assert container.exec("bash", "/root/install-new.sh", "--local", "/root/v1.0.0", check=False).returncode != 0

    _check_update_failures(container, package)


def _check_update_failures(container, package):
    apt = "/usr/local/bin/apt-get"
    failures = (
        ('#!/bin/sh\necho "Could not get lock /var/lib/dpkg/lock-frontend" >&2\nexit 100\n', "retry.deb", "failed:APT is busy"),
        ('#!/bin/sh\necho "No space left on device" >&2\nexit 100\n', "no-space.deb", "failed:Insufficient space"),
    )
    for script, filename, result in failures:
        container.write(apt, script)
        container.exec("chmod", "+x", apt)
        container.exec("cp", f"/root/{package.path.name}", f"/tmp/{filename}")
        outcome = container.exec("/usr/lib/replay-control/package-update-deb", f"/tmp/{filename}", package.version, check=False)
        assert outcome.returncode != 0
        assert result in container.read("/var/lib/replay-control/update-result")
    container.write(apt, "#!/bin/sh\nsleep 3\n")
    container.exec("cp", f"/root/{package.path.name}", "/tmp/interrupted.deb")
    container.exec("timeout", "-s", "TERM", "1", "/usr/lib/replay-control/package-update-deb", "/tmp/interrupted.deb", package.version, check=False)
    assert "failed:Package update was interrupted" in container.read("/var/lib/replay-control/update-result")
