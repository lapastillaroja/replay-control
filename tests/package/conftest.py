"""Inputs and containers shared by package scenarios."""

import os
from pathlib import Path

import pytest

from support import Container, Package, build_image, command


def pytest_addoption(parser):
    group = parser.getgroup("package")
    group.addoption("--package", help="Built .deb or .rpm to test")
    group.addoption("--format", choices=("deb", "rpm"), help="Package format")
    group.addoption("--arch", choices=("amd64", "arm64", "x86_64"), help="Package architecture")


@pytest.fixture(scope="session")
def package(request):
    source = request.config.getoption("--package")
    kind = request.config.getoption("--format")
    arch = request.config.getoption("--arch")
    if not (source and kind and arch):
        pytest.skip("provide --package, --format, and --arch")
    path = Path(source).resolve()
    assert path.is_file() and path.stat().st_size > 0, path
    if kind == "deb":
        assert arch in ("arm64", "amd64")
        assert command("dpkg-deb", "--field", path, "Package").stdout.strip() == "replay-control"
        assert command("dpkg-deb", "--field", path, "Architecture").stdout.strip() == arch
        version = command("dpkg-deb", "--field", path, "Version").stdout.strip()
    else:
        assert arch == "x86_64"
        assert path.name.endswith(".x86_64.rpm")
        version = command("rpm", "-qp", "--qf", "%{VERSION}", path).stdout.strip()
    return Package(path, kind, arch, version)


@pytest.fixture(scope="session")
def engine():
    return os.environ.get("CONTAINER_ENGINE", "podman")


@pytest.fixture(scope="session")
def browser_enabled():
    return os.environ.get("RUN_BROWSER_E2E") == "1"


@pytest.fixture(scope="session")
def base_image(package, engine):
    if package.kind == "rpm":
        image = "replay-control-package-test:fedora42"
        build_image(engine, image, "Fedora.Dockerfile", "amd64")
    else:
        base = os.environ.get("PACKAGE_TEST_BASE", "debian:trixie")
        image = f"replay-control-package-test:{base.replace(':', '-').replace('/', '-')}-{package.arch}"
        build_image(engine, image, "Dockerfile", package.arch, base=base)
    return image


@pytest.fixture
def container_factory(package, engine, base_image, browser_enabled, request):
    containers = []

    def create(*, image=None, replayos=False, browser=None):
        container = Container(
            engine,
            image or base_image,
            "amd64" if package.arch == "x86_64" else package.arch,
            browser=browser_enabled if browser is None else browser,
            replayos=replayos,
        )
        containers.append(container)
        container.install_shims(arm64=package.arch == "arm64")
        container.copy(package.path, f"/root/{package.path.name}")
        container.write("/root/package-version", package.version + "\n")
        return container

    yield create
    if request.node.rep_call.failed and os.environ.get("KEEP_FAILED_PACKAGE_CONTAINER") == "1":
        print("Keeping failed package containers:", ", ".join(c.name for c in containers))
    else:
        for container in reversed(containers):
            container.close()


@pytest.hookimpl(hookwrapper=True)
def pytest_runtest_makereport(item, call):
    outcome = yield
    setattr(item, f"rep_{call.when}", outcome.get_result())
